# Lyric-video contract

This crate contains failing contracts and API/CLI scaffolding. Planning, image-spec
parsing, validation, and video encoding are not implemented.

## Boundaries

`parse_document` reads versioned Lockstep JSON. `plan` also validates documents built
directly by Rust callers. Neither depends on the Lockstep implementation crate.
Unneeded metadata and word-timing provenance do not affect display.
Lockstep can omit `words`; those lines display as plain text without invented timings.

`plan` is pure: no image reads, font discovery, subprocesses, or output writes.
Its color source remains beneath images, including during gaps. Image paths stay
structured in `BackgroundCue`; they are not interpolated into a filter expression.
`render` owns file I/O and FFmpeg invocation. CLI defaults come from `Style::default`.
The planner stub returns an error, never a successful empty artifact.

## Time and display

All library times are seconds relative to track start. Display and image ranges are
half-open: start included, end excluded. Adjacent ranges do not overlap.
Track duration must be finite and positive. Use the supplied timestamps without
adding a lead offset; Lockstep already accounts for its own display lead.

`line_count` counts source lyric lines: current line first, followed by upcoming
lines. A new line replaces the previous display window; old lines are not retained.
Lockstep guarantees increasing line starts but can emit overlapping or zero-length
spans. A newer start ends the older display window; empty spans are skipped.
Only gaps between display windows become musical notes, including intro and outro.
An empty lyric track is one full-length note cue.

Highlight each whole word at its onset and retain its highlight until its line
leaves the screen. Words sharing a span form one karaoke group, so carried words
do not advance timing twice. Advance karaoke timing to the next group's start,
including any inter-word gap, and use the final end for the last group. Summing
word durations alone would pull later highlights early when there are gaps.
This retains the existing cumulative karaoke design.

ASS is the planned rendering interchange format. Generate one dialogue event per
display window or rest, not one event per video frame or word. Use two styles:
`Lyrics` (highlight primary color, text secondary color) and `Plain` (text color
for both). Upcoming lines start with a `Plain` reset and explicit line breaks;
notes and lines without word timings also use `Plain`.
Escape literal lyric braces in current and preview text.

ASS uses centiseconds; Lockstep already emits hundredths of seconds. Convert
absolute endpoints to centiseconds before subtracting to obtain karaoke durations;
do not accumulate rounded deltas. Background schedules retain millisecond precision
independently of ASS timing. Visible changes occur on encoded frame boundaries.

## Background images

The library accepts `Vec<BackgroundImage>`. No images means solid color. One image
without a range fills `[0, duration)`; one with a range keeps that range. Multiple
images require an explicit range on every entry. Array index zero is not special.

Validate finite endpoints and `0 <= start < end <= duration` before sorting a copy
by start time and checking adjacent entries for overlap. This covers partial and
contained overlaps in any input order without an all-pairs comparison. Return
sorted cues with required `TimeRange` values; gaps retain the background color.
Do not round image endpoints to ASS precision or clamp invalid inputs.

Repeat the CLI flag for multiple images:

```text
--background-image cover.jpg
--background-image '00:00:02.500..00:00:05.000=verse.jpg'
```

Timestamps use the full `HH:MM:SS.mmm` form of
[WebVTT timestamps](https://www.w3.org/TR/webvtt1/#webvtt-timestamp).
The `..` and `=` delimiters are this tool's CLI syntax, not a WebVTT file format.
Minutes and seconds are 00–59; milliseconds have three digits. Hours may exceed 99.
Preserve spaces, Unicode, relative paths, and equals signs in image paths. A malformed
timestamp prefix must produce a useful error rather than becoming a filename.

## Validation and verification

Reject zero dimensions, frame rate, font size, or line count; malformed `#RRGGBB`
colors; and font names containing ASS field/record delimiters. Error messages must
identify the invalid field or range so an unrelated failure cannot satisfy tests.

The contracts inspect declared ASS fields and complete event schedules, not a full
byte-for-byte file snapshot. Invalid cases have separate test identities so an early
panic does not hide later cases. No rendering dependency is needed for these tests.

```sh
cargo test --workspace --no-fail-fast
```

Video tests intentionally fail until implementation. Before calling the encoder
complete, add a small rendered fixture that verifies audio, duration, image changes,
text colors, and preview behavior using FFmpeg/ffprobe and a fixed test font. Image
fitting, long-line layout, font fallback, literal backslashes, and audio-duration
mismatches still need renderer-level decisions and verification; string contracts
do not establish pixel correctness.

FFmpeg must include the libass-backed `ass` filter; some FFmpeg builds omit it.
Check filter availability explicitly and report a useful error. See
[FFmpeg's ASS filter](https://ffmpeg.org/ffmpeg-filters.html#ass) and
[Aegisub's karaoke and style-reset semantics](https://aegisub.org/docs/latest/ass_tags/#karaoke-effect).
