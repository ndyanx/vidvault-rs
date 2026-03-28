# VidVault — Tauri 2 Migration

Migración completa de Electron + Node.js → **Tauri 2 + Rust**.  
El frontend Vue 3 es idéntico. El proceso main de Electron fue reescrito completamente en Rust.

---

## Arquitectura

```
vidvault-tauri/
├── src-tauri/              ← Backend Rust (reemplaza src/main/index.js)
│   └── src/
│       ├── lib.rs          ← Setup de plugins, protocolo, commands
│       ├── main.rs         ← Entry point
│       ├── state.rs        ← Persistencia app-state.json + dimensions-cache.json
│       ├── pipeline.rs     ← OnDemandProcessor → Tokio tasks + Semaphore
│       ├── commands.rs     ← Todos los ipcMain.handle → #[tauri::command]
│       ├── video_protocol.rs ← protocolo localvideo:// con Range streaming
│       └── watcher.rs      ← Folder watcher con `notify` crate (real-time vs poll)
└── src/                    ← Renderer Vue 3 (sin cambios estructurales)
    ├── composables/        ← window.electronAPI.* → invoke/listen de Tauri
    └── components/         ← Drag-drop adaptado a tauri://drag-drop events
```

### Equivalencias Electron → Tauri

| Electron (Node.js) | Tauri (Rust) |
|---|---|
| `ipcMain.handle('fs:readVideos')` | `#[tauri::command] async fn fs_read_videos()` |
| `execFile('ffprobe', ...)` | `tokio::process::Command::new("ffprobe")` |
| `execFile('ffmpeg', ...)` | `tokio::process::Command::new("ffmpeg")` |
| `protocol.handle('localvideo', ...)` | `register_asynchronous_uri_scheme_protocol` |
| `new OnDemandProcessor()` | `PipelineHandle` con `Semaphore` + `Notify` |
| `setInterval(poll, 30_000)` | crate `notify` con eventos reales del FS |
| `app.getPath('userData')` | `dirs::data_dir()` + `"vidvault"` |
| `writeFile(state.json)` | `tokio::fs::write` con debounce 300ms |
| `shell.showItemInFolder` | `tauri-plugin-shell` |
| `clipboard.writeText` | `tauri-plugin-clipboard-manager` |
| `dialog.showOpenDialog` | `tauri-plugin-dialog` |
| `app.requestSingleInstanceLock()` | `tauri-plugin-single-instance` |
| `webUtils.getPathForFile(file)` | `file.path` directo (sin contextIsolation) |
| `ipcRenderer.invoke(...)` | `invoke(...)` de `@tauri-apps/api/core` |
| `ipcRenderer.on(channel, cb)` | `listen(channel, cb)` de `@tauri-apps/api/event` |

---

## Prerrequisitos

### Sistema
- **Rust** (stable) — https://rustup.rs
- **Node.js** 18+ — https://nodejs.org
- **ffmpeg + ffprobe** en PATH — https://ffmpeg.org/download.html

### macOS
```bash
xcode-select --install
brew install ffmpeg
```

### Windows
```powershell
winget install Gyan.FFmpeg
# O descarga desde https://www.gyan.dev/ffmpeg/builds/
```

### Linux (Ubuntu/Debian)
```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
                 librsvg2-dev patchelf ffmpeg
```

---

## Instalación

```bash
# 1. Instalar dependencias JS
npm install

# 2. Tauri CLI (si no lo tienes)
cargo install tauri-cli --version "^2.0"

# 3. Dev mode (Vite hot-reload + Rust auto-recompile)
npm run tauri dev

# 4. Build de producción
npm run tauri build
```

El binario final estará en `src-tauri/target/release/vidvault`.

---

## Notas de migración

### Thumbnails existentes
Los thumbnails generados por la versión Electron se migran automáticamente del layout plano
(`thumbnails/{base64}.jpg`) al bucketed (`thumbnails/{xx}/{yy}/{sha1}.jpg`) en el primer
arranque. No se pierde ningún thumbnail.

### app-state.json
El JSON persiste en el mismo directorio (`$userData/app-state.json`) con el mismo schema.
Si el usuario ya tenía la versión Electron, el estado se carga correctamente.

### Drag & drop
La versión Electron usaba `webUtils.getPathForFile()` en el preload porque el renderer
tenía `contextIsolation: true`. En Tauri no existe ese sandbox: `file.path` está disponible
directamente. Además, se escuchan los eventos nativos `tauri://drag-drop` para mayor
fiabilidad en macOS y Windows.

### Protocolo localvideo://
Implementado con `register_asynchronous_uri_scheme_protocol`. Soporta Range requests
(`206 Partial Content`) para que el `<video>` pueda hacer seeking eficiente.

### Watcher de carpetas
La versión Electron usaba polling cada 30 segundos. Tauri usa el crate `notify` con
inotify/FSEvents/ReadDirectoryChangesW según el SO — los cambios llegan en < 200ms.

---

## Estructura de datos en disco

```
$HOME/.local/share/vidvault/     (Linux)
~/Library/Application Support/vidvault/  (macOS)
%APPDATA%\vidvault\              (Windows)
├── app-state.json               ← lastFolder, folderHistory, favorites, theme, locale
├── dimensions-cache.json        ← filePath → { width, height, duration, mtime }
└── thumbnails/
    └── {xx}/
        └── {yy}/
            └── {sha1}.jpg
```
