use lockstep_video::{
    parse_background_image, plan, BackgroundCue, BackgroundImage, Document, Style, TimeRange,
};
use std::path::PathBuf;

/// Supplies a ten-second track without lyrics because only background planning matters here.
fn document() -> Document {
    Document {
        version: 1,
        duration: 10.0,
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

/// Builds one image with an optional half-open time range.
fn image(path: &str, range: Option<(f64, f64)>) -> BackgroundImage {
    BackgroundImage {
        path: PathBuf::from(path),
        range: range.map(|(start, end)| TimeRange { start, end }),
    }
}

#[test]
/// Treats a bare CLI value as one image without an explicit range.
fn parses_an_unranged_background_image() {
    assert_eq!(
        parse_background_image("cover art.jpg").unwrap(),
        image("cover art.jpg", None)
    );
}

#[test]
/// Reads range endpoints in WebVTT's media-relative timestamp form.
fn parses_a_webvtt_timestamp_range() {
    assert_eq!(
        parse_background_image("00:00:02.500..00:00:05.000=verse.jpg").unwrap(),
        image("verse.jpg", Some((2.5, 5.0)))
    );
}

#[test]
/// Expands one unranged image across the complete track.
fn one_unranged_image_covers_the_whole_video() {
    let plan = plan(&document(), &style(vec![image("cover.jpg", None)])).unwrap();

    assert_eq!(
        plan.background_images,
        [BackgroundCue {
            path: PathBuf::from("cover.jpg"),
            start: 0.0,
            end: 10.0,
        }]
    );
}

#[test]
/// Preserves valid explicit ranges while allowing the color background to show through gaps.
fn multiple_ranged_images_keep_their_track_spans() {
    let backgrounds = vec![
        image("intro.jpg", Some((0.0, 3.0))),
        image("outro.jpg", Some((5.0, 10.0))),
    ];
    let plan = plan(&document(), &style(backgrounds)).unwrap();

    assert_eq!(
        plan.background_images,
        [
            BackgroundCue {
                path: PathBuf::from("intro.jpg"),
                start: 0.0,
                end: 3.0,
            },
            BackgroundCue {
                path: PathBuf::from("outro.jpg"),
                start: 5.0,
                end: 10.0,
            },
        ]
    );
}

#[test]
/// Allows one image to begin exactly when the previous image ends.
fn adjacent_background_image_ranges_do_not_overlap() {
    let backgrounds = vec![
        image("first.jpg", Some((0.0, 5.0))),
        image("second.jpg", Some((5.0, 10.0))),
    ];
    let plan = plan(&document(), &style(backgrounds)).unwrap();

    assert_eq!(plan.background_images.len(), 2);
}

#[test]
/// Requires every image to carry a range when more than one image is configured.
fn multiple_images_cannot_include_an_unranged_image() {
    let backgrounds = vec![
        image("intro.jpg", None),
        image("outro.jpg", Some((5.0, 10.0))),
    ];
    let error = plan(&document(), &style(backgrounds)).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("multiple background images require timestamp ranges"),
        "{error:#}"
    );
}

#[test]
/// Rejects image ranges whose interiors cover any shared track time.
fn background_image_ranges_cannot_overlap() {
    let backgrounds = vec![
        image("first.jpg", Some((0.0, 6.0))),
        image("second.jpg", Some((5.0, 10.0))),
    ];
    let error = plan(&document(), &style(backgrounds)).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("background image ranges overlap"),
        "{error:#}"
    );
}

#[test]
/// Requires every explicit image range to move forward in time.
fn background_image_ranges_need_positive_duration() {
    for range in [(5.0, 5.0), (6.0, 5.0)] {
        let error = plan(&document(), &style(vec![image("still.jpg", Some(range))])).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("background image range must end after it starts"),
            "{error:#}"
        );
    }
}

#[test]
/// Rejects image ranges extending before zero or beyond the track duration.
fn background_image_ranges_must_stay_inside_the_track() {
    for range in [(-1.0, 3.0), (8.0, 11.0)] {
        let error = plan(&document(), &style(vec![image("outside.jpg", Some(range))])).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("background image range falls outside track duration"),
            "{error:#}"
        );
    }
}
