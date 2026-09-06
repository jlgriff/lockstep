//! Finding whisper.cpp, running it with DTW timing on, and reading word times back out.

use crate::script::key;
use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Longest a single word is allowed to run, so instrumental breaks are seen rather than absorbed.
const MAX_WORD: f64 = 1.0;

/// Longest silence inside one word before its earlier tokens are treated as strays.
const MAX_TOKEN_GAP: f64 = 3.0;

const BINARIES: [&str; 3] = ["whisper-cli", "whisper-cpp", "whisper"];

/// The alignment-heads presets whisper.cpp ships. A preset it does not know is a hard error
/// several seconds into a run, so a guess is checked against this before being passed on.
const DTW_PRESETS: [&str; 11] = [
    "tiny", "tiny.en", "base", "base.en", "small", "small.en", "medium", "medium.en", "large.v1",
    "large.v2", "large.v3",
];

/// A word whisper heard, reduced to its match key and the span it occupies.
pub struct Heard {
    pub key: String,
    pub start: f64,
    pub end: f64,
}

/// Where the transcription step should get its whisper binary and model.
pub struct Config {
    pub binary: Option<PathBuf>,
    pub model: Option<PathBuf>,
    pub dtw: Option<String>,
}

#[derive(Deserialize)]
struct Transcript {
    transcription: Vec<Segment>,
}

#[derive(Deserialize)]
struct Segment {
    tokens: Vec<RawToken>,
}

#[derive(Deserialize)]
struct RawToken {
    text: String,
    t_dtw: i64,
}

/// A word under construction: the text seen so far and every DTW time its tokens carried.
struct Partial {
    text: String,
    times: Vec<f64>,
}

/// True for whisper's own markers and for the filler it emits over instrumental passages.
fn is_noise(text: &str) -> bool {
    text.starts_with('[')
        || text.chars().all(|c| c.is_whitespace() || c == '\u{266a}' || c == '\u{266b}')
}

/// Onset of a word's final unbroken run of tokens, ignoring strays timed far too early.
fn onset(times: &[f64]) -> Option<f64> {
    let mut i = times.len().checked_sub(1)?;
    while i > 0 && times[i] - times[i - 1] <= MAX_TOKEN_GAP {
        i -= 1;
    }
    Some(times[i])
}

/// Rebuilds words from whisper's DTW-timed tokens, dropping markers and instrumental filler.
pub fn parse(json: &str) -> Result<Vec<Heard>> {
    let transcript: Transcript =
        serde_json::from_str(json).context("reading whisper transcript JSON")?;

    let mut words: Vec<Partial> = Vec::new();
    for token in transcript.transcription.iter().flat_map(|seg| &seg.tokens) {
        if is_noise(&token.text) {
            continue;
        }
        let time = (token.t_dtw >= 0).then(|| token.t_dtw as f64 / 100.0);
        match words.last_mut() {
            Some(word) if !token.text.starts_with(' ') => {
                word.text.push_str(&token.text);
                word.times.extend(time);
            }
            _ => words.push(Partial { text: token.text.clone(), times: Vec::from_iter(time) }),
        }
    }

    let mut heard: Vec<Heard> = words
        .iter()
        .filter_map(|word| {
            let start = onset(&word.times)?;
            let key = key(&word.text);
            (!key.is_empty()).then_some(Heard { key, start, end: start + MAX_WORD })
        })
        .collect();

    for i in 0..heard.len().saturating_sub(1) {
        heard[i].end = heard[i].end.min(heard[i + 1].start);
    }
    Ok(heard)
}

/// Looks up a bare command name on PATH.
fn on_path(name: &str) -> Option<PathBuf> {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(&file))
        .find(|candidate| candidate.is_file())
}

/// Resolves a path from what the caller was given, falling back to a search of the usual places.
fn locate(
    given: Option<&PathBuf>,
    search: impl FnOnce() -> Option<PathBuf>,
    missing: impl FnOnce() -> String,
) -> Result<PathBuf> {
    given.cloned().or_else(search).ok_or_else(|| anyhow!(missing()))
}

/// The largest `ggml-*.bin` in a directory, which is the most capable model installed there.
fn best_model_in(dir: &Path) -> Option<PathBuf> {
    let mut models: Vec<(u64, PathBuf)> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|entry| {
            entry.file_name().to_string_lossy().starts_with("ggml-")
                && entry.path().extension().is_some_and(|ext| ext == "bin")
        })
        .filter_map(|entry| Some((entry.metadata().ok()?.len(), entry.path())))
        .collect();
    models.sort_by_key(|(size, _)| *size);
    models.pop().map(|(_, path)| path)
}

/// Every directory a whisper model is conventionally installed into.
fn model_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("models"), PathBuf::from(".")];
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        dirs.push(home.join(".cache/whisper"));
        dirs.push(home.join(".local/share/whisper"));
        dirs.push(home.join("Library/Application Support/whisper"));
    }
    for prefix in ["/opt/homebrew", "/usr/local", "/usr"] {
        dirs.push(PathBuf::from(prefix).join("share/whisper.cpp/models"));
    }
    dirs
}

/// The `-dtw` alignment-heads preset matching a model file, which whisper names slightly differently.
fn dtw_preset(model: &Path) -> Result<String> {
    let stem = model
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("unreadable model filename {}", model.display()))?;
    let name = stem.strip_prefix("ggml-").unwrap_or(stem);
    let name = name.split("-q").next().unwrap_or(name).replace("-v", ".v");

    if !DTW_PRESETS.contains(&name.as_str()) {
        bail!(
            "cannot tell from its name which alignment-heads preset {} needs (guessed {name:?}, \
             which whisper does not know). Pass --dtw with one of: {}",
            model.display(),
            DTW_PRESETS.join(", ")
        );
    }
    Ok(name)
}

/// Transcribes a 16 kHz mono WAV, returning whisper's raw JSON.
pub fn transcribe(wav: &Path, config: &Config) -> Result<String> {
    let binary = locate(
        config.binary.as_ref(),
        || BINARIES.iter().find_map(|name| on_path(name)),
        || {
            format!(
                "no whisper.cpp binary found (tried {}). Install it (brew install whisper-cpp, \
                 apt install whisper.cpp, or build from source), or pass --whisper or set \
                 LOCKSTEP_WHISPER.",
                BINARIES.join(", ")
            )
        },
    )?;
    let model = locate(
        config.model.as_ref(),
        || model_dirs().iter().find_map(|dir| best_model_in(dir)),
        || {
            format!(
                "no whisper model found in {}. Download one from \
                 https://huggingface.co/ggerganov/whisper.cpp, then pass --model or set \
                 LOCKSTEP_MODEL.",
                model_dirs().iter().map(|dir| dir.display().to_string()).collect::<Vec<_>>().join(", ")
            )
        },
    )?;
    let dtw = match &config.dtw {
        Some(preset) => preset.clone(),
        None => dtw_preset(&model)?,
    };
    // `-nfa` matters: flash attention is on by default and silently disables DTW, which leaves
    // every word in a segment sharing one timestamp.
    let out = wav.with_extension("");
    let status = Command::new(&binary)
        .args(["-m".as_ref(), model.as_os_str(), "-f".as_ref(), wav.as_os_str()])
        .args(["-nfa", "-ml", "1", "-sow", "-oj", "-ojf", "-dtw", &dtw])
        .args(["-of".as_ref(), out.as_os_str()])
        .status()
        .with_context(|| format!("running {}", binary.display()))?;
    if !status.success() {
        bail!("{} exited with {status}", binary.display());
    }

    let json = out.with_extension("json");
    std::fs::read_to_string(&json).with_context(|| format!("reading {}", json.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds the shape of whisper's JSON around a list of token text and DTW centiseconds.
    fn transcript(tokens: &[(&str, i64)]) -> String {
        let tokens: Vec<String> = tokens
            .iter()
            .map(|(text, t_dtw)| format!(r#"{{"text":{text:?},"t_dtw":{t_dtw}}}"#))
            .collect();
        format!(r#"{{"transcription":[{{"tokens":[{}]}}]}}"#, tokens.join(","))
    }

    #[test]
    fn tokens_join_into_words_on_their_leading_space() {
        let heard = parse(&transcript(&[(" don", 500), ("'", 520), ("t", 540)])).unwrap();
        assert_eq!(heard.len(), 1);
        assert_eq!(heard[0].key, "dont");
        assert_eq!(heard[0].start, 5.0);
    }

    #[test]
    fn markers_and_instrumental_filler_are_dropped() {
        let heard = parse(&transcript(&[
            ("[_BEG_]", -1),
            (" \u{266a}", 100),
            (" one", 200),
            ("[_TT_50]", -1),
        ]))
        .unwrap();
        assert_eq!(heard.iter().map(|word| word.key.as_str()).collect::<Vec<_>>(), ["one"]);
    }

    #[test]
    fn a_word_whose_tokens_carry_no_time_is_dropped() {
        assert!(parse(&transcript(&[(" one", -1)])).unwrap().is_empty());
    }

    #[test]
    fn a_stray_early_token_does_not_pull_the_word_forward() {
        // The first token is timed 9s before the rest, far past MAX_TOKEN_GAP.
        let heard = parse(&transcript(&[(" ho", 100), ("ld", 1000), ("ing", 1010)])).unwrap();
        assert_eq!(heard[0].start, 10.0);
    }

    #[test]
    fn a_held_syllable_is_not_treated_as_a_stray() {
        let heard = parse(&transcript(&[(" ho", 100), ("ld", 350)])).unwrap();
        assert_eq!(heard[0].start, 1.0);
    }

    #[test]
    fn a_word_ends_at_the_next_one_or_after_a_second_whichever_is_sooner() {
        let heard = parse(&transcript(&[(" one", 100), (" two", 140), (" three", 900)])).unwrap();
        assert_eq!(heard[0].end, 1.4);
        assert_eq!(heard[1].end, 2.4);
        assert_eq!(heard[2].end, 10.0);
    }

    #[test]
    fn the_dtw_preset_follows_the_model_filename() {
        assert_eq!(dtw_preset(Path::new("ggml-small.en.bin")).unwrap(), "small.en");
        assert_eq!(dtw_preset(Path::new("/m/ggml-large-v3.bin")).unwrap(), "large.v3");
        assert_eq!(dtw_preset(Path::new("ggml-base.en-q5_1.bin")).unwrap(), "base.en");
    }

    #[test]
    fn a_name_that_implies_no_preset_asks_for_dtw_instead_of_guessing() {
        // whisper rejects an unknown preset several seconds into a run, with an error naming
        // neither the model nor the flag that would fix it.
        for name in ["/models/whisper-small.bin", "model.bin", "ggml-small.en-tdrz.bin"] {
            let error = dtw_preset(Path::new(name)).unwrap_err().to_string();
            assert!(error.contains("--dtw"), "{name}: {error}");
            assert!(error.contains("small.en"), "{name}: {error}");
        }
    }
}
