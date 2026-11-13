use std::fmt;

macro_rules! keys {
    ($($key:ident = $spelt:literal $(, $default:ident)?;)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum ConfigKey {
            $($key,)*
        }

        impl ConfigKey {
            pub const ALL: [Self; 0 $(+ keys!(@one $key))*] = [$(Self::$key,)*];

            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$key => $spelt,)*
                }
            }

            pub const fn switch(self) -> Option<Switch> {
                match self {
                    $(Self::$key => keys!(@switch $key $($default)?),)*
                }
            }
        }

        keys!(@sorted [] [] $($key $($default)?;)*);
    };
    (@one $key:ident) => { 1 };
    (@switch $key:ident) => { None };
    (@switch $key:ident $default:ident) => { Some(Switch::$key) };
    (@on on) => { true };
    (@on off) => { false };
    (@sorted [$($switch:ident $default:ident,)*] [$($valued:ident,)*]) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Switch {
            $($switch,)*
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Valued {
            $($valued,)*
        }

        impl Switch {
            pub const ALL: [Self; 0 $(+ keys!(@one $switch))*] = [$(Self::$switch,)*];

            pub const fn key(self) -> ConfigKey {
                match self {
                    $(Self::$switch => ConfigKey::$switch,)*
                }
            }

            pub const fn on_unless_said(self) -> bool {
                match self {
                    $(Self::$switch => keys!(@on $default),)*
                }
            }
        }

        impl Valued {
            pub const fn key(self) -> ConfigKey {
                match self {
                    $(Self::$valued => ConfigKey::$valued,)*
                }
            }
        }

        impl ConfigKey {
            pub const fn sorted(self) -> Sorted {
                match self {
                    $(Self::$switch => Sorted::Switch(Switch::$switch),)*
                    $(Self::$valued => Sorted::Valued(Valued::$valued),)*
                }
            }
        }
    };
    (@sorted [$($switches:tt)*] [$($valued:tt)*] $key:ident; $($rest:tt)*) => {
        keys!(@sorted [$($switches)*] [$($valued)* $key,] $($rest)*);
    };
    (@sorted [$($switches:tt)*] [$($valued:tt)*] $key:ident $default:ident; $($rest:tt)*) => {
        keys!(@sorted [$($switches)* $key $default,] [$($valued)*] $($rest)*);
    };
}

keys! {
    Sink = "sink";
    Library = "library";
    Vault = "vault";
    Quality = "quality";
    FilterPhase = "filter-phase";
    TruePeak = "true-peak";
    RestoreLossy = "restore-lossy";
    Crossfeed = "crossfeed";
    Dither = "dither";
    NoiseShaping = "noise-shaping";
    ReplayGain = "replay-gain";
    ReplayGainPreAmp = "replay-gain-pre-amp";
    ReplayGainUntagged = "replay-gain-untagged";
    BitPerfect = "bit-perfect";
    Dop = "dop";
    DsdLikePcm = "dsd-like-pcm";
    DeviceVolume = "device-volume";
    ForceGraphRate = "force-graph-rate";
    BluetoothWake = "bluetooth-wake";
    BluetoothLeadMs = "bluetooth-lead-ms";
    BluetoothAwakeS = "bluetooth-awake-s";
    Volume = "volume";
    BufferMs = "buffer-ms";
    Theme = "theme";
    Accent = "accent";
    TextSize = "text-size";
    Language = "language";
    FollowDesktopScheme = "follow-desktop-scheme";
    FollowDesktopTextSize = "follow-desktop-text-size";
    Online = "online", on;
    EnrichAfterScan = "enrich-after-scan", on;
    EnrichOnMetered = "enrich-on-metered", off;
    MusicbrainzMirror = "musicbrainz-mirror", off;
    MusicbrainzReplicationToken = "musicbrainz-replication-token";
    StallDownloadsOnMetered = "stall-downloads-on-metered", off;
    Study = "study", on;
    FetchLyrics = "fetch-lyrics", on;
    LyricsLanguageFromLocale = "lyrics-language-from-locale", off;
    IdentifyBySound = "identify-by-sound", off;
    Contact = "contact";
    Equaliser = "equaliser", off;
    EqualiserFor = "equaliser-for";
    EqualiserMeasurer = "equaliser-measurer";
    EqualiserMeasurerFor = "equaliser-measurer-for";
    EqualiserProfile = "equaliser-profile";
    Convolution = "convolution";
    SpeakerAngle = "speaker-angle";
    SpeakerElevation = "speaker-elevation";
    HeadYaw = "head-yaw";
    Resume = "resume", on;
    Renderers = "renderers", off;
    FlacAbove96kFor = "flac-above-96k-for";
    FormatsLearntFor = "formats-learnt-for";
    RenderersAt = "renderers-at";
    CastAt = "cast-at";
    HistoryKept = "history-kept";
    SkipRepeatsQueue = "skip-repeats-queue";
    PreviousRestarts = "previous-restarts";
    ShuffleStyle = "shuffle-style";
    ShuffleSpreadsArtists = "shuffle-spreads-artists";
    ShuffleSpreadsAlbums = "shuffle-spreads-albums";
    ShuffleFreshFirst = "shuffle-fresh-first";
    OrganiseAs = "organise-as";
    Notify = "notify", on;
    CheckForUpdates = "check-for-updates", on;
    UpdateDismissed = "update-dismissed";
    MinimiseButton = "minimise-button";
    MaximiseButton = "maximise-button";
    ScrollVolume = "scroll-volume", on;
    MouseNavigation = "mouse-navigation", on;
    ReducedMotion = "reduced-motion";
    Scrollbars = "scrollbars";
    SpectrumTilt = "spectrum-tilt";
    SpectrumFloor = "spectrum-floor";
    SpectrumBands = "spectrum-bands";
    SpectrumFalls = "spectrum-falls";
    SuggestionsTab = "suggestions-tab";
    MissingTab = "missing-tab";
    TabCounts = "tab-counts";
    RememberTab = "remember-tab", on;
    LastTab = "last-tab";
    ArtistsDrawn = "artists-drawn";
    RememberWindowSize = "remember-window-size", on;
    WindowSize = "window-size";
    RememberSettingsCategory = "remember-settings-category", on;
    LastSettingsCategory = "last-settings-category";
    Inbox = "inbox";
    MusicFolder = "music-folder";
    MusicExtensions = "music-extensions";
    MinimumLength = "minimum-length-s";
    FileDropped = "file-dropped", on;
    Subsonic = "subsonic";
    SubsonicUser = "subsonic-user";
    SubsonicPassword = "subsonic-password";
    Jellyfin = "jellyfin";
    JellyfinUser = "jellyfin-user";
    JellyfinPassword = "jellyfin-password";
    JellyfinToken = "jellyfin-token";
    JellyfinUserId = "jellyfin-user-id";
    JellyfinDevice = "jellyfin-device";
    TidalClientId = "tidal-client-id";
    TidalClientSecret = "tidal-client-secret";
    TidalRefreshToken = "tidal-refresh-token";
    SpotifyClientId = "spotify-client-id";
    SpotifyRefreshToken = "spotify-refresh-token";
    HifiApi = "hifi-api";
    HifiSecret = "hifi-secret";
    Monochrome = "monochrome";
    HostedServices = "hosted-services", on;
    YoutubeMusic = "youtube-music", off;
    YoutubeMusicCookies = "youtube-music-cookies";
    YtDlp = "yt-dlp";
    YoutubeMusicKeepOrder = "youtube-music-keep-order", on;
    AcoustidKey = "acoustid-key";
    AuddToken = "audd-token";
    ListenbrainzToken = "listenbrainz-token";
    LastfmKey = "lastfm-key";
    LastfmSecret = "lastfm-secret";
    LastfmSession = "lastfm-session";
    ListenFrom = "listen-from";
    ListenFor = "listen-for";
    Discord = "discord";
    DiscordApp = "discord-app";
    DiscordShows = "discord-shows";
    DiscordArt = "discord-art";
    DiscordIcon = "discord-icon";
    DiscordProgress = "discord-progress";
    DiscordPaused = "discord-paused";
    Sync = "sync", on;
    SyncNearbyOnly = "sync-nearby-only", off;
    SyncServer = "sync-server";
    SyncServerCertificate = "sync-server-certificate";
    SyncStun = "sync-stun";
    SyncKey = "sync-key";
    SyncDevice = "sync-device";
    SharedName = "shared-name";
    SyncSongs = "sync-songs";
    SyncControl = "sync-control";
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sorted {
    Switch(Switch),
    Valued(Valued),
}

impl ConfigKey {
    pub const fn on_unless_said(self) -> Option<bool> {
        match self.switch() {
            Some(switch) => Some(switch.on_unless_said()),
            None => None,
        }
    }

    pub const fn switched(self, said: Option<bool>) -> bool {
        match (said, self.on_unless_said()) {
            (Some(said), _) => said,
            (None, Some(on)) => on,
            (None, None) => false,
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|candidate| candidate.as_str() == key)
    }
}

impl Switch {
    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn as_str(self) -> &'static str {
        self.key().as_str()
    }

    pub const fn switched(self, said: Option<bool>) -> bool {
        match said {
            Some(said) => said,
            None => self.on_unless_said(),
        }
    }
}

impl fmt::Display for ConfigKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for Switch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Switches {
    said: [Option<bool>; Switch::ALL.len()],
}

impl Default for Switches {
    fn default() -> Self {
        Self::UNSAID
    }
}

impl Switches {
    pub const UNSAID: Self = Self {
        said: [None; Switch::ALL.len()],
    };

    pub const fn said(&self, switch: Switch) -> Option<bool> {
        self.said[switch.index()]
    }

    pub const fn is_on(&self, switch: Switch) -> bool {
        switch.switched(self.said(switch))
    }

    pub const fn say(&mut self, switch: Switch, on: bool) {
        self.said[switch.index()] = Some(on);
    }

    pub const fn forget(&mut self, switch: Switch) {
        self.said[switch.index()] = None;
    }

    #[must_use]
    pub const fn with(mut self, switch: Switch, on: bool) -> Self {
        self.say(switch, on);
        self
    }

    pub fn as_said(&self) -> impl Iterator<Item = (Switch, bool)> + '_ {
        Switch::ALL
            .into_iter()
            .filter_map(|switch| self.said(switch).map(|on| (switch, on)))
    }
}

impl fmt::Debug for Switches {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.as_said()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{ConfigKey, Sorted, Switch, Switches};

    #[test]
    fn a_switch_is_what_was_said_and_else_what_it_is_unless_said() {
        assert!(ConfigKey::Online.switched(None));
        assert!(!ConfigKey::Online.switched(Some(false)));
        assert!(!ConfigKey::EnrichOnMetered.switched(None));
        assert!(ConfigKey::EnrichOnMetered.switched(Some(true)));
        assert_eq!(ConfigKey::Sink.on_unless_said(), None);
    }

    #[test]
    fn every_switch_is_the_key_it_names_and_only_a_switch_names_a_default() {
        for switch in Switch::ALL {
            assert_eq!(switch.key().switch(), Some(switch));
            assert_eq!(switch.as_str(), switch.key().as_str());
            assert_eq!(switch.key().sorted(), Sorted::Switch(switch));
        }
        for key in ConfigKey::ALL {
            assert_eq!(key.switch().is_some(), key.on_unless_said().is_some());
            match key.sorted() {
                Sorted::Switch(switch) => assert_eq!(switch.key(), key),
                Sorted::Valued(valued) => assert_eq!(valued.key(), key),
            }
        }
    }

    #[test]
    fn switches_hold_what_was_said_apart_from_what_was_not() {
        let mut switches = Switches::UNSAID.with(Switch::Online, false);

        assert!(!switches.is_on(Switch::Online));
        assert!(switches.is_on(Switch::Resume));
        assert_eq!(switches.said(Switch::Resume), None);
        assert_eq!(
            switches.as_said().collect::<Vec<_>>(),
            [(Switch::Online, false)]
        );

        switches.forget(Switch::Online);

        assert_eq!(switches, Switches::UNSAID);
    }
}
