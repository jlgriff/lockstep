//! Minimal command wrapper for the lockstep-video library.

use anyhow::Result;
use clap::Parser;
use lockstep_video::{parse_background_image, render, HighlightStyle, RenderRequest, Style};
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about = "Renders Lockstep timing JSON as a lyric video")]
struct Args {
    #[arg(value_name = "TIMINGS")]
    timings: PathBuf,

    #[arg(value_name = "AUDIO")]
    audio: PathBuf,

    /// Output video [default: AUDIO lyric video.mp4 beside the recording]
    #[arg(short, long, value_name = "VIDEO")]
    output: Option<PathBuf>,

    #[arg(long, default_value_t = Style::default().width)]
    width: u32,

    #[arg(long, default_value_t = Style::default().height)]
    height: u32,

    #[arg(long, default_value_t = Style::default().frames_per_second)]
    frames_per_second: u32,

    #[arg(long, default_value_t = Style::default().background_color)]
    background_color: String,

    #[arg(long, default_value_t = Style::default().text_color)]
    text_color: String,

    #[arg(long, default_value_t = Style::default().highlight_color)]
    highlight_color: String,

    #[arg(long, default_value_t = Style::default().font)]
    font: String,

    /// Font size in video pixels
    #[arg(long, default_value_t = Style::default().font_size)]
    font_size: u32,

    #[arg(long, default_value_t = Style::default().line_count)]
    lines: usize,

    /// Enable per-word color and animation; false keeps every lyric in the text color
    #[arg(long, action = clap::ArgAction::Set, default_value_t = Style::default().highlight_words)]
    highlight_words: bool,

    /// Treatment used for the currently spoken word
    #[arg(long, value_enum, default_value = "color")]
    highlight_style: HighlightStyle,

    /// Duration of each active-word fade in and out; zero switches instantly
    #[arg(long, default_value_t = Style::default().highlight_transition_ms)]
    highlight_transition_ms: u32,

    #[arg(long, default_value_t = Style::default().rest_text)]
    rest_text: String,

    /// One image defaults to the whole track; repeated images each require a non-overlapping range
    #[arg(long, value_name = "[HH:MM:SS.mmm..HH:MM:SS.mmm=]IMAGE")]
    background_image: Vec<String>,
}

/// Passes command-line input directly into the rendering library.
fn main() -> Result<()> {
    let args = Args::parse();
    let output = args.output.unwrap_or_else(|| {
        let stem = args.audio.file_stem().unwrap_or_default().to_string_lossy();
        args.audio.with_file_name(format!("{stem} lyric video.mp4"))
    });
    let background_images = args
        .background_image
        .iter()
        .map(|value| parse_background_image(value))
        .collect::<Result<Vec<_>>>()?;
    render(&RenderRequest {
        timings: args.timings,
        audio: args.audio,
        output: output.clone(),
        style: Style {
            width: args.width,
            height: args.height,
            frames_per_second: args.frames_per_second,
            background_color: args.background_color,
            text_color: args.text_color,
            highlight_color: args.highlight_color,
            font: args.font,
            font_size: args.font_size,
            line_count: args.lines,
            highlight_words: args.highlight_words,
            highlight_style: args.highlight_style,
            highlight_transition_ms: args.highlight_transition_ms,
            rest_text: args.rest_text,
            background_images,
        },
    })?;
    println!("{}", output.display());
    Ok(())
}
