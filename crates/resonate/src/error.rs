use std::{fmt, io, path::PathBuf, result};

use resonate_core::MediaLocation;
use resonate_engine::NodeName;
use resonate_library::{PassKind, PlaylistName};
use resonate_mpris::{PlayerName, ZoneAsked};
use thiserror::Error;

use crate::{cli::AbxOfArg, sleep::Spoken, suggest::SuggestionName};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConfigKey {
    Sink,
    Library,
    Vault,
    Quality,
    FilterPhase,
    TruePeak,
    RestoreLossy,
    Crossfeed,
    Dither,
    NoiseShaping,
    ReplayGain,
    ReplayGainPreAmp,
    ReplayGainUntagged,
    BitPerfect,
    Dop,
    DsdLikePcm,
    DeviceVolume,
    ForceGraphRate,
    BluetoothWake,
    BluetoothLeadMs,
    BluetoothAwakeS,
    Volume,
    BufferMs,
    Theme,
    Accent,
    TextSize,
    Language,
    FollowDesktopScheme,
    FollowDesktopTextSize,
    Online,
    EnrichAfterScan,
    EnrichOnMetered,
    MusicbrainzMirror,
    StallDownloadsOnMetered,
    Study,
    FetchLyrics,
    LyricsLanguageFromLocale,
    IdentifyBySound,
    Contact,
    Equaliser,
    EqualiserFor,
    EqualiserMeasurer,
    EqualiserMeasurerFor,
    EqualiserProfile,
    Convolution,
    SpeakerAngle,
    SpeakerElevation,
    HeadYaw,
    Resume,
    Renderers,
    FlacAbove96kFor,
    RenderersAt,
    CastAt,
    HistoryKept,
    SkipRepeatsQueue,
    PreviousRestarts,
    ShuffleStyle,
    ShuffleSpreadsArtists,
    ShuffleSpreadsAlbums,
    ShuffleFreshFirst,
    OrganiseAs,
    Notify,
    CheckForUpdates,
    UpdateDismissed,
    MinimiseButton,
    MaximiseButton,
    ScrollVolume,
    MouseNavigation,
    ReducedMotion,
    Scrollbars,
    SpectrumTilt,
    SpectrumFloor,
    SpectrumBands,
    SpectrumFalls,
    SuggestionsTab,
    MissingTab,
    TabCounts,
    RememberTab,
    LastTab,
    ArtistsDrawn,
    RememberWindowSize,
    WindowSize,
    RememberSettingsCategory,
    LastSettingsCategory,
    Inbox,
    MusicFolder,
    MusicExtensions,
    MinimumLength,
    FileDropped,
    Subsonic,
    SubsonicUser,
    SubsonicPassword,
    Jellyfin,
    JellyfinUser,
    JellyfinPassword,
    JellyfinToken,
    JellyfinUserId,
    JellyfinDevice,
    TidalClientId,
    TidalClientSecret,
    TidalRefreshToken,
    SpotifyClientId,
    SpotifyRefreshToken,
    HifiApi,
    HifiSecret,
    Monochrome,
    HostedServices,
    YoutubeMusic,
    YoutubeMusicCookies,
    YtDlp,
    YoutubeMusicKeepOrder,
    AcoustidKey,
    AuddToken,
    ListenbrainzToken,
    LastfmKey,
    LastfmSecret,
    LastfmSession,
    ListenFrom,
    ListenFor,
    Discord,
    DiscordApp,
    DiscordShows,
    DiscordArt,
    DiscordIcon,
    DiscordProgress,
    DiscordPaused,
    Sync,
    SyncNearbyOnly,
    SyncServer,
    SyncServerCertificate,
    SyncStun,
    SyncKey,
    SyncDevice,
    SharedName,
    SyncSongs,
    SyncControl,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ArtistName(Box<str>);

impl ArtistName {
    pub fn new(name: &str) -> Self {
        Self(name.into())
    }
}

impl fmt::Display for ArtistName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Given(Box<str>);

impl Given {
    pub fn new(text: &str) -> Self {
        Self(text.into())
    }
}

impl fmt::Display for Given {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Named {
    Track,
    Album,
    MissingTrack,
    WantedTrack,
    UnheldRelease,
}

impl fmt::Display for Named {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Track => "track",
            Self::Album => "album",
            Self::MissingTrack => "missing track",
            Self::WantedTrack => "wanted track",
            Self::UnheldRelease => "release not held",
        })
    }
}

impl ConfigKey {
    pub const ALL: [Self; 136] = [
        Self::Sink,
        Self::Library,
        Self::Vault,
        Self::Quality,
        Self::FilterPhase,
        Self::TruePeak,
        Self::RestoreLossy,
        Self::Crossfeed,
        Self::Dither,
        Self::NoiseShaping,
        Self::ReplayGain,
        Self::ReplayGainPreAmp,
        Self::ReplayGainUntagged,
        Self::BitPerfect,
        Self::Dop,
        Self::DsdLikePcm,
        Self::DeviceVolume,
        Self::ForceGraphRate,
        Self::BluetoothWake,
        Self::BluetoothLeadMs,
        Self::BluetoothAwakeS,
        Self::Volume,
        Self::BufferMs,
        Self::Theme,
        Self::Accent,
        Self::TextSize,
        Self::Language,
        Self::FollowDesktopScheme,
        Self::FollowDesktopTextSize,
        Self::Online,
        Self::EnrichAfterScan,
        Self::EnrichOnMetered,
        Self::MusicbrainzMirror,
        Self::StallDownloadsOnMetered,
        Self::Study,
        Self::FetchLyrics,
        Self::LyricsLanguageFromLocale,
        Self::IdentifyBySound,
        Self::Contact,
        Self::Equaliser,
        Self::EqualiserFor,
        Self::EqualiserMeasurer,
        Self::EqualiserMeasurerFor,
        Self::EqualiserProfile,
        Self::Convolution,
        Self::SpeakerAngle,
        Self::SpeakerElevation,
        Self::HeadYaw,
        Self::Resume,
        Self::Renderers,
        Self::FlacAbove96kFor,
        Self::RenderersAt,
        Self::CastAt,
        Self::HistoryKept,
        Self::SkipRepeatsQueue,
        Self::PreviousRestarts,
        Self::ShuffleStyle,
        Self::ShuffleSpreadsArtists,
        Self::ShuffleSpreadsAlbums,
        Self::ShuffleFreshFirst,
        Self::OrganiseAs,
        Self::Notify,
        Self::CheckForUpdates,
        Self::UpdateDismissed,
        Self::MinimiseButton,
        Self::MaximiseButton,
        Self::ScrollVolume,
        Self::MouseNavigation,
        Self::ReducedMotion,
        Self::Scrollbars,
        Self::SpectrumTilt,
        Self::SpectrumFloor,
        Self::SpectrumBands,
        Self::SpectrumFalls,
        Self::SuggestionsTab,
        Self::MissingTab,
        Self::TabCounts,
        Self::RememberTab,
        Self::LastTab,
        Self::ArtistsDrawn,
        Self::RememberWindowSize,
        Self::WindowSize,
        Self::RememberSettingsCategory,
        Self::LastSettingsCategory,
        Self::Inbox,
        Self::MusicFolder,
        Self::MusicExtensions,
        Self::MinimumLength,
        Self::FileDropped,
        Self::Subsonic,
        Self::SubsonicUser,
        Self::SubsonicPassword,
        Self::Jellyfin,
        Self::JellyfinUser,
        Self::JellyfinPassword,
        Self::JellyfinToken,
        Self::JellyfinUserId,
        Self::JellyfinDevice,
        Self::TidalClientId,
        Self::TidalClientSecret,
        Self::TidalRefreshToken,
        Self::SpotifyClientId,
        Self::SpotifyRefreshToken,
        Self::HifiApi,
        Self::HifiSecret,
        Self::Monochrome,
        Self::HostedServices,
        Self::YoutubeMusic,
        Self::YoutubeMusicCookies,
        Self::YtDlp,
        Self::YoutubeMusicKeepOrder,
        Self::AcoustidKey,
        Self::AuddToken,
        Self::ListenbrainzToken,
        Self::LastfmKey,
        Self::LastfmSecret,
        Self::LastfmSession,
        Self::ListenFrom,
        Self::ListenFor,
        Self::Discord,
        Self::DiscordApp,
        Self::DiscordShows,
        Self::DiscordArt,
        Self::DiscordIcon,
        Self::DiscordProgress,
        Self::DiscordPaused,
        Self::Sync,
        Self::SyncNearbyOnly,
        Self::SyncServer,
        Self::SyncServerCertificate,
        Self::SyncStun,
        Self::SyncKey,
        Self::SyncDevice,
        Self::SharedName,
        Self::SyncSongs,
        Self::SyncControl,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sink => "sink",
            Self::Library => "library",
            Self::Vault => "vault",
            Self::Quality => "quality",
            Self::FilterPhase => "filter-phase",
            Self::TruePeak => "true-peak",
            Self::RestoreLossy => "restore-lossy",
            Self::Crossfeed => "crossfeed",
            Self::Dither => "dither",
            Self::NoiseShaping => "noise-shaping",
            Self::ReplayGain => "replay-gain",
            Self::ReplayGainPreAmp => "replay-gain-pre-amp",
            Self::ReplayGainUntagged => "replay-gain-untagged",
            Self::BitPerfect => "bit-perfect",
            Self::Dop => "dop",
            Self::DsdLikePcm => "dsd-like-pcm",
            Self::DeviceVolume => "device-volume",
            Self::ForceGraphRate => "force-graph-rate",
            Self::BluetoothWake => "bluetooth-wake",
            Self::BluetoothLeadMs => "bluetooth-lead-ms",
            Self::BluetoothAwakeS => "bluetooth-awake-s",
            Self::Volume => "volume",
            Self::BufferMs => "buffer-ms",
            Self::Theme => "theme",
            Self::Accent => "accent",
            Self::TextSize => "text-size",
            Self::Language => "language",
            Self::FollowDesktopScheme => "follow-desktop-scheme",
            Self::FollowDesktopTextSize => "follow-desktop-text-size",
            Self::Online => "online",
            Self::EnrichAfterScan => "enrich-after-scan",
            Self::EnrichOnMetered => "enrich-on-metered",
            Self::MusicbrainzMirror => "musicbrainz-mirror",
            Self::StallDownloadsOnMetered => "stall-downloads-on-metered",
            Self::Study => "study",
            Self::FetchLyrics => "fetch-lyrics",
            Self::LyricsLanguageFromLocale => "lyrics-language-from-locale",
            Self::IdentifyBySound => "identify-by-sound",
            Self::SkipRepeatsQueue => "skip-repeats-queue",
            Self::PreviousRestarts => "previous-restarts",
            Self::ShuffleStyle => "shuffle-style",
            Self::ShuffleSpreadsArtists => "shuffle-spreads-artists",
            Self::ShuffleSpreadsAlbums => "shuffle-spreads-albums",
            Self::ShuffleFreshFirst => "shuffle-fresh-first",
            Self::Contact => "contact",
            Self::Equaliser => "equaliser",
            Self::EqualiserFor => "equaliser-for",
            Self::EqualiserMeasurer => "equaliser-measurer",
            Self::EqualiserMeasurerFor => "equaliser-measurer-for",
            Self::EqualiserProfile => "equaliser-profile",
            Self::Convolution => "convolution",
            Self::SpeakerAngle => "speaker-angle",
            Self::SpeakerElevation => "speaker-elevation",
            Self::HeadYaw => "head-yaw",
            Self::Resume => "resume",
            Self::Renderers => "renderers",
            Self::FlacAbove96kFor => "flac-above-96k-for",
            Self::RenderersAt => "renderers-at",
            Self::CastAt => "cast-at",
            Self::HistoryKept => "history-kept",
            Self::OrganiseAs => "organise-as",
            Self::Notify => "notify",
            Self::CheckForUpdates => "check-for-updates",
            Self::UpdateDismissed => "update-dismissed",
            Self::MinimiseButton => "minimise-button",
            Self::MaximiseButton => "maximise-button",
            Self::ScrollVolume => "scroll-volume",
            Self::MouseNavigation => "mouse-navigation",
            Self::ReducedMotion => "reduced-motion",
            Self::Scrollbars => "scrollbars",
            Self::SpectrumTilt => "spectrum-tilt",
            Self::SpectrumFloor => "spectrum-floor",
            Self::SpectrumBands => "spectrum-bands",
            Self::SpectrumFalls => "spectrum-falls",
            Self::SuggestionsTab => "suggestions-tab",
            Self::MissingTab => "missing-tab",
            Self::TabCounts => "tab-counts",
            Self::RememberTab => "remember-tab",
            Self::LastTab => "last-tab",
            Self::ArtistsDrawn => "artists-drawn",
            Self::RememberWindowSize => "remember-window-size",
            Self::WindowSize => "window-size",
            Self::RememberSettingsCategory => "remember-settings-category",
            Self::LastSettingsCategory => "last-settings-category",
            Self::Inbox => "inbox",
            Self::MusicExtensions => "music-extensions",
            Self::MinimumLength => "minimum-length-s",
            Self::MusicFolder => "music-folder",
            Self::FileDropped => "file-dropped",
            Self::Subsonic => "subsonic",
            Self::SubsonicUser => "subsonic-user",
            Self::SubsonicPassword => "subsonic-password",
            Self::Jellyfin => "jellyfin",
            Self::JellyfinUser => "jellyfin-user",
            Self::JellyfinPassword => "jellyfin-password",
            Self::JellyfinToken => "jellyfin-token",
            Self::JellyfinUserId => "jellyfin-user-id",
            Self::JellyfinDevice => "jellyfin-device",
            Self::TidalClientId => "tidal-client-id",
            Self::TidalClientSecret => "tidal-client-secret",
            Self::TidalRefreshToken => "tidal-refresh-token",
            Self::SpotifyClientId => "spotify-client-id",
            Self::SpotifyRefreshToken => "spotify-refresh-token",
            Self::HifiApi => "hifi-api",
            Self::HifiSecret => "hifi-secret",
            Self::Monochrome => "monochrome",
            Self::HostedServices => "hosted-services",
            Self::YoutubeMusic => "youtube-music",
            Self::YoutubeMusicCookies => "youtube-music-cookies",
            Self::YtDlp => "yt-dlp",
            Self::YoutubeMusicKeepOrder => "youtube-music-keep-order",
            Self::AcoustidKey => "acoustid-key",
            Self::AuddToken => "audd-token",
            Self::ListenbrainzToken => "listenbrainz-token",
            Self::LastfmKey => "lastfm-key",
            Self::LastfmSecret => "lastfm-secret",
            Self::LastfmSession => "lastfm-session",
            Self::ListenFrom => "listen-from",
            Self::ListenFor => "listen-for",
            Self::Discord => "discord",
            Self::DiscordApp => "discord-app",
            Self::DiscordShows => "discord-shows",
            Self::DiscordArt => "discord-art",
            Self::DiscordIcon => "discord-icon",
            Self::DiscordProgress => "discord-progress",
            Self::DiscordPaused => "discord-paused",
            Self::Sync => "sync",
            Self::SyncNearbyOnly => "sync-nearby-only",
            Self::SyncServer => "sync-server",
            Self::SyncServerCertificate => "sync-server-certificate",
            Self::SyncStun => "sync-stun",
            Self::SyncKey => "sync-key",
            Self::SyncDevice => "sync-device",
            Self::SharedName => "shared-name",
            Self::SyncSongs => "sync-songs",
            Self::SyncControl => "sync-control",
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|candidate| candidate.as_str() == key)
    }
}

impl fmt::Display for ConfigKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ValueKind {
    Text,
    Boolean,
    Integer,
    Number,
    Table,
    Array,
}

impl fmt::Display for ValueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Text => "string",
            Self::Boolean => "boolean",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::Table => "table",
            Self::Array => "array",
        })
    }
}

#[cfg(all(feature = "ui", unix, not(target_os = "macos")))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IconOp {
    Read,
    Stage,
    Place,
}

#[cfg(feature = "online")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SignOutOp {
    Jar,
    Profile,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("this user has no XDG data directory")]
    NoDataDir,

    #[error("this user has no XDG configuration directory")]
    NoConfigDir,

    #[error("cannot read {path}", path = path.display())]
    ReadConfig {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("{path} is not valid TOML", path = path.display())]
    ConfigSyntax {
        path: PathBuf,
        #[source]
        source: Box<toml_edit::TomlError>,
    },

    #[error("cannot write {path}", path = path.display())]
    WriteConfig {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("{key} in {path} must be a {expected}", path = path.display())]
    ConfigType {
        path: PathBuf,
        key: ConfigKey,
        expected: ValueKind,
    },

    #[error("{key} in {path} is not one of the settings it accepts", path = path.display())]
    ConfigValue { path: PathBuf, key: ConfigKey },

    #[error(
        "TIDAL rotated the refresh token and it could not be written to {path}; sign in again \
         with `resonate tidal` once it can",
        path = path.display()
    )]
    #[cfg(feature = "online")]
    TidalTokenNotKept {
        path: PathBuf,
        #[source]
        source: Box<Error>,
    },

    #[error(
        "TIDAL rotated the refresh token and there is no settings file to write it to; sign in \
         again with `resonate tidal`"
    )]
    #[cfg(feature = "online")]
    TidalTokenHasNoSettings,

    #[error("cannot create {path}", path = path.display())]
    CreateDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[cfg(unix)]
    #[error("cannot keep {path} to its owner", path = path.display())]
    Private {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[cfg(all(feature = "ui", unix, not(target_os = "macos")))]
    #[error("{op:?} failed on the application's icon at {path}", path = path.display())]
    Icon {
        op: IconOp,
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("library root {path} does not exist", path = path.display())]
    MissingLibraryRoot { path: PathBuf },

    #[error("cannot resolve {path}", path = path.display())]
    UnresolvedFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("nothing to play; name a file, a file:// URI or a cue sheet")]
    NoFileNamed,

    #[error("nothing named is a file, URI or cue track that plays")]
    NothingPlayableNamed,

    #[error("the listening test loops one song, and {named} were named")]
    AbxSeveralSongs { named: usize },

    #[error("the output did not open in time, so there is no rate to level-match the arms at")]
    AbxNoOutput,

    #[error(
        "no curve is bound to {sink} with the equaliser on, so there is no chain to test; \
         resonate eq binds one"
    )]
    AbxNothingBound { sink: NodeName },

    #[error("an --against flag names an arm of another test than the {testing:?} one asked for")]
    AbxAgainstElsewhere { testing: AbxOfArg },

    #[error("none of the {failed} tracks queued could be played")]
    NothingPlayed { failed: usize },

    #[error("nothing has been played yet, so there is no year to review")]
    NothingHeardYet,

    #[cfg(feature = "sync")]
    #[error("another Resonate holds the shared playlists; do this from there")]
    SharedBusy,

    #[cfg(feature = "sync")]
    #[error("a party needs a server, and this device is set to use none")]
    PartyNeedsAServer,

    #[cfg(feature = "sync")]
    #[error("the party could not go on: {0}")]
    Party(#[source] resonate_sync::Error),

    #[cfg(feature = "sync")]
    #[error("{0} is not shared with anyone")]
    PlaylistNotShared(Box<str>),

    #[cfg(feature = "sync")]
    #[error("nobody called {0} is on that playlist")]
    NoSuchPersonOnTheList(Box<str>),

    #[cfg(feature = "sync")]
    #[error("say which playlist to remove them from with --from")]
    SharedNeedsAList,

    #[error("no player is on the session bus; start one with resonate play or open the window")]
    NothingRunning,

    #[error("no player named {name} is on the session bus; resonate players lists what is")]
    NoSuchPlayer { name: PlayerName },

    #[error("no zone {zone} is on the session bus; resonate players lists the zones running")]
    NoSuchZone { zone: ZoneAsked },

    #[error("{address} is not an http address a renderer can be reached at")]
    RendererAddress {
        address: String,
        #[source]
        source: Box<resonate_upnp::Error>,
    },

    #[error("{address} is neither an IP address nor a host name a Cast receiver can be reached at")]
    CastAddress { address: String },

    #[error("no playlist named {0}")]
    NoSuchPlaylist(PlaylistName),

    #[error("the catalog holds no artist named {0}")]
    NoSuchArtist(ArtistName),

    #[error("the catalog holds no {kind} named {given}")]
    NothingNamed { kind: Named, given: Given },

    #[error("{given} names {held} rows of the catalog; --by names the artist whose it is")]
    SeveralNamed { given: Given, held: usize },

    #[error("--add and --remove take one of --tracks, --albums or --artists to say what they name")]
    MarkNeedsOneKind,

    #[error("nothing is playing on the session bus, so there is nothing to share")]
    NothingPlaying,

    #[error("the catalog holds no track at {uri}", uri = location.to_uri())]
    NotInTheCatalog { location: MediaLocation },

    #[error("the catalog holds no link for that track")]
    NothingToShare,

    #[error("nothing the audio at {uri} was heard as is held; a lookup with an acoustid-key \
             recognises it", uri = location.to_uri())]
    NothingHeardAs { location: MediaLocation },

    #[error("{sheet} is a cue sheet; name one of its tracks with --track", sheet = sheet.display())]
    SheetWithoutATrack { sheet: PathBuf },

    #[error("{sheet} holds no audio track numbered {track}", sheet = sheet.display())]
    NoSuchSheetTrack { sheet: PathBuf, track: u32 },

    #[error("--track names a track of a .cue sheet, and {uri} is not one", uri = location.to_uri())]
    TrackOutsideASheet { location: MediaLocation },

    #[error("the frames named after {uri} are not a span of it", uri = location.to_uri())]
    UnreadableSpan { location: MediaLocation },

    #[error("nothing is suggested under the name {0}; resonate suggest lists what is")]
    NoSuchSuggestion(SuggestionName),

    #[error(
        "{given} is not a sleep timer; give it minutes, a length such as 1h30m or 1:30:00, track, queue or off"
    )]
    NotASleepTimer { given: Spoken },

    #[error("--as names one playlist, and {given} files were given")]
    NameForOnePlaylistOnly { given: usize },

    #[error("{file} is a playlist file, and only a playlist read from a link is kept in step with one", file = file.display())]
    KeptInStepOnlyWithALink { file: PathBuf },

    #[error("the link names no playlist the service shows")]
    NoPlaylistAtTheLink,

    #[error("the profile is private, or the service shows no playlist of it")]
    NoProfileAtTheLink,

    #[error("no sink named {0} is present in the graph")]
    SinkNotFound(NodeName),

    #[error("--bit-perfect and --no-bit-perfect ask for opposite things; name one")]
    BitPerfectBothWays,

    #[error("the {} was cancelled; what it finished before is kept", pass_named(*pass))]
    Cancelled { pass: PassKind },

    #[error("the {} finished, but {failed} of its files failed", pass_named(*pass))]
    FilesFailed { pass: PassKind, failed: u64 },

    #[error(
        "{moved} vault objects did not read back as what went in and {unreached} could not be read at all"
    )]
    ObjectsUnverified { moved: u64, unreached: u64 },

    #[error("a tracing subscriber was already installed")]
    LoggingAlreadyInstalled,

    #[cfg(not(feature = "online"))]
    #[error("this build has no online reference; it was built without the online feature")]
    NoReference,

    #[cfg(feature = "online")]
    #[error("online is off in the settings, so there is no reference to ask")]
    OnlineOff,

    #[cfg(feature = "online")]
    #[error("the copy of MusicBrainz could not be built or read")]
    Musicbrainz(#[source] Box<resonate_musicbrainz::Error>),

    #[cfg(feature = "online")]
    #[error("the copy of MusicBrainz at {path} could not be removed")]
    RemoveMirror {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[cfg(not(feature = "online"))]
    #[error("this build keeps no copy of MusicBrainz; it was built without the online feature")]
    NoMirror,

    #[cfg(not(feature = "online"))]
    #[error("this build asks about no release; it was built without the online feature")]
    NoUpdates,

    #[cfg(feature = "online")]
    #[error("this build names no repository it was released from, so it has nowhere to ask")]
    NoReleasesNamed,

    #[cfg(feature = "online")]
    #[error("the latest release could not be asked for")]
    LatestRelease(#[source] Box<resonate_online::Error>),

    #[cfg(feature = "online")]
    #[error("what was last heard of the releases could not be kept at {path}")]
    KeepLatestRelease {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[cfg(not(feature = "online"))]
    #[error("this build has no TIDAL sign-in; it was built without the online feature")]
    NoSignIn,

    #[cfg(not(feature = "online"))]
    #[error("this build plays no service in place; it was built without the online feature")]
    NoServed,

    #[cfg(feature = "online")]
    #[error(
        "subsonic, subsonic-user and subsonic-password are not all set in the settings, so there \
         is no Subsonic server to browse"
    )]
    NoSubsonicServer,

    #[cfg(feature = "online")]
    #[error(
        "no service is set in the settings to play in place (subsonic, jellyfin, tidal-refresh-token, \
         hifi-secret, hifi-api, monochrome, hosted-services or youtube-music)"
    )]
    NoServedService,

    #[cfg(feature = "online")]
    #[error(
        "{service} is not a service the settings set to play in place; resonate served lists \
         the first that is"
    )]
    ServiceNotServed { service: String },

    #[cfg(feature = "online")]
    #[error("an id was asked for but none was given")]
    BlankServerId,

    #[cfg(feature = "online")]
    #[error("youtube-music is off in the settings, so nothing was asked of YouTube Music")]
    YoutubeMusicOff,

    #[cfg(feature = "online")]
    #[error(
        "resonate-signin is not installed beside resonate or on the path; install it to sign in \
         to YouTube Music here, or name a browser or a cookies file in youtube-music-cookies"
    )]
    NoSignInProgram,

    #[cfg(feature = "online")]
    #[error("the sign-in window was closed before signing in, so nothing was kept")]
    YoutubeMusicSignInClosed,

    #[cfg(feature = "online")]
    #[error("the sign-in window stopped without signing in (exit code {code:?})")]
    YoutubeMusicSignInFailed { code: Option<i32> },

    #[cfg(feature = "online")]
    #[error("the sign-in window could not be started")]
    YoutubeMusicSignInNotStarted {
        #[source]
        source: io::Error,
    },

    #[cfg(feature = "online")]
    #[error("the data folder's path is not text, so youtube-music-cookies cannot name the jar")]
    YoutubeMusicJarNotText,

    #[cfg(feature = "online")]
    #[error("signing out of YouTube Music could not take away the {op:?}")]
    YoutubeMusicSignOut {
        op: SignOutOp,
        #[source]
        source: io::Error,
    },

    #[cfg(feature = "online")]
    #[error("{link} is not a link to a song or a playlist on YouTube Music")]
    NotAYoutubeMusicLink { link: String },

    #[cfg(feature = "online")]
    #[error("YouTube Music shows no video {video}; it may be private or gone")]
    NoSuchVideo { video: String },

    #[cfg(feature = "online")]
    #[error(
        "another pass is asking the providers, so nothing was fetched now; what was asked for \
         stays queued for the next keep, here or when the window next opens"
    )]
    KeepWaitsItsTurn,

    #[cfg(feature = "online")]
    #[error("the keep was cancelled; what landed stays and the rest stays queued")]
    KeepCancelled,

    #[cfg(feature = "online")]
    #[error("{provider} stopped answering at {song:?}, so {left} kept songs stay queued")]
    KeepStopped {
        provider: resonate_core::SourceId,
        song: String,
        left: u64,
    },

    #[cfg(feature = "online")]
    #[error(
        "the cookies lapsed at {song:?}: sign in to YouTube again and run this once more; \
         {left} kept songs stay queued"
    )]
    KeepSignInLapsed { song: String, left: u64 },

    #[cfg(feature = "online")]
    #[error("this account has lost Premium at {song:?}, so {left} kept songs stay queued")]
    KeepPremiumLost { song: String, left: u64 },

    #[cfg(feature = "online")]
    #[error(
        "yt-dlp could not be run at {song:?}: install it, or name it in the yt-dlp setting; \
         {left} kept songs stay queued"
    )]
    KeepToolMissing { song: String, left: u64 },

    #[cfg(feature = "online")]
    #[error(
        "tidal-client-id is not set in the settings, and TIDAL's sign-in needs the client it names"
    )]
    NoTidalClient,

    #[cfg(feature = "online")]
    #[error("the TIDAL sign-in was cancelled before it was approved")]
    SignInCancelled,

    #[cfg(feature = "online")]
    #[error("jellyfin is not set in the settings, and Quick Connect needs the server it names")]
    NoJellyfinServer,

    #[cfg(feature = "online")]
    #[error(
        "this server has Quick Connect off (or is Emby); sign in with jellyfin-user and jellyfin-password"
    )]
    QuickConnectOff,

    #[cfg(feature = "online")]
    #[error("the Jellyfin sign-in was cancelled before the code was approved")]
    JellyfinSignInCancelled,

    #[cfg(feature = "online")]
    #[error(
        "spotify-client-id is not set in the settings; register an app at \
         developer.spotify.com and name its client ID there"
    )]
    NoSpotifyClient,

    #[cfg(feature = "online")]
    #[error("the Spotify sign-in was cancelled before it was approved")]
    SpotifySignInCancelled,

    #[cfg(feature = "online")]
    #[error(transparent)]
    SpotifySignIn(#[from] resonate_online::SpotifySignInError),

    #[error("no Spotify account is signed in; `resonate spotify` signs in")]
    NotSignedInToSpotify,

    #[error("Spotify refused the account's refresh token; `resonate spotify` signs in again")]
    SpotifySignInLapsed,

    #[cfg(feature = "online")]
    #[error(
        "lastfm-key and lastfm-secret are not both set, and Last.fm signs in only the \
         application they name"
    )]
    NoLastfmApplication,

    #[cfg(feature = "online")]
    #[error("Last.fm turned down the API key and secret")]
    LastfmRefused,

    #[cfg(feature = "online")]
    #[error("the Last.fm sign-in ended before Resonate was allowed, so nothing was kept")]
    LastfmSignInEnded,

    #[cfg(not(feature = "mcp"))]
    #[error(
        "this build cannot serve the Model Context Protocol; it was built without the mcp feature"
    )]
    NoMcp,

    #[cfg(feature = "mcp")]
    #[error(transparent)]
    Mcp(#[from] resonate_mcp::Error),

    #[error(transparent)]
    Core(#[from] resonate_core::Error),

    #[error(transparent)]
    Codec(#[from] resonate_codec::Error),

    #[error(transparent)]
    Library(#[from] resonate_library::Error),

    #[error(transparent)]
    Providers(#[from] resonate_providers::Error),

    #[error(transparent)]
    Vault(#[from] resonate_vault::Error),

    #[error(transparent)]
    Eq(#[from] resonate_eq::Error),

    #[error(transparent)]
    Sweep(#[from] resonate_dsp::Error),

    #[error("cannot write the measurement to {path}", path = path.display())]
    WriteMeasurement {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error(transparent)]
    Engine(#[from] resonate_engine::Error),

    #[error(transparent)]
    Analysis(#[from] resonate_engine::AnalysisError),

    #[error(transparent)]
    Mpris(#[from] resonate_mpris::Error),

    #[error(transparent)]
    Sink(#[from] resonate_engine::SinkError),

    #[error(transparent)]
    Listen(#[from] resonate_listen::Error),

    #[error("no recognition service is on, so there is nothing to ask what was heard")]
    NoRecogniser,

    #[cfg(feature = "sync")]
    #[error(transparent)]
    Sync(resonate_sync::Error),

    #[error("device sync is off: online and sync must both be on")]
    SyncOff,

    #[cfg(feature = "sync")]
    #[error("another Resonate is synchronising this catalog; ask it there")]
    SyncBusy,

    #[cfg(feature = "sync")]
    #[error("the sync server could not be reached")]
    SyncUnreachable,

    #[cfg(feature = "sync")]
    #[error("{} could not be locked for syncing", path.display())]
    SyncLock {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[cfg(feature = "sync")]
    #[error("no synced device is named {0}")]
    NoSuchDevice(Box<str>),

    #[cfg(feature = "sync")]
    #[error("{device} did not take the command: {answer:?}")]
    NotControlled {
        device: Box<str>,
        answer: resonate_sync::Controlling,
    },

    #[cfg(feature = "sync")]
    #[error("{0} told nothing of what it plays; it may be away or too old to say")]
    NothingMirrored(Box<str>),

    #[cfg(feature = "sync")]
    #[error("{device} plays no zone {zone}")]
    NoSuchRemoteZone { device: Box<str>, zone: ZoneAsked },

    #[cfg(feature = "sync")]
    #[error("{given} is no place in a song; give seconds, 1:30 or 1m30s")]
    NotAPlace { given: Box<str> },

    #[cfg(feature = "sync")]
    #[error("{0} handed off nothing this catalog holds")]
    NothingHandedOff(Box<str>),

    #[cfg(feature = "sync")]
    #[error("{0} is online, so it stays listed; only a device that is away can be taken off")]
    SyncDeviceOnline(Box<str>),

    #[cfg(feature = "ui")]
    #[error(transparent)]
    Ui(#[from] resonate_ui::Error),
}

pub type Result<T> = result::Result<T, Error>;

const fn pass_named(pass: PassKind) -> &'static str {
    match pass {
        PassKind::Scan => "scan",
        PassKind::Enrich => "lookup",
        PassKind::Poll => "poll",
        PassKind::Organise => "organising",
        PassKind::Retag => "tagging",
        PassKind::Import => "import into the vault",
        PassKind::TakeIn => "copy into the music folder",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_stays_small_enough_for_result_large_err() {
        assert!(size_of::<Error>() <= 128, "{}", size_of::<Error>());
    }
}
