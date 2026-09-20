use lockstep_video::{parse_document, plan, Document, Line, Style, Word};
use std::collections::BTreeMap;

/// Supplies unequal centisecond spans so fixed durations and extra timing offsets cannot pass.
fn document() -> Document {
    Document {
        version: 1,
        duration: 10.37,
        lines: vec![
            line(
                1.23,
                3.01,
                vec![word(1.23, 1.60, "One,"), word(1.60, 3.01, "two")],
            ),
            line(
                5.07,
                7.30,
                vec![word(5.07, 5.57, "Three"), word(5.57, 7.30, "four")],
            ),
            line(8.0, 9.12, vec![word(8.0, 9.12, "Five")]),
        ],
    }
}

/// Builds a printable lyric row from timed words without exercising JSON parsing.
fn line(start: f64, end: f64, words: Vec<Word>) -> Line {
    Line {
        start,
        end,
        text: words
            .iter()
            .map(|word| word.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        words,
    }
}

/// Supplies one timed token for the rendering fixtures.
fn word(start: f64, end: f64, text: &str) -> Word {
    Word {
        start,
        end,
        text: text.to_string(),
    }
}

/// Uses non-default styling and isolates the current line unless a test requests previews.
fn style() -> Style {
    Style {
        width: 1280,
        height: 720,
        frames_per_second: 24,
        background_color: "#112233".to_string(),
        text_color: "#DDEEFF".to_string(),
        highlight_color: "#FFCC00".to_string(),
        font: "Avenir Next".to_string(),
        font_size: 64,
        line_count: 1,
        highlight_transition_ms: 0,
        rest_text: "♪ ♫".to_string(),
        ..Style::default()
    }
}

/// Reads ASS records by their declared fields, retaining commas inside the final text field.
fn records<'a>(script: &'a str, section: &str, prefix: &str) -> Vec<BTreeMap<&'a str, &'a str>> {
    let lines: Vec<_> = script
        .lines()
        .skip_while(|line| line.trim() != section)
        .skip(1)
        .take_while(|line| !line.starts_with('['))
        .collect();
    let format = lines
        .iter()
        .find_map(|line| line.strip_prefix("Format:"))
        .unwrap_or_else(|| panic!("missing {section} Format declaration in {script:?}"));
    let fields: Vec<_> = format.split(',').map(str::trim).collect();
    lines
        .iter()
        .filter_map(|line| line.strip_prefix(prefix))
        .map(|row| {
            let values: Vec<_> = row.trim_start().splitn(fields.len(), ',').collect();
            assert_eq!(values.len(), fields.len(), "malformed ASS record: {row:?}");
            fields.iter().copied().zip(values).collect()
        })
        .collect()
}

/// Extracts every event so extra, missing, or mistimed dialogue cannot hide behind substring checks.
fn events(script: &str) -> Vec<(&str, &str, &str, &str)> {
    records(script, "[Events]", "Dialogue:")
        .iter()
        .map(|row| {
            let text = row["Text"];
            let text = if text.starts_with(r"{\an5\pos(") {
                text.split_once('}').unwrap().1
            } else {
                text
            };
            (row["Start"], row["End"], row["Style"], text)
        })
        .collect()
}

/// Selects complete lyric bodies while retaining their order and count.
fn lyrics(script: &str) -> Vec<&str> {
    events(script)
        .into_iter()
        .filter(|event| event.2 == "Lyrics")
        .map(|event| event.3)
        .collect()
}

/// Refuses future JSON versions before interpreting their timing schema.
#[test]
fn rejects_an_unsupported_lockstep_format_version() {
    let error = parse_document(r#"{"version":2,"cues":[]}"#).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unsupported Lockstep format version 2"),
        "{error:#}"
    );
}

/// Keeps a later word at its recorded onset when words within a line have a timing gap.
#[test]
fn gaps_between_words_do_not_pull_later_highlights_forward() {
    let document = Document {
        version: 1,
        duration: 2.0,
        lines: vec![line(
            0.0,
            2.0,
            vec![word(0.0, 0.30, "One"), word(1.23, 2.0, "Two")],
        )],
    };
    let plan = plan(&document, &style()).unwrap();
    assert_eq!(
        events(&plan.subtitles),
        [(
            "0:00:00.00",
            "0:00:02.00",
            "Lyrics",
            r"{\rPlain\1c&H0000CCFF&\t(299,300,\1c&H00FFEEDD&)}One {\rPlain\t(1229,1230,\1c&H0000CCFF&)\t(1999,2000,\1c&H00FFEEDD&)}Two"
        )]
    );
}

/// Applies version checks to direct Rust callers as well as JSON callers.
#[test]
fn planning_rejects_an_unsupported_document_version() {
    let document = Document {
        version: 2,
        ..document()
    };
    let error = plan(&document, &style()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unsupported Lockstep format version 2"),
        "{error:#}"
    );
}

/// Keeps canvas and subtitle resolution aligned and retains the fractional track duration.
#[test]
fn uses_the_configured_canvas() {
    let plan = plan(&document(), &style()).unwrap();
    assert_eq!(plan.video_source, "color=c=#112233:s=1280x720:r=24:d=10.37");
    for expected in ["ScriptType: v4.00+", "PlayResX: 1280", "PlayResY: 720"] {
        assert!(
            plan.subtitles.lines().any(|line| line == expected),
            "missing {expected:?}"
        );
    }
    assert!(
        plan.background_images.is_empty(),
        "no images were requested"
    );
}

/// Keeps previews and notes in the text color while sung words use the highlight color.
#[test]
fn uses_the_configured_typography_without_highlighting_plain_text() {
    let plan = plan(&document(), &style()).unwrap();
    let styles = records(&plan.subtitles, "[V4+ Styles]", "Style:");
    for (name, primary) in [("Lyrics", "&H0000CCFF"), ("Plain", "&H00FFEEDD")] {
        let row = styles
            .iter()
            .find(|row| row["Name"] == name)
            .unwrap_or_else(|| panic!("missing {name} style: {styles:?}"));
        assert_eq!(row["Fontname"], "Avenir Next");
        assert_eq!(row["Fontsize"], "64");
        assert_eq!(row["PrimaryColour"], primary);
        assert_eq!(row["SecondaryColour"], "&H00FFEEDD");
    }
}

/// Covers the entire track exactly once with correctly timed lyrics or musical notes.
#[test]
fn schedules_words_and_rests_without_extra_or_missing_events() {
    let plan = plan(&document(), &style()).unwrap();
    assert_eq!(
        events(&plan.subtitles),
        [
            ("0:00:00.00", "0:00:01.23", "Plain", "♪ ♫"),
            (
                "0:00:01.23",
                "0:00:03.01",
                "Lyrics",
                r"{\rPlain\1c&H0000CCFF&\t(369,370,\1c&H00FFEEDD&)}One, {\rPlain\t(369,370,\1c&H0000CCFF&)\t(1779,1780,\1c&H00FFEEDD&)}two"
            ),
            ("0:00:03.01", "0:00:05.07", "Plain", "♪ ♫"),
            (
                "0:00:05.07",
                "0:00:07.30",
                "Lyrics",
                r"{\rPlain\1c&H0000CCFF&\t(499,500,\1c&H00FFEEDD&)}Three {\rPlain\t(499,500,\1c&H0000CCFF&)\t(2229,2230,\1c&H00FFEEDD&)}four"
            ),
            ("0:00:07.30", "0:00:08.00", "Plain", "♪ ♫"),
            (
                "0:00:08.00",
                "0:00:09.12",
                "Lyrics",
                r"{\rPlain\1c&H0000CCFF&\t(1119,1120,\1c&H00FFEEDD&)}Five"
            ),
            ("0:00:09.12", "0:00:10.37", "Plain", "♪ ♫"),
        ]
    );
}

/// Advances to the next time group once even when multiple consecutive groups contain carried words.
#[test]
fn highlights_shared_spans_together_without_double_counting_time() {
    let mut document = document();
    document.lines[0] = line(
        1.23,
        3.01,
        vec![
            word(1.23, 1.60, "One,"),
            word(1.23, 1.60, "—"),
            word(1.60, 3.01, "two"),
            word(1.60, 3.01, "!"),
        ],
    );
    let plan = plan(&document, &style()).unwrap();
    assert_eq!(
        lyrics(&plan.subtitles),
        [
            r"{\rPlain\1c&H0000CCFF&\t(369,370,\1c&H00FFEEDD&)}One, {\rPlain\1c&H0000CCFF&\t(369,370,\1c&H00FFEEDD&)}— {\rPlain\t(369,370,\1c&H0000CCFF&)\t(1779,1780,\1c&H00FFEEDD&)}two {\rPlain\t(369,370,\1c&H0000CCFF&)\t(1779,1780,\1c&H00FFEEDD&)}!",
            r"{\rPlain\1c&H0000CCFF&\t(499,500,\1c&H00FFEEDD&)}Three {\rPlain\t(499,500,\1c&H0000CCFF&)\t(2229,2230,\1c&H00FFEEDD&)}four",
            r"{\rPlain\1c&H0000CCFF&\t(1119,1120,\1c&H00FFEEDD&)}Five",
        ]
    );
}

/// Keeps whole couplets visible until their final word ends, including a final incomplete page.
#[test]
fn lyric_pages_change_together_after_their_last_line_finishes() {
    let style = Style {
        line_count: 2,
        ..style()
    };
    let document = Document {
        version: 1,
        duration: 5.0,
        lines: vec![
            line(0.0, 1.0, vec![word(0.0, 1.0, "One")]),
            line(1.0, 2.0, vec![word(1.0, 2.0, "Two")]),
            line(2.0, 3.0, vec![word(2.0, 3.0, "Three")]),
            line(3.0, 4.0, vec![word(3.0, 4.0, "Four")]),
            line(4.0, 5.0, vec![word(4.0, 5.0, "Five")]),
        ],
    };
    let plan = plan(&document, &style).unwrap();
    let rows = records(&plan.subtitles, "[Events]", "Dialogue:");
    assert_eq!(
        rows.iter()
            .map(|row| (row["Start"], row["End"]))
            .collect::<Vec<_>>(),
        [
            ("0:00:00.00", "0:00:02.00"),
            ("0:00:00.00", "0:00:02.00"),
            ("0:00:02.00", "0:00:04.00"),
            ("0:00:02.00", "0:00:04.00"),
            ("0:00:04.00", "0:00:05.00"),
        ]
    );
    for (index, row) in rows.iter().enumerate() {
        let expected = if index % 2 == 0 {
            r"{\an5\pos(640,312)}"
        } else {
            r"{\an5\pos(640,408)}"
        };
        assert!(row["Text"].starts_with(expected), "{}", row["Text"]);
        assert!(!row["Text"].contains(r"\N"));
    }
}

/// Measures word effects from the preview event's start and finishes the fade promptly on held notes.
#[test]
fn previews_wait_until_word_onset_before_a_short_color_transition() {
    let style = Style {
        line_count: 2,
        highlight_transition_ms: 120,
        ..style()
    };
    let plan = plan(&document(), &style).unwrap();
    let text = lyrics(&plan.subtitles);
    assert!(
        text[0].contains(r"{\rPlain\t(0,120,0.5,\1c&H0000CCFF&)\t(250,370,2,\1c&H00FFEEDD&)}One,"),
        "{}",
        text[0]
    );
    assert!(
        text[0]
            .contains(r"{\rPlain\t(370,490,0.5,\1c&H0000CCFF&)\t(1660,1780,2,\1c&H00FFEEDD&)}two"),
        "{}",
        text[0]
    );
    assert!(
        text[1].contains(
            r"{\rPlain\t(3840,3960,0.5,\1c&H0000CCFF&)\t(4220,4340,2,\1c&H00FFEEDD&)}Three"
        ),
        "{}",
        text[1]
    );
}

/// Clears overlaps at the next onset and fits both fades inside short or collapsed word spans.
#[test]
fn active_highlights_end_at_the_next_onset_or_word_end() {
    let document = Document {
        version: 1,
        duration: 3.0,
        lines: vec![line(
            0.0,
            3.0,
            vec![
                word(0.0, 2.0, "Held"),
                word(1.0, 1.1, "short"),
                word(1.1, 1.1, "collapsed"),
            ],
        )],
    };
    let style = Style {
        highlight_transition_ms: 80,
        ..style()
    };
    let plan = plan(&document, &style).unwrap();
    assert_eq!(
        lyrics(&plan.subtitles),
        [concat!(
            r"{\rPlain\t(0,80,0.5,\1c&H0000CCFF&)\t(920,1000,2,\1c&H00FFEEDD&)}Held ",
            r"{\rPlain\t(1000,1050,0.5,\1c&H0000CCFF&)\t(1050,1100,2,\1c&H00FFEEDD&)}short ",
            r"{\rPlain}collapsed",
        )]
    );
}

/// Replaces lyrics directly at touching line boundaries without inserting a zero-length rest.
#[test]
fn adjacent_lines_have_no_rest_between_them() {
    let mut document = document();
    document.lines = vec![
        line(0.0, 0.37, vec![word(0.0, 0.37, "One")]),
        line(0.37, 10.37, vec![word(0.37, 10.37, "Two")]),
    ];
    let plan = plan(&document, &style()).unwrap();
    assert_eq!(
        events(&plan.subtitles),
        [
            (
                "0:00:00.00",
                "0:00:00.37",
                "Lyrics",
                r"{\rPlain\1c&H0000CCFF&\t(369,370,\1c&H00FFEEDD&)}One"
            ),
            (
                "0:00:00.37",
                "0:00:10.37",
                "Lyrics",
                r"{\rPlain\1c&H0000CCFF&\t(9999,10000,\1c&H00FFEEDD&)}Two"
            ),
        ]
    );
}

/// Shows default musical notes throughout an instrumental-only track.
#[test]
fn an_empty_lyric_track_is_one_full_length_rest() {
    let document = Document {
        lines: Vec::new(),
        ..document()
    };
    let plan = plan(&document, &Style::default()).unwrap();
    assert_eq!(
        events(&plan.subtitles),
        [("0:00:00.00", "0:00:10.37", "Plain", "♪ ♪ ♪")]
    );
}

/// Replaces an older line when a newer one starts, even when alignment spans overlap.
#[test]
fn overlapping_source_lines_do_not_stack_display_windows() {
    let mut document = document();
    document.lines = vec![
        line(0.0, 2.0, vec![word(0.0, 2.0, "First")]),
        line(1.5, 10.37, vec![word(1.5, 10.37, "Second")]),
    ];
    let plan = plan(&document, &style()).unwrap();
    let events = events(&plan.subtitles);
    assert_eq!(
        events
            .iter()
            .map(|event| (event.0, event.1, event.2))
            .collect::<Vec<_>>(),
        [
            ("0:00:00.00", "0:00:01.50", "Lyrics"),
            ("0:00:01.50", "0:00:10.37", "Lyrics"),
        ]
    );
    assert!(events[0].3.ends_with("First"), "{events:?}");
    assert!(events[1].3.ends_with("Second"), "{events:?}");
}

/// Lets the last line at a shared start replace earlier zero-window lines without duplicating the preceding rest.
#[test]
fn equal_source_line_starts_emit_one_rest_and_one_display_window() {
    let mut document = document();
    document.lines = vec![
        line(1.0, 2.0, vec![word(1.0, 2.0, "Replaced")]),
        line(1.0, 10.37, vec![word(1.0, 10.37, "Visible")]),
    ];
    let plan = plan(&document, &style()).unwrap();
    assert_eq!(
        events(&plan.subtitles),
        [
            ("0:00:00.00", "0:00:01.00", "Plain", "♪ ♫"),
            (
                "0:00:01.00",
                "0:00:10.37",
                "Lyrics",
                r"{\rPlain\1c&H0000CCFF&\t(9369,9370,\1c&H00FFEEDD&)}Visible"
            ),
        ]
    );
}

/// Ignores empty spans produced by alignment rather than emitting invalid ASS events.
#[test]
fn zero_length_source_lines_do_not_interrupt_rests() {
    let document = Document {
        lines: vec![line(1.0, 1.0, vec![word(1.0, 1.0, "Collapsed")])],
        ..document()
    };
    let plan = plan(&document, &style()).unwrap();
    assert_eq!(
        events(&plan.subtitles),
        [("0:00:00.00", "0:00:10.37", "Plain", "♪ ♫")]
    );
}

/// Accepts Lockstep metadata and timing provenance without requiring its implementation crate.
#[test]
fn consumes_lockstep_json_with_carried_and_spread_words() {
    let document = parse_document(
        r#"{
        "version": 1, "duration": 1.5, "generator": "lockstep", "script": "song.txt",
        "alignment": {"matched": 1, "words": 3, "rate": 0.33},
        "lines": [{"start": 0, "end": 1.5, "text": "Oui, — 夜",
            "words": [
                {"start": 0, "end": 0.5, "text": "Oui,"},
                {"start": 0, "end": 0.5, "text": "—", "timing": "carried"},
                {"start": 0.5, "end": 1.5, "text": "夜", "timing": "spread"}
            ]}]
    }"#,
    )
    .unwrap();
    let plan = plan(&document, &style()).unwrap();
    assert_eq!(
        events(&plan.subtitles),
        [(
            "0:00:00.00",
            "0:00:01.50",
            "Lyrics",
            r"{\rPlain\1c&H0000CCFF&\t(499,500,\1c&H00FFEEDD&)}Oui, {\rPlain\1c&H0000CCFF&\t(499,500,\1c&H00FFEEDD&)}— {\rPlain\t(499,500,\1c&H0000CCFF&)\t(1499,1500,\1c&H00FFEEDD&)}夜"
        ),]
    );
}

/// Displays line-only Lockstep entries without inventing per-word highlights or treating them as silence.
#[test]
fn missing_word_timings_leave_the_line_in_plain_text() {
    let document = parse_document(
        r#"{
        "version": 1, "duration": 1.5,
        "lines": [{"start": 0, "end": 1.5, "text": "Sing this line"}]
    }"#,
    )
    .unwrap();
    let plan = plan(&document, &style()).unwrap();
    assert_eq!(
        events(&plan.subtitles),
        [("0:00:00.00", "0:00:01.50", "Plain", "Sing this line")]
    );
}

/// Protects literal lyric braces from ASS interpretation in both current and upcoming lines.
#[test]
fn escapes_literal_lyric_braces_in_current_and_preview_text() {
    let mut document = document();
    document.lines[0] = line(1.23, 3.01, vec![word(1.23, 3.01, "{Oui},")]);
    document.lines[1] = line(5.07, 7.30, vec![word(5.07, 7.30, "{夜}")]);
    let style = Style {
        line_count: 2,
        ..style()
    };
    let plan = plan(&document, &style).unwrap();
    assert_eq!(
        lyrics(&plan.subtitles),
        [
            r"{\rPlain\1c&H0000CCFF&\t(1779,1780,\1c&H00FFEEDD&)}\{Oui\},",
            r"{\rPlain\t(3839,3840,\1c&H0000CCFF&)\t(6069,6070,\1c&H00FFEEDD&)}\{夜\}",
            r"{\rPlain\1c&H0000CCFF&\t(1119,1120,\1c&H00FFEEDD&)}Five",
        ]
    );
}

macro_rules! invalid_style {
    ($name:ident, $field:ident, $value:expr) => {
        #[doc = concat!("Rejects invalid ", stringify!($field), " before handing values to ASS or FFmpeg.")]
        #[test]
        fn $name() {
            let style = Style { $field: $value, ..style() };
            let error = plan(&document(), &style).unwrap_err();
            assert!(error.to_string().contains(stringify!($field)), "{error:#}");
        }
    };
}

invalid_style!(rejects_zero_line_count, line_count, 0);
invalid_style!(rejects_zero_frame_rate, frames_per_second, 0);
invalid_style!(rejects_zero_width, width, 0);
invalid_style!(rejects_zero_height, height, 0);
invalid_style!(rejects_zero_font_size, font_size, 0);
invalid_style!(
    rejects_invalid_background_color,
    background_color,
    "#12GG00".to_string()
);
invalid_style!(
    rejects_invalid_text_color,
    text_color,
    "#XYZXYZ".to_string()
);
invalid_style!(
    rejects_invalid_highlight_color,
    highlight_color,
    "red,blue".to_string()
);
invalid_style!(
    rejects_font_names_that_break_ass_fields,
    font,
    "Avenir,Next".to_string()
);

macro_rules! invalid_duration {
    ($name:ident, $duration:expr) => {
        #[doc = concat!("Rejects ", stringify!($name), " before generating track-length sources.")]
        #[test]
        fn $name() {
            let document = Document {
                duration: $duration,
                ..document()
            };
            let error = plan(&document, &style()).unwrap_err();
            assert!(error.to_string().contains("duration"), "{error:#}");
        }
    };
}

invalid_duration!(rejects_zero_track_duration, 0.0);
invalid_duration!(rejects_negative_track_duration, -1.0);
invalid_duration!(rejects_nan_track_duration, f64::NAN);
invalid_duration!(rejects_infinite_track_duration, f64::INFINITY);
