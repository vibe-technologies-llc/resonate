use std::{
    collections::BTreeMap,
    fmt, mem,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use crossbeam_channel::Sender;
use futures_channel::oneshot;
use gpui::{Pixels, Size, px, size};
use resonate_core::{
    Accent, AppId, ArtistsDrawn, Frames, Icon, Pictured, PlaylistId, Presence, ScrollbarMode,
    Shown, SpeakerAngle, SpeakerElevation, Spectral, SpectrumBands, SpectrumFalls, SpectrumFloor,
    SpectrumTilt, SyncedSongs, TextSize, Theme, Trim, Volume,
};
use resonate_engine::{
    Crossfeed, DitherKind, FilterPhase, NodeName, NoiseShaping, PreviousRestarts, Quality,
    ReplayGainMode, Restoration, SkipUnderRepeat,
};
use resonate_eq::Binding;
use resonate_library::{Cut, HistoryKept, MinimumLength, MusicExtensions};
use resonate_listen::Listening;
use resonate_providers::Providers;

use crate::{Launcher, Notice, Result, equaliser::Curve};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SettingsCategory {
    #[default]
    Output,
    Processing,
    Equaliser,
    Library,
    Filters,
    Online,
    Recognition,
    Scrobbling,
    Sync,
    Desktop,
    Appearance,
    About,
}

impl SettingsCategory {
    pub const ALL: [Self; 12] = [
        Self::Output,
        Self::Processing,
        Self::Equaliser,
        Self::Library,
        Self::Filters,
        Self::Online,
        Self::Recognition,
        Self::Scrobbling,
        Self::Sync,
        Self::Desktop,
        Self::Appearance,
        Self::About,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Output => "output",
            Self::Processing => "processing",
            Self::Equaliser => "equaliser",
            Self::Library => "library",
            Self::Filters => "filters",
            Self::Online => "online",
            Self::Recognition => "recognition",
            Self::Scrobbling => "scrobbling",
            Self::Sync => "sync",
            Self::Desktop => "desktop",
            Self::Appearance => "appearance",
            Self::About => "about",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|category| category.as_str() == text)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowSize {
    width: u32,
    height: u32,
}

impl WindowSize {
    pub const MIN_WIDTH: u32 = 720;
    pub const MIN_HEIGHT: u32 = 520;
    pub const MAX_DIMENSION: u32 = 8192;

    pub const fn new(width: u32, height: u32) -> Option<Self> {
        if width < Self::MIN_WIDTH
            || height < Self::MIN_HEIGHT
            || width > Self::MAX_DIMENSION
            || height > Self::MAX_DIMENSION
        {
            return None;
        }

        Some(Self { width, height })
    }

    pub const fn width(self) -> u32 {
        self.width
    }

    pub const fn height(self) -> u32 {
        self.height
    }

    pub fn pixels(self) -> Size<Pixels> {
        #[expect(
            clippy::cast_precision_loss,
            reason = "window dimensions are bounded to values exactly represented by f32"
        )]
        let (width, height) = (self.width as f32, self.height as f32);

        size(px(width), px(height))
    }

    pub fn from_pixels(size: Size<Pixels>) -> Option<Self> {
        let width = f64::from(f32::from(size.width).round());
        let height = f64::from(f32::from(size.height).round());
        if !width.is_finite()
            || !height.is_finite()
            || width < f64::from(Self::MIN_WIDTH)
            || height < f64::from(Self::MIN_HEIGHT)
            || width > f64::from(Self::MAX_DIMENSION)
            || height > f64::from(Self::MAX_DIMENSION)
        {
            return None;
        }

        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "window dimensions are rounded and bounded above zero"
        )]
        let (width, height) = (width as u32, height as u32);

        Self::new(width, height)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SettingKey {
    Sink,
    Quality,
    FilterPhase,
    TruePeak,
    Restoration,
    Crossfeed,
    Dither,
    NoiseShaping,
    ReplayGain,
    PreAmp,
    Untagged,
    BitPerfect,
    Dop,
    DsdLikePcm,
    DeviceVolume,
    ForceGraphRate,
    BluetoothWake,
    BluetoothLead,
    BluetoothAwake,
    Buffer,
    Volume,
    Theme,
    Accent,
    TextSize,
    Online,
    EnrichAfterScan,
    Study,
    FetchLyrics,
    LyricsByTheLocale,
    IdentifyBySound,
    Contact,
    AcoustidKey,
    AuddToken,
    ListenbrainzToken,
    LastfmKey,
    LastfmSecret,
    LastfmSession,
    ListenFrom,
    ListenFor,
    Equaliser,
    EqualiserFor,
    EqualiserProfile,
    EqualiserMeasurer,
    EqualiserMeasurerFor,
    Convolution,
    SpeakerAngle,
    SpeakerElevation,
    Resume,
    HistoryKept,
    SkipUnderRepeat,
    PreviousRestarts,
    OrganiseAs,
    Notify,
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
    Vault,
    MusicFolder,
    MusicExtensions,
    MinimumLength,
    FileDropped,
    Subsonic,
    SubsonicUser,
    SubsonicPassword,
    TidalClientId,
    TidalClientSecret,
    TidalRefreshToken,
    HifiApi,
    HifiSecret,
    Monochrome,
    HostedServices,
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
    SyncKey,
    SyncDevice,
    SyncSongs,
}

impl SettingKey {
    pub const ALL: [Self; 102] = [
        Self::Sink,
        Self::Quality,
        Self::FilterPhase,
        Self::TruePeak,
        Self::Restoration,
        Self::Crossfeed,
        Self::Dither,
        Self::NoiseShaping,
        Self::ReplayGain,
        Self::PreAmp,
        Self::Untagged,
        Self::BitPerfect,
        Self::Dop,
        Self::DsdLikePcm,
        Self::DeviceVolume,
        Self::ForceGraphRate,
        Self::BluetoothWake,
        Self::BluetoothLead,
        Self::BluetoothAwake,
        Self::Buffer,
        Self::Volume,
        Self::Theme,
        Self::Accent,
        Self::TextSize,
        Self::Online,
        Self::EnrichAfterScan,
        Self::Study,
        Self::FetchLyrics,
        Self::LyricsByTheLocale,
        Self::IdentifyBySound,
        Self::Contact,
        Self::AcoustidKey,
        Self::AuddToken,
        Self::ListenbrainzToken,
        Self::LastfmKey,
        Self::LastfmSecret,
        Self::LastfmSession,
        Self::ListenFrom,
        Self::ListenFor,
        Self::Equaliser,
        Self::EqualiserFor,
        Self::EqualiserProfile,
        Self::EqualiserMeasurer,
        Self::EqualiserMeasurerFor,
        Self::Convolution,
        Self::SpeakerAngle,
        Self::SpeakerElevation,
        Self::Resume,
        Self::HistoryKept,
        Self::SkipUnderRepeat,
        Self::PreviousRestarts,
        Self::OrganiseAs,
        Self::Notify,
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
        Self::Vault,
        Self::MusicFolder,
        Self::MusicExtensions,
        Self::MinimumLength,
        Self::FileDropped,
        Self::Subsonic,
        Self::SubsonicUser,
        Self::SubsonicPassword,
        Self::TidalClientId,
        Self::TidalClientSecret,
        Self::TidalRefreshToken,
        Self::HifiApi,
        Self::HifiSecret,
        Self::Monochrome,
        Self::HostedServices,
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
        Self::SyncKey,
        Self::SyncDevice,
        Self::SyncSongs,
    ];
}

#[derive(Clone, Debug, PartialEq)]
pub enum Setting {
    Sink(Option<NodeName>),
    Quality(Quality),
    FilterPhase(FilterPhase),
    TruePeak(bool),
    Restoration(Restoration),
    Crossfeed(Crossfeed),
    Dither(DitherKind),
    NoiseShaping(NoiseShaping),
    ReplayGain(ReplayGainMode),
    PreAmp(Trim),
    Untagged(Trim),
    BitPerfect(bool),
    Dop(bool),
    DsdLikePcm(bool),
    DeviceVolume(bool),
    ForceGraphRate(bool),
    BluetoothWake(bool),
    BluetoothLead(Duration),
    BluetoothAwake(Duration),
    Buffer(Duration),
    Volume(Volume),
    Theme(Theme),
    Accent(Option<Accent>),
    TextSize(TextSize),
    Online(bool),
    EnrichAfterScan(bool),
    Study(bool),
    FetchLyrics(bool),
    LyricsByTheLocale(bool),
    IdentifyBySound(bool),
    Contact(String),
    AcoustidKey(String),
    AuddToken(String),
    ListenbrainzToken(String),
    LastfmKey(String),
    LastfmSecret(String),
    LastfmSession(String),
    ListenFrom(Listening),
    ListenFor(Duration),
    Equaliser(bool),
    EqualiserFor {
        sink: NodeName,
        binding: Option<Binding>,
    },
    EqualiserProfile(Option<Binding>),
    EqualiserMeasurer(Option<String>),
    EqualiserMeasurerFor {
        sink: NodeName,
        measurer: Option<String>,
    },
    Convolution(Option<PathBuf>),
    SpeakerAngle(SpeakerAngle),
    SpeakerElevation(SpeakerElevation),
    Resume(bool),
    HistoryKept(HistoryKept),
    SkipUnderRepeat(SkipUnderRepeat),
    PreviousRestarts(PreviousRestarts),
    OrganiseAs(String),
    Notify(bool),
    MinimiseButton(bool),
    MaximiseButton(bool),
    ScrollVolume(bool),
    MouseNavigation(bool),
    ReducedMotion(bool),
    Scrollbars(ScrollbarMode),
    SpectrumTilt(SpectrumTilt),
    SpectrumFloor(SpectrumFloor),
    SpectrumBands(SpectrumBands),
    SpectrumFalls(SpectrumFalls),
    SuggestionsTab(bool),
    MissingTab(bool),
    TabCounts(bool),
    RememberTab(bool),
    LastTab(crate::Pane),
    ArtistsDrawn(ArtistsDrawn),
    RememberWindowSize(bool),
    WindowSize(WindowSize),
    RememberSettingsCategory(bool),
    LastSettingsCategory(SettingsCategory),
    Inbox(PathBuf),
    Vault(PathBuf),
    MusicFolder(PathBuf),
    MusicExtensions(MusicExtensions),
    MinimumLength(MinimumLength),
    FileDropped(bool),
    Subsonic(String),
    SubsonicUser(String),
    SubsonicPassword(String),
    TidalClientId(String),
    TidalClientSecret(String),
    TidalRefreshToken(String),
    HifiApi(String),
    HifiSecret(String),
    Monochrome(String),
    HostedServices(bool),
    Discord(bool),
    DiscordApp(Option<AppId>),
    DiscordShows(Shown),
    DiscordArt(Pictured),
    DiscordIcon(Option<Icon>),
    DiscordProgress(bool),
    DiscordPaused(bool),
    Sync(bool),
    SyncNearbyOnly(bool),
    SyncServer(String),
    SyncKey(String),
    SyncDevice(String),
    SyncSongs(SyncedSongs),
}

impl SettingKey {
    pub const fn is_the_zones_own(self) -> bool {
        matches!(self, Self::Sink | Self::Volume)
    }
}

impl Setting {
    pub const fn key(&self) -> SettingKey {
        match self {
            Self::Sink(_) => SettingKey::Sink,
            Self::Quality(_) => SettingKey::Quality,
            Self::FilterPhase(_) => SettingKey::FilterPhase,
            Self::TruePeak(_) => SettingKey::TruePeak,
            Self::Restoration(_) => SettingKey::Restoration,
            Self::Crossfeed(_) => SettingKey::Crossfeed,
            Self::Dither(_) => SettingKey::Dither,
            Self::NoiseShaping(_) => SettingKey::NoiseShaping,
            Self::ReplayGain(_) => SettingKey::ReplayGain,
            Self::PreAmp(_) => SettingKey::PreAmp,
            Self::Untagged(_) => SettingKey::Untagged,
            Self::BitPerfect(_) => SettingKey::BitPerfect,
            Self::Dop(_) => SettingKey::Dop,
            Self::DsdLikePcm(_) => SettingKey::DsdLikePcm,
            Self::DeviceVolume(_) => SettingKey::DeviceVolume,
            Self::ForceGraphRate(_) => SettingKey::ForceGraphRate,
            Self::BluetoothWake(_) => SettingKey::BluetoothWake,
            Self::BluetoothLead(_) => SettingKey::BluetoothLead,
            Self::BluetoothAwake(_) => SettingKey::BluetoothAwake,
            Self::Buffer(_) => SettingKey::Buffer,
            Self::Volume(_) => SettingKey::Volume,
            Self::Theme(_) => SettingKey::Theme,
            Self::Accent(_) => SettingKey::Accent,
            Self::TextSize(_) => SettingKey::TextSize,
            Self::Online(_) => SettingKey::Online,
            Self::EnrichAfterScan(_) => SettingKey::EnrichAfterScan,
            Self::Study(_) => SettingKey::Study,
            Self::FetchLyrics(_) => SettingKey::FetchLyrics,
            Self::LyricsByTheLocale(_) => SettingKey::LyricsByTheLocale,
            Self::IdentifyBySound(_) => SettingKey::IdentifyBySound,
            Self::Contact(_) => SettingKey::Contact,
            Self::AcoustidKey(_) => SettingKey::AcoustidKey,
            Self::AuddToken(_) => SettingKey::AuddToken,
            Self::ListenbrainzToken(_) => SettingKey::ListenbrainzToken,
            Self::LastfmKey(_) => SettingKey::LastfmKey,
            Self::LastfmSecret(_) => SettingKey::LastfmSecret,
            Self::LastfmSession(_) => SettingKey::LastfmSession,
            Self::ListenFrom(_) => SettingKey::ListenFrom,
            Self::ListenFor(_) => SettingKey::ListenFor,
            Self::Equaliser(_) => SettingKey::Equaliser,
            Self::EqualiserFor { .. } => SettingKey::EqualiserFor,
            Self::EqualiserProfile(_) => SettingKey::EqualiserProfile,
            Self::EqualiserMeasurer(_) => SettingKey::EqualiserMeasurer,
            Self::EqualiserMeasurerFor { .. } => SettingKey::EqualiserMeasurerFor,
            Self::Convolution(_) => SettingKey::Convolution,
            Self::SpeakerAngle(_) => SettingKey::SpeakerAngle,
            Self::SpeakerElevation(_) => SettingKey::SpeakerElevation,
            Self::Resume(_) => SettingKey::Resume,
            Self::HistoryKept(_) => SettingKey::HistoryKept,
            Self::SkipUnderRepeat(_) => SettingKey::SkipUnderRepeat,
            Self::PreviousRestarts(_) => SettingKey::PreviousRestarts,
            Self::OrganiseAs(_) => SettingKey::OrganiseAs,
            Self::Notify(_) => SettingKey::Notify,
            Self::MinimiseButton(_) => SettingKey::MinimiseButton,
            Self::MaximiseButton(_) => SettingKey::MaximiseButton,
            Self::ScrollVolume(_) => SettingKey::ScrollVolume,
            Self::MouseNavigation(_) => SettingKey::MouseNavigation,
            Self::ReducedMotion(_) => SettingKey::ReducedMotion,
            Self::Scrollbars(_) => SettingKey::Scrollbars,
            Self::SpectrumTilt(_) => SettingKey::SpectrumTilt,
            Self::SpectrumFloor(_) => SettingKey::SpectrumFloor,
            Self::SpectrumBands(_) => SettingKey::SpectrumBands,
            Self::SpectrumFalls(_) => SettingKey::SpectrumFalls,
            Self::SuggestionsTab(_) => SettingKey::SuggestionsTab,
            Self::MissingTab(_) => SettingKey::MissingTab,
            Self::TabCounts(_) => SettingKey::TabCounts,
            Self::RememberTab(_) => SettingKey::RememberTab,
            Self::LastTab(_) => SettingKey::LastTab,
            Self::ArtistsDrawn(_) => SettingKey::ArtistsDrawn,
            Self::RememberWindowSize(_) => SettingKey::RememberWindowSize,
            Self::WindowSize(_) => SettingKey::WindowSize,
            Self::RememberSettingsCategory(_) => SettingKey::RememberSettingsCategory,
            Self::LastSettingsCategory(_) => SettingKey::LastSettingsCategory,
            Self::Inbox(_) => SettingKey::Inbox,
            Self::Vault(_) => SettingKey::Vault,
            Self::MusicExtensions(_) => SettingKey::MusicExtensions,
            Self::MinimumLength(_) => SettingKey::MinimumLength,
            Self::MusicFolder(_) => SettingKey::MusicFolder,
            Self::FileDropped(_) => SettingKey::FileDropped,
            Self::Subsonic(_) => SettingKey::Subsonic,
            Self::SubsonicUser(_) => SettingKey::SubsonicUser,
            Self::SubsonicPassword(_) => SettingKey::SubsonicPassword,
            Self::TidalClientId(_) => SettingKey::TidalClientId,
            Self::TidalClientSecret(_) => SettingKey::TidalClientSecret,
            Self::TidalRefreshToken(_) => SettingKey::TidalRefreshToken,
            Self::HifiApi(_) => SettingKey::HifiApi,
            Self::HifiSecret(_) => SettingKey::HifiSecret,
            Self::Monochrome(_) => SettingKey::Monochrome,
            Self::HostedServices(_) => SettingKey::HostedServices,
            Self::Discord(_) => SettingKey::Discord,
            Self::DiscordApp(_) => SettingKey::DiscordApp,
            Self::DiscordShows(_) => SettingKey::DiscordShows,
            Self::DiscordArt(_) => SettingKey::DiscordArt,
            Self::DiscordIcon(_) => SettingKey::DiscordIcon,
            Self::DiscordProgress(_) => SettingKey::DiscordProgress,
            Self::DiscordPaused(_) => SettingKey::DiscordPaused,
            Self::Sync(_) => SettingKey::Sync,
            Self::SyncNearbyOnly(_) => SettingKey::SyncNearbyOnly,
            Self::SyncServer(_) => SettingKey::SyncServer,
            Self::SyncKey(_) => SettingKey::SyncKey,
            Self::SyncDevice(_) => SettingKey::SyncDevice,
            Self::SyncSongs(_) => SettingKey::SyncSongs,
        }
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct Online {
    pub enabled: bool,
    pub after_scan: bool,
    pub studies: bool,
    pub lyrics: bool,
    pub contact: String,
    pub acoustid_key: String,
    pub audd_token: String,
    pub listenbrainz_token: String,
    pub lastfm_key: String,
    pub lastfm_secret: String,
    pub lastfm_session: String,
    pub subsonic: String,
    pub subsonic_user: String,
    pub subsonic_password: String,
    pub tidal_client_id: String,
    pub tidal_client_secret: String,
    pub tidal_refresh_token: String,
    pub hifi_api: String,
    pub hifi_secret: String,
    pub monochrome: String,
    pub hosted_services: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub struct TidalSignIn {
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
}

impl fmt::Debug for TidalSignIn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TidalSignIn")
            .field("client_id", &self.client_id)
            .field("client_secret", &withheld(&self.client_secret))
            .field("refresh_token", &withheld(&self.refresh_token))
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Followed {
    Bindings(Bindings),
    Tidal(TidalSignIn),
}

struct Withheld;

impl fmt::Debug for Withheld {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<withheld>")
    }
}

fn withheld(secret: &str) -> Option<Withheld> {
    (!secret.is_empty()).then_some(Withheld)
}

impl fmt::Debug for Online {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            enabled,
            after_scan,
            studies,
            lyrics,
            contact,
            acoustid_key,
            audd_token,
            listenbrainz_token,
            lastfm_key,
            lastfm_secret,
            lastfm_session,
            subsonic,
            subsonic_user,
            subsonic_password,
            tidal_client_id,
            tidal_client_secret,
            tidal_refresh_token,
            hifi_api,
            hifi_secret,
            monochrome,
            hosted_services,
        } = self;

        f.debug_struct("Online")
            .field("enabled", enabled)
            .field("after_scan", after_scan)
            .field("studies", studies)
            .field("lyrics", lyrics)
            .field("contact", &withheld(contact))
            .field("acoustid_key", &withheld(acoustid_key))
            .field("audd_token", &withheld(audd_token))
            .field("listenbrainz_token", &withheld(listenbrainz_token))
            .field("lastfm_key", &withheld(lastfm_key))
            .field("lastfm_secret", &withheld(lastfm_secret))
            .field("lastfm_session", &withheld(lastfm_session))
            .field("subsonic", subsonic)
            .field("subsonic_user", subsonic_user)
            .field("subsonic_password", &withheld(subsonic_password))
            .field("tidal_client_id", tidal_client_id)
            .field("tidal_client_secret", &withheld(tidal_client_secret))
            .field("tidal_refresh_token", &withheld(tidal_refresh_token))
            .field("hifi_api", hifi_api)
            .field("hifi_secret", &withheld(hifi_secret))
            .field("monochrome", monochrome)
            .field("hosted_services", hosted_services)
            .finish()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Supplying<'a> {
    pub inbox: Option<&'a Path>,
    pub online: &'a Online,
    pub asking: Asking,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Asking {
    #[default]
    EveryProvider,
    TheInboxAlone,
}

pub type Registering = Arc<dyn Fn(&Supplying<'_>) -> Providers + Send + Sync>;

pub type Unkept = Arc<dyn Fn() -> bool + Send + Sync>;

const TOKEN_NOT_KEPT: &str = "TIDAL's new sign-in couldn't be saved — sign in to TIDAL again once the settings file can be written";

#[derive(Clone)]
pub struct Sourcing {
    pub inbox: Option<PathBuf>,
    pub register: Registering,
    pub unkept: Unkept,
}

impl Sourcing {
    pub(crate) fn unkept_notice(&self) -> Option<Notice> {
        (self.unkept)().then(|| Notice::Trouble(TOKEN_NOT_KEPT.to_owned()))
    }

    pub fn providers(&self, online: &Online, asking: Asking) -> Providers {
        (self.register)(&Supplying {
            inbox: self.inbox.as_deref(),
            online,
            asking,
        })
    }
}

impl Default for Sourcing {
    fn default() -> Self {
        Self {
            inbox: None,
            register: Arc::new(|_| Providers::none()),
            unkept: Arc::new(|| false),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Measurers {
    pub fallback: Option<String>,
    pub by_sink: BTreeMap<NodeName, String>,
}

impl Measurers {
    pub fn for_sink(&self, sink: Option<&NodeName>) -> Option<&str> {
        sink.and_then(|sink| self.by_sink.get(sink))
            .or(self.fallback.as_ref())
            .map(String::as_str)
    }

    pub fn of_the_device(&self, sink: &NodeName) -> Option<&str> {
        self.by_sink.get(sink).map(String::as_str)
    }

    pub fn is_empty(&self) -> bool {
        self.fallback.is_none() && self.by_sink.is_empty()
    }

    pub fn prefer_for_every_device(&mut self, measurer: Option<String>) {
        self.fallback = measurer;
    }

    pub fn prefer_for(&mut self, sink: NodeName, measurer: Option<String>) {
        match measurer {
            Some(measurer) => {
                self.by_sink.insert(sink, measurer);
            }
            None => {
                self.by_sink.remove(&sink);
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bindings {
    pub enabled: bool,
    pub fallback: Option<Binding>,
    pub by_sink: Vec<(NodeName, Binding)>,
}

impl Bindings {
    fn of_the_device(&self, sink: &NodeName) -> Option<&Binding> {
        self.by_sink
            .iter()
            .find(|(held, _)| held == sink)
            .map(|(_, binding)| binding)
    }

    pub fn bound_to(&self, sink: Option<&NodeName>) -> Option<Curve> {
        let own = sink
            .and_then(|sink| {
                self.of_the_device(sink)
                    .map(|binding| (Some(sink), binding))
            })
            .or_else(|| self.fallback.as_ref().map(|binding| (None, binding)));
        own.map(|(owner, binding)| Curve::of(owner, binding))
    }

    pub fn named(&self, sink: Option<&NodeName>) -> Option<&Binding> {
        match sink {
            Some(sink) => self.of_the_device(sink),
            None => self.fallback.as_ref(),
        }
    }

    pub fn curves(&self) -> impl Iterator<Item = Curve> + '_ {
        self.by_sink
            .iter()
            .map(|(sink, binding)| Curve::of(Some(sink), binding))
            .chain(self.fallback.iter().map(|binding| Curve::of(None, binding)))
    }

    pub fn bind(&mut self, sink: Option<&NodeName>, binding: Option<Binding>) {
        match sink {
            Some(sink) => {
                self.by_sink.retain(|(held, _)| held != sink);
                if let Some(binding) = binding {
                    self.by_sink.push((sink.clone(), binding));
                }
                self.by_sink.sort_by(|one, other| one.0.cmp(&other.0));
            }
            None => self.fallback = binding,
        }
    }

    pub fn binds_anything(&self) -> bool {
        self.fallback.is_some() || !self.by_sink.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowButtons {
    pub minimise: bool,
    pub maximise: bool,
}

impl WindowButtons {
    pub const SHOWN: Self = Self {
        minimise: true,
        maximise: true,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tabs {
    pub suggestions: bool,
    pub missing: bool,
    pub counts: bool,
}

impl Tabs {
    pub const AS_BUILT: Self = Self {
        suggestions: true,
        missing: false,
        counts: false,
    };
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Places {
    pub config: PathBuf,
    pub library: PathBuf,
    pub equaliser: PathBuf,
}

pub struct Stored {
    pub settings: Arc<dyn Settings>,
    pub caret: CaretBlink,
    pub places: Places,
    pub resume: bool,
    pub history_kept: HistoryKept,
    pub organise_as: String,
    pub notify: Arc<AtomicBool>,
    pub by_sound: Arc<AtomicBool>,
    pub lyrics_by_the_locale: Arc<AtomicBool>,
    pub convolution: Option<PathBuf>,
    pub speaker_angle: SpeakerAngle,
    pub speaker_elevation: SpeakerElevation,
    pub music_folder: Option<PathBuf>,
    pub file_dropped: bool,
    pub window_buttons: WindowButtons,
    pub scroll_volume: bool,
    pub mouse_navigation: bool,
    pub reduced_motion: bool,
    pub scrollbars: ScrollbarMode,
    pub spectral: Spectral,
    pub tabs: Tabs,
    pub remember_tab: bool,
    pub last_tab: Option<crate::Pane>,
    pub artists_drawn: ArtistsDrawn,
    pub remember_window_size: bool,
    pub window_size: Option<WindowSize>,
    pub remember_settings_category: bool,
    pub last_settings_category: SettingsCategory,
    pub presence: Presence,
    pub present: Arc<dyn Present>,
    pub synchronised: Synchronised,
    pub syncing: Option<Arc<dyn Syncing>>,
    pub reads_as_a_code: Option<fn(&str) -> bool>,
    pub launcher: Arc<dyn Launcher>,
}

#[derive(Clone, Debug)]
pub struct CaretBlink {
    half: Arc<AtomicU64>,
}

const STEADY: u64 = 0;
const HALF_A_BLINK_AS_BUILT: Duration = Duration::from_millis(500);

impl CaretBlink {
    pub fn as_built() -> Self {
        Self {
            half: Arc::new(AtomicU64::new(millis(HALF_A_BLINK_AS_BUILT))),
        }
    }

    pub fn steady(&self) {
        self.half.store(STEADY, Ordering::Relaxed);
    }

    pub fn every(&self, half: Duration) {
        self.half.store(millis(half).max(1), Ordering::Relaxed);
    }

    pub(crate) fn half(&self) -> Option<Duration> {
        match self.half.load(Ordering::Relaxed) {
            STEADY => None,
            half => Some(Duration::from_millis(half)),
        }
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

pub trait Present: Send + Sync {
    fn follow(&self, presence: &Presence);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Synchronised {
    pub enabled: bool,
    pub nearby_only: bool,
    pub server: String,
    pub device: String,
    pub songs: SyncedSongs,
}

impl Default for Synchronised {
    fn default() -> Self {
        Self {
            enabled: true,
            nearby_only: false,
            server: String::new(),
            device: String::new(),
            songs: SyncedSongs::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SyncStanding {
    #[default]
    Waiting,
    Connecting,
    Connected,
    Unreachable,
    Refused,
    Nearby,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncedPlatform {
    Linux,
    Android,
    Ios,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncedDevice {
    pub id: String,
    pub name: String,
    pub platform: SyncedPlatform,
    pub online: bool,
    pub nearby: bool,
    pub this: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncShown {
    pub standing: SyncStanding,
    pub code: String,
    pub link: String,
    pub devices: Vec<SyncedDevice>,
    pub moving: usize,
    pub downloading: usize,
    pub waiting: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandedOff {
    pub device: String,
    pub songs: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Continuing {
    pub cuts: Vec<Cut>,
    pub at: Frames,
}

pub trait Syncing: Send + Sync {
    fn shown(&self) -> SyncShown;

    fn force(&self);

    fn forget(&self, device: &str) -> bool;

    fn missing_from(&self, playlist: PlaylistId) -> usize;

    fn fetch_missing(&self, playlist: PlaylistId, from: &str);

    fn keeps_songs_of(&self, playlist: PlaylistId) -> bool;

    fn keep_songs_of(&self, playlist: PlaylistId, keep: bool);

    fn handed_off(&self) -> Vec<HandedOff>;

    fn continue_from(&self, device: &str) -> Continuing;
}

#[derive(Clone, Debug, PartialEq)]
pub enum SettingChange {
    Store(Setting),
    Forget(SettingKey),
}

impl SettingChange {
    pub fn key(&self) -> SettingKey {
        match self {
            Self::Store(setting) => setting.key(),
            Self::Forget(key) => *key,
        }
    }
}

pub trait Settings: Send + Sync {
    fn apply(&self, changes: &[SettingChange]) -> Result<()>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Ephemeral;

impl Settings for Ephemeral {
    fn apply(&self, _changes: &[SettingChange]) -> Result<()> {
        Ok(())
    }
}

const SETTINGS_WRITER_THREAD: &str = "resonate-settings";

struct Batch {
    changes: Vec<SettingChange>,
    landed: Option<oneshot::Sender<Result<()>>>,
}

impl Batch {
    fn written_by(self, settings: &dyn Settings) {
        let written = settings.apply(&self.changes);
        match self.landed {
            Some(landed) => {
                let _ = landed.send(written);
            }
            None => {
                if let Err(error) = written {
                    tracing::error!(%error, "the settings changed at the close could not be saved");
                }
            }
        }
    }
}

pub(crate) struct SettingsWriter {
    settings: Arc<dyn Settings>,
    pending: Vec<SettingChange>,
    writing: bool,
    sent: Option<Sender<Batch>>,
    worker: Option<JoinHandle<()>>,
}

impl SettingsWriter {
    pub(crate) fn over(settings: Arc<dyn Settings>) -> Self {
        let (sent, heard) = crossbeam_channel::unbounded::<Batch>();
        let writing = Arc::clone(&settings);
        let worker = thread::Builder::new()
            .name(SETTINGS_WRITER_THREAD.to_owned())
            .spawn(move || {
                for batch in heard {
                    batch.written_by(writing.as_ref());
                }
            })
            .inspect_err(|error| {
                tracing::warn!(%error, "no thread writes the settings, so the window writes them itself");
            })
            .ok();

        Self {
            settings,
            pending: Vec::new(),
            writing: false,
            sent: worker.is_some().then_some(sent),
            worker,
        }
    }

    pub(crate) fn change(&mut self, change: SettingChange) {
        self.pending.push(change);
    }

    pub(crate) fn next_batch(&mut self) -> Option<oneshot::Receiver<Result<()>>> {
        if self.writing || self.pending.is_empty() {
            return None;
        }
        let (landed, heard) = oneshot::channel();
        let batch = Batch {
            changes: mem::take(&mut self.pending),
            landed: Some(landed),
        };
        let unsent = match &self.sent {
            Some(sent) => sent.send(batch).err().map(|unsent| unsent.0),
            None => Some(batch),
        };
        if let Some(batch) = unsent {
            batch.written_by(self.settings.as_ref());
        }
        self.writing = true;
        Some(heard)
    }

    pub(crate) const fn landed(&mut self) {
        self.writing = false;
    }
}

impl Drop for SettingsWriter {
    fn drop(&mut self) {
        let changes = mem::take(&mut self.pending);
        let batch = (!changes.is_empty()).then_some(Batch {
            changes,
            landed: None,
        });
        let unsent = match (self.sent.take(), batch) {
            (Some(sent), Some(batch)) => sent.send(batch).err().map(|unsent| unsent.0),
            (None, batch) => batch,
            (Some(_), None) => None,
        };
        if let Some(batch) = unsent {
            batch.written_by(self.settings.as_ref());
        }
        if let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            tracing::error!("the thread writing the settings panicked");
        }
    }
}

#[cfg(test)]
mod tests {
    use resonate_eq::ProfileName;

    use super::*;

    fn kept(name: &str) -> Binding {
        Binding::Profile(ProfileName::new(name).expect("a usable name"))
    }

    #[test]
    fn a_rotated_token_that_could_not_be_kept_is_told_once_to_the_listener() {
        let left = Arc::new(AtomicBool::new(true));
        let sourcing = Sourcing {
            unkept: Arc::new({
                let left = Arc::clone(&left);
                move || left.swap(false, Ordering::Relaxed)
            }),
            ..Sourcing::default()
        };

        assert!(matches!(sourcing.unkept_notice(), Some(Notice::Trouble(_))));
        assert_eq!(sourcing.unkept_notice(), None);
        assert_eq!(Sourcing::default().unkept_notice(), None);
    }

    #[test]
    fn the_online_settings_print_no_secret_and_no_contact() {
        let online = Online {
            contact: "someone at an address".to_owned(),
            subsonic_user: "listener".to_owned(),
            subsonic_password: "subsonic-sesame".to_owned(),
            tidal_client_secret: "tidal-hush".to_owned(),
            tidal_refresh_token: "tidal-sesame".to_owned(),
            lastfm_secret: "lastfm-hush".to_owned(),
            lastfm_session: "lastfm-sesame".to_owned(),
            listenbrainz_token: "listenbrainz-sesame".to_owned(),
            ..Online::default()
        };
        let supplying = Supplying {
            inbox: None,
            online: &online,
            asking: Asking::EveryProvider,
        };

        let printed = format!("{supplying:?}");

        assert!(printed.contains("listener"));
        assert!(printed.contains("<withheld>"));
        for secret in [
            "someone at an address",
            "subsonic-sesame",
            "tidal-hush",
            "tidal-sesame",
            "lastfm-hush",
            "lastfm-sesame",
            "listenbrainz-sesame",
        ] {
            assert!(!printed.contains(secret), "{secret} was printed");
        }
    }

    #[test]
    fn a_device_given_its_own_curve_is_shown_its_own_and_every_other_the_fallbacks() {
        let headphones = NodeName::new("bluez_output.headphones");
        let speakers = NodeName::new("alsa_output.speakers");
        let mut bindings = Bindings::default();

        assert_eq!(bindings.bound_to(Some(&headphones)), None);

        bindings.bind(Some(&headphones), Some(Binding::Own));
        bindings.bind(None, Some(Binding::Own));
        assert_eq!(
            bindings.bound_to(Some(&headphones)),
            Some(Curve::Own(Some(headphones.clone())))
        );
        assert_eq!(bindings.bound_to(Some(&speakers)), Some(Curve::Own(None)));
        assert_eq!(bindings.bound_to(None), Some(Curve::Own(None)));

        bindings.bind(Some(&speakers), Some(kept("HD 650")));
        assert_eq!(
            bindings.bound_to(Some(&speakers)),
            Some(Curve::Kept(
                ProfileName::new("HD 650").expect("a usable name")
            ))
        );
        assert_eq!(bindings.named(Some(&headphones)), Some(&Binding::Own));

        let curves: Vec<Curve> = bindings.curves().collect();
        assert_eq!(curves.len(), 3);
        assert!(curves.contains(&Curve::Own(Some(headphones.clone()))));
        assert!(curves.contains(&Curve::Own(None)));

        bindings.bind(Some(&headphones), None);
        assert_eq!(bindings.bound_to(Some(&headphones)), Some(Curve::Own(None)));
    }
}
