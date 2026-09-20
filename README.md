# lockstep

Times a known script against its recording, word by word.

Give it an audio file and the text that was spoken or sung in it. You get back JSON, WebVTT or
LRC saying when every word arrives — enough to drive a lyric video, a karaoke display, or a
follow-along transcript for something that is not music at all.

The words in the output always come from your script, never from the transcriber. Missing or
misheard words borrow neighboring timings, which can produce grouped highlights and inaccurate
word boundaries in the default transcription mode. `--forced-align` instead sends the
supplied lyrics directly to an acoustic aligner and rejects incomplete or invalid results.

A Rust library with a CLI on top. Audio decoding streams through bounded buffers;
the optional forced-alignment backend uses additional memory for its model and alignment path.

## Install

```
cargo build --release          # binary lands in target/release/lockstep
```

Or `cargo install --path .` to put it on your `PATH`.

Timing a recording also needs whisper.cpp and a model:

```
brew install whisper-cpp       # or: apt install whisper.cpp
mkdir -p ~/.cache/whisper
curl -L -o ~/.cache/whisper/ggml-base.en.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin
```

lockstep finds both on its own from there. Decoding and transcript matching are pure Rust.
Transcription and optional acoustic forced alignment use external model runtimes.

### Forced alignment

From the repository, install the optional backend once, then generate timings:

```sh
./scripts/setup-alignment.sh
target/release/lockstep "song.mp3" "lyrics.md" --forced-align -o "song.forced.json"
```

This command produces timing JSON only. It does not render a video. Setup creates an isolated
`.venv-align` environment and builds the Rust binary. The acoustic model downloads on first use.
Set `--alignment-python` or `LOCKSTEP_ALIGNMENT_PYTHON` when using another environment or running
outside the repository. Python 3.14 on macOS was used for the supplied-song comparison.

Rust handles bracket removal, original text, audio decoding, validation, and JSON/VTT/LRC output.
A small embedded Python adapter uses the [CTC aligner](https://github.com/MahmoudAshraf97/ctc-forced-aligner)
model and normalization helpers. A local Viterbi recurrence handles tied scores and repeated
letters; it avoids an upstream tie-selection error found by a synthetic test. Instrumental gaps
are allowed before, between, and after words. Word boundaries exclude blank padding.
No Whisper transcription, lyric prompt, neighboring-word copying, or fixed word-duration cap is used.

The default model is `MahmoudAshraf/mms-300m-1130-forced-aligner`; its model card declares
[CC-BY-NC-4.0](https://huggingface.co/MahmoudAshraf/mms-300m-1130-forced-aligner).
`--alignment-model` accepts another compatible CTC model or a local model directory;
`--alignment-language` defaults to the ISO 639-3 code `eng` for normalization.
Other models and languages have not been validated here. Write numbers as sung words.

Forced results include `alignment.method: "forced"`. Their `alignment.rate` describes supplied-word
coverage, not independent recognition or timing accuracy. Incorrect lyrics can still align to audio;
listen to the result before treating it as verified. Missing, reordered, overlapping, zero-length,
or out-of-track word spans fail before an output file is written. `--keep` retains the decoded WAV
and raw `aligned-words.json` for inspection. `--transcript` cannot be combined with
`--forced-align`. Whisper options and their environment variables have no effect in forced mode.

Run the backend's synthetic acoustic-boundary and repeated-letter tests after setup:

```sh
.venv-align/bin/python tests/forced_backend.py
```

## Use

```
lockstep recording.mp3 script.txt
```

Writes `recording.json` beside the recording, and prints what it managed:

```
recording.json: 42 lines (41/42 anchored), 168/172 script words matched (98%), 175 words heard
```

`-o somewhere.json` picks the output path, `--format vtt` or `--format lrc` writes a standard
subtitle file instead.

**The recording** can be mp3, wav, flac, aac, m4a/alac or ogg. No converting first.

**The script** is plain text, one line per line of output. Blank lines and whole-line bracketed
labels such as `[Verse 1]` are dropped. Punctuation is kept in the output and ignored when
matching; hyphens and dashes separate words, so `honey-sweet` and `servant—whom` each get
two timing anchors while keeping their original punctuation on screen.

## Options

| flag | |
| --- | --- |
| `-o`, `--out` | where to write it. Defaults to the recording's name with the format's extension |
| `--format` | `json` (default), `vtt` for WebVTT, `lrc` for karaoke lyrics |
| `--transcript` | reuse a whisper JSON instead of transcribing again |
| `--model` | model file, also `$LOCKSTEP_MODEL` |
| `--whisper` | whisper.cpp executable, also `$LOCKSTEP_WHISPER` |
| `--dtw` | alignment-heads preset, if the model's filename does not imply one |
| `--no-gpu` | run Whisper on CPU when GPU/Metal is unavailable |
| `--keep` | keep the intermediate WAV and transcript, and say where |
| `--forced-align` | align supplied lyrics directly to audio using the optional CTC backend |
| `--alignment-python` | Python executable for forced alignment; also `$LOCKSTEP_ALIGNMENT_PYTHON` |
| `--alignment-model` | CTC model name or directory |
| `--alignment-language` | normalization language in ISO 639-3 form; defaults to `eng` |

## Lyric videos

`lockstep-video` is a separate workspace crate. It reads Lockstep JSON, writes timed ASS
karaoke subtitles, and asks FFmpeg to encode them with the original audio. FFmpeg must include
the libass-backed `ass` filter. The renderer checks `PATH` and Homebrew's unlinked `ffmpeg-full`
installation automatically; `LOCKSTEP_VIDEO_FFMPEG` selects a specific executable.

On macOS with Rust and Homebrew installed, run setup once, then one command per song:

```sh
./scripts/setup-video.sh
./scripts/lyric-video.sh "song.mp3" "lyrics.md"
```

Setup installs Whisper and FFmpeg, downloads the English base model into `~/.cache/whisper`,
and builds both Rust tools. The song command builds any changed code, aligns on CPU, saves
`song.lockstep.json`, and renders `song lyric video.mp4` beside the audio. No temporary wrappers,
manual section-label removal, or environment variables are needed. Errors stop the workflow.
For manual installation, install Whisper and its model as above, install libass-enabled FFmpeg,
then build with `cargo build --release --workspace`.

Render again with saved timings when changing styling:

```sh
./scripts/lyric-video.sh "song.mp3" "lyrics.md" --reuse-timings --font 'Avenir Next' --font-size 64
```

Omit `--reuse-timings` after changing audio or lyrics. Remaining options go to `lockstep-video`;
`-o path.mp4` chooses the output. `LOCKSTEP_MODEL` and `LOCKSTEP_WHISPER` can select an installed
model or Whisper executable. To use pre-existing timing JSON directly:

```
cargo build --release -p lockstep-video
target/release/lockstep-video recording.json recording.mp3 -o recording.mp4
```

After alignment setup, the combined workflow accepts `--forced-align` immediately after the lyrics:

```sh
./scripts/lyric-video.sh "song.mp3" "lyrics.md" --forced-align --background-color '#0B1730'
```

That combined command also renders a video. Use `lockstep --forced-align` alone for timing work.

Defaults: dark blue (`#0B1730`) 1920x1080 canvas, white 72-pixel sans-serif text, soft cyan
(`#67E8F9`) for the active word, two lines per page, and 30 FPS. Each page shows its lines
together, in order, and keeps every line visible until the page's final sung word ends.
Then the whole page changes; upcoming pages never replace individual rows. Words ease into cyan
over 80ms from their recorded onset and back to white before their end or the next onset.
Both fades shorten for quick words. Completed and upcoming words remain white; internal
pauses have no highlighted word. Leading and trailing punctuation stays white while the word
itself changes color; internal apostrophes remain part of the word. Words sharing a timing span still highlight together because the input
does not distinguish their onsets. `--highlight-transition-ms 0` switches instantly.
`--highlight-style underline` keeps every lyric white and places a soft pill beneath the active
word. The pill holds through breaths, resizes and glides into place before the next word starts,
and fades only when the page changes. Font shaping keeps it centered beneath proportional text;
leading and trailing punctuation do not affect its size.
`--highlight-words false` disables all per-word color and animation while preserving the lyric
pages and instrumental notes. The Rust library exposes the same `Style::highlight_words` boolean,
which defaults to `true`. Notes appear centered only while no lyric page is visible.

The palette and timing are design choices: strong text/background contrast follows
[W3C readability guidance](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html),
while fixed text geometry keeps the highlight from moving surrounding words.

```
lockstep-video recording.json recording.mp3 -o recording.mp4 \
  --width 1280 --height 720 --frames-per-second 24 \
  --background-color '#112233' --text-color '#DDEEFF' \
  --highlight-color '#FFCC00' --font 'Avenir Next' --font-size 64 \
  --lines 3 --rest-text '♪ ♫' --highlight-style underline
```

One image without timestamps stays for the whole video. Images fit inside the canvas, centered
without cropping, with their aspect ratio preserved. `--background-color` fills the space around
each image and any gaps in the image schedule. Give every image a half-open timestamp range when
using more than one.

```
lockstep-video recording.json recording.mp3 -o recording.mp4 \
  --background-image cover.jpg --background-color '#0B1730' --highlight-words false
```

```
lockstep-video recording.json recording.mp3 -o recording.mp4 \
  --background-image '00:00:00.000..00:00:12.500=intro.jpg' \
  --background-image '00:00:12.500..00:01:03.000=verse.jpg'
```

## Output

```json
{
  "version": 1,
  "generator": "lockstep 0.1.0",
  "script": "script.txt",
  "duration": 180.0,
  "alignment": { "matched": 166, "words": 170, "rate": 0.98 },
  "lines": [
    {
      "start": 11.7,
      "end": 16.5,
      "text": "The first line",
      "words": [
        { "start": 12.0, "end": 12.4, "text": "The" },
        { "start": 12.4, "end": 13.1, "text": "first" },
        { "start": 13.1, "end": 16.5, "text": "line" }
      ]
    }
  ]
}
```

Times are seconds. In transcription mode, `alignment.rate` is the share of your script matched
against what Whisper heard. With `alignment.method: "forced"`, it measures supplied-word coverage
and cannot detect a mismatched recording. `version` lets consumers refuse an unknown shape.

A line's `start` is its display time, up to 300ms before the first word. Word starts and ends
retain their recorded timing; the display lead does not shift or shorten them. Words can have
gaps, and a line's `end` includes its final word.

**Silence is a gap, not an entry.** Use word spans for sung intervals; a later line's preview
can start before the preceding line finishes singing. The run-out is between the last line's
`end` and `duration`.

`timing` says how a word got its time, and appears **only when that time was not measured**:

| `timing` | |
| --- | --- |
| absent | measured — the word aligned to something heard |
| `carried` | the line was heard but this word was not, so it takes its neighbour's time exactly. Usually punctuation, or a word the transcriber misheard |
| `spread` | nothing in the line was heard, so its words are spaced evenly between the nearest anchors. Treat these as placeholders |

Two words with the same span are timed as a unit and should be highlighted together.
Punctuation timestamps cannot replace a word's onset; late punctuation can extend a held
word's end. Whisper still estimates timing, and words marked `carried` or `spread` remain guesses.

### WebVTT and LRC

```
WEBVTT

00:00:11.700 --> 00:00:16.500
<00:00:12.000>The <00:00:12.400>first <00:00:13.100>line
```

```
[00:11.70]<00:12.00>The <00:12.40>first <00:13.10>line
```

Both carry the word times but not `timing`, so use JSON if you need to know which words were
guessed.

## When the script and the recording do not match

Hand it the wrong text and it still produces a file — every line gets a plausible-looking
timestamp. So it tells you instead. Correctly paired files match around 98% of the script;
mismatched ones under 20%. Below 50% lockstep warns:

```
warning: only 8% of wrong-script.txt was heard in recording.mp3. A correctly paired script and
recording match around 98%, so these two are probably not the same piece, or the model cannot
hear this language. The timings written are guesses.
```

It still writes the file: a genuinely hard recording is not the same as a wrong one. To gate on
this in a script, read `alignment.rate` from the JSON.

A low score usually means the wrong file, an `.en` model against non-English audio, or a vocal
the model cannot make out.

## How it works

1. **Decode.** The recording is decoded and mixed to mono 16 kHz in pure Rust, so no external
   converter is involved.
2. **Transcribe.** whisper.cpp runs with token-level DTW timestamps. Its words are used only as
   clocks.
3. **Align.** Your script is matched against that transcript with Needleman-Wunsch, so a word
   whisper missed or misheard shifts nothing around it.
4. **Place.** Each line takes the span of its matched words; lines nobody heard are spread
   between their timed neighbours.

## Finding whisper and the model

Both are found automatically, so the usual install needs no flags:

| | order |
| --- | --- |
| binary | `--whisper`, `$LOCKSTEP_WHISPER`, then `whisper-cli` / `whisper-cpp` / `whisper` on `PATH` |
| model | `--model`, `$LOCKSTEP_MODEL`, then `./models`, `~/.cache/whisper`, `~/.local/share/whisper`, `~/Library/Application Support/whisper`, `{/opt/homebrew,/usr/local,/usr}/share/whisper.cpp/models` |

When several models are installed, **base is preferred over a bigger one** for speed.
Recognition errors become borrowed timestamps, which can be early or too short.
`--model` selects a larger installed model for difficult recordings; it does not guarantee
accurate lyrics or timing.

The `-dtw` preset is read from the model's filename, so `ggml-large-v3.bin` selects `large.v3`
by itself. A name implying no preset it knows is refused up front rather than failing several
seconds into a whisper run.

### Running without whisper installed

`--transcript` reuses a whisper JSON produced elsewhere and skips transcription. Everything else
is pure Rust, so this runs on a machine with nothing installed — useful when transcription
happens on a GPU box and the timing does not. The recording is still read, for its duration.

```
lockstep recording.mp3 script.txt --transcript from-elsewhere.json
```

## Using it from Rust

```rust
use lockstep::export::{render, Format};
use std::path::Path;

let lines = lockstep::script::read(Path::new("script.txt"))?;
let duration = lockstep::audio::prepare(Path::new("song.mp3"), Some(Path::new("/tmp/a.wav")))?;
let transcript = lockstep::whisper::transcribe(Path::new("/tmp/a.wav"), &Default::default())?;
let heard = lockstep::whisper::parse(&transcript)?;

let (document, report) = lockstep::timing::build(&lines, &heard, Path::new("script.txt"), duration)?;
println!("{}", render(&document, Format::Vtt)?);
```

`audio::stream` hands decoded mono 16 kHz samples to a closure a piece at a time, if you want
them somewhere other than a WAV file.

Memory stays around 3 MB whatever the length, because nothing holds the recording. Transcription
is a subprocess, so the model's ~450 MB is spent and returned rather than held for the life of
your process.

## Other tools

The general problem is called **forced alignment**, if you want to read around it.
[aeneas](https://github.com/readbeyond/aeneas) and the
[Montreal Forced Aligner](https://montreal-forced-aligner.readthedocs.io) are the established
options; both align acoustically at the phoneme level rather than leaning on a transcriber, and
both come with a Python or Kaldi stack. lockstep has not been benchmarked against either.

## Limits

- Alignment is exact rather than banded, so it holds an `n × m` score matrix. Past about ten
  thousand words on each side it refuses rather than exhausting memory; split long recordings.
- Accuracy is roughly a tenth of a second, which is what the DTW timestamps are worth.
- An `.en` model only transcribes English. Using the wrong one shows up as a low `rate`.
- `-nfa` is passed always: whisper.cpp enables flash attention by default, which silently
  disables DTW and leaves every word in a segment sharing one timestamp.
- Encode audio CBR if a browser will seek it. Browsers seek VBR through a 100-entry table of
  contents, so a seek can land seconds from the requested time and correct timings look broken.

## Development

```
cargo test --workspace
LOCKSTEP_VIDEO_FFMPEG=/opt/homebrew/opt/ffmpeg-full/bin/ffmpeg \
  cargo test -p lockstep-video --test render -- --ignored
```

The optional rendered tests require libass and Arial. They compare encoded frames to catch
preview movement and verify a brief highlight transition at onset.

`unsafe_code = "forbid"` is set on the crate. Minimum supported Rust is 1.85, set by clap and
verified by building and testing on that toolchain.
