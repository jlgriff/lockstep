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

    #[arg(long, default_value_t = Style::default().font_size)]
    font_size: u32,

    #[arg(long, default_value_t = Style::default().line_count)]
    lines: usize,

    #[arg(long, default_value_t = Style::default().rest_text)]
    rest_text: String,

    /// One image defaults to the whole track; repeated images each require a non-overlapping range
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
