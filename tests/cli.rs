#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::process::Command;

/// Verifies CPU selection reaches Whisper and produces timings through the real CLI.
#[test]
fn cpu_transcription_needs_no_external_wrapper() {
    let dir = std::env::temp_dir().join(format!("lockstep-cpu-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("audio.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&wav, spec).unwrap();
    for _ in 0..16000 {
        writer.write_sample(0_i16).unwrap();
    }
    writer.finalize().unwrap();
    let script = dir.join("lyrics.txt");
    std::fs::write(&script, "[Verse]\nOne").unwrap();
    let whisper = dir.join("whisper");
    std::fs::write(
        &whisper,
        r#"#!/bin/sh
cpu=no
out=
while [ "$#" -gt 0 ]; do
  case "$1" in
    -ng) cpu=yes ;;
    -of) shift; out=$1 ;;
  esac
  shift
done
[ "$cpu" = yes ] || { echo 'CPU flag missing' >&2; exit 1; }
printf '%s' '{"transcription":[{"tokens":[{"text":" One","t_dtw":50}]}]}' > "$out.json"
"#,
    )
    .unwrap();
    std::fs::set_permissions(&whisper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let output_path = dir.join("timings.json");
    let output = Command::new(env!("CARGO_BIN_EXE_lockstep"))
        .args([wav.as_os_str(), script.as_os_str()])
        .arg("--no-gpu")
        .arg("--whisper")
        .arg(&whisper)
        .args(["--model", "ggml-base.en.bin", "-o"])
        .arg(&output_path)
        .env("LOCKSTEP_ALIGNMENT_PYTHON", "unused-in-transcription-mode")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&output_path).unwrap()).unwrap();
    assert_eq!(document["lines"].as_array().unwrap().len(), 1);
    assert_eq!(document["lines"][0]["text"], "One");
    std::fs::remove_dir_all(dir).unwrap();
}

/// Sends cleaned lyrics and decoded audio to the forced backend through the actual CLI.
#[test]
fn forced_alignment_receives_the_lyrics_and_keeps_word_endpoints() {
    let dir = std::env::temp_dir().join(format!("lockstep-forced-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wav = dir.join("audio.wav");
    let mut writer = hound::WavWriter::create(
        &wav,
        hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for _ in 0..64000 {
        writer.write_sample(0_i16).unwrap();
    }
    writer.finalize().unwrap();
    let script = dir.join("lyrics.txt");
    std::fs::write(&script, "[Verse]\nOne two").unwrap();
    let python = dir.join("python");
    std::fs::write(
        &python,
        r#"#!/bin/sh
[ "$1" = '-c' ] && [ -s "$3" ] || exit 1
cat > "$(dirname "$0")/input.json"
printf '%s' '[{"text":"One","start":1.0,"end":1.3},{"text":"two","start":2.0,"end":3.8}]'
"#,
    )
    .unwrap();
    let whisper = dir.join("whisper-cli");
    std::fs::write(
        &whisper,
        r#"#!/bin/sh
while [ "$#" -gt 0 ]; do
  if [ "$1" = '-of' ]; then shift; out=$1; fi
  shift
done
printf '%s' '{"transcription":[{"tokens":[{"text":" One","t_dtw":100}]}]}' > "$out.json"
"#,
    )
    .unwrap();
    for executable in [&python, &whisper] {
        std::fs::set_permissions(executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut paths = vec![dir.clone()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let output_path = dir.join("timings.json");
    let output = Command::new(env!("CARGO_BIN_EXE_lockstep"))
        .arg(&wav)
        .arg(&script)
        .arg("--forced-align")
        .arg("--alignment-python")
        .arg(&python)
        .arg("-o")
        .arg(&output_path)
        .env("PATH", std::env::join_paths(paths).unwrap())
        .env("LOCKSTEP_MODEL", "unused-in-forced-mode.bin")
        .env("LOCKSTEP_WHISPER", "unused-in-forced-mode")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        dir.join("input.json").is_file(),
        "forced alignment backend was never called"
    );
    let input: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("input.json")).unwrap()).unwrap();
    assert_eq!(input, serde_json::json!(["One", "two"]));
    let document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output_path).unwrap()).unwrap();
    assert_eq!(document["alignment"]["method"], "forced");
    assert_eq!(document["lines"][0]["words"][1]["start"], 2.0);
    assert_eq!(document["lines"][0]["words"][1]["end"], 3.8);
    assert!(document["lines"][0]["words"][1].get("timing").is_none());
    std::fs::remove_dir_all(dir).unwrap();
}
