use serde::{Deserialize, Serialize};

/// Milliseconds since the Unix epoch (UTC). All persisted timestamps use this.
pub type Ms = i64;

pub const MS_PER_DAY: f64 = 86_400_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Audio,
    Video,
    Photo,
}

impl MediaKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            MediaKind::Audio => "audio",
            MediaKind::Video => "video",
            MediaKind::Photo => "photo",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "audio" => Some(Self::Audio),
            "video" => Some(Self::Video),
            "photo" => Some(Self::Photo),
            _ => None,
        }
    }
}

/// An imported media file. The UI refers to media by `id`; the bytes live inside the app's
/// data directory (content-addressed), never at a user-visible path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaItem {
    pub id: String,
    pub kind: MediaKind,
    pub title: String,
    /// When the thing happened (file modification date by default). Drives timeline placement.
    pub recorded_at: Ms,
    /// Seconds.
    pub duration: Option<f64>,
    pub sha256: String,
    pub file_size: i64,
    pub imported_at: Ms,
    /// True when the stored file can no longer be found.
    pub is_missing: bool,
    pub has_transcript: bool,
    pub mime: String,
    pub original_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegment {
    /// Index within the transcript (stable, 0-based).
    pub id: usize,
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub speaker: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub segments: Vec<TranscriptSegment>,
    pub language: Option<String>,
    /// True when timestamps were estimated (plain-text fallback without timestamps).
    pub has_synthesized_timing: bool,
}

impl Transcript {
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
}

/// A Markdown note pinned to a date.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub title: String,
    pub body: String,
    /// The date the note is pinned to on the timeline.
    pub date: Ms,
    pub created_at: Ms,
    pub updated_at: Ms,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Recording,
    Note,
    Photo,
    Video,
}

/// What the layout engine consumes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEntry {
    pub id: String,
    pub kind: EntryKind,
    pub start: Ms,
    /// Seconds. Zero for notes and photos.
    pub duration: f64,
    pub title: String,
    pub snippet: String,
    pub tags: Vec<String>,
}

impl TimelineEntry {
    pub fn end(&self) -> Ms {
        self.start + (self.duration * 1000.0).round() as Ms
    }
}
