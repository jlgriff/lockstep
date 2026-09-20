use lockstep_video::{render, RenderRequest, Style};
use std::path::PathBuf;
use std::process::Command;

const COMPRESSION_RESIDUE_PIXELS: usize = 5;

/// Holds a short encoded fixture and deletes its generated files on exit.
struct Fixture(PathBuf);

impl Fixture {
    /// Encodes two successive lines with a preview and a held first word.
    fn new(name: &str) -> Self {
        Self::with_lyrics(
            name,
            r#"{"version":1,"duration":3,"lines":[
            {"start":0.5,"end":1.4,"text":"First","words":[{"start":0.5,"end":1.4,"text":"First"}]},
            {"start":1.4,"end":2.8,"text":"Second","words":[{"start":1.4,"end":2.8,"text":"Second"}]}
        ]}"#,
            Style {
                highlight_color: "#FFD700".into(),
                highlight_transition_ms: 120,
                ..Style::default()
            },
        )
    }

    /// Renders supplied timings with fixed frame geometry for pixel comparisons.
    fn with_lyrics(name: &str, json: &str, style: Style) -> Self {
        let dir =
            std::env::temp_dir().join(format!("lockstep-video-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fixture = Self(dir);
        let timings = fixture.0.join("lyrics.json");
        let audio = fixture.0.join("audio.wav");
        std::fs::write(&timings, json).unwrap();
        let result = Command::new(ffmpeg())
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=48000:cl=mono",
                "-t",
                "3",
            ])
            .arg(&audio)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        render(&RenderRequest {
            timings,
            audio,
            output: fixture.0.join("video.mp4"),
            style: Style {
                width: 640,
                height: 360,
                font: "Arial".into(),
                font_size: 36,
                ..style
            },
        })
        .unwrap();
        fixture
    }

    /// Decodes an actual encoded frame so layout and color assertions include libass behavior.
    fn frame(&self, seconds: &str) -> Vec<u8> {
        let output = Command::new(ffmpeg())
            .args(["-v", "error", "-ss", seconds, "-i"])
            .arg(self.0.join("video.mp4"))
            .args([
                "-frames:v",
                "1",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "pipe:1",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout.len(), 640 * 360 * 3);
        output.stdout
    }
}

impl Drop for Fixture {
    /// Removes only this fixture's generated media.
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Selects the same FFmpeg executable as the rendering library.
fn ffmpeg() -> std::ffi::OsString {
    std::env::var_os("LOCKSTEP_VIDEO_FFMPEG").unwrap_or_else(|| "ffmpeg".into())
}

/// Locates the lower lyric row regardless of whether its text is white or gold.
fn lower_row_bounds(frame: &[u8]) -> (usize, usize) {
    let rows: Vec<_> = frame
        .chunks_exact(640 * 3)
        .enumerate()
        .filter(|(y, row)| *y >= 180 && row.chunks_exact(3).any(|p| p[0] > 180 && p[1] > 150))
        .map(|(y, _)| y)
        .collect();
    (
        *rows.first().expect("lower line must be visible"),
        *rows.last().unwrap(),
    )
}

/// Counts interior pixels between white and gold rather than counting antialiased edges.
fn transitioning_pixels(frame: &[u8]) -> usize {
    frame
        .chunks_exact(3)
        .filter(|p| p[0] > 230 && p[1] > 180 && p[2] > 40 && p[2] < 210)
        .count()
}

/// Keeps a preview at its original screen position after the preceding line disappears.
#[test]
#[ignore = "requires FFmpeg with libass and Arial"]
fn a_visible_line_never_moves_when_it_becomes_current() {
    let fixture = Fixture::new("fixed-rows");
    let before = lower_row_bounds(&fixture.frame("0.9"));
    let after = lower_row_bounds(&fixture.frame("1.9"));
    assert!(
        before.0.abs_diff(after.0) <= 1 && before.1.abs_diff(after.1) <= 1,
        "preview moved from {before:?} to {after:?}"
    );
}

/// Retains both members of a page and introduces the next pair only after its last sung word.
#[test]
#[ignore = "requires FFmpeg with libass and Arial"]
fn couplet_text_stays_unchanged_until_the_page_ends() {
    let fixture = Fixture::with_lyrics(
        "couplet-pages",
        r#"{"version":1,"duration":3,"lines":[
        {"start":0,"end":0.7,"text":"MMMM","words":[{"start":0,"end":0.7,"text":"MMMM"}]},
        {"start":0.7,"end":1.5,"text":"MMMM","words":[{"start":0.7,"end":1.5,"text":"MMMM"}]},
        {"start":1.5,"end":2.2,"text":"I","words":[{"start":1.5,"end":2.2,"text":"I"}]},
        {"start":2.2,"end":3,"text":"I","words":[{"start":2.2,"end":3,"text":"I"}]}
    ]}"#,
        Style {
            highlight_color: "#FFFFFF".into(),
            ..Style::default()
        },
    );
    for time in ["0.3", "1.0", "1.4"] {
        let frame = fixture.frame(time);
        for bottom in [false, true] {
            let width = row_width(&frame, bottom);
            assert!(
                width > 80,
                "page changed before its last word ended at {time}s: row width {width}"
            );
        }
    }
    for time in ["1.5", "2.5"] {
        let frame = fixture.frame(time);
        for bottom in [false, true] {
            let width = row_width(&frame, bottom);
            assert!(
                width < 20,
                "old page still visible at {time}s: row width {width}"
            );
        }
    }
}

/// Measures visible text width within a fixed row, ignoring background and compression edges.
fn row_width(frame: &[u8], bottom: bool) -> usize {
    let columns: Vec<_> = frame
        .chunks_exact(3)
        .enumerate()
        .filter(|(index, pixel)| {
            (index / 640 >= 180) == bottom && pixel.iter().all(|value| *value > 200)
        })
        .map(|(index, _)| index % 640)
        .collect();
    columns.iter().max().expect("lyric row must remain visible") - columns.iter().min().unwrap()
}

/// Starts a short color transition at word onset and finishes before the held note ends.
#[test]
#[ignore = "requires FFmpeg with libass and Arial"]
fn highlighting_transitions_at_onset_instead_of_snapping_or_waiting() {
    let fixture = Fixture::new("onset-fade");
    assert_eq!(transitioning_pixels(&fixture.frame("0.4")), 0);
    let during = transitioning_pixels(&fixture.frame("0.534"));
    let settled = transitioning_pixels(&fixture.frame("0.9"));
    assert!(
        during > settled + 100,
        "no onset transition: {during} intermediate pixels versus {settled} after settling"
    );
}

/// Counts colored glyph interiors in either half of a centered, equal-width word pair.
fn highlighted_pixels(frame: &[u8], right: bool) -> usize {
    frame
        .chunks_exact(3)
        .enumerate()
        .filter(|(index, pixel)| {
            (index % 640 >= 320) == right
                && *pixel.iter().max().unwrap() > 180
                && pixel.iter().max().unwrap() - pixel.iter().min().unwrap() > 60
        })
        .count()
}

/// Checks each word's activation, an internal pause, and the unhighlighted tail in encoded frames.
fn assert_only_active_words(name: &str, style: Style) {
    let fixture = Fixture::with_lyrics(
        name,
        r#"{"version":1,"duration":3,"lines":[
        {"start":0.5,"end":3,"text":"WORD WORD","words":[
            {"start":0.5,"end":1.1,"text":"WORD"},
            {"start":1.4,"end":2.8,"text":"WORD"}
        ]}
    ]}"#,
        style,
    );
    for (time, active) in [
        ("0.8", Some(false)),
        ("1.2", None),
        ("1.9", Some(true)),
        ("2.9", None),
    ] {
        let frame = fixture.frame(time);
        for right in [false, true] {
            let count = highlighted_pixels(&frame, right);
            if active == Some(right) {
                assert!(
                    count > 100,
                    "active word lacks emphasis at {time}s: {count} pixels"
                );
            } else {
                assert!(
                    count <= COMPRESSION_RESIDUE_PIXELS,
                    "inactive word still highlighted at {time}s, right={right}: {count} pixels"
                );
            }
        }
    }
}

/// Removes the previous word's emphasis while retaining its text and the next word's preview.
#[test]
#[ignore = "requires FFmpeg with libass and Arial"]
fn only_the_current_word_is_highlighted_with_fades() {
    assert_only_active_words("active-fade", Style::default());
}

/// Keeps instant mode limited to the current word too, including silent gaps inside a line.
#[test]
#[ignore = "requires FFmpeg with libass and Arial"]
fn only_the_current_word_is_highlighted_without_animation() {
    assert_only_active_words(
        "active-instant",
        Style {
            highlight_transition_ms: 0,
            ..Style::default()
        },
    );
}
