//! End-to-end import of `fixtures/vault` (an Obsidian-style vault) into a real library.

use timeline_core::dispatch::dispatch;
use timeline_core::import::{import_inline, import_paths, InlineFile};
use timeline_core::jobs::JobCtx;
use timeline_core::Library;

fn vault() -> String {
    format!("{}/../../fixtures/vault", env!("CARGO_MANIFEST_DIR"))
}
fn lib(dir: &std::path::Path) -> Library {
    Library::open_with_clock(dir, Box::new(|| 1_800_000_000_000)).unwrap()
}

#[test]
fn imports_vault_with_front_matter_links_attachments_and_dedupe() {
    let dir = tempfile::tempdir().unwrap();
    let lib = lib(dir.path());
    let s = import_paths(&lib, &[vault()], 120, &JobCtx::detached()).unwrap();

    // Welcome, Atlas, Roadmap, Ideas, Reading list, Daily, Заметки = 7; "Atlas copy" is a duplicate, binary.md is an error.
    assert_eq!(s.imported, 7, "{s:#?}");
    assert_eq!(s.skipped_duplicate, 1);
    assert_eq!(s.error_count, 1);
    assert!(s.errors[0].path.contains("binary.md"));
    assert_eq!(s.vaults, vec!["vault".to_string()]);
    assert_eq!(s.attachments_imported, 2);
    assert_eq!(s.attachments_missing, 0);
    assert!(s.folders >= 1);
    // .obsidian/.trash content was never even seen
    assert_eq!(s.files_seen, 9);

    let notes = lib.list_notes().unwrap();
    assert_eq!(notes.len(), 7);
    let by = |t: &str| notes.iter().find(|n| n.title == t).unwrap_or_else(|| panic!("no note {t}: {:?}", notes.iter().map(|n| &n.title).collect::<Vec<_>>())).clone();

    let welcome = by("Welcome to the vault");
    assert_eq!(welcome.tags, vec!["intro", "getting started"]);
    assert_eq!(welcome.folder, "");

    let atlas = by("Atlas");
    assert_eq!(atlas.folder, "Projects");
    assert!(atlas.tags.contains(&"project".to_string()) && atlas.tags.contains(&"roadmap".to_string()));

    let full = lib.get_note(&atlas.id).unwrap();
    assert!(!full.body.starts_with("---"), "front matter is stripped from the body");
    assert!(full.body.contains("](media:"), "image rewritten: {}", full.body);
    assert_eq!(full.properties["status"], "in-progress");
    assert_eq!(full.slug.as_deref(), Some("Atlas"));
    // 2024-05-01 09:30 at UTC+2
    assert_eq!(full.date, 1_714_548_600_000);
    assert_eq!(by("Welcome to the vault").date, 1_707_559_200_000); // date-only => 12:00 local
    assert_eq!(by("Reading list").date, 1_703_433_600_000); // `created:` key, 18:00 local
    // file name date for daily notes (title falls back to the file name)
    let daily = &by("2024-03-05 Daily");
    assert_eq!(daily.date, 1_709_632_800_000);
    assert_eq!(daily.title, "2024-03-05 Daily");

    // no heading, no front matter -> file name is the title
    let ideas = &by("Ideas");
    assert_eq!(ideas.title, "Ideas");
    assert!(ideas.tags.contains(&"brainstorm".to_string()));
    // Unicode survives
    assert!(notes.iter().any(|n| n.title == "Заметки" && n.tags == vec!["заметки".to_string()]));

    // links resolve by title, file name, alias, folder path, alias with `|label` and `#heading`
    let g = lib.graph().unwrap();
    let id_of = |t: &str| g.nodes.iter().find(|n| n.title == t).map(|n| n.id.clone());
    let ghosts: Vec<_> = g.nodes.iter().filter(|n| n.is_ghost).map(|n| n.title.clone()).collect();
    assert_eq!(ghosts, vec!["Nowhere".to_string()], "only the deliberately broken link is unresolved");
    let linked = |from: &str, to: &str| g.edges.iter().any(|e| e.kind == timeline_core::graph::EdgeKind::Link && Some(&e.from) == id_of(from).as_ref() && Some(&e.to) == id_of(to).as_ref());
    assert!(linked("Welcome to the vault", "Atlas"), "[[Projects/Atlas]]");
    assert!(linked("Welcome to the vault", "Ideas"), "[[Ideas|my ideas]]");
    assert!(linked("Welcome to the vault", "Reading list"), "[[Reading list#Books]]");
    assert!(linked("Roadmap", "Atlas"), "[[Atlas project]] resolves through the alias");
    assert!(linked("Atlas", "Welcome to the vault"));
    assert_eq!(s.links_unresolved, 1);

    // backlinks use file name / alias as well
    let ctx = lib.note_context(&atlas.id).unwrap();
    let back: Vec<_> = ctx.backlinks.iter().map(|b| b.title.clone()).collect();
    assert!(back.contains(&"Welcome to the vault".to_string()) && back.contains(&"Roadmap".to_string()), "{back:?}");

    // attachments are stored but hidden from media lists/timeline
    assert!(lib.list_media().unwrap().is_empty());
    assert_eq!(lib.timeline_entries().unwrap().len(), 7);
    let entry = lib.timeline_entries().unwrap().into_iter().find(|e| e.title == "Atlas").unwrap();
    assert!(entry.image.is_some() && entry.folder == "Projects");
    assert!(entry.preview.contains("Atlas maps"));
    eprintln!("preview: {:?}", entry.preview);
    assert!(!entry.preview.contains("**") && !entry.preview.contains("media:") && !entry.preview.contains("!["));

    // re-importing the same folder skips everything
    let again = import_paths(&lib, &[vault()], 120, &JobCtx::detached()).unwrap();
    assert_eq!(again.imported, 0, "{again:#?}");
    assert_eq!(again.skipped_duplicate, 8);
    assert_eq!(lib.list_notes().unwrap().len(), 7);
}

#[test]
fn single_files_and_unsupported_paths() {
    let dir = tempfile::tempdir().unwrap();
    let lib = lib(dir.path());
    let one = format!("{}/Ideas.md", vault());
    let s = import_paths(&lib, &[one, format!("{}/assets/chart.png", vault()), "/definitely/not/here".into()], 0, &JobCtx::detached()).unwrap();
    assert_eq!(s.imported, 1);
    assert_eq!(s.skipped_other, 1);
    assert_eq!(s.error_count, 1);
}

#[test]
fn survives_persistence_and_edits_keep_imported_title() {
    let dir = tempfile::tempdir().unwrap();
    {
        let lib = lib(dir.path());
        import_paths(&lib, &[vault()], 0, &JobCtx::detached()).unwrap();
    }
    let lib = lib(dir.path());
    let notes = lib.list_notes().unwrap();
    assert_eq!(notes.len(), 7);
    let ideas = lib.get_note(&notes.iter().find(|n| n.title == "Ideas").unwrap().id).unwrap();
    // editing the body of a note without heading must not turn its title into the first line
    let updated = lib.update_note(&ideas.id, Some("changed text without heading"), None).unwrap();
    assert_eq!(updated.title, "Ideas");
    assert_eq!(updated.folder, "");
    // search sees imported text and front matter tags
    let hits = lib.search("graph", 5).unwrap();
    assert!(!hits.is_empty());
    // export puts properties back into front matter
    let ex = lib.export_markdown().unwrap();
    let atlas = std::fs::read_to_string(std::path::Path::new(&ex.dir).join("Atlas.md")).unwrap();
    assert!(atlas.contains("status: in-progress") && atlas.contains("- project"), "{atlas}");
}

#[test]
fn inline_import_for_browsers_and_dispatch_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let lib = std::sync::Arc::new(lib(dir.path()));
    let files = vec![
        InlineFile { path: "Notes/a.md".into(), text: "---\ntitle: A\n---\nhello [[b]]".replace("\\n", "\n"), modified_ms: Some(1_700_000_000_000) },
        InlineFile { path: "Notes/b.md".into(), text: "# B\n\nworld".replace("\\n", "\n"), modified_ms: None },
        InlineFile { path: "Notes/.hidden/c.md".into(), text: "x".into(), modified_ms: None },
        InlineFile { path: "pic.png".into(), text: "x".into(), modified_ms: None },
    ];
    let s = import_inline(&lib, &files, &[], 0).unwrap();
    assert_eq!((s.imported, s.skipped_other), (2, 1));
    assert_eq!(lib.list_notes().unwrap().iter().filter(|n| n.folder == "Notes").count(), 2);

    // the same through the job API
    let r = dispatch(&lib, "import_start", serde_json::json!({"paths": [vault()]}));
    assert!(r.is_err(), "import_start needs the shared dispatcher");
    let r = timeline_core::dispatch::dispatch_shared(&lib, "import_start", serde_json::json!({"paths": [vault()], "tzOffsetMinutes": 0})).unwrap();
    let id = r["jobId"].as_str().unwrap().to_string();
    let mut last = serde_json::Value::Null;
    for _ in 0..500 {
        last = timeline_core::dispatch::dispatch_shared(&lib, "job_status", serde_json::json!({"jobId": id})).unwrap();
        if last["state"] != "running" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(last["state"], "done", "{last}");
    assert_eq!(last["result"]["imported"], 7);
}

#[test]
fn thousands_of_notes_import_quickly() {
    let dir = tempfile::tempdir().unwrap();
    let src = tempfile::tempdir().unwrap();
    for i in 0..3000 {
        let sub = src.path().join(format!("topic{}", i % 20));
        std::fs::create_dir_all(&sub).unwrap();
        let body = format!("---\ntags: [t{}]\n---\n# Note {i}\n\nSome text number {i} linking to [[Note {}]] and [[Note {}]].\n", i % 7, (i + 1) % 3000, (i * 7) % 3000).replace("\\n", "\n");
        std::fs::write(sub.join(format!("Note {i}.md")), body).unwrap();
    }
    let lib = lib(dir.path());
    let t = std::time::Instant::now();
    let s = import_paths(&lib, &[src.path().to_string_lossy().into_owned()], 0, &JobCtx::detached()).unwrap();
    let ms = t.elapsed().as_millis();
    assert_eq!(s.imported, 3000);
    assert_eq!(s.links_unresolved, 0);
    assert_eq!(s.folders, 20);
    eprintln!("imported 3000 notes in {ms} ms");
    assert!(ms < 30_000, "import took {ms} ms");
}
