//! Times a known script against its recording, word by word.
//!
//! The words written out always come from the script, never from the transcriber, so a misheard
//! word costs a little precision on its line rather than putting the wrong text on screen.
//!
//! ```no_run
//! use lockstep::export::{render, Format};
//! use std::path::Path;
//!
//! # fn main() -> anyhow::Result<()> {
//! let lines = lockstep::script::read(Path::new("script.txt"))?;
//! let duration = lockstep::audio::prepare(Path::new("song.mp3"), Some(Path::new("/tmp/a.wav")))?;
//! let transcript = lockstep::whisper::transcribe(Path::new("/tmp/a.wav"), &Default::default())?;
//! let heard = lockstep::whisper::parse(&transcript)?;
//!
//! let (document, report) =
//!     lockstep::timing::build(&lines, &heard, Path::new("script.txt"), duration)?;
//! println!("{}", render(&document, Format::Vtt)?);
//! # let _ = report; Ok(())
//! # }
//! ```

pub mod align;
pub mod audio;
pub mod export;
pub mod forced;
pub mod script;
pub mod timing;
pub mod whisper;
