# VidVault

Local video library viewer built with Tauri 2 + Vue 3.

---

## Features

- **Virtual masonry gallery** — hardware-accelerated grid that only renders visible cards, keeping performance smooth on large libraries.
- **Background pipeline** — 4 concurrent workers run `ffprobe` + `ffmpeg` to extract dimensions and generate thumbnails without blocking the UI.
- **Real-time folder watcher** — detects new, modified, and removed files via OS events (no polling). Newly downloaded files are held in a stability queue until their size has been stable for 5 s, preventing race conditions with tools like yt-dlp.
- **Video streaming** — axum HTTP server on a random local port handles range requests so the `<video>` element can seek without buffering the full file.
- **Favorites** — pin videos across sessions.
- **Folder history** — the last opened folders are remembered with a preview thumbnail.
- **Sort** — sort the gallery by date modified, date created, name, or size.
- **Themes** — light and dark mode, persisted across restarts.
- **i18n** — English and Spanish UI; locale is persisted.
- **Global volume** — volume and mute state are shared across the modal and hover previews, and survive restarts.
- **Single instance** — launching a second instance focuses the existing window.
- **Window state** — size and position are restored on restart.

---

## Architecture

```
vidvault/
├── src-tauri/src/
│   ├── lib.rs              — app setup, plugins, IPC registration
│   ├── state.rs            — persisted app state + dimensions cache (debounced writes)
│   ├── pipeline.rs         — background workers: ffprobe + ffmpeg, 4 concurrent
│   ├── commands.rs         — all #[tauri::command] handlers
│   ├── video_server.rs     — axum HTTP server for video streaming (range requests)
│   ├── video_protocol.rs   — localvideo:// custom protocol for thumbnails
│   └── watcher.rs          — real-time folder watcher via notify crate
└── src/
    ├── composables/
    │   ├── useVideoLibrary.js    — folder scan, pipeline events, diff/patch
    │   ├── useVirtualMasonry.js  — layout engine + virtual scroll
    │   ├── useFavorites.js       — favorites state (persisted via IPC store)
    │   ├── useTheme.js           — light/dark theme
    │   ├── useLocale.js          — i18n locale switching
    │   └── useVolume.js          — shared global volume state
    ├── components/
    │   ├── GalleryPanel.vue      — masonry grid, sorting, context menu
    │   ├── VideoModal.vue        — fullscreen player with keyboard navigation
    │   ├── TitleBar.vue          — custom title bar, folder picker, history dropdown
    │   ├── EmptyState.vue        — drag-and-drop landing screen
    │   └── VideoSkeleton.vue     — loading placeholder card
    └── locales/
        ├── en.js
        └── es.js
```

### Why two file-serving mechanisms

Videos are served by `video_server.rs` (axum on `127.0.0.1:{random port}`) because the
custom protocol passes through wry, which cannot support real range requests — the WebView
would buffer the entire file before playback [wry#1404](https://github.com/tauri-apps/wry/issues/1404).

Thumbnails are served via `localvideo://` because they are small JPEGs loaded in one shot.

### Dimensions cache

`dimensions-cache.json` stores `{ width, height, duration, mtime }` per file path.
On re-opening a folder the cached values are returned immediately (no ffprobe needed),
and the pipeline only probes files whose mtime has changed or that have no entry yet.
Files where ffprobe finds no valid video stream are marked `no_stream: true` and their
cards are hidden from the gallery.

---

## Prerequisites

- Rust (stable) — https://rustup.rs
- Node.js 18+
- ffmpeg + ffprobe in PATH — https://ffmpeg.org/download.html

### macOS
```bash
xcode-select --install
brew install ffmpeg
```

### Windows
```powershell
winget install Gyan.FFmpeg
```

On Windows, VidVault also probes common install locations (`C:\ffmpeg\bin`, Chocolatey,
Scoop) before falling back to PATH, so ffmpeg does not need to be on the system PATH.

### Linux (Ubuntu/Debian)
```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
                 librsvg2-dev patchelf ffmpeg
```

---

## Setup

```bash
npm install
npm run tauri dev       # dev mode (Vite + Rust watch)
npm run tauri build     # production build → src-tauri/target/release/vidvault
```

---

## Data on disk

```
~/.local/share/vidvault/                    (Linux)
~/Library/Application Support/vidvault/    (macOS)
%APPDATA%\vidvault\                         (Windows)
├── app-state.json          — lastFolder, folderHistory, favorites, theme, locale, sortBy
├── dimensions-cache.json   — filePath → { width, height, duration, mtime, no_stream? }
└── thumbnails/
    └── {xx}/{yy}/{sha1}.jpg
```

State and cache writes are debounced 300 ms to avoid hammering disk during rapid updates
(e.g. scrolling through a large library that triggers many pipeline events).

---

## Windows note

In production, WebView2 blocks requests to `http://127.0.0.1` as mixed content.
This is handled in `tauri.conf.json` with:

```
--allow-running-insecure-content
--unsafely-treat-insecure-origin-as-secure=http://127.0.0.1
```

These flags are safe because `127.0.0.1` is never reachable from outside the machine.
