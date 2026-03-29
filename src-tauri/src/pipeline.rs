// pipeline.rs
// Replaces Electron's OnDemandProcessor + ffprobe/ffmpeg calls.
// Uses tokio::sync channels + a semaphore for bounded concurrency (4 workers).
//
// Flow:
//   1. commands::pipeline_process sends a list of filePaths (visible first)
//   2. PipelineHandle::reprioritize reorders the queue and wakes workers
//   3. Each worker calls probe_video() then generate_thumbnail()
//   4. Results are sent to the renderer via tauri::Emitter::emit_to()

use crate::state::{thumb_path_for_file, AppStateHandle, DimEntry};
use serde::Serialize;
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::{Mutex, Notify, Semaphore};

// FIX: En Windows build (subsistema GUI), los procesos hijo como ffprobe/ffmpeg
// son binarios de consola. Sin este flag, Windows les abre una ventana de
// consola temporal por cada invocación — el "flash" visible en release.
// CREATE_NO_WINDOW (0x08000000) le dice al kernel que cree el proceso sin
// asignarle ninguna ventana de consola.
// En dev esto no ocurre porque el proceso padre ya tiene una consola (la
// terminal de cargo/tauri-cli) que los hijos heredan.
// Ref: https://learn.microsoft.com/windows/win32/procthread/process-creation-flags
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const CONCURRENCY: usize = 4;

// ── Events emitted to renderer ────────────────────────────────────────────────

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

// ── Video metadata returned by ffprobe ───────────────────────────────────────

#[derive(Debug, Clone)]
pub struct VideoDims {
    pub width: u32,
    pub height: u32,
    pub duration: Option<f64>,
}

// ── Pipeline state shared between command handlers and workers ────────────────

struct PipelineInner {
    /// Monotonic token; incremented on every pipeline:cancel / folder load.
    /// Workers check this before every I/O to bail out early.
    token: u64,
    /// Ordered queue: visible first, lookahead after.
    queue: VecDeque<String>,
    /// Paths currently being processed by a worker.
    in_flight: HashSet<String>,
    /// Map from filePath → video id, populated on fs:read_videos.
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

    /// Cancel all pending work and reset the queue.
    pub async fn cancel(&self) {
        let mut g = self.0.inner.lock().await;
        g.token += 1;
        g.queue.clear();
        g.in_flight.clear();
        // video_map is NOT cleared here; it is replaced in set_video_map
    }

    /// Called by fs:read_videos after a successful folder load.
    pub async fn set_video_map(&self, map: std::collections::HashMap<String, VideoMeta>) {
        let mut g = self.0.inner.lock().await;
        g.token += 1;
        g.queue.clear();
        g.in_flight.clear();
        g.video_map = map;
    }

    /// Called by fs:read_video_entries to register new videos detected by the
    /// watcher without resetting the existing map or cancelling in-flight work.
    /// Without this, pipeline_process called from applyDiff would look up the
    /// new file path in video_map, find nothing, and silently skip it —
    /// leaving the card without a thumbnail forever.
    pub async fn insert_video_meta(&self, file_path: String, meta: VideoMeta) {
        let mut g = self.0.inner.lock().await;
        g.video_map.insert(file_path, meta);
    }

    /// Reprioritize queue: visible first, lookahead after, drop off-screen.
    /// filePaths must be ordered: visible items first.
    pub async fn reprioritize(&self, file_paths: Vec<String>) {
        {
            let mut g = self.0.inner.lock().await;
            let incoming: HashSet<_> = file_paths.iter().cloned().collect();

            // Drop queued items that scrolled off
            g.queue.retain(|p| incoming.contains(p));

            // Prepend new paths not already queued or in-flight
            let already: HashSet<_> = g.queue.iter().chain(g.in_flight.iter()).cloned().collect();

            let to_add: Vec<_> = file_paths
                .into_iter()
                .filter(|p| !already.contains(p))
                .collect();

            // Prepend to keep visible items at front
            for p in to_add.into_iter().rev() {
                g.queue.push_front(p);
            }
        }
        self.0.notify.notify_waiters();
    }

    /// Spawn background worker loop. Call once after setup.
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
            // Wait until there's work or a notify fires
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

            // Acquire concurrency slot
            let _permit = self.0.sem.acquire().await.unwrap();

            // Snapshot token before starting
            let my_token = {
                let g = self.0.inner.lock().await;
                g.token
            };

            self.process_one(&file_path, my_token, &state, &emit).await;

            // Remove from in-flight
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
        // Check token before every step
        macro_rules! alive {
            () => {{
                let g = self.0.inner.lock().await;
                if g.token != my_token {
                    return;
                }
            }};
        }

        // Get video metadata from map
        let meta = {
            let g = self.0.inner.lock().await;
            g.video_map.get(file_path).cloned()
        };
        let Some(meta) = meta else { return };

        alive!();

        // Check / refresh dims in cache
        let cached = state.read_dim_cache(|c| c.get(file_path).cloned()).await;

        let dims = if let Some(ref entry) = cached {
            if (entry.mtime - meta.mtime).abs() < 1.0 {
                // Cache hit
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

        // Generate thumbnail if not already on disk
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

// ── Events from workers to the command layer ──────────────────────────────────

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

fn find_binary(name: &str) -> String {
    // En Windows, la app GUI no hereda el PATH de usuario.
    // Intentamos rutas comunes antes de hacer fallback al PATH.
    #[cfg(target_os = "windows")]
    {
        let exe = format!("{}.exe", name);
        let candidates = [
            // winget / scoop / manual install típico
            format!(r"C:\ffmpeg\bin\{}", exe),
            format!(r"C:\ffmpeg\{}", exe),
            format!(r"C:\Program Files\ffmpeg\bin\{}", exe),
            format!(r"C:\Program Files (x86)\ffmpeg\bin\{}", exe),
            // chocolatey
            format!(r"C:\ProgramData\chocolatey\bin\{}", exe),
            // scoop (usuario actual)
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
    name.to_string() // fallback: que el OS lo resuelva
}

// ── ffprobe ───────────────────────────────────────────────────────────────────

pub async fn probe_video(file_path: &str) -> Option<VideoDims> {
    // FIX: En Windows build (subsistema GUI), sin CREATE_NO_WINDOW cada llamada
    // a ffprobe abre y cierra una ventana de consola brevemente (el "flash").
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

    // Account for rotation metadata
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

    // FIX: igual que probe_video — CREATE_NO_WINDOW evita el flash de consola
    // en Windows build al generar cada thumbnail con ffmpeg.
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
// Los videos se sirven por el servidor HTTP real (axum, 127.0.0.1:{puerto}).
// Esto evita la limitación de wry/WebView (wry#1404) que impide streaming real
// a través del protocolo custom localvideo://.
//
// Los thumbnails (imágenes JPEG pequeñas) siguen usando localvideo:// — no
// necesitan streaming, se cargan de una vez y el protocolo custom funciona bien.

/// URL HTTP para reproducir un video. Requiere el puerto del VideoServerState.
pub fn video_url_for_path(path: &PathBuf, port: u16) -> String {
    let encoded = percent_encode(path.to_str().unwrap_or(""));
    format!("http://127.0.0.1:{}/{}", port, encoded)
}

/// URL localvideo:// para mostrar un thumbnail. No usa el servidor HTTP.
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
