use lockstep::{forced, script};
use std::path::Path;

/// Preserves distinct onsets, held-note ends, and original spacing without borrowed word spans.
#[test]
fn supplied_words_keep_their_aligned_spans_and_provenance() {
    let lines = script::parse("[Verse]\nThe merchant's son\nhoney-sweet");
    let json = r#"[
        {"text":"The","start":1.1,"end":1.2},
        {"text":"merchant's","start":1.3,"end":2.0},
        {"text":"son","start":2.1,"end":3.8},
        {"text":"honey-","start":4.0,"end":4.5},
        {"text":"sweet","start":4.6,"end":7.9}
    ]"#;
    let document = forced::build(&lines, json, Path::new("lyrics.md"), 10.0).unwrap();
    assert_eq!(document.alignment.method, Some("forced"));
    assert_eq!(document.alignment.matched, 5);
    assert_eq!(document.lines[1].text, "honey-sweet");
    assert_eq!(document.lines[1].words[1].end, 7.9);
    assert_eq!(
        document.lines[0]
            .words
            .iter()
            .map(|word| word.start)
            .collect::<Vec<_>>(),
        [1.1, 1.3, 2.1]
    );
    assert!(document
        .lines
        .iter()
        .flat_map(|line| &line.words)
        .all(|word| word.source.is_none()));
}

/// Leaves standalone punctuation in line text without inventing an aligned word for it.
#[test]
fn standalone_punctuation_is_not_a_timed_word() {
    let lines = script::parse("one —");
    let document = forced::build(
        &lines,
        r#"[{"text":"one","start":1.0,"end":2.0}]"#,
        Path::new("lyrics.txt"),
        3.0,
    )
    .unwrap();
    assert_eq!(document.lines[0].text, "one —");
    assert_eq!(document.lines[0].words.len(), 1);
    assert_eq!(document.lines[0].words[0].text, "one");
    assert!(document.lines[0].words[0].source.is_none());
}

/// Ensures an invalid forced result fails instead of inventing replacement timings.
fn rejects(json: &str, message: &str) {
    let error = forced::build(
        &script::parse("one two"),
        json,
        Path::new("lyrics.txt"),
        5.0,
    )
    .err()
    .expect("invalid forced alignment must fail");
    assert!(error.to_string().contains(message), "{error:#}");
}

/// Rejects incomplete results instead of carrying the previous word's span.
#[test]
fn missing_words_fail() {
    rejects(r#"[{"text":"one","start":1,"end":2}]"#, "word count");
}

/// Rejects wrong or reordered text even when it has plausible timestamps.
#[test]
fn reordered_words_fail() {
    rejects(
        r#"[{"text":"two","start":1,"end":2},{"text":"one","start":2,"end":3}]"#,
        "word 1",
    );
}

/// Keeps overlapping words from becoming a simultaneous highlight group.
#[test]
fn overlapping_words_fail() {
    rejects(
        r#"[{"text":"one","start":1,"end":2},{"text":"two","start":1.5,"end":3}]"#,
        "overlap",
    );
}

/// Refuses zero-length alignments instead of presenting them as measured words.
#[test]
fn unresolved_words_fail() {
    rejects(
        r#"[{"text":"one","start":1,"end":1},{"text":"two","start":2,"end":3}]"#,
        "word 1",
    );
}

/// Refuses timings beyond the decoded recording instead of silently clamping them.
#[test]
fn times_outside_the_track_fail() {
    rejects(
        r#"[{"text":"one","start":1,"end":2},{"text":"two","start":3,"end":6}]"#,
        "word 2",
    );
}
