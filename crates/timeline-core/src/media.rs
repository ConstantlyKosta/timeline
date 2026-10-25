//! Media kinds, MIME types, duration probing and synthetic sample media generation.

use crate::models::MediaKind;
use std::io::Cursor;

const AUDIO: &[&str] = &["mp3", "m4a", "wav", "aac", "aif", "aiff", "caf", "flac", "ogg", "opus"];
const VIDEO: &[&str] = &["mov", "mp4", "m4v", "webm"];
const PHOTO: &[&str] = &["jpg", "jpeg", "png", "heic", "webp", "gif"];
pub const TRANSCRIPT_EXTS: &[&str] = &["srt", "vtt", "json", "txt"];

pub fn extension(file_name: &str) -> String {
    match file_name.rsplit_once('.') {
        Some((_, e)) => e.to_lowercase(),
        None => String::new(),
    }
}

pub fn kind_for_extension(ext: &str) -> Option<MediaKind> {
    let e = ext.to_lowercase();
    if AUDIO.contains(&e.as_str()) {
        Some(MediaKind::Audio)
    } else if VIDEO.contains(&e.as_str()) {
        Some(MediaKind::Video)
    } else if PHOTO.contains(&e.as_str()) {
        Some(MediaKind::Photo)
    } else {
        None
    }
}

pub fn is_transcript_file(file_name: &str) -> bool {
    TRANSCRIPT_EXTS.contains(&extension(file_name).as_str())
}

pub fn mime_for_extension(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "mp3" => "audio/mpeg",
        "m4a" | "aac" => "audio/mp4",
        "wav" => "audio/wav",
        "aif" | "aiff" => "audio/aiff",
        "caf" => "audio/x-caf",
        "flac" => "audio/flac",
        "ogg" | "opus" => "audio/ogg",
        "mov" => "video/quicktime",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "heic" => "image/heic",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "application/octet-stream",
    }
}

/// Title from a file name: stem with `_`/`-` turned into spaces.
pub fn title_from_file_name(file_name: &str) -> String {
    let base = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    let stem = match base.rsplit_once('.') {
        Some((s, _)) if !s.is_empty() => s,
        _ => base,
    };
    let t: String = stem.chars().map(|c| if c == '_' { ' ' } else { c }).collect();
    let t = t.trim().to_string();
    if t.is_empty() { "Untitled".into() } else { t }
}

/// Playback duration in seconds for audio/video where cheaply knowable (WAV via hound, others via lofty).
pub fn probe_duration(ext: &str, bytes: &[u8]) -> Option<f64> {
    if ext.eq_ignore_ascii_case("wav") {
        if let Ok(r) = hound::WavReader::new(Cursor::new(bytes)) {
            let spec = r.spec();
            if spec.sample_rate > 0 && spec.channels > 0 {
                return Some(r.duration() as f64 / spec.sample_rate as f64);
            }
        }
    }
    use lofty::file::AudioFile;
    let probe = lofty::probe::Probe::new(Cursor::new(bytes)).guess_file_type().ok()?;
    let tagged = probe.read().ok()?;
    let d = tagged.properties().duration().as_secs_f64();
    if d > 0.0 { Some(d) } else { None }
}

/// Synthetic 8 kHz mono 16-bit WAV: a soft tone per `segment_secs` window whose pitch changes
/// between windows (so a waveform view looks alive). Used for the built-in sample recordings.
pub fn synth_wav(total_secs: f64, segment_secs: f64, base_hz: f64) -> Vec<u8> {
    let rate = 8000u32;
    let spec = hound::WavSpec { channels: 1, sample_rate: rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut buf = Cursor::new(Vec::new());
    {
        let mut w = hound::WavWriter::new(&mut buf, spec).expect("wav writer");
        let n = (total_secs * rate as f64) as usize;
        for i in 0..n {
            let t = i as f64 / rate as f64;
            let seg = (t / segment_secs).floor();
            let in_seg = (t % segment_secs) / segment_secs;
            let hz = base_hz * (1.0 + 0.12 * (seg % 5.0));
            // gentle envelope per window + slow tremolo
            let env = (std::f64::consts::PI * in_seg).sin().powf(0.5) * (0.6 + 0.4 * (t * 3.0).sin().abs());
            let v = (2.0 * std::f64::consts::PI * hz * t).sin() * env * 0.25;
            w.write_sample((v * i16::MAX as f64) as i16).expect("sample");
        }
        w.finalize().expect("finalize");
    }
    buf.into_inner()
}

/// Synthetic gradient PNG used as the sample photo. `seed` varies the colours.
pub fn synth_png(width: u32, height: u32, seed: u8) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().expect("png header");
        let mut data = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                let fx = x as f32 / width as f32;
                let fy = y as f32 / height as f32;
                // warm tile pattern
                let tile = (((x / 20) + (y / 20)) % 2) as f32 * 0.12;
                let r = (0.25 + 0.6 * fx + tile) * 255.0;
                let g = (0.45 + 0.3 * fy + tile + seed as f32 / 800.0) * 255.0;
                let b = (0.75 - 0.4 * fx * fy) * 255.0;
                data.extend_from_slice(&[r.clamp(0.0, 255.0) as u8, g.clamp(0.0, 255.0) as u8, b.clamp(0.0, 255.0) as u8]);
            }
        }
        w.write_image_data(&data).expect("png data");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_and_mimes() {
        assert_eq!(kind_for_extension("M4A"), Some(MediaKind::Audio));
        assert_eq!(kind_for_extension("mov"), Some(MediaKind::Video));
        assert_eq!(kind_for_extension("HEIC"), Some(MediaKind::Photo));
        assert_eq!(kind_for_extension("pdf"), None);
        assert_eq!(mime_for_extension("jpeg"), "image/jpeg");
        assert_eq!(mime_for_extension("zzz"), "application/octet-stream");
        assert_eq!(extension("a.b.WAV"), "wav");
        assert_eq!(extension("noext"), "");
        assert!(is_transcript_file("x.SRT") && !is_transcript_file("x.wav"));
    }

    #[test]
    fn titles() {
        assert_eq!(title_from_file_name("/tmp/Voice_memo_12.m4a"), "Voice memo 12");
        assert_eq!(title_from_file_name(".hidden"), ".hidden");
        assert_eq!(title_from_file_name(""), "Untitled");
    }

    #[test]
    fn wav_roundtrip_duration() {
        let w = synth_wav(12.5, 5.0, 220.0);
        assert!(w.starts_with(b"RIFF"));
        let d = probe_duration("wav", &w).unwrap();
        assert!((d - 12.5).abs() < 0.01, "{d}");
        assert_eq!(probe_duration("wav", b"garbage"), None);
        assert_eq!(probe_duration("mp3", b"garbage"), None);
    }

    #[test]
    fn png_is_valid() {
        let p = synth_png(64, 40, 3);
        assert_eq!(&p[1..4], b"PNG");
        let dec = png::Decoder::new(Cursor::new(&p));
        let r = dec.read_info().unwrap();
        assert_eq!((r.info().width, r.info().height), (64, 40));
    }
}
