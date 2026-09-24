mod embedded;
mod error;
mod lrc;
mod model;
mod provider;
mod sidecar;

#[cfg(fuzzing)]
pub fn read_an_lrc_sheet(text: &str) {
    let source = resonate_core::SourceId::local();
    let _ = crate::lrc::read(source, text);
}

pub use crate::{
    embedded::Embedded,
    error::{Error, LyricOp, Result},
    model::{Credits, LyricLine, Lyrics, Timing, Voice, Waiting},
    provider::{LyricProvider, Lyricists, Unsourced, Wanted, read_lyrics},
    sidecar::Sidecar,
};
