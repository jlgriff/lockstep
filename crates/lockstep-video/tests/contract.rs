use lockstep_video::{parse_document, plan, Document, Style, Word};

const TIMINGS: &str = r##"
{
  "version": 1,
  "generator": "lockstep 0.1.0",
  "script": "lyrics.txt",
  "duration": 10.0,
  "alignment": { "matched": 5, "words": 5, "rate": 1.0 },
  "lines": [
    {
      "start": 1.0,
      "end": 3.0,
      "text": "One two",
      "words": [
        { "start": 1.0, "end": 2.0, "text": "One" },
        { "start": 2.0, "end": 3.0, "text": "two" }
      ]
    },
    {
      "start": 5.0,
      "end": 7.0,
      "text": "Three four",
      "words": [
        { "start": 5.0, "end": 6.0, "text": "Three" },
        { "start": 6.0, "end": 7.0, "text": "four" }
      ]
    },
    {
      "start": 8.0,
      "end": 9.0,
      "text": "Five",
      "words": [
        { "start": 8.0, "end": 9.0, "text": "Five" }
      ]
    }
  ]
}
"##;

/// Parses the representative Lockstep document shared by rendering tests.
fn document() -> Document {
    parse_document(TIMINGS).unwrap()
}

/// Returns conspicuous custom values so defaults cannot satisfy customization tests.
fn custom_style() -> Style {
    Style {
        width: 1280,
        height: 720,
        frames_per_second: 24,
        background_color: "#112233".to_string(),
        text_color: "#DDEEFF".to_string(),
        highlight_color: "#FFCC00".to_string(),
        font: "Avenir Next".to_string(),
        font_size: 64,
        line_count: 2,
        rest_text: "♪ ♫".to_string(),
        background_images: Vec::new(),
    }
}

#[test]
/// Refuses future Lockstep shapes rather than silently misreading them.
fn rejects_an_unsupported_lockstep_format_version() {
    let future = TIMINGS.replacen(r#""version": 1"#, r#""version": 2"#, 1);
    let error = parse_document(&future).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("unsupported Lockstep format version 2"),
        "{error:#}"
    );
}

#[test]
/// Uses configured dimensions, frame rate, and background for the generated video source.
fn uses_the_configured_canvas() {
    let plan = plan(&document(), &custom_style()).unwrap();

    assert_eq!(plan.video_source, "color=c=#112233:s=1280x720:r=24:d=10");
}

#[test]
/// Maps font and text colors into the ASS lyric style.
fn uses_the_configured_typography() {
    let plan = plan(&document(), &custom_style()).unwrap();

    assert!(
        plan.subtitles
            .contains("Style: Lyrics,Avenir Next,64,&H0000CCFF,&H00FFEEDD,"),
        "{}",
        plan.subtitles
    );
}

#[test]
/// Gives each word an ASS karaoke span matching its Lockstep timing.
fn highlights_words_at_their_spoken_times() {
    let plan = plan(&document(), &custom_style()).unwrap();

    assert!(
        plan.subtitles.contains(r"{\k100}One {\k100}two"),
        "{}",
        plan.subtitles
    );
}

#[test]
/// Gives words sharing one Lockstep span one simultaneous highlight step.
fn highlights_words_with_the_same_span_as_one_unit() {
    let mut document = document();
    document.lines[0].text = "One ! two".to_string();
    document.lines[0].words = vec![
        Word {
            start: 1.0,
            end: 2.0,
            text: "One".to_string(),
        },
        Word {
            start: 1.0,
            end: 2.0,
            text: "!".to_string(),
        },
        Word {
            start: 2.0,
            end: 3.0,
            text: "two".to_string(),
        },
    ];
    let plan = plan(&document, &custom_style()).unwrap();

    assert!(
        plan.subtitles.contains(r"{\k100}One ! {\k100}two"),
        "{}",
        plan.subtitles
    );
}

/// Finds ASS dialogue active over one exact time span.
fn dialogue_during(subtitles: &str, start: &str, end: &str) -> String {
    let span = format!(",{start},{end},");
    subtitles
        .lines()
        .filter(|line| line.starts_with("Dialogue:") && line.contains(&span))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
/// Shows the current line followed by only enough upcoming lines to reach the configured limit.
fn limits_each_lyric_window_to_the_configured_line_count() {
    let plan = plan(&document(), &custom_style()).unwrap();
    let first_window = dialogue_during(&plan.subtitles, "0:00:01.00", "0:00:03.00");

    assert!(
        first_window.contains("One") && first_window.contains("Three four"),
        "{first_window}"
    );
    assert!(!first_window.contains("Five"), "{first_window}");
}

#[test]
/// Replaces every gap outside sung line spans with the configured musical notes.
fn shows_musical_notes_whenever_no_lyrics_are_sung() {
    let plan = plan(&document(), &custom_style()).unwrap();

    for expected in [
        "Dialogue: 0,0:00:00.00,0:00:01.00,Rest,,0,0,0,,♪ ♫",
        "Dialogue: 0,0:00:03.00,0:00:05.00,Rest,,0,0,0,,♪ ♫",
        "Dialogue: 0,0:00:07.00,0:00:08.00,Rest,,0,0,0,,♪ ♫",
        "Dialogue: 0,0:00:09.00,0:00:10.00,Rest,,0,0,0,,♪ ♫",
    ] {
        assert!(
            plan.subtitles.contains(expected),
            "missing {expected:?} in {}",
            plan.subtitles
        );
    }
}
