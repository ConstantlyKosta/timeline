//! Runs real whisper.cpp inference. Skipped unless `TIMELINE_TEST_MODELS` points at a folder
//! containing `ggml-base.en.bin` and `TIMELINE_TEST_AUDIO` at a speech clip, e.g.
//!   TIMELINE_TEST_MODELS=/models TIMELINE_TEST_AUDIO=jfk.wav cargo test -p timeline-asr --release --test real_model -- --nocapture

use std::sync::Arc;
use timeline_core::asr::Asr;

#[test]
fn transcribes_a_real_clip() {
    let (Ok(models), Ok(audio)) = (std::env::var("TIMELINE_TEST_MODELS"), std::env::var("TIMELINE_TEST_AUDIO")) else {
        eprintln!("skipped: TIMELINE_TEST_MODELS / TIMELINE_TEST_AUDIO not set");
        return;
    };
    let model = std::env::var("TIMELINE_TEST_MODEL").unwrap_or_else(|_| "base.en".into());
    let lang = std::env::var("TIMELINE_TEST_LANG").ok();
    let asr = timeline_asr::WhisperAsr::new(models);
    assert!(asr.models().iter().any(|m| m.id == model && m.installed), "model {model} not installed in TIMELINE_TEST_MODELS");
    let last = Arc::new(std::sync::Mutex::new(0.0f64));
    let l2 = last.clone();
    let out = asr
        .transcribe(std::path::Path::new(&audio), &model, lang.as_deref(), Arc::new(move |p, _| *l2.lock().unwrap() = p), Arc::new(|| false))
        .unwrap();
    for s in &out.segments {
        eprintln!("[{:6.2} -> {:6.2}] {}", s.start, s.end, s.text.trim());
    }
    eprintln!("language={:?} audio={:.2}s elapsed={:.2}s RTF={:.3}", out.language, out.audio_seconds, out.elapsed_seconds, out.elapsed_seconds / out.audio_seconds);
    assert!(!out.segments.is_empty());
    assert_eq!(*last.lock().unwrap(), 1.0);
    if let Ok(expect) = std::env::var("TIMELINE_TEST_EXPECT") {
        let all = out.segments.iter().map(|s| s.text.to_lowercase()).collect::<Vec<_>>().join(" ");
        assert!(all.contains(&expect.to_lowercase()), "expected {expect:?} in {all:?}");
    }
}
