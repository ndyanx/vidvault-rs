// video_server.rs
// Servidor HTTP real con axum escuchando en 127.0.0.1 en un puerto aleatorio.
//
// Por qué existe este archivo:
//   El protocolo custom localvideo:// pasa por wry/WebView y tiene una limitación
//   conocida (wry#1404): no soporta streaming real. El WebView carga el archivo
//   completo en memoria antes de reproducirlo, lo que rompe seek y hace que
//   archivos grandes nunca terminen de cargar.
//
//   La solución es levantar un servidor HTTP TCP normal. El <video> del frontend
//   apunta a http://127.0.0.1:{puerto}/... — una conexión TCP estándar que axum
//   sirve con range requests reales, igual que cualquier servidor web. wry no
//   interviene en absoluto.
//
// Flujo:
//   1. En setup, llamamos start_video_server() → vincula en puerto aleatorio
//   2. El puerto se guarda en VideoServerState (managed por Tauri)
//   3. El command get_video_server_port lo expone al backend (pipeline/commands)
//   4. video_url_for_path() construye http://127.0.0.1:{puerto}/...
//   5. thumb_url_for_path() sigue usando localvideo:// (thumbnails son pequeños,
//      no necesitan streaming)

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use std::sync::Arc;
use tokio::io::AsyncSeekExt;
use tokio::net::TcpListener;

// ── Estado compartido del servidor ───────────────────────────────────────────

/// El puerto en el que está escuchando el servidor. Se almacena en Tauri state
/// para que commands.rs y pipeline.rs puedan construir URLs sin IPC adicional.
#[derive(Clone)]
pub struct VideoServerState(Arc<u16>);

impl VideoServerState {
    pub fn port(&self) -> u16 {
        *self.0
    }
}

// ── Arranque ──────────────────────────────────────────────────────────────────

/// Vincula el servidor en 127.0.0.1:0 (puerto aleatorio asignado por el OS),
/// lo lanza en background y devuelve el estado con el puerto real.
pub async fn start_video_server() -> VideoServerState {
    // Puerto 0 → el OS elige un puerto libre automáticamente
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("[video_server] No se pudo vincular en 127.0.0.1:0");

    let port = listener
        .local_addr()
        .expect("[video_server] local_addr falló")
        .port();

    eprintln!("[video_server] Escuchando en 127.0.0.1:{}", port);

    let state = VideoServerState(Arc::new(port));

    let app = Router::new()
        .route("/*path", get(serve_file))
        .with_state(state.clone());

    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("[video_server] axum::serve falló");
    });

    state
}

// ── Handler principal ─────────────────────────────────────────────────────────

async fn serve_file(State(_state): State<VideoServerState>, req: Request<Body>) -> Response<Body> {
    // La URL llega como /{file_path_encoded}
    // En macOS/Linux: /home/user/videos/pelicula.mp4
    // En Windows:     /C:/Users/user/Videos/pelicula.mp4  (con slash inicial extra)
    let raw_path = req.uri().path();

    // Quitar el slash inicial
    let stripped = raw_path.strip_prefix('/').unwrap_or(raw_path);

    // Decodificar percent-encoding
    let decoded = percent_decode(stripped);

    // En Windows la ruta llega como "C:/..." — está bien. En Unix como "/home/..." —
    // al quitar el slash inicial queda "home/...", hay que reponerlo.
    #[cfg(not(target_os = "windows"))]
    let file_path = format!("/{}", decoded);
    #[cfg(target_os = "windows")]
    let file_path = decoded;

    eprintln!("[video_server] GET {}", file_path);

    match serve_with_range(&file_path, req.headers()).await {
        Ok(resp) => resp,
        Err(status) => {
            eprintln!("[video_server] error {} para '{}'", status, file_path);
            Response::builder()
                .status(status)
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .body(Body::empty())
                .unwrap()
        }
    }
}

// ── Lógica de range requests ──────────────────────────────────────────────────

async fn serve_with_range(
    file_path: &str,
    headers: &HeaderMap,
) -> Result<Response<Body>, StatusCode> {
    let meta = tokio::fs::metadata(file_path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let file_size = meta.len();
    let mime = mime_for_path(file_path);

    // ETag: tamaño + mtime — permite 304 sin leer el archivo
    let mtime_secs = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let etag = format!("\"{}-{}\"", file_size, mtime_secs);

    // If-None-Match → 304
    if let Some(client_etag) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
    {
        if client_etag == etag {
            return Ok(Response::builder()
                .status(StatusCode::NOT_MODIFIED)
                .header(header::ETAG, &etag)
                .header(header::CACHE_CONTROL, "no-cache")
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .body(Body::empty())
                .unwrap());
        }
    }

    let range_header = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_owned());

    if let Some(range) = range_header {
        serve_range(file_path, file_size, mime, &range, &etag).await
    } else {
        serve_full(file_path, file_size, mime, &etag).await
    }
}

async fn serve_full(
    file_path: &str,
    file_size: u64,
    mime: &'static str,
    etag: &str,
) -> Result<Response<Body>, StatusCode> {
    let file = tokio::fs::File::open(file_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let stream = tokio_util::io::ReaderStream::new(file);
    let body = Body::from_stream(stream);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_LENGTH, file_size)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::ETAG, etag)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(body)
        .unwrap())
}

async fn serve_range(
    file_path: &str,
    file_size: u64,
    mime: &'static str,
    range: &str,
    etag: &str,
) -> Result<Response<Body>, StatusCode> {
    let range = range
        .strip_prefix("bytes=")
        .ok_or(StatusCode::RANGE_NOT_SATISFIABLE)?;

    let mut parts = range.splitn(2, '-');
    let start: u64 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or(StatusCode::RANGE_NOT_SATISFIABLE)?;
    let end: u64 = parts
        .next()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse().ok())
        .unwrap_or(file_size.saturating_sub(1));

    if start >= file_size || end >= file_size || start > end {
        return Ok(Response::builder()
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::CONTENT_RANGE, format!("bytes */{}", file_size))
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .body(Body::empty())
            .unwrap());
    }

    let chunk_len = end - start + 1;

    let mut file = tokio::fs::File::open(file_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    file.seek(std::io::SeekFrom::Start(start))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Leer solo el chunk pedido — streaming real sin cargar el archivo completo
    let limited = tokio::io::AsyncReadExt::take(file, chunk_len);
    let stream = tokio_util::io::ReaderStream::new(limited);
    let body = Body::from_stream(stream);

    Ok(Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_LENGTH, chunk_len)
        .header(
            header::CONTENT_RANGE,
            format!("bytes {}-{}/{}", start, end, file_size),
        )
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::ETAG, etag)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(body)
        .unwrap())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn mime_for_path(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        "webm" => "video/webm",
        "wmv" => "video/x-ms-wmv",
        "flv" => "video/x-flv",
        "3gp" => "video/3gpp",
        "ts" | "mts" => "video/mp2t",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        _ => "application/octet-stream",
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
