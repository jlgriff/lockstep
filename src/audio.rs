//! Decoding any supported recording down to the mono 16 kHz signal whisper expects.

use anyhow::{anyhow, Context, Result};
use rubato::{FftFixedIn, Resampler};
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as Symphonia;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// The only rate whisper.cpp accepts.
pub const SAMPLE_RATE: u32 = 16_000;

/// Input frames per resampler pass, large enough that the FFT overhead disappears.
const CHUNK: usize = 4096;

/// A decoded recording, mixed to mono at its own sample rate.
struct Decoded {
    samples: Vec<f32>,
    rate: u32,
}

/// Decodes a recording of any supported format, averaging its channels to mono.
fn decode(path: &Path) -> Result<Decoded> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("opening audio {}", path.display()))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|name| name.to_str()) {
        hint.with_extension(extension);
    }

    // Gapless playback trims the encoder's delay and padding, so the duration measured here is
    // the one a player will report for the same file.
    let format_options = FormatOptions { enable_gapless: true, ..Default::default() };
    let probed = symphonia::default::get_probe()
        .format(&hint, stream, &format_options, &MetadataOptions::default())
        .with_context(|| format!("recognising the format of {}", path.display()))?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow!("no audio track in {}", path.display()))?;
    let track_id = track.id;
    let rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| anyhow!("{} does not declare a sample rate", path.display()))?;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .with_context(|| format!("no decoder for {}", path.display()))?;

    let mut interleaved: Option<SampleBuffer<f32>> = None;
    let mut samples = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Symphonia::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
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
        samples.extend(
            buffer.samples().chunks(channels).map(|frame| {
                frame.iter().sum::<f32>() / channels as f32
            }),
        );
    }

    if samples.is_empty() {
        return Err(anyhow!("decoded no audio from {}", path.display()));
    }
    Ok(Decoded { samples, rate })
}

/// Resamples a mono signal to the rate whisper expects.
fn resample(decoded: Decoded) -> Result<Vec<f32>> {
    if decoded.rate == SAMPLE_RATE {
        return Ok(decoded.samples);
    }

    let mut resampler =
        FftFixedIn::<f32>::new(decoded.rate as usize, SAMPLE_RATE as usize, CHUNK, 2, 1)
            .context("building the resampler")?;
    // The resampler's own group delay would otherwise push every timestamp late by the same
    // amount, so the leading frames it invents are dropped rather than carried into the WAV.
    let delay = resampler.output_delay();
    let mut out = Vec::with_capacity(
        decoded.samples.len() * SAMPLE_RATE as usize / decoded.rate as usize + CHUNK,
    );

    let mut taken = 0;
    while taken < decoded.samples.len() {
        let wanted = resampler.input_frames_next();
        let end = (taken + wanted).min(decoded.samples.len());
        let chunk = [&decoded.samples[taken..end]];
        let mut done = if end - taken == wanted {
            resampler.process(&chunk, None)
        } else {
            resampler.process_partial(Some(&chunk), None)
        }
        .context("resampling")?;
        out.append(&mut done[0]);
        taken = end;
    }
    out.drain(..delay.min(out.len()));
    Ok(out)
}

/// Writes a mono 16-bit PCM WAV at the whisper sample rate.
fn write_wav(path: &Path, samples: &[f32]) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)
        .with_context(|| format!("writing {}", path.display()))?;
    for sample in samples {
        writer.write_sample((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
    }
    writer.finalize().context("closing the WAV")
}

/// Reads a recording's length, writing the WAV whisper reads when one is wanted.
pub fn prepare(source: &Path, wav: Option<&Path>) -> Result<f64> {
    let decoded = decode(source)?;
    let duration = decoded.samples.len() as f64 / decoded.rate as f64;
    if let Some(path) = wav {
        write_wav(path, &resample(decoded)?)?;
    }
    Ok(duration)
}
