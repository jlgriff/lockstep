//! Decoding any supported recording down to the mono 16 kHz signal whisper expects.
//!
//! Nothing here holds the whole recording: packets are decoded, resampled and passed on a piece
//! at a time, so memory stays flat however long the audio is.

use anyhow::{anyhow, Context, Result};
use rubato::{FftFixedIn, Resampler};
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as Symphonia;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// The only rate whisper.cpp accepts.
pub const SAMPLE_RATE: u32 = 16_000;

/// Frames handed to the resampler at a time, and the most that ever reaches a sink at once.
const CHUNK: usize = 4096;

/// Where resampled audio goes, once the resampler's leading group delay has been dropped.
struct Sink<'a, S: FnMut(&[f32])> {
    inner: &'a mut S,
    skip: usize,
}

impl<S: FnMut(&[f32])> Sink<'_, S> {
    /// Passes samples on, discarding the resampler's invented leading frames first.
    fn push(&mut self, samples: &[f32]) {
        let dropped = self.skip.min(samples.len());
        self.skip -= dropped;
        for piece in samples[dropped..].chunks(CHUNK) {
            (self.inner)(piece);
        }
    }
}

/// A recording opened for reading: its container, which track carries audio, and how to decode it.
struct Source {
    format: Box<dyn FormatReader>,
    track: u32,
    rate: u32,
    decoder: Box<dyn Decoder>,
}

/// Opens a recording's first audio track.
fn open(path: &Path) -> Result<Source> {
    let file =
        std::fs::File::open(path).with_context(|| format!("opening audio {}", path.display()))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|name| name.to_str()) {
        hint.with_extension(extension);
    }

    // Gapless playback trims the encoder's delay and padding, so the duration measured here is
    // the one a player will report for the same file.
    let options = FormatOptions {
        enable_gapless: true,
        ..Default::default()
    };
    let probed = symphonia::default::get_probe()
        .format(&hint, stream, &options, &MetadataOptions::default())
        .with_context(|| format!("recognising the format of {}", path.display()))?;

    let track = probed
        .format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow!("no audio track in {}", path.display()))?;
    let rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| anyhow!("{} does not declare a sample rate", path.display()))?;
    let id = track.id;
    let decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .with_context(|| format!("no decoder for {}", path.display()))?;

    Ok(Source {
        format: probed.format,
        track: id,
        rate,
        decoder,
    })
}

/// Decodes a recording to mono 16 kHz, handing it to `sink` in pieces, and returns its length.
pub fn stream(source: &Path, sink: &mut impl FnMut(&[f32])) -> Result<f64> {
    let Source {
        mut format,
        track: track_id,
        rate,
        mut decoder,
    } = open(source)?;

    let mut resampler = match rate == SAMPLE_RATE {
        true => None,
        false => Some(
            FftFixedIn::<f32>::new(rate as usize, SAMPLE_RATE as usize, CHUNK, 2, 1)
                .context("building the resampler")?,
        ),
    };
    let skip = resampler.as_ref().map_or(0, Resampler::output_delay);
    let mut out = Sink { inner: sink, skip };

    let mut interleaved: Option<SampleBuffer<f32>> = None;
    let mut pending: Vec<f32> = Vec::with_capacity(CHUNK * 2);
    let mut frames: u64 = 0;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Symphonia::IoError(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                break
            }
            Err(error) => return Err(error).context("reading the next packet"),
        };
        if packet.track_id() != track_id {
            continue;
        }

        let audio = match decoder.decode(&packet) {
            Ok(audio) => audio,
            // A damaged packet costs its own few milliseconds rather than the whole file.
            Err(Symphonia::DecodeError(_)) => continue,
            Err(error) => return Err(error).context("decoding audio"),
        };

        let spec = *audio.spec();
        let channels = spec.channels.count();
        let buffer = match interleaved.as_mut() {
            Some(buffer) if buffer.capacity() >= audio.frames() * channels => buffer,
            _ => interleaved.insert(SampleBuffer::new(audio.capacity() as u64, spec)),
        };
        buffer.copy_interleaved_ref(audio);

        let before = pending.len();
        pending.extend(
            buffer
                .samples()
                .chunks(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32),
        );
        frames += (pending.len() - before) as u64;

        match resampler.as_mut() {
            Some(resampler) => {
                while pending.len() >= resampler.input_frames_next() {
                    let take = resampler.input_frames_next();
                    let done = resampler
                        .process(&[&pending[..take]], None)
                        .context("resampling")?;
                    out.push(&done[0]);
                    pending.drain(..take);
                }
            }
            None => {
                out.push(&pending);
                pending.clear();
            }
        }
    }

    if !pending.is_empty() {
        match resampler.as_mut() {
            Some(resampler) => {
                let done = resampler
                    .process_partial(Some(&[&pending[..]]), None)
                    .context("resampling the last piece")?;
                out.push(&done[0]);
            }
            None => out.push(&pending),
        }
    }

    match frames {
        0 => Err(anyhow!("decoded no audio from {}", source.display())),
        frames => Ok(frames as f64 / rate as f64),
    }
}

/// Reads a recording's length, writing the WAV whisper reads when one is wanted.
pub fn prepare(source: &Path, wav: Option<&Path>) -> Result<f64> {
    let Some(path) = wav else {
        return stream(source, &mut |_| {});
    };

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)
        .with_context(|| format!("writing {}", path.display()))?;

    // The sink cannot report failure, so the first write error is kept and raised afterwards.
    let mut failed = None;
    let duration = stream(source, &mut |chunk| {
        for sample in chunk {
            let written = writer.write_sample((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
            if let Err(error) = written {
                failed.get_or_insert(error);
                return;
            }
        }
    })?;
    if let Some(error) = failed {
        return Err(error).with_context(|| format!("writing {}", path.display()));
    }
    writer.finalize().context("closing the WAV")?;
    Ok(duration)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a test recording of `seconds` at `rate`, with `channels` interleaved.
    fn recording(name: &str, seconds: f64, rate: u32, channels: u16) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(name);
        let spec = hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        let frames = (seconds * rate as f64) as usize;
        for frame in 0..frames {
            // A slow sine, well under the 8 kHz the 16 kHz output can carry.
            let value = ((frame as f64 / rate as f64) * 220.0 * std::f64::consts::TAU).sin();
            for _ in 0..channels {
                writer
                    .write_sample((value * i16::MAX as f64) as i16)
                    .unwrap();
            }
        }
        writer.finalize().unwrap();
        path
    }

    #[test]
    fn a_recording_reaches_the_sink_in_pieces_rather_than_all_at_once() {
        let path = recording("lockstep-stream.wav", 10.0, 44_100, 2);
        let (mut total, mut largest) = (0usize, 0usize);
        let duration = stream(&path, &mut |chunk| {
            total += chunk.len();
            largest = largest.max(chunk.len());
        })
        .unwrap();

        assert!((duration - 10.0).abs() < 0.01, "duration was {duration}");
        assert!(
            (total as f64 - 160_000.0).abs() < 2_000.0,
            "got {total} samples"
        );
        assert!(
            largest <= CHUNK,
            "handed over {largest} samples in one piece"
        );
    }

    #[test]
    fn a_recording_already_at_the_right_rate_passes_through_unchanged() {
        let path = recording("lockstep-passthrough.wav", 0.5, SAMPLE_RATE, 1);
        let mut samples = Vec::new();
        let duration = stream(&path, &mut |chunk| samples.extend_from_slice(chunk)).unwrap();

        assert!((duration - 0.5).abs() < 0.01);
        assert_eq!(samples.len(), 8_000);
        let expected = ((100.0 / SAMPLE_RATE as f64) * 220.0 * std::f64::consts::TAU).sin();
        assert!(
            (samples[100] as f64 - expected).abs() < 0.01,
            "got {}",
            samples[100]
        );
    }

    #[test]
    fn resampling_does_not_shift_the_audio_later() {
        // Half a second of silence then a burst. The resampler's group delay would push the
        // onset ~341 samples late at 16 kHz if it were not trimmed off.
        let path = std::env::temp_dir().join("lockstep-onset.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for frame in 0..44_100 {
            let loud = frame >= 22_050;
            let value = ((frame as f64 / 44_100.0) * 440.0 * std::f64::consts::TAU).sin();
            writer
                .write_sample(if loud {
                    (value * i16::MAX as f64) as i16
                } else {
                    0
                })
                .unwrap();
        }
        writer.finalize().unwrap();

        let mut samples = Vec::new();
        stream(&path, &mut |chunk| samples.extend_from_slice(chunk)).unwrap();
        let onset = samples
            .iter()
            .position(|s| s.abs() > 0.2)
            .expect("no burst found");
        assert!(
            (onset as i64 - 8_000).abs() < 100,
            "burst starts at {onset}, expected about 8000"
        );
    }

    #[test]
    fn channels_are_averaged_rather_than_one_being_taken() {
        // Only the left channel carries signal, so averaging halves it while keeping just the
        // first channel would not.
        let path = std::env::temp_dir().join("lockstep-lopsided.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for frame in 0..SAMPLE_RATE / 2 {
            let value = ((frame as f64 / SAMPLE_RATE as f64) * 220.0 * std::f64::consts::TAU).sin();
            writer
                .write_sample((value * i16::MAX as f64) as i16)
                .unwrap();
            writer.write_sample(0i16).unwrap();
        }
        writer.finalize().unwrap();

        let mut samples = Vec::new();
        stream(&path, &mut |chunk| samples.extend_from_slice(chunk)).unwrap();
        let expected = ((100.0 / SAMPLE_RATE as f64) * 220.0 * std::f64::consts::TAU).sin() / 2.0;
        assert!(
            (samples[100] as f64 - expected).abs() < 0.01,
            "got {}",
            samples[100]
        );
    }

    #[test]
    fn no_single_piece_larger_than_a_chunk_reaches_the_sink() {
        let mut sizes = Vec::new();
        let mut sink = |piece: &[f32]| sizes.push(piece.len());
        Sink {
            inner: &mut sink,
            skip: 0,
        }
        .push(&vec![0.0; CHUNK * 2 + 7]);
        assert_eq!(sizes, [CHUNK, CHUNK, 7]);
    }

    #[test]
    fn the_leading_delay_is_dropped_before_anything_reaches_the_sink() {
        let mut got: Vec<f32> = Vec::new();
        let mut sink = |piece: &[f32]| got.extend_from_slice(piece);
        Sink {
            inner: &mut sink,
            skip: 3,
        }
        .push(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(got, [4.0, 5.0]);
    }
}
