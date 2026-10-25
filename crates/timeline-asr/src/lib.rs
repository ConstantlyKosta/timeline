//! Local speech-to-text for Timeline: audio decoding (symphonia), model download, and
//! whisper.cpp inference, exposed to the core through `timeline_core::asr::Asr`.

pub mod decode;
pub mod models;
mod transcribe;

pub use transcribe::WhisperAsr;
