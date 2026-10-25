//! Speech-to-text plumbing. The core knows nothing about whisper: an engine (the `timeline-asr`
//! crate, or a fake in tests) implements [`Asr`] and is plugged in with `Library::set_asr`.
//! Everything user-visible (settings, background job, storing the transcript) lives here so the
//! desktop app and the dev bridge behave identically.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

/// `(fraction 0..1, message)`.
pub type ProgressFn = Arc<dyn Fn(f64, &str) + Send + Sync>;
/// Returns true when the user asked to cancel.
pub type CancelFn = Arc<dyn Fn() -> bool + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AsrModel {
    pub id: String,
    pub label: String,
    pub description: String,
    pub size_bytes: u64,
    pub multilingual: bool,
    pub installed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AsrSegment {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AsrOutput {
    pub segments: Vec<AsrSegment>,
    /// ISO-639-1 code detected (or given).
    pub language: Option<String>,
    pub audio_seconds: f64,
    pub elapsed_seconds: f64,
}

pub trait Asr: Send + Sync {
    /// Human-readable engine description, e.g. "whisper.cpp 1.8 (CPU)".
    fn backend(&self) -> String;
    fn models(&self) -> Vec<AsrModel>;
    /// Downloads `model` into the engine's model folder, verifying its checksum.
    fn download(&self, model: &str, progress: ProgressFn, cancel: CancelFn) -> Result<()>;
    fn delete_model(&self, model: &str) -> Result<()>;
    /// `language`: `None` = auto-detect.
    fn transcribe(&self, audio: &Path, model: &str, language: Option<&str>, progress: ProgressFn, cancel: CancelFn) -> Result<AsrOutput>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AsrSettings {
    /// Model id, e.g. `base.en`.
    pub model: String,
    /// `auto` or an ISO code (`en`, `ru`, …).
    pub language: String,
    /// Transcribe new audio/video automatically after import (off by default).
    pub auto_transcribe: bool,
}

impl Default for AsrSettings {
    fn default() -> Self {
        AsrSettings { model: "base.en".into(), language: "auto".into(), auto_transcribe: false }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AsrStatus {
    pub available: bool,
    pub backend: String,
    pub models: Vec<AsrModel>,
    pub settings: AsrSettings,
}

/// Drops whisper's non-speech markers and empty segments.
pub fn clean_segments(raw: Vec<AsrSegment>) -> Vec<AsrSegment> {
    raw.into_iter()
        .map(|mut s| {
            s.text = s.text.split_whitespace().collect::<Vec<_>>().join(" ");
            s
        })
        .filter(|s| {
            let t = s.text.as_str();
            !t.is_empty() && !matches!(t, "[BLANK_AUDIO]" | "[ Silence ]" | "[silence]" | "(silence)" | "[MUSIC]" | "[Music]" | "[ Music ]")
        })
        .collect()
}

pub fn require<'a>(asr: &'a Option<Arc<dyn Asr>>) -> Result<&'a Arc<dyn Asr>> {
    asr.as_ref().ok_or_else(|| Error::InvalidArgument("speech-to-text is not available in this build".into()))
}


/// A deterministic stand-in engine, used by the unit tests and by the dev bridge in
/// `TIMELINE_ASR=fake` mode (so the UI can be tested without downloading a 150 MB model).
pub struct FakeAsr {
    installed: std::sync::Mutex<std::collections::HashSet<String>>,
    pub step_ms: u64,
}

impl FakeAsr {
    pub fn new(step_ms: u64) -> Self {
        FakeAsr { installed: Default::default(), step_ms }
    }
}

impl Asr for FakeAsr {
    fn backend(&self) -> String {
        "fake engine (tests)".into()
    }
    fn models(&self) -> Vec<AsrModel> {
        let inst = self.installed.lock().unwrap();
        [("base.en", false), ("base", true)]
            .iter()
            .map(|(id, multi)| AsrModel { id: id.to_string(), label: format!("Fake {id}"), description: "test model".into(), size_bytes: 1_000_000, multilingual: *multi, installed: inst.contains(*id) })
            .collect()
    }
    fn download(&self, model: &str, progress: ProgressFn, cancel: CancelFn) -> Result<()> {
        for i in 0..=4 {
            if cancel() {
                return Err(Error::Asr("cancelled".into()));
            }
            progress(i as f64 / 4.0, &format!("Downloading fake {model}"));
            std::thread::sleep(std::time::Duration::from_millis(self.step_ms));
        }
        self.installed.lock().unwrap().insert(model.to_string());
        Ok(())
    }
    fn delete_model(&self, model: &str) -> Result<()> {
        self.installed.lock().unwrap().remove(model);
        Ok(())
    }
    fn transcribe(&self, _audio: &Path, _model: &str, language: Option<&str>, progress: ProgressFn, cancel: CancelFn) -> Result<AsrOutput> {
        let lines = ["Welcome to the fake transcript.", "This second line proves segments are stored.", "[BLANK_AUDIO]", "And the third closes the recording."];
        let mut segments = Vec::new();
        for (i, l) in lines.iter().enumerate() {
            if cancel() {
                return Err(Error::Asr("cancelled".into()));
            }
            progress(i as f64 / lines.len() as f64, "Transcribing\u{2026}");
            std::thread::sleep(std::time::Duration::from_millis(self.step_ms));
            segments.push(AsrSegment { start: i as f64 * 2.0, end: i as f64 * 2.0 + 1.9, text: l.to_string() });
        }
        Ok(AsrOutput { segments, language: Some(language.unwrap_or("en").to_string()), audio_seconds: 8.0, elapsed_seconds: 0.5 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleaning_drops_markers_and_collapses_whitespace() {
        let v = clean_segments(vec![
            AsrSegment { start: 0.0, end: 1.0, text: "  hello \n  world ".into() },
            AsrSegment { start: 1.0, end: 2.0, text: "[BLANK_AUDIO]".into() },
            AsrSegment { start: 2.0, end: 3.0, text: "   ".into() },
        ]);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].text, "hello world");
    }
}
