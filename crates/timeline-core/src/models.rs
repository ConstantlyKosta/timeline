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
    /// Folder the note was imported from ("" for notes created in the app), `/`-separated.
    #[serde(default)]
    pub folder: String,
    /// Original file path for imported notes (used to skip re-imports).
    #[serde(default)]
    pub source_path: Option<String>,
    /// SHA-256 of the imported file's bytes (used to skip duplicates).
    #[serde(default)]
    pub content_hash: Option<String>,
    /// File name without extension for imported notes; also resolves `[[wiki links]]`.
    #[serde(default)]
    pub slug: Option<String>,
    /// YAML front matter of imported notes as a JSON object (title, tags, aliases, custom keys...).
    #[serde(default)]
    pub properties: serde_json::Map<String, serde_json::Value>,
}

impl Note {
    /// A note with only the essentials set (everything imported-related empty).
    pub fn basic(id: &str, title: &str, body: &str, date: Ms, now: Ms) -> Note {
        Note { id: id.into(), title: title.into(), body: body.into(), date, created_at: now, updated_at: now, folder: String::new(), source_path: None, content_hash: None, slug: None, properties: Default::default() }
    }
    /// Lowercased tags from front matter (`tags: [a, b]`), without the leading `#`.
    pub fn front_matter_tags(&self) -> Vec<String> {
        let mut out = Vec::new();
        for key in ["tags", "tag"] {
            match self.properties.get(key) {
                Some(serde_json::Value::Array(a)) => out.extend(a.iter().filter_map(|v| v.as_str().map(String::from))),
                Some(serde_json::Value::String(s)) => out.extend(s.split([',', ' ']).map(String::from)),
                _ => {}
            }
        }
        let mut seen = std::collections::HashSet::new();
        out.into_iter().map(|t| t.trim().trim_start_matches('#').to_lowercase()).filter(|t| !t.is_empty() && seen.insert(t.clone())).collect()
    }
    /// Names other notes can link to this one by (title, file name, aliases), unnormalised.
    pub fn link_names(&self) -> Vec<String> {
        let mut v = vec![self.title.clone()];
        if let Some(s) = &self.slug {
            v.push(s.clone());
        }
        for key in ["aliases", "alias"] {
            match self.properties.get(key) {
                Some(serde_json::Value::Array(a)) => v.extend(a.iter().filter_map(|x| x.as_str().map(String::from))),
                Some(serde_json::Value::String(s)) => v.push(s.clone()),
                _ => {}
            }
        }
        v
    }
    /// Title fallback used when the body has no `# Heading`.
    pub fn title_fallback(&self) -> Option<String> {
        self.properties.get("title").and_then(|v| v.as_str()).map(String::from).or_else(|| self.slug.clone())
    }
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
    /// Markdown-stripped preview text (first meaningful lines, newline-separated, up to ~420 chars).
    #[serde(default)]
    pub preview: String,
    /// Media id of the first image in a note (for thumbnails).
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub folder: String,
}

impl TimelineEntry {
    pub fn end(&self) -> Ms {
        self.start + (self.duration * 1000.0).round() as Ms
    }
}
