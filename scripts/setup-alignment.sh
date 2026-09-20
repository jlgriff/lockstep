#!/usr/bin/env bash
set -euo pipefail

repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
python3 -m venv "$repo/.venv-align"
"$repo/.venv-align/bin/python" -m pip install -r "$repo/scripts/alignment-requirements.txt"
cargo build --release --manifest-path "$repo/Cargo.toml"
printf '\nReady. From this repository, run: target/release/lockstep "song.mp3" "lyrics.md" --forced-align\n'
printf 'The alignment model downloads on first use. This command generates timings only.\n'
