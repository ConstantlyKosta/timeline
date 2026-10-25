//! Built-in sample content so a fresh install is not an empty screen: 7 notes, 4 recordings with
//! transcripts (synthetic tone audio) and 1 photo, spread over about six weeks.

use crate::error::Result;
use crate::library::Library;
use crate::media;
use crate::models::*;

const DAY: i64 = 86_400_000;

/// Local `hh:mm` on the day `days_ago` days before `now`, as epoch ms (`tz_min` = minutes east of UTC).
pub fn local_time(now: Ms, tz_min: i32, days_ago: i64, hour: i64, minute: i64) -> Ms {
    let tz = tz_min as i64 * 60_000;
    let local_midnight = (now + tz).div_euclid(DAY) * DAY - tz;
    local_midnight - days_ago * DAY + (hour * 60 + minute) * 60_000
}

struct SampleNote {
    id: &'static str,
    days_ago: i64,
    hour: i64,
    body: &'static str,
}

const NOTES: &[SampleNote] = &[
    SampleNote { id: "sample-note-welcome", days_ago: 0, hour: 9, body: "# Welcome to Timeline\n\nTimeline keeps **notes**, **recordings** and **photos** on one zoomable line, entirely on your device.\n\n## Try this\n- [x] Open the Timeline view and switch between Month, Week and Day\n- [ ] Click a cluster to zoom into it\n- [ ] Press `Ctrl/Cmd+K` to jump to any note\n- [ ] Type `[[` here to link to [[Lisbon Trip]] or another note\n- [ ] Search for *worn out* - it finds notes about being tired, even without those words\n\n## Markdown cheat sheet\n| Syntax | Result |\n| --- | --- |\n| `**bold**` | **bold** |\n| `~~gone~~` | ~~gone~~ |\n| `#tag` | a tag |\n\n```rust\nfn main() {\n    println!(\"hello, timeline\");\n}\n```\n\n#guide" },
    SampleNote { id: "sample-note-lisbon", days_ago: 40, hour: 12, body: "# Lisbon Trip\n\nThree days of walking, **tiles**, and pastries. The recording *Lisbon Walk* was made on the first afternoon.\n\n## Itinerary\n| Day | Plan | Done |\n| --- | --- | --- |\n| 1 | Alfama and the tram | yes |\n| 2 | Belem and pasteis de nata | yes |\n| 3 | Sintra day trip | no |\n\n## To do\n- [x] Book flights\n- [x] Buy pastries for the family\n- [ ] Print photos for [[Call with Mom]]\n\nMore ideas live in [[Travel Ideas]].\n\n#travel #family" },
    SampleNote { id: "sample-note-travel", days_ago: 33, hour: 12, body: "# Travel Ideas\n\nPlaces for the next year:\n\n1. Japan in spring\n2. A slow train through the Alps\n3. Back to Portugal - see [[Lisbon Trip]]\n\n> The journey is the reward.\n\nAlso: [[Japan in Spring]] needs its own note.\n\n#travel" },
    SampleNote { id: "sample-note-reading", days_ago: 24, hour: 12, body: "# Reading List\n\n- [x] *The Overstory*\n- [ ] *Four Thousand Weeks*\n- [ ] ~~Another productivity book~~\n\nQuote to remember: \"Attention is the rarest form of generosity.\"\n\n#books" },
    SampleNote { id: "sample-note-atlas", days_ago: 27, hour: 12, body: "# Project Atlas Kickoff\n\nThe client wants a prototype in three weeks. The **deadline pressure** is real.\n\n## Milestones\n| Week | Goal |\n| --- | --- |\n| 1 | Plan and budget |\n| 2 | Prototype |\n| 3 | Demo |\n\n- [ ] Write the plan and share it with the team\n- [ ] Talk to finance about the invoice\n- [ ] Ask for help early\n\n#work" },
    SampleNote { id: "sample-note-burnout", days_ago: 14, hour: 12, body: "# Burnout Notes\n\nTwo weeks after the [[Project Atlas Kickoff]] I feel exhausted and drained. Couldn't focus, slept badly, skipped lunch.\n\nWhat helps:\n- a walk by the river\n- cooking dinner properly\n- going to bed early\n\nSee my [[Weekly Review]].\n\n#health #work" },
    SampleNote { id: "sample-note-review", days_ago: 7, hour: 12, body: "# Weekly Review\n\n## Wins\n- Shipped the prototype plan\n- Called family\n\n## Next week\n- [ ] Rest first, then plan\n- [ ] Finish [[Reading List]]\n- [ ] Revisit [[Burnout Notes]]\n\n#review" },
];

struct SampleRec {
    id: &'static str,
    title: &'static str,
    days_ago: i64,
    hour: i64,
    minute: i64,
    hz: f64,
    lines: &'static [(&'static str, &'static str)],
}

const RECS: &[SampleRec] = &[
    SampleRec { id: "sample-rec-lisbon", title: "Lisbon Walk", days_ago: 39, hour: 16, minute: 30, hz: 196.0, lines: &[
        ("", "We just got off the tram and the tiles on these buildings are unreal."),
        ("", "The light here in the late afternoon is golden, everything looks warm."),
        ("", "I can smell grilled sardines from somewhere down the hill."),
        ("", "Remember to buy pastries from that little bakery near Alfama."),
        ("", "My legs are tired from all the hills but I am so happy."),
        ("", "Dinner tonight with the cousins, the whole family together."),
    ] },
    SampleRec { id: "sample-rec-mom", title: "Call with Mom", days_ago: 30, hour: 19, minute: 5, hz: 233.0, lines: &[
        ("Mom", "Hi sweetheart, how was your week?"),
        ("Me", "Busy, honestly. Work has been a lot and I am exhausted."),
        ("Mom", "You need to rest. Come for dinner on Sunday, I will cook."),
        ("Me", "I would love that. I miss your cooking and the whole family."),
        ("Mom", "Your father keeps asking about the trip. Tell us everything."),
        ("Me", "Lisbon was wonderful, I will bring photos."),
    ] },
    SampleRec { id: "sample-rec-atlas", title: "Morning thoughts on Atlas", days_ago: 12, hour: 8, minute: 40, hz: 175.0, lines: &[
        ("", "Morning. Thinking about the Atlas project kickoff and the deadline pressure."),
        ("", "The client wants a prototype in three weeks, that is stressful."),
        ("", "I should split the work into small milestones and ask for help."),
        ("", "Budget is tight too, I need to talk to finance about the invoice."),
        ("", "First step today: write the plan and share it with the team."),
        ("", "Feeling anxious but also excited about the challenge."),
    ] },
    SampleRec { id: "sample-rec-evening", title: "Evening reflection", days_ago: 5, hour: 21, minute: 15, hz: 208.0, lines: &[
        ("", "It has been a long week and I feel drained."),
        ("", "I skipped lunch twice and barely slept, classic burnout signs."),
        ("", "Tomorrow I will take a walk by the river and rest."),
        ("", "Grateful for friends who checked in on me today."),
        ("", "I want to cook a proper dinner and go to bed early."),
        ("", "Small things bring joy: tea, music, a quiet evening."),
    ] },
];

pub const SECONDS_PER_LINE: f64 = 8.0;

/// Seeds the sample content. Idempotent: items already present (by fixed id / content hash) are skipped.
pub fn seed(lib: &Library, now: Ms, tz_min: i32) -> Result<()> {
    for n in NOTES {
        if lib.get_note(n.id).is_err() {
            lib.create_note_with_id(n.id, n.body, Some(local_time(now, tz_min, n.days_ago, n.hour, 15)))?;
        }
    }
    for r in RECS {
        if lib.get_media(r.id).is_ok() {
            continue;
        }
        let total = r.lines.len() as f64 * SECONDS_PER_LINE;
        let wav = media::synth_wav(total, SECONDS_PER_LINE, r.hz);
        let at = local_time(now, tz_min, r.days_ago, r.hour, r.minute);
        let out = lib.import_media_inner(&format!("{}.wav", r.title.replace(' ', "_")), &wav, Some(at), Some(r.title), Some(r.id))?;
        let segments = r
            .lines
            .iter()
            .enumerate()
            .map(|(i, (sp, text))| TranscriptSegment {
                id: i,
                start: i as f64 * SECONDS_PER_LINE,
                end: (i + 1) as f64 * SECONDS_PER_LINE,
                text: text.to_string(),
                speaker: if sp.is_empty() { None } else { Some(sp.to_string()) },
            })
            .collect();
        lib.store_transcript(&out.item.id, &Transcript { segments, language: Some("en".into()), has_synthesized_timing: false })?;
    }
    if lib.get_media("sample-photo-tiles").is_err() {
        let png = media::synth_png(320, 200, 7);
        lib.import_media_inner("Alfama_tiles.png", &png, Some(local_time(now, tz_min, 39, 17, 2)), Some("Alfama tiles"), Some("sample-photo-tiles"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::SearchDocKind;

    const NOW: Ms = 1_760_000_000_000; // 2025-10-09T08:53:20Z

    #[test]
    fn local_time_handles_zones_and_midnight() {
        // In UTC+3, local midnight is 21:00 UTC the previous day.
        let t = local_time(NOW, 180, 0, 12, 15);
        let d = chrono::DateTime::from_timestamp_millis(t).unwrap();
        assert_eq!(d.format("%Y-%m-%d %H:%M").to_string(), "2025-10-09 09:15");
        let t = local_time(NOW, -480, 1, 0, 0); // UTC-8, one day ago, local midnight
        let d = chrono::DateTime::from_timestamp_millis(t).unwrap();
        assert_eq!(d.format("%Y-%m-%d %H:%M").to_string(), "2025-10-08 08:00");
    }

    #[test]
    fn seed_creates_expected_content_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open_with_clock(dir.path(), Box::new(|| NOW)).unwrap();
        seed(&lib, NOW, 180).unwrap();
        seed(&lib, NOW, 180).unwrap();
        let s = lib.stats().unwrap();
        assert_eq!((s.notes, s.recordings, s.photos, s.transcripts), (7, 4, 1, 4));
        let rec = lib.get_media("sample-rec-mom").unwrap();
        assert!((rec.duration.unwrap() - 48.0).abs() < 0.05);
        // The spec'd semantic behaviour: "worn out" finds fatigue content by meaning.
        let hits = lib.search("worn out", 10).unwrap();
        let owners: Vec<&str> = hits.iter().map(|h| h.owner_id.as_str()).collect();
        assert!(owners.contains(&"sample-note-burnout"), "{owners:?}");
        assert!(owners.contains(&"sample-rec-evening"), "{owners:?}");
        // A transcript segment hit carries a seek time.
        let seg = lib.search("sardines", 5).unwrap().into_iter().find(|h| h.kind == SearchDocKind::TranscriptSegment).unwrap();
        assert_eq!((seg.owner_id.as_str(), seg.time), ("sample-rec-lisbon", Some(16.0)));
        // Links resolve and ghosts exist ("Japan in Spring" has no note).
        let g = lib.graph().unwrap();
        assert!(g.nodes.iter().any(|n| n.is_ghost && n.title == "Japan in Spring"));
        let ctx = lib.note_context("sample-note-lisbon").unwrap();
        let back: Vec<&str> = ctx.backlinks.iter().map(|b| b.title.as_str()).collect();
        assert_eq!(back, vec!["Travel Ideas", "Welcome to Timeline"]);
        // everything lands within ~6 weeks before now
        let e = lib.timeline_entries().unwrap();
        assert_eq!(e.len(), 12);
        assert!(e.first().unwrap().start > NOW - 42 * DAY && e.last().unwrap().start < NOW + DAY);
    }
}
