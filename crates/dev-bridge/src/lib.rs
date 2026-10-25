//! Dev HTTP bridge: the browser build of the UI talks to the *same* Rust core the desktop app uses.
//!
//! * `POST /api/<command>`      JSON body -> `timeline_core::dispatch::dispatch` (JSON in/out)
//! * `POST /import?name=&recordedAt=`  raw file bytes -> `Library::import_media`
//! * `GET  /media/<id>`          stored media bytes (HTTP Range supported, so audio seeking works)
//! * anything else               static files from `static_dir` (SPA fallback to index.html)

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tiny_http::{Header, Method, Request, Response, ResponseBox, Server, StatusCode};
use timeline_core::dispatch::dispatch_shared;
use timeline_core::Library;

pub struct Config {
    pub static_dir: Option<PathBuf>,
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap()
}

fn json_response(status: u16, body: String) -> ResponseBox {
    Response::from_string(body)
        .with_status_code(StatusCode(status))
        .with_header(header("Content-Type", "application/json"))
        .boxed()
}

fn error_response(status: u16, msg: &str) -> ResponseBox {
    json_response(status, serde_json::json!({ "error": msg }).to_string())
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| percent_encoding::percent_decode_str(v).decode_utf8_lossy().into_owned())
    })
}

/// Parses `bytes=a-b`, `bytes=a-`, `bytes=-n` against a file of `len` bytes.
pub fn parse_range(h: &str, len: u64) -> Option<(u64, u64)> {
    let spec = h.strip_prefix("bytes=")?;
    let (a, b) = spec.split_once('-')?;
    if len == 0 {
        return None;
    }
    let (start, end) = if a.is_empty() {
        let n: u64 = b.parse().ok()?;
        (len.saturating_sub(n), len - 1)
    } else {
        let s: u64 = a.parse().ok()?;
        let e: u64 = if b.is_empty() { len - 1 } else { b.parse().ok()? };
        (s, e.min(len - 1))
    };
    if start > end || start >= len { None } else { Some((start, end)) }
}

fn media_response(lib: &Library, id: &str, range: Option<String>) -> ResponseBox {
    let (path, mime) = match lib.media_file(id) {
        Ok(v) => v,
        Err(e) => return error_response(404, &e.to_string()),
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => return error_response(500, &e.to_string()),
    };
    let len = bytes.len() as u64;
    if let Some(r) = range {
        return match parse_range(&r, len) {
            Some((s, e)) => Response::from_data(bytes[s as usize..=e as usize].to_vec())
                .with_status_code(StatusCode(206))
                .with_header(header("Content-Type", &mime))
                .with_header(header("Accept-Ranges", "bytes"))
                .with_header(header("Content-Range", &format!("bytes {s}-{e}/{len}")))
                .boxed(),
            None => Response::from_string("")
                .with_status_code(StatusCode(416))
                .with_header(header("Content-Range", &format!("bytes */{len}")))
                .boxed(),
        };
    }
    Response::from_data(bytes).with_header(header("Content-Type", &mime)).with_header(header("Accept-Ranges", "bytes")).boxed()
}

fn content_type(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "svg" => "image/svg+xml",
        "json" => "application/json",
        "png" => "image/png",
        "woff2" => "font/woff2",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

fn static_response(dir: &Path, url_path: &str) -> ResponseBox {
    let rel = url_path.trim_start_matches('/');
    let mut p = dir.join(rel);
    // never escape the static dir
    let canon_dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let ok = |p: &Path| p.canonicalize().map(|c| c.starts_with(&canon_dir) && c.is_file()).unwrap_or(false);
    if rel.is_empty() || !ok(&p) {
        p = dir.join("index.html");
    }
    match std::fs::read(&p) {
        Ok(b) => Response::from_data(b).with_header(header("Content-Type", content_type(&p))).boxed(),
        Err(_) => error_response(404, "not found"),
    }
}

fn handle(lib: &Arc<Library>, cfg: &Config, req: &mut Request) -> ResponseBox {
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
    if *req.method() == Method::Options {
        return Response::from_string("").boxed();
    }
    if let Some(cmd) = path.strip_prefix("/api/") {
        if *req.method() != Method::Post {
            return error_response(405, "use POST");
        }
        let mut body = String::new();
        if req.as_reader().read_to_string(&mut body).is_err() {
            return error_response(400, "bad body");
        }
        let args: serde_json::Value = if body.trim().is_empty() { serde_json::json!({}) } else {
            match serde_json::from_str(&body) {
                Ok(v) => v,
                Err(e) => return error_response(400, &format!("bad JSON: {e}")),
            }
        };
        return match dispatch_shared(lib, cmd, args) {
            Ok(v) => json_response(200, v.to_string()),
            Err(e) => error_response(400, &e.to_string()),
        };
    }
    if path == "/import" && *req.method() == Method::Post {
        let name = query_param(query, "name").unwrap_or_else(|| "file".into());
        let recorded_at = query_param(query, "recordedAt").and_then(|v| v.parse::<i64>().ok());
        let mut bytes = Vec::new();
        if req.as_reader().read_to_end(&mut bytes).is_err() {
            return error_response(400, "bad body");
        }
        return match lib.import_media(&name, &bytes, recorded_at) {
            Ok(o) => json_response(200, serde_json::to_string(&o).unwrap()),
            Err(e) => error_response(400, &e.to_string()),
        };
    }
    if let Some(id) = path.strip_prefix("/media/") {
        let range = req.headers().iter().find(|h| h.field.equiv("Range")).map(|h| h.value.to_string());
        return media_response(lib, id, range);
    }
    match &cfg.static_dir {
        Some(d) if *req.method() == Method::Get => static_response(d, path),
        _ => error_response(404, "not found"),
    }
}

/// Binds and serves forever (blocking). Returns the bound address via `on_ready`.
pub fn serve(lib: Arc<Library>, cfg: Config, addr: &str, on_ready: impl FnOnce(std::net::SocketAddr)) -> Result<(), String> {
    let server = Server::http(addr).map_err(|e| e.to_string())?;
    if let Some(a) = server.server_addr().to_ip() {
        on_ready(a);
    }
    let cfg = Arc::new(cfg);
    for mut req in server.incoming_requests() {
        let (lib, cfg) = (lib.clone(), cfg.clone());
        std::thread::spawn(move || {
            let resp = handle(&lib, &cfg, &mut req);
            let resp = resp
                .with_header(header("Access-Control-Allow-Origin", "*"))
                .with_header(header("Access-Control-Allow-Headers", "*"))
                .with_header(header("Access-Control-Allow-Methods", "GET, POST, OPTIONS"));
            let _ = req.respond(resp);
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-9", 100), Some((0, 9)));
        assert_eq!(parse_range("bytes=90-", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=-10", 100), Some((90, 99)));
        assert_eq!(parse_range("bytes=50-500", 100), Some((50, 99)));
        assert_eq!(parse_range("bytes=100-", 100), None);
        assert_eq!(parse_range("bytes=9-3", 100), None);
        assert_eq!(parse_range("items=1-2", 100), None);
        assert_eq!(parse_range("bytes=0-1", 0), None);
    }
}
