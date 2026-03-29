# VidVault — Tauri 2

Migración completa de Electron + Node.js a **Tauri 2 + Rust**.  
El frontend Vue 3 es idéntico. El proceso main de Electron fue reescrito completamente en Rust.

---

## Arquitectura

```
vidvault-rs/
├── src-tauri/              <- Backend Rust (reemplaza src/main/index.js de Electron)
│   └── src/
│       ├── lib.rs          <- Setup de plugins, protocolo, servidor HTTP, commands
│       ├── main.rs         <- Entry point
│       ├── state.rs        <- Persistencia app-state.json + dimensions-cache.json
│       ├── pipeline.rs     <- OnDemandProcessor -> Tokio tasks + Semaphore (4 workers)
│       ├── commands.rs     <- Todos los ipcMain.handle -> #[tauri::command]
│       ├── video_server.rs <- Servidor HTTP real (axum) para streaming de video
│       ├── video_protocol.rs <- Protocolo localvideo:// para thumbnails (imágenes)
│       └── watcher.rs      <- Folder watcher con crate notify (real-time vs polling)
└── src/                    <- Renderer Vue 3
    ├── composables/        <- window.electronAPI.* -> invoke/listen de Tauri
    └── components/         <- Drag-drop adaptado a eventos tauri://drag-*
```

### Por qué dos mecanismos para servir archivos locales

| Mecanismo | Qué sirve | Por qué |
|---|---|---|
| `video_server.rs` (axum, TCP) | Archivos de video | Streaming real con range requests. wry tiene una limitación conocida (wry#1404) que impide streaming verdadero a través de protocolos custom — el WebView acumula el archivo completo en memoria antes de reproducir. axum escucha en `127.0.0.1:{puerto aleatorio}` y sirve por TCP puro, sin que wry intervenga. |
| `video_protocol.rs` (localvideo://) | Thumbnails (JPEG) | Las imágenes son pequeñas y se cargan de una vez. El protocolo custom funciona bien para este caso. |

### Equivalencias Electron -> Tauri

| Electron (Node.js) | Tauri (Rust) |
|---|---|
| `ipcMain.handle('fs:readVideos')` | `#[tauri::command] async fn fs_read_videos()` |
| `execFile('ffprobe', ...)` | `tokio::process::Command::new("ffprobe")` |
| `execFile('ffmpeg', ...)` | `tokio::process::Command::new("ffmpeg")` |
| `protocol.handle('localvideo', ...)` | `register_asynchronous_uri_scheme_protocol` |
| Servidor de video con Express/http | `axum` en `127.0.0.1` con Tokio |
| `new OnDemandProcessor()` | `PipelineHandle` con `Semaphore` + `Notify` |
| `setInterval(poll, 30_000)` | crate `notify` con eventos reales del FS |
| `app.getPath('userData')` | `dirs::data_dir()` + `"vidvault"` |
| `writeFile(state.json)` | `tokio::fs::write` con debounce 300ms |
| `shell.showItemInFolder` | `tauri-plugin-shell` |
| `clipboard.writeText` | `tauri-plugin-clipboard-manager` |
| `dialog.showOpenDialog` | `tauri-plugin-dialog` |
| `app.requestSingleInstanceLock()` | `tauri-plugin-single-instance` |
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

### Streaming de video
La versión Electron usaba `protocol.handle('localvideo', ...)` con soporte nativo de range
requests del renderer de Chromium embebido. En Tauri, wry intercepta el protocolo custom y
no permite streaming real (wry#1404). La solución es `video_server.rs`: un servidor axum que
arranca en `127.0.0.1` en un puerto aleatorio al iniciar la app. El frontend recibe URLs
`http://127.0.0.1:{puerto}/...` para video y `localvideo://...` para thumbnails.

### WebView2 en Windows (release)
En producción, el origen del renderer es `tauri://localhost`, que WebView2 trata como
secure context. Las peticiones a `http://127.0.0.1` son bloqueadas como mixed content.
Se resuelve con `additionalBrowserArgs` en `tauri.conf.json`:

```
--allow-running-insecure-content
--unsafely-treat-insecure-origin-as-secure=http://127.0.0.1
```

Estas flags son seguras porque `127.0.0.1` nunca es accesible desde fuera de la máquina.

### Thumbnails existentes
Los thumbnails generados por la versión Electron se migran automáticamente del layout plano
(`thumbnails/{base64}.jpg`) al bucketed (`thumbnails/{xx}/{yy}/{sha1}.jpg`) en el primer
arranque. No se pierde ningún thumbnail.

### app-state.json
El JSON persiste en el mismo directorio (`$userData/app-state.json`) con el mismo schema.
Si el usuario ya tenía la versión Electron, el estado se carga correctamente.

### Drag & drop
La versión Electron usaba `webUtils.getPathForFile()` en el preload porque el renderer
tenía `contextIsolation: true`. En Tauri se escuchan los eventos nativos `tauri://drag-drop`,
`tauri://drag-enter` y `tauri://drag-leave` para mayor fiabilidad en macOS y Windows.

### Watcher de carpetas
La versión Electron usaba polling cada 30 segundos. Tauri usa el crate `notify` con
inotify / FSEvents / ReadDirectoryChangesW según el SO — los cambios llegan en < 200ms.

---

## Estructura de datos en disco

```
$HOME/.local/share/vidvault/          (Linux)
~/Library/Application Support/vidvault/  (macOS)
%APPDATA%\vidvault\                   (Windows)
├── app-state.json               <- lastFolder, folderHistory, favorites, theme, locale
├── dimensions-cache.json        <- filePath -> { width, height, duration, mtime }
└── thumbnails/
    └── {xx}/
        └── {yy}/
            └── {sha1}.jpg
```

---

## Consumo de recursos

El ejecutable de VidVault ocupa ~3MB en disco y ~10MB de RAM en ejecución. En Windows,
WebView2 aparece como un proceso separado con mayor consumo porque es el motor de
renderizado compartido del sistema (el mismo que usa Edge). Ese costo existiría aunque
VidVault no estuviera abierto — no es overhead exclusivo de la app.
