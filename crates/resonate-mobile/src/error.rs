use std::{fmt, io, path::PathBuf, result};

use resonate_core::{abx::TRIALS_AT_MOST, eq::MAX_BANDS};
use thiserror::Error;

use crate::{
    downloads::MusicProvider,
    equaliser::{GAIN_AT_MOST_DB, HIGHEST_HZ, LOWEST_HZ, PREAMP_AT_MOST_DB, Q_AT_LEAST, Q_AT_MOST},
};

#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum StoreOp {
    CreateFolder,
    Open,
    LayOut,
    Read,
    Write,
}

impl fmt::Display for StoreOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::CreateFolder => "created",
            Self::Open => "opened",
            Self::LayOut => "laid out",
            Self::Read => "read",
            Self::Write => "written",
        })
    }
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
pub enum RewriteOp {
    CopiedAside,
    Opened,
    Written,
    ReadBack,
    PutBack,
}

impl fmt::Display for RewriteOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::CopiedAside => "copied aside to edit",
            Self::Opened => "opened for writing",
            Self::Written => "written",
            Self::ReadBack => "read back",
            Self::PutBack => "put back",
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
    Forget,
}

impl fmt::Display for SyncOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Start => "start",
            Self::Mint => "make a new pairing code",
            Self::Missing => "list the songs this device lacks",
            Self::Keep => "keep your changes",
            Self::Handoffs => "read what other devices are playing",
            Self::Forget => "forget that device",
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ReceivedOp {
    Open,
    Copy,
    Finish,
}

impl fmt::Display for ReceivedOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Open => "opened",
            Self::Copy => "copied",
            Self::Finish => "finished",
        })
    }
}

#[derive(Debug, Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum Error {
    #[error("{}", crate::failure::of_a_player(.0))]
    Player(#[from] resonate_engine::Error),
    #[error("{}", crate::failure::of_a_read(.0))]
    Decode(#[from] resonate_codec::Error),
    #[error(transparent)]
    Domain(#[from] resonate_core::Error),
    #[error("The document catalog could not be {op}: {source}")]
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
    #[error("Renderers on the network cannot be played to from this phone")]
    RenderersUnreachable,
    #[error("The network could not be searched for renderers")]
    RendererSearch {
        #[source]
        source: Box<resonate_upnp::Error>,
    },
    #[error("That renderer is no longer on the network")]
    UnknownRenderer,
    #[error("the destination descriptor is invalid")]
    InvalidDescriptor,
    #[error("The downloaded file is not the song asked for: its length differs")]
    WrongSong,
    #[error("{}", crate::failure::of_a_lookup(*op, source))]
    Reference {
        op: resonate_library::LookupOp,
        #[source]
        source: Box<resonate_library::Error>,
    },
    #[error("the equaliser profile could not be read")]
    Equaliser(#[from] resonate_eq::Error),
    #[error("{}", crate::failure::of_a_measurement(*file, source))]
    Measurement {
        file: crate::measurement::MeasuredFile,
        #[source]
        source: Box<resonate_eq::Error>,
    },
    #[error("{}", crate::failure::of_a_sweep(source))]
    Sweep {
        #[source]
        source: resonate_dsp::Error,
    },
    #[error("No built-in target is called {name}")]
    UnknownTarget { name: String },
    #[error("this service does not recognise the account")]
    UnknownAccount,
    #[error("audio analysis failed")]
    Analysis(#[from] resonate_analysis::Error),
    #[error("lyrics could not be read")]
    Lyrics(#[from] resonate_lyrics::Error),
    #[error("The lyrics are larger than one MiB ({bytes} bytes)")]
    LyricsTooLarge { bytes: u64 },
    #[error("The clip is longer than thirty seconds ({samples} samples)")]
    ClipTooLong { samples: u64 },
    #[error("none of these recordings could be matched to the local library")]
    NoLocalMatches,
    #[error("The playlist file is larger than eight MiB ({bytes} bytes)")]
    PlaylistFileTooLarge { bytes: u64 },
    #[error("The playlist file lists no songs")]
    PlaylistFileEmpty,
    #[error("The playlist could not be written to a file")]
    PlaylistFileUnwritten {
        #[source]
        source: Box<resonate_library::Error>,
    },
    #[error("a saved recording must have a title")]
    InvalidWantedSong,
    #[error("online features are switched off")]
    OnlineOff,
    #[error("No service to play from is set up")]
    NoServedServer,
    #[error("The clip could not be named: {0}")]
    Heard(#[from] resonate_listen::Error),
    #[error("a playlist name must have between one and 255 bytes")]
    InvalidPlaylistName,
    #[error("another playlist already has that name")]
    DuplicatePlaylistName,
    #[error("the playlist no longer exists")]
    UnknownPlaylist,
    #[error("the playlist changed; refresh it before editing")]
    PlaylistChanged,
    #[error("A saved search fills itself; its songs cannot be added, removed or moved")]
    NotAList,
    #[error("A playlist of songs has no search to revise")]
    NotASavedSearch,
    #[error("the picture is unsupported or too large")]
    InvalidPicture,
    #[error("the document URI is not supported")]
    InvalidUri,
    #[error("document catalog version {found} is newer than this build supports")]
    CatalogVersion { found: u32 },
    #[error("the requested track is not in the library")]
    UnknownTrack,
    #[error("An equaliser holds at most {most} bands, not {found}", most = MAX_BANDS)]
    TooManyBands { found: u32 },
    #[error("Band {} must sit between {LOWEST_HZ} Hz and {} kHz, not {hertz} Hz", band + 1, HIGHEST_HZ / 1000.0)]
    BandFrequency { band: u32, hertz: f32 },
    #[error("Band {} may boost or cut at most {GAIN_AT_MOST_DB} dB, not {decibels} dB", band + 1)]
    BandGain { band: u32, decibels: f32 },
    #[error("Band {}'s Q must lie between {Q_AT_LEAST} and {Q_AT_MOST}, not {q}", band + 1)]
    BandQ { band: u32, q: f32 },
    #[error("Band {} is played on no channel", band + 1)]
    BandChannels { band: u32 },
    #[error("The preamp may lift or lower at most {PREAMP_AT_MOST_DB} dB, not {decibels} dB")]
    PreampRange { decibels: f32 },
    #[error("A listening test holds from 1 to {TRIALS_AT_MOST} trials, not {trials}")]
    ListeningTestTrials { trials: u32 },
    #[error("The curve has no bands and no preamp, so there is nothing to compare with its bypass")]
    ListeningTestNothingToTest,
    #[error(
        "The curve cannot be level-matched to its bypass, {millibels} thousandths of a decibel apart, so it is not tested"
    )]
    ListeningTestLevels { millibels: i32 },
    #[error("That listening test has no trial left to answer")]
    ListeningTestFinished,
    #[error("buffer length {milliseconds} ms is outside 50 to 2000 ms")]
    BufferRange { milliseconds: u32 },
    #[error("the player has been closed")]
    Closed,
    #[error("There is no zone {zone}")]
    NoZone { zone: u32 },
    #[error("A zone needs a name")]
    ZoneUnnamed,
    #[error("The first zone stays")]
    FirstZoneStays,
    #[error("no queue row has the id {queue_id}")]
    UnknownQueueRow { queue_id: u64 },
    #[error("queue row {row} is outside a queue of {rows}")]
    QueueRowOutOfRange { row: u32, rows: u64 },
    #[error("Device sync could not {op}")]
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
    #[error("The song from another device could not be {op}")]
    Received {
        op: ReceivedOp,
        #[source]
        source: io::Error,
    },
    #[error("The layout cannot be read: {source}")]
    LayoutUnreadable {
        #[source]
        source: Box<resonate_library::Error>,
    },
    #[error("Tags of this kind of file cannot be written")]
    TagsUnwritable,
    #[error("The file did not read back with the tags written")]
    TagsUnconfirmed,
    #[error("The song's file could not be {op}")]
    TagsDocument {
        op: RewriteOp,
        #[source]
        source: io::Error,
    },
    #[error("The song's file was left damaged by a failed write and could not be put back; its original is kept at {}", .kept.display())]
    TagsUnrestored { kept: PathBuf },
    #[error("A track or disc number must be a whole number above zero")]
    TagNumber,
    #[error("A room correction file must be an audio file")]
    ImpulseExtension,
    #[error("The room correction file is larger than 64 MiB")]
    ImpulseTooLarge,
    #[error("The file holds no response room correction can use")]
    NoImpulse,
    #[error("The two speaker responses are not a pair: {source}")]
    ImpulsePair {
        #[source]
        source: Box<resonate_engine::PairedReadError>,
    },
    #[error("The room correction file could not be {op}")]
    ImpulseFile {
        op: crate::room::ImpulseOp,
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

    pub(crate) fn measurement(
        file: crate::measurement::MeasuredFile,
        source: resonate_eq::Error,
    ) -> Self {
        Self::Measurement {
            file,
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

    #[test]
    fn domain_error_names_the_rejected_value() {
        let domain = resonate_core::Error::VolumeOutOfRange(1.5);
        let mobile = Error::from(domain);
        assert_eq!(mobile.to_string(), domain.to_string());
    }

    #[test]
    fn a_refused_command_and_a_failed_op_are_sentences_not_debug_prints() {
        let refused = Error::from(resonate_engine::Error::QueueEmpty);
        let opened = Error::store(StoreOp::Open, rusqlite::Error::InvalidQuery);
        let kept = Error::Received {
            op: ReceivedOp::Copy,
            source: io::Error::other("full"),
        };

        assert_eq!(refused.to_string(), "Nothing is playing right now");
        assert!(
            opened
                .to_string()
                .starts_with("The document catalog could not be opened")
        );
        assert_eq!(
            kept.to_string(),
            "The song from another device could not be copied"
        );
    }
}
