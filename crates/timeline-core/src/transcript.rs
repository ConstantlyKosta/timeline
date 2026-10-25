//! Transcript parsing (SRT, WebVTT, JSON, plain text), time lookup and plain-text export.

use crate::error::{Error, Result};
use crate::models::{Transcript, TranscriptSegment};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptFormat {
    Srt,
    Vtt,
    Json,
    Text,
}

pub fn detect_format(file_name: &str, content: &str) -> TranscriptFormat {
    let lower = file_name.to_lowercase();
    let trimmed = content.trim_start_matches('\u{feff}').trim_start();
    if lower.ends_with(".srt") {
        TranscriptFormat::Srt
    } else if lower.ends_with(".vtt") || trimmed.starts_with("WEBVTT") {
        TranscriptFormat::Vtt
    } else if lower.ends_with(".json") || trimmed.starts_with('{') || (trimmed.starts_with('[') && trimmed[1..].trim_start().starts_with(['{', ']'])) {
        TranscriptFormat::Json
    } else if trimmed.contains("-->") {
        TranscriptFormat::Srt
    } else {
        TranscriptFormat::Text
    }
}

/// Parse any supported transcript. `duration` (seconds) is only used to spread plain text lines.
pub fn parse(file_name: &str, content: &str, duration: Option<f64>) -> Result<Transcript> {
    let content = content.trim_start_matches('\u{feff}');
    let t = match detect_format(file_name, content) {
        TranscriptFormat::Srt | TranscriptFormat::Vtt => parse_cues(content)?,
        TranscriptFormat::Json => parse_json(content)?,
        TranscriptFormat::Text => parse_text(content, duration),
    };
    if t.segments.is_empty() {
        return Err(Error::InvalidTranscript("no segments found".into()));
    }
    Ok(t)
}

fn finish(mut segs: Vec<TranscriptSegment>, language: Option<String>, synthesized: bool) -> Transcript {
    segs.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
    for (i, s) in segs.iter_mut().enumerate() {
        s.id = i;
        if s.end < s.start {
            s.end = s.start;
        }
    }
    Transcript { segments: segs, language, has_synthesized_timing: synthesized }
}

/// "01:02:03,500", "02:03.500", "1:02" -> seconds.
pub fn parse_timestamp(s: &str) -> Option<f64> {
    let s = s.trim().replace(',', ".");
    let parts: Vec<&str> = s.split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let mut total = 0f64;
    for p in &parts {
        let v: f64 = p.trim().parse().ok()?;
        if v < 0.0 {
            return None;
        }
        total = total * 60.0 + v;
    }
    Some(total)
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// SRT and WebVTT share the "start --> end" cue structure.
fn parse_cues(content: &str) -> Result<Transcript> {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let mut segs = Vec::new();
    for block in normalized.split("\n\n") {
        let lines: Vec<&str> = block.lines().collect();
        let Some(ti) = lines.iter().position(|l| l.contains("-->")) else { continue };
        let (a, b) = lines[ti].split_once("-->").unwrap();
        // VTT cue settings follow the end time ("00:01.000 align:start")
        let b = b.trim().split_whitespace().next().unwrap_or("");
        let (Some(start), Some(end)) = (parse_timestamp(a), parse_timestamp(b)) else {
            return Err(Error::InvalidTranscript(format!("bad timestamp line: {}", lines[ti])));
        };
        let text = strip_tags(&lines[ti + 1..].join(" ")).trim().to_string();
        if text.is_empty() {
            continue;
        }
        let (speaker, text) = split_speaker(&text);
        segs.push(TranscriptSegment { id: 0, start, end, text, speaker });
    }
    Ok(finish(segs, None, false))
}

/// "Mom: hello" -> (Some("Mom"), "hello"). Speaker labels are short (<= 24 chars, <= 3 words).
fn split_speaker(text: &str) -> (Option<String>, String) {
    if let Some((label, rest)) = text.split_once(": ") {
        let words = label.split_whitespace().count();
        if (1..=3).contains(&words) && label.chars().count() <= 24 && !label.contains(['.', '?', '!', ',']) && label.chars().next().map_or(false, |c| c.is_uppercase()) {
            return (Some(label.to_string()), rest.trim().to_string());
        }
    }
    (None, text.to_string())
}

fn parse_json(content: &str) -> Result<Transcript> {
    let v: serde_json::Value = serde_json::from_str(content)?;
    let (arr, language) = match &v {
        serde_json::Value::Array(a) => (a.clone(), None),
        serde_json::Value::Object(o) => {
            let a = o
                .get("segments")
                .and_then(|s| s.as_array())
                .cloned()
                .ok_or_else(|| Error::InvalidTranscript("JSON has no \"segments\" array".into()))?;
            (a, o.get("language").and_then(|l| l.as_str()).map(String::from))
        }
        _ => return Err(Error::InvalidTranscript("unexpected JSON shape".into())),
    };
    let mut segs = Vec::new();
    for item in arr {
        let o = item.as_object().ok_or_else(|| Error::InvalidTranscript("segment is not an object".into()))?;
        let num = |k: &str| o.get(k).and_then(|x| x.as_f64());
        let start = num("start").ok_or_else(|| Error::InvalidTranscript("segment missing \"start\"".into()))?;
        let end = num("end").unwrap_or(start);
        let text = o.get("text").and_then(|t| t.as_str()).unwrap_or("").trim().to_string();
        if text.is_empty() {
            continue;
        }
        let speaker = o.get("speaker").and_then(|t| t.as_str()).map(String::from);
        segs.push(TranscriptSegment { id: 0, start, end, text, speaker });
    }
    Ok(finish(segs, language, false))
}

/// Plain text: lines may start with `[mm:ss]` / `[hh:mm:ss]`; otherwise timing is synthesised.
fn parse_text(content: &str, duration: Option<f64>) -> Transcript {
    let lines: Vec<&str> = content.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let stamped: Vec<(Option<f64>, String)> = lines
        .iter()
        .map(|l| {
            if l.starts_with('[') {
                if let Some(close) = l.find(']') {
                    if let Some(t) = parse_timestamp(&l[1..close]) {
                        return (Some(t), l[close + 1..].trim().to_string());
                    }
                }
            }
            (None, l.to_string())
        })
        .collect();
    let all_stamped = !stamped.is_empty() && stamped.iter().all(|(t, _)| t.is_some());
    let mut segs = Vec::new();
    if all_stamped {
        for (i, (t, text)) in stamped.iter().enumerate() {
            let start = t.unwrap();
            let end = stamped.get(i + 1).and_then(|n| n.0).unwrap_or(start + 4.0);
            let (speaker, text) = split_speaker(text);
            segs.push(TranscriptSegment { id: 0, start, end, text, speaker });
        }
        return finish(segs, None, false);
    }
    let total_chars: usize = stamped.iter().map(|(_, t)| t.chars().count().max(1)).sum();
    let total = duration.filter(|d| *d > 0.0).unwrap_or(stamped.len() as f64 * 4.0);
    let mut cursor = 0f64;
    for (_, text) in &stamped {
        let share = total * text.chars().count().max(1) as f64 / total_chars as f64;
        let (speaker, text) = split_speaker(text);
        segs.push(TranscriptSegment { id: 0, start: cursor, end: cursor + share, text, speaker });
        cursor += share;
    }
    finish(segs, None, true)
}

/// Binary-search lookup of the segment active at a playback time.
pub struct TranscriptIndex<'a> {
    segs: &'a [TranscriptSegment],
}

impl<'a> TranscriptIndex<'a> {
    pub fn new(t: &'a Transcript) -> Self {
        Self { segs: &t.segments }
    }
    /// Index of the last segment whose start <= time (and whose end is not far past), or None
    /// before the first segment.
    pub fn segment_at(&self, time: f64) -> Option<usize> {
        let idx = self.segs.partition_point(|s| s.start <= time);
        if idx == 0 {
            None
        } else {
            Some(idx - 1)
        }
    }
}

pub fn format_clock(seconds: f64) -> String {
    let t = seconds.max(0.0).floor() as u64;
    let (h, m, s) = (t / 3600, (t / 60) % 60, t % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// Plain-text export: `[m:ss] Speaker: text`.
pub fn export_text(title: &str, t: &Transcript) -> String {
    let mut out = format!("{title}\n\n");
    for s in &t.segments {
        out.push_str(&format!("[{}] ", format_clock(s.start)));
        if let Some(sp) = &s.speaker {
            out.push_str(sp);
            out.push_str(": ");
        }
        out.push_str(&s.text);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps() {
        assert_eq!(parse_timestamp("00:00:01,500"), Some(1.5));
        assert_eq!(parse_timestamp("01:02.250"), Some(62.25));
        assert_eq!(parse_timestamp("1:01:01"), Some(3661.0));
        assert_eq!(parse_timestamp("12"), Some(12.0));
        assert_eq!(parse_timestamp("a:b"), None);
        assert_eq!(parse_timestamp("1:2:3:4"), None);
    }

    #[test]
    fn srt_with_speakers_and_multiline() {
        let srt = "1\n00:00:01,000 --> 00:00:03,000\nMom: Hello there\n\n2\n00:00:03,500 --> 00:00:06,000\nline one\nline <i>two</i>\n";
        let t = parse("x.srt", srt, None).unwrap();
        assert_eq!(t.segments.len(), 2);
        assert_eq!(t.segments[0].speaker.as_deref(), Some("Mom"));
        assert_eq!(t.segments[0].text, "Hello there");
        assert_eq!(t.segments[1].text, "line one line two");
        assert_eq!(t.segments[1].start, 3.5);
        assert!(!t.has_synthesized_timing);
    }

    #[test]
    fn vtt_with_header_settings_and_crlf() {
        let vtt = "WEBVTT\r\n\r\nNOTE a comment\r\n\r\ncue1\r\n00:01.000 --> 00:02.500 align:start\r\nHi\r\n\r\n00:03.000 --> 00:04.000\r\nThere\r\n";
        let t = parse("a.vtt", vtt, None).unwrap();
        assert_eq!(t.segments.len(), 2);
        assert_eq!(t.segments[0].end, 2.5);
        assert_eq!(t.segments[1].text, "There");
    }

    #[test]
    fn json_object_and_array() {
        let o = r#"{"language":"en","segments":[{"start":5,"end":9,"text":"b"},{"start":0,"end":4,"text":"a","speaker":"Me"}]}"#;
        let t = parse("t.json", o, None).unwrap();
        assert_eq!(t.language.as_deref(), Some("en"));
        assert_eq!(t.segments[0].text, "a"); // sorted
        assert_eq!(t.segments[0].id, 0);
        assert_eq!(t.segments[1].id, 1);
        let a = r#"[{"start":1,"text":"x"}]"#;
        let t = parse("t.json", a, None).unwrap();
        assert_eq!(t.segments[0].end, 1.0);
        assert!(parse("t.json", "{\"nope\":1}", None).is_err());
        assert!(parse("t.json", "not json {", None).is_err());
    }

    #[test]
    fn text_with_and_without_timestamps() {
        let t = parse("a.txt", "[0:05] first\n[0:12] Me: second\n", None).unwrap();
        assert_eq!(t.segments[0].start, 5.0);
        assert_eq!(t.segments[0].end, 12.0);
        assert_eq!(t.segments[1].speaker.as_deref(), Some("Me"));
        assert_eq!(t.segments[1].end, 16.0);
        assert!(!t.has_synthesized_timing);
        let t = parse("a.txt", "one\ntwo\n\nthree", Some(60.0)).unwrap();
        assert!(t.has_synthesized_timing);
        assert_eq!(t.segments.len(), 3);
        assert!((t.segments.last().unwrap().end - 60.0).abs() < 1e-6);
        let t = parse("a.txt", "one\ntwo", None).unwrap();
        assert_eq!(t.segments[1].end, 8.0);
    }

    #[test]
    fn empty_is_error() {
        assert!(parse("a.txt", "  \n ", None).is_err());
        assert!(parse("a.srt", "", None).is_err());
    }

    #[test]
    fn index_lookup() {
        let t = parse("t.json", r#"[{"start":2,"end":4,"text":"a"},{"start":6,"end":8,"text":"b"}]"#, None).unwrap();
        let ix = TranscriptIndex::new(&t);
        assert_eq!(ix.segment_at(0.0), None);
        assert_eq!(ix.segment_at(2.0), Some(0));
        assert_eq!(ix.segment_at(5.0), Some(0));
        assert_eq!(ix.segment_at(6.0), Some(1));
        assert_eq!(ix.segment_at(100.0), Some(1));
    }

    #[test]
    fn export_formats_clock_and_speaker() {
        let t = parse("t.json", r#"[{"start":65,"end":70,"text":"hi","speaker":"Mom"},{"start":3700,"text":"late"}]"#, None).unwrap();
        let s = export_text("Call", &t);
        assert_eq!(s, "Call\n\n[1:05] Mom: hi\n[1:01:40] late\n");
    }

    #[test]
    fn format_detection() {
        assert_eq!(detect_format("a.txt", "WEBVTT\n"), TranscriptFormat::Vtt);
        assert_eq!(detect_format("a.txt", "[{}]"), TranscriptFormat::Json);
        assert_eq!(detect_format("a.txt", "00:01 --> 00:02\nx"), TranscriptFormat::Srt);
        assert_eq!(detect_format("a.txt", "hello"), TranscriptFormat::Text);
    }
}
