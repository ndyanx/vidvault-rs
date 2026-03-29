// Handles localvideo:// requests for thumbnail images.
//
// WebView2 (Windows) rewrites localvideo://local/path to
// https://localvideo.localhost/path, so both URI forms are accepted.
//
// Cache strategy:
//   - Thumbnails: Cache-Control: max-age=86400, immutable — they're written
//     once and never change.
//   - Videos (if served through this protocol): no-cache with ETag, allowing
//     the WebView to reuse a cached response after revalidation instead of
//     discarding it and reissuing the full range-request sequence.

use tauri::http::{header, Request, Response, StatusCode};
use tauri::UriSchemeResponder;
use tokio::io::AsyncReadExt;

pub async fn handle(request: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let uri_str = request.uri().to_string();
    eprintln!("[protocol] received URI: {}", uri_str);

    let response = serve(request).await.unwrap_or_else(|status| {
        eprintln!("[protocol] error status: {}", status);
        Response::builder()
            .status(status)
            // Include CORS header on error responses too; WebView2 may
            // otherwise silently suppress the error in the console.
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .body(
                status
                    .canonical_reason()
                    .unwrap_or("Error")
                    .as_bytes()
                    .to_vec(),
            )
            .unwrap()
    });

    eprintln!("[protocol] responding with status: {}", response.status());
    responder.respond(response);
}

async fn serve(request: Request<Vec<u8>>) -> Result<Response<Vec<u8>>, StatusCode> {
    let uri_str = request.uri().to_string();

    // Accept all URI forms emitted by different WebView backends
    let encoded_path = if let Some(p) = uri_str.strip_prefix("localvideo://local/") {
        p.to_owned()
    } else if let Some(p) = uri_str.strip_prefix("https://localvideo.localhost/") {
        p.to_owned()
    } else if let Some(p) = uri_str.strip_prefix("http://localvideo.localhost/") {
        p.to_owned()
    } else if let Some(p) = uri_str.strip_prefix("localvideo://localhost/") {
        p.to_owned()
    } else {
        eprintln!("[protocol] unrecognized URI prefix: {}", uri_str);
        return Err(StatusCode::BAD_REQUEST);
    };

    let decoded = percent_decode(&encoded_path);

    // WebView2 produces http://localvideo.localhost/C:/... where the path
    // component starts with /C:/. Strip the leading slash for Windows paths.
    #[cfg(target_os = "windows")]
    let file_path = if decoded.starts_with('/') {
        decoded[1..].to_string()
    } else {
        decoded
    };
    #[cfg(not(target_os = "windows"))]
    let file_path = decoded;

    eprintln!("[protocol] decoded file path: {}", file_path);

    let meta = tokio::fs::metadata(&file_path).await.map_err(|e| {
        eprintln!("[protocol] metadata error for '{}': {}", file_path, e);
        StatusCode::NOT_FOUND
    })?;

    let file_size = meta.len();
    let mime = mime_for_path(&file_path);
    eprintln!("[protocol] file_size={} mime={}", file_size, mime);

    // ETag for video files: identifies content by size + mtime without reading
    // the file. Allows 304 responses when the same video is reopened.
    let etag = if !mime.starts_with("image/") {
        let mtime_secs = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Some(format!("\"{}-{}\"", file_size, mtime_secs))
    } else {
        None
    };

    if let Some(ref etag_val) = etag {
        let client_etag = request
            .headers()
            .get(header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok());

        if client_etag == Some(etag_val.as_str()) {
            eprintln!("[protocol] 304 Not Modified for: {}", file_path);
            return Response::builder()
                .status(StatusCode::NOT_MODIFIED)
                .header(header::ETAG, etag_val)
                .header(header::CACHE_CONTROL, "no-cache")
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .body(vec![])
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    let range_header = request
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_owned());

    if let Some(range) = range_header {
        eprintln!("[protocol] range request: {}", range);
        serve_range(&file_path, file_size, mime, &range, etag.as_deref()).await
    } else {
        serve_full(&file_path, file_size, mime, etag.as_deref()).await
    }
}

async fn serve_full(
    file_path: &str,
    file_size: u64,
    mime: &'static str,
    etag: Option<&str>,
) -> Result<Response<Vec<u8>>, StatusCode> {
    let body = tokio::fs::read(file_path).await.map_err(|e| {
        eprintln!("[protocol] read error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Thumbnails are immutable once generated; use an aggressive cache.
    // Video files use no-cache + ETag so the WebView can revalidate rather
    // than discard and re-fetch.
    let cache_control = if mime.starts_with("image/") {
        "public, max-age=86400, immutable"
    } else {
        "no-cache"
    };

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_LENGTH, file_size)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, cache_control)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");

    if let Some(etag_val) = etag {
        builder = builder.header(header::ETAG, etag_val);
    }

    builder
        .body(body)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn serve_range(
    file_path: &str,
    file_size: u64,
    mime: &'static str,
    range: &str,
    etag: Option<&str>,
) -> Result<Response<Vec<u8>>, StatusCode> {
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
        return Response::builder()
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::CONTENT_RANGE, format!("bytes */{}", file_size))
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .body(vec![])
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
    }

    let chunk_size = end - start + 1;

    let mut file = tokio::fs::File::open(file_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    use tokio::io::AsyncSeekExt;
    file.seek(std::io::SeekFrom::Start(start))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut buf = vec![0u8; chunk_size as usize];
    file.read_exact(&mut buf)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut builder = Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header(header::CONTENT_TYPE, mime)
        // Content-Length must reflect the chunk size, not the total file size.
        // Some WebViews use this to determine when the chunk ends; a wrong value
        // can leave the <video> element stuck in a loading state.
        .header(header::CONTENT_LENGTH, chunk_size)
        .header(
            header::CONTENT_RANGE,
            format!("bytes {}-{}/{}", start, end, file_size),
        )
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");

    if let Some(etag_val) = etag {
        builder = builder.header(header::ETAG, etag_val);
    }

    builder
        .body(buf)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

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
