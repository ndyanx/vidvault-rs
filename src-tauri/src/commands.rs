// commands.rs
// All #[tauri::command] functions.
// These replace every ipcMain.handle / ipcMain.on in the Electron main process.

use crate::pipeline::{
    thumb_url_for_path, video_url_for_path, PipelineHandle, VideoMeta, WorkerEvent,
};
use crate::state::{thumb_path_for_file, AppStateHandle};
use crate::video_server::VideoServerState;
use crate::watcher;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use tauri::{AppHandle, Emitter, Runtime, State};

// ── Video extensions whitelist ─────────────────────────────────────────────────

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mov", "mkv", "avi", "webm", "m4v", "wmv", "flv", "3gp", "ts", "mts",
];

fn is_video_ext(ext: &str) -> bool {
    VIDEO_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
}

// ── store:get ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn store_get(key: String, state: State<'_, AppStateHandle>) -> Result<Value, String> {
    Ok(state.get_key(&key).await)
}

// ── store:set ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn store_set(
    key: String,
    value: Value,
    state: State<'_, AppStateHandle>,
) -> Result<(), String> {
    state.set_key(&key, value).await;
    Ok(())
}

// ── store:getAll ──────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn store_get_all(state: State<'_, AppStateHandle>) -> Result<Value, String> {
    let s = state
        .read_state(|s| serde_json::to_value(s).unwrap_or(Value::Null))
        .await;
    Ok(s)
}

// ── store:getFolderThumb ──────────────────────────────────────────────────────

#[tauri::command]
pub async fn store_get_folder_thumb(
    dir_path: String,
    state: State<'_, AppStateHandle>,
) -> Result<Option<String>, String> {
    let normal_dir = dir_path.replace('\\', "/");
    let result = state
        .read_dim_cache(|cache| {
            for (file_path, entry) in cache {
                if entry.no_stream {
                    continue;
                }
                let normal_file = file_path.replace('\\', "/");
                if !normal_file.starts_with(&normal_dir) {
                    continue;
                }
                let thumb = thumb_path_for_file(file_path);
                if thumb.exists() {
                    return Some(thumb_url_for_path(&thumb));
                }
            }
            None
        })
        .await;
    Ok(result)
}

// ── Shared types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoEntry {
    pub id: String,
    pub file_name: String,
    pub file_path: String,
    pub video_url: String,
    pub size: u64,
    pub mtime: f64,
    pub created_at: f64,
    pub modified_at: f64,
    pub ext: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration: Option<f64>,
    pub thumbnail_url: Option<String>,
    pub size_formatted: String,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ReadVideosResult {
    Error { error: String },
    Videos(Vec<VideoEntry>),
}

// ── fs:readVideos ─────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn fs_read_videos<R: Runtime>(
    app: AppHandle<R>,
    dir_path: String,
    state: State<'_, AppStateHandle>,
    pipeline: State<'_, PipelineHandle>,
    server: State<'_, VideoServerState>,
) -> Result<ReadVideosResult, String> {
    let path = Path::new(&dir_path);

    match tokio::fs::metadata(path).await {
        Err(_) => {
            return Ok(ReadVideosResult::Error {
                error: "not_found".into(),
            })
        }
        Ok(m) if !m.is_dir() => {
            return Ok(ReadVideosResult::Error {
                error: "not_found".into(),
            })
        }
        _ => {}
    }

    let mut raw_videos: Vec<RawVideo> = vec![];
    collect_videos(path, &mut raw_videos).await;
    raw_videos.sort_by(|a, b| {
        b.mtime
            .partial_cmp(&a.mtime)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Purge stale cache entries
    {
        let fresh_paths: std::collections::HashSet<_> =
            raw_videos.iter().map(|v| v.file_path.clone()).collect();
        let stale: Vec<_> = state
            .read_dim_cache(|c| {
                c.keys()
                    .filter(|p| !fresh_paths.contains(*p) && !Path::new(p).exists())
                    .cloned()
                    .collect()
            })
            .await;
        if !stale.is_empty() {
            state
                .mutate_dim_cache(|c| {
                    for p in &stale {
                        c.remove(p);
                    }
                })
                .await;
        }
    }

    // Build initial response using cached dims (no ffprobe yet)
    let videos: Vec<VideoEntry> = state
        .read_dim_cache(|cache| {
            raw_videos
                .iter()
                .filter_map(|v| {
                    let dims = cache.get(&v.file_path);
                    if let Some(d) = dims {
                        if d.no_stream && (d.mtime - v.mtime).abs() < 1.0 {
                            return None; // skip blank cards for known-bad files
                        }
                    }
                    let thumb_path = thumb_path_for_file(&v.file_path);
                    let thumbnail_url = if thumb_path.exists() {
                        Some(thumb_url_for_path(&thumb_path))
                    } else {
                        None
                    };
                    Some(VideoEntry {
                        id: v.id.clone(),
                        file_name: v.file_name.clone(),
                        file_path: v.file_path.clone(),
                        video_url: video_url_for_path(
                            &std::path::PathBuf::from(&v.file_path),
                            server.port(),
                        ),
                        size: v.size,
                        mtime: v.mtime,
                        created_at: v.created_at,
                        modified_at: v.mtime,
                        ext: v.ext.clone(),
                        width: dims.and_then(|d| d.width),
                        height: dims.and_then(|d| d.height),
                        duration: dims.and_then(|d| d.duration),
                        thumbnail_url,
                        size_formatted: format_size(v.size),
                    })
                })
                .collect()
        })
        .await;

    // Build video_map for the pipeline
    let video_map: HashMap<String, VideoMeta> = videos
        .iter()
        .map(|v| {
            (
                v.file_path.clone(),
                VideoMeta {
                    id: v.id.clone(),
                    mtime: v.mtime,
                    thumbnail_url: v.thumbnail_url.clone(),
                },
            )
        })
        .collect();

    // Arm the pipeline
    pipeline.set_video_map(video_map).await;

    // Spawn workers if this is the first call (idempotent via OnceLock)
    spawn_workers_once(&app, state.inner().clone(), pipeline.inner().clone());

    // Start folder watcher
    watcher::start(&app, dir_path.clone()).await;

    Ok(ReadVideosResult::Videos(videos))
}

// ── fs:readVideoEntries ───────────────────────────────────────────────────────
// Command ligero usado por applyDiff en el renderer.
// Recibe los paths específicos detectados por el watcher como nuevos y devuelve
// su metadata lista para mostrar en la UI, sin hacer walk completo del
// directorio, sin reiniciar el watcher y sin tocar el pipeline.
//
// Por qué existe: applyDiff antes llamaba fs_read_videos, que hace un walk
// completo Y llama watcher::start (que internamente hace stop() → nuevo
// watcher), introduciendo una ventana ciega donde se pierden eventos del OS.

#[tauri::command]
pub async fn fs_read_video_entries(
    file_paths: Vec<String>,
    state: State<'_, AppStateHandle>,
    pipeline: State<'_, PipelineHandle>,
    server: State<'_, VideoServerState>,
) -> Result<Vec<VideoEntry>, String> {
    if file_paths.is_empty() {
        return Ok(vec![]);
    }

    let mut entries = Vec::new();

    for file_path in &file_paths {
        let path = Path::new(file_path);

        // Stat del archivo — si no existe o no es accesible, se omite
        let meta = match tokio::fs::metadata(path).await {
            Ok(m) if m.is_file() => m,
            _ => continue,
        };

        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_owned(),
            None => continue,
        };

        let ext = match path.extension().and_then(|e| e.to_str()) {
            Some(e) => e.to_ascii_lowercase(),
            None => continue,
        };

        if !is_video_ext(&ext) {
            continue;
        }

        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0);

        let created_at = meta
            .created()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(mtime);

        let id = video_id(file_path);

        // Leer dims del cache si están disponibles. Para un archivo recién
        // descargado normalmente no estarán aún — el pipeline las generará
        // después de que applyDiff llame pipeline_process.
        let maybe_entry = state
            .read_dim_cache(|cache| {
                let dims = cache.get(file_path.as_str());
                let thumb_path = thumb_path_for_file(file_path);
                let thumb_url = if thumb_path.exists() {
                    Some(thumb_url_for_path(&thumb_path))
                } else {
                    None
                };
                if let Some(d) = dims {
                    // Archivo conocido como sin stream válido con mismo mtime — omitir
                    if d.no_stream && (d.mtime - mtime).abs() < 1.0 {
                        return None;
                    }
                    Some((d.width, d.height, d.duration, thumb_url))
                } else {
                    Some((None, None, None, thumb_url))
                }
            })
            .await;

        let (width, height, duration, thumbnail_url) = match maybe_entry {
            Some(t) => t,
            None => continue, // known no_stream — skip
        };

        let entry = VideoEntry {
            id: id.clone(),
            file_name,
            file_path: file_path.clone(),
            video_url: video_url_for_path(&std::path::PathBuf::from(file_path), server.port()),
            size: meta.len(),
            mtime,
            created_at,
            modified_at: mtime,
            ext: ext.to_uppercase(),
            width,
            height,
            duration,
            thumbnail_url: thumbnail_url.clone(),
            size_formatted: format_size(meta.len()),
        };

        // Registrar en el video_map del pipeline para que pipeline_process
        // pueda generar el thumbnail. Sin esto el worker busca el path en
        // video_map, no lo encuentra, y sale silenciosamente → card sin thumb.
        pipeline
            .insert_video_meta(
                file_path.clone(),
                VideoMeta {
                    id,
                    mtime,
                    thumbnail_url,
                },
            )
            .await;

        entries.push(entry);
    }

    Ok(entries)
}

// ── dialog:openFolder ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn dialog_open_folder<R: Runtime>(
    app: AppHandle<R>,
    title: Option<String>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut builder = app.dialog().file();
    if let Some(t) = title {
        builder = builder.set_title(&t);
    }
    builder.pick_folder(move |path| {
        // pick_folder returns Option<FilePath> — convert to Option<String>
        let _ = tx.send(path.map(|p| p.to_string()));
    });
    Ok(rx.await.map_err(|e| e.to_string())?)
}

// ── shell:showInFolder ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn shell_show_in_folder<R: Runtime>(
    app: AppHandle<R>,
    file_path: String,
) -> Result<(), String> {
    use tauri_plugin_shell::ShellExt;
    if file_path.is_empty() {
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        app.shell()
            .command("explorer")
            .args([format!("/select,{}", file_path)])
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        app.shell()
            .command("open")
            .args(["-R", &file_path])
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        app.shell()
            .command("xdg-open")
            .args([Path::new(&file_path)
                .parent()
                .unwrap_or(Path::new("/"))
                .to_str()
                .unwrap_or("/")])
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ── shell:copyPath ────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn shell_copy_path<R: Runtime>(
    app: AppHandle<R>,
    file_path: String,
) -> Result<(), String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    app.clipboard()
        .write_text(file_path)
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ── pipeline:cancel ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn pipeline_cancel(pipeline: State<'_, PipelineHandle>) -> Result<(), String> {
    pipeline.cancel().await;
    watcher::stop().await;
    Ok(())
}

// ── pipeline:process ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn pipeline_process(
    file_paths: Vec<String>,
    pipeline: State<'_, PipelineHandle>,
) -> Result<(), String> {
    if file_paths.is_empty() {
        return Ok(());
    }
    pipeline.reprioritize(file_paths).await;
    Ok(())
}

// ── Internal helpers ──────────────────────────────────────────────────────────

struct RawVideo {
    id: String,
    file_name: String,
    file_path: String,
    size: u64,
    mtime: f64,
    created_at: f64,
    ext: String,
}

fn video_id(file_path: &str) -> String {
    use sha1::{Digest, Sha1};
    let mut h = Sha1::new();
    h.update(file_path.as_bytes());
    hex::encode(h.finalize())
}

// Async-recursive directory walk using Box::pin (no external crate needed)
fn collect_videos<'a>(
    dir: &'a Path,
    out: &'a mut Vec<RawVideo>,
) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
    Box::pin(async move {
        let mut entries = match tokio::fs::read_dir(dir).await {
            Ok(e) => e,
            Err(_) => return,
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            let file_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_owned(),
                None => continue,
            };
            if file_name.starts_with('.') {
                continue;
            }
            if path.is_dir() {
                collect_videos(&path, out).await;
                continue;
            }
            let ext = match path.extension().and_then(|e| e.to_str()) {
                Some(e) => e.to_ascii_lowercase(),
                None => continue,
            };
            if !is_video_ext(&ext) {
                continue;
            }
            let meta = match tokio::fs::metadata(&path).await {
                Ok(m) => m,
                Err(_) => continue,
            };
            let file_path = path.to_string_lossy().to_string();
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0);
            let created_at = meta
                .created()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(mtime);

            out.push(RawVideo {
                id: video_id(&file_path),
                file_name,
                file_path,
                size: meta.len(),
                mtime,
                created_at,
                ext: ext.to_uppercase(),
            });
        }
    })
}

pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 * 1024 {
        format!("{} KB", bytes / 1024)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / 1024.0 / 1024.0)
    } else {
        format!("{:.2} GB", bytes as f64 / 1024.0 / 1024.0 / 1024.0)
    }
}

// ── video:getServerPort ───────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_video_server_port(server: State<'_, VideoServerState>) -> Result<u16, String> {
    Ok(server.port())
}

// ── Worker spawner (called once per app lifetime) ─────────────────────────────

use std::sync::OnceLock;
static WORKERS_SPAWNED: OnceLock<()> = OnceLock::new();

fn spawn_workers_once<R: Runtime>(
    app: &AppHandle<R>,
    state: AppStateHandle,
    pipeline: PipelineHandle,
) {
    WORKERS_SPAWNED.get_or_init(|| {
        let app = app.clone();
        pipeline.spawn_workers(state, move |event| match event {
            WorkerEvent::Dims {
                id,
                width,
                height,
                duration,
            } => {
                let _ = app.emit(
                    "dims:ready",
                    crate::pipeline::DimsReadyPayload {
                        id,
                        width,
                        height,
                        duration,
                    },
                );
            }
            WorkerEvent::Thumbnail { id, thumbnail_url } => {
                let _ = app.emit(
                    "thumbnail:ready",
                    crate::pipeline::ThumbnailReadyPayload { id, thumbnail_url },
                );
            }
            WorkerEvent::NoStream { id } => {
                let _ = app.emit(
                    "video:no-stream",
                    crate::pipeline::VideoNoStreamPayload { id },
                );
            }
        });
    });
}
