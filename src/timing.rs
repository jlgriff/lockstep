//! Placing the script on the timeline: spans per line, times per word, rests in between.

use crate::align::align;
use crate::script;
use crate::whisper::Heard;
use anyhow::Result;
use serde::Serialize;

/// How far before its first word a line appears, so it is readable by the time it is reached.
const LEAD: f64 = 0.3;

/// How a word came by its time, when that time was not measured from the recording.
#[derive(Serialize, Debug, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// The line was heard but this word was not, so it took a neighbouring word's time.
    Carried,
    /// Nothing in the line was heard, so its words are spread between the nearest anchors.
    Spread,
}

/// One word of output, at the time it is reached. Its keys echo a line's: same name, same meaning.
#[derive(Serialize)]
pub struct Word {
    pub start: f64,
    pub end: f64,
    pub text: String,
    /// Absent when the time came straight from the recording, which is the ordinary case.
    #[serde(rename = "timing", skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
}

/// One line of output: its span, its text, and its words when it has them.
#[derive(Serialize)]
pub struct Line {
    pub start: f64,
    pub end: f64,
    pub text: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<Word>,
}

/// The version of this file format. A consumer should refuse a version it does not know.
pub const FORMAT_VERSION: u32 = 1;

/// Script coverage, with the optional method distinguishing forced alignment from recognition.
#[derive(Serialize)]
pub struct Alignment {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<&'static str>,
    pub matched: usize,
    pub words: usize,
    pub rate: f64,
}

/// A timed script, in the shape a player reads.
#[derive(Serialize)]
pub struct Document {
    pub version: u32,
    pub generator: String,
    pub script: String,
    pub duration: f64,
    pub alignment: Alignment,
    pub lines: Vec<Line>,
}

/// What the alignment managed, for the summary the tool prints when it finishes.
pub struct Report {
    pub lines: usize,
    pub anchored: usize,
    pub script_lines: usize,
    pub heard: usize,
    pub matched: usize,
    pub script_words: usize,
}

impl Report {
    /// The share of the script's words the recording was actually heard to say.
    pub fn confidence(&self) -> f64 {
        match self.script_words {
            0 => 0.0,
            total => self.matched as f64 / total as f64,
        }
    }
}

/// A word before its time is settled, since a word that was never heard has none yet.
struct Loose {
    text: String,
    start: Option<f64>,
    end: Option<f64>,
    measured: bool,
}

/// A line whose words are timed, carrying a span only if any of them were actually heard.
struct Timed {
    text: String,
    words: Vec<Loose>,
    span: Option<(f64, f64)>,
}

/// A line whose span is settled and whose every word has a time.
struct Placed {
    text: String,
    words: Vec<Word>,
    start: f64,
    end: f64,
}

/// Position `i` of `count` evenly spaced across a span.
fn slot(from: f64, to: f64, count: usize, i: usize) -> f64 {
    from + (to - from) * i as f64 / count as f64
}

/// Rounds to hundredths, the precision these timings actually carry.
fn round(seconds: f64) -> f64 {
    (seconds * 100.0).round() / 100.0
}

/// Carries a neighbour's span across words that never matched, so punctuation rides along.
fn carry<'a>(words: impl Iterator<Item = &'a mut Loose>) {
    let mut last = None;
    for word in words {
        match word.start.zip(word.end) {
            Some(span) => last = Some(span),
            None => {
                if let Some((start, end)) = last {
                    (word.start, word.end) = (Some(start), Some(end));
                }
            }
        }
    }
}

/// Preserves measured word spans while giving each line an earlier display start.
fn place(lines: &[script::Line], heard: &[Heard], matched: &[Option<usize>]) -> Vec<Timed> {
    let mut cursor = 0;
    lines
        .iter()
        .map(|line| {
            let mut words: Vec<Loose> = line
                .tokens
                .iter()
                .map(|token| {
                    let hit = (!token.key.is_empty())
                        .then(|| {
                            cursor += 1;
                            matched.get(cursor - 1).copied().flatten()
                        })
                        .flatten()
                        .and_then(|index| heard.get(index));
                    Loose {
                        text: token.raw.clone(),
                        start: hit.map(|word| word.start),
                        end: hit.map(|word| word.end),
                        measured: hit.is_some(),
                    }
                })
                .collect();

            let span = words
                .iter()
                .find_map(|word| word.start)
                .map(|start| (start - LEAD).max(0.0))
                .zip(words.iter().rev().find_map(|word| word.end));

            carry(words.iter_mut());
            carry(words.iter_mut().rev());

            Timed {
                text: line.text.clone(),
                words,
                span,
            }
        })
        .collect()
}

/// Settles every line's span, spreading lines that were never heard across their timed neighbours.
fn bridge(timed: Vec<Timed>, duration: f64) -> Vec<Placed> {
    let spans: Vec<Option<(f64, f64)>> = timed.iter().map(|line| line.span).collect();
    let total = spans.len();

    timed
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            let anchored = line.span.is_some();
            let (start, end) = line.span.unwrap_or_else(|| {
                let before = spans[..i].iter().rposition(Option::is_some);
                let after = spans[i + 1..]
                    .iter()
                    .position(Option::is_some)
                    .map(|offset| i + 1 + offset);
                let from = before.and_then(|b| spans[b]).map_or(0.0, |span| span.1);
                let to = after.and_then(|a| spans[a]).map_or(duration, |span| span.0);

                let first = before.map_or(0, |b| b + 1);
                let count = after.unwrap_or(total) - first + 1;
                (
                    slot(from, to, count, i - first),
                    slot(from, to, count, i - first + 1),
                )
            });

            let spread = line.words.len();
            let words = line
                .words
                .into_iter()
                .enumerate()
                .map(|(i, word)| Word {
                    start: word.start.unwrap_or_else(|| slot(start, end, spread, i)),
                    end: word.end.unwrap_or_else(|| slot(start, end, spread, i + 1)),
                    text: word.text,
                    source: (!word.measured).then_some(if anchored {
                        Source::Carried
                    } else {
                        Source::Spread
                    }),
                })
                .collect();
            Placed {
                text: line.text,
                words,
                start,
                end,
            }
        })
        .collect()
}

/// Rounds to hundredths and keeps starts strictly increasing.
fn finish(placed: Vec<Placed>) -> Vec<Line> {
    let mut previous = f64::NEG_INFINITY;
    placed
        .into_iter()
        .map(|line| {
            let start = round(round(line.start).max(previous + 0.01));
            previous = start;
            Line {
                start,
                end: round(line.end).max(start),
                text: line.text,
                words: line
                    .words
                    .into_iter()
                    .map(|word| {
                        let word_start = round(word.start.max(start));
                        Word {
                            start: word_start,
                            end: round(word.end).max(word_start),
                            ..word
                        }
                    })
                    .collect(),
            }
        })
        .collect()
}

/// Aligns a script against what was heard and lays the result out on the timeline.
pub fn build(
    lines: &[script::Line],
    heard: &[Heard],
    script: &std::path::Path,
    duration: f64,
) -> Result<(Document, Report)> {
    let script_keys: Vec<&str> = lines.iter().flat_map(script::Line::keys).collect();
    let heard_keys: Vec<&str> = heard.iter().map(|word| word.key.as_str()).collect();

    let matched = align(&script_keys, &heard_keys)?;
    let hits = matched.iter().filter(|hit| hit.is_some()).count();
    let timed = place(lines, heard, &matched);
    let anchored = timed.iter().filter(|line| line.span.is_some()).count();
    let out = finish(bridge(timed, duration));

    let report = Report {
        lines: out.len(),
        anchored,
        script_lines: lines.len(),
        heard: heard.len(),
        matched: hits,
        script_words: script_keys.len(),
    };
    let document = Document {
        version: FORMAT_VERSION,
        generator: concat!(env!("CARGO_PKG_NAME"), " ", env!("CARGO_PKG_VERSION")).to_string(),
        script: script
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        duration: round(duration),
        alignment: Alignment {
            method: None,
            matched: report.matched,
            words: report.script_words,
            rate: round(report.confidence()),
        },
        lines: out,
    };
    Ok((document, report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Builds the heard words for a fixture, each running a second or until the next begins.
    fn heard(words: &[(&str, f64)]) -> Vec<Heard> {
        let mut heard: Vec<Heard> = words
            .iter()
            .map(|(key, start)| Heard {
                key: key.to_string(),
                start: *start,
                end: start + 1.0,
            })
            .collect();
        for i in 0..heard.len().saturating_sub(1) {
            heard[i].end = heard[i].end.min(heard[i + 1].start);
        }
        heard
    }

    /// Times a fixture script against fixture words over a thirty second recording.
    fn build_fixture(text: &str, words: &[(&str, f64)]) -> (Document, Report) {
        build(
            &script::parse(text),
            &heard(words),
            Path::new("fixture.txt"),
            30.0,
        )
        .unwrap()
    }

    /// The (start, end) of every word in a line.
    fn spans(line: &Line) -> Vec<(f64, f64)> {
        line.words
            .iter()
            .map(|word| (word.start, word.end))
            .collect()
    }

    /// How each word of a line came by its time.
    fn sources(line: &Line) -> Vec<Option<Source>> {
        line.words.iter().map(|word| word.source).collect()
    }

    /// Previews a line early without shifting its spoken words or shortening its final word.
    #[test]
    fn display_lead_does_not_shift_word_timing() {
        let (document, _) = build_fixture("one two", &[("one", 10.0), ("two", 10.5)]);
        assert_eq!(spans(&document.lines[0]), [(10.0, 10.5), (10.5, 11.5)]);
        assert_eq!(
            (document.lines[0].start, document.lines[0].end),
            (9.7, 11.5)
        );
    }

    /// Copies measured spans for unmatched neighbors without moving their timestamps.
    #[test]
    fn a_carried_word_takes_the_whole_span_of_its_neighbour() {
        let (document, _) = build_fixture(
            "well listen here friend",
            &[("listen", 10.0), ("here", 10.5)],
        );
        assert_eq!(
            spans(&document.lines[0]),
            [(10.0, 10.5), (10.0, 10.5), (10.5, 11.5), (10.5, 11.5)]
        );
        assert_eq!(
            sources(&document.lines[0]),
            [Some(Source::Carried), None, None, Some(Source::Carried)]
        );
    }

    /// Places missing lines in the gap between measured endings and the next line's preview.
    #[test]
    fn spread_words_tile_their_line_exactly() {
        let (document, _) = build_fixture(
            "one two\nthree four\nfive six",
            &[("one", 10.0), ("two", 10.5), ("five", 19.5), ("six", 20.0)],
        );
        let bridged = &document.lines[1];
        assert_eq!(bridged.text, "three four");
        assert_eq!((bridged.start, bridged.end), (11.5, 15.35));
        assert_eq!(spans(bridged), [(11.5, 13.43), (13.43, 15.35)]);
        assert_eq!(sources(bridged), [Some(Source::Spread); 2]);
    }

    /// Preserves ordered word spans inside a line that can preview before the first word.
    #[test]
    fn a_lines_words_run_in_order_and_fill_it_end_to_end() {
        let (document, _) = build_fixture(
            "well one two\nthree four\nfive six",
            &[("one", 10.0), ("two", 10.5), ("five", 19.5), ("six", 20.0)],
        );
        for line in &document.lines {
            for pair in line.words.windows(2) {
                assert!(
                    pair[0].start <= pair[1].start,
                    "starts go backwards in {:?}",
                    line.text
                );
                assert!(
                    pair[0].end <= pair[1].end,
                    "ends go backwards in {:?}",
                    line.text
                );
            }
            assert!(
                line.words[0].start >= line.start,
                "words cannot precede their preview"
            );
            assert_eq!(
                line.words.last().unwrap().end,
                line.end,
                "line ends with its last word"
            );
        }
        assert_eq!(document.lines[0].words[0].start, 10.0);
    }

    #[test]
    fn a_carried_word_shares_its_neighbours_exact_time() {
        // Sharing a time is what "carried" means, so the two spans coincide rather than abut.
        let (document, _) = build_fixture("well one two", &[("one", 10.0), ("two", 10.5)]);
        let words = &document.lines[0].words;
        assert_eq!(
            (words[0].start, words[0].end),
            (words[1].start, words[1].end)
        );
        assert_eq!(words[0].source, Some(Source::Carried));
    }

    /// Leaves untimed intervals visible between anchored and estimated lyric lines.
    #[test]
    fn silence_is_left_as_a_gap_rather_than_written_as_a_rest() {
        let (document, _) = build_fixture(
            "one two\nthree four\nfive six",
            &[("one", 10.0), ("two", 10.5), ("five", 19.5), ("six", 20.0)],
        );
        assert_eq!(
            document.lines.len(),
            3,
            "one entry per script line, nothing inserted"
        );
        assert!(document.lines.iter().all(|line| !line.text.is_empty()));
        assert_eq!(round(document.lines[2].start - document.lines[1].end), 3.85);
    }

    #[test]
    fn the_document_says_what_made_it_and_from_what() {
        let (document, _) = build(
            &script::parse("one"),
            &heard(&[("one", 10.0)]),
            Path::new("/deep/path/script.txt"),
            30.0,
        )
        .unwrap();
        assert_eq!(document.version, FORMAT_VERSION);
        assert!(
            document.generator.starts_with("lockstep "),
            "{}",
            document.generator
        );
        assert!(document.generator.contains(env!("CARGO_PKG_VERSION")));
        assert_eq!(
            document.script, "script.txt",
            "only the name, never the caller's path"
        );
        assert_eq!(document.duration, 30.0);
    }

    #[test]
    fn alignment_keeps_the_counts_and_not_only_the_rate() {
        let (document, report) =
            build_fixture("one two three four", &[("one", 10.0), ("two", 10.5)]);
        assert_eq!(
            (document.alignment.matched, document.alignment.words),
            (2, 4)
        );
        assert_eq!(document.alignment.rate, 0.5);
        assert_eq!(report.confidence(), 0.5);
    }

    #[test]
    fn a_misheard_word_counts_against_the_rate() {
        let (document, _) = build_fixture("one two", &[("one", 10.0), ("too", 10.5)]);
        assert_eq!(
            (document.alignment.matched, document.alignment.words),
            (1, 2)
        );
    }

    #[test]
    fn an_empty_script_reports_no_alignment_rather_than_dividing_by_zero() {
        let (document, _) = build_fixture("", &[]);
        assert_eq!(document.alignment.rate, 0.0);
        assert!(document.lines.is_empty());
    }

    #[test]
    fn a_word_timed_from_the_recording_is_left_unmarked() {
        let (document, _) = build_fixture("one two", &[("one", 10.0), ("two", 10.5)]);
        assert_eq!(sources(&document.lines[0]), [None, None]);
    }

    /// Borrows the nearest measured onset for words with no matching transcript token.
    #[test]
    fn a_word_never_heard_takes_the_time_of_the_nearest_word_that_was() {
        let (document, _) = build_fixture(
            "well now listen here friend",
            &[("listen", 10.0), ("here", 10.5)],
        );
        let starts: Vec<f64> = document.lines[0]
            .words
            .iter()
            .map(|word| word.start)
            .collect();
        assert_eq!(starts, [10.0, 10.0, 10.0, 10.5, 10.5]);
    }

    #[test]
    fn a_script_heard_nowhere_is_spread_across_the_whole_recording() {
        let (document, report) = build_fixture("one\ntwo", &[]);
        assert_eq!(report.anchored, 0);
        assert_eq!(
            (document.lines[0].start, document.lines[0].end),
            (0.0, 10.0)
        );
        assert_eq!(
            (document.lines[1].start, document.lines[1].end),
            (10.0, 20.0)
        );
    }

    /// Orders previews at shared onsets without changing the measured word starts.
    #[test]
    fn two_lines_heard_at_the_same_moment_still_start_in_order() {
        let (document, _) = build_fixture("one\ntwo", &[("one", 10.0), ("two", 10.0)]);
        assert_eq!(document.lines[0].start, 9.7);
        assert_eq!(document.lines[1].start, 9.71);
        assert_eq!(
            document.lines[1].words[0].start, 10.0,
            "the measured onset is preserved"
        );
    }

    /// Serializes provenance alongside word timing independently of the line preview.
    #[test]
    fn the_json_names_every_key_in_full() {
        let (document, _) = build_fixture("well listen", &[("listen", 10.0)]);
        let json = serde_json::to_string(&document).unwrap();
        assert!(json.contains(r#""version":1"#), "{json}");
        assert!(
            json.contains(r#""alignment":{"matched":1,"words":2,"rate":0.5}"#),
            "{json}"
        );
        assert!(
            json.contains(r#"{"start":10.0,"end":11.0,"text":"well","timing":"carried"}"#),
            "{json}"
        );
        assert!(
            json.contains(r#"{"start":10.0,"end":11.0,"text":"listen"}"#),
            "{json}"
        );
    }
}
