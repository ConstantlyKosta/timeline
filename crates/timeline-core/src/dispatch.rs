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
struct NameArg {
    name: String,
}
#[derive(Deserialize)]
struct SaveGraphView {
    name: String,
    json: Value,
}
#[derive(Deserialize)]
struct SavePositions {
    positions: Vec<crate::library::NodePos>,
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
struct TranscribeStart {
    media_id: String,
    model: Option<String>,
    language: Option<String>,
}
#[derive(Deserialize)]
struct ModelArg {
    model: String,
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
        "transcribe_start" => {
            let r: TranscribeStart = args(a)?;
            lib.get_media(&r.media_id)?; // fail fast on unknown ids
            crate::asr::require(&lib.asr())?;
            let l = lib.clone();
            let subject = r.media_id.clone();
            let id = lib.jobs().spawn("transcribe", Some(subject), move |ctx| l.transcribe_media(&r.media_id, r.model.as_deref(), r.language.as_deref(), &ctx));
            Ok(json!({ "jobId": id }))
        }
        "asr_download_start" => {
            let r: ModelArg = args(a)?;
            let engine = lib.asr();
            let engine = crate::asr::require(&engine)?.clone();
            let model = r.model.clone();
            let id = lib.jobs().spawn("asr-download", Some(r.model), move |ctx| {
                let c = ctx.clone();
                let progress: crate::asr::ProgressFn = Arc::new(move |p, m| c.progress(p, m));
                let c = ctx.clone();
                let cancel: crate::asr::CancelFn = Arc::new(move || c.cancelled());
                engine.download(&model, progress, cancel)?;
                Ok(json!({ "model": model }))
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
        "graph_data" => out(lib.graph_data()?),
        "graph_views" => out(lib.graph_views()?),
        "save_graph_view" => {
            let r: SaveGraphView = args(a)?;
            lib.save_graph_view(&r.name, &r.json)?;
            Ok(Value::Null)
        }
        "delete_graph_view" => {
            lib.delete_graph_view(&args::<NameArg>(a)?.name)?;
            Ok(Value::Null)
        }
        "graph_positions" => out(lib.graph_positions()?),
        "save_graph_positions" => {
            let r: SavePositions = args(a)?;
            lib.save_graph_positions(&r.positions)?;
            Ok(Value::Null)
        }
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
        "asr_status" => out(lib.asr_status()?),
        "asr_set_settings" => {
            lib.set_asr_settings(&args::<crate::asr::AsrSettings>(a)?)?;
            out(lib.asr_status()?)
        }
        "asr_delete_model" => {
            let engine = lib.asr();
            crate::asr::require(&engine)?.delete_model(&args::<ModelArg>(a)?.model)?;
            out(lib.asr_status()?)
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
    fn graph_views_and_positions_persist() {
        let (_d, l) = lib();
        dispatch(&l, "create_note", json!({"body": "# A\n[[B]] #x", "date": 5})).unwrap();
        let g = dispatch(&l, "graph_data", json!({})).unwrap();
        assert_eq!(g["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(g["edges"][0]["kind"], "link");
        dispatch(&l, "save_graph_view", json!({"name": "Mine", "json": {"colorBy": "tag"}})).unwrap();
        dispatch(&l, "save_graph_view", json!({"name": "Mine", "json": {"colorBy": "folder"}})).unwrap();
        let v = dispatch(&l, "graph_views", json!({})).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!(v[0]["json"]["colorBy"], "folder");
        assert!(dispatch(&l, "save_graph_view", json!({"name": " ", "json": {}})).is_err());
        dispatch(&l, "delete_graph_view", json!({"name": "Mine"})).unwrap();
        assert!(dispatch(&l, "graph_views", json!({})).unwrap().as_array().unwrap().is_empty());
        dispatch(&l, "save_graph_positions", json!({"positions": [{"id": "a", "x": 1.5, "y": -2.0}, {"id": "b", "x": 3.0, "y": 4.0}]})).unwrap();
        dispatch(&l, "save_graph_positions", json!({"positions": [{"id": "b", "x": 9.0, "y": 4.0}]})).unwrap();
        let p = dispatch(&l, "graph_positions", json!({})).unwrap();
        assert_eq!(p.as_array().unwrap().len(), 1);
        assert_eq!(p[0]["x"], 9.0);
    }

    #[test]
    fn errors_are_reported_not_panicked() {
        let (_d, l) = lib();
        assert!(dispatch(&l, "nope", json!({})).unwrap_err().to_string().contains("unknown command"));
        assert!(dispatch(&l, "get_note", json!({})).unwrap_err().to_string().contains("Invalid argument"));
        assert!(dispatch(&l, "get_note", json!({"id": "x"})).unwrap_err().to_string().starts_with("Not found"));
    }

    fn wait_job(l: &Arc<Library>, id: &str) -> Value {
        for _ in 0..400 {
            let s = dispatch_shared(l, "job_status", json!({ "jobId": id })).unwrap();
            if s["state"] != "running" {
                return s;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("job did not finish");
    }

    #[test]
    fn transcription_flow_with_a_fake_engine() {
        let d = tempfile::tempdir().unwrap();
        let l = Arc::new(Library::open_with_clock(d.path(), Box::new(|| 1_760_000_000_000)).unwrap());
        // no engine plugged in: clear error, status says unavailable
        assert_eq!(dispatch(&l, "asr_status", json!({})).unwrap()["available"], false);
        let wav = crate::media::synth_wav(3.0, 1.0, 300.0);
        let id = l.import_media("a.wav", &wav, Some(5)).unwrap().item.id;
        assert!(dispatch_shared(&l, "transcribe_start", json!({ "mediaId": id })).unwrap_err().to_string().contains("not available"));

        l.set_asr(Arc::new(crate::asr::FakeAsr::new(1)));
        let st = dispatch(&l, "asr_status", json!({})).unwrap();
        assert_eq!(st["available"], true);
        assert_eq!(st["settings"]["model"], "base.en");
        assert_eq!(st["settings"]["autoTranscribe"], false);

        // model not downloaded yet -> the job fails with a helpful message
        let j = dispatch_shared(&l, "transcribe_start", json!({ "mediaId": id })).unwrap();
        let r = wait_job(&l, j["jobId"].as_str().unwrap());
        assert_eq!(r["state"], "error");
        assert!(r["error"].as_str().unwrap().contains("not downloaded"));

        // download (job with progress), then transcribe
        let j = dispatch_shared(&l, "asr_download_start", json!({ "model": "base.en" })).unwrap();
        assert_eq!(wait_job(&l, j["jobId"].as_str().unwrap())["state"], "done");
        let j = dispatch_shared(&l, "transcribe_start", json!({ "mediaId": id })).unwrap();
        let r = wait_job(&l, j["jobId"].as_str().unwrap());
        assert_eq!(r["state"], "done", "{r}");
        assert_eq!(r["result"]["segments"], 3, "blank marker dropped");
        assert_eq!(r["result"]["language"], "en");
        let t = l.get_transcript(&id).unwrap().unwrap();
        assert_eq!(t.segments.len(), 3);
        assert!(!t.has_synthesized_timing);
        assert_eq!(t.segments[1].text, "This second line proves segments are stored.");
        assert!(t.segments[2].start > t.segments[1].start);

        // settings persist and are validated
        let st = dispatch(&l, "asr_set_settings", json!({ "model": "base", "language": "ru", "autoTranscribe": true })).unwrap();
        assert_eq!(st["settings"]["language"], "ru");
        assert_eq!(l.asr_settings().unwrap().auto_transcribe, true);
        assert!(dispatch(&l, "asr_set_settings", json!({ "model": "nope", "language": "en", "autoTranscribe": false })).is_err());
        assert!(dispatch(&l, "asr_set_settings", json!({ "model": "base", "language": "../x", "autoTranscribe": false })).is_err());

        // deleting a model flips it back to not installed
        let st = dispatch(&l, "asr_delete_model", json!({ "model": "base.en" })).unwrap();
        assert!(st["models"].as_array().unwrap().iter().all(|m| m["installed"] == false));
    }

    #[test]
    fn transcription_can_be_cancelled_and_rejects_photos() {
        let d = tempfile::tempdir().unwrap();
        let l = Arc::new(Library::open_with_clock(d.path(), Box::new(|| 1_760_000_000_000)).unwrap());
        l.set_asr(Arc::new(crate::asr::FakeAsr::new(60)));
        let engine = l.asr().unwrap();
        engine.download("base.en", Arc::new(|_, _| {}), Arc::new(|| false)).unwrap();
        let wav = crate::media::synth_wav(3.0, 1.0, 300.0);
        let id = l.import_media("a.wav", &wav, Some(5)).unwrap().item.id;
        let j = dispatch_shared(&l, "transcribe_start", json!({ "mediaId": id })).unwrap();
        let jid = j["jobId"].as_str().unwrap().to_string();
        dispatch_shared(&l, "job_cancel", json!({ "jobId": jid })).unwrap();
        let r = wait_job(&l, &jid);
        assert_eq!(r["state"], "cancelled");
        assert!(l.get_transcript(&id).unwrap().is_none(), "nothing stored when cancelled");
    }
}
