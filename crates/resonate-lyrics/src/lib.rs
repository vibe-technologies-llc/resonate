mod embedded;
mod error;
mod lrc;
mod lyricsfile;
mod model;
mod provider;
mod sidecar;

#[cfg(fuzzing)]
pub fn read_an_lrc_sheet(text: &str) {
    let source = resonate_core::SourceId::local();
    let _ = crate::lrc::read(source, text);
}

#[cfg(fuzzing)]
pub fn read_a_lyricsfile(text: &str) {
    let source = resonate_core::SourceId::local();
    let _ = crate::lyricsfile::read_lyricsfile(source, text);
}

pub use crate::{
    embedded::Embedded,
    error::{Error, LyricOp, Result},
    lyricsfile::{LARGEST_LYRICSFILE, Lyricsfile, Unread, read_lyricsfile},
    model::{Credits, Detail, LyricLine, Lyrics, Singing, SungWord, Sweep, Timing, Voice, Waiting},
    provider::{LyricProvider, Lyricists, Unsourced, Wanted, read_lyrics},
    sidecar::Sidecar,
};
