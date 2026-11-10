use std::{fmt, io, path::PathBuf, result};

use resonate_core::{abx::TRIALS_AT_MOST, eq::MAX_BANDS};
use thiserror::Error;

use crate::{
    downloads::MusicProvider,
    equaliser::{GAIN_AT_MOST_DB, HIGHEST_HZ, LOWEST_HZ, PREAMP_AT_MOST_DB, Q_AT_LEAST, Q_AT_MOST},
    words,
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
            Self::CreateFolder => words::errors::store_created(),
            Self::Open => words::errors::store_opened(),
            Self::LayOut => words::errors::store_laid_out(),
            Self::Read => words::errors::store_read(),
            Self::Write => words::errors::store_written(),
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
            Self::Open => words::errors::download_opened(),
            Self::Read => words::errors::download_read(),
            Self::Write => words::errors::download_written(),
            Self::Finish => words::errors::download_finished(),
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
            Self::CopiedAside => words::errors::rewrite_copied_aside(),
            Self::Opened => words::errors::rewrite_opened(),
            Self::Written => words::errors::rewrite_written(),
            Self::ReadBack => words::errors::rewrite_read_back(),
            Self::PutBack => words::errors::rewrite_put_back(),
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
            Self::Start => words::errors::sync_start(),
            Self::Mint => words::errors::sync_mint(),
            Self::Missing => words::errors::sync_missing(),
            Self::Keep => words::errors::sync_keep(),
            Self::Handoffs => words::errors::sync_handoffs(),
            Self::Forget => words::errors::sync_forget(),
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
            Self::Open => words::errors::received_opened(),
            Self::Copy => words::errors::received_copied(),
            Self::Finish => words::errors::received_finished(),
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
    #[error("{}", words::errors::store(op.to_string(), source.to_string()))]
    Store {
        op: StoreOp,
        #[source]
        source: Box<rusqlite::Error>,
    },
    #[error("{}", words::errors::no_provider())]
    NoProvider,
    #[error("{}", words::errors::not_found())]
    NotFound,
    #[error("{}", words::errors::unreachable(provider.to_string()))]
    Unreachable { provider: MusicProvider },
    #[error("{}", words::errors::provider_away(provider.to_string()))]
    ProviderAway { provider: MusicProvider },
    #[error("{}", words::errors::provider_refused(provider.to_string(), *status))]
    ProviderRefused {
        provider: MusicProvider,
        status: u16,
    },
    #[error("{}", words::errors::account_refused(*code, provider.to_string()))]
    AccountRefused { provider: MusicProvider, code: u16 },
    #[error("{}", words::errors::provider_untrusted(provider.to_string()))]
    ProviderUntrusted { provider: MusicProvider },
    #[error("{}", words::errors::provider_misaddressed(provider.to_string()))]
    ProviderMisaddressed { provider: MusicProvider },
    #[error("{}", words::errors::provider_unreadable(provider.to_string()))]
    ProviderUnreadable { provider: MusicProvider },
    #[error("{}", words::errors::sign_in_lapsed(provider.to_string()))]
    SignInLapsed { provider: MusicProvider },
    #[error("{}", words::errors::sign_in_denied(provider.to_string()))]
    SignInDenied { provider: MusicProvider },
    #[error("{}", words::errors::download_io(op.to_string()))]
    DownloadIo {
        op: DownloadOp,
        #[source]
        source: io::Error,
    },
    #[error("{}", words::errors::download_cancelled())]
    DownloadCancelled,
    #[error("{}", words::errors::download_expired())]
    DownloadExpired,
    #[error("{}", words::errors::quick_connect_off())]
    QuickConnectOff,
    #[error("{}", words::errors::sign_in_cancelled())]
    SignInCancelled,
    #[error("{}", words::errors::unknown_download())]
    UnknownDownload,
    #[error("{}", words::errors::download_too_large())]
    DownloadTooLarge,
    #[error("{}", words::errors::output_unavailable())]
    OutputUnavailable,
    #[error("{}", words::errors::renderers_unreachable())]
    RenderersUnreachable,
    #[error("{}", words::errors::renderer_search())]
    RendererSearch {
        #[source]
        source: Box<resonate_upnp::Error>,
    },
    #[error("{}", words::errors::renderer_address())]
    RendererAddress {
        #[source]
        source: Box<resonate_upnp::Error>,
    },
    #[error("{}", words::errors::receiver_search())]
    ReceiverSearch {
        #[source]
        source: Box<resonate_cast::Error>,
    },
    #[error("{}", words::errors::receiver_address())]
    ReceiverAddress,
    #[error("{}", words::errors::receiver_unreached())]
    ReceiverUnreached {
        #[source]
        source: Box<resonate_cast::Error>,
    },
    #[error("{}", words::errors::unknown_renderer())]
    UnknownRenderer,
    #[error("{}", words::errors::invalid_descriptor())]
    InvalidDescriptor,
    #[error("{}", words::errors::wrong_song())]
    WrongSong,
    #[error("{}", crate::failure::of_a_lookup(*op, source))]
    Reference {
        op: resonate_library::LookupOp,
        #[source]
        source: Box<resonate_library::Error>,
    },
    #[error("{}", words::errors::equaliser())]
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
    #[error("{}", words::errors::unknown_target(name))]
    UnknownTarget { name: String },
    #[error("{}", words::errors::unknown_account())]
    UnknownAccount,
    #[error("{}", words::errors::analysis())]
    Analysis(#[from] resonate_analysis::Error),
    #[error("{}", words::errors::lyrics())]
    Lyrics(#[from] resonate_lyrics::Error),
    #[error("{}", words::errors::lyrics_too_large(*bytes))]
    LyricsTooLarge { bytes: u64 },
    #[error("{}", words::errors::clip_too_long(*samples))]
    ClipTooLong { samples: u64 },
    #[error("{}", words::errors::no_local_matches())]
    NoLocalMatches,
    #[error("{}", words::errors::not_a_playlist_link())]
    NotAPlaylistLink,
    #[error("{}", words::errors::no_playlist_link())]
    NoPlaylistLink,
    #[error("{}", words::errors::not_a_profile_link())]
    NotAProfileLink,
    #[error("{}", words::errors::no_profile_at_the_link())]
    NoProfileAtTheLink,
    #[error("{}", words::errors::not_signed_in_to_spotify())]
    NotSignedInToSpotify,
    #[error("{}", words::errors::no_spotify_client())]
    NoSpotifyClient,
    #[error("{}", words::errors::spotify_sign_in_lapsed())]
    SpotifySignInLapsed,
    #[error("{}", words::errors::spotify_sign_in(failure.to_string()))]
    SpotifySignIn {
        failure: resonate_library::SpotifySignInFailure,
    },
    #[error("{}", words::errors::unreadable_playlist_link())]
    UnreadablePlaylistLink,
    #[error("{}", words::errors::playlist_file_too_large(*bytes))]
    PlaylistFileTooLarge { bytes: u64 },
    #[error("{}", words::errors::playlist_file_empty())]
    PlaylistFileEmpty,
    #[error("{}", words::errors::playlist_file_unwritten())]
    PlaylistFileUnwritten {
        #[source]
        source: Box<resonate_library::Error>,
    },
    #[error("{}", words::errors::invalid_wanted_song())]
    InvalidWantedSong,
    #[error("{}", words::errors::online_off())]
    OnlineOff,
    #[error("{}", words::errors::no_served_server())]
    NoServedServer,
    #[error("{}", words::errors::heard(.0.to_string()))]
    Heard(#[from] resonate_listen::Error),
    #[error("{}", words::errors::invalid_playlist_name())]
    InvalidPlaylistName,
    #[error("{}", words::errors::duplicate_playlist_name())]
    DuplicatePlaylistName,
    #[error("{}", words::errors::unknown_playlist())]
    UnknownPlaylist,
    #[error("{}", words::errors::playlist_changed())]
    PlaylistChanged,
    #[error("{}", words::errors::playlist_followed())]
    PlaylistFollowed,
    #[error("{}", words::errors::not_a_list())]
    NotAList,
    #[error("{}", words::errors::not_a_saved_search())]
    NotASavedSearch,
    #[error("{}", words::errors::invalid_picture())]
    InvalidPicture,
    #[error("{}", words::errors::invalid_uri())]
    InvalidUri,
    #[error("{}", words::errors::catalog_version(*found))]
    CatalogVersion { found: u32 },
    #[error("{}", words::errors::unknown_track())]
    UnknownTrack,
    #[error("{}", words::errors::too_many_bands(*found, MAX_BANDS))]
    TooManyBands { found: u32 },
    #[error("{}", words::errors::band_frequency(band + 1, hertz.to_string(), (HIGHEST_HZ / 1000.0).to_string(), LOWEST_HZ.to_string()))]
    BandFrequency { band: u32, hertz: f32 },
    #[error("{}", words::errors::band_gain(band + 1, decibels.to_string(), GAIN_AT_MOST_DB.to_string()))]
    BandGain { band: u32, decibels: f32 },
    #[error("{}", words::errors::band_q(band + 1, Q_AT_LEAST.to_string(), Q_AT_MOST.to_string(), q.to_string()))]
    BandQ { band: u32, q: f32 },
    #[error("{}", words::errors::band_channels(band + 1))]
    BandChannels { band: u32 },
    #[error("{}", words::errors::preamp_range(decibels.to_string(), PREAMP_AT_MOST_DB.to_string()))]
    PreampRange { decibels: f32 },
    #[error("{}", words::errors::listening_test_trials(TRIALS_AT_MOST, *trials))]
    ListeningTestTrials { trials: u32 },
    #[error("{}", words::errors::listening_test_nothing_to_test())]
    ListeningTestNothingToTest,
    #[error("{}", words::errors::listening_test_levels(*millibels))]
    ListeningTestLevels { millibels: i32 },
    #[error("{}", words::errors::listening_test_finished())]
    ListeningTestFinished,
    #[error("{}", words::errors::buffer_range(*milliseconds))]
    BufferRange { milliseconds: u32 },
    #[error("{}", words::errors::closed())]
    Closed,
    #[error("{}", words::errors::no_zone(*zone))]
    NoZone { zone: u32 },
    #[error("{}", words::errors::zone_unnamed())]
    ZoneUnnamed,
    #[error("{}", words::errors::first_zone_stays())]
    FirstZoneStays,
    #[error("{}", words::errors::unknown_queue_row(*queue_id))]
    UnknownQueueRow { queue_id: u64 },
    #[error("{}", words::errors::queue_row_out_of_range(*row, *rows))]
    QueueRowOutOfRange { row: u32, rows: u64 },
    #[error("{}", words::errors::sync(op.to_string()))]
    Sync {
        op: SyncOp,
        #[source]
        source: Box<resonate_sync::Error>,
    },
    #[error("{}", words::errors::sync_off())]
    SyncOff,
    #[error("{}", words::errors::unknown_sync_device())]
    UnknownSyncDevice,
    #[error("{}", words::errors::nothing_handed_off())]
    NothingHandedOff,
    #[error("{}", words::errors::unknown_received_song())]
    UnknownReceivedSong,
    #[error("{}", words::errors::received(op.to_string()))]
    Received {
        op: ReceivedOp,
        #[source]
        source: io::Error,
    },
    #[error("{}", words::errors::layout_unreadable(source.to_string()))]
    LayoutUnreadable {
        #[source]
        source: Box<resonate_library::Error>,
    },
    #[error("{}", words::errors::tags_unwritable())]
    TagsUnwritable,
    #[error("{}", words::errors::tags_unconfirmed())]
    TagsUnconfirmed,
    #[error("{}", words::errors::tags_document(op.to_string()))]
    TagsDocument {
        op: RewriteOp,
        #[source]
        source: io::Error,
    },
    #[error("{}", words::errors::tags_unrestored(kept.display().to_string()))]
    TagsUnrestored { kept: PathBuf },
    #[error("{}", words::errors::tag_number())]
    TagNumber,
    #[error("{}", words::errors::impulse_extension())]
    ImpulseExtension,
    #[error("{}", words::errors::impulse_too_large())]
    ImpulseTooLarge,
    #[error("{}", words::errors::impulse_not_staged())]
    ImpulseNotStaged,
    #[error("{}", words::errors::no_impulse())]
    NoImpulse,
    #[error("{}", words::errors::impulse_pair(source.to_string()))]
    ImpulsePair {
        #[source]
        source: Box<resonate_engine::PairedReadError>,
    },
    #[error("{}", words::errors::impulse_file(op.to_string()))]
    ImpulseFile {
        op: crate::room::ImpulseOp,
        #[source]
        source: io::Error,
    },
    #[error("{}", words::errors::folder())]
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

    #[test]
    fn a_figure_reads_as_it_was_given_and_a_count_takes_its_form() {
        let frequency = Error::BandFrequency {
            band: 2,
            hertz: 25_000.5,
        };
        let q = Error::BandQ { band: 0, q: 0.05 };
        let apart = |millibels| Error::ListeningTestLevels { millibels }.to_string();
        let provider = Error::ProviderRefused {
            provider: MusicProvider::Subsonic,
            status: 503,
        };

        assert_eq!(
            frequency.to_string(),
            "Band 3 must sit between 20 Hz and 20 kHz, not 25000.5 Hz"
        );
        assert_eq!(
            q.to_string(),
            "Band 1's Q must lie between 0.1 and 40, not 0.05"
        );
        assert_eq!(
            apart(1250),
            "The curve cannot be level-matched to its bypass, 1250 thousandths of a decibel apart, so it is not tested"
        );
        assert_eq!(
            apart(1),
            "The curve cannot be level-matched to its bypass, 1 thousandth of a decibel apart, so it is not tested"
        );
        assert_eq!(
            provider.to_string(),
            "The Subsonic server refused the request (503)"
        );
        assert_eq!(
            Error::TooManyBands { found: 40 }.to_string(),
            "An equaliser holds at most 32 bands, not 40"
        );
    }
}
