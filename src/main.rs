//! Times a known script against its recording, word by word.

mod align;
mod audio;
mod script;
mod timing;
mod whisper;

use anyhow::{Context, Result};
use clap::Parser;
use std::path::{Path, PathBuf};

/// Below this share of matched words a script and a recording almost certainly disagree.
/// Correctly paired recordings measure around 0.98; mismatched ones around 0.08 to 0.16.
const SUSPECT: f64 = 0.5;

const AFTER_HELP: &str = "\
Examples:
  lockstep song.mp3 lyrics.txt
      Times the song and writes song.json beside it.

  lockstep talk.m4a script.txt -o timed.json --model ~/models/ggml-medium.en.bin
      Picks the output path and the model explicitly.

  lockstep song.mp3 lyrics.txt --transcript from-ci.json
      Skips transcription and reuses a whisper JSON produced elsewhere. Needs no
      whisper install; the recording is still read, for its duration.

Transcribing needs whisper.cpp and a model. Both are found automatically when not
given: the binary on PATH, the model in ./models, ~/.cache/whisper and the usual
system directories. Decoding and alignment are pure Rust and need nothing.

lockstep prints the share of the script it actually heard, and warns below 50% that
the two files probably do not go together. Words whose time could not be measured
are marked \"carried\" or \"spread\" in the output. README.md has the details.";

#[derive(Parser)]
#[command(
    version,
    about = "Times a known script against its recording, word by word",
    long_about = "Times a known script against its recording, word by word.\n\n\
        Give it an audio file and the text spoken or sung in it, and it writes JSON saying \
        when every word arrives: enough to drive a lyric video, a karaoke display, or a \
        follow-along transcript. The words written out always come from your script, never \
        from the transcriber, so a misheard word costs a little precision rather than \
        putting the wrong text on screen.",
    after_help = AFTER_HELP
)]
struct Args {
    /// Recording to time against. Any format symphonia decodes: mp3, wav, flac, aac, m4a, ogg
    #[arg(value_name = "AUDIO")]
    audio: PathBuf,

    /// Plain-text script to time. One line of it per line of output; blank lines are dropped
    #[arg(value_name = "SCRIPT")]
    script: PathBuf,

    /// Where to write the timed JSON [default: the recording's name with a .json extension]
    #[arg(short, long, value_name = "FILE")]
    out: Option<PathBuf>,

    /// Reuse a whisper.cpp JSON transcript instead of transcribing again
    #[arg(long, value_name = "FILE")]
    transcript: Option<PathBuf>,

    /// whisper.cpp model file [default: the largest ggml-*.bin found in the usual places]
    #[arg(long, value_name = "FILE", env = "LOCKSTEP_MODEL")]
    model: Option<PathBuf>,

    /// whisper.cpp executable [default: whisper-cli, whisper-cpp or whisper on PATH]
    #[arg(long, value_name = "FILE", env = "LOCKSTEP_WHISPER")]
    whisper: Option<PathBuf>,

    /// Alignment-heads preset for whisper's -dtw, such as small.en or large.v3
    /// [default: read from the model's filename]
    #[arg(long, value_name = "PRESET")]
    dtw: Option<String>,

    /// Keep the intermediate WAV and transcript, and print the directory holding them
    #[arg(long)]
    keep: bool,
}

/// Times the script and writes the result, using `work` for anything intermediate.
fn run(args: &Args, work: &Path) -> Result<()> {
    let lines = script::read(&args.script)?;
    let wav = work.join("audio.wav");

    let duration = audio::prepare(&args.audio, args.transcript.is_none().then_some(&*wav))?;
    let transcript = match &args.transcript {
        Some(path) => std::fs::read_to_string(path)
            .with_context(|| format!("reading transcript {}", path.display()))?,
        None => whisper::transcribe(
            &wav,
            &whisper::Config {
                binary: args.whisper.clone(),
                model: args.model.clone(),
                dtw: args.dtw.clone(),
            },
        )?,
    };

    let heard = whisper::parse(&transcript)?;
    let source = args.script.display().to_string();
    let (document, report) = timing::build(&lines, &heard, &source, duration)?;

    let matched = report.confidence();
    let out = args.out.clone().unwrap_or_else(|| args.audio.with_extension("json"));
    let json = serde_json::to_string_pretty(&document)?;
    std::fs::write(&out, format!("{json}\n"))
        .with_context(|| format!("writing {}", out.display()))?;

    println!(
        "{}: {} lines ({}/{} anchored, {} rests), {}/{} script words matched ({:.0}%), \
         {} words heard",
        out.display(),
        report.lines,
        report.anchored,
        report.script_lines,
        report.rests,
        report.matched,
        report.script_words,
        matched * 100.0,
        report.heard
    );
    if matched < SUSPECT {
        eprintln!(
            "warning: only {:.0}% of {} was heard in {}. A correctly paired script and \
             recording match around 98%, so these two are probably not the same piece, or the \
             model cannot hear this language. The timings written are guesses.",
            matched * 100.0,
            args.script.display(),
            args.audio.display()
        );
    }
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    let work = std::env::temp_dir().join(format!("lockstep-{}", std::process::id()));
    std::fs::create_dir_all(&work)
        .with_context(|| format!("creating work directory {}", work.display()))?;

    let result = run(&args, &work);
    if args.keep {
        eprintln!("kept intermediates in {}", work.display());
    } else {
        let _ = std::fs::remove_dir_all(&work);
    }
    result
}
