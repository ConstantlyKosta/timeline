//! JSON command dispatch. The Tauri commands and the dev HTTP bridge both funnel through
//! [`dispatch`], so there is exactly one command surface. Binary commands (importing media bytes,
//! serving media) are handled by the callers using [`Library::import_media`] / [`Library::media_file`].

use crate::error::{Error, Result};
use crate::library::{system_now_ms, Library};
use crate::models::Ms;
use crate::sample;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

fn args<T: DeserializeOwned>(v: Value) -> Result<T> {
    serde_json::from_value(v).map_err(|e| Error::InvalidArgument(e.to_string()))
}
fn out<T: serde::Serialize>(v: T) -> Result<Value> {
    Ok(serde_json::to_value(v)?)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Id {
    id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Tz {
    #[serde(default)]
    tz_offset_minutes: i32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateNote {
    #[serde(default)]
    body: String,
    date: Option<Ms>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateNote {
    id: String,
    body: Option<String>,
    date: Option<Ms>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ToggleTask {
    id: String,
    line: usize,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenByTitle {
    title: String,
    date: Option<Ms>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateMedia {
    id: String,
    title: Option<String>,
    recorded_at: Option<Ms>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttachTranscript {
    media_id: String,
    file_name: String,
    text: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MediaId {
    media_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Search {
    query: String,
    limit: Option<usize>,
}

/// Seeds the sample content on the very first launch (an empty library that never had samples).
pub fn bootstrap(lib: &Library, tz_min: i32) -> Result<Value> {
    let mut seeded = false;
    if lib.meta("sample_seeded")?.is_none() {
        if lib.is_empty()? {
            sample::seed(lib, lib.now(), tz_min)?;
            seeded = true;
        }
        lib.set_meta("sample_seeded", "1")?;
    }
    Ok(json!({ "seeded": seeded, "stats": lib.stats()? }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportStart {
    paths: Vec<String>,
    #[serde(default)]
    tz_offset_minutes: i32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportInline {
    files: Vec<crate::import::InlineFile>,
    #[serde(default)]
    vaults: Vec<String>,
    #[serde(default)]
    tz_offset_minutes: i32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobId {
    job_id: String,
}

/// Like [`dispatch`], plus the commands that start background jobs (they need a shared library
/// handle to move into the worker thread): `import_start`, `job_status`, `job_cancel`, `job_list`.
pub fn dispatch_shared(lib: &Arc<Library>, name: &str, a: Value) -> Result<Value> {
    match name {
        "import_start" => {
            let r: ImportStart = args(a)?;
            if r.paths.is_empty() {
                return Err(Error::InvalidArgument("no files or folders given".into()));
            }
            let l = lib.clone();
            let id = lib.jobs().spawn("import", None, move |ctx| {
                let s = crate::import::import_paths(&l, &r.paths, r.tz_offset_minutes, &ctx)?;
                Ok(serde_json::to_value(s)?)
            });
            Ok(json!({ "jobId": id }))
        }
        "job_status" => {
            let id = args::<JobId>(a)?.job_id;
            lib.jobs().status(&id).map(|s| serde_json::to_value(s).map_err(Error::from)).unwrap_or_else(|| Err(Error::NotFound(format!("job {id}"))))
        }
        "job_cancel" => Ok(json!(lib.jobs().cancel(&args::<JobId>(a)?.job_id))),
        "job_list" => out(lib.jobs().list()),
        _ => dispatch(lib, name, a),
    }
}

pub fn dispatch(lib: &Library, name: &str, a: Value) -> Result<Value> {
    match name {
        "import_inline" => {
            let r: ImportInline = args(a)?;
            out(crate::import::import_inline(lib, &r.files, &r.vaults, r.tz_offset_minutes)?)
        }
        "bootstrap" => bootstrap(lib, args::<Tz>(a)?.tz_offset_minutes),
        "load_sample_data" => {
            sample::seed(lib, system_now_ms(), args::<Tz>(a)?.tz_offset_minutes)?;
            out(lib.stats()?)
        }
        "stats" => out(lib.stats()?),
        "timeline_entries" => out(lib.timeline_entries()?),
        "list_notes" => out(lib.list_notes()?),
        "get_note" => out(lib.get_note(&args::<Id>(a)?.id)?),
        "note_titles" => out(lib.note_titles()?),
        "list_tags" => out(lib.list_tags()?),
        "create_note" => {
            let r: CreateNote = args(a)?;
            out(lib.create_note(&r.body, r.date)?)
        }
        "update_note" => {
            let r: UpdateNote = args(a)?;
            out(lib.update_note(&r.id, r.body.as_deref(), r.date)?)
        }
        "delete_note" => {
            lib.delete_note(&args::<Id>(a)?.id)?;
            Ok(Value::Null)
        }
        "toggle_task" => {
            let r: ToggleTask = args(a)?;
            out(lib.toggle_task(&r.id, r.line)?)
        }
        "open_note_by_title" => {
            let r: OpenByTitle = args(a)?;
            out(lib.open_note_by_title(&r.title, r.date)?)
        }
        "note_context" => out(lib.note_context(&args::<Id>(a)?.id)?),
        "graph" => out(lib.graph()?),
        "search" => {
            let r: Search = args(a)?;
            out(lib.search(&r.query, r.limit.unwrap_or(30))?)
        }
        "list_media" => out(lib.list_media()?),
        "get_media" => out(lib.get_media(&args::<Id>(a)?.id)?),
        "update_media" => {
            let r: UpdateMedia = args(a)?;
            out(lib.update_media(&r.id, r.title.as_deref(), r.recorded_at)?)
        }
        "delete_media" => {
            lib.delete_media(&args::<Id>(a)?.id)?;
            Ok(Value::Null)
        }
        "attach_transcript" => {
            let r: AttachTranscript = args(a)?;
            out(lib.attach_transcript(&r.media_id, &r.file_name, &r.text)?)
        }
        "get_transcript" => out(lib.get_transcript(&args::<MediaId>(a)?.media_id)?),
        "remove_transcript" => {
            lib.remove_transcript(&args::<MediaId>(a)?.media_id)?;
            Ok(Value::Null)
        }
        "export_transcript" => out(lib.export_transcript_text(&args::<MediaId>(a)?.media_id)?),
        "export_markdown" => out(lib.export_markdown()?),
        other => Err(Error::InvalidArgument(format!("unknown command: {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib() -> (tempfile::TempDir, Library) {
        let d = tempfile::tempdir().unwrap();
        let l = Library::open_with_clock(d.path(), Box::new(|| 1_760_000_000_000)).unwrap();
        (d, l)
    }

    #[test]
    fn bootstrap_seeds_once_even_after_user_deletes_everything() {
        let (_d, l) = lib();
        let r = dispatch(&l, "bootstrap", json!({"tzOffsetMinutes": 180})).unwrap();
        assert_eq!(r["seeded"], true);
        assert_eq!(r["stats"]["notes"], 7);
        let r = dispatch(&l, "bootstrap", json!({"tzOffsetMinutes": 180})).unwrap();
        assert_eq!(r["seeded"], false);
        for n in l.list_notes().unwrap() {
            l.delete_note(&n.id).unwrap();
        }
        let r = dispatch(&l, "bootstrap", json!({})).unwrap();
        assert_eq!(r["seeded"], false);
        assert_eq!(r["stats"]["notes"], 0);
    }

    #[test]
    fn bootstrap_does_not_seed_a_library_that_already_has_content() {
        let (_d, l) = lib();
        l.create_note("# Mine", None).unwrap();
        assert_eq!(dispatch(&l, "bootstrap", json!({})).unwrap()["seeded"], false);
        assert_eq!(l.list_notes().unwrap().len(), 1);
    }

    #[test]
    fn commands_roundtrip_camel_case_json() {
        let (_d, l) = lib();
        let n = dispatch(&l, "create_note", json!({"body": "# Hi\n- [ ] a", "date": 5})).unwrap();
        assert_eq!(n["title"], "Hi");
        assert_eq!(n["createdAt"], 1_760_000_000_000i64);
        let id = n["id"].as_str().unwrap();
        let t = dispatch(&l, "toggle_task", json!({"id": id, "line": 1})).unwrap();
        assert!(t["body"].as_str().unwrap().contains("[x]"));
        let ctx = dispatch(&l, "note_context", json!({"id": id})).unwrap();
        assert_eq!(ctx["tasks"][0]["isDone"], true);
        let hits = dispatch(&l, "search", json!({"query": "hi"})).unwrap();
        assert_eq!(hits[0]["ownerId"], id);
        dispatch(&l, "delete_note", json!({"id": id})).unwrap();
        assert!(dispatch(&l, "get_note", json!({"id": id})).is_err());
    }

    #[test]
    fn errors_are_reported_not_panicked() {
        let (_d, l) = lib();
        assert!(dispatch(&l, "nope", json!({})).unwrap_err().to_string().contains("unknown command"));
        assert!(dispatch(&l, "get_note", json!({})).unwrap_err().to_string().contains("Invalid argument"));
        assert!(dispatch(&l, "get_note", json!({"id": "x"})).unwrap_err().to_string().starts_with("Not found"));
    }
}
