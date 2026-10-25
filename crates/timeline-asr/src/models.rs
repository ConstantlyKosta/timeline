//! The downloadable model catalog and the checksum-verified downloader.
//! Models are ggml files from the whisper.cpp repository on Hugging Face. Nothing is bundled:
//! the app downloads a model once, on first use, into `<data dir>/models/`.

use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use timeline_core::asr::{AsrModel, CancelFn, ProgressFn};
use timeline_core::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct ModelSpec {
    pub id: String,
    pub label: String,
    pub description: String,
    pub multilingual: bool,
    pub file: String,
    pub size: u64,
    pub sha256: String,
    /// Full download URL.
    pub url: String,
}

pub const DEFAULT_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

/// `TIMELINE_ASR_MIRROR` swaps the host (for mirrors / tests); file names stay the same.
pub fn base_url() -> String {
    std::env::var("TIMELINE_ASR_MIRROR").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string()).trim_end_matches('/').to_string()
}

pub fn catalog() -> Vec<ModelSpec> {
    let base = base_url();
    // (id, label, description, multilingual, file, size in bytes, sha256)
    let rows: &[(&str, &str, &str, bool, &str, u64, &str)] = &[
        ("tiny", "Tiny (multilingual)", "Fastest, lowest accuracy. Good for a quick test.", true, "ggml-tiny.bin", 77691713, "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21"),
        ("base.en", "Base (English)", "Recommended default: fast and accurate for clear English speech.", false, "ggml-base.en.bin", 147964211, "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002"),
        ("base", "Base (multilingual)", "Same speed as Base English; handles Russian and ~100 other languages.", true, "ggml-base.bin", 147951465, "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe"),
        ("small.en", "Small (English)", "Noticeably more accurate than Base, about 3x slower.", false, "ggml-small.en.bin", 487614201, "c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d"),
        ("small", "Small (multilingual)", "Good accuracy for Russian and mixed-language notes.", true, "ggml-small.bin", 487601967, "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b"),
        ("large-v3-turbo-q5_0", "Large v3 Turbo (multilingual, compressed)", "Best quality for Russian/English; large download and slower on older machines.", true, "ggml-large-v3-turbo-q5_0.bin", 574041195, "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2"),
    ];
    rows.iter()
        .map(|(id, label, description, multilingual, file, size, sha)| ModelSpec {
            id: id.to_string(),
            label: label.to_string(),
            description: description.to_string(),
            multilingual: *multilingual,
            file: file.to_string(),
            size: *size,
            sha256: sha.to_string(),
            url: format!("{base}/{file}"),
        })
        .collect()
}

pub fn model_path(dir: &Path, spec: &ModelSpec) -> PathBuf {
    dir.join(&spec.file)
}

pub fn is_installed(dir: &Path, spec: &ModelSpec) -> bool {
    fs::metadata(model_path(dir, spec)).map(|m| m.len() == spec.size).unwrap_or(false)
}

pub fn to_info(dir: &Path, spec: &ModelSpec) -> AsrModel {
    AsrModel {
        id: spec.id.clone(),
        label: spec.label.clone(),
        description: spec.description.clone(),
        size_bytes: spec.size,
        multilingual: spec.multilingual,
        installed: is_installed(dir, spec),
    }
}

const OFFLINE: &str = "Could not reach the model server (huggingface.co). Check your internet connection and try again; the model is only downloaded once.";

fn mb(n: u64) -> String {
    format!("{:.0} MB", n as f64 / 1_048_576.0)
}

/// Downloads `spec` into `dir`: writes `<file>.part`, verifies size + SHA-256, then renames. A
/// failed or cancelled download leaves no half-file behind.
pub fn download(dir: &Path, spec: &ModelSpec, progress: &ProgressFn, cancel: &CancelFn) -> Result<()> {
    fs::create_dir_all(dir).map_err(Error::Io)?;
    let final_path = model_path(dir, spec);
    if is_installed(dir, spec) {
        progress(1.0, "Model already downloaded");
        return Ok(());
    }
    let part = dir.join(format!("{}.part", spec.file));
    match download_inner(spec, &part, progress, cancel) {
        Ok(()) => fs::rename(&part, &final_path).map_err(Error::Io),
        Err(e) => {
            let _ = fs::remove_file(&part);
            Err(e)
        }
    }
}

fn download_inner(spec: &ModelSpec, part: &Path, progress: &ProgressFn, cancel: &CancelFn) -> Result<()> {
    progress(0.0, &format!("Connecting to download {} ({})\u{2026}", spec.label, mb(spec.size)));
    let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).timeout_read(Duration::from_secs(60)).build();
    let resp = agent.get(&spec.url).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => Error::Asr(format!("the model server answered HTTP {code} for {}", spec.file)),
        ureq::Error::Transport(_) => Error::Asr(OFFLINE.to_string()),
    })?;
    let total = resp.header("content-length").and_then(|v| v.parse::<u64>().ok()).unwrap_or(spec.size);
    let mut reader = resp.into_reader();
    let mut file = File::create(part).map_err(Error::Io)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut done: u64 = 0;
    let mut last = -1.0f64;
    loop {
        if cancel() {
            return Err(Error::Asr("cancelled".into()));
        }
        let n = reader.read(&mut buf).map_err(|e| Error::Asr(format!("the download was interrupted ({e}). {OFFLINE}")))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n]).map_err(Error::Io)?;
        done += n as u64;
        let f = (done as f64 / total.max(1) as f64).min(1.0);
        if f - last >= 0.005 {
            last = f;
            progress(f * 0.98, &format!("Downloading {}: {} of {}", spec.label, mb(done), mb(total)));
        }
    }
    file.flush().map_err(Error::Io)?;
    drop(file);
    if done != spec.size {
        return Err(Error::Asr(format!("the downloaded file has the wrong size ({done} bytes, expected {}). Try again.", spec.size)));
    }
    progress(0.99, "Verifying checksum\u{2026}");
    let digest: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if digest != spec.sha256 {
        return Err(Error::Asr("the downloaded model failed its checksum check and was discarded. Try again.".into()));
    }
    progress(1.0, "Model ready");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn serve(body: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/m.bin", listener.local_addr().unwrap());
        let h = std::thread::spawn(move || {
            use std::io::{BufRead, BufReader};
            let (mut s, _) = listener.accept().unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut line = String::new();
            while r.read_line(&mut line).unwrap() > 2 {
                line.clear();
            }
            let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = s.write_all(&body);
        });
        (url, h)
    }

    fn spec_for(body: &[u8], url: &str, sha_override: Option<&str>) -> ModelSpec {
        let sha: String = Sha256::digest(body).iter().map(|b| format!("{b:02x}")).collect();
        ModelSpec { id: "t".into(), label: "Test".into(), description: String::new(), multilingual: false, file: "t.bin".into(), size: body.len() as u64, sha256: sha_override.map(String::from).unwrap_or(sha), url: url.into() }
    }

    fn noop() -> (ProgressFn, CancelFn) {
        (Arc::new(|_, _| {}), Arc::new(|| false))
    }

    #[test]
    fn catalog_is_sane() {
        let c = catalog();
        assert!(c.iter().any(|m| m.id == "base.en"));
        for m in &c {
            assert_eq!(m.sha256.len(), 64);
            assert!(m.url.ends_with(&m.file));
            assert_eq!(m.multilingual, !m.id.ends_with(".en"));
        }
    }

    #[test]
    fn downloads_verifies_and_reports_progress() {
        let body: Vec<u8> = (0..900_000u32).map(|i| (i % 251) as u8).collect();
        let (url, h) = serve(body.clone());
        let dir = tempfile::tempdir().unwrap();
        let spec = spec_for(&body, &url, None);
        let seen = Arc::new(std::sync::Mutex::new(Vec::<f64>::new()));
        let s2 = seen.clone();
        let progress: ProgressFn = Arc::new(move |p, _| s2.lock().unwrap().push(p));
        download(dir.path(), &spec, &progress, &noop().1).unwrap();
        h.join().unwrap();
        assert!(is_installed(dir.path(), &spec));
        assert!(!dir.path().join("t.bin.part").exists());
        let v = seen.lock().unwrap();
        assert!(v.windows(2).all(|w| w[0] <= w[1]) && *v.last().unwrap() == 1.0 && v.len() > 3);
    }

    #[test]
    fn checksum_mismatch_is_rejected_and_cleaned_up() {
        let body = vec![7u8; 10_000];
        let (url, h) = serve(body.clone());
        let dir = tempfile::tempdir().unwrap();
        let spec = spec_for(&body, &url, Some(&"0".repeat(64)));
        let (p, c) = noop();
        let err = download(dir.path(), &spec, &p, &c).unwrap_err().to_string();
        h.join().unwrap();
        assert!(err.contains("checksum"), "{err}");
        assert!(!is_installed(dir.path(), &spec));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn offline_gives_a_clear_message() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/m.bin", l.local_addr().unwrap());
        drop(l); // nothing listens any more
        let dir = tempfile::tempdir().unwrap();
        let spec = spec_for(b"x", &url, None);
        let (p, c) = noop();
        let err = download(dir.path(), &spec, &p, &c).unwrap_err().to_string();
        assert!(err.contains("internet connection"), "{err}");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn cancel_stops_and_removes_the_part_file() {
        let body = vec![1u8; 2_000_000];
        let (url, h) = serve(body.clone());
        let dir = tempfile::tempdir().unwrap();
        let spec = spec_for(&body, &url, None);
        let progress: ProgressFn = Arc::new(|_, _| {});
        let cancel: CancelFn = Arc::new(|| true);
        let err = download(dir.path(), &spec, &progress, &cancel).unwrap_err().to_string();
        let _ = h.join();
        assert!(err.contains("cancel"));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
