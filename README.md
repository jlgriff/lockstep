# lockstep

Times a known script against its recording, word by word.

Give it an audio file and the text that was spoken or sung in it. You get back JSON, WebVTT or
LRC saying when every word arrives — enough to drive a lyric video, a karaoke display, or a
follow-along transcript for something that is not music at all.

The words in the output always come from your script, never from the transcriber. A misheard
word costs a little precision on that line rather than putting the wrong text on screen.

A Rust library with a CLI on top. 3 MB binary, ~3 MB of memory however long the recording is.

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

lockstep finds both on its own from there. Decoding and alignment are pure Rust — only
transcription needs anything installed.

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

**The script** is plain text, one line per line of output. Blank lines are dropped. Punctuation
is kept in the output and ignored when matching, so `There's` and `theres` are the same word to
the aligner.

## Options

| flag | |
| --- | --- |
| `-o`, `--out` | where to write it. Defaults to the recording's name with the format's extension |
| `--format` | `json` (default), `vtt` for WebVTT, `lrc` for karaoke lyrics |
| `--transcript` | reuse a whisper JSON instead of transcribing again |
| `--model` | model file, also `$LOCKSTEP_MODEL` |
| `--whisper` | whisper.cpp executable, also `$LOCKSTEP_WHISPER` |
| `--dtw` | alignment-heads preset, if the model's filename does not imply one |
| `--keep` | keep the intermediate WAV and transcript, and say where |

## Lyric videos

`lockstep-video` is a separate workspace crate. It reads Lockstep JSON, writes timed ASS
karaoke subtitles, and asks FFmpeg to encode them with the original audio. FFmpeg must include
the libass-backed `ass` filter.

```
cargo build --release -p lockstep-video
target/release/lockstep-video recording.json recording.mp3 -o recording.mp4
```

Its defaults are a black 1920x1080 canvas, white 72-pixel sans-serif text, a gold current-word
highlight, two displayed lines, and 30 frames per second. Every value is configurable:

```
lockstep-video recording.json recording.mp3 -o recording.mp4 \
  --width 1280 --height 720 --frames-per-second 24 \
  --background-color '#112233' --text-color '#DDEEFF' \
  --highlight-color '#FFCC00' --font 'Avenir Next' --font-size 64 \
  --lines 3 --rest-text '♪ ♫'
```

One unqualified image fills the full video. Give every image a half-open timestamp range when
using more than one; uncovered time keeps the background color.

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
      "start": 12.0,
      "end": 16.5,
      "text": "The first line as it is printed",
      "words": [
        { "start": 12.0, "end": 12.4, "text": "The" },
        { "start": 12.4, "end": 13.1, "text": "first" },
        { "start": 13.1, "end": 16.5, "text": "line", "timing": "carried" }
      ]
    }
  ]
}
```

Times are seconds. `alignment.rate` is the share of your script the recording was heard to say —
the number to gate on. `version` is there so a consumer can refuse a shape it does not know.

A word's keys are a line's keys, meaning the same thing at either level. Words fill their line
end to end, each running until the next begins.

**Silence is a gap, not an entry.** A pause is the space between one line's `end` and the next
line's `start`; the run-out is between the last line's `end` and `duration`.

`timing` says how a word got its time, and appears **only when that time was not measured**:

| `timing` | |
| --- | --- |
| absent | measured — the word aligned to something heard |
| `carried` | the line was heard but this word was not, so it takes its neighbour's time exactly. Usually punctuation, or a word the transcriber misheard |
| `spread` | nothing in the line was heard, so its words are spaced evenly between the nearest anchors. Treat these as placeholders |

Two words with the same span are timed as a unit and should be highlighted together.

### WebVTT and LRC

```
WEBVTT

00:00:12.000 --> 00:00:16.500
The <00:00:12.400>first <00:00:13.100>line
```

```
[00:12.00]<00:12.00>The <00:12.40>first <00:13.10>line
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

When several models are installed, **base is preferred over a bigger one**. That is deliberate:
whisper only supplies the clock here, so a weaker model costs borrowed timestamps rather than
wrong ones. `base.en` agreed with `small.en` to within lockstep's own tenth-of-a-second
precision while using a third of the memory and running twice as fast.

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
cargo test
```

`unsafe_code = "forbid"` is set on the crate. Minimum supported Rust is 1.85, set by clap and
verified by building and testing on that toolchain.
