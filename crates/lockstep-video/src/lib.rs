//! Plans and renders lyric videos from Lockstep timing documents.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

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
    #[serde(default)]
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

/// Image placement with a required range after defaults and ordering are resolved.
#[derive(Clone, Debug, PartialEq)]
pub struct BackgroundCue {
    pub path: PathBuf,
    pub range: TimeRange,
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
    pub highlight_transition_ms: u32,
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
            background_color: "#0B1730".to_string(),
            text_color: "#FFFFFF".to_string(),
            highlight_color: "#67E8F9".to_string(),
            font: "sans-serif".to_string(),
            font_size: 72,
            line_count: 2,
            highlight_transition_ms: 80,
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
    let value: serde_json::Value =
        serde_json::from_str(json).context("reading Lockstep timing JSON")?;
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .context("Lockstep timing JSON has no numeric format version")?;
    if version != u64::from(SUPPORTED_FORMAT_VERSION) {
        bail!("unsupported Lockstep format version {version}");
    }
    serde_json::from_value(value).context("reading Lockstep timing JSON")
}

/// Parses a bare path or a WebVTT-timestamped image range from one CLI value.
pub fn parse_background_image(value: &str) -> Result<BackgroundImage> {
    if value.is_empty() {
        bail!("background image path cannot be empty");
    }

    let starts_like_timestamp = value.split("..").next().is_some_and(|prefix| {
        prefix.contains(':') && prefix.as_bytes().first().is_some_and(u8::is_ascii_digit)
    });
    let Some((prefix, path)) = value.split_once('=') else {
        if value.contains("..") && starts_like_timestamp {
            bail!("background image timestamp range must be followed by =IMAGE");
        }
        return Ok(BackgroundImage {
            path: value.into(),
            range: None,
        });
    };
    let looks_ranged = prefix.contains("..") && starts_like_timestamp;
    if !looks_ranged {
        return Ok(BackgroundImage {
            path: value.into(),
            range: None,
        });
    }
    if path.is_empty() {
        bail!("background image path cannot be empty");
    }
    let (start, end) = prefix
        .split_once("..")
        .context("background image timestamp range must contain two timestamps")?;
    Ok(BackgroundImage {
        path: path.into(),
        range: Some(TimeRange {
            start: parse_timestamp(start)?,
            end: parse_timestamp(end)?,
        }),
    })
}

/// Validates the inputs and builds background cues and an ASS karaoke script without file I/O.
pub fn plan(document: &Document, style: &Style) -> Result<RenderPlan> {
    validate_document(document)?;
    validate_style(style)?;
    let background_images = resolve_backgrounds(document.duration, &style.background_images)?;
    let video_source = format!(
        "color=c={}:s={}x{}:r={}:d={}",
        style.background_color,
        style.width,
        style.height,
        style.frames_per_second,
        document.duration
    );
    Ok(RenderPlan {
        video_source,
        background_images,
        subtitles: build_subtitles(document, style),
    })
}

/// Encodes one lyric video from its timing document and audio recording.
pub fn render(request: &RenderRequest) -> Result<()> {
    let json = fs::read_to_string(&request.timings)
        .with_context(|| format!("reading timing file {}", request.timings.display()))?;
    let document = parse_document(&json)?;
    let plan = plan(&document, &request.style)?;
    let ffmpeg = find_ffmpeg()?;
    let subtitle_file = TemporarySubtitle::create(&plan.subtitles)?;
    run_ffmpeg(&ffmpeg, request, &document, &plan, subtitle_file.path())
}

/// Parses one strict `HH:MM:SS.mmm` timestamp into seconds.
fn parse_timestamp(value: &str) -> Result<f64> {
    let parts: Vec<_> = value.split(':').collect();
    if parts.len() != 3
        || parts[0].len() < 2
        || parts[1].len() != 2
        || !parts[0].bytes().all(|byte| byte.is_ascii_digit())
        || !parts[1].bytes().all(|byte| byte.is_ascii_digit())
    {
        bail!("invalid background image timestamp {value:?}");
    }
    let seconds: Vec<_> = parts[2].split('.').collect();
    if seconds.len() != 2
        || seconds[0].len() != 2
        || seconds[1].len() != 3
        || !seconds
            .iter()
            .all(|part| part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        bail!("invalid background image timestamp {value:?}");
    }
    let hours: u64 = parts[0]
        .parse()
        .with_context(|| format!("invalid background image timestamp {value:?}"))?;
    let minutes: u64 = parts[1]
        .parse()
        .with_context(|| format!("invalid background image timestamp {value:?}"))?;
    let whole_seconds: u64 = seconds[0]
        .parse()
        .with_context(|| format!("invalid background image timestamp {value:?}"))?;
    let milliseconds: u64 = seconds[1]
        .parse()
        .with_context(|| format!("invalid background image timestamp {value:?}"))?;
    if minutes >= 60 || whole_seconds >= 60 {
        bail!("invalid background image timestamp {value:?}");
    }
    let total_seconds = hours
        .checked_mul(3_600)
        .and_then(|total| {
            minutes
                .checked_mul(60)
                .and_then(|part| total.checked_add(part))
        })
        .and_then(|total| total.checked_add(whole_seconds))
        .with_context(|| format!("invalid background image timestamp {value:?}"))?;
    Ok(total_seconds as f64 + milliseconds as f64 / 1_000.0)
}

/// Rejects document values that cannot produce a bounded subtitle timeline.
fn validate_document(document: &Document) -> Result<()> {
    if document.version != SUPPORTED_FORMAT_VERSION {
        bail!("unsupported Lockstep format version {}", document.version);
    }
    if !document.duration.is_finite() || document.duration <= 0.0 {
        bail!("duration must be finite and positive");
    }
    let mut previous_start = None;
    for (line_index, line) in document.lines.iter().enumerate() {
        if !line.start.is_finite()
            || !line.end.is_finite()
            || line.start < 0.0
            || line.end < line.start
            || line.end > document.duration
        {
            bail!("line {line_index} has an invalid time range");
        }
        if previous_start.is_some_and(|start| line.start < start) {
            bail!("lyric lines must be ordered by start time");
        }
        previous_start = Some(line.start);
        let mut previous_word_start = None;
        for (word_index, word) in line.words.iter().enumerate() {
            if !word.start.is_finite()
                || !word.end.is_finite()
                || word.start < line.start
                || word.end < word.start
                || word.end > line.end
            {
                bail!("word {word_index} in line {line_index} has an invalid time range");
            }
            if previous_word_start.is_some_and(|start| word.start < start) {
                bail!("words in line {line_index} must be ordered by start time");
            }
            previous_word_start = Some(word.start);
        }
    }
    Ok(())
}

/// Rejects style values that would break ASS records or FFmpeg sources.
fn validate_style(style: &Style) -> Result<()> {
    for (name, value) in [
        ("width", style.width),
        ("height", style.height),
        ("frames_per_second", style.frames_per_second),
        ("font_size", style.font_size),
    ] {
        if value == 0 {
            bail!("{name} must be greater than zero");
        }
    }
    if style.line_count == 0 {
        bail!("line_count must be greater than zero");
    }
    for (name, color) in [
        ("background_color", &style.background_color),
        ("text_color", &style.text_color),
        ("highlight_color", &style.highlight_color),
    ] {
        if color.len() != 7
            || !color.starts_with('#')
            || !color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            bail!("{name} must be a #RRGGBB color");
        }
    }
    if style.font.is_empty()
        || style
            .font
            .chars()
            .any(|character| matches!(character, ',' | '\r' | '\n'))
    {
        bail!("font contains an ASS field or record delimiter");
    }
    Ok(())
}

/// Applies single-image defaults, validates ranges, and sorts cues by start time.
fn resolve_backgrounds(duration: f64, images: &[BackgroundImage]) -> Result<Vec<BackgroundCue>> {
    if images.len() > 1 && images.iter().any(|image| image.range.is_none()) {
        bail!("multiple background images require timestamp ranges");
    }
    let mut cues = images
        .iter()
        .map(|image| BackgroundCue {
            path: image.path.clone(),
            range: image.range.unwrap_or(TimeRange {
                start: 0.0,
                end: duration,
            }),
        })
        .collect::<Vec<_>>();
    for cue in &cues {
        if cue.path.as_os_str().is_empty() {
            bail!("background image path cannot be empty");
        }
        if !cue.range.start.is_finite() || !cue.range.end.is_finite() {
            bail!("background image range must be finite");
        }
        if cue.range.end <= cue.range.start {
            bail!("background image range must end after it starts");
        }
        if cue.range.start < 0.0 || cue.range.end > duration {
            bail!("background image range falls outside track duration");
        }
    }
    cues.sort_by(|left, right| left.range.start.total_cmp(&right.range.start));
    if cues
        .windows(2)
        .any(|pair| pair[0].range.end > pair[1].range.start)
    {
        bail!("background image ranges overlap");
    }
    Ok(cues)
}

/// Builds the complete ASS document from validated lyrics and styling.
fn build_subtitles(document: &Document, style: &Style) -> String {
    let mut script = String::new();
    writeln!(script, "[Script Info]").unwrap();
    writeln!(script, "ScriptType: v4.00+").unwrap();
    writeln!(script, "PlayResX: {}", style.width).unwrap();
    writeln!(script, "PlayResY: {}", style.height).unwrap();
    writeln!(script, "WrapStyle: 2\n").unwrap();
    writeln!(script, "[V4+ Styles]").unwrap();
    writeln!(script, "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding").unwrap();
    writeln!(
        script,
        "Style: Lyrics,{},{},{},{},&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,5,40,40,40,1",
        style.font,
        style.font_size,
        ass_color(&style.highlight_color),
        ass_color(&style.text_color)
    )
    .unwrap();
    writeln!(
        script,
        "Style: Plain,{},{},{},{},&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,5,40,40,40,1\n",
        style.font,
        style.font_size,
        ass_color(&style.text_color),
        ass_color(&style.text_color)
    )
    .unwrap();
    writeln!(script, "[Events]").unwrap();
    writeln!(
        script,
        "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text"
    )
    .unwrap();
    append_events(&mut script, document, style);
    script
}

/// Converts `#RRGGBB` to ASS's alpha-blue-green-red notation.
fn ass_color(color: &str) -> String {
    format!("&H00{}{}{}", &color[5..7], &color[3..5], &color[1..3])
}

/// Holds complete lyric pages through their final sung line and shows notes during rests.
fn append_events(script: &mut String, document: &Document, style: &Style) {
    let lines = document
        .lines
        .iter()
        .filter(|line| line.end > line.start)
        .collect::<Vec<_>>();
    let windows = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            let end = line.end.min(
                lines
                    .get(index + 1)
                    .map_or(document.duration, |next| next.start),
            );
            (end > line.start).then_some((*line, end))
        })
        .collect::<Vec<_>>();
    let mut cursor = 0.0;
    for page in windows.chunks(style.line_count) {
        let display_start = page[0].0.start;
        let display_end = page.last().unwrap().1;
        for (slot, (line, end)) in page.iter().enumerate() {
            if line.start > cursor {
                append_rest(script, cursor, line.start, style);
            }
            let (event_style, body) = if line.words.is_empty() {
                ("Plain", escape_text(&line.text))
            } else {
                ("Lyrics", active_word_text(line, display_start, *end, style))
            };
            let y = row_y(style, slot);
            let text = format!(r"{{\an5\pos({},{y})}}{body}", style.width / 2);
            append_dialogue(script, display_start, display_end, event_style, &text);
            cursor = *end;
        }
    }
    if cursor < document.duration {
        append_rest(script, cursor, document.duration, style);
    }
}

/// Centers fixed lyric slots without recentering when another slot becomes empty.
fn row_y(style: &Style, slot: usize) -> i64 {
    i64::from(style.height) / 2
        + (2 * slot as i64 + 1 - style.line_count as i64) * i64::from(style.font_size) * 3 / 4
}

/// Places notes below preview rows during silence, or centrally in single-line mode.
fn append_rest(script: &mut String, start: f64, end: f64, style: &Style) {
    let y = if style.line_count == 1 {
        i64::from(style.height) / 2
    } else {
        row_y(style, style.line_count)
    };
    let text = format!(
        r"{{\an5\pos({},{y})}}{}",
        style.width / 2,
        escape_text(&style.rest_text)
    );
    append_dialogue(script, start, end, "Plain", &text);
}

/// Highlights only active words with effects contained within their spans and fixed glyph positions.
fn active_word_text(line: &Line, display_start: f64, event_end: f64, style: &Style) -> String {
    let mut text = String::new();
    let mut remaining = line.text.as_str();
    let highlight = ass_color(&style.highlight_color);
    let plain = ass_color(&style.text_color);
    let mut next = 0;
    for (index, word) in line.words.iter().enumerate() {
        if let Some(offset) = remaining.find(&word.text) {
            text.push_str(&escape_text(&remaining[..offset]));
            remaining = &remaining[offset + word.text.len()..];
        } else if index > 0 {
            text.push(' ');
        }
        while next < line.words.len() && line.words[next].start <= word.start {
            next += 1;
        }
        let end = word
            .end
            .min(line.words.get(next).map_or(event_end, |word| word.start))
            .min(event_end);
        let start = centiseconds(word.start).saturating_sub(centiseconds(display_start)) * 10;
        let end = centiseconds(end).saturating_sub(centiseconds(display_start)) * 10;
        text.push_str(r"{\rPlain");
        if end > start {
            let fade = u64::from(style.highlight_transition_ms).min((end - start) / 2);
            if fade > 0 {
                write!(
                    text,
                    r"\t({start},{},0.5,\1c{highlight}&)\t({},{end},2,\1c{plain}&)",
                    start + fade,
                    end - fade
                )
                .unwrap();
            } else {
                if start == 0 {
                    write!(text, r"\1c{highlight}&").unwrap();
                } else {
                    write!(text, r"\t({},{start},\1c{highlight}&)", start - 1).unwrap();
                }
                write!(text, r"\t({},{end},\1c{plain}&)", end - 1).unwrap();
            }
        }
        write!(text, "}}{}", escape_text(&word.text)).unwrap();
    }
    text
}

/// Writes one ASS dialogue row using absolute second endpoints.
fn append_dialogue(script: &mut String, start: f64, end: f64, style: &str, text: &str) {
    writeln!(
        script,
        "Dialogue: 0,{},{},{style},,0,0,0,,{text}",
        ass_time(start),
        ass_time(end)
    )
    .unwrap();
}

/// Escapes literal braces that ASS would otherwise parse as override blocks.
fn escape_text(text: &str) -> String {
    text.replace('{', r"\{").replace('}', r"\}")
}

/// Converts seconds to a rounded absolute ASS centisecond offset.
fn centiseconds(seconds: f64) -> u64 {
    (seconds * 100.0).round() as u64
}

/// Formats seconds as an ASS `H:MM:SS.cc` timestamp.
fn ass_time(seconds: f64) -> String {
    let total = centiseconds(seconds);
    let hours = total / 360_000;
    let minutes = total / 6_000 % 60;
    let whole_seconds = total / 100 % 60;
    let fraction = total % 100;
    format!("{hours}:{minutes:02}:{whole_seconds:02}.{fraction:02}")
}

/// Confirms that the selected FFmpeg exposes the libass-backed ASS filter.
fn require_ass_filter(ffmpeg: &std::ffi::OsStr) -> Result<()> {
    let output = Command::new(ffmpeg)
        .args(["-hide_banner", "-filters"])
        .output()
        .with_context(|| format!("running {}", Path::new(ffmpeg).display()))?;
    if !output.status.success() {
        bail!(
            "FFmpeg filter discovery failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let filters = String::from_utf8_lossy(&output.stdout);
    let has_ass = filters.lines().any(|line| {
        line.split_whitespace()
            .nth(1)
            .is_some_and(|name| name == "ass")
    });
    if !has_ass {
        bail!("FFmpeg lacks the libass-backed ass filter required for lyric rendering");
    }
    Ok(())
}

/// Finds libass-enabled FFmpeg, including Homebrew's unlinked full build.
fn find_ffmpeg() -> Result<std::ffi::OsString> {
    if let Some(binary) = std::env::var_os("LOCKSTEP_VIDEO_FFMPEG") {
        require_ass_filter(&binary)?;
        return Ok(binary);
    }
    for binary in [
        "ffmpeg",
        "/opt/homebrew/opt/ffmpeg-full/bin/ffmpeg",
        "/usr/local/opt/ffmpeg-full/bin/ffmpeg",
    ] {
        if require_ass_filter(binary.as_ref()).is_ok() {
            return Ok(binary.into());
        }
    }
    bail!("no FFmpeg with the ass filter found; install ffmpeg-full or set LOCKSTEP_VIDEO_FFMPEG to a libass-enabled build")
}

/// Runs FFmpeg with a color base, optional timed image overlays, ASS, and copied audio timing.
fn run_ffmpeg(
    ffmpeg: &std::ffi::OsStr,
    request: &RenderRequest,
    document: &Document,
    plan: &RenderPlan,
    subtitle_path: &Path,
) -> Result<()> {
    let mut command = Command::new(ffmpeg);
    command
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-y",
            "-f",
            "lavfi",
            "-i",
        ])
        .arg(&plan.video_source)
        .arg("-i")
        .arg(&request.audio);
    for cue in &plan.background_images {
        command
            .args([
                "-loop",
                "1",
                "-framerate",
                &request.style.frames_per_second.to_string(),
                "-i",
            ])
            .arg(&cue.path);
    }
    command
        .arg("-filter_complex")
        .arg(filter_graph(request, plan, subtitle_path))
        .args([
            "-map",
            "[video]",
            "-map",
            "1:a:0",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-t",
            &document.duration.to_string(),
        ])
        .arg(&request.output);
    let output = command
        .output()
        .context("running FFmpeg lyric-video encode")?;
    if !output.status.success() {
        bail!(
            "FFmpeg lyric-video encode failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// Builds the image-overlay and subtitle filter graph without interpolating image paths.
fn filter_graph(request: &RenderRequest, plan: &RenderPlan, subtitle_path: &Path) -> String {
    let mut filters = Vec::new();
    let mut base = "0:v".to_string();
    for (index, cue) in plan.background_images.iter().enumerate() {
        let input = index + 2;
        let scaled = format!("image{index}");
        let overlaid = format!("background{index}");
        filters.push(format!(
            "[{input}:v]scale={}:{}:force_original_aspect_ratio=increase,crop={}:{},setsar=1[{scaled}]",
            request.style.width, request.style.height, request.style.width, request.style.height
        ));
        filters.push(format!(
            "[{base}][{scaled}]overlay=0:0:enable='gte(t,{})*lt(t,{})'[{overlaid}]",
            cue.range.start, cue.range.end
        ));
        base = overlaid;
    }
    filters.push(format!(
        "[{base}]ass=filename='{}'[video]",
        escape_filter_path(subtitle_path)
    ));
    filters.join(";")
}

/// Escapes an ASS filename for FFmpeg's filter-option parser.
fn escape_filter_path(path: &Path) -> String {
    let mut escaped = String::new();
    for character in path.to_string_lossy().chars() {
        if matches!(character, '\\' | '\'' | ':' | ',' | ';' | '[' | ']') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

static SUBTITLE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Temporary subtitle path removed after the encoder exits.
struct TemporarySubtitle(PathBuf);

impl TemporarySubtitle {
    /// Writes a uniquely named ASS file in the process temporary directory.
    fn create(contents: &str) -> Result<Self> {
        let sequence = SUBTITLE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "lockstep-video-{}-{sequence}.ass",
            std::process::id()
        ));
        fs::write(&path, contents)
            .with_context(|| format!("writing temporary subtitle file {}", path.display()))?;
        Ok(Self(path))
    }

    /// Returns the temporary subtitle path for FFmpeg.
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporarySubtitle {
    /// Removes the temporary subtitle file after encoding or failure.
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
