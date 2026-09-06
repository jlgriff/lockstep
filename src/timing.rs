//! Placing the script on the timeline: spans per line, times per word, rests in between.

use crate::align::align;
use crate::script;
use crate::whisper::Heard;
use anyhow::Result;
use serde::Serialize;

/// How far before its first word a line appears, so it is readable by the time it is reached.
const LEAD: f64 = 0.3;

/// Silence longer than this between two lines becomes an explicit rest.
const REST_GAP: f64 = 3.0;

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

/// A timed script, in the shape a player reads.
#[derive(Serialize)]
pub struct Document {
    pub source: String,
    pub timing: &'static str,
    pub matched: f64,
    pub duration: f64,
    pub lines: Vec<Line>,
}

/// What the alignment managed, for the summary the tool prints when it finishes.
pub struct Report {
    pub lines: usize,
    pub anchored: usize,
    pub script_lines: usize,
    pub rests: usize,
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

/// Carries a neighbour's time across words that never matched, so punctuation rides along.
fn carry<'a>(words: impl Iterator<Item = &'a mut Loose>) {
    let mut last = None;
    for word in words {
        match word.start {
            Some(start) => last = Some(start),
            None => word.start = last,
        }
    }
}

/// Times each line's words from the alignment, showing every one of them slightly early.
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
                        start: hit.map(|word| (word.start - LEAD).max(0.0)),
                        end: hit.map(|word| word.end),
                        measured: hit.is_some(),
                    }
                })
                .collect();

            let span = words
                .iter()
                .find_map(|word| word.start)
                .zip(words.iter().rev().find_map(|word| word.end));

            carry(words.iter_mut());
            carry(words.iter_mut().rev());

            Timed { text: line.text.clone(), words, span }
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
                (slot(from, to, count, i - first), slot(from, to, count, i - first + 1))
            });

            let spread = line.words.len();
            let words = line
                .words
                .into_iter()
                .enumerate()
                .map(|(i, word)| Word {
                    start: word.start.unwrap_or_else(|| slot(start, end, spread, i)),
                    text: word.text,
                    source: (!word.measured)
                        .then_some(if anchored { Source::Carried } else { Source::Spread }),
                })
                .collect();
            Placed { text: line.text, words, start, end }
        })
        .collect()
}

/// Inserts a silent rest wherever the recording goes quiet long enough to strand a line.
fn rests(placed: Vec<Placed>, duration: f64) -> Vec<Placed> {
    let cues: Vec<f64> =
        placed.iter().skip(1).map(|line| line.start).chain([duration]).collect();

    let mut out = Vec::with_capacity(placed.len());
    for (line, next) in placed.into_iter().zip(cues) {
        let quiet = next - line.end > REST_GAP;
        let start = line.end;
        out.push(line);
        if quiet {
            out.push(Placed { text: String::new(), words: Vec::new(), start, end: next });
        }
    }
    out
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
                    .map(|word| Word { start: round(word.start.max(start)), ..word })
                    .collect(),
            }
        })
        .collect()
}

/// Aligns a script against what was heard and lays the result out on the timeline.
pub fn build(
    lines: &[script::Line],
    heard: &[Heard],
    source: &str,
    duration: f64,
) -> Result<(Document, Report)> {
    let script_keys: Vec<&str> = lines.iter().flat_map(script::Line::keys).collect();
    let heard_keys: Vec<&str> = heard.iter().map(|word| word.key.as_str()).collect();

    let matched = align(&script_keys, &heard_keys)?;
    let hits = matched.iter().filter(|hit| hit.is_some()).count();
    let timed = place(lines, heard, &matched);
    let anchored = timed.iter().filter(|line| line.span.is_some()).count();
    let out = finish(rests(bridge(timed, duration), duration));

    let report = Report {
        lines: out.len(),
        anchored,
        script_lines: lines.len(),
        rests: out.iter().filter(|line| line.text.is_empty()).count(),
        heard: heard.len(),
        matched: hits,
        script_words: script_keys.len(),
    };
    let document = Document {
        source: source.to_string(),
        timing: "aligned",
        matched: round(report.confidence()),
        duration: round(duration),
        lines: out,
    };
    Ok((document, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds the heard words for a fixture, each ending where the next one starts.
    fn heard(words: &[(&str, f64)]) -> Vec<Heard> {
        let mut heard: Vec<Heard> = words
            .iter()
            .map(|(key, start)| Heard { key: key.to_string(), start: *start, end: start + 1.0 })
            .collect();
        for i in 0..heard.len().saturating_sub(1) {
            heard[i].end = heard[i].end.min(heard[i + 1].start);
        }
        heard
    }

    /// Times a fixture script against fixture words over a thirty second recording.
    fn build_fixture(text: &str, words: &[(&str, f64)]) -> (Document, Report) {
        let lines = script::parse(text);
        build(&lines, &heard(words), "fixture.txt", 30.0).unwrap()
    }

    #[test]
    fn a_heard_line_starts_just_before_its_first_word() {
        let (document, _) = build_fixture("one two", &[("one", 10.0), ("two", 10.5)]);
        assert_eq!(document.lines[0].start, 9.7);
        assert_eq!(document.lines[0].end, 11.5);
        assert_eq!(document.lines[0].words[0].start, 9.7);
        assert_eq!(document.lines[0].words[1].start, 10.2);
    }

    #[test]
    fn a_line_that_was_never_heard_is_spread_between_its_timed_neighbours() {
        let (document, report) = build_fixture(
            "one two\nthree four\nfive six",
            &[("one", 10.0), ("two", 10.5), ("five", 20.0), ("six", 20.5)],
        );
        assert_eq!(report.anchored, 2);
        assert_eq!(report.script_lines, 3);

        let bridged = &document.lines[1];
        assert_eq!(bridged.text, "three four");
        assert_eq!((bridged.start, bridged.end), (11.5, 15.6));
        assert_eq!(bridged.words.iter().map(|word| word.start).collect::<Vec<_>>(), [11.5, 13.55]);
    }

    #[test]
    fn a_long_silence_becomes_a_rest() {
        let (document, report) = build_fixture(
            "one two\nthree four\nfive six",
            &[("one", 10.0), ("two", 10.5), ("five", 20.0), ("six", 20.5)],
        );
        assert_eq!(report.rests, 2);
        assert_eq!(document.lines.len(), 5);

        let rest = &document.lines[2];
        assert_eq!(rest.text, "");
        assert_eq!((rest.start, rest.end), (15.6, 19.7));
        assert!(rest.words.is_empty());
    }

    #[test]
    fn a_short_silence_does_not() {
        let (document, report) =
            build_fixture("one\ntwo", &[("one", 10.0), ("two", 12.0)]);
        assert_eq!(report.rests, 1, "only the run-out after the last line");
        assert_eq!(document.lines.len(), 3);
        assert_eq!(document.lines[1].text, "two");
    }

    #[test]
    fn two_lines_heard_at_the_same_moment_still_start_in_order() {
        let (document, _) = build_fixture("one\ntwo", &[("one", 10.0), ("two", 10.0)]);
        assert_eq!(document.lines[0].start, 9.7);
        assert_eq!(document.lines[1].start, 9.71);
        assert_eq!(document.lines[1].words[0].start, 9.71, "a word never precedes its own line");
    }

    #[test]
    fn a_word_never_heard_takes_the_time_of_the_nearest_word_that_was() {
        // The two words opening the line were missed and so was the one closing it, so the
        // openers have to look forward for a time and the closer has to look back.
        let (document, _) =
            build_fixture("well now listen here friend", &[("listen", 10.0), ("here", 10.5)]);
        let times: Vec<f64> = document.lines[0].words.iter().map(|word| word.start).collect();
        assert_eq!(times, [9.7, 9.7, 9.7, 10.2, 10.2]);
    }

    #[test]
    fn a_script_heard_nowhere_is_spread_across_the_whole_recording() {
        let (document, report) = build_fixture("one\ntwo", &[]);
        assert_eq!(report.anchored, 0);
        assert_eq!((document.lines[0].start, document.lines[0].end), (0.0, 10.0));
        assert_eq!((document.lines[1].start, document.lines[1].end), (10.0, 20.0));
    }

    #[test]
    fn a_rest_is_written_without_a_words_key() {
        let (document, _) = build_fixture("one", &[("one", 10.0)]);
        let json = serde_json::to_string(&document).unwrap();
        assert!(json.contains(r#"{"start":9.7,"end":11.0,"text":"one","words":[{"start":9.7,"text":"one"}]}"#));
        assert!(json.contains(r#"{"start":11.0,"end":30.0,"text":""}"#));
    }

    /// How each word of a line came by its time.
    fn sources(line: &Line) -> Vec<Option<Source>> {
        line.words.iter().map(|word| word.source).collect()
    }

    #[test]
    fn a_word_timed_from_the_recording_is_left_unmarked() {
        let (document, _) = build_fixture("one two", &[("one", 10.0), ("two", 10.5)]);
        assert_eq!(sources(&document.lines[0]), [None, None]);
    }

    #[test]
    fn a_word_that_took_a_neighbours_time_is_marked_carried() {
        let (document, _) =
            build_fixture("well now listen here friend", &[("listen", 10.0), ("here", 10.5)]);
        use Source::Carried;
        assert_eq!(
            sources(&document.lines[0]),
            [Some(Carried), Some(Carried), None, None, Some(Carried)]
        );
    }

    #[test]
    fn a_line_nobody_was_heard_saying_has_every_word_marked_spread() {
        let (document, _) = build_fixture(
            "one two\nthree four\nfive six",
            &[("one", 10.0), ("two", 10.5), ("five", 20.0), ("six", 20.5)],
        );
        assert_eq!(document.lines[1].text, "three four");
        assert_eq!(sources(&document.lines[1]), [Some(Source::Spread); 2]);
        assert_eq!(sources(&document.lines[0]), [None, None]);
    }

    #[test]
    fn only_the_unmeasured_words_reach_the_json() {
        let (document, _) =
            build_fixture("well listen here", &[("listen", 10.0), ("here", 10.5)]);
        let json = serde_json::to_string(&document).unwrap();
        assert!(json.contains(r#"{"start":9.7,"text":"well","timing":"carried"}"#), "{json}");
        assert!(json.contains(r#"{"start":9.7,"text":"listen"},{"start":10.2,"text":"here"}"#), "{json}");
    }

    #[test]
    fn confidence_is_the_share_of_the_script_that_was_heard() {
        let (_, all) = build_fixture("one two", &[("one", 10.0), ("two", 10.5)]);
        assert_eq!((all.matched, all.script_words), (2, 2));
        assert_eq!(all.confidence(), 1.0);

        let (_, half) = build_fixture("one two three four", &[("one", 10.0), ("two", 10.5)]);
        assert_eq!((half.matched, half.script_words), (2, 4));
        assert_eq!(half.confidence(), 0.5);

        let (_, none) = build_fixture("one two", &[]);
        assert_eq!(none.confidence(), 0.0);
    }

    #[test]
    fn a_misheard_word_does_not_count_towards_confidence() {
        let (_, report) = build_fixture("one two", &[("one", 10.0), ("too", 10.5)]);
        assert_eq!(report.matched, 1);
        assert_eq!(report.confidence(), 0.5);
    }

    #[test]
    fn the_document_carries_the_confidence_a_consumer_can_gate_on() {
        let (document, _) = build_fixture("one two three four", &[("one", 10.0), ("two", 10.5)]);
        assert_eq!(document.matched, 0.5);
        assert!(serde_json::to_string(&document).unwrap().contains(r#""matched":0.5"#));
    }

    #[test]
    fn an_empty_script_reports_no_confidence_rather_than_dividing_by_zero() {
        let (document, report) = build_fixture("", &[]);
        assert_eq!(report.confidence(), 0.0);
        assert!(document.matched.is_finite());
        assert!(document.lines.is_empty());
    }

    #[test]
    fn the_run_ends_where_the_recording_does() {
        let (document, _) = build_fixture("one", &[("one", 10.0)]);
        assert_eq!(document.duration, 30.0);
        assert_eq!(document.lines.last().unwrap().end, 30.0);
    }
}
