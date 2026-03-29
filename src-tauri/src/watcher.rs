// Filesystem watcher for the active video folder.
//
// Uses the `notify` crate for real OS events instead of polling.
// Emits "folder:changed" to the renderer with { added, removed } arrays.
//
// Key behaviors:
// - An initial snapshot of known files is taken before the watcher attaches,
//   so Modify events for pre-existing files are silently ignored.
// - On Linux, inotify emits Modify(Name(To)) for rename-to; we treat paths
//   absent from the snapshot as additions, same as Create.
// - Rapid bursts (e.g. multi-file downloads) are coalesced with a 200 ms
//   debounce before the event is forwarded to the renderer.
// - A file appearing in both added and removed within the same burst is kept
//   as added (file was modified, not truly removed).
// - Newly added files are held in a "pending" queue and only forwarded once
//   their size has been stable for STABLE_TICKS consecutive seconds. This
//   prevents yt-dlp's metadata-embed rename (temp.mp4 -> .mp4) from racing
//   with ffprobe/thumbnail generation and causing [WinError 5] on Windows.

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Runtime};
use tokio::sync::oneshot;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FolderChangedPayload {
    added: Vec<String>,
    removed: Vec<String>,
}

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mov", "mkv", "avi", "webm", "m4v", "wmv", "flv", "3gp", "ts", "mts",
];

/// How many consecutive stable polls before we consider the file fully written.
/// Each poll interval is POLL_MS milliseconds.
/// 3 ticks x 1 000 ms = 3 s of silence -> safe for yt-dlp metadata embed.
const STABLE_TICKS: u32 = 5;
const POLL_MS: u64 = 1_000;

fn is_temp_file(path: &Path) -> bool {
    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n.to_ascii_lowercase(),
        None => return false,
    };
    // Exclude yt-dlp temp files (.temp.mp4, .part, .ytdl) and other
    // common in-progress download patterns before the final rename.
    name.contains(".temp.")
        || name.ends_with(".part")
        || name.ends_with(".ytdl")
        || name.ends_with(".download")
        || name.ends_with(".crdownload")
}

fn is_video(path: &Path) -> bool {
    if is_temp_file(path) {
        return false;
    }
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| VIDEO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Synchronous recursive scan to build the initial snapshot of known files.
/// Called in a blocking task before the watcher starts receiving events.
fn collect_known(dir: &Path) -> HashSet<String> {
    let mut known = HashSet::new();
    collect_known_recursive(dir, &mut known);
    known
}

fn collect_known_recursive(dir: &Path, out: &mut HashSet<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_owned(),
            None => continue,
        };
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect_known_recursive(&path, out);
        } else if is_video(&path) {
            out.insert(path.to_string_lossy().to_string());
        }
    }
}

// -- File-stability checker ---------------------------------------------------
//
// Tracks files that were just created/renamed into the folder. Each entry
// records the last observed file size and how many consecutive polls found
// the same size. Once stable_ticks reaches STABLE_TICKS the file is promoted
// to the "ready" list and emitted to the renderer.

#[derive(Debug)]
struct PendingEntry {
    last_size: u64,
    stable_ticks: u32,
}

/// Poll all pending paths once. Returns paths that have become stable.
async fn poll_pending(pending: &mut HashMap<String, PendingEntry>) -> Vec<String> {
    let mut ready = vec![];
    let mut gone = vec![];

    for (path_str, entry) in pending.iter_mut() {
        match tokio::fs::metadata(path_str).await {
            Ok(meta) => {
                let size = meta.len();
                if size == entry.last_size && size > 0 {
                    entry.stable_ticks += 1;
                    if entry.stable_ticks >= STABLE_TICKS {
                        ready.push(path_str.clone());
                    }
                } else {
                    // Still growing (or just appeared with size 0) -- reset
                    entry.last_size = size;
                    entry.stable_ticks = 0;
                }
            }
            Err(_) => {
                // File disappeared (e.g. cancelled download) -- drop it
                gone.push(path_str.clone());
            }
        }
    }

    for p in &ready {
        pending.remove(p);
    }
    for p in &gone {
        pending.remove(p);
    }

    ready
}

// -- Global stop token --------------------------------------------------------

type StopTx = oneshot::Sender<()>;
static STOP: Mutex<Option<StopTx>> = Mutex::new(None);

pub async fn stop() {
    if let Some(tx) = STOP.lock().unwrap().take() {
        let _ = tx.send(());
    }
}

pub async fn start<R: Runtime>(app: &AppHandle<R>, dir_path: String) {
    stop().await;

    let (stop_tx, mut stop_rx) = oneshot::channel::<()>();
    *STOP.lock().unwrap() = Some(stop_tx);

    let app = app.clone();

    tokio::spawn(async move {
        let dir = PathBuf::from(&dir_path);

        let dir_clone = dir.clone();
        let known_set = tokio::task::spawn_blocking(move || collect_known(&dir_clone))
            .await
            .unwrap_or_default();

        let known: std::sync::Arc<Mutex<HashSet<String>>> =
            std::sync::Arc::new(Mutex::new(known_set));

        let (tx, rx) = std::sync::mpsc::channel::<notify::Result<Event>>();

        let mut watcher = match RecommendedWatcher::new(tx, Config::default()) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("[watcher] Failed to create watcher: {}", e);
                return;
            }
        };

        if let Err(e) = watcher.watch(&dir, RecursiveMode::Recursive) {
            eprintln!("[watcher] Failed to watch {}: {}", dir.display(), e);
            return;
        }

        eprintln!("[watcher] Watching {}", dir.display());

        // raw OS events -> async task
        let (async_tx, mut async_rx) = tokio::sync::mpsc::channel::<(Vec<String>, Vec<String>)>(32);

        let known_thread = known.clone();
        std::thread::spawn(move || {
            let _watcher = watcher;
            for result in rx {
                let event = match result {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                let mut added = vec![];
                let mut removed = vec![];

                for path in &event.paths {
                    if !is_video(path) {
                        continue;
                    }
                    let s = path.to_string_lossy().to_string();

                    match event.kind {
                        EventKind::Create(_) => {
                            known_thread.lock().unwrap().insert(s.clone());
                            added.push(s);
                        }
                        EventKind::Modify(_) => {
                            // On Linux, rename-to arrives as Modify(Name(To)).
                            // Treat it as added only if the path wasn't in the
                            // snapshot; otherwise it's a write to an existing file.
                            let is_new = {
                                let mut k = known_thread.lock().unwrap();
                                if k.contains(&s) {
                                    false
                                } else {
                                    k.insert(s.clone());
                                    true
                                }
                            };
                            if is_new {
                                added.push(s);
                            }
                        }
                        EventKind::Remove(_) => {
                            known_thread.lock().unwrap().remove(&s);
                            removed.push(s);
                        }
                        _ => {}
                    }
                }

                if !added.is_empty() || !removed.is_empty() {
                    let _ = async_tx.blocking_send((added, removed));
                }
            }
        });

        // pending: files waiting to stabilise before being emitted
        let mut pending: HashMap<String, PendingEntry> = HashMap::new();

        let poll_interval = std::time::Duration::from_millis(POLL_MS);

        loop {
            tokio::select! {
                _ = &mut stop_rx => {
                    eprintln!("[watcher] Stopped for {}", dir_path);
                    break;
                }

                // Drain all OS events that arrived, coalescing bursts within 200 ms.
                Some((added, removed)) = async_rx.recv() => {
                    let mut all_added = added;
                    let mut all_removed = removed;
                    loop {
                        tokio::select! {
                            Some((a, r)) = async_rx.recv() => {
                                all_added.extend(a);
                                all_removed.extend(r);
                            }
                            _ = tokio::time::sleep(std::time::Duration::from_millis(200)) => break,
                        }
                    }

                    // Queue added files into the pending stability check
                    for path_str in all_added {
                        let size = tokio::fs::metadata(&path_str)
                            .await
                            .map(|m| m.len())
                            .unwrap_or(0);
                        eprintln!("[watcher] Pending stability check: {} ({} bytes)", path_str, size);
                        pending.entry(path_str).or_insert(PendingEntry {
                            last_size: size,
                            stable_ticks: 0,
                        });
                    }

                    // Removed events are emitted immediately (no stability check needed)
                    let all_removed: Vec<_> = all_removed.into_iter().collect();
                    if !all_removed.is_empty() {
                        let _ = app.emit(
                            "folder:changed",
                            FolderChangedPayload {
                                added: vec![],
                                removed: all_removed,
                            },
                        );
                    }
                }

                // Every POLL_MS, check stability of pending files
                _ = tokio::time::sleep(poll_interval), if !pending.is_empty() => {
                    let ready = poll_pending(&mut pending).await;
                    if !ready.is_empty() {
                        eprintln!("[watcher] {} file(s) stable -> emitting to renderer", ready.len());
                        let _ = app.emit(
                            "folder:changed",
                            FolderChangedPayload {
                                added: ready,
                                removed: vec![],
                            },
                        );
                    }
                }
            }
        }
    });
}
