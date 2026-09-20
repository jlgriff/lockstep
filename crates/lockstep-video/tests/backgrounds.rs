use lockstep_video::{
    parse_background_image, plan, BackgroundCue, BackgroundImage, Document, Style, TimeRange,
};

/// Supplies a fractional duration to detect accidental truncation of the track boundary.
fn document() -> Document {
    Document {
        version: 1,
        duration: 10.37,
        lines: Vec::new(),
    }
}

/// Adds test images to otherwise ordinary rendering defaults.
fn style(background_images: Vec<BackgroundImage>) -> Style {
    Style {
        background_images,
        ..Style::default()
    }
}

/// Builds one explicit input image range.
fn image(path: &str, start: f64, end: f64) -> BackgroundImage {
    BackgroundImage {
        path: path.into(),
        range: Some(TimeRange { start, end }),
    }
}

/// Specifies a resolved cue independently of the input parsing and scheduling code.
fn cue(path: &str, start: f64, end: f64) -> BackgroundCue {
    BackgroundCue {
        path: path.into(),
        range: TimeRange { start, end },
    }
}

/// Preserves relative paths, spaces, Unicode, and equals signs in unranged CLI values.
#[test]
fn parses_an_unranged_background_image() {
    assert_eq!(
        parse_background_image("../images/夜 sky=cover.jpg").unwrap(),
        BackgroundImage {
            path: "../images/夜 sky=cover.jpg".into(),
            range: None
        }
    );
}

/// Converts hours, minutes, and milliseconds without splitting equals signs inside the filename.
#[test]
fn parses_a_webvtt_timestamp_range() {
    assert_eq!(
        parse_background_image("01:02:03.045..01:02:06.789=images/verse=夜.jpg").unwrap(),
        image("images/verse=夜.jpg", 3723.045, 3726.789)
    );
}

/// Expands a sole unranged image across the complete fractional track duration.
#[test]
fn one_unranged_image_covers_the_whole_video() {
    let images = vec![BackgroundImage {
        path: "cover.jpg".into(),
        range: None,
    }];
    let plan = plan(&document(), &style(images)).unwrap();
    assert_eq!(plan.background_images, [cue("cover.jpg", 0.0, 10.37)]);
}

/// Honors an explicit range on a single image instead of silently extending it to fill the track.
#[test]
fn one_ranged_image_keeps_its_explicit_span() {
    let plan = plan(&document(), &style(vec![image("verse.jpg", 2.125, 5.375)])).unwrap();
    assert_eq!(plan.background_images, [cue("verse.jpg", 2.125, 5.375)]);
}

/// Sorts image schedules by time and leaves the configured color beneath uncovered intervals.
#[test]
fn multiple_ranged_images_are_sorted_without_filling_gaps() {
    let style = Style {
        background_color: "#112233".to_string(),
        ..style(vec![
            image("outro.jpg", 5.125, 10.37),
            image("intro.jpg", 0.0, 3.375),
        ])
    };
    let plan = plan(&document(), &style).unwrap();
    assert_eq!(
        plan.background_images,
        [cue("intro.jpg", 0.0, 3.375), cue("outro.jpg", 5.125, 10.37),]
    );
    assert_eq!(
        plan.video_source,
        "color=c=#112233:s=1920x1080:r=30:d=10.37"
    );
}

/// Preserves exact endpoints at an adjacent image transition regardless of input order.
#[test]
fn adjacent_background_image_ranges_do_not_overlap() {
    let images = vec![
        image("second.jpg", 5.125, 10.37),
        image("first.jpg", 0.0, 5.125),
    ];
    let plan = plan(&document(), &style(images)).unwrap();
    assert_eq!(
        plan.background_images,
        [
            cue("first.jpg", 0.0, 5.125),
            cue("second.jpg", 5.125, 10.37),
        ]
    );
}

/// Checks planner diagnostics so an unrelated failure cannot masquerade as range validation.
fn rejects_images(images: Vec<BackgroundImage>, expected: &str) {
    let error = plan(&document(), &style(images)).unwrap_err();
    assert!(
        error.to_string().contains(expected),
        "expected {expected:?}, got {error:#}"
    );
}

/// Requires a range on the first image when multiple images are supplied.
#[test]
fn multiple_images_cannot_start_with_an_unranged_image() {
    rejects_images(
        vec![
            BackgroundImage {
                path: "first.jpg".into(),
                range: None,
            },
            image("last.jpg", 5.0, 10.37),
        ],
        "multiple background images require timestamp ranges",
    );
}

/// Requires a range on later images too rather than treating the first entry specially.
#[test]
fn multiple_images_cannot_end_with_an_unranged_image() {
    rejects_images(
        vec![
            image("first.jpg", 0.0, 5.0),
            BackgroundImage {
                path: "last.jpg".into(),
                range: None,
            },
        ],
        "multiple background images require timestamp ranges",
    );
}

/// Finds an overlap even when an unrelated image separates the conflicting entries in input order.
#[test]
fn background_image_ranges_cannot_partially_overlap() {
    rejects_images(
        vec![
            image("first.jpg", 0.0, 4.0),
            image("last.jpg", 8.0, 10.37),
            image("middle.jpg", 3.0, 6.0),
        ],
        "background image ranges overlap",
    );
}

/// Rejects containment as overlap even though every endpoint differs.
#[test]
fn background_image_ranges_cannot_contain_each_other() {
    rejects_images(
        vec![image("inner.jpg", 3.0, 4.0), image("outer.jpg", 0.0, 10.37)],
        "background image ranges overlap",
    );
}

macro_rules! invalid_range {
    ($name:ident, $start:expr, $end:expr, $reason:literal) => {
        #[doc = concat!("Rejects the ", stringify!($name), " range independently of other invalid inputs.")]
        #[test]
        fn $name() {
            rejects_images(vec![image("invalid.jpg", $start, $end)], $reason);
        }
    };
}

invalid_range!(
    negative_start,
    -0.001,
    3.0,
    "background image range falls outside track duration"
);
invalid_range!(
    end_past_track,
    8.0,
    10.371,
    "background image range falls outside track duration"
);
invalid_range!(
    zero_duration,
    5.0,
    5.0,
    "background image range must end after it starts"
);
invalid_range!(
    reversed_range,
    6.0,
    5.0,
    "background image range must end after it starts"
);
invalid_range!(
    nan_start,
    f64::NAN,
    5.0,
    "background image range must be finite"
);
invalid_range!(
    nan_end,
    0.0,
    f64::NAN,
    "background image range must be finite"
);
invalid_range!(
    infinite_end,
    0.0,
    f64::INFINITY,
    "background image range must be finite"
);

macro_rules! invalid_cli_image {
    ($name:ident, $value:literal, $reason:literal) => {
        #[doc = concat!("Reports ", stringify!($name), " as invalid input rather than an image filename.")]
        #[test]
        fn $name() {
            let error = parse_background_image($value).unwrap_err();
            assert!(error.to_string().contains($reason), "expected {:?}, got {error:#}", $reason);
        }
    };
}

invalid_cli_image!(rejects_empty_image_paths, "", "background image path");
invalid_cli_image!(
    rejects_missing_ranged_image_paths,
    "00:00:00.000..00:00:01.000=",
    "background image path"
);
invalid_cli_image!(
    rejects_minutes_above_59,
    "00:60:00.000..01:01:00.000=x.jpg",
    "timestamp"
);
invalid_cli_image!(
    rejects_seconds_above_59,
    "00:00:60.000..00:01:01.000=x.jpg",
    "timestamp"
);
invalid_cli_image!(
    rejects_incomplete_ranges,
    "00:00:00.000..=x.jpg",
    "timestamp"
);
invalid_cli_image!(
    rejects_frame_based_timecodes,
    "00:00:00:12..00:00:01:12=x.jpg",
    "timestamp"
);
