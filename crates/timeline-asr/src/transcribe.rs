//! whisper.cpp (via whisper-rs) transcription.

use crate::{decode, models};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use timeline_core::asr::{Asr, AsrModel, AsrOutput, AsrSegment, CancelFn, ProgressFn};
use timeline_core::error::{Error, Result};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub struct WhisperAsr {
    models_dir: PathBuf,
    specs: Vec<models::ModelSpec>,
    /// Loaded model, kept between jobs so a second transcription starts instantly.
    loaded: Mutex<Option<(String, Arc<WhisperContext>)>>,
    /// One transcription at a time: whisper already uses every core.
    run: Mutex<()>,
}

impl WhisperAsr {
    pub fn new(models_dir: impl Into<PathBuf>) -> Self {
        Self::with_catalog(models_dir, models::catalog())
    }

    pub fn with_catalog(models_dir: impl Into<PathBuf>, specs: Vec<models::ModelSpec>) -> Self {
        WhisperAsr { models_dir: models_dir.into(), specs, loaded: Mutex::new(None), run: Mutex::new(()) }
    }

    fn spec(&self, id: &str) -> Result<&models::ModelSpec> {
        self.specs.iter().find(|s| s.id == id).ok_or_else(|| Error::InvalidArgument(format!("unknown model {id:?}")))
    }

    fn context(&self, spec: &models::ModelSpec) -> Result<Arc<WhisperContext>> {
        let mut g = self.loaded.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((id, c)) = g.as_ref() {
            if *id == spec.id {
                return Ok(c.clone());
            }
        }
        *g = None; // free the previous model first
        let path = models::model_path(&self.models_dir, spec);
        let mut p = WhisperContextParameters::default();
        p.use_gpu(cfg!(feature = "gpu"));
        let c = WhisperContext::new_with_params(path.to_string_lossy().as_ref(), p)
            .map_err(|e| Error::Asr(format!("could not load the speech model ({e}). Delete it in settings and download it again.")))?;
        let c = Arc::new(c);
        *g = Some((spec.id.clone(), c.clone()));
        Ok(c)
    }
}

impl Asr for WhisperAsr {
    fn backend(&self) -> String {
        format!("whisper.cpp via whisper-rs ({})", if cfg!(feature = "gpu") { "GPU if available" } else { "CPU" })
    }

    fn models(&self) -> Vec<AsrModel> {
        self.specs.iter().map(|s| models::to_info(&self.models_dir, s)).collect()
    }

    fn download(&self, model: &str, progress: ProgressFn, cancel: CancelFn) -> Result<()> {
        let spec = self.spec(model)?.clone();
        models::download(&self.models_dir, &spec, &progress, &cancel)
    }

    fn delete_model(&self, model: &str) -> Result<()> {
        let spec = self.spec(model)?.clone();
        {
            let mut g = self.loaded.lock().unwrap_or_else(|e| e.into_inner());
            if g.as_ref().map(|(id, _)| *id == spec.id).unwrap_or(false) {
                *g = None;
            }
        }
        match std::fs::remove_file(models::model_path(&self.models_dir, &spec)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::Io(e)),
        }
    }

    fn transcribe(&self, audio: &Path, model: &str, language: Option<&str>, progress: ProgressFn, cancel: CancelFn) -> Result<AsrOutput> {
        let spec = self.spec(model)?.clone();
        if !models::is_installed(&self.models_dir, &spec) {
            return Err(Error::Asr(format!("the speech model \"{}\" is not downloaded yet", spec.label)));
        }
        if !spec.multilingual && matches!(language, Some(l) if l != "en") {
            return Err(Error::Asr(format!("\"{}\" only understands English. Pick a multilingual model for other languages.", spec.label)));
        }
        let _one_at_a_time = self.run.lock().unwrap_or_else(|e| e.into_inner());
        let started = Instant::now();

        progress(0.0, "Decoding audio\u{2026}");
        let p2 = progress.clone();
        let pcm = decode::decode_16k_mono(audio, &move |f| p2(f * 0.05, "Decoding audio\u{2026}"), &|| cancel())
            .map_err(Error::Asr)?;
        if cancel() {
            return Err(Error::Asr("cancelled".into()));
        }
        let audio_seconds = pcm.len() as f64 / decode::TARGET_RATE as f64;

        progress(0.05, &format!("Loading {}\u{2026}", spec.label));
        let ctx = self.context(&spec)?;
        let mut state = ctx.create_state().map_err(|e| Error::Asr(format!("could not start the speech engine ({e})")))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 8);
        params.set_n_threads(threads as i32);
        let lang = if spec.multilingual { language.unwrap_or("auto") } else { "en" };
        params.set_language(Some(lang));
        params.set_translate(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);
        let p3 = progress.clone();
        params.set_progress_callback_safe(move |pct: i32| {
            let f = (pct as f64 / 100.0).clamp(0.0, 1.0);
            p3(0.06 + 0.94 * f, &format!("Transcribing\u{2026} {pct}%"));
        });
        // whisper-rs 0.16's `set_abort_callback_safe` mixes up closure types (segfaults), so the
        // raw callback is used with a pointer to `cancel`, which outlives `state.full` below.
        unsafe extern "C" fn abort_trampoline(user_data: *mut std::ffi::c_void) -> bool {
            let cancel = &*(user_data as *const CancelFn);
            cancel()
        }
        unsafe {
            params.set_abort_callback(Some(abort_trampoline));
            params.set_abort_callback_user_data(&cancel as *const CancelFn as *mut std::ffi::c_void);
        }

        progress(0.06, "Transcribing\u{2026} 0%");
        let res = state.full(params, &pcm);
        if cancel() {
            return Err(Error::Asr("cancelled".into()));
        }
        res.map_err(|e| Error::Asr(format!("transcription failed ({e})")))?;

        let mut segments = Vec::new();
        for seg in state.as_iter() {
            let text = seg.to_str_lossy().map(|t| t.into_owned()).unwrap_or_default();
            segments.push(AsrSegment { start: seg.start_timestamp() as f64 / 100.0, end: seg.end_timestamp() as f64 / 100.0, text });
        }
        let detected = if lang == "auto" {
            whisper_rs::get_lang_str(state.full_lang_id_from_state()).map(str::to_string)
        } else {
            Some(lang.to_string())
        };
        progress(1.0, "Done");
        Ok(AsrOutput { segments, language: detected, audio_seconds, elapsed_seconds: started.elapsed().as_secs_f64() })
    }
}
