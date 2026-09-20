#!/usr/bin/env bash
set -euo pipefail

if [[ ${1:-} == --help || $# -lt 2 ]]; then
    printf 'Usage: %s AUDIO LYRICS [--reuse-timings|--forced-align] [VIDEO_OPTIONS...]\n' "$0"
    printf 'Creates AUDIO.lockstep.json and AUDIO lyric video.mp4. Pass -o to choose the video path.\n'
    printf 'Use --reuse-timings immediately after LYRICS to restyle without transcribing again.\n'
    printf 'Use --forced-align immediately after LYRICS for supplied-text alignment; run scripts/setup-alignment.sh first.\n'
    if [[ ${1:-} == --help ]]; then exit 0; else exit 1; fi
fi

repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
audio=$1
lyrics=$2
shift 2
timings="${audio%.*}.lockstep.json"
reuse=false
forced=false
if [[ ${1:-} == --reuse-timings ]]; then
    reuse=true
    shift
elif [[ ${1:-} == --forced-align ]]; then
    forced=true
    shift
fi

cargo build --quiet --release --workspace --manifest-path "$repo/Cargo.toml"
if [[ "$reuse" == true ]]; then
    [[ -s "$timings" ]] || { printf 'No saved timings: %s\n' "$timings" >&2; exit 1; }
elif [[ "$forced" == true ]]; then
    "$repo/target/release/lockstep" "$audio" "$lyrics" --forced-align \
        --alignment-python "${LOCKSTEP_ALIGNMENT_PYTHON:-$repo/.venv-align/bin/python}" -o "$timings"
else
    "$repo/target/release/lockstep" "$audio" "$lyrics" --no-gpu -o "$timings"
fi
printf 'Rendering lyric video...\n'
"$repo/target/release/lockstep-video" "$timings" "$audio" "$@"
