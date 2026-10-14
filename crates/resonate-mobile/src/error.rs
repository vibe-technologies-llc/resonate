use std::{fmt, io, path::PathBuf, result};

use thiserror::Error;

use crate::downloads::MusicProvider;

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
    Open,
    Read,
    Write,
    Finish,
}

impl fmt::Display for DownloadOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Open => "opened",
            Self::Read => "read",
            Self::Write => "written",
            Self::Finish => "finished",
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub enum SyncOp {
    Start,
    Mint,
    Missing,
    Keep,
    Handoffs,
}

#[derive(Clone, Copy, Debug)]
pub enum ReceivedOp {
    Open,
    Copy,
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
    #[error("No download provider is set up")]
    NoProvider,
    #[error("No provider had this song")]
    NotFound,
    #[error("{provider} could not be reached")]
    Unreachable { provider: MusicProvider },
    #[error("{provider} did not answer in time")]
    ProviderAway { provider: MusicProvider },
    #[error("{provider} refused the request ({status})")]
    ProviderRefused {
        provider: MusicProvider,
        status: u16,
    },
    #[error("{provider} did not accept the account (error {code})")]
    AccountRefused { provider: MusicProvider, code: u16 },
    #[error("{provider} has a certificate this phone does not trust")]
    ProviderUntrusted { provider: MusicProvider },
    #[error("{provider} is not at that address")]
    ProviderMisaddressed { provider: MusicProvider },
    #[error("{provider} answered with something that could not be read")]
    ProviderUnreadable { provider: MusicProvider },
    #[error("The {provider} sign-in expired before it was approved")]
    SignInLapsed { provider: MusicProvider },
    #[error("The {provider} sign-in was turned down")]
    SignInDenied { provider: MusicProvider },
    #[error("The download could not be {op}")]
    DownloadIo {
        op: DownloadOp,
        #[source]
        source: io::Error,
    },
    #[error("The download was cancelled")]
    DownloadCancelled,
    #[error("The download offer expired; search again")]
    DownloadExpired,
    #[error("No download is under that number")]
    UnknownDownload,
    #[error("The downloaded file exceeds the eight GiB limit")]
    DownloadTooLarge,
    #[error("this audio output is unavailable")]
    OutputUnavailable,
    #[error("the destination descriptor is invalid")]
    InvalidDescriptor,
    #[error("The downloaded file is not the song asked for: its length differs")]
    WrongSong,
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
    #[error("no queue row has the id {queue_id}")]
    UnknownQueueRow { queue_id: u64 },
    #[error("queue row {row} is outside a queue of {rows}")]
    QueueRowOutOfRange { row: u32, rows: u64 },
    #[error("device sync operation {op:?} failed")]
    Sync {
        op: SyncOp,
        #[source]
        source: Box<resonate_sync::Error>,
    },
    #[error("device sync is switched off")]
    SyncOff,
    #[error("no device in sync is called that")]
    UnknownSyncDevice,
    #[error("that device is playing nothing held here")]
    NothingHandedOff,
    #[error("that song from another device is no longer waiting")]
    UnknownReceivedSong,
    #[error("received song operation {op:?} failed")]
    Received {
        op: ReceivedOp,
        #[source]
        source: io::Error,
    },
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

    pub(crate) fn sync(op: SyncOp, source: resonate_sync::Error) -> Self {
        Self::Sync {
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
