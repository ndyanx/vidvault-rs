// watcher.rs
// Replaces Electron's poll-based folder watcher (setInterval every 30s).
// Uses the `notify` crate for real filesystem events — no polling needed.
// Emits "folder:changed" to the renderer with { added, removed } payload.

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use std::collections::HashSet;
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

fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| VIDEO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

// ── Global watcher handle (stop token) ───────────────────────────────────────

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

        // Channel for raw notify events
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

        // Drain events in a blocking thread, forward to tokio via mpsc
        let (async_tx, mut async_rx) =
            tokio::sync::mpsc::channel::<(Vec<String>, Vec<String>)>(32);

        std::thread::spawn(move || {
            // Keep watcher alive in this thread
            let _watcher = watcher;
            for result in rx {
                let event = match result {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                let mut added = vec![];
                let mut removed = vec![];
                for path in event.paths {
                    if !is_video(&path) {
                        continue;
                    }
                    let s = path.to_string_lossy().to_string();
                    match event.kind {
                        EventKind::Create(_) | EventKind::Modify(_) => added.push(s),
                        EventKind::Remove(_) => removed.push(s),
                        _ => {}
                    }
                }
                if !added.is_empty() || !removed.is_empty() {
                    let _ = async_tx.blocking_send((added, removed));
                }
            }
        });

        // Merge rapid bursts with a small debounce
        loop {
            tokio::select! {
                _ = &mut stop_rx => {
                    eprintln!("[watcher] Stopped for {}", dir_path);
                    break;
                }
                Some((added, removed)) = async_rx.recv() => {
                    // Drain any burst within 200ms
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
                    // Deduplicate
                    let added_set: HashSet<_> = all_added.into_iter().collect();
                    let removed_set: HashSet<_> = all_removed.into_iter().collect();
                    // If a file appears in both added + removed it was modified — keep as added
                    let final_removed: Vec<_> = removed_set.difference(&added_set).cloned().collect();
                    let final_added: Vec<_> = added_set.into_iter().collect();

                    if !final_added.is_empty() || !final_removed.is_empty() {
                        let _ = app.emit(
                            "folder:changed",
                            FolderChangedPayload {
                                added: final_added,
                                removed: final_removed,
                            },
                        );
                    }
                }
            }
        }
    });
}
