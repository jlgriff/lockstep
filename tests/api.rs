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
    format!(r#"{{"transcription":[{{"tokens":[{}]}}]}}"#, tokens.join(","))
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

    assert!(render(&document, Format::Json).unwrap().contains(r#""version": 1"#));
    assert!(render(&document, Format::Vtt).unwrap().starts_with("WEBVTT"));
    assert!(render(&document, Format::Lrc).unwrap().starts_with("[00:00.70]"));
}

#[test]
fn a_caller_can_reach_the_pieces_separately() {
    let lines = script::parse("one two");
    assert_eq!(lines[0].keys().collect::<Vec<_>>(), ["one", "two"]);
    assert_eq!(whisper::parse(&transcript()).unwrap().len(), 3);
}
