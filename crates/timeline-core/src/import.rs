//! Import of Markdown notes (single files or whole folders, e.g. an Obsidian vault).
//!
//! * YAML front matter (`title`, `date`/`created`, `tags`, `aliases`, any other key) is parsed into
//!   `Note::properties`; the body keeps only the Markdown text.
//! * The note date comes from front matter, else a leading `YYYY-MM-DD` in the file name (daily
//!   notes), else the older of the file's creation/modification time.
//! * `[[wiki links]]` (incl. `[[a|b]]`, `[[a#h]]`, `[[folder/a]]`) are kept verbatim; the graph resolves
//!   them by title, file name or alias across all notes, so links between imported notes just work.
//! * Folder structure is kept in `Note::folder`. Dot-folders (`.obsidian`, `.git`, `.trash`) are skipped.
//! * Images referenced as `![alt](rel/path.png)` or `![[pic.png]]` are copied into the media store
//!   (hidden from the timeline) and rewritten to `media:<id>`.
//! * Files already imported (same content hash or same source path) are skipped.

use crate::graph;
use crate::jobs::JobCtx;
use crate::library::Library;
use crate::markdown;
use crate::media;
use crate::models::{MediaKind, Ms, Note};
use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const MAX_LISTED: usize = 200;
const BATCH: usize = 200;
const NOTE_EXTS: [&str; 3] = ["md", "markdown", "mdown"];

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub files_seen: usize,
    pub imported: usize,
    pub skipped_duplicate: usize,
    pub skipped_other: usize,
    pub error_count: usize,
    /// First `MAX_LISTED` skipped files with the reason.
    pub skipped: Vec<FileIssue>,
    /// First `MAX_LISTED` errors.
    pub errors: Vec<FileIssue>,
    pub attachments_imported: usize,
    pub attachments_missing: usize,
    pub links_resolved: usize,
    pub links_unresolved: usize,
    pub folders: usize,
    /// Names of detected Obsidian vault roots (folders containing `.obsidian`).
    pub vaults: Vec<String>,
    pub cancelled: bool,
    pub elapsed_ms: u64,
    /// Id of one imported note (to open it) – the earliest-dated one.
    pub first_note_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FileIssue {
    pub path: String,
    pub reason: String,
}

impl ImportSummary {
    pub fn merge(&mut self, o: ImportSummary) {
        self.files_seen += o.files_seen;
        self.imported += o.imported;
        self.skipped_duplicate += o.skipped_duplicate;
        self.skipped_other += o.skipped_other;
        self.error_count += o.error_count;
        self.skipped.extend(o.skipped);
        self.skipped.truncate(MAX_LISTED);
        self.errors.extend(o.errors);
        self.errors.truncate(MAX_LISTED);
        self.attachments_imported += o.attachments_imported;
        self.attachments_missing += o.attachments_missing;
        self.links_resolved += o.links_resolved;
        self.links_unresolved += o.links_unresolved;
        self.folders = self.folders.max(o.folders);
        for v in o.vaults {
            if !self.vaults.contains(&v) {
                self.vaults.push(v);
            }
        }
        self.cancelled |= o.cancelled;
        self.elapsed_ms += o.elapsed_ms;
        if self.first_note_id.is_none() {
            self.first_note_id = o.first_note_id;
        }
    }
    fn skip(&mut self, path: &str, reason: &str, duplicate: bool) {
        if duplicate {
            self.skipped_duplicate += 1;
        } else {
            self.skipped_other += 1;
        }
        if self.skipped.len() < MAX_LISTED {
            self.skipped.push(FileIssue { path: path.into(), reason: reason.into() });
        }
    }
    fn error(&mut self, path: &str, reason: String) {
        self.error_count += 1;
        if self.errors.len() < MAX_LISTED {
            self.errors.push(FileIssue { path: path.into(), reason });
        }
    }
}

// ───────────────────────────── front matter ─────────────────────────────

/// Splits `---\nyaml\n---\nbody`. Returns the parsed map (if a well-formed block exists) and the body.
pub fn split_front_matter(text: &str) -> (Option<Map<String, Value>>, String) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.split('\n');
    match lines.next() {
        Some(l) if l.trim_end() == "---" => {}
        _ => return (None, text.to_string()),
    }
    let mut block: Vec<&str> = Vec::new();
    let mut consumed = text.find('\n').map(|i| i + 1).unwrap_or(text.len());
    let mut closed = false;
    for l in lines {
        consumed += l.len() + 1;
        let t = l.trim_end();
        if t == "---" || t == "..." {
            closed = true;
            break;
        }
        block.push(l.trim_end_matches('\r'));
    }
    if !closed {
        return (None, text.to_string());
    }
    let body = if consumed >= text.len() { "" } else { &text[consumed..] };
    (Some(parse_yaml_block(&block)), body.trim_start_matches(['\r', '\n']).to_string())
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\''))) {
        let inner = &s[1..s.len() - 1];
        if s.starts_with('"') {
            return inner.replace("\\\"", "\"").replace("\\n", "\n").replace("\\\\", "\\");
        }
        return inner.replace("''", "'");
    }
    s.to_string()
}

fn scalar(s: &str) -> Value {
    let t = s.trim();
    if t.starts_with('"') || t.starts_with('\'') {
        return Value::String(unquote(t));
    }
    match t {
        "true" | "True" | "yes" => return Value::Bool(true),
        "false" | "False" | "no" => return Value::Bool(false),
        "null" | "~" | "" => return Value::Null,
        _ => {}
    }
    if t.len() < 16 && !t.starts_with('0') || t == "0" {
        if let Ok(i) = t.parse::<i64>() {
            return Value::from(i);
        }
        if t.contains('.') {
            if let Ok(f) = t.parse::<f64>() {
                if let Some(n) = serde_json::Number::from_f64(f) {
                    return Value::Number(n);
                }
            }
        }
    }
    // strip trailing comment
    let t = match t.find(" #") {
        Some(i) => t[..i].trim_end(),
        None => t,
    };
    Value::String(t.to_string())
}

fn split_inline(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in s.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => {
                quote = None;
                cur.push(c)
            }
            (Some(_), c) => cur.push(c),
            (None, '"') | (None, '\'') => {
                quote = Some(c);
                cur.push(c)
            }
            (None, ',') => out.push(std::mem::take(&mut cur)),
            (None, c) => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out.into_iter().map(|x| unquote(&x)).filter(|x| !x.is_empty()).collect()
}

/// Minimal YAML subset: `key: scalar`, `key: [a, b]`, block lists (`- a`), `|`/`>` block scalars;
/// nested maps are kept as their raw text. Enough for Obsidian properties.
pub fn parse_yaml_block(lines: &[&str]) -> Map<String, Value> {
    let mut map = Map::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        i += 1;
        if line.trim().is_empty() || line.trim_start().starts_with('#') || line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        let Some((k, v)) = line.split_once(':') else { continue };
        let key = unquote(k.trim());
        if key.is_empty() {
            continue;
        }
        let v = v.trim();
        let mut nested: Vec<&str> = Vec::new();
        while i < lines.len() && (lines[i].starts_with(' ') || lines[i].starts_with('\t') || lines[i].trim().is_empty() || lines[i].starts_with("- ") && v.is_empty()) {
            nested.push(lines[i]);
            i += 1;
        }
        while nested.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
            nested.pop();
        }
        let value = if v.is_empty() {
            if nested.is_empty() {
                Value::Null
            } else if nested.iter().all(|l| l.trim().is_empty() || l.trim_start().starts_with("- ") || l.trim() == "-") {
                Value::Array(nested.iter().filter(|l| !l.trim().is_empty()).map(|l| scalar(l.trim_start().trim_start_matches('-').trim())).collect())
            } else {
                Value::String(nested.iter().map(|l| l.trim()).collect::<Vec<_>>().join("\n"))
            }
        } else if v.starts_with('|') || v.starts_with('>') {
            let folded = v.starts_with('>');
            let parts: Vec<&str> = nested.iter().map(|l| l.trim()).collect();
            Value::String(if folded { parts.join(" ") } else { parts.join("\n") })
        } else if v.starts_with('[') && v.ends_with(']') {
            Value::Array(split_inline(&v[1..v.len() - 1]).into_iter().map(Value::String).collect())
        } else if v.starts_with('{') {
            Value::String(v.to_string())
        } else {
            scalar(v)
        };
        map.insert(key, value);
    }
    map
}

fn prop<'a>(m: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    for k in keys {
        if let Some((_, v)) = m.iter().find(|(name, _)| name.eq_ignore_ascii_case(k)) {
            return Some(v);
        }
    }
    None
}

/// Serialises properties back into a front matter block (used by Markdown export).
pub fn front_matter_text(props: &Map<String, Value>) -> String {
    if props.is_empty() {
        return String::new();
    }
    fn q(s: &str) -> String {
        let plain_date = s.len() >= 10 && s.as_bytes()[0].is_ascii_digit() && parse_date(s, 0).is_some();
        if !plain_date && (s.is_empty() || s.contains(": ") || s.contains(" #") || s.ends_with(':') || s.contains(['\n', '"', ',']) || s.trim() != s || s.starts_with(['-', '>', '|', '!', '&', '*', '%', '@', '[', '{', '\'', '#', '?']) || scalar(s) != Value::String(s.to_string())) {
            format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n"))
        } else {
            s.to_string()
        }
    }
    fn sc(v: &Value) -> String {
        match v {
            Value::String(s) => q(s),
            Value::Null => String::new(),
            o => o.to_string(),
        }
    }
    let mut out = String::from("---\n");
    for (k, v) in props {
        match v {
            Value::Array(a) => {
                out.push_str(&format!("{k}:\n"));
                for x in a {
                    out.push_str(&format!("  - {}\n", sc(x)));
                }
            }
            _ => out.push_str(&format!("{k}: {}\n", sc(v))),
        }
    }
    out.push_str("---\n\n");
    out
}

// ───────────────────────────── dates ─────────────────────────────

/// Parses the date notations found in note properties. Zone-less values are taken as local time
/// at `tz_offset_minutes` (minutes **east** of UTC); date-only values land on 12:00 local.
pub fn parse_date(s: &str, tz_offset_minutes: i32) -> Option<Ms> {
    let s = s.trim().trim_matches(['"', '\'']);
    if s.is_empty() {
        return None;
    }
    if let Ok(d) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(d.timestamp_millis());
    }
    let off = tz_offset_minutes as i64 * 60_000;
    for f in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M", "%Y/%m/%d %H:%M", "%d.%m.%Y %H:%M"] {
        if let Ok(d) = chrono::NaiveDateTime::parse_from_str(s, f) {
            return Some(d.and_utc().timestamp_millis() - off);
        }
    }
    for f in ["%Y-%m-%d", "%Y/%m/%d", "%Y.%m.%d", "%d.%m.%Y"] {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(s, f) {
            let dt = d.and_hms_opt(12, 0, 0)?;
            return Some(dt.and_utc().timestamp_millis() - off);
        }
    }
    None
}

/// `2024-03-05 Daily.md`, `2024-03-05.md`, `20240305 notes.md` → that date.
pub fn date_from_file_stem(stem: &str, tz: i32) -> Option<Ms> {
    let b = stem.as_bytes();
    if b.len() >= 10 && stem.is_char_boundary(10) && b[..4].iter().all(u8::is_ascii_digit) && b[4] == b'-' && b[7] == b'-' {
        return parse_date(&stem[..10], tz);
    }
    None
}

// ───────────────────────────── attachments ─────────────────────────────

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && s.is_char_boundary(i + 1) && s.is_char_boundary(i + 3) {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn is_image_name(name: &str) -> bool {
    media::kind_for_extension(&media::extension(name)) == Some(MediaKind::Photo)
}

/// Rewrites image references in `body` using `resolve(reference) -> media id`. References that don't
/// resolve (remote URLs, missing files, note embeds) are left untouched. Returns (body, missing count).
pub fn rewrite_images(body: &str, resolve: &mut dyn FnMut(&str) -> Option<String>) -> (String, usize) {
    let mut out = String::with_capacity(body.len());
    let mut missing = 0;
    let mut in_fence = false;
    // process line-wise so that fenced code is left alone
    for line in body.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
        }
        if in_fence || !line.contains("![") {
            out.push_str(line);
            continue;
        }
        let mut s = line;
        while let Some(p) = s.find("![") {
            out.push_str(&s[..p]);
            let after = &s[p..];
            if let Some(inner) = after.strip_prefix("![[") {
                if let Some(end) = inner.find("]]") {
                    let target = &inner[..end];
                    let name = target.split('|').next().unwrap_or("").split('#').next().unwrap_or("").trim();
                    if is_image_name(name) {
                        if let Some(id) = resolve(name) {
                            let alt = name.rsplit('/').next().unwrap_or(name);
                            out.push_str(&format!("![{alt}](media:{id})"));
                        } else {
                            missing += 1;
                            out.push_str(&after[..3 + end + 2]);
                        }
                    } else {
                        out.push_str(&after[..3 + end + 2]);
                    }
                    s = &after[3 + end + 2..];
                    continue;
                }
            } else if let Some(close) = after.find("](") {
                let alt = &after[2..close];
                if !alt.contains('\n') && !alt.contains("![") {
                    let dest_part = &after[close + 2..];
                    if let Some(end) = dest_part.find(')') {
                        let raw = dest_part[..end].trim();
                        let dest = if let Some(r) = raw.strip_prefix('<') { r.split('>').next().unwrap_or("") } else { raw.split_whitespace().next().unwrap_or("") };
                        let lower = dest.to_ascii_lowercase();
                        let remote = lower.starts_with("http:") || lower.starts_with("https:") || lower.starts_with("data:") || lower.starts_with("media:") || lower.starts_with("tlmedia:");
                        if !remote && !dest.is_empty() && is_image_name(dest) {
                            let decoded = percent_decode(dest);
                            let decoded = decoded.split('#').next().unwrap_or("").to_string();
                            if let Some(id) = resolve(&decoded) {
                                out.push_str(&format!("![{alt}](media:{id})"));
                            } else {
                                missing += 1;
                                out.push_str(&after[..close + 2 + end + 1]);
                            }
                        } else {
                            out.push_str(&after[..close + 2 + end + 1]);
                        }
                        s = &after[close + 2 + end + 1..];
                        continue;
                    }
                }
            }
            // not an image we understand: copy "![" and move on
            out.push_str("![");
            s = &after[2..];
        }
        out.push_str(s);
    }
    (out, missing)
}

// ───────────────────────────── building notes ─────────────────────────────

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

struct Builder<'a> {
    lib: &'a Library,
    tz: i32,
    now: Ms,
    hashes: HashSet<String>,
    paths: HashSet<String>,
    pending: Vec<Note>,
    summary: ImportSummary,
    folders: HashSet<String>,
    imported: Vec<Note>,
}

impl<'a> Builder<'a> {
    fn new(lib: &'a Library, tz: i32) -> Result<Self> {
        let (hashes, paths) = lib.imported_keys()?;
        Ok(Builder { lib, tz, now: lib.now(), hashes, paths, pending: Vec::new(), summary: ImportSummary::default(), folders: HashSet::new(), imported: Vec::new() })
    }

    #[allow(clippy::too_many_arguments)]
    fn add(&mut self, display: &str, folder: &str, stem: &str, raw: &[u8], source_path: Option<String>, file_ms: Option<Ms>, resolve: &mut dyn FnMut(&str) -> Option<String>) {
        self.summary.files_seen += 1;
        if raw.contains(&0) {
            self.summary.error(display, "not a text file (binary content)".into());
            return;
        }
        let hash = hex(&Sha256::digest(raw));
        if let Some(sp) = &source_path {
            if self.paths.contains(sp) {
                self.summary.skip(display, "already imported (same file path)", true);
                return;
            }
        }
        if self.hashes.contains(&hash) {
            self.summary.skip(display, "duplicate content (identical note already imported)", true);
            return;
        }
        let text = String::from_utf8_lossy(raw).replace("\r\n", "\n");
        let (fm, body) = split_front_matter(&text);
        let props = fm.unwrap_or_default();
        let date = prop(&props, &["date", "created", "created_at", "createdat", "date created", "creation date", "datetime", "published"])
            .and_then(|v| v.as_str().and_then(|s| parse_date(s, self.tz)).or_else(|| v.as_i64().filter(|n| *n > 1_000_000_000_000)))
            .or_else(|| date_from_file_stem(stem, self.tz))
            .or(file_ms)
            .unwrap_or(self.now);
        let (body, missing) = rewrite_images(&body, resolve);
        self.summary.attachments_missing += missing;
        let fallback = prop(&props, &["title"]).and_then(|v| v.as_str()).map(String::from).unwrap_or_else(|| stem.to_string());
        let title = markdown::derive_title_with(&body, Some(&fallback));
        let id = uuid::Uuid::new_v4().to_string();
        let note = Note {
            id,
            title,
            body,
            date,
            created_at: self.now,
            updated_at: self.now,
            folder: folder.to_string(),
            source_path: source_path.clone(),
            content_hash: Some(hash.clone()),
            slug: Some(stem.to_string()),
            properties: props,
        };
        self.hashes.insert(hash);
        if let Some(sp) = source_path {
            self.paths.insert(sp);
        }
        self.folders.insert(folder.to_string());
        self.pending.push(note);
        if self.pending.len() >= BATCH {
            self.flush(&mut |_| {});
        }
    }

    fn flush(&mut self, _on: &mut dyn FnMut(usize)) {
        if self.pending.is_empty() {
            return;
        }
        let batch = std::mem::take(&mut self.pending);
        match self.lib.insert_notes_bulk(&batch) {
            Ok(()) => {
                self.summary.imported += batch.len();
                self.imported.extend(batch);
            }
            Err(e) => {
                for n in &batch {
                    self.summary.error(n.source_path.as_deref().unwrap_or(&n.title), format!("database error: {e}"));
                }
            }
        }
    }

    fn finish(mut self, started: std::time::Instant) -> Result<ImportSummary> {
        self.flush(&mut |_| {});
        // link statistics against the whole library
        if !self.imported.is_empty() {
            let all = self.lib.all_notes()?;
            let refs: Vec<&Note> = all.iter().collect();
            let names = graph::name_index(&refs);
            for n in &self.imported {
                for t in markdown::parse(&n.body).unique_link_targets() {
                    if names.contains_key(&graph::target_key(&t)) {
                        self.summary.links_resolved += 1;
                    } else {
                        self.summary.links_unresolved += 1;
                    }
                }
            }
            self.summary.first_note_id = self.imported.iter().min_by_key(|n| n.date).map(|n| n.id.clone());
        }
        self.summary.folders = self.folders.iter().filter(|f| !f.is_empty()).count();
        self.summary.elapsed_ms = started.elapsed().as_millis() as u64;
        Ok(self.summary)
    }
}

fn is_note_file(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).map(|e| NOTE_EXTS.contains(&e.to_ascii_lowercase().as_str())).unwrap_or(false)
}

fn file_ms(p: &Path) -> Option<Ms> {
    let m = std::fs::metadata(p).ok()?;
    let t = |r: std::io::Result<std::time::SystemTime>| r.ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as Ms);
    match (t(m.created()), t(m.modified())) {
        (Some(c), Some(u)) => Some(c.min(u)),
        (c, u) => c.or(u),
    }
}

struct Scanned {
    notes: Vec<(PathBuf, PathBuf)>, // (file, root it was found under)
    images: HashMap<String, Vec<PathBuf>>,
    vaults: Vec<String>,
}

fn scan(paths: &[String], summary: &mut ImportSummary, ctx: &JobCtx) -> Scanned {
    let mut sc = Scanned { notes: Vec::new(), images: HashMap::new(), vaults: Vec::new() };
    for p in paths {
        let path = PathBuf::from(p);
        if path.is_dir() {
            if path.join(".obsidian").is_dir() {
                sc.vaults.push(path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.clone()));
            }
            let walker = walkdir::WalkDir::new(&path).follow_links(false).sort_by_file_name().into_iter().filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                !(e.depth() > 0 && e.file_type().is_dir() && (name.starts_with('.') || name == "node_modules"))
            });
            for e in walker {
                if ctx.cancelled() {
                    return sc;
                }
                match e {
                    Ok(e) if e.file_type().is_file() => {
                        let fp = e.path();
                        let name = fp.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        if name.starts_with('.') {
                            continue;
                        }
                        if is_note_file(fp) {
                            sc.notes.push((fp.to_path_buf(), path.clone()));
                        } else if is_image_name(&name) {
                            sc.images.entry(name.to_lowercase()).or_default().push(fp.to_path_buf());
                        }
                    }
                    Ok(_) => {}
                    Err(err) => summary.error(&err.path().map(|p| p.display().to_string()).unwrap_or_default(), err.to_string()),
                }
            }
        } else if path.is_file() {
            let ok = is_note_file(&path) || path.extension().map(|e| e.eq_ignore_ascii_case("txt")).unwrap_or(false);
            if ok {
                let root = path.parent().map(Path::to_path_buf).unwrap_or_default();
                sc.notes.push((path, root));
            } else {
                summary.files_seen += 1;
                summary.skip(p, "not a Markdown file (.md / .markdown)", false);
            }
        } else {
            summary.files_seen += 1;
            summary.error(p, "file or folder not found".into());
        }
    }
    sc
}

/// Imports Markdown files and/or folders from disk. `tz_offset_minutes` = minutes east of UTC
/// (what the UI's `-Date.getTimezoneOffset()` gives) for zone-less dates in front matter.
pub fn import_paths(lib: &Library, paths: &[String], tz_offset_minutes: i32, ctx: &JobCtx) -> Result<ImportSummary> {
    let started = std::time::Instant::now();
    let mut b = Builder::new(lib, tz_offset_minutes)?;
    ctx.progress(0.0, "Scanning…");
    let scanned = scan(paths, &mut b.summary, ctx);
    b.summary.vaults = scanned.vaults.clone();
    let total = scanned.notes.len().max(1);
    let mut att_cache: HashMap<PathBuf, Option<String>> = HashMap::new();
    let mut att_new = 0usize;
    for (idx, (file, root)) in scanned.notes.iter().enumerate() {
        if ctx.cancelled() {
            b.summary.cancelled = true;
            break;
        }
        if idx % 25 == 0 {
            ctx.progress(0.05 + 0.9 * idx as f64 / total as f64, format!("Importing {} of {}…", idx + 1, total));
        }
        let display = file.strip_prefix(root).unwrap_or(file).to_string_lossy().replace('\\', "/");
        let folder = file.parent().and_then(|d| d.strip_prefix(root).ok()).map(|d| d.to_string_lossy().replace('\\', "/")).unwrap_or_default();
        let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let raw = match std::fs::read(file) {
            Ok(r) => r,
            Err(e) => {
                b.summary.files_seen += 1;
                b.summary.error(&display, e.to_string());
                continue;
            }
        };
        let source = std::fs::canonicalize(file).unwrap_or_else(|_| file.clone()).to_string_lossy().into_owned();
        let note_dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
        let ms = file_ms(file);
        let images = &scanned.images;
        let lib_ref = lib;
        let mut resolve = |r: &str| -> Option<String> {
            let rel = r.replace('\\', "/");
            let rel = rel.trim_start_matches("./");
            let mut cands: Vec<PathBuf> = vec![note_dir.join(rel), root.join(rel)];
            if let Some(name) = rel.rsplit('/').next() {
                if let Some(list) = images.get(&name.to_lowercase()) {
                    // prefer the file whose path ends with the reference, then the closest to the note
                    let mut sorted: Vec<&PathBuf> = list.iter().collect();
                    sorted.sort_by_key(|p| (!p.to_string_lossy().replace('\\', "/").ends_with(rel), p.parent() != Some(note_dir.as_path())));
                    cands.extend(sorted.into_iter().cloned());
                }
            }
            for c in cands {
                if !c.is_file() {
                    continue;
                }
                if let Some(v) = att_cache.get(&c) {
                    return v.clone();
                }
                let id = std::fs::read(&c).ok().and_then(|bytes| {
                    let name = c.file_name()?.to_string_lossy().into_owned();
                    lib_ref.import_attachment(&name, &bytes, file_ms(&c).unwrap_or(0)).ok()
                });
                if id.is_some() {
                    att_new += 1;
                }
                att_cache.insert(c, id.clone());
                return id;
            }
            None
        };
        b.add(&display, &folder, &stem, &raw, Some(source), ms, &mut resolve);
    }
    b.summary.attachments_imported = att_new;
    b.finish(started)
}

/// A file uploaded by the browser fallback (no real paths available there).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineFile {
    /// Path relative to the dropped/selected folder, `/`-separated (just the name for single files).
    pub path: String,
    pub text: String,
    #[serde(default)]
    pub modified_ms: Option<Ms>,
}

/// Imports Markdown uploaded as text (browser mode). Images are not available here.
pub fn import_inline(lib: &Library, files: &[InlineFile], vaults: &[String], tz_offset_minutes: i32) -> Result<ImportSummary> {
    let started = std::time::Instant::now();
    let mut b = Builder::new(lib, tz_offset_minutes)?;
    b.summary.vaults = vaults.to_vec();
    for f in files {
        let path = f.path.replace('\\', "/");
        let comps: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
        let Some(name) = comps.last() else { continue };
        if comps[..comps.len() - 1].iter().any(|c| c.starts_with('.')) || name.starts_with('.') {
            continue;
        }
        let lower = name.to_ascii_lowercase();
        let ok = lower.ends_with(".md") || lower.ends_with(".markdown") || lower.ends_with(".mdown") || lower.ends_with(".txt");
        if !ok {
            b.summary.files_seen += 1;
            b.summary.skip(&path, "not a Markdown file (.md / .markdown)", false);
            continue;
        }
        let folder = comps[..comps.len() - 1].join("/");
        let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
        b.add(&path, &folder, stem, f.text.as_bytes(), None, f.modified_ms, &mut |_| None);
    }
    b.finish(started)
}

impl Library {
    pub(crate) fn all_notes(&self) -> Result<Vec<Note>> {
        self.notes_all()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_variants() {
        let (fm, body) = split_front_matter("---\ntitle: \"Hello: World\"\ndate: 2024-03-05\ntags: [a, \"b c\", #d]\naliases:\n  - Alt One\n  - Alt Two\nrating: 4\ndone: true\nsummary: >\n  folded\n  text\n---\n# Body\ntext");
        let fm = fm.unwrap();
        assert_eq!(fm["title"], "Hello: World");
        assert_eq!(fm["date"], "2024-03-05");
        assert_eq!(fm["tags"], serde_json::json!(["a", "b c", "#d"]));
        assert_eq!(fm["aliases"], serde_json::json!(["Alt One", "Alt Two"]));
        assert_eq!(fm["rating"], 4);
        assert_eq!(fm["done"], true);
        assert_eq!(fm["summary"], "folded text");
        assert_eq!(body, "# Body\ntext");
    }

    #[test]
    fn front_matter_absent_or_unclosed_is_left_alone() {
        assert!(split_front_matter("no front matter\n---\nx").0.is_none());
        let (fm, body) = split_front_matter("---\ntitle: x\nno end");
        assert!(fm.is_none());
        assert!(body.starts_with("---"));
        let (fm, body) = split_front_matter("\u{feff}---\r\ntitle: x\r\n---\r\nbody");
        assert_eq!(fm.unwrap()["title"], "x");
        assert_eq!(body, "body");
    }

    #[test]
    fn dates() {
        // 2024-03-05T12:00 at UTC+2 is 10:00Z
        assert_eq!(parse_date("2024-03-05", 120), Some(1_709_632_800_000));
        assert_eq!(parse_date("2024-03-05T10:00:00Z", 120), Some(1_709_632_800_000));
        assert_eq!(parse_date("2024-03-05 12:00", 120), Some(1_709_632_800_000));
        assert_eq!(parse_date("2024-03-05T12:00:00+02:00", 0), parse_date("2024-03-05T10:00:00Z", 0));
        assert_eq!(parse_date("05.03.2024", 0), parse_date("2024-03-05", 0));
        assert_eq!(parse_date("someday", 0), None);
        assert_eq!(date_from_file_stem("2024-03-05 Daily", 0), parse_date("2024-03-05", 0));
        assert_eq!(date_from_file_stem("Meeting notes", 0), None);
    }

    #[test]
    fn image_rewriting() {
        let mut seen = vec![];
        let mut r = |s: &str| {
            seen.push(s.to_string());
            if s.contains("missing") {
                None
            } else {
                Some("ID1".to_string())
            }
        };
        let body = "a ![x](img/my%20pic.png) b ![[pic.jpg|200]] c ![r](https://x/y.png) d ![[Other note]] e ![m](missing.png)\n```\n![c](code.png)\n```\n";
        let (out, missing) = rewrite_images(body, &mut r);
        assert!(out.contains("![x](media:ID1)"));
        assert!(out.contains("![pic.jpg](media:ID1)"));
        assert!(out.contains("![r](https://x/y.png)"));
        assert!(out.contains("![[Other note]]"));
        assert!(out.contains("![m](missing.png)"));
        assert!(out.contains("![c](code.png)"));
        assert_eq!(missing, 1);
        assert_eq!(seen, vec!["img/my pic.png", "pic.jpg", "missing.png"]);
    }

    #[test]
    fn front_matter_roundtrip_text() {
        let (fm, _) = split_front_matter("---\ntitle: a: b\ntags: [x, y]\n---\n");
        let t = front_matter_text(&fm.unwrap());
        let (fm2, _) = split_front_matter(&format!("{t}body"));
        assert_eq!(fm2.unwrap()["tags"], serde_json::json!(["x", "y"]));
    }
}
