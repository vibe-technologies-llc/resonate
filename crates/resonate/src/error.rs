use std::{fmt, io, path::PathBuf, result};

use resonate_core::MediaLocation;
use resonate_library::PlaylistName;
use resonate_mpris::PlayerName;
use resonate_pipewire::NodeName;
use thiserror::Error;

use crate::{sleep::Spoken, suggest::SuggestionName};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConfigKey {
    Sink,
    Library,
    Vault,
    Quality,
    FilterPhase,
    TruePeak,
    RestoreLossy,
    Dither,
    NoiseShaping,
    ReplayGain,
    ReplayGainPreAmp,
    ReplayGainUntagged,
    BitPerfect,
    Dop,
    ForceGraphRate,
    BluetoothWake,
    BluetoothLeadMs,
    BluetoothAwakeS,
    Volume,
    BufferMs,
    Theme,
    Accent,
    TextSize,
    Online,
    EnrichAfterScan,
    Study,
    Contact,
    Equaliser,
    EqualiserFor,
    EqualiserProfile,
    Resume,
    SkipRepeatsQueue,
    PreviousRestarts,
    OrganiseAs,
    Notify,
    MinimiseButton,
    MaximiseButton,
    ScrollVolume,
    Scrollbars,
    SuggestionsTab,
    MissingTab,
    TabCounts,
    Inbox,
    AcoustidKey,
    AuddToken,
    ListenbrainzToken,
    ListenFrom,
    ListenFor,
    Discord,
    DiscordApp,
    DiscordShows,
    DiscordArt,
    DiscordIcon,
    DiscordProgress,
    DiscordPaused,
}

impl ConfigKey {
    pub const ALL: [Self; 55] = [
        Self::Sink,
        Self::Library,
        Self::Vault,
        Self::Quality,
        Self::FilterPhase,
        Self::TruePeak,
        Self::RestoreLossy,
        Self::Dither,
        Self::NoiseShaping,
        Self::ReplayGain,
        Self::ReplayGainPreAmp,
        Self::ReplayGainUntagged,
        Self::BitPerfect,
        Self::Dop,
        Self::ForceGraphRate,
        Self::BluetoothWake,
        Self::BluetoothLeadMs,
        Self::BluetoothAwakeS,
        Self::Volume,
        Self::BufferMs,
        Self::Theme,
        Self::Accent,
        Self::TextSize,
        Self::Online,
        Self::EnrichAfterScan,
        Self::Study,
        Self::Contact,
        Self::Equaliser,
        Self::EqualiserFor,
        Self::EqualiserProfile,
        Self::Resume,
        Self::SkipRepeatsQueue,
        Self::PreviousRestarts,
        Self::OrganiseAs,
        Self::Notify,
        Self::MinimiseButton,
        Self::MaximiseButton,
        Self::ScrollVolume,
        Self::Scrollbars,
        Self::SuggestionsTab,
        Self::MissingTab,
        Self::TabCounts,
        Self::Inbox,
        Self::AcoustidKey,
        Self::AuddToken,
        Self::ListenbrainzToken,
        Self::ListenFrom,
        Self::ListenFor,
        Self::Discord,
        Self::DiscordApp,
        Self::DiscordShows,
        Self::DiscordArt,
        Self::DiscordIcon,
        Self::DiscordProgress,
        Self::DiscordPaused,
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
            Self::Dither => "dither",
            Self::NoiseShaping => "noise-shaping",
            Self::ReplayGain => "replay-gain",
            Self::ReplayGainPreAmp => "replay-gain-pre-amp",
            Self::ReplayGainUntagged => "replay-gain-untagged",
            Self::BitPerfect => "bit-perfect",
            Self::Dop => "dop",
            Self::ForceGraphRate => "force-graph-rate",
            Self::BluetoothWake => "bluetooth-wake",
            Self::BluetoothLeadMs => "bluetooth-lead-ms",
            Self::BluetoothAwakeS => "bluetooth-awake-s",
            Self::Volume => "volume",
            Self::BufferMs => "buffer-ms",
            Self::Theme => "theme",
            Self::Accent => "accent",
            Self::TextSize => "text-size",
            Self::Online => "online",
            Self::EnrichAfterScan => "enrich-after-scan",
            Self::Study => "study",
            Self::SkipRepeatsQueue => "skip-repeats-queue",
            Self::PreviousRestarts => "previous-restarts",
            Self::Contact => "contact",
            Self::Equaliser => "equaliser",
            Self::EqualiserFor => "equaliser-for",
            Self::EqualiserProfile => "equaliser-profile",
            Self::Resume => "resume",
            Self::OrganiseAs => "organise-as",
            Self::Notify => "notify",
            Self::MinimiseButton => "minimise-button",
            Self::MaximiseButton => "maximise-button",
            Self::ScrollVolume => "scroll-volume",
            Self::Scrollbars => "scrollbars",
            Self::SuggestionsTab => "suggestions-tab",
            Self::MissingTab => "missing-tab",
            Self::TabCounts => "tab-counts",
            Self::Inbox => "inbox",
            Self::AcoustidKey => "acoustid-key",
            Self::AuddToken => "audd-token",
            Self::ListenbrainzToken => "listenbrainz-token",
            Self::ListenFrom => "listen-from",
            Self::ListenFor => "listen-for",
            Self::Discord => "discord",
            Self::DiscordApp => "discord-app",
            Self::DiscordShows => "discord-shows",
            Self::DiscordArt => "discord-art",
            Self::DiscordIcon => "discord-icon",
            Self::DiscordProgress => "discord-progress",
            Self::DiscordPaused => "discord-paused",
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
}

impl fmt::Display for ValueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Text => "string",
            Self::Boolean => "boolean",
            Self::Integer => "integer",
            Self::Number => "number",
            Self::Table => "table",
        })
    }
}

#[cfg(feature = "ui")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IconOp {
    Read,
    Stage,
    Place,
    Remove,
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

    #[error("cannot create {path}", path = path.display())]
    CreateDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[cfg(feature = "ui")]
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

    #[error("no player is on the session bus; start one with resonate play or open the window")]
    NothingRunning,

    #[error("no player named {name} is on the session bus; resonate players lists what is")]
    NoSuchPlayer { name: PlayerName },

    #[error("no playlist named {0}")]
    NoSuchPlaylist(PlaylistName),

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

    #[error("{given} is not a sleep timer; give it a number of minutes, track, queue or off")]
    NotASleepTimer { given: Spoken },

    #[error("--as names one playlist, and {given} files were given")]
    NameForOnePlaylistOnly { given: usize },

    #[error("no sink named {0} is present in the graph")]
    SinkNotFound(NodeName),

    #[error("a tracing subscriber was already installed")]
    LoggingAlreadyInstalled,

    #[cfg(not(feature = "online"))]
    #[error("this build has no online reference; it was built without the online feature")]
    NoReference,

    #[cfg(feature = "online")]
    #[error("online is off in the settings, so there is no reference to ask")]
    OnlineOff,

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
    Vault(#[from] resonate_vault::Error),

    #[error(transparent)]
    Eq(#[from] resonate_eq::Error),

    #[error(transparent)]
    Engine(#[from] resonate_engine::Error),

    #[error(transparent)]
    Analysis(#[from] resonate_engine::AnalysisError),

    #[error(transparent)]
    Mpris(#[from] resonate_mpris::Error),

    #[error(transparent)]
    Sink(#[from] resonate_pipewire::Error),

    #[error(transparent)]
    Listen(#[from] resonate_listen::Error),

    #[error("no recognition service is on, so there is nothing to ask what was heard")]
    NoRecogniser,

    #[cfg(feature = "ui")]
    #[error(transparent)]
    Ui(#[from] resonate_ui::Error),
}

pub type Result<T> = result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_stays_small_enough_for_result_large_err() {
        assert!(size_of::<Error>() <= 128, "{}", size_of::<Error>());
    }
}
