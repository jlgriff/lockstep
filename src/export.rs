//! Writing the timed script out: natively, or as one of the standard timed-text formats.

use crate::timing::{Document, Line};
use anyhow::Result;
use clap::ValueEnum;

/// The formats lockstep can write.
#[derive(Copy, Clone, PartialEq, Eq, Debug, ValueEnum)]
#[value(rename_all = "lowercase")]
pub enum Format {
    /// lockstep's own format, the only one carrying per-word provenance.
    Json,
    /// WebVTT, which a browser plays natively from a <track> element.
    Vtt,
    /// Enhanced LRC, which music players read as karaoke lyrics.
    Lrc,
}

impl Format {
    /// The extension a file of this format conventionally carries.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Json => "json",
            Format::Vtt => "vtt",
            Format::Lrc => "lrc",
        }
    }
}

/// Seconds as WebVTT's HH:MM:SS.mmm.
fn vtt_time(seconds: f64) -> String {
    let millis = (seconds * 1000.0).round().max(0.0) as u64;
    let (hours, minutes) = (millis / 3_600_000, millis / 60_000 % 60);
    format!("{hours:02}:{minutes:02}:{:02}.{:03}", millis / 1000 % 60, millis % 1000)
}

/// Seconds as LRC's MM:SS.xx, which has no hours field so minutes keep counting past sixty.
fn lrc_time(seconds: f64) -> String {
    let centis = (seconds * 100.0).round().max(0.0) as u64;
    format!("{:02}:{:02}.{:02}", centis / 6000, centis / 100 % 60, centis % 100)
}

/// Preserves lyric spacing and unique word timestamps, including onsets after a cue's preview.
fn karaoke(line: &Line, stamp: impl Fn(f64) -> String, tag_first: bool) -> String {
    let mut out = String::new();
    let mut remaining = line.text.as_str();
    let mut tagged: Option<f64> = None;
    for word in &line.words {
        if let Some(offset) = remaining.find(&word.text) {
            out.push_str(&remaining[..offset]);
            remaining = &remaining[offset + word.text.len()..];
        } else if !out.is_empty() {
            out.push(' ');
        }
        let fresh = match tagged {
            Some(previous) => word.start > previous,
            None => true,
        };
        if fresh {
            if tag_first || tagged.is_some() || word.start > line.start {
                out.push_str(&format!("<{}>", stamp(word.start)));
            }
            tagged = Some(word.start);
        }
        out.push_str(&word.text);
    }
    out
}

/// Renders a timed script in the requested format.
pub fn render(document: &Document, format: Format) -> Result<String> {
    Ok(match format {
        Format::Json => format!("{}\n", serde_json::to_string_pretty(document)?),
        Format::Vtt => {
            let cues: Vec<String> = document
                .lines
                .iter()
                .map(|line| {
                    format!(
                        "{} --> {}\n{}",
                        vtt_time(line.start),
                        vtt_time(line.end),
                        karaoke(line, vtt_time, false)
                    )
                })
                .collect();
            match cues.is_empty() {
                true => "WEBVTT\n".to_string(),
                false => format!("WEBVTT\n\n{}\n", cues.join("\n\n")),
            }
        }
        Format::Lrc => document
            .lines
            .iter()
            .map(|line| format!("[{}]{}\n", lrc_time(line.start), karaoke(line, lrc_time, true)))
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timing::{Alignment, Line, Word, FORMAT_VERSION};

    /// Two timed lines, enough to show a cue boundary and a per-word tag.
    fn document() -> Document {
        let word = |start: f64, end: f64, text: &str| Word {
            start,
            end,
            text: text.to_string(),
            source: None,
        };
        Document {
            version: FORMAT_VERSION,
            generator: "lockstep 0.0.0".to_string(),
            script: "fixture.txt".to_string(),
            duration: 30.0,
            alignment: Alignment { matched: 4, words: 4, rate: 1.0 },
            lines: vec![
                Line {
                    start: 9.7,
                    end: 11.2,
                    text: "one two".to_string(),
                    words: vec![word(9.7, 10.2, "one"), word(10.2, 11.2, "two")],
                },
                Line {
                    start: 19.2,
                    end: 20.7,
                    text: "five six".to_string(),
                    words: vec![word(19.2, 19.7, "five"), word(19.7, 20.7, "six")],
                },
            ],
        }
    }

    #[test]
    fn vtt_is_a_header_then_one_cue_per_line() {
        let vtt = render(&document(), Format::Vtt).unwrap();
        assert_eq!(
            vtt,
            "WEBVTT\n\
             \n\
             00:00:09.700 --> 00:00:11.200\n\
             one <00:00:10.200>two\n\
             \n\
             00:00:19.200 --> 00:00:20.700\n\
             five <00:00:19.700>six\n"
        );
    }

    #[test]
    fn vtt_leaves_the_first_word_untagged_because_the_cue_already_starts_there() {
        let vtt = render(&document(), Format::Vtt).unwrap();
        assert!(vtt.contains("\none <00:00:10.200>two\n"), "{vtt}");
        assert!(!vtt.contains("<00:00:09.700>one"), "{vtt}");
    }

    /// Tags the first word when its cue previews before the measured onset.
    #[test]
    fn vtt_preserves_a_first_word_onset_after_the_preview_starts() {
        let mut document = document();
        document.lines[0].start = 9.4;
        let vtt = render(&document, Format::Vtt).unwrap();
        assert!(vtt.contains("00:00:09.400 --> 00:00:11.200\n<00:00:09.700>one"), "{vtt}");
    }

    /// Preserves a compound's hyphen without inserting spaces around inline word timestamps.
    #[test]
    fn timed_exports_keep_hyphenated_word_spacing() {
        let mut document = document();
        document.lines[0].text = "one-two".into();
        document.lines[0].words[0].text = "one-".into();
        assert!(render(&document, Format::Vtt).unwrap().contains("one-<00:00:10.200>two"));
        assert!(render(&document, Format::Lrc).unwrap().contains("one-<00:10.20>two"));
    }

    #[test]
    fn lrc_stamps_the_line_and_then_every_word() {
        let lrc = render(&document(), Format::Lrc).unwrap();
        assert_eq!(
            lrc,
            "[00:09.70]<00:09.70>one <00:10.20>two\n\
             [00:19.20]<00:19.20>five <00:19.70>six\n"
        );
    }

    /// One line with two separate groups of words sharing a time, as carried words do. Two
    /// groups rather than one, so a tag cursor that never advances past the first is caught.
    fn shared_time_document() -> Document {
        let mut document = document();
        document.lines.truncate(1);
        document.lines[0].text = "well one two three".to_string();
        document.lines[0].words = vec![
            Word { start: 9.7, end: 10.2, text: "well".to_string(), source: None },
            Word { start: 9.7, end: 10.2, text: "one".to_string(), source: None },
            Word { start: 10.2, end: 11.2, text: "two".to_string(), source: None },
            Word { start: 10.2, end: 11.2, text: "three".to_string(), source: None },
        ];
        document
    }

    #[test]
    fn vtt_gives_words_sharing_a_time_one_tag_between_them() {
        // WebVTT requires a cue's inline timestamps to strictly increase, so two words timed
        // together have to share a tag rather than repeat one.
        let vtt = render(&shared_time_document(), Format::Vtt).unwrap();
        assert!(vtt.contains("\nwell one <00:00:10.200>two three\n"), "{vtt}");
        let tags: Vec<&str> = vtt.matches("<00:00:09.700>").collect();
        assert!(tags.is_empty(), "the first word needs no tag: {vtt}");
    }

    #[test]
    fn lrc_gives_words_sharing_a_time_one_tag_between_them() {
        let lrc = render(&shared_time_document(), Format::Lrc).unwrap();
        assert_eq!(lrc, "[00:09.70]<00:09.70>well one <00:10.20>two three\n");
    }

    #[test]
    fn every_inline_timestamp_strictly_increases() {
        for format in [Format::Vtt, Format::Lrc] {
            let out = render(&shared_time_document(), format).unwrap();
            let stamps: Vec<&str> = out
                .split('<')
                .skip(1)
                .filter_map(|rest| rest.split('>').next())
                .collect();
            assert!(
                stamps.windows(2).all(|pair| pair[0] < pair[1]),
                "{format:?} repeats a timestamp: {stamps:?}"
            );
        }
    }

    #[test]
    fn json_is_the_native_document() {
        let json = render(&document(), Format::Json).unwrap();
        assert!(json.contains(r#""version": 1"#), "{json}");
        assert!(json.ends_with('\n'));
    }

    #[test]
    fn vtt_timestamps_carry_hours_and_milliseconds() {
        assert_eq!(vtt_time(0.0), "00:00:00.000");
        assert_eq!(vtt_time(9.7), "00:00:09.700");
        assert_eq!(vtt_time(3725.5), "01:02:05.500");
    }

    #[test]
    fn lrc_timestamps_are_minutes_seconds_and_hundredths() {
        assert_eq!(lrc_time(0.0), "00:00.00");
        assert_eq!(lrc_time(9.7), "00:09.70");
        // LRC has no hours field, so a long recording keeps counting minutes.
        assert_eq!(lrc_time(3725.5), "62:05.50");
    }

    #[test]
    fn an_extension_is_offered_for_each_format() {
        assert_eq!(Format::Json.extension(), "json");
        assert_eq!(Format::Vtt.extension(), "vtt");
        assert_eq!(Format::Lrc.extension(), "lrc");
    }
}
