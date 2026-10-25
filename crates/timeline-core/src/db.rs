//! SQLite storage: schema migrations (rusqlite_migration) and row <-> model mapping.

use crate::error::Result;
use crate::models::*;
use rusqlite::{params, Connection, OptionalExtension, Row};
use rusqlite_migration::{Migrations, M};

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up(
        "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE media_item (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            title TEXT NOT NULL,
            recorded_at INTEGER NOT NULL,
            duration REAL,
            sha256 TEXT NOT NULL UNIQUE,
            file_size INTEGER NOT NULL,
            imported_at INTEGER NOT NULL,
            mime TEXT NOT NULL,
            original_name TEXT NOT NULL,
            ext TEXT NOT NULL
         );
         CREATE INDEX media_recorded ON media_item(recorded_at);
         CREATE TABLE transcript_meta (
            media_id TEXT PRIMARY KEY REFERENCES media_item(id) ON DELETE CASCADE,
            language TEXT,
            synthesized INTEGER NOT NULL DEFAULT 0
         );
         CREATE TABLE transcript_segment (
            media_id TEXT NOT NULL REFERENCES media_item(id) ON DELETE CASCADE,
            idx INTEGER NOT NULL,
            start REAL NOT NULL,
            end REAL NOT NULL,
            text TEXT NOT NULL,
            speaker TEXT,
            PRIMARY KEY (media_id, idx)
         );
         CREATE TABLE note (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            body TEXT NOT NULL,
            date INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
         );
         CREATE INDEX note_date ON note(date);",
    ),
    M::up(
        "ALTER TABLE note ADD COLUMN folder TEXT NOT NULL DEFAULT '';
         ALTER TABLE note ADD COLUMN source_path TEXT;
         ALTER TABLE note ADD COLUMN content_hash TEXT;
         ALTER TABLE note ADD COLUMN slug TEXT;
         ALTER TABLE note ADD COLUMN properties TEXT NOT NULL DEFAULT '{}';
         CREATE INDEX note_hash ON note(content_hash);
         CREATE INDEX note_source ON note(source_path);
         ALTER TABLE media_item ADD COLUMN attachment INTEGER NOT NULL DEFAULT 0;
         CREATE TABLE graph_view (
            name TEXT PRIMARY KEY,
            json TEXT NOT NULL,
            updated_at INTEGER NOT NULL
         );
         CREATE TABLE node_position (
            node_id TEXT PRIMARY KEY,
            x REAL NOT NULL,
            y REAL NOT NULL
         );",
    )])
}

pub fn open(path: &std::path::Path) -> Result<Connection> {
    let mut c = Connection::open(path)?;
    c.pragma_update(None, "journal_mode", "WAL")?;
    init(&mut c)?;
    Ok(c)
}

pub fn open_in_memory() -> Result<Connection> {
    let mut c = Connection::open_in_memory()?;
    init(&mut c)?;
    Ok(c)
}

fn init(c: &mut Connection) -> Result<()> {
    c.pragma_update(None, "foreign_keys", "ON")?;
    migrations().to_latest(c).map_err(|e| crate::error::Error::InvalidArgument(format!("migration failed: {e}")))?;
    Ok(())
}

pub fn get_meta(c: &Connection, key: &str) -> Result<Option<String>> {
    Ok(c.query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0)).optional()?)
}
pub fn set_meta(c: &Connection, key: &str, value: &str) -> Result<()> {
    c.execute("INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [key, value])?;
    Ok(())
}

// ------------------------------------------------------------------- media

const MEDIA_COLS: &str = "m.id,m.kind,m.title,m.recorded_at,m.duration,m.sha256,m.file_size,m.imported_at,m.mime,m.original_name,m.ext,\
     EXISTS(SELECT 1 FROM transcript_meta t WHERE t.media_id=m.id)";

/// A media row plus the stored file extension (not part of the public model).
pub struct MediaRow {
    pub item: MediaItem,
    pub ext: String,
}

fn media_from_row(r: &Row) -> rusqlite::Result<MediaRow> {
    let kind: String = r.get(1)?;
    Ok(MediaRow {
        item: MediaItem {
            id: r.get(0)?,
            kind: MediaKind::parse(&kind).unwrap_or(MediaKind::Audio),
            title: r.get(2)?,
            recorded_at: r.get(3)?,
            duration: r.get(4)?,
            sha256: r.get(5)?,
            file_size: r.get(6)?,
            imported_at: r.get(7)?,
            mime: r.get(8)?,
            original_name: r.get(9)?,
            is_missing: false,
            has_transcript: r.get(11)?,
        },
        ext: r.get(10)?,
    })
}

pub fn insert_media(c: &Connection, m: &MediaItem, ext: &str) -> Result<()> {
    c.execute(
        "INSERT INTO media_item(id,kind,title,recorded_at,duration,sha256,file_size,imported_at,mime,original_name,ext)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![m.id, m.kind.as_str(), m.title, m.recorded_at, m.duration, m.sha256, m.file_size, m.imported_at, m.mime, m.original_name, ext],
    )?;
    Ok(())
}
pub fn list_media(c: &Connection) -> Result<Vec<MediaRow>> {
    let mut st = c.prepare(&format!("SELECT {MEDIA_COLS} FROM media_item m ORDER BY m.recorded_at, m.id"))?;
    let rows = st.query_map([], media_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
pub fn get_media(c: &Connection, id: &str) -> Result<Option<MediaRow>> {
    Ok(c.query_row(&format!("SELECT {MEDIA_COLS} FROM media_item m WHERE m.id=?1"), [id], media_from_row).optional()?)
}
pub fn find_media_by_sha(c: &Connection, sha: &str) -> Result<Option<MediaRow>> {
    Ok(c.query_row(&format!("SELECT {MEDIA_COLS} FROM media_item m WHERE m.sha256=?1"), [sha], media_from_row).optional()?)
}
pub fn update_media(c: &Connection, id: &str, title: Option<&str>, recorded_at: Option<Ms>) -> Result<usize> {
    let mut n = 0;
    if let Some(t) = title {
        n += c.execute("UPDATE media_item SET title=?2 WHERE id=?1", params![id, t])?;
    }
    if let Some(r) = recorded_at {
        n += c.execute("UPDATE media_item SET recorded_at=?2 WHERE id=?1", params![id, r])?;
    }
    Ok(n)
}
pub fn delete_media(c: &Connection, id: &str) -> Result<usize> {
    Ok(c.execute("DELETE FROM media_item WHERE id=?1", [id])?)
}

// -------------------------------------------------------------- transcripts

pub fn save_transcript(c: &mut Connection, media_id: &str, t: &Transcript) -> Result<()> {
    let tx = c.transaction()?;
    tx.execute("DELETE FROM transcript_segment WHERE media_id=?1", [media_id])?;
    tx.execute("DELETE FROM transcript_meta WHERE media_id=?1", [media_id])?;
    tx.execute("INSERT INTO transcript_meta(media_id,language,synthesized) VALUES(?1,?2,?3)", params![media_id, t.language, t.has_synthesized_timing])?;
    for s in &t.segments {
        tx.execute(
            "INSERT INTO transcript_segment(media_id,idx,start,end,text,speaker) VALUES(?1,?2,?3,?4,?5,?6)",
            params![media_id, s.id as i64, s.start, s.end, s.text, s.speaker],
        )?;
    }
    tx.commit()?;
    Ok(())
}
pub fn load_transcript(c: &Connection, media_id: &str) -> Result<Option<Transcript>> {
    let meta: Option<(Option<String>, bool)> =
        c.query_row("SELECT language,synthesized FROM transcript_meta WHERE media_id=?1", [media_id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
    let Some((language, synthesized)) = meta else { return Ok(None) };
    let mut st = c.prepare("SELECT idx,start,end,text,speaker FROM transcript_segment WHERE media_id=?1 ORDER BY idx")?;
    let segments = st
        .query_map([media_id], |r| Ok(TranscriptSegment { id: r.get::<_, i64>(0)? as usize, start: r.get(1)?, end: r.get(2)?, text: r.get(3)?, speaker: r.get(4)? }))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Some(Transcript { segments, language, has_synthesized_timing: synthesized }))
}
pub fn delete_transcript(c: &Connection, media_id: &str) -> Result<()> {
    c.execute("DELETE FROM transcript_segment WHERE media_id=?1", [media_id])?;
    c.execute("DELETE FROM transcript_meta WHERE media_id=?1", [media_id])?;
    Ok(())
}

// ------------------------------------------------------------------- notes

const NOTE_COLS: &str = "id,title,body,date,created_at,updated_at,folder,source_path,content_hash,slug,properties";
fn note_from_row(r: &Row) -> rusqlite::Result<Note> {
    let props: String = r.get(10)?;
    Ok(Note {
        id: r.get(0)?,
        title: r.get(1)?,
        body: r.get(2)?,
        date: r.get(3)?,
        created_at: r.get(4)?,
        updated_at: r.get(5)?,
        folder: r.get(6)?,
        source_path: r.get(7)?,
        content_hash: r.get(8)?,
        slug: r.get(9)?,
        properties: serde_json::from_str(&props).unwrap_or_default(),
    })
}
pub fn insert_note(c: &Connection, n: &Note) -> Result<()> {
    c.execute(
        "INSERT INTO note(id,title,body,date,created_at,updated_at,folder,source_path,content_hash,slug,properties) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![n.id, n.title, n.body, n.date, n.created_at, n.updated_at, n.folder, n.source_path, n.content_hash, n.slug, serde_json::to_string(&n.properties)?],
    )?;
    Ok(())
}
pub fn update_note(c: &Connection, n: &Note) -> Result<usize> {
    Ok(c.execute("UPDATE note SET title=?2, body=?3, date=?4, updated_at=?5 WHERE id=?1", params![n.id, n.title, n.body, n.date, n.updated_at])?)
}
pub fn get_note(c: &Connection, id: &str) -> Result<Option<Note>> {
    Ok(c.query_row(&format!("SELECT {NOTE_COLS} FROM note WHERE id=?1"), [id], note_from_row).optional()?)
}
pub fn list_notes(c: &Connection) -> Result<Vec<Note>> {
    let mut st = c.prepare(&format!("SELECT {NOTE_COLS} FROM note ORDER BY date, id"))?;
    let rows = st.query_map([], note_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
/// (content_hash set, source_path set) of all imported notes, for duplicate detection.
pub fn imported_keys(c: &Connection) -> Result<(std::collections::HashSet<String>, std::collections::HashSet<String>)> {
    let mut hashes = std::collections::HashSet::new();
    let mut paths = std::collections::HashSet::new();
    let mut st = c.prepare("SELECT content_hash, source_path FROM note WHERE content_hash IS NOT NULL OR source_path IS NOT NULL")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?)))?;
    for r in rows {
        let (h, p) = r?;
        hashes.extend(h);
        paths.extend(p);
    }
    Ok((hashes, paths))
}
pub fn set_media_attachment(c: &Connection, id: &str, attachment: bool) -> Result<()> {
    c.execute("UPDATE media_item SET attachment=?2 WHERE id=?1", params![id, attachment])?;
    Ok(())
}
pub fn attachment_ids(c: &Connection) -> Result<std::collections::HashSet<String>> {
    let mut st = c.prepare("SELECT id FROM media_item WHERE attachment=1")?;
    let v = st.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<_>>()?;
    Ok(v)
}
pub fn graph_views(c: &Connection) -> Result<Vec<(String, String, Ms)>> {
    let mut st = c.prepare("SELECT name, json, updated_at FROM graph_view ORDER BY name COLLATE NOCASE")?;
    let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
    Ok(v)
}
pub fn save_graph_view(c: &Connection, name: &str, json: &str, now: Ms) -> Result<()> {
    c.execute(
        "INSERT INTO graph_view(name,json,updated_at) VALUES(?1,?2,?3) ON CONFLICT(name) DO UPDATE SET json=?2, updated_at=?3",
        params![name, json, now],
    )?;
    Ok(())
}
pub fn delete_graph_view(c: &Connection, name: &str) -> Result<usize> {
    Ok(c.execute("DELETE FROM graph_view WHERE name=?1", [name])?)
}
pub fn node_positions(c: &Connection) -> Result<Vec<(String, f64, f64)>> {
    let mut st = c.prepare("SELECT node_id, x, y FROM node_position ORDER BY node_id")?;
    let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
    Ok(v)
}
/// Replaces all stored positions in one transaction.
pub fn replace_node_positions(c: &mut Connection, pos: &[(String, f64, f64)]) -> Result<()> {
    let tx = c.transaction()?;
    tx.execute("DELETE FROM node_position", [])?;
    {
        let mut st = tx.prepare("INSERT OR REPLACE INTO node_position(node_id,x,y) VALUES(?1,?2,?3)")?;
        for (id, x, y) in pos {
            if x.is_finite() && y.is_finite() {
                st.execute(params![id, x, y])?;
            }
        }
    }
    tx.commit()?;
    Ok(())
}
pub fn delete_note(c: &Connection, id: &str) -> Result<usize> {
    Ok(c.execute("DELETE FROM note WHERE id=?1", [id])?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media(id: &str, sha: &str) -> MediaItem {
        MediaItem { id: id.into(), kind: MediaKind::Audio, title: "T".into(), recorded_at: 5, duration: Some(3.0), sha256: sha.into(), file_size: 10, imported_at: 6, is_missing: false, has_transcript: false, mime: "audio/wav".into(), original_name: "a.wav".into() }
    }

    #[test]
    fn migrations_are_valid_and_idempotent() {
        assert!(migrations().validate().is_ok());
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.db");
        drop(open(&p).unwrap());
        drop(open(&p).unwrap()); // reopening must not fail
    }

    #[test]
    fn media_roundtrip_unique_sha_and_cascade() {
        let mut c = open_in_memory().unwrap();
        insert_media(&c, &media("a", "s1"), "wav").unwrap();
        assert!(insert_media(&c, &media("b", "s1"), "wav").is_err());
        let t = Transcript { segments: vec![TranscriptSegment { id: 0, start: 0.0, end: 1.0, text: "hi".into(), speaker: Some("Me".into()) }], language: Some("en".into()), has_synthesized_timing: false };
        save_transcript(&mut c, "a", &t).unwrap();
        let row = get_media(&c, "a").unwrap().unwrap();
        assert!(row.item.has_transcript);
        assert_eq!(row.ext, "wav");
        assert_eq!(load_transcript(&c, "a").unwrap().unwrap(), t);
        // replace
        let t2 = Transcript { segments: vec![], language: None, has_synthesized_timing: true };
        save_transcript(&mut c, "a", &t2).unwrap();
        assert_eq!(load_transcript(&c, "a").unwrap().unwrap(), t2);
        delete_media(&c, "a").unwrap();
        assert!(load_transcript(&c, "a").unwrap().is_none());
        let n: i64 = c.query_row("SELECT COUNT(*) FROM transcript_segment", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn notes_roundtrip_and_meta() {
        let c = open_in_memory().unwrap();
        let n = Note::basic("n", "T", "b", 1, 1);
        insert_note(&c, &n).unwrap();
        let mut n2 = n.clone();
        n2.body = "changed".into();
        assert_eq!(update_note(&c, &n2).unwrap(), 1);
        assert_eq!(get_note(&c, "n").unwrap().unwrap().body, "changed");
        assert_eq!(list_notes(&c).unwrap().len(), 1);
        assert_eq!(delete_note(&c, "n").unwrap(), 1);
        assert!(get_note(&c, "n").unwrap().is_none());
        assert_eq!(get_meta(&c, "k").unwrap(), None);
        set_meta(&c, "k", "1").unwrap();
        set_meta(&c, "k", "2").unwrap();
        assert_eq!(get_meta(&c, "k").unwrap().as_deref(), Some("2"));
    }
}
