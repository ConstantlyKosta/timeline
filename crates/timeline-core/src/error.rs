use serde::{Serialize, Serializer};

/// Every failure the core can report. Serialises to a plain message string so UI layers
/// (Tauri commands, the dev HTTP bridge) can forward it unchanged.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Unsupported file type: {0}")]
    UnsupportedFileType(String),
    #[error("Invalid transcript: {0}")]
    InvalidTranscript(String),
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),
    #[error("File missing: {0}")]
    FileMissing(String),
    #[error("Database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl Serialize for Error {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
