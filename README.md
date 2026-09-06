# lockstep

Times a known script against its recording, word by word.

Give it an audio file and the text that was spoken or sung in it. You get back JSON saying when
every word arrives — enough to drive a lyric video, a karaoke display, or a follow-along
transcript for something that is not music at all.

The words in the output always come from your script, never from the transcriber. A misheard
word costs a little precision on that line rather than putting the wrong text on screen.

## Install

```
cargo build --release          # binary lands in target/release/lockstep
```

Or `cargo install --path .` to put it on your `PATH`.

Timing a recording also needs whisper.cpp and a model:

```
brew install whisper-cpp       # or: apt install whisper.cpp
mkdir -p ~/.cache/whisper
curl -L -o ~/.cache/whisper/ggml-small.en.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin
```

lockstep finds both of those on its own from there. Decoding and alignment are pure Rust — only
transcription needs anything installed.

## Use

```
lockstep recording.mp3 script.txt
```

That writes `recording.json` beside the recording, and prints what it managed:

```
recording.json: 42 lines (41/42 anchored), 168/172 script words matched (98%), 175 words heard
```

`-o somewhere.json` picks the output path, and `--format vtt` or `--format lrc` writes a
standard subtitle file instead.

**The recording** can be mp3, wav, flac, aac, m4a/alac or ogg. No converting first.

**The script** is plain text, one line per line of output. Blank lines are dropped. Punctuation
is kept in the output and ignored when matching, so `There's` and `theres` are the same word to
the aligner.

```
The first line as it is printed
The second line, punctuation and all
```

## Output

Three formats. `--format json` is the default and the only one carrying per-word provenance;
the other two are the standards, for players that already read them.

| `--format` | | |
| --- | --- | --- |
| `json` | lockstep's own | full detail, including how each word got its time |
| `vtt` | WebVTT | a browser plays it natively from a `<track>` element |
| `lrc` | Enhanced LRC | music players read it as karaoke lyrics |

### json

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

| field | |
| --- | --- |
| `version` | this file format. Refuse a version you do not know |
| `generator` | which lockstep wrote it |
| `script` | the script's filename, never the path it was read from |
| `alignment` | how much of the script was actually heard: the counts, and the rate to gate on |
| `start`, `end` | seconds |

A word's keys echo a line's, so the same name means the same thing at either level. Words fill
their line end to end: the first starts where the line starts, the last ends where it ends, and
each runs until the next begins.

**Silence is a gap, not an entry.** A pause is the space between one line's `end` and the next
line's `start`, and the run-out is the space between the last line's `end` and `duration`. There
are no placeholder entries to skip over.

The one extra key is `timing`, which says how a word got its time and is **written only when
that time was not measured**. Top level, `"timing"` is absent for the same reason it is on a
measured word: nothing needed saying.

| `timing` | meaning |
| --- | --- |
| absent | the word aligned to something heard in the recording. Its time is measured |
| `carried` | the line was heard but this word was not, so it takes its neighbour's time exactly. Usually punctuation, or a word the transcriber misheard |
| `spread` | nothing in the line was heard, so its words are spaced evenly between the nearest anchors. Treat these as placeholders |

A `carried` word shares its neighbour's span rather than abutting it, because sharing a time is
what carrying one means. Two words with the same span are timed as a unit and should be
highlighted together.

Marking only the exceptions keeps the ordinary case clean and makes the handful of words worth
reviewing easy to find. A correctly paired song typically has four to six `carried` and no
`spread` out of a couple of hundred words.

There is deliberately no per-word confidence *number*. Nothing continuous produces one, so any
float here would be one of three constants dressed up as a measurement.

### vtt

```
WEBVTT

00:00:12.000 --> 00:00:16.500
The <00:00:12.400>first <00:00:13.100>line
```

The first word carries no tag because the cue already starts there. Words sharing a time share a
tag, since WebVTT requires a cue's inline timestamps to strictly increase.

### lrc

```
[00:12.00]<00:12.00>The <00:12.40>first <00:13.10>line
```

Enhanced LRC, with a line stamp followed by a stamp per word. Minutes keep counting past sixty,
as the format has no hours field.

## Options

| flag | |
| --- | --- |
| `-o`, `--out` | where to write it. Defaults to the recording's name with the format's extension |
| `--format` | `json`, `vtt` or `lrc`. Defaults to `json` |
| `--transcript` | reuse a whisper JSON instead of transcribing again |
| `--model` | model file, also `$LOCKSTEP_MODEL` |
| `--whisper` | whisper.cpp executable, also `$LOCKSTEP_WHISPER` |
| `--dtw` | alignment-heads preset, if the model's filename does not imply one |
| `--keep` | keep the intermediate WAV and transcript, and say where |

## When the script and the recording disagree

Hand it the wrong text for a recording and it will still produce a file — every line gets a
plausible-looking timestamp, because lines nobody heard are spread evenly between the ones that
were. Nothing about the output looks broken. So the tool says so instead.

`matched` separates the two cases sharply. Measured across nine pairings of three recordings and
three scripts:

| | words matched | lines anchored |
| --- | --- | --- |
| correct pairing | 98%, 99%, 98% | 100% |
| wrong pairing | 8%, 8%, 9%, 9%, 16%, 16% | 37-68% |

Nothing lands between 16% and 98%. Line anchoring is the weaker signal, because a line anchors on
a single word and common words match by chance.

Below 50% lockstep warns, naming both files:

```
$ lockstep recording.mp3 wrong-script.txt
recording.json: 55 lines (20/55 anchored), 25/300 script words matched (8%), 170 words heard
warning: only 8% of wrong-script.txt was heard in recording.mp3. A correctly paired script and
recording match around 98%, so these two are probably not the same piece, or the model cannot
hear this language. The timings written are guesses.
```

It still writes the file either way: a genuinely hard recording is not the same thing as a wrong
one, and that call is yours. A pipeline that wants to gate should read `matched` from the JSON.

A low score has three causes worth checking in order: the wrong file, an `.en` model against
non-English audio, and a recording whose vocal the model genuinely cannot make out.

## How it works

1. **Decode.** The recording is decoded and mixed to the mono 16 kHz signal whisper needs, in
   pure Rust via symphonia, so no external converter is involved.
2. **Transcribe.** whisper.cpp runs with token-level DTW timestamps, producing times for what it
   heard. Its words are used only as clocks.
3. **Align.** Your script is matched against that transcript with Needleman-Wunsch, so a word
   whisper missed, invented or misheard shifts nothing around it.
4. **Place.** Each line takes the span of its matched words, and lines nobody heard are spread
   between their timed neighbours. Silence is simply the gap left between lines.

## Finding whisper and the model

Both are looked for in order, so the usual install needs no flags at all:

| | order |
| --- | --- |
| binary | `--whisper`, `$LOCKSTEP_WHISPER`, then `whisper-cli` / `whisper-cpp` / `whisper` on `PATH` |
| model | `--model`, `$LOCKSTEP_MODEL`, then `./models`, `~/.cache/whisper`, `~/.local/share/whisper`, `~/Library/Application Support/whisper`, `{/opt/homebrew,/usr/local,/usr}/share/whisper.cpp/models` |

When several models sit in one directory the largest is taken, being the most capable.

The `-dtw` alignment-heads preset is read from the model's filename, so `ggml-large-v3.bin`
selects `large.v3` without being told. A filename implying no preset it recognises is refused up
front, listing the eleven valid values, rather than being handed to whisper to be rejected
several seconds into a run. `--dtw` overrides that guess and is passed through unchecked, so a
preset added by a newer whisper still works.

### Running without whisper installed

`--transcript` takes a whisper JSON produced elsewhere and skips transcription entirely.
Everything else is pure Rust, so this runs on a machine with nothing installed — useful when
transcription happens on a GPU box or in CI and the timing does not.

```
lockstep recording.mp3 script.txt --transcript from-elsewhere.json
```

The recording is still read, for its duration.

## Details that turned out to matter

**Flash attention silently disables DTW.** whisper.cpp enables it by default, and with it on
every word in a segment shares one timestamp, which puts lines seconds out. lockstep always
passes `-nfa`.

**A word's onset comes from its last unbroken run of tokens.** DTW occasionally times a word's
first token many seconds before the rest of it, which would light the whole line up early. The
threshold is a deliberately loose 3s, because a held syllable is not a stray.

**The resampler's group delay is trimmed.** Left in, it pushes every timestamp late by the same
~21 ms. Cross-correlating the output against ffmpeg's shows a 341-sample offset before the trim
and none after.

**Encode your audio CBR if a browser will seek it.** Browsers seek a VBR file through a
100-entry table of contents, so a seek can land seconds from the requested time while
`currentTime` reports the value you asked for — and timings that were correct will look broken.

## Limits

- Alignment is exact rather than banded, so it holds an `n × m` score matrix. Past about ten
  thousand words on each side it refuses rather than exhausting memory; split long recordings
  into sections.
- Accuracy is roughly a tenth of a second, which is what the DTW timestamps are worth.
- An `.en` model only transcribes English. Using the wrong one shows up as a low `matched` score.
- The 50% warning threshold sits in the gap between the two cases measured above. Those
  measurements are all clean studio vocals against an English model, so it is a tripwire for an
  obvious mistake rather than a calibrated confidence bound.

## Development

```
cargo test
```

`unsafe_code = "forbid"` is set on the crate, so no unsafe can be written in it. The minimum
supported Rust is 1.85, set by clap and verified by building and testing on that toolchain.
