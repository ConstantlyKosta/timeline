//! The `Library` service: the one object the UI layers (Tauri commands, dev HTTP bridge) talk to.
//! It owns the SQLite connection, the media directory and the in-memory search index.

use crate::db;
use crate::error::{Error, Result};
use crate::graph::{self, Backlink, Graph};
use crate::markdown::{self, normalize_title, ParsedNote, TaskItem, Heading};
use crate::media;
use crate::models::*;
use crate::search::{SearchDocKind, SearchDocument, SearchEngine, SearchHit};
use crate::transcript;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteSummary {
    pub id: String,
    pub title: String,
    pub date: Ms,
    pub updated_at: Ms,
    pub snippet: String,
    pub tags: Vec<String>,
    pub folder: String,
    pub task_count: usize,
    pub done_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteRef {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagCount {
    pub tag: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutgoingLink {
    pub target: String,
    pub alias: Option<String>,
    pub note_id: Option<String>,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteContext {
    pub note: Note,
    pub backlinks: Vec<Backlink>,
    pub outgoing: Vec<OutgoingLink>,
    pub related: Vec<SearchHit>,
    pub tags: Vec<String>,
    pub tasks: Vec<TaskItem>,
    pub headings: Vec<Heading>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcome {
    pub item: MediaItem,
    /// True when identical bytes were already in the library (the existing item is returned).
    pub duplicate: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphViewRow {
    pub name: String,
    pub json: serde_json::Value,
    pub updated_at: Ms,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodePos {
    pub id: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub notes: usize,
    pub recordings: usize,
    pub videos: usize,
    pub photos: usize,
    pub transcripts: usize,
    pub indexed_documents: usize,
    pub embedding: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub dir: String,
    pub count: usize,
}

struct Inner {
    conn: Connection,
    search: SearchEngine,
    parsed: HashMap<String, ParsedNote>,
}

pub struct Library {
    data_dir: PathBuf,
    inner: Mutex<Inner>,
    clock: Box<dyn Fn() -> Ms + Send + Sync>,
    jobs: crate::jobs::Jobs,
}

pub fn system_now_ms() -> Ms {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as Ms).unwrap_or(0)
}

fn seg_doc_id(media_id: &str, idx: usize) -> String {
    format!("seg:{media_id}:{idx}")
}

impl Library {
    /// Opens (creating if needed) a library rooted at `data_dir` (DB + `media/` + `export/`).
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_clock(data_dir, Box::new(system_now_ms))
    }

    pub fn open_with_clock(data_dir: impl AsRef<Path>, clock: Box<dyn Fn() -> Ms + Send + Sync>) -> Result<Self> {
        let data_dir = data_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(data_dir.join("media"))?;
        let conn = db::open(&data_dir.join("timeline.db"))?;
        let lib = Library { data_dir, inner: Mutex::new(Inner { conn, search: SearchEngine::new(), parsed: HashMap::new() }), clock, jobs: Default::default() };
        lib.rebuild_index()?;
        Ok(lib)
    }

    pub fn jobs(&self) -> &crate::jobs::Jobs {
        &self.jobs
    }

    /// Inserts many notes in one transaction and indexes them (used by the Markdown importer).
    pub(crate) fn insert_notes_bulk(&self, notes: &[Note]) -> Result<()> {
        let mut g = self.lock();
        {
            let tx = g.conn.transaction()?;
            for n in notes {
                db::insert_note(&tx, n)?;
            }
            tx.commit()?;
        }
        for n in notes {
            Self::index_note(&mut g, n);
        }
        Ok(())
    }

    pub(crate) fn notes_all(&self) -> Result<Vec<Note>> {
        db::list_notes(&self.lock().conn)
    }

    pub(crate) fn imported_keys(&self) -> Result<(std::collections::HashSet<String>, std::collections::HashSet<String>)> {
        db::imported_keys(&self.lock().conn)
    }

    /// Stores an image that an imported note refers to. It is hidden from the media lists/timeline
    /// (it lives inside the note) unless the same file already existed as ordinary media.
    pub(crate) fn import_attachment(&self, file_name: &str, bytes: &[u8], recorded_at: Ms) -> Result<String> {
        let out = self.import_media_inner(file_name, bytes, Some(recorded_at), None, None)?;
        if !out.duplicate {
            db::set_media_attachment(&self.lock().conn, &out.item.id, true)?;
        }
        Ok(out.item.id)
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
    pub fn now(&self) -> Ms {
        (self.clock)()
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Rebuilds the in-memory search index and parsed-note cache from the database.
    pub fn rebuild_index(&self) -> Result<()> {
        let mut g = self.lock();
        g.search = SearchEngine::new();
        g.parsed.clear();
        for n in db::list_notes(&g.conn)? {
            Self::index_note(&mut g, &n);
        }
        for row in db::list_media(&g.conn)? {
            let t = db::load_transcript(&g.conn, &row.item.id)?;
            Self::index_media(&mut g, &row.item, t.as_ref());
        }
        Ok(())
    }

    fn index_note(g: &mut Inner, n: &Note) {
        let mut p = markdown::parse(&n.body);
        for t in n.front_matter_tags() {
            if !p.tags.contains(&t) {
                p.tags.push(t);
            }
        }
        g.search.upsert(SearchDocument { id: n.id.clone(), kind: SearchDocKind::Note, owner_id: n.id.clone(), title: n.title.clone(), text: p.plain_text.clone(), date: n.date, time: None });
        g.parsed.insert(n.id.clone(), p);
    }

    fn index_media(g: &mut Inner, m: &MediaItem, t: Option<&Transcript>) {
        g.search.remove_owner(&m.id);
        let kind = if m.kind == MediaKind::Audio { SearchDocKind::Recording } else { SearchDocKind::Recording };
        g.search.upsert(SearchDocument { id: m.id.clone(), kind, owner_id: m.id.clone(), title: m.title.clone(), text: m.original_name.clone(), date: m.recorded_at, time: None });
        if let Some(t) = t {
            for s in &t.segments {
                g.search.upsert(SearchDocument {
                    id: seg_doc_id(&m.id, s.id),
                    kind: SearchDocKind::TranscriptSegment,
                    owner_id: m.id.clone(),
                    title: m.title.clone(),
                    text: s.text.clone(),
                    date: m.recorded_at + (s.start * 1000.0) as Ms,
                    time: Some(s.start),
                });
            }
        }
    }

    // ------------------------------------------------------------- timeline

    pub fn timeline_entries(&self) -> Result<Vec<TimelineEntry>> {
        let g = self.lock();
        let mut out = Vec::new();
        for n in db::list_notes(&g.conn)? {
            let tags = g.parsed.get(&n.id).map(|p| p.tags.clone()).unwrap_or_default();
            out.push(TimelineEntry {
                id: n.id.clone(),
                kind: EntryKind::Note,
                start: n.date,
                duration: 0.0,
                title: n.title.clone(),
                snippet: markdown::snippet(&n.body, 140),
                tags,
                preview: markdown::preview(&n.body, 420),
                image: markdown::first_image(&n.body),
                folder: n.folder.clone(),
            });
        }
        let hidden = db::attachment_ids(&g.conn)?;
        for r in db::list_media(&g.conn)? {
            if hidden.contains(&r.item.id) {
                continue;
            }
            let m = r.item;
            let (kind, duration) = match m.kind {
                MediaKind::Audio => (EntryKind::Recording, m.duration.unwrap_or(0.0)),
                MediaKind::Video => (EntryKind::Video, m.duration.unwrap_or(0.0)),
                MediaKind::Photo => (EntryKind::Photo, 0.0),
            };
            let snippet = match db::load_transcript(&g.conn, &m.id)? {
                Some(t) => t.segments.iter().take(2).map(|s| s.text.as_str()).collect::<Vec<_>>().join(" "),
                None => String::new(),
            };
            let image = (m.kind == MediaKind::Photo).then(|| m.id.clone());
            out.push(TimelineEntry { id: m.id, kind, start: m.recorded_at, duration, title: m.title, snippet: snippet.clone(), tags: vec![], preview: snippet, image, folder: String::new() });
        }
        out.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.id.cmp(&b.id)));
        Ok(out)
    }

    // ---------------------------------------------------------------- notes

    pub fn list_notes(&self) -> Result<Vec<NoteSummary>> {
        let g = self.lock();
        let mut notes = db::list_notes(&g.conn)?;
        notes.sort_by(|a, b| b.date.cmp(&a.date).then_with(|| a.id.cmp(&b.id)));
        Ok(notes
            .into_iter()
            .map(|n| {
                let p = g.parsed.get(&n.id);
                NoteSummary {
                    snippet: markdown::snippet(&n.body, 140),
                    tags: p.map(|p| p.tags.clone()).unwrap_or_default(),
                    folder: n.folder.clone(),
                    task_count: p.map(|p| p.tasks.len()).unwrap_or(0),
                    done_count: p.map(|p| p.tasks.iter().filter(|t| t.is_done).count()).unwrap_or(0),
                    id: n.id,
                    title: n.title,
                    date: n.date,
                    updated_at: n.updated_at,
                }
            })
            .collect())
    }

    pub fn get_note(&self, id: &str) -> Result<Note> {
        let g = self.lock();
        db::get_note(&g.conn, id)?.ok_or_else(|| Error::NotFound(format!("note {id}")))
    }

    pub fn note_titles(&self) -> Result<Vec<NoteRef>> {
        let g = self.lock();
        let mut v: Vec<NoteRef> = db::list_notes(&g.conn)?.into_iter().map(|n| NoteRef { id: n.id, title: n.title }).collect();
        v.sort_by(|a, b| normalize_title(&a.title).cmp(&normalize_title(&b.title)));
        Ok(v)
    }

    pub fn list_tags(&self) -> Result<Vec<TagCount>> {
        let g = self.lock();
        let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
        for p in g.parsed.values() {
            for t in &p.tags {
                *counts.entry(t.clone()).or_default() += 1;
            }
        }
        let mut v: Vec<TagCount> = counts.into_iter().map(|(tag, count)| TagCount { tag, count }).collect();
        v.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.tag.cmp(&b.tag)));
        Ok(v)
    }

    /// Creates a note from Markdown. `date` defaults to now.
    pub fn create_note(&self, body: &str, date: Option<Ms>) -> Result<Note> {
        self.create_note_with_id(&uuid::Uuid::new_v4().to_string(), body, date)
    }

    pub(crate) fn create_note_with_id(&self, id: &str, body: &str, date: Option<Ms>) -> Result<Note> {
        let now = self.now();
        let n = Note::basic(id, &markdown::derive_title(body), body, date.unwrap_or(now), now);
        let mut g = self.lock();
        db::insert_note(&g.conn, &n)?;
        Self::index_note(&mut g, &n);
        Ok(n)
    }

    pub fn update_note(&self, id: &str, body: Option<&str>, date: Option<Ms>) -> Result<Note> {
        let now = self.now();
        let mut g = self.lock();
        let mut n = db::get_note(&g.conn, id)?.ok_or_else(|| Error::NotFound(format!("note {id}")))?;
        if let Some(b) = body {
            n.body = b.to_string();
            n.title = markdown::derive_title_with(b, n.title_fallback().as_deref());
        }
        if let Some(d) = date {
            n.date = d;
        }
        n.updated_at = now;
        db::update_note(&g.conn, &n)?;
        Self::index_note(&mut g, &n);
        Ok(n)
    }

    pub fn delete_note(&self, id: &str) -> Result<()> {
        let mut g = self.lock();
        if db::delete_note(&g.conn, id)? == 0 {
            return Err(Error::NotFound(format!("note {id}")));
        }
        g.search.remove(id);
        g.parsed.remove(id);
        Ok(())
    }

    pub fn toggle_task(&self, id: &str, line: usize) -> Result<Note> {
        let body = self.get_note(id)?.body;
        let new = markdown::toggle_task(&body, line).ok_or_else(|| Error::InvalidArgument(format!("line {line} is not a task")))?;
        self.update_note(id, Some(&new), None)
    }

    /// Finds the note with this title (case/diacritic-insensitive) or creates `# Title`.
    pub fn open_note_by_title(&self, title: &str, date: Option<Ms>) -> Result<Note> {
        let title = title.trim();
        if title.is_empty() {
            return Err(Error::InvalidArgument("empty title".into()));
        }
        {
            let g = self.lock();
            let key = graph::target_key(title);
            if let Some(n) = db::list_notes(&g.conn)?.into_iter().find(|n| n.link_names().iter().any(|x| graph::target_key(x) == key)) {
                return Ok(n);
            }
        }
        self.create_note(&format!("# {title}\n\n"), date)
    }

    pub fn note_context(&self, id: &str) -> Result<NoteContext> {
        let g = self.lock();
        let note = db::get_note(&g.conn, id)?.ok_or_else(|| Error::NotFound(format!("note {id}")))?;
        let all = db::list_notes(&g.conn)?;
        let p = g.parsed.get(id).cloned().unwrap_or_else(|| markdown::parse(&note.body));
        let my_keys: std::collections::HashSet<String> = note.link_names().iter().map(|x| graph::target_key(x)).collect();
        let mut backlinks = Vec::new();
        for other in &all {
            if other.id == id {
                continue;
            }
            let Some(op) = g.parsed.get(&other.id) else { continue };
            if let Some(l) = op.links.iter().find(|l| my_keys.contains(&graph::target_key(&l.target))) {
                let line_text = other.body.lines().nth(l.line).unwrap_or("");
                backlinks.push(Backlink { note_id: other.id.clone(), title: other.title.clone(), snippet: markdown::snippet(line_text, 140), line: l.line });
            }
        }
        backlinks.sort_by(|a, b| normalize_title(&a.title).cmp(&normalize_title(&b.title)));
        let outgoing = p
            .links
            .iter()
            .map(|l| OutgoingLink { target: l.target.clone(), alias: l.alias.clone(), note_id: graph::resolve_target(&all, &l.target), line: l.line })
            .collect();
        let mut seen = std::collections::HashSet::new();
        let related = g
            .search
            .related(id, None, 12)
            .into_iter()
            .filter(|h| h.owner_id != id && seen.insert(h.owner_id.clone()))
            .take(5)
            .collect();
        Ok(NoteContext { note, backlinks, outgoing, related, tags: p.tags.clone(), tasks: p.tasks.clone(), headings: p.headings.clone() })
    }

    /// Rich graph description for the interactive mind map (layout happens in the UI).
    pub fn graph_data(&self) -> Result<graph::GraphData> {
        let g = self.lock();
        let notes = db::list_notes(&g.conn)?;
        let media: HashMap<String, (String, bool)> = db::list_media(&g.conn)?
            .into_iter()
            .map(|m| (m.item.id.clone(), (m.item.title.clone(), m.item.kind == MediaKind::Photo)))
            .collect();
        Ok(graph::build_data(&notes, &g.parsed, &media))
    }

    pub fn graph_views(&self) -> Result<Vec<GraphViewRow>> {
        let g = self.lock();
        Ok(db::graph_views(&g.conn)?
            .into_iter()
            .map(|(name, json, updated_at)| GraphViewRow { name, json: serde_json::from_str(&json).unwrap_or(serde_json::Value::Null), updated_at })
            .collect())
    }

    pub fn save_graph_view(&self, name: &str, json: &serde_json::Value) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::InvalidArgument("a view needs a name".into()));
        }
        let g = self.lock();
        db::save_graph_view(&g.conn, name, &json.to_string(), self.now())?;
        Ok(())
    }

    pub fn delete_graph_view(&self, name: &str) -> Result<()> {
        db::delete_graph_view(&self.lock().conn, name)?;
        Ok(())
    }

    pub fn graph_positions(&self) -> Result<Vec<NodePos>> {
        Ok(db::node_positions(&self.lock().conn)?.into_iter().map(|(id, x, y)| NodePos { id, x, y }).collect())
    }

    pub fn save_graph_positions(&self, pos: &[NodePos]) -> Result<()> {
        let rows: Vec<(String, f64, f64)> = pos.iter().map(|p| (p.id.clone(), p.x, p.y)).collect();
        db::replace_node_positions(&mut self.lock().conn, &rows)
    }

    pub fn graph(&self) -> Result<Graph> {
        let g = self.lock();
        let notes = db::list_notes(&g.conn)?;
        Ok(graph::build(&notes, &g.parsed))
    }

    // --------------------------------------------------------------- search

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        Ok(self.lock().search.search(query, limit))
    }

    // ---------------------------------------------------------------- media

    fn media_path_for(&self, sha: &str, ext: &str) -> PathBuf {
        let name = if ext.is_empty() { sha.to_string() } else { format!("{sha}.{ext}") };
        self.data_dir.join("media").join(name)
    }

    /// Copies bytes into the library (content-addressed) and records the item. Identical bytes
    /// already imported return the existing item with `duplicate = true`.
    pub fn import_media(&self, file_name: &str, bytes: &[u8], recorded_at: Option<Ms>) -> Result<ImportOutcome> {
        self.import_media_inner(file_name, bytes, recorded_at, None, None)
    }

    pub(crate) fn import_media_inner(&self, file_name: &str, bytes: &[u8], recorded_at: Option<Ms>, title: Option<&str>, id: Option<&str>) -> Result<ImportOutcome> {
        let ext = media::extension(file_name);
        let kind = media::kind_for_extension(&ext).ok_or_else(|| Error::UnsupportedFileType(if ext.is_empty() { file_name.to_string() } else { format!(".{ext}") }))?;
        if bytes.is_empty() {
            return Err(Error::InvalidArgument("file is empty".into()));
        }
        let sha = hex(&Sha256::digest(bytes));
        {
            let g = self.lock();
            if let Some(existing) = db::find_media_by_sha(&g.conn, &sha)? {
                let mut item = existing.item;
                item.is_missing = !self.media_path_for(&sha, &existing.ext).exists();
                return Ok(ImportOutcome { item, duplicate: true });
            }
        }
        let path = self.media_path_for(&sha, &ext);
        let tmp = path.with_extension("part");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, &path)?;
        let duration = if kind == MediaKind::Photo { None } else { media::probe_duration(&ext, bytes) };
        let now = self.now();
        let item = MediaItem {
            id: id.map(String::from).unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            kind,
            title: title.map(String::from).unwrap_or_else(|| media::title_from_file_name(file_name)),
            recorded_at: recorded_at.unwrap_or(now),
            duration,
            sha256: sha,
            file_size: bytes.len() as i64,
            imported_at: now,
            is_missing: false,
            has_transcript: false,
            mime: media::mime_for_extension(&ext).to_string(),
            original_name: file_name.rsplit(['/', '\\']).next().unwrap_or(file_name).to_string(),
        };
        let mut g = self.lock();
        db::insert_media(&g.conn, &item, &ext)?;
        Self::index_media(&mut g, &item, None);
        Ok(ImportOutcome { item, duplicate: false })
    }

    pub fn list_media(&self) -> Result<Vec<MediaItem>> {
        let g = self.lock();
        let hidden = db::attachment_ids(&g.conn)?;
        Ok(db::list_media(&g.conn)?
            .into_iter()
            .filter(|r| !hidden.contains(&r.item.id))
            .map(|r| {
                let mut i = r.item;
                i.is_missing = !self.media_path_for(&i.sha256, &r.ext).exists();
                i
            })
            .collect())
    }

    pub fn get_media(&self, id: &str) -> Result<MediaItem> {
        let g = self.lock();
        let r = db::get_media(&g.conn, id)?.ok_or_else(|| Error::NotFound(format!("media {id}")))?;
        let mut i = r.item;
        i.is_missing = !self.media_path_for(&i.sha256, &r.ext).exists();
        Ok(i)
    }

    /// (path on disk, mime type) of a stored media file.
    pub fn media_file(&self, id: &str) -> Result<(PathBuf, String)> {
        let g = self.lock();
        let r = db::get_media(&g.conn, id)?.ok_or_else(|| Error::NotFound(format!("media {id}")))?;
        let p = self.media_path_for(&r.item.sha256, &r.ext);
        if !p.exists() {
            return Err(Error::FileMissing(r.item.title));
        }
        Ok((p, r.item.mime))
    }

    pub fn media_bytes(&self, id: &str) -> Result<(Vec<u8>, String)> {
        let (p, mime) = self.media_file(id)?;
        Ok((std::fs::read(p)?, mime))
    }

    pub fn update_media(&self, id: &str, title: Option<&str>, recorded_at: Option<Ms>) -> Result<MediaItem> {
        {
            let mut g = self.lock();
            if db::get_media(&g.conn, id)?.is_none() {
                return Err(Error::NotFound(format!("media {id}")));
            }
            db::update_media(&g.conn, id, title.map(str::trim).filter(|t| !t.is_empty()), recorded_at)?;
            let item = db::get_media(&g.conn, id)?.unwrap().item;
            let t = db::load_transcript(&g.conn, id)?;
            Self::index_media(&mut g, &item, t.as_ref());
        }
        self.get_media(id)
    }

    pub fn delete_media(&self, id: &str) -> Result<()> {
        let mut g = self.lock();
        let r = db::get_media(&g.conn, id)?.ok_or_else(|| Error::NotFound(format!("media {id}")))?;
        db::delete_media(&g.conn, id)?;
        g.search.remove_owner(id);
        let _ = std::fs::remove_file(self.media_path_for(&r.item.sha256, &r.ext));
        Ok(())
    }

    // ----------------------------------------------------------- transcripts

    pub fn attach_transcript(&self, media_id: &str, file_name: &str, text: &str) -> Result<Transcript> {
        let item = self.get_media(media_id)?;
        if item.kind == MediaKind::Photo {
            return Err(Error::InvalidArgument("photos cannot have transcripts".into()));
        }
        let t = transcript::parse(file_name, text, item.duration)?;
        self.store_transcript(media_id, &t)?;
        Ok(t)
    }

    pub(crate) fn store_transcript(&self, media_id: &str, t: &Transcript) -> Result<()> {
        let mut g = self.lock();
        db::save_transcript(&mut g.conn, media_id, t)?;
        let item = db::get_media(&g.conn, media_id)?.ok_or_else(|| Error::NotFound(format!("media {media_id}")))?.item;
        Self::index_media(&mut g, &item, Some(t));
        Ok(())
    }

    pub fn get_transcript(&self, media_id: &str) -> Result<Option<Transcript>> {
        let g = self.lock();
        db::load_transcript(&g.conn, media_id)
    }

    pub fn remove_transcript(&self, media_id: &str) -> Result<()> {
        let mut g = self.lock();
        db::delete_transcript(&g.conn, media_id)?;
        if let Some(r) = db::get_media(&g.conn, media_id)? {
            Self::index_media(&mut g, &r.item, None);
        }
        Ok(())
    }

    pub fn export_transcript_text(&self, media_id: &str) -> Result<String> {
        let item = self.get_media(media_id)?;
        let t = self.get_transcript(media_id)?.ok_or_else(|| Error::NotFound("transcript".into()))?;
        Ok(transcript::export_text(&item.title, &t))
    }

    // --------------------------------------------------------------- export

    /// Writes every note to `<data_dir>/export/<title>.md` with a small YAML front matter.
    pub fn export_markdown(&self) -> Result<ExportResult> {
        let dir = self.data_dir.join("export");
        std::fs::create_dir_all(&dir)?;
        let notes = { db::list_notes(&self.lock().conn)? };
        let mut used: std::collections::HashSet<String> = Default::default();
        for n in &notes {
            let base = sanitize_file_name(&n.title);
            let mut name = base.clone();
            let mut i = 2;
            while !used.insert(name.to_lowercase()) {
                name = format!("{base} {i}");
                i += 1;
            }
            let date = chrono::DateTime::from_timestamp_millis(n.date).map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string()).unwrap_or_default();
            let mut props = n.properties.clone();
            props.insert("date".into(), serde_json::Value::String(date));
            let content = format!("{}{}\n", crate::import::front_matter_text(&props), n.body.trim_end());
            std::fs::write(dir.join(format!("{name}.md")), content)?;
        }
        Ok(ExportResult { dir: dir.to_string_lossy().into_owned(), count: notes.len() })
    }

    pub fn stats(&self) -> Result<Stats> {
        let g = self.lock();
        let hidden = db::attachment_ids(&g.conn)?;
        let media: Vec<_> = db::list_media(&g.conn)?.into_iter().filter(|m| !hidden.contains(&m.item.id)).collect();
        let count = |k: MediaKind| media.iter().filter(|m| m.item.kind == k).count();
        Ok(Stats {
            notes: db::list_notes(&g.conn)?.len(),
            recordings: count(MediaKind::Audio),
            videos: count(MediaKind::Video),
            photos: count(MediaKind::Photo),
            transcripts: media.iter().filter(|m| m.item.has_transcript).count(),
            indexed_documents: g.search.len(),
            embedding: g.search.provider_id(),
        })
    }

    pub fn is_empty(&self) -> Result<bool> {
        let g = self.lock();
        Ok(db::list_notes(&g.conn)?.is_empty() && db::list_media(&g.conn)?.is_empty())
    }

    pub(crate) fn meta(&self, key: &str) -> Result<Option<String>> {
        db::get_meta(&self.lock().conn, key)
    }
    pub(crate) fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        db::set_meta(&self.lock().conn, key, value)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sanitize_file_name(title: &str) -> String {
    let s: String = title.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() { '-' } else { c }).collect();
    let s = s.trim().trim_matches('.').trim().to_string();
    let s: String = s.chars().take(100).collect();
    if s.is_empty() { "Untitled".into() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn lib() -> (tempfile::TempDir, Library) {
        let dir = tempfile::tempdir().unwrap();
        let l = Library::open_with_clock(dir.path(), Box::new(|| 1_700_000_000_000)).unwrap();
        (dir, l)
    }

    #[test]
    fn note_lifecycle_title_search_tasks() {
        let (_d, l) = lib();
        let n = l.create_note("# Lisbon Trip\n\n- [ ] book flights\n- [x] pack\n\n#travel", Some(1000)).unwrap();
        assert_eq!(n.title, "Lisbon Trip");
        assert_eq!(l.list_notes().unwrap()[0].task_count, 2);
        assert_eq!(l.list_notes().unwrap()[0].done_count, 1);
        assert_eq!(l.list_notes().unwrap()[0].tags, vec!["travel"]);
        assert_eq!(l.search("flights", 5).unwrap()[0].owner_id, n.id);
        let n2 = l.toggle_task(&n.id, 2).unwrap();
        assert!(n2.body.contains("- [x] book flights"));
        assert!(l.toggle_task(&n.id, 0).is_err());
        let n3 = l.update_note(&n.id, Some("# Renamed\nnothing"), Some(5)).unwrap();
        assert_eq!((n3.title.as_str(), n3.date), ("Renamed", 5));
        assert!(l.search("flights", 5).unwrap().is_empty());
        assert_eq!(l.list_tags().unwrap().len(), 0);
        l.delete_note(&n.id).unwrap();
        assert!(l.get_note(&n.id).is_err());
        assert!(l.delete_note(&n.id).is_err());
        assert!(l.search("renamed", 5).unwrap().is_empty());
    }

    #[test]
    fn context_backlinks_outgoing_and_open_by_title() {
        let (_d, l) = lib();
        let a = l.create_note("# Atlas\nsee [[Lisbon Trip]] and [[Ghost Note]]", Some(1)).unwrap();
        let b = l.create_note("# Lisbon Trip\nthe sun, the tiles. Related: [[atlas]]", Some(2)).unwrap();
        let ctx = l.note_context(&b.id).unwrap();
        assert_eq!(ctx.backlinks.len(), 1);
        assert_eq!(ctx.backlinks[0].note_id, a.id);
        assert!(ctx.backlinks[0].snippet.contains("Lisbon Trip"));
        assert_eq!(ctx.outgoing[0].note_id.as_deref(), Some(a.id.as_str()));
        let ctx = l.note_context(&a.id).unwrap();
        assert_eq!(ctx.outgoing.iter().filter(|o| o.note_id.is_none()).count(), 1);
        // open existing (case-insensitive) vs create
        assert_eq!(l.open_note_by_title("lisbon trip", None).unwrap().id, b.id);
        let g = l.open_note_by_title("Ghost Note", Some(3)).unwrap();
        assert_eq!(g.title, "Ghost Note");
        assert_eq!(l.note_titles().unwrap().len(), 3);
        assert!(l.open_note_by_title("  ", None).is_err());
        let graph = l.graph().unwrap();
        assert!(graph.nodes.iter().all(|n| !n.is_ghost)); // ghost resolved now
    }

    #[test]
    fn import_dedupes_probes_and_serves_bytes() {
        let (_d, l) = lib();
        let wav = media::synth_wav(5.0, 5.0, 200.0);
        let r = l.import_media("Voice_note.wav", &wav, Some(42)).unwrap();
        assert!(!r.duplicate);
        assert_eq!(r.item.title, "Voice note");
        assert_eq!(r.item.kind, MediaKind::Audio);
        assert!((r.item.duration.unwrap() - 5.0).abs() < 0.01);
        assert_eq!(r.item.recorded_at, 42);
        let again = l.import_media("copy.wav", &wav, Some(99)).unwrap();
        assert!(again.duplicate);
        assert_eq!(again.item.id, r.item.id);
        assert_eq!(l.list_media().unwrap().len(), 1);
        let (bytes, mime) = l.media_bytes(&r.item.id).unwrap();
        assert_eq!((bytes, mime.as_str()), (wav.clone(), "audio/wav"));
        assert!(matches!(l.import_media("x.pdf", b"abc", None), Err(Error::UnsupportedFileType(_))));
        assert!(matches!(l.import_media("x.wav", b"", None), Err(Error::InvalidArgument(_))));
        // missing file detection
        let (p, _) = l.media_file(&r.item.id).unwrap();
        std::fs::remove_file(p).unwrap();
        assert!(l.get_media(&r.item.id).unwrap().is_missing);
        assert!(matches!(l.media_bytes(&r.item.id), Err(Error::FileMissing(_))));
        l.delete_media(&r.item.id).unwrap();
        assert!(l.list_media().unwrap().is_empty());
    }

    #[test]
    fn transcripts_attach_search_and_export() {
        let (_d, l) = lib();
        let wav = media::synth_wav(20.0, 5.0, 200.0);
        let m = l.import_media("walk.wav", &wav, Some(1_000_000)).unwrap().item;
        let t = l.attach_transcript(&m.id, "walk.srt", "1\n00:00:01,000 --> 00:00:04,000\nThe tiles are unreal\n\n2\n00:00:06,000 --> 00:00:09,000\nSardines grilling nearby\n").unwrap();
        assert_eq!(t.segments.len(), 2);
        assert!(l.get_media(&m.id).unwrap().has_transcript);
        let hit = &l.search("sardines", 5).unwrap()[0];
        assert_eq!(hit.kind, SearchDocKind::TranscriptSegment);
        assert_eq!(hit.owner_id, m.id);
        assert_eq!(hit.time, Some(6.0));
        assert_eq!(l.export_transcript_text(&m.id).unwrap(), "walk\n\n[0:01] The tiles are unreal\n[0:06] Sardines grilling nearby\n");
        assert!(l.attach_transcript(&m.id, "bad.json", "{").is_err());
        l.remove_transcript(&m.id).unwrap();
        assert!(l.search("sardines", 5).unwrap().is_empty());
        let png = media::synth_png(8, 8, 1);
        let p = l.import_media("p.png", &png, None).unwrap().item;
        assert!(l.attach_transcript(&p.id, "a.txt", "hi").is_err());
        assert!(l.get_transcript("nope").unwrap().is_none());
    }

    #[test]
    fn timeline_entries_mix_sorted_with_tags_and_snippets() {
        let (_d, l) = lib();
        l.create_note("# B note\nhello #x", Some(2000)).unwrap();
        let m = l.import_media("a.wav", &media::synth_wav(3.0, 3.0, 200.0), Some(1000)).unwrap().item;
        l.attach_transcript(&m.id, "a.txt", "first line\nsecond line\nthird").unwrap();
        l.import_media("p.png", &media::synth_png(4, 4, 0), Some(3000)).unwrap();
        let e = l.timeline_entries().unwrap();
        assert_eq!(e.iter().map(|x| x.kind).collect::<Vec<_>>(), vec![EntryKind::Recording, EntryKind::Note, EntryKind::Photo]);
        assert_eq!(e[0].snippet, "first line second line");
        assert_eq!(e[1].tags, vec!["x"]);
        assert!((e[0].duration - 3.0).abs() < 0.01);
    }

    #[test]
    fn persistence_across_reopen_rebuilds_index() {
        let dir = tempfile::tempdir().unwrap();
        {
            let l = Library::open(dir.path()).unwrap();
            l.create_note("# Persisted\nwhale songs", Some(1)).unwrap();
            let m = l.import_media("a.wav", &media::synth_wav(2.0, 2.0, 200.0), Some(1)).unwrap().item;
            l.attach_transcript(&m.id, "a.txt", "dolphins chatting").unwrap();
        }
        let l = Library::open(dir.path()).unwrap();
        assert_eq!(l.search("whale", 3).unwrap().len(), 1);
        assert_eq!(l.search("dolphins", 3).unwrap().len(), 1);
        assert_eq!(l.stats().unwrap().notes, 1);
    }

    #[test]
    fn markdown_export_writes_files_with_unique_names() {
        let (_d, l) = lib();
        l.create_note("# Same: Title?\nbody one", Some(0)).unwrap();
        l.create_note("# Same- Title-\nbody two", Some(1000)).unwrap();
        let r = l.export_markdown().unwrap();
        assert_eq!(r.count, 2);
        let mut names: Vec<String> = std::fs::read_dir(&r.dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, vec!["Same- Title- 2.md", "Same- Title-.md"]);
        let c = std::fs::read_to_string(std::path::Path::new(&r.dir).join("Same- Title-.md")).unwrap();
        assert!(c.starts_with("---\ndate: 1970-01-01T00:00:00Z\n---\n\n# Same: Title?"));
        assert_eq!(sanitize_file_name("..//"), "--");
        assert_eq!(sanitize_file_name("   "), "Untitled");
    }

    #[test]
    fn update_media_renames_and_reindexes() {
        let (_d, l) = lib();
        let m = l.import_media("a.wav", &media::synth_wav(2.0, 2.0, 200.0), Some(1)).unwrap().item;
        l.update_media(&m.id, Some("Garden chat"), Some(77)).unwrap();
        let g = l.get_media(&m.id).unwrap();
        assert_eq!((g.title.as_str(), g.recorded_at), ("Garden chat", 77));
        assert_eq!(l.search("garden", 3).unwrap()[0].owner_id, m.id);
        assert!(l.update_media("nope", Some("x"), None).is_err());
    }
}
