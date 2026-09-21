//! Validates supplied-text alignment results and adapts them to Lockstep's timing format.

use crate::{script, timing, whisper::Heard};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const DEFAULT_MODEL: &str = "MahmoudAshraf/mms-300m-1130-forced-aligner";

/// Selects the optional Python environment, acoustic model, and text-normalization language.
pub struct Config {
    pub python: PathBuf,
    pub model: String,
    pub language: String,
}

impl Default for Config {
    /// Finds the repository's optional environment before falling back to Python on PATH.
    fn default() -> Self {
        let local = PathBuf::from(".venv-align/bin/python");
        Self {
            python: if local.is_file() {
                local
            } else {
                "python3".into()
            },
            model: DEFAULT_MODEL.into(),
            language: "eng".into(),
        }
    }
}

#[derive(Deserialize)]
struct Word {
    text: String,
    start: f64,
    end: f64,
}

/// Converts aligned words to a document while preserving the user's original lyric text.
pub fn build(
    lines: &[script::Line],
    json: &str,
    script: &Path,
    duration: f64,
) -> Result<timing::Document> {
    let words: Vec<Word> = serde_json::from_str(json).context("reading forced alignment JSON")?;
    if !duration.is_finite() || duration <= 0.0 {
        bail!("invalid track duration for forced alignment");
    }
    let expected = lines
        .iter()
        .flat_map(script::Line::keys)
        .collect::<Vec<_>>();
    if words.len() != expected.len() {
        bail!(
            "forced alignment word count: expected {}, received {}",
            expected.len(),
            words.len()
        );
    }
    let mut previous_end = 0.0;
    for (index, (word, expected)) in words.iter().zip(expected).enumerate() {
        let number = index + 1;
        if script::key(&word.text) != expected {
            bail!(
                "forced alignment word {number}: expected {expected:?}, received {:?}",
                word.text
            );
        }
        if !word.start.is_finite()
            || !word.end.is_finite()
            || word.start < 0.0
            || word.end <= word.start
            || word.end > duration
        {
            bail!("forced alignment word {number} ({:?}) has invalid or unresolved timing {}..{} for track duration {duration}", word.text, word.start, word.end);
        }
        if word.start < previous_end {
            bail!(
                "forced alignment word {number} ({:?}) overlaps its predecessor",
                word.text
            );
        }
        previous_end = word.end;
    }
    let heard = words
        .into_iter()
        .map(|word| Heard {
            key: script::key(&word.text),
            start: word.start,
            end: word.end,
        })
        .collect::<Vec<_>>();
    let mut document = timing::build(lines, &heard, script, duration)?.0;
    for line in &mut document.lines {
        line.words
            .retain(|word| !script::key(&word.text).is_empty());
    }
    document.alignment.method = Some("forced");
    Ok(document)
}

/// Sends known lyric words and decoded audio to the acoustic aligner without transcription.
pub fn align(wav: &Path, lines: &[script::Line], config: &Config) -> Result<String> {
    let words = lines
        .iter()
        .flat_map(|line| &line.tokens)
        .filter(|token| !token.key.is_empty())
        .map(|token| token.raw.as_str())
        .collect::<Vec<_>>();
    if words.is_empty() {
        return Ok("[]".into());
    }
    let mut child = Command::new(&config.python)
        .args(["-c", include_str!("forced_align.py")])
        .arg(wav).arg(&config.model).arg(&config.language)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().with_context(|| format!("starting forced alignment with {}; run scripts/setup-alignment.sh or set --alignment-python", config.python.display()))?;
    let input = serde_json::to_vec(&words)?;
    let sent = child.stdin.take().unwrap().write_all(&input);
    let output = child
        .wait_with_output()
        .context("waiting for forced alignment")?;
    if !output.status.success() {
        bail!(
            "forced alignment failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    sent.context("sending lyrics to forced alignment")?;
    String::from_utf8(output.stdout).context("reading forced alignment output")
}
