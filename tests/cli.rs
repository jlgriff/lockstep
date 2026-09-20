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
