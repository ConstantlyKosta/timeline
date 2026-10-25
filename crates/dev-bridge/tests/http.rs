use std::io::Read;
use std::sync::Arc;
use timeline_core::Library;

fn start() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let lib = Arc::new(Library::open(dir.path()).unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    let ui = dir.path().join("ui");
    std::fs::create_dir_all(ui.join("assets")).unwrap();
    std::fs::write(ui.join("index.html"), "<html>app</html>").unwrap();
    std::fs::write(ui.join("assets/a.js"), "1").unwrap();
    std::thread::spawn(move || {
        dev_bridge::serve(lib, dev_bridge::Config { static_dir: Some(ui) }, "127.0.0.1:0", move |a| tx.send(a).unwrap()).unwrap();
    });
    let addr = rx.recv().unwrap();
    (dir, format!("http://{addr}"))
}

#[test]
fn api_import_media_range_and_static() {
    let (_d, base) = start();
    // JSON commands incl. error mapping
    let r: serde_json::Value = ureq::post(&format!("{base}/api/bootstrap")).send_json(serde_json::json!({"tzOffsetMinutes": 0})).unwrap().into_json().unwrap();
    assert_eq!(r["seeded"], true);
    let notes: serde_json::Value = ureq::post(&format!("{base}/api/list_notes")).send_string("").unwrap().into_json().unwrap();
    assert_eq!(notes.as_array().unwrap().len(), 7);
    match ureq::post(&format!("{base}/api/get_note")).send_json(serde_json::json!({"id": "nope"})) {
        Err(ureq::Error::Status(400, resp)) => assert!(resp.into_json::<serde_json::Value>().unwrap()["error"].as_str().unwrap().starts_with("Not found")),
        other => panic!("expected 400, got {other:?}"),
    }
    // import raw bytes (WAV header is enough for kind detection; probe may give None)
    let out: serde_json::Value = ureq::post(&format!("{base}/import?name=My%20clip.wav&recordedAt=1234")).send_bytes(b"RIFFnotreallywav").unwrap().into_json().unwrap();
    assert_eq!(out["duplicate"], false);
    assert_eq!(out["item"]["title"], "My clip");
    assert_eq!(out["item"]["recordedAt"], 1234);
    let id = out["item"]["id"].as_str().unwrap().to_string();
    // full + ranged media
    let mut full = Vec::new();
    let resp = ureq::get(&format!("{base}/media/{id}")).call().unwrap();
    assert_eq!(resp.header("Content-Type"), Some("audio/wav"));
    resp.into_reader().read_to_end(&mut full).unwrap();
    assert_eq!(full, b"RIFFnotreallywav");
    let resp = ureq::get(&format!("{base}/media/{id}")).set("Range", "bytes=4-8").call().unwrap();
    assert_eq!(resp.status(), 206);
    assert_eq!(resp.header("Content-Range"), Some("bytes 4-8/16"));
    assert_eq!(resp.into_string().unwrap(), "notre");
    match ureq::get(&format!("{base}/media/{id}")).set("Range", "bytes=99-").call() {
        Err(ureq::Error::Status(416, _)) => {}
        other => panic!("expected 416, got {other:?}"),
    }
    assert!(matches!(ureq::get(&format!("{base}/media/nope")).call(), Err(ureq::Error::Status(404, _))));
    // static + SPA fallback + traversal guard
    assert_eq!(ureq::get(&format!("{base}/assets/a.js")).call().unwrap().into_string().unwrap(), "1");
    assert_eq!(ureq::get(&format!("{base}/some/route")).call().unwrap().into_string().unwrap(), "<html>app</html>");
    assert_eq!(ureq::get(&format!("{base}/../timeline.db")).call().unwrap().into_string().unwrap(), "<html>app</html>");
}
