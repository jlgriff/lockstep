# Lyric-video contract

This crate implements lyric-video planning, image-spec parsing, validation, and FFmpeg
encoding behind its own library and minimal CLI.

## Boundaries

`parse_document` reads versioned Lockstep JSON. `plan` also validates documents built
directly by Rust callers. Neither depends on the Lockstep implementation crate.
Unneeded metadata and word-timing provenance do not affect display.
Lockstep can omit `words`; those lines display as plain text without invented timings.

`plan` is pure: no image reads, font discovery, subprocesses, or output writes.
Its color source remains beneath images, including during gaps. Image paths stay
structured in `BackgroundCue`; they are not interpolated into a filter expression.
`render` owns file I/O and FFmpeg invocation. CLI defaults come from `Style::default`.

## Time and display

All library times are seconds relative to track start. Display and image ranges are
half-open: start included, end excluded. Adjacent ranges do not overlap.
Track duration must be finite and positive. Use the supplied timestamps without
adding a lead offset; Lockstep already accounts for its own display lead.

`line_count` counts source lines per page. Consecutive groups of that size appear together
in reading order at the first line's onset and stay until the last line's end. All rows
share the page's display interval; completed lines remain visible in plain text. No line
from the next page appears early. The final incomplete page uses the same fixed row positions.
Each source line has one ASS event with an explicit `\pos`. A new lyric's start truncates
an overlapping older sung span without removing its text mid-page; zero-length spans are
skipped before grouping. Gaps between sung spans show notes below the page's fixed rows.

Only the active word is highlighted. The default is soft cyan (`#67E8F9`) over navy, with
white surrounding text. An 80ms ease-out at onset brings in the accent promptly; an 80ms
ease-in returns to white by the earlier of word end, the next distinct onset, or line end.
Each fade is capped at half that interval, so short words do not overlap transitions.
Empty spans stay plain. Held words retain the accent until their closing fade; gaps have
no active highlight. Words sharing an onset can highlight together because the source
does not distinguish them. Times are absolute offsets from the event's preview start;
gaps cannot accumulate timing drift. Glyph positions and original punctuation spacing stay fixed.

ASS is the rendering interchange format. Generate one dialogue event per lyric or rest,
not one event per video frame or word. Use two styles:
`Lyrics` (highlight primary color, text secondary color) and `Plain` (text color
for both). Every word uses a `Plain` reset and bounded ASS color transforms. Notes and lines
without word timings use `Plain`. Setting `highlight_transition_ms` to zero uses 1ms changes
ending at the onset and endpoint; a word already active at event start uses a static accent.
These avoid ASS's special zero-duration transform semantics while remaining instant at video frame rates.
Escape literal lyric braces in current and preview text.

ASS uses centiseconds; Lockstep already emits hundredths of seconds. Convert
absolute endpoints to centiseconds before subtracting to obtain animation offsets;
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

The contract suite verifies timing, styling, validation, image schedules, and ASS events
without requiring a renderer. The optional `tests/render.rs` suite encodes short fixtures
using FFmpeg/libass and Arial, then checks stable row positions and highlight transitions
in decoded frames. Run it with `cargo test -p lockstep-video --test render -- --ignored`,
setting `LOCKSTEP_VIDEO_FFMPEG` if FFmpeg is not on PATH. Image fitting uses center-cropped
cover scaling. Long-line layout, font fallback, literal backslashes, and audio-duration
mismatches remain dependent on libass and FFmpeg behavior.

`scripts/setup-video.sh` installs macOS dependencies and a persistent model once.
`scripts/lyric-video.sh` composes the two binaries, saves timings, and optionally reuses them
for restyling. No alignment or transcription logic is added to the video library.

FFmpeg must include the libass-backed `ass` filter; some FFmpeg builds omit it.
Check filter availability explicitly and report a useful error. See
[FFmpeg's ASS filter](https://ffmpeg.org/ffmpeg-filters.html#ass) and
[Aegisub's karaoke and style-reset semantics](https://aegisub.org/docs/latest/ass_tags/#karaoke-effect).
