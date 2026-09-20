//! Plans and renders lyric videos from Lockstep timing documents.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

/// Lockstep JSON format version understood by this crate.
pub const SUPPORTED_FORMAT_VERSION: u32 = 1;

/// Timed lyrics consumed independently from Lockstep's implementation crate.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Document {
    pub version: u32,
    pub duration: f64,
    pub lines: Vec<Line>,
}

/// One displayed lyric line and its timed words.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Line {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub words: Vec<Word>,
}

/// One word whose span controls its karaoke highlight.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Word {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// Half-open media span measured in seconds from track start.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeRange {
    pub start: f64,
    pub end: f64,
}

/// Source image and its optional explicit time range.
#[derive(Clone, Debug, PartialEq)]
pub struct BackgroundImage {
    pub path: PathBuf,
    pub range: Option<TimeRange>,
}

/// Image placement after default ranges and ordering are resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct BackgroundCue {
    pub path: PathBuf,
    pub start: f64,
    pub end: f64,
}

/// Visual and encoding choices for a lyric video.
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub width: u32,
    pub height: u32,
    pub frames_per_second: u32,
    pub background_color: String,
    pub text_color: String,
    pub highlight_color: String,
    pub font: String,
    pub font_size: u32,
    pub line_count: usize,
    pub rest_text: String,
    pub background_images: Vec<BackgroundImage>,
}

impl Default for Style {
    /// Supplies conventional full-HD lyric-video styling.
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            frames_per_second: 30,
            background_color: "#000000".to_string(),
            text_color: "#FFFFFF".to_string(),
            highlight_color: "#FFD700".to_string(),
            font: "sans-serif".to_string(),
            font_size: 72,
            line_count: 2,
            rest_text: "♪ ♪ ♪".to_string(),
            background_images: Vec::new(),
        }
    }
}

/// Files and styling needed by the minimal command wrapper.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderRequest {
    pub timings: PathBuf,
    pub audio: PathBuf,
    pub output: PathBuf,
    pub style: Style,
}

/// Pure assets later passed to FFmpeg for encoding.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderPlan {
    pub video_source: String,
    pub background_images: Vec<BackgroundCue>,
    pub subtitles: String,
}

/// Reads Lockstep's versioned JSON shape without linking its implementation crate.
pub fn parse_document(json: &str) -> Result<Document> {
    serde_json::from_str(json).context("reading Lockstep timing JSON")
}

/// Parses a bare path or a WebVTT-timestamped image range from one CLI value.
pub fn parse_background_image(_value: &str) -> Result<BackgroundImage> {
    bail!("background image parsing is not implemented")
}

/// Builds the solid-color source and ASS karaoke script for a render.
pub fn plan(_document: &Document, _style: &Style) -> Result<RenderPlan> {
    Ok(RenderPlan {
        video_source: String::new(),
        background_images: Vec::new(),
        subtitles: String::new(),
    })
}

/// Encodes one lyric video from its timing document and audio recording.
pub fn render(_request: &RenderRequest) -> Result<()> {
    bail!("lyric video rendering is not implemented")
}
