//! Decoding audio (MP3, M4A/AAC, WAV, FLAC, OGG, and the audio track of MP4/MOV/MKV video) to the
//! 16 kHz mono f32 PCM that whisper wants. Pure Rust (symphonia): no ffmpeg needed.

use std::fs::File;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub const TARGET_RATE: u32 = 16_000;

/// Streaming windowed-sinc resampler (mono). Downsampling low-passes at the target Nyquist, so
/// 44.1/48 kHz speech does not alias. Memory use is bounded by the kernel width.
pub struct Resampler {
    ratio: f64, // input samples per output sample
    cutoff: f64, // in cycles/input-sample
    half: f64,  // kernel half-width in input samples
    buf: Vec<f32>,
    base: f64, // input index of buf[0]
    pos: f64,  // input position of the next output sample
    passthrough: bool,
}

impl Resampler {
    pub fn new(in_rate: u32, out_rate: u32) -> Self {
        let ratio = in_rate as f64 / out_rate as f64;
        let fc = 0.5 / ratio.max(1.0) * 0.97;
        let half = 16.0 * ratio.max(1.0);
        Resampler { ratio, cutoff: fc, half, buf: Vec::new(), base: 0.0, pos: 0.0, passthrough: in_rate == out_rate }
    }

    fn kernel(&self, x: f64) -> f64 {
        // Blackman-windowed sinc, x in input samples relative to the output position.
        if x.abs() >= self.half {
            return 0.0;
        }
        let s = if x.abs() < 1e-9 { 2.0 * self.cutoff } else { (2.0 * std::f64::consts::PI * self.cutoff * x).sin() / (std::f64::consts::PI * x) };
        let t = x / self.half; // -1..1
        let w = 0.42 + 0.5 * (std::f64::consts::PI * t).cos() + 0.08 * (2.0 * std::f64::consts::PI * t).cos();
        s * w
    }

    fn run(&mut self, out: &mut Vec<f32>, flush: bool) {
        let end = self.base + self.buf.len() as f64;
        loop {
            // need samples up to pos+half (or the end of data when flushing)
            if !flush && self.pos + self.half >= end {
                break;
            }
            if flush && self.pos >= end {
                break;
            }
            let lo = ((self.pos - self.half).ceil().max(self.base)) as i64;
            let hi = ((self.pos + self.half).floor().min(end - 1.0)) as i64;
            let mut acc = 0.0f64;
            for i in lo..=hi {
                acc += self.buf[(i as f64 - self.base) as usize] as f64 * self.kernel(i as f64 - self.pos);
            }
            out.push(acc as f32);
            self.pos += self.ratio;
        }
        // drop what no future output can touch
        let keep_from = (self.pos - self.half).floor().max(self.base);
        let drop = (keep_from - self.base) as usize;
        if drop > 0 && drop <= self.buf.len() {
            self.buf.drain(..drop);
            self.base += drop as f64;
        }
    }

    pub fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if self.passthrough {
            out.extend_from_slice(input);
            return;
        }
        self.buf.extend_from_slice(input);
        self.run(out, false);
    }

    pub fn finish(&mut self, out: &mut Vec<f32>) {
        if !self.passthrough {
            self.run(out, true);
        }
    }
}

/// Decodes `path` to 16 kHz mono. `progress` gets 0..1 (when the length is known); `cancelled`
/// is polled between packets.
pub fn decode_16k_mono(path: &Path, progress: &dyn Fn(f64), cancelled: &dyn Fn() -> bool) -> Result<Vec<f32>, String> {
    let file = File::open(path).map_err(|e| format!("cannot open the audio file: {e}"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("unsupported or damaged audio file ({e})"))?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL && t.codec_params.sample_rate.is_some())
        .ok_or("no audio track found in this file")?
        .clone();
    let track_id = track.id;
    let in_rate = track.codec_params.sample_rate.ok_or("unknown sample rate")?;
    let total_frames = track.codec_params.n_frames;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("this audio format cannot be decoded ({e})"))?;

    let mut resampler = Resampler::new(in_rate, TARGET_RATE);
    let mut out: Vec<f32> = Vec::new();
    let mut sbuf: Option<SampleBuffer<f32>> = None;
    let mut mono: Vec<f32> = Vec::new();
    let mut frames_done: u64 = 0;
    let mut last_report = 0.0;

    loop {
        if cancelled() {
            return Err("cancelled".into());
        }
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(e) => return Err(format!("reading the audio failed ({e})")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymError::DecodeError(_)) => continue, // skip a corrupt frame
            Err(SymError::IoError(_)) => continue,
            Err(e) => return Err(format!("decoding the audio failed ({e})")),
        };
        let spec = *decoded.spec();
        let ch = spec.channels.count().max(1);
        let frames = decoded.frames();
        if sbuf.as_ref().map(|b| b.capacity() < decoded.capacity() * ch).unwrap_or(true) {
            sbuf = Some(SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
        }
        let b = sbuf.as_mut().unwrap();
        b.copy_interleaved_ref(decoded);
        mono.clear();
        for f in b.samples().chunks(ch).take(frames) {
            mono.push(f.iter().sum::<f32>() / ch as f32);
        }
        resampler.push(&mono, &mut out);
        frames_done += frames as u64;
        if let Some(t) = total_frames {
            let p = (frames_done as f64 / t.max(1) as f64).min(1.0);
            if p - last_report > 0.02 {
                last_report = p;
                progress(p);
            }
        }
    }
    resampler.finish(&mut out);
    if out.is_empty() {
        return Err("the file contains no audio".into());
    }
    progress(1.0);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, secs: f64, freq: f64) -> Vec<f32> {
        (0..(rate as f64 * secs) as usize).map(|i| (2.0 * std::f64::consts::PI * freq * i as f64 / rate as f64).sin() as f32 * 0.5).collect()
    }
    fn rms(v: &[f32]) -> f64 {
        (v.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / v.len() as f64).sqrt()
    }

    #[test]
    fn resamples_44k1_to_16k_preserving_length_and_level() {
        let input = sine(44_100, 2.0, 440.0);
        let mut r = Resampler::new(44_100, 16_000);
        let mut out = Vec::new();
        for c in input.chunks(1000) {
            r.push(c, &mut out);
        }
        r.finish(&mut out);
        assert!((out.len() as i64 - 32_000).abs() <= 2, "len {}", out.len());
        // level of the passband tone is preserved (0.5/sqrt2 = 0.3535), ignoring edges
        let mid = &out[2000..out.len() - 2000];
        assert!((rms(mid) - 0.3535).abs() < 0.01, "rms {}", rms(mid));
    }

    #[test]
    fn tones_above_the_new_nyquist_are_removed_not_aliased() {
        let input = sine(48_000, 1.0, 12_000.0); // > 8 kHz Nyquist of 16 kHz
        let mut r = Resampler::new(48_000, 16_000);
        let mut out = Vec::new();
        r.push(&input, &mut out);
        r.finish(&mut out);
        assert!(rms(&out[500..out.len() - 500]) < 0.02);
    }

    #[test]
    fn chunking_does_not_change_the_result() {
        let input = sine(22_050, 1.0, 300.0);
        let (mut a, mut b) = (Vec::new(), Vec::new());
        let mut r1 = Resampler::new(22_050, 16_000);
        r1.push(&input, &mut a);
        r1.finish(&mut a);
        let mut r2 = Resampler::new(22_050, 16_000);
        for c in input.chunks(333) {
            r2.push(c, &mut b);
        }
        r2.finish(&mut b);
        assert_eq!(a.len(), b.len());
        assert!(a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-5));
    }

    #[test]
    fn decodes_a_stereo_wav_to_16k_mono() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.wav");
        let mut w = hound::WavWriter::create(&p, hound::WavSpec { channels: 2, sample_rate: 32_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int }).unwrap();
        for i in 0..32_000 {
            let s = ((2.0 * std::f64::consts::PI * 500.0 * i as f64 / 32_000.0).sin() * 8000.0) as i16;
            w.write_sample(s).unwrap();
            w.write_sample(s).unwrap();
        }
        w.finalize().unwrap();
        let pcm = decode_16k_mono(&p, &|_| {}, &|| false).unwrap();
        assert!((pcm.len() as i64 - 16_000).abs() <= 2, "len {}", pcm.len());
        assert!(pcm.iter().any(|x| x.abs() > 0.1));
        assert!(decode_16k_mono(dir.path().join("nope.wav").as_path(), &|_| {}, &|| false).is_err());
        let junk = dir.path().join("junk.mp3");
        std::fs::write(&junk, b"this is not audio at all").unwrap();
        assert!(decode_16k_mono(&junk, &|_| {}, &|| false).is_err());
    }
}
