// Background pipeline: runs ffprobe + ffmpeg for each video in the queue.
//
// Flow:
//   1. fs_read_videos arms the pipeline with a video_map (path → VideoMeta)
//   2. pipeline_process sends a prioritized list of visible file paths
//   3. reprioritize reorders the queue (visible first) and wakes workers
//   4. Each worker calls probe_video() then generate_thumbnail()
//   5. Results are emitted to the renderer via tauri::Emitter

use crate::state::{thumb_path_for_file, AppStateHandle, DimEntry};
use serde::Serialize;
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::{Mutex, Notify, Semaphore};

// On Windows GUI builds, child console processes (ffprobe/ffmpeg) get a
// temporary console window per invocation without this flag. CREATE_NO_WINDOW
// suppresses that visible flash in release builds.
// In dev the parent already owns a console, so children inherit it silently.
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const CONCURRENCY: usize = 4;

// ── Events emitted to the renderer ───────────────────────────────────────────

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbnailReadyPayload {
    pub id: String,
    pub thumbnail_url: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DimsReadyPayload {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub duration: Option<f64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoNoStreamPayload {
    pub id: String,
}

#[derive(Debug, Clone)]
pub struct VideoDims {
    pub width: u32,
    pub height: u32,
    pub duration: Option<f64>,
}

// ── Pipeline state ────────────────────────────────────────────────────────────

struct PipelineInner {
    /// Incremented on every cancel / folder load. Workers bail out when this
    /// changes between steps, avoiding stale work after navigation.
    token: u64,
    queue: VecDeque<String>,
    in_flight: HashSet<String>,
    pub video_map: std::collections::HashMap<String, VideoMeta>,
}

#[derive(Clone)]
pub struct VideoMeta {
    pub id: String,
    pub mtime: f64,
    pub thumbnail_url: Option<String>,
}

impl PipelineInner {
    fn new() -> Self {
        Self {
            token: 0,
            queue: VecDeque::new(),
            in_flight: HashSet::new(),
            video_map: std::collections::HashMap::new(),
        }
    }
}

#[derive(Clone)]
pub struct PipelineHandle(Arc<PipelineShared>);

struct PipelineShared {
    inner: Mutex<PipelineInner>,
    notify: Notify,
    sem: Semaphore,
}

impl PipelineHandle {
    pub fn new() -> Self {
        Self(Arc::new(PipelineShared {
            inner: Mutex::new(PipelineInner::new()),
            notify: Notify::new(),
            sem: Semaphore::new(CONCURRENCY),
        }))
    }

    pub async fn cancel(&self) {
        let mut g = self.0.inner.lock().await;
        g.token += 1;
        g.queue.clear();
        g.in_flight.clear();
    }

    pub async fn set_video_map(&self, map: std::collections::HashMap<String, VideoMeta>) {
        let mut g = self.0.inner.lock().await;
        g.token += 1;
        g.queue.clear();
        g.in_flight.clear();
        g.video_map = map;
    }

    /// Inserts a single entry without resetting the existing map or cancelling
    /// in-flight work. Used by fs_read_video_entries for watcher-detected files.
    pub async fn insert_video_meta(&self, file_path: String, meta: VideoMeta) {
        let mut g = self.0.inner.lock().await;
        g.video_map.insert(file_path, meta);
    }

    /// Reorders the queue so that the given paths (visible first) are processed
    /// next. Paths that scrolled off are dropped; new paths are prepended.
    pub async fn reprioritize(&self, file_paths: Vec<String>) {
        {
            let mut g = self.0.inner.lock().await;
            let incoming: HashSet<_> = file_paths.iter().cloned().collect();

            g.queue.retain(|p| incoming.contains(p));

            let already: HashSet<_> = g.queue.iter().chain(g.in_flight.iter()).cloned().collect();
            let to_add: Vec<_> = file_paths
                .into_iter()
                .filter(|p| !already.contains(p))
                .collect();

            for p in to_add.into_iter().rev() {
                g.queue.push_front(p);
            }
        }
        self.0.notify.notify_waiters();
    }

    /// Spawns CONCURRENCY worker tasks. Call once after app setup.
    pub fn spawn_workers<F>(&self, state: AppStateHandle, emit: F)
    where
        F: Fn(WorkerEvent) + Send + Sync + 'static,
    {
        let emit = Arc::new(emit);
        for _ in 0..CONCURRENCY {
            let handle = self.clone();
            let state = state.clone();
            let emit = emit.clone();
            tokio::spawn(async move {
                handle.worker_loop(state, emit).await;
            });
        }
    }

    async fn worker_loop<F>(&self, state: AppStateHandle, emit: Arc<F>)
    where
        F: Fn(WorkerEvent) + Send + Sync + 'static,
    {
        loop {
            let file_path = loop {
                {
                    let mut g = self.0.inner.lock().await;
                    if let Some(fp) = g.queue.pop_front() {
                        g.in_flight.insert(fp.clone());
                        break fp;
                    }
                }
                self.0.notify.notified().await;
            };

            let _permit = self.0.sem.acquire().await.unwrap();

            let my_token = {
                let g = self.0.inner.lock().await;
                g.token
            };

            self.process_one(&file_path, my_token, &state, &emit).await;

            {
                let mut g = self.0.inner.lock().await;
                g.in_flight.remove(&file_path);
            }
        }
    }

    async fn process_one<F>(
        &self,
        file_path: &str,
        my_token: u64,
        state: &AppStateHandle,
        emit: &Arc<F>,
    ) where
        F: Fn(WorkerEvent) + Send + Sync + 'static,
    {
        // Bail out early if the folder changed since this job was queued
        macro_rules! alive {
            () => {{
                let g = self.0.inner.lock().await;
                if g.token != my_token {
                    return;
                }
            }};
        }

        let meta = {
            let g = self.0.inner.lock().await;
            g.video_map.get(file_path).cloned()
        };
        let Some(meta) = meta else { return };

        alive!();

        let cached = state.read_dim_cache(|c| c.get(file_path).cloned()).await;

        let dims = if let Some(ref entry) = cached {
            if (entry.mtime - meta.mtime).abs() < 1.0 {
                if entry.no_stream {
                    emit(WorkerEvent::NoStream {
                        id: meta.id.clone(),
                    });
                    return;
                }
                Some(VideoDims {
                    width: entry.width.unwrap_or(0),
                    height: entry.height.unwrap_or(0),
                    duration: entry.duration,
                })
            } else {
                None
            }
        } else {
            None
        };

        let dims = if let Some(d) = dims {
            d
        } else {
            alive!();
            match probe_video(file_path).await {
                None => {
                    state
                        .upsert_dim_entry(
                            file_path.to_owned(),
                            DimEntry {
                                width: None,
                                height: None,
                                duration: None,
                                mtime: meta.mtime,
                                no_stream: true,
                            },
                        )
                        .await;
                    alive!();
                    emit(WorkerEvent::NoStream {
                        id: meta.id.clone(),
                    });
                    return;
                }
                Some(d) => {
                    state
                        .upsert_dim_entry(
                            file_path.to_owned(),
                            DimEntry {
                                width: Some(d.width),
                                height: Some(d.height),
                                duration: d.duration,
                                mtime: meta.mtime,
                                no_stream: false,
                            },
                        )
                        .await;
                    alive!();
                    emit(WorkerEvent::Dims {
                        id: meta.id.clone(),
                        width: d.width,
                        height: d.height,
                        duration: d.duration,
                    });
                    d
                }
            }
        };

        let thumb_path = thumb_path_for_file(file_path);
        if !thumb_path.exists() {
            alive!();
            if let Some(out) = generate_thumbnail(file_path, dims.duration, &thumb_path).await {
                alive!();
                let url = thumb_url_for_path(&out);
                emit(WorkerEvent::Thumbnail {
                    id: meta.id.clone(),
                    thumbnail_url: url,
                });
            }
        } else if meta.thumbnail_url.is_none() {
            let url = thumb_url_for_path(&thumb_path);
            emit(WorkerEvent::Thumbnail {
                id: meta.id.clone(),
                thumbnail_url: url,
            });
        }
    }
}

// ── Worker events ─────────────────────────────────────────────────────────────

pub enum WorkerEvent {
    Dims {
        id: String,
        width: u32,
        height: u32,
        duration: Option<f64>,
    },
    Thumbnail {
        id: String,
        thumbnail_url: String,
    },
    NoStream {
        id: String,
    },
}

// ── Binary resolution ─────────────────────────────────────────────────────────

fn find_binary(name: &str) -> String {
    // Windows GUI apps don't inherit the user PATH, so probe common install
    // locations before falling back to PATH resolution.
    #[cfg(target_os = "windows")]
    {
        let exe = format!("{}.exe", name);
        let candidates = [
            format!(r"C:\ffmpeg\bin\{}", exe),
            format!(r"C:\ffmpeg\{}", exe),
            format!(r"C:\Program Files\ffmpeg\bin\{}", exe),
            format!(r"C:\Program Files (x86)\ffmpeg\bin\{}", exe),
            format!(r"C:\ProgramData\chocolatey\bin\{}", exe),
            format!(
                r"{}\scoop\apps\ffmpeg\current\bin\{}",
                std::env::var("USERPROFILE").unwrap_or_default(),
                exe
            ),
        ];
        if let Some(found) = candidates
            .iter()
            .find(|p| std::path::Path::new(p.as_str()).exists())
        {
            return found.clone();
        }
    }
    name.to_string()
}

// ── ffprobe ───────────────────────────────────────────────────────────────────

pub async fn probe_video(file_path: &str) -> Option<VideoDims> {
    let mut cmd = Command::new(find_binary("ffprobe"));
    cmd.args([
        "-v",
        "quiet",
        "-print_format",
        "json",
        "-show_streams",
        "-select_streams",
        "v:0",
        file_path,
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .kill_on_drop(true);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = cmd.output().await.ok()?;

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let stream = json.get("streams")?.get(0)?;

    let raw_w = stream
        .get("coded_width")
        .or_else(|| stream.get("width"))
        .and_then(|v| v.as_u64())? as u32;
    let raw_h = stream
        .get("coded_height")
        .or_else(|| stream.get("height"))
        .and_then(|v| v.as_u64())? as u32;

    // Swap dimensions for 90° / 270° rotated videos
    let rotation: i32 = stream
        .get("tags")
        .and_then(|t| t.get("rotate"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .or_else(|| {
            stream
                .get("side_data_list")
                .and_then(|l| l.get(0))
                .and_then(|d| d.get("rotation"))
                .and_then(|v| v.as_i64())
                .map(|v| v as i32)
        })
        .unwrap_or(0)
        .abs();

    let (width, height) = if rotation == 90 || rotation == 270 {
        (raw_h, raw_w)
    } else {
        (raw_w, raw_h)
    };

    let duration = stream
        .get("duration")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<f64>().ok());

    Some(VideoDims {
        width,
        height,
        duration,
    })
}

// ── ffmpeg thumbnail ──────────────────────────────────────────────────────────

pub async fn generate_thumbnail(
    file_path: &str,
    _duration: Option<f64>,
    out_path: &PathBuf,
) -> Option<PathBuf> {
    if let Some(parent) = out_path.parent() {
        tokio::fs::create_dir_all(parent).await.ok()?;
    }

    let mut cmd = Command::new(find_binary("ffmpeg"));
    cmd.args([
        "-ss",
        "1",
        "-i",
        file_path,
        "-frames:v",
        "1",
        "-vf",
        "scale=480:-2",
        "-q:v",
        "2",
        "-y",
        out_path.to_str()?,
    ])
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .kill_on_drop(true);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let status = cmd.status().await.ok()?;

    if status.success() {
        Some(out_path.clone())
    } else {
        None
    }
}

// ── URL helpers ───────────────────────────────────────────────────────────────
//
// Videos use the axum HTTP server (real TCP) to support range requests and
// seeking. The custom localvideo:// protocol goes through wry/WebView and
// cannot support streaming (wry#1404).
//
// Thumbnails are small JPEGs loaded in one shot, so localvideo:// is fine.
// On Windows, WebView2 rewrites localvideo:// to http://localvideo.localhost/,
// so we emit that form directly to keep the URL consistent.

pub fn video_url_for_path(path: &PathBuf, port: u16) -> String {
    let encoded = percent_encode(path.to_str().unwrap_or(""));
    format!("http://127.0.0.1:{}/{}", port, encoded)
}

pub fn thumb_url_for_path(path: &PathBuf) -> String {
    let encoded = percent_encode(path.to_str().unwrap_or(""));
    #[cfg(target_os = "windows")]
    return format!("http://localvideo.localhost/{}", encoded);
    #[cfg(not(target_os = "windows"))]
    return format!("localvideo://localhost/{}", encoded);
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                out.push(byte as char);
            }
            b => {
                use std::fmt::Write;
                write!(out, "%{:02X}", b).unwrap();
            }
        }
    }
    out
}
