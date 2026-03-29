// watcher.rs
// Replaces Electron's poll-based folder watcher (setInterval every 30s).
// Uses the `notify` crate for real filesystem events — no polling needed.
// Emits "folder:changed" to the renderer with { added, removed } payload.
//
// FIXES vs previous version:
//
// 1. Snapshot de archivos conocidos — igual que Electron's watchSnapshot.
//    Al arrancar, se escanea el directorio y se guarda el conjunto de paths
//    ya conocidos. Solo se emite "added" para paths que NO estaban en el
//    snapshot. Los eventos Modify sobre archivos existentes se ignoran,
//    evitando applyDiff innecesarios y la race condition de las dos cards.
//
// 2. Modify ignorado para archivos ya conocidos — un Modify de un video
//    existente (antivirus, metadata, escritura parcial) ya no dispara
//    folder:changed. Solo Create/Rename-to de archivos nuevos lo hace.

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

// Recorre el directorio de forma síncrona para construir el snapshot inicial.
// Se llama una sola vez en start(), en un thread bloqueante, antes de que el
// watcher empiece a recibir eventos.
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

        // Build the initial snapshot of known video files BEFORE attaching the
        // watcher. This way any Modify event for a pre-existing file is ignored.
        // We do this in a blocking task so we don't block the tokio runtime.
        let dir_clone = dir.clone();
        let known_set = tokio::task::spawn_blocking(move || collect_known(&dir_clone))
            .await
            .unwrap_or_default();

        // known is shared with the blocking thread via a Mutex.
        let known: std::sync::Arc<Mutex<HashSet<String>>> =
            std::sync::Arc::new(Mutex::new(known_set));

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
        let (async_tx, mut async_rx) = tokio::sync::mpsc::channel::<(Vec<String>, Vec<String>)>(32);

        let known_thread = known.clone();
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

                for path in &event.paths {
                    if !is_video(path) {
                        continue;
                    }
                    let s = path.to_string_lossy().to_string();

                    match event.kind {
                        EventKind::Create(_) => {
                            // Always a new file (or rename-to on macOS/Windows).
                            // Update snapshot so subsequent Modify events are ignored.
                            known_thread.lock().unwrap().insert(s.clone());
                            added.push(s);
                        }
                        EventKind::Modify(_) => {
                            // Only emit as "added" if this path wasn't known before.
                            // This handles rename-to on Linux (inotify emits Modify(Name(To)))
                            // while ignoring writes to files already in the gallery.
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
