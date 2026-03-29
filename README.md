# VidVault

Local video library viewer built with Tauri 2 + Vue 3.

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
    ├── composables/        — useVideoLibrary, useFavorites, useTheme, useLocale
    └── components/         — GalleryPanel, VideoModal, TitleBar, EmptyState
```

### Why two file-serving mechanisms

Videos are served by `video_server.rs` (axum on `127.0.0.1:{random port}`) because the
custom protocol passes through wry, which cannot support real range requests — the WebView
would buffer the entire file before playback (wry#1404).

Thumbnails are served via `localvideo://` because they are small JPEGs loaded in one shot.

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
~/.local/share/vidvault/            (Linux)
~/Library/Application Support/vidvault/  (macOS)
%APPDATA%\vidvault\                 (Windows)
├── app-state.json          — lastFolder, folderHistory, favorites, theme, locale
├── dimensions-cache.json   — filePath → { width, height, duration, mtime }
└── thumbnails/
    └── {xx}/{yy}/{sha1}.jpg
```

---

## Windows note

In production, WebView2 blocks requests to `http://127.0.0.1` as mixed content.
This is handled in `tauri.conf.json` with:

```
--allow-running-insecure-content
--unsafely-treat-insecure-origin-as-secure=http://127.0.0.1
```

These flags are safe because `127.0.0.1` is never reachable from outside the machine.
