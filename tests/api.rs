//! Exercises lockstep the way another crate would, over the public API only.

use lockstep::export::{render, Format};
use lockstep::timing::build;
use lockstep::{script, whisper};
use std::path::Path;

/// A whisper transcript naming three words a second apart.
fn transcript() -> String {
    let tokens: Vec<String> = [(" one", 100), (" two", 200), (" three", 300)]
        .iter()
        .map(|(text, t)| format!(r#"{{"text":{text:?},"t_dtw":{t}}}"#))
        .collect();
    format!(
        r#"{{"transcription":[{{"tokens":[{}]}}]}}"#,
        tokens.join(",")
    )
}

#[test]
fn a_script_and_a_transcript_become_a_timed_document() {
    let lines = script::parse("one two\nthree");
    let heard = whisper::parse(&transcript()).unwrap();
    let (document, report) = build(&lines, &heard, Path::new("script.txt"), 30.0).unwrap();

    assert_eq!(document.script, "script.txt");
    assert_eq!(document.lines.len(), 2);
    assert_eq!(document.alignment.matched, 3);
    assert_eq!(report.confidence(), 1.0);
}

#[test]
fn a_document_renders_in_every_format() {
    let lines = script::parse("one two\nthree");
    let heard = whisper::parse(&transcript()).unwrap();
    let (document, _) = build(&lines, &heard, Path::new("script.txt"), 30.0).unwrap();

    assert!(render(&document, Format::Json)
        .unwrap()
        .contains(r#""version": 1"#));
    assert!(render(&document, Format::Vtt)
        .unwrap()
        .starts_with("WEBVTT"));
    assert!(render(&document, Format::Lrc)
        .unwrap()
        .starts_with("[00:00.70]"));
}

#[test]
fn a_caller_can_reach_the_pieces_separately() {
    let lines = script::parse("one two");
    assert_eq!(lines[0].keys().collect::<Vec<_>>(), ["one", "two"]);
    assert_eq!(whisper::parse(&transcript()).unwrap().len(), 3);
}

/// Retains separate measured spans for a hyphenated lyric instead of copying the preceding word.
#[test]
fn hyphenated_lyrics_keep_their_spoken_words_and_full_duration() {
    let lines = script::parse("grew honey-sweet;");
    let heard = whisper::parse(
        r#"{"transcription":[{"tokens":[
        {"text":" grew","t_dtw":7358},
        {"text":" honey","t_dtw":7458},
        {"text":" sweet","t_dtw":7504},
        {"text":"?","t_dtw":7570},
        {"text":" He","t_dtw":7574}
    ]}]}"#,
    )
    .unwrap();
    let (document, report) = build(&lines, &heard, Path::new("lyrics.txt"), 80.0).unwrap();
    assert_eq!(report.matched, 3);
    let line = &document.lines[0];
    assert_eq!(line.text, "grew honey-sweet;");
    assert_eq!(
        line.words
            .iter()
            .map(|word| (word.text.as_str(), word.start, word.end))
            .collect::<Vec<_>>(),
        [
            ("grew", 73.58, 74.58),
            ("honey-", 74.58, 75.04),
            ("sweet;", 75.04, 75.74),
        ]
    );
    assert!(line.words.iter().all(|word| word.source.is_none()));
    assert_eq!(line.end, 75.74);
}
