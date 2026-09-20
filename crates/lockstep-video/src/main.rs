//! Minimal command wrapper for the lockstep-video library.

use anyhow::Result;
use clap::Parser;
use lockstep_video::{parse_background_image, render, RenderRequest, Style};
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about = "Renders Lockstep timing JSON as a lyric video")]
struct Args {
    #[arg(value_name = "TIMINGS")]
    timings: PathBuf,

    #[arg(value_name = "AUDIO")]
    audio: PathBuf,

    #[arg(short, long, value_name = "VIDEO")]
    output: PathBuf,

    #[arg(long, default_value_t = 1920)]
    width: u32,

    #[arg(long, default_value_t = 1080)]
    height: u32,

    #[arg(long, default_value_t = 30)]
    frames_per_second: u32,

    #[arg(long, default_value = "#000000")]
    background_color: String,

    #[arg(long, default_value = "#FFFFFF")]
    text_color: String,

    #[arg(long, default_value = "#FFD700")]
    highlight_color: String,

    #[arg(long, default_value = "sans-serif")]
    font: String,

    #[arg(long, default_value_t = 72)]
    font_size: u32,

    #[arg(long, default_value_t = 2)]
    lines: usize,

    #[arg(long, default_value = "♪ ♪ ♪")]
    rest_text: String,

    /// Background image, optionally prefixed by a WebVTT range; repeat for multiple images
    #[arg(long, value_name = "[HH:MM:SS.mmm..HH:MM:SS.mmm=]IMAGE")]
    background_image: Vec<String>,
}

/// Passes command-line input directly into the rendering library.
fn main() -> Result<()> {
    let args = Args::parse();
    let background_images = args
        .background_image
        .iter()
        .map(|value| parse_background_image(value))
        .collect::<Result<Vec<_>>>()?;
    render(&RenderRequest {
        timings: args.timings,
        audio: args.audio,
        output: args.output,
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
            rest_text: args.rest_text,
            background_images,
        },
    })
}
