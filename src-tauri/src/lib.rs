//! Tauri shell. All product logic lives in `timeline-core`; this file only wires three things:
//!  * `dispatch`      – one generic IPC command (JSON in / JSON out), same surface as the dev bridge's `/api/<cmd>`
//!  * `import_media`  – raw-bytes IPC (file bytes in the request body, name/time in headers)
//!  * `tlmedia://`    – a custom URI scheme that streams stored media with HTTP Range support (audio/video seeking)

use std::sync::Arc;
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{Manager, State};
use timeline_core::Library;

struct AppLib(Arc<Library>);

#[tauri::command]
fn dispatch(lib: State<'_, AppLib>, name: String, args: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
    timeline_core::dispatch::dispatch_shared(&lib.0, &name, args.unwrap_or_else(|| serde_json::json!({}))).map_err(|e| e.to_string())
}

#[tauri::command]
fn import_media(lib: State<'_, AppLib>, request: tauri::ipc::Request<'_>) -> Result<serde_json::Value, String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("expected a raw body".into());
    };
    let name = request
        .headers()
        .get("x-file-name")
        .and_then(|v| v.to_str().ok())
        .map(|v| percent_encoding::percent_decode_str(v).decode_utf8_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let recorded_at = request.headers().get("x-recorded-at").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<i64>().ok());
    let out = lib.0.import_media(&name, bytes, recorded_at).map_err(|e| e.to_string())?;
    serde_json::to_value(out).map_err(|e| e.to_string())
}

/// Imports media files straight from disk paths (native file dialog / drag-and-drop), so large
/// recordings never travel through the webview. Returns one entry per file.
#[tauri::command]
fn import_media_paths(lib: State<'_, AppLib>, paths: Vec<String>) -> Vec<serde_json::Value> {
    paths
        .into_iter()
        .map(|p| {
            let path = std::path::Path::new(&p);
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.clone());
            let recorded = std::fs::metadata(path).ok().and_then(|m| m.created().or_else(|_| m.modified()).ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64);
            match std::fs::read(path).map_err(|e| e.to_string()).and_then(|bytes| lib.0.import_media(&name, &bytes, recorded).map_err(|e| e.to_string())) {
                Ok(o) => serde_json::json!({ "name": name, "ok": serde_json::to_value(o).unwrap_or_default() }),
                Err(e) => serde_json::json!({ "name": name, "error": e }),
            }
        })
        .collect()
}

/// Writes text the user chose a destination for (native save dialog runs in the UI).
#[tauri::command]
fn save_text(path: String, text: String) -> Result<(), String> {
    std::fs::write(path, text).map_err(|e| e.to_string())
}

/// `bytes=a-b` / `bytes=a-` / `bytes=-n` against a file of `len` bytes (mirrors dev-bridge's parser, which has tests).
fn parse_range(h: &str, len: u64) -> Option<(u64, u64)> {
    let (a, b) = h.strip_prefix("bytes=")?.split_once('-')?;
    if len == 0 {
        return None;
    }
    let (start, end) = if a.is_empty() {
        (len.saturating_sub(b.parse().ok()?), len - 1)
    } else {
        let s: u64 = a.parse().ok()?;
        (s, if b.is_empty() { len - 1 } else { b.parse::<u64>().ok()?.min(len - 1) })
    };
    (start <= end && start < len).then_some((start, end))
}

fn media_response(lib: &Library, req: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let build = |status: StatusCode, mime: &str, body: Vec<u8>| {
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, mime)
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .header(header::ACCESS_CONTROL_EXPOSE_HEADERS, "Content-Range, Content-Length, Accept-Ranges")
            .body(body)
            .unwrap()
    };
    let id = req.uri().path().trim_start_matches('/');
    let Ok((path, mime)) = lib.media_file(id) else {
        return build(StatusCode::NOT_FOUND, "text/plain", b"not found".to_vec());
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return build(StatusCode::INTERNAL_SERVER_ERROR, "text/plain", b"read error".to_vec());
    };
    let len = bytes.len() as u64;
    match req.headers().get(header::RANGE).and_then(|v| v.to_str().ok()) {
        None => build(StatusCode::OK, &mime, bytes),
        Some(r) => match parse_range(r, len) {
            Some((s, e)) => {
                let mut resp = build(StatusCode::PARTIAL_CONTENT, &mime, bytes[s as usize..=e as usize].to_vec());
                resp.headers_mut().insert(header::CONTENT_RANGE, format!("bytes {s}-{e}/{len}").parse().unwrap());
                resp
            }
            None => {
                let mut resp = build(StatusCode::RANGE_NOT_SATISFIABLE, "text/plain", Vec::new());
                resp.headers_mut().insert(header::CONTENT_RANGE, format!("bytes */{len}").parse().unwrap());
                resp
            }
        },
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default().plugin(tauri_plugin_dialog::init());
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_window_state::Builder::default().build());
    }
    builder
        .setup(|app| {
            // Data lives in the OS app-data dir (never uploaded anywhere): SQLite DB + media/ + exports.
            let dir = std::env::var("TIMELINE_DATA_DIR").map(std::path::PathBuf::from).or_else(|_| app.path().app_data_dir())?;
            std::fs::create_dir_all(&dir)?;
            let lib = Library::open(&dir).map_err(|e| e.to_string())?;
            // Local speech-to-text (whisper.cpp). Models are downloaded on first use into <data dir>/models.
            lib.set_asr(Arc::new(timeline_asr::WhisperAsr::new(dir.join("models"))));
            app.manage(AppLib(Arc::new(lib)));
            Ok(())
        })
        .register_uri_scheme_protocol("tlmedia", |ctx, req| {
            let state = ctx.app_handle().state::<AppLib>();
            media_response(&state.0, &req)
        })
        .invoke_handler(tauri::generate_handler![dispatch, import_media, import_media_paths, save_text])
        .run(tauri::generate_context!())
        .expect("error while running Timeline");
}

#[cfg(test)]
mod tests {
    use super::parse_range;
    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-9", 100), Some((0, 9)));
        assert_eq!(parse_range("bytes=90-", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=-10", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=100-", 100), None);
        assert_eq!(parse_range("bytes=0-1", 0), None);
    }
}
