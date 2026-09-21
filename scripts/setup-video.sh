#!/usr/bin/env bash
set -euo pipefail

repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
command -v cargo >/dev/null || { printf 'Install Rust first: https://rustup.rs\n' >&2; exit 1; }
command -v brew >/dev/null || { printf 'This setup command requires Homebrew. See README.md for manual installation.\n' >&2; exit 1; }

brew install whisper-cpp ffmpeg-full
model=${LOCKSTEP_MODEL:-"$HOME/.cache/whisper/ggml-base.en.bin"}
if [[ ! -s "$model" ]]; then
    mkdir -p -- "$(dirname -- "$model")"
    curl --fail --location --retry 2 \
        https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin \
        --output "$model.partial"
    mv -- "$model.partial" "$model"
fi
cargo build --release --workspace --manifest-path "$repo/Cargo.toml"
printf '\nReady. Run: ./scripts/lyric-video.sh "song.mp3" "lyrics.md"\n'
