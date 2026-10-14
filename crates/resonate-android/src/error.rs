use std::{io, path::PathBuf, result};

use thiserror::Error;

#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum StoreOp {
    CreateFolder,
    Open,
    Migrate,
    Read,
    Write,
}

#[derive(Clone, Copy, Debug)]
pub enum DownloadOp {
    Find,
    Open,
    Read,
    Write,
    Finish,
}

#[derive(Debug, Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum Error {
    #[error("the player could not complete the command")]
    Player(#[from] resonate_engine::Error),
    #[error("the audio file could not be read")]
    Decode(#[from] resonate_codec::Error),
    #[error("a domain value is out of range")]
    Domain(#[from] resonate_core::Error),
    #[error("the document catalog operation {op:?} failed")]
    Store {
        op: StoreOp,
        #[source]
        source: Box<rusqlite::Error>,
    },
    #[error("music provider operation {op:?} failed")]
    Provider {
        op: DownloadOp,
        #[source]
        source: Box<resonate_providers::Error>,
    },
    #[error("download I/O operation {op:?} failed")]
    DownloadIo {
        op: DownloadOp,
        #[source]
        source: io::Error,
    },
    #[error("the download was cancelled")]
    DownloadCancelled,
    #[error("the download offer expired; search again")]
    DownloadExpired,
    #[error("the downloaded file exceeds the eight GiB limit")]
    DownloadTooLarge,
    #[error("this audio output is unavailable")]
    OutputUnavailable,
    #[error("the destination descriptor is invalid")]
    InvalidDescriptor,
    #[error("the downloaded track length does not match the selected song")]
    WrongDeliveryLength,
    #[error("the online library operation {op:?} failed")]
    Reference {
        op: resonate_library::LookupOp,
        #[source]
        source: Box<resonate_library::Error>,
    },
    #[error("the equaliser profile could not be read")]
    Equaliser(#[from] resonate_eq::Error),
    #[error("this service does not recognise the account")]
    UnknownAccount,
    #[error("audio analysis failed")]
    Analysis(#[from] resonate_analysis::Error),
    #[error("lyrics could not be read")]
    Lyrics(#[from] resonate_lyrics::Error),
    #[error("none of these recordings could be matched to the local library")]
    NoLocalMatches,
    #[error("a saved recording must have a title")]
    InvalidWantedSong,
    #[error("online features are switched off")]
    OnlineOff,
    #[error("a playlist name must have between one and 255 bytes")]
    InvalidPlaylistName,
    #[error("another playlist already has that name")]
    DuplicatePlaylistName,
    #[error("the playlist no longer exists")]
    UnknownPlaylist,
    #[error("the playlist changed; refresh it before editing")]
    PlaylistChanged,
    #[error("the picture is unsupported or too large")]
    InvalidPicture,
    #[error("the document URI is not supported")]
    InvalidUri,
    #[error("the document catalog is damaged and has not been migrated")]
    CatalogDamaged,
    #[error("the catalog backup path is not UTF-8")]
    InvalidCatalogPath,
    #[error("the catalog backup could not be kept at {path}")]
    CatalogFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("document catalog version {found} is newer than this build supports")]
    CatalogVersion { found: u32 },
    #[error("the requested track is not in the library")]
    UnknownTrack,
    #[error("the equaliser needs ten bands, received {found}")]
    EqualiserBandCount { found: u64 },
    #[error("buffer length {milliseconds} ms is outside 50 to 2000 ms")]
    BufferRange { milliseconds: u32 },
    #[error("the player has been closed")]
    Closed,
    #[error("the application folder could not be created")]
    Folder {
        op: StoreOp,
        #[source]
        source: io::Error,
    },
}

impl Error {
    pub(crate) fn reference(
        op: resonate_library::LookupOp,
        source: resonate_library::Error,
    ) -> Self {
        Self::Reference {
            op,
            source: Box::new(source),
        }
    }

    pub(crate) fn store(op: StoreOp, source: rusqlite::Error) -> Self {
        Self::Store {
            op,
            source: Box::new(source),
        }
    }
}

pub type Result<T> = result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_stays_small_enough_for_result_large_err() {
        assert!(size_of::<Error>() <= 128);
    }
}
