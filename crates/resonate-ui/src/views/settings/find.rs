use std::cmp::Reverse;

pub(crate) use crate::SettingsCategory as Category;
use crate::{SettingKey, icons::Icon, system::sound_system};

impl Category {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Output => "Output",
            Self::Processing => "Processing",
            Self::Equaliser => "Equaliser",
            Self::Playback => "Playback",
            Self::Library => "Library",
            Self::Filters => "Filters",
            Self::Online => "Online",
            Self::Recognition => "Recognition",
            Self::Accounts => "Accounts",
            Self::Sync => "Sync",
            Self::Desktop => "Desktop",
            Self::Appearance => "Appearance",
            Self::Window => "Window",
            Self::About => "About",
        }
    }

    pub(crate) const fn about(self) -> &'static str {
        match self {
            Self::Output => {
                "The device, the rate the stream opens at and how much is held ahead of it"
            }
            Self::Processing => "What may touch the samples between the decoder and the sink",
            Self::Equaliser => {
                "The correction applied to each device, and where its curves come from"
            }
            Self::Playback => "What the queue keeps between runs, and what repeat and previous do",
            Self::Library => {
                "The folders a scan walks, keeping the catalog up to date, and how the files are \
                 filed and kept"
            }
            Self::Filters => "Which file extensions and track lengths appear in your library",
            Self::Online => {
                "Whether the player may reach the network, what a request says about you, and what \
                 is looked up when"
            }
            Self::Recognition => {
                "Studying tracks, and naming a song by ear from the desktop or a microphone"
            }
            Self::Accounts => {
                "Music services, sign-ins, server credentials and where your plays are scrobbled"
            }
            Self::Sync => {
                "Your other devices kept in step: pairing them, who is there, how they meet and \
                 which songs travel"
            }
            Self::Desktop => "What this player tells the rest of the session about itself",
            Self::Appearance => {
                "The palette the window is painted in, how large it is drawn, how it moves and \
                 how its scrollbars and spectrum read"
            }
            Self::Window => {
                "What the titlebar and the sidebar carry, what the window remembers, and what the \
                 wheel and the mouse's side buttons do"
            }
            Self::About => "What this build is, where it keeps things, and how to put it back",
        }
    }

    pub(crate) const fn icon(self) -> Icon {
        match self {
            Self::Output => Icon::Volume,
            Self::Processing => Icon::Inspector,
            Self::Equaliser => Icon::Equaliser,
            Self::Playback => Icon::Play,
            Self::Library => Icon::Folder,
            Self::Filters => Icon::Search,
            Self::Online => Icon::Globe,
            Self::Recognition => Icon::Listen,
            Self::Accounts => Icon::Server,
            Self::Sync => Icon::Sync,
            Self::Desktop => Icon::Computer,
            Self::Appearance => Icon::Appearance,
            Self::Window => Icon::WindowMaximise,
            Self::About => Icon::Info,
        }
    }

    pub(crate) fn groups(self) -> impl Iterator<Item = Group> {
        Group::ALL
            .into_iter()
            .filter(move |group| group.category() == self && group.is_offered())
    }

    pub(crate) fn shown_on(self, page: Option<Page>) -> impl Iterator<Item = Group> {
        self.groups().filter(move |group| group.page() == page)
    }

    pub(crate) fn drawn(self) -> Vec<Drawn> {
        let mut drawn: Vec<Drawn> = Vec::new();
        for group in self.groups() {
            match group.page() {
                None => drawn.push(Drawn::Group(group)),
                Some(page) => {
                    let gathered = drawn.iter().any(|held| match held {
                        Drawn::Pages(pages) => pages.contains(&page),
                        Drawn::Group(_) => false,
                    });
                    if gathered {
                        continue;
                    }
                    match drawn.last_mut() {
                        Some(Drawn::Pages(pages))
                            if pages.first().is_some_and(|first| {
                                first.gathered_under() == page.gathered_under()
                            }) =>
                        {
                            pages.push(page);
                        }
                        _ => drawn.push(Drawn::Pages(vec![page])),
                    }
                }
            }
        }
        drawn
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Drawn {
    Group(Group),
    Pages(Vec<Page>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Page {
    HeadphoneCorrection,
    OwnMeasurement,
    RoomMeasurement,
    RoomCorrection,
    ListeningTest,
    Enrichment,
    Devices,
}

impl Page {
    #[cfg(test)]
    pub(crate) const ALL: [Self; 7] = [
        Self::HeadphoneCorrection,
        Self::OwnMeasurement,
        Self::RoomMeasurement,
        Self::RoomCorrection,
        Self::ListeningTest,
        Self::Enrichment,
        Self::Devices,
    ];

    pub(crate) const fn category(self) -> Category {
        match self {
            Self::HeadphoneCorrection
            | Self::OwnMeasurement
            | Self::RoomMeasurement
            | Self::RoomCorrection
            | Self::ListeningTest => Category::Equaliser,
            Self::Enrichment => Category::Library,
            Self::Devices => Category::Sync,
        }
    }

    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::HeadphoneCorrection => "Headphone correction",
            Self::OwnMeasurement => "Your own measurement",
            Self::RoomMeasurement => "Measure with a microphone",
            Self::RoomCorrection => "Room correction",
            Self::ListeningTest => "Listening test",
            Self::Enrichment => "Rescan, refresh and retag",
            Self::Devices => "Your devices",
        }
    }

    pub(crate) const fn about(self) -> &'static str {
        match self {
            Self::HeadphoneCorrection => {
                "AutoEq's measured corrections, the one for what is plugged in, and whose to prefer"
            }
            Self::OwnMeasurement => "A response measured elsewhere, fitted toward a target",
            Self::RoomMeasurement => "A sweep played through the output and heard back",
            Self::RoomCorrection => "An impulse response, or speakers placed around your head",
            Self::ListeningTest => "Hear whether a setting can be told apart, blind",
            Self::Enrichment => "Read the files again, ask what online knows, write tags back",
            Self::Devices => "Who shares the code, and driving the zones of another",
        }
    }

    pub(crate) const fn icon(self) -> Icon {
        match self {
            Self::HeadphoneCorrection => Icon::Download,
            Self::OwnMeasurement => Icon::Analysis,
            Self::RoomMeasurement => Icon::Listen,
            Self::ListeningTest => Icon::Check,
            Self::RoomCorrection => Icon::Volume,
            Self::Enrichment => Icon::Redo,
            Self::Devices => Icon::Phone,
        }
    }

    pub(crate) const fn gathered_under(self) -> &'static str {
        match self {
            Self::HeadphoneCorrection
            | Self::OwnMeasurement
            | Self::RoomMeasurement
            | Self::RoomCorrection
            | Self::ListeningTest => "Corrections and tests",
            Self::Enrichment => "Keeping the catalog current",
            Self::Devices => "Your other devices",
        }
    }

    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::HeadphoneCorrection => "headphone-correction",
            Self::OwnMeasurement => "own-measurement",
            Self::RoomMeasurement => "room-measurement",
            Self::RoomCorrection => "room-correction",
            Self::ListeningTest => "listening-test",
            Self::Enrichment => "enrichment",
            Self::Devices => "devices",
        }
    }

    #[cfg(test)]
    pub(crate) fn groups(self) -> impl Iterator<Item = Group> {
        Group::ALL
            .into_iter()
            .filter(move |group| group.page() == Some(self) && group.is_offered())
    }
}

pub(crate) struct Described {
    pub(crate) category: Category,
    pub(crate) page: Option<Page>,
    pub(crate) title: &'static str,
    pub(crate) hint: &'static str,
    pub(crate) also_called: &'static str,
    pub(crate) keys: &'static [SettingKey],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Group {
    Device,
    DeviceVolume,
    SampleRate,
    GraphRate,
    Buffer,
    Dop,
    Bluetooth,
    FlacRenderers,
    Resampler,
    Dither,
    NoiseShaping,
    ReplayGain,
    TruePeak,
    LossySources,
    Crossfeed,
    Equalising,
    BoundTo,
    Bands,
    AutoEq,
    Measurers,
    OwnMeasurement,
    RoomMeasurement,
    RoomCorrection,
    Speakers,
    Abx,
    Resuming,
    Repeating,
    PreviousButton,
    Folders,
    Rescanning,
    LookingUp,
    Retagging,
    Organising,
    Vault,
    Inbox,
    History,
    MusicExtensions,
    MinimumLength,
    Hidden,
    Lookups,
    AfterScan,
    Metered,
    Lyrics,
    Contact,
    Studies,
    Recognition,
    Listening,
    ResonateKey,
    Subsonic,
    Jellyfin,
    Tidal,
    YoutubeMusic,
    Spotify,
    Lastfm,
    Submitting,
    Syncing,
    Pairing,
    SyncDevices,
    SyncControl,
    SyncConnection,
    SyncedSongs,
    Notifications,
    Discord,
    DiscordShows,
    Colour,
    Layout,
    Motion,
    Scrollbars,
    LiveSpectrum,
    WindowButtons,
    Tabs,
    WindowState,
    VolumeWheel,
    MouseNavigation,
    Build,
    Places,
    Everything,
}

impl Group {
    pub(crate) const ALL: [Self; 77] = [
        Self::Device,
        Self::DeviceVolume,
        Self::SampleRate,
        Self::GraphRate,
        Self::Buffer,
        Self::Dop,
        Self::Bluetooth,
        Self::FlacRenderers,
        Self::Resampler,
        Self::Dither,
        Self::NoiseShaping,
        Self::ReplayGain,
        Self::TruePeak,
        Self::LossySources,
        Self::Crossfeed,
        Self::Equalising,
        Self::BoundTo,
        Self::Bands,
        Self::AutoEq,
        Self::Measurers,
        Self::OwnMeasurement,
        Self::RoomMeasurement,
        Self::RoomCorrection,
        Self::Speakers,
        Self::Abx,
        Self::Resuming,
        Self::Repeating,
        Self::PreviousButton,
        Self::Folders,
        Self::Rescanning,
        Self::LookingUp,
        Self::Retagging,
        Self::Organising,
        Self::Vault,
        Self::Inbox,
        Self::History,
        Self::MusicExtensions,
        Self::MinimumLength,
        Self::Hidden,
        Self::Lookups,
        Self::AfterScan,
        Self::Metered,
        Self::Lyrics,
        Self::Contact,
        Self::Studies,
        Self::Recognition,
        Self::Listening,
        Self::ResonateKey,
        Self::Subsonic,
        Self::Jellyfin,
        Self::Tidal,
        Self::YoutubeMusic,
        Self::Spotify,
        Self::Lastfm,
        Self::Submitting,
        Self::Syncing,
        Self::Pairing,
        Self::SyncDevices,
        Self::SyncControl,
        Self::SyncConnection,
        Self::SyncedSongs,
        Self::Notifications,
        Self::Discord,
        Self::DiscordShows,
        Self::Colour,
        Self::Layout,
        Self::Motion,
        Self::Scrollbars,
        Self::LiveSpectrum,
        Self::WindowButtons,
        Self::Tabs,
        Self::WindowState,
        Self::VolumeWheel,
        Self::MouseNavigation,
        Self::Build,
        Self::Places,
        Self::Everything,
    ];

    pub(crate) const fn is_offered(self) -> bool {
        !cfg!(windows) || !matches!(self, Self::GraphRate | Self::Bluetooth)
    }

    pub(crate) const fn is_experimental(self) -> bool {
        matches!(self, Self::LossySources)
    }

    pub(crate) const fn category(self) -> Category {
        self.described().category
    }

    pub(crate) const fn page(self) -> Option<Page> {
        self.described().page
    }

    pub(crate) const fn title(self) -> &'static str {
        self.described().title
    }

    pub(crate) const fn hint(self) -> &'static str {
        self.described().hint
    }

    pub(crate) const fn also_called(self) -> &'static str {
        self.described().also_called
    }

    pub(crate) const fn keys(self) -> &'static [SettingKey] {
        self.described().keys
    }

    pub(crate) const fn described(self) -> Described {
        match self {
            Self::Device => Described {
                category: Category::Output,
                page: None,
                title: "Playback device",
                hint: DEVICE_HINT,
                also_called: "sink output card dac headphones speakers pipewire wasapi exclusive profile pro audio upnp dlna \
                 renderer network streamer chromecast google cast receiver",
                keys: &[SettingKey::Sink, SettingKey::Renderers],
            },
            Self::DeviceVolume => Described {
                category: Category::Output,
                page: None,
                title: "Hardware volume",
                hint: DEVICE_VOLUME_HINT,
                also_called: "hardware mixer amplifier alsa route loudness slider system volume bit perfect \
                 attenuation",
                keys: &[SettingKey::DeviceVolume],
            },
            Self::SampleRate => Described {
                category: Category::Output,
                page: None,
                title: "Sample rate",
                hint: SAMPLE_RATE_HINT,
                also_called: "bit perfect khz hz resample untouched",
                keys: &[SettingKey::BitPerfect],
            },
            Self::GraphRate => Described {
                category: Category::Output,
                page: None,
                title: "Graph rate",
                hint: GRAPH_RATE_HINT,
                also_called: "pipewire daemon switch rate",
                keys: &[SettingKey::ForceGraphRate],
            },
            Self::Buffer => Described {
                category: Category::Output,
                page: None,
                title: "Buffer",
                hint: BUFFER_HINT,
                also_called: "latency milliseconds ms depth ring underrun",
                keys: &[SettingKey::Buffer],
            },
            Self::Dop => Described {
                category: Category::Output,
                page: None,
                title: "DSD over PCM",
                hint: DOP_HINT,
                also_called: "dsd dsf dff sacd marker packing level quieter louder six decibels 6 db",
                keys: &[SettingKey::Dop, SettingKey::DsdLikePcm],
            },
            Self::Bluetooth => Described {
                category: Category::Output,
                page: None,
                title: "Bluetooth",
                hint: BLUETOOTH_HINT,
                also_called: "headphones earbuds airpods a2dp wireless cut off clipped start first second \
                 wake sleep idle power save experimental",
                keys: &[
                    SettingKey::BluetoothWake,
                    SettingKey::BluetoothLead,
                    SettingKey::BluetoothAwake,
                ],
            },
            Self::FlacRenderers => Described {
                category: Category::Output,
                page: None,
                title: "High-rate FLAC to renderers",
                hint: FLAC_RENDERERS_HINT,
                also_called: "upnp dlna renderer streamer network flac 96 176 192 khz high resolution rate \
                 vouch decode lossless wave uncompressed",
                keys: &[SettingKey::FlacAbove96kFor],
            },
            Self::Resampler => Described {
                category: Category::Processing,
                page: None,
                title: "Resampler",
                hint: RESAMPLER_HINT,
                also_called: "quality sinc kaiser taps filter conversion phase minimum intermediate pre-ringing",
                keys: &[SettingKey::Quality, SettingKey::FilterPhase],
            },
            Self::Dither => Described {
                category: Category::Processing,
                page: None,
                title: "Dither",
                hint: DITHER_HINT,
                also_called: "truncation triangular rectangular noise",
                keys: &[SettingKey::Dither],
            },
            Self::NoiseShaping => Described {
                category: Category::Processing,
                page: None,
                title: "Noise shaping",
                hint: SHAPING_HINT,
                also_called: "lipshitz threshold hearing terhardt e-weighted psychoacoustic curve shaped",
                keys: &[SettingKey::NoiseShaping],
            },
            Self::ReplayGain => Described {
                category: Category::Processing,
                page: None,
                title: "ReplayGain",
                hint: REPLAY_GAIN_HINT,
                also_called: "loudness volume normalisation gain album track pre-amp untagged",
                keys: &[
                    SettingKey::ReplayGain,
                    SettingKey::PreAmp,
                    SettingKey::Untagged,
                ],
            },
            Self::TruePeak => Described {
                category: Category::Processing,
                page: None,
                title: "True peak",
                hint: TRUE_PEAK_HINT,
                also_called: "clipping intersample over limiter dbtp headroom lossy",
                keys: &[SettingKey::TruePeak],
            },
            Self::LossySources => Described {
                category: Category::Processing,
                page: None,
                title: "Artificially enhance lossy files",
                hint: LOSSY_SOURCES_HINT,
                also_called: "lossy sources mp3 aac vorbis restore restoration repair enhance enhancement \
                 artificial experimental bandwidth extension sbr dsee holes swirly cider",
                keys: &[SettingKey::Restoration],
            },
            Self::Crossfeed => Described {
                category: Category::Processing,
                page: None,
                title: "Crossfeed",
                hint: CROSSFEED_HINT,
                also_called: "crossfeed bauer headphones stereo image binaural speakers narrow soundstage \
                 bs2b meier chu moy fatigue",
                keys: &[SettingKey::Crossfeed],
            },
            Self::Equalising => Described {
                category: Category::Equaliser,
                page: None,
                title: "Equaliser",
                hint: EQUALISING_HINT,
                also_called: "eq equalizer parametric filter correction headphone tone \
                                 bit-perfect",
                keys: &[SettingKey::Equaliser],
            },
            Self::BoundTo => Described {
                category: Category::Equaliser,
                page: None,
                title: "Bound to each device",
                hint: BOUND_TO_HINT,
                also_called: "sink device per-device default binding preset assign headphones own custom",
                keys: &[SettingKey::EqualiserFor],
            },
            Self::Bands => Described {
                category: Category::Equaliser,
                page: None,
                title: "Bands",
                hint: BANDS_HINT,
                also_called: "curve response graph frequency gain q peaking shelf preamp band drag handle \
                 custom own point",
                keys: &[],
            },
            Self::AutoEq => Described {
                category: Category::Equaliser,
                page: Some(Page::HeadphoneCorrection),
                title: "AutoEq",
                hint: AUTOEQ_HINT,
                also_called: "autoeq oratory1990 crinacle harman target download fetch measurement rig suggest plugged in headphones earphones",
                keys: &[],
            },
            Self::Measurers => Described {
                category: Category::Equaliser,
                page: Some(Page::HeadphoneCorrection),
                title: "Preferred measurers",
                hint: MEASURERS_HINT,
                also_called: "measurer oratory1990 crinacle rtings rig ranking prefer best ranked",
                keys: &[
                    SettingKey::EqualiserMeasurer,
                    SettingKey::EqualiserMeasurerFor,
                ],
            },
            Self::OwnMeasurement => Described {
                category: Category::Equaliser,
                page: Some(Page::OwnMeasurement),
                title: "Fit a measurement",
                hint: OWN_MEASUREMENT_HINT,
                also_called: "own measurement rew fit csv response flat target harman file",
                keys: &[],
            },
            Self::RoomMeasurement => Described {
                category: Category::Equaliser,
                page: Some(Page::RoomMeasurement),
                title: "Measure your room",
                hint: ROOM_MEASUREMENT_HINT,
                also_called: "microphone mic sweep room measure calibration rew umik desktop record",
                keys: &[],
            },
            Self::RoomCorrection => Described {
                category: Category::Equaliser,
                page: Some(Page::RoomCorrection),
                title: "The room's response",
                hint: ROOM_HINT,
                also_called: "convolution impulse response ir fir room correction speakers rew drc measurement wav sofa",
                keys: &[SettingKey::Convolution],
            },
            Self::Speakers => Described {
                category: Category::Equaliser,
                page: Some(Page::RoomCorrection),
                title: "Where the speakers stand",
                hint: SPEAKERS_HINT,
                also_called: "sofa hrir hrtf angle degrees speaker elevation yaw head turned binaural brir",
                keys: &[
                    SettingKey::SpeakerAngle,
                    SettingKey::SpeakerElevation,
                    SettingKey::HeadYaw,
                ],
            },
            Self::Abx => Described {
                category: Category::Equaliser,
                page: Some(Page::ListeningTest),
                title: "A blind test",
                hint: ABX_HINT,
                also_called: "abx blind test listening level matched equaliser bypass double blind \
                 audible difference trial p value restoration resampler dither measuring",
                keys: &[],
            },
            Self::Resuming => Described {
                category: Category::Playback,
                page: None,
                title: "Resuming",
                hint: RESUMING_HINT,
                also_called: "queue restart restore carry on position where left off",
                keys: &[SettingKey::Resume],
            },
            Self::Repeating => Described {
                category: Category::Playback,
                page: None,
                title: "Repeating a track",
                hint: REPEATING_HINT,
                also_called: "loop repeat one single track skip next previous spotify",
                keys: &[SettingKey::SkipUnderRepeat],
            },
            Self::PreviousButton => Described {
                category: Category::Playback,
                page: None,
                title: "The previous button",
                hint: PREVIOUS_HINT,
                also_called: "restart replay beginning start over back previous button spotify apple first \
                 seconds opening",
                keys: &[SettingKey::PreviousRestarts],
            },
            Self::Folders => Described {
                category: Category::Library,
                page: None,
                title: "Music folders",
                hint: FOLDERS_HINT,
                also_called: "roots directories add scan music path primary main default home destination \
                 import copy drop drag songs files library outside vault file organise layout \
                 covers lyrics",
                keys: &[SettingKey::MusicFolder, SettingKey::FileDropped],
            },
            Self::Rescanning => Described {
                category: Category::Library,
                page: Some(Page::Enrichment),
                title: "The files",
                hint: RESCANNING_HINT,
                also_called: "rescan scan stop reindex re-read reread rebuild length duration timestamps modified cue sheet search index database",
                keys: &[],
            },
            Self::LookingUp => Described {
                category: Category::Library,
                page: Some(Page::Enrichment),
                title: "What online knows",
                hint: LOOKING_UP_HINT,
                also_called: "enrich look up refresh all force musicbrainz covers portraits missing artwork speed up hurry stop lookup",
                keys: &[],
            },
            Self::Retagging => Described {
                category: Category::Library,
                page: Some(Page::Enrichment),
                title: "The tags in the files",
                hint: RETAGGING_HINT,
                also_called: "write back retag tags metadata ids embed files preview apply put back",
                keys: &[],
            },
            Self::Organising => Described {
                category: Category::Library,
                page: None,
                title: "Organising",
                hint: ORGANISING_HINT,
                also_called: "tidy rename move layout template pattern folder structure filename",
                keys: &[SettingKey::OrganiseAs],
            },
            Self::Vault => Described {
                category: Category::Library,
                page: None,
                title: "The vault",
                hint: VAULT_HINT,
                also_called: "archive import re-encode recompress flac strip metadata covers jxl \
                              managed store deduplicate validate folder location choose",
                keys: &[],
            },
            Self::Inbox => Described {
                category: Category::Library,
                page: None,
                title: "The inbox",
                hint: INBOX_HINT,
                also_called: "provider poll wants missing download drop folder fill obtain supply",
                keys: &[SettingKey::Inbox],
            },
            Self::History => Described {
                category: Category::Library,
                page: None,
                title: "Listening history",
                hint: HISTORY_HINT,
                also_called: "history listens plays statistics forget age prune older keep years months \
                 privacy",
                keys: &[SettingKey::HistoryKept],
            },
            Self::MusicExtensions => Described {
                category: Category::Filters,
                page: None,
                title: "File extensions",
                hint: "Choose which supported file extensions appear in the library. Every supported extension is included by default. Files and catalog entries are kept when excluded.",
                also_called: "music filters formats mp3 flac m4a audio files extension include exclude",
                keys: &[SettingKey::MusicExtensions],
            },
            Self::MinimumLength => Described {
                category: Category::Filters,
                page: None,
                title: "Minimum length",
                hint: "Only show tracks at least this long. The minimum can be between 0 seconds and 10 minutes. Zero keeps the current behavior, including tracks whose length is unknown.",
                also_called: "music filters duration seconds minutes short songs minimum length",
                keys: &[SettingKey::MinimumLength],
            },
            Self::Hidden => Described {
                category: Category::Filters,
                page: None,
                title: "Hidden songs",
                hint: "The songs taken out of the listings with Hide. Show one again, or every one at once; nothing about them was lost.",
                also_called: "hidden hide unhide show again songs tracks removed taken out listings undo",
                keys: &[],
            },
            Self::Lookups => Described {
                category: Category::Online,
                page: None,
                title: "Reference lookups",
                hint: ONLINE_HINT,
                also_called: "musicbrainz network internet covers lyrics lrclib better lyrics betterlyrics offline \
                 mirror dump database local",
                keys: &[SettingKey::Online, SettingKey::MusicbrainzMirror],
            },
            Self::AfterScan => Described {
                category: Category::Online,
                page: None,
                title: "After a scan",
                hint: AFTER_SCAN_HINT,
                also_called: "enrich automatic handover",
                keys: &[SettingKey::EnrichAfterScan],
            },
            Self::Metered => Described {
                category: Category::Online,
                page: None,
                title: "Metered connections",
                hint: METERED_HINT,
                also_called: "metered mobile data hotspot tethering bandwidth cap networkmanager download \
                 stall lookups enrich wait pause",
                keys: &[
                    SettingKey::EnrichOnMetered,
                    SettingKey::StallDownloadsOnMetered,
                ],
            },
            Self::Lyrics => Described {
                category: Category::Online,
                page: None,
                title: "Fetching lyrics",
                hint: LYRICS_HINT,
                also_called: "lyrics lrclib better lyrics betterlyrics ttml lyricsfile synced karaoke words voices \
                 singers duet cache upgrade language locale",
                keys: &[SettingKey::FetchLyrics, SettingKey::LyricsByTheLocale],
            },
            Self::Contact => Described {
                category: Category::Online,
                page: None,
                title: "Contact",
                hint: CONTACT_HINT,
                also_called: "user-agent email identity request",
                keys: &[SettingKey::Contact],
            },
            Self::Studies => Described {
                category: Category::Recognition,
                page: None,
                title: "Studying tracks",
                hint: STUDIES_HINT,
                also_called: "analyse analysis decode fake lossless transcode verdict loudness true peak \
                 fingerprint cpu cores shazam sound identify unnamed untagged",
                keys: &[SettingKey::Study, SettingKey::IdentifyBySound],
            },
            Self::Recognition => Described {
                category: Category::Recognition,
                page: None,
                title: "Recognition",
                hint: RECOGNITION_HINT,
                also_called: "acoustid audd key token fingerprint chromaprint identify recognise unnamed \
                 misnamed",
                keys: &[SettingKey::AcoustidKey, SettingKey::AuddToken],
            },
            Self::Listening => Described {
                category: Category::Recognition,
                page: None,
                title: "Listening",
                hint: LISTENING_HINT,
                also_called: "listen shazam microphone mic desktop monitor what is playing identify song \
                 name that tune seconds clip",
                keys: &[SettingKey::ListenFrom, SettingKey::ListenFor],
            },
            Self::ResonateKey => Described {
                category: Category::Accounts,
                page: None,
                title: "Resonate server key",
                hint: RESONATE_KEY_HINT,
                also_called: "resonate server key secret hifi-api hifi token password download wants missing \
                 fill obtain account bearer tecnio gateway",
                keys: &[SettingKey::HifiSecret],
            },
            Self::Subsonic => Described {
                category: Category::Accounts,
                page: None,
                title: "A Subsonic server",
                hint: SUBSONIC_HINT,
                also_called: "navidrome airsonic gonic server provider download wants missing stream fill \
                 obtain account password",
                keys: &[
                    SettingKey::Subsonic,
                    SettingKey::SubsonicUser,
                    SettingKey::SubsonicPassword,
                ],
            },
            Self::Jellyfin => Described {
                category: Category::Accounts,
                page: None,
                title: "A Jellyfin or Emby server",
                hint: JELLYFIN_HINT,
                also_called: "emby server provider stream play in place browse account password token",
                keys: &[
                    SettingKey::Jellyfin,
                    SettingKey::JellyfinUser,
                    SettingKey::JellyfinPassword,
                ],
            },
            Self::Tidal => Described {
                category: Category::Accounts,
                page: None,
                title: "A TIDAL account",
                hint: TIDAL_HINT,
                also_called: "tidal streaming service subscription provider download wants missing fill \
                 obtain account token client lossless hi-res flac hifi-api hifi homelab monochrome",
                keys: &[
                    SettingKey::TidalClientId,
                    SettingKey::TidalClientSecret,
                    SettingKey::TidalRefreshToken,
                    SettingKey::HifiApi,
                    SettingKey::Monochrome,
                    SettingKey::HostedServices,
                ],
            },
            Self::YoutubeMusic => Described {
                category: Category::Accounts,
                page: None,
                title: "YouTube Music",
                hint: YOUTUBE_MUSIC_HINT,
                also_called: "youtube music ytmusic yt-dlp ytdlp cookies premium google account browser firefox \
                 zen opus provider download wants missing fill obtain terms check connection \
                 playlist order",
                keys: &[
                    SettingKey::YoutubeMusic,
                    SettingKey::YoutubeMusicCookies,
                    SettingKey::YtDlp,
                    SettingKey::YoutubeMusicKeepOrder,
                ],
            },
            Self::Spotify => Described {
                category: Category::Accounts,
                page: None,
                title: "A Spotify account",
                hint: SPOTIFY_HINT,
                also_called: "spotify account sign in playlists liked songs import move follow keep in step \
                 client id redirect isrc",
                keys: &[SettingKey::SpotifyClientId, SettingKey::SpotifyRefreshToken],
            },
            Self::Lastfm => Described {
                category: Category::Accounts,
                page: None,
                title: "Last.fm",
                hint: LASTFM_HINT,
                also_called: "scrobble scrobbling last.fm lastfm audioscrobbler sign in log in session api key \
                 secret user name password account",
                keys: &[
                    SettingKey::LastfmKey,
                    SettingKey::LastfmSecret,
                    SettingKey::LastfmSession,
                ],
            },
            Self::Submitting => Described {
                category: Category::Accounts,
                page: None,
                title: "ListenBrainz",
                hint: SUBMITTING_HINT,
                also_called: "scrobble scrobbling listens history submit plays heard token profile statistics",
                keys: &[SettingKey::ListenbrainzToken],
            },
            Self::Syncing => Described {
                category: Category::Sync,
                page: None,
                title: "Device sync",
                hint: SYNCING_HINT,
                also_called: "sync synchronise synchronize devices playlists favourites history handoff now \
                 status",
                keys: &[SettingKey::Sync],
            },
            Self::Pairing => Described {
                category: Category::Sync,
                page: None,
                title: "Pairing",
                hint: PAIRING_HINT,
                also_called: "pair pairing code qr scan camera join regenerate new code phone iphone android \
                 link",
                keys: &[],
            },
            Self::SyncDevices => Described {
                category: Category::Sync,
                page: Some(Page::Devices),
                title: "Devices",
                hint: SYNC_DEVICES_HINT,
                also_called: "devices phone iphone android computer remove forget stale old",
                keys: &[],
            },
            Self::SyncControl => Described {
                category: Category::Sync,
                page: Some(Page::Devices),
                title: resonate_core::REMOTE_TRUST_TITLE,
                hint: SYNC_CONTROL_HINT,
                also_called: "drive driven remote control another device zones queue transport trust consent \
                 play pause skip seek volume",
                keys: &[SettingKey::SyncControl],
            },
            Self::SyncConnection => Described {
                category: Category::Sync,
                page: None,
                title: "Connection",
                hint: SYNC_CONNECTION_HINT,
                also_called: "server self hosted own nearby network lan local wifi offline name device",
                keys: &[
                    SettingKey::SyncNearbyOnly,
                    SettingKey::SyncServer,
                    SettingKey::SyncDevice,
                    SettingKey::SharedName,
                ],
            },
            Self::SyncedSongs => Described {
                category: Category::Sync,
                page: None,
                title: "Songs between devices",
                hint: SYNCED_SONGS_HINT,
                also_called: "sync copy transfer download songs files phone android peer playlists favourites \
                 everything automatic",
                keys: &[SettingKey::SyncSongs],
            },
            Self::Notifications => Described {
                category: Category::Desktop,
                page: None,
                title: "Notifications",
                hint: NOTIFICATIONS_HINT,
                also_called: "notify popup toast banner song track change desktop osd",
                keys: &[SettingKey::Notify],
            },
            Self::Discord => Described {
                category: Category::Desktop,
                page: None,
                title: "Discord",
                hint: DISCORD_HINT,
                also_called: "rich presence status activity listening profile application id client",
                keys: &[SettingKey::Discord, SettingKey::DiscordApp],
            },
            Self::DiscordShows => Described {
                category: Category::Desktop,
                page: None,
                title: "What Discord shows",
                hint: DISCORD_SHOWS_HINT,
                also_called: "rich presence status activity cover art album icon asset progress bar elapsed \
                 paused details",
                keys: &[
                    SettingKey::DiscordShows,
                    SettingKey::DiscordArt,
                    SettingKey::DiscordIcon,
                    SettingKey::DiscordProgress,
                    SettingKey::DiscordPaused,
                ],
            },
            Self::Colour => Described {
                category: Category::Appearance,
                page: None,
                title: "Colour",
                hint: COLOUR_HINT,
                also_called: "theme accent palette dark light day night swatch high contrast contrast accessibility follow desktop scheme",
                keys: &[
                    SettingKey::Theme,
                    SettingKey::Accent,
                    SettingKey::FollowDesktopScheme,
                ],
            },
            Self::Layout => Described {
                category: Category::Appearance,
                page: None,
                title: "Layout",
                hint: LAYOUT_HINT,
                also_called: "text size type scale larger largest smaller density follow desktop scaling",
                keys: &[SettingKey::TextSize, SettingKey::FollowDesktopTextSize],
            },
            Self::Motion => Described {
                category: Category::Appearance,
                page: None,
                title: "Motion",
                hint: MOTION_HINT,
                also_called: "animation animations reduced reduce motion fade slide glide transition still \
                 vestibular accessibility",
                keys: &[SettingKey::ReducedMotion],
            },
            Self::Scrollbars => Described {
                category: Category::Appearance,
                page: None,
                title: "Scrollbars",
                hint: SCROLLBARS_HINT,
                also_called: "scroll bar thumb track overlay lists panes drag hide auto-hide automatic hidden \
                 always never while scrolling fade mode",
                keys: &[SettingKey::Scrollbars],
            },
            Self::LiveSpectrum => Described {
                category: Category::Appearance,
                page: None,
                title: "Live spectrum",
                hint: LIVE_SPECTRUM_HINT,
                also_called: "visualiser analyser analyzer bars tilt slope pink floor range decibels bands \
                 octave resolution fall decay release peak",
                keys: &[
                    SettingKey::SpectrumTilt,
                    SettingKey::SpectrumFloor,
                    SettingKey::SpectrumBands,
                    SettingKey::SpectrumFalls,
                ],
            },
            Self::WindowButtons => Described {
                category: Category::Window,
                page: None,
                title: "Window buttons",
                hint: WINDOW_BUTTONS_HINT,
                also_called: "titlebar minimize maximize minimise maximise zoom decorations chrome tiling \
                                     controls hide",
                keys: &[SettingKey::MinimiseButton, SettingKey::MaximiseButton],
            },
            Self::Tabs => Described {
                category: Category::Window,
                page: None,
                title: "Sidebar tabs",
                hint: TABS_HINT,
                also_called: "sidebar panes suggestions missing wanted hide show collection counts numbers \
                 totals badges",
                keys: &[
                    SettingKey::SuggestionsTab,
                    SettingKey::MissingTab,
                    SettingKey::TabCounts,
                ],
            },
            Self::WindowState => Described {
                category: Category::Window,
                page: None,
                title: "Window state",
                hint: WINDOW_STATE_HINT,
                also_called: "remember restore last tab pane window size settings category",
                keys: &[
                    SettingKey::RememberTab,
                    SettingKey::RememberWindowSize,
                    SettingKey::RememberSettingsCategory,
                ],
            },
            Self::VolumeWheel => Described {
                category: Category::Window,
                page: None,
                title: "The volume wheel",
                hint: VOLUME_WHEEL_HINT,
                also_called: "scroll mouse wheel touchpad volume slider louder quieter",
                keys: &[SettingKey::ScrollVolume],
            },
            Self::MouseNavigation => Described {
                category: Category::Window,
                page: None,
                title: "Mouse navigation",
                hint: MOUSE_NAVIGATION_HINT,
                also_called: "mouse side buttons back forward browser history navigate previous next",
                keys: &[SettingKey::MouseNavigation],
            },
            Self::Build => Described {
                category: Category::About,
                page: None,
                title: "This build",
                hint: BUILD_HINT,
                also_called: "version typeface font features sinks",
                keys: &[],
            },
            Self::Places => Described {
                category: Category::About,
                page: None,
                title: "Where things are kept",
                hint: PLACES_HINT,
                also_called: "config.toml library database path xdg",
                keys: &[],
            },
            Self::Everything => Described {
                category: Category::About,
                page: None,
                title: "Start again",
                hint: EVERYTHING_HINT,
                also_called: "reset defaults factory put back",
                keys: &[],
            },
        }
    }

    fn matches(self, word: &str) -> bool {
        self.read_as(word).is_some()
    }

    fn read_as(self, word: &str) -> Option<AnsweredBy> {
        let reads = |text: &str| text.to_lowercase().contains(word);

        if reads(self.title()) {
            Some(AnsweredBy::Title)
        } else if reads(self.also_called()) {
            Some(AnsweredBy::AnotherName)
        } else if reads(self.category().label())
            || self.page().is_some_and(|page| reads(page.title()))
        {
            Some(AnsweredBy::Place)
        } else if reads(self.hint()) {
            Some(AnsweredBy::Hint)
        } else {
            None
        }
    }
}

type Ranked = (
    Reverse<Option<AnsweredBy>>,
    u8,
    Reverse<Option<AnsweredBy>>,
    Option<usize>,
    Reverse<AnsweredBy>,
    Group,
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum AnsweredBy {
    Hint,
    Place,
    AnotherName,
    Title,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Narrowing {
    words: Vec<String>,
}

impl Narrowing {
    pub(crate) fn of(query: &str) -> Self {
        Self {
            words: query
                .split_whitespace()
                .map(str::to_lowercase)
                .filter(|word| !word.is_empty())
                .collect(),
        }
    }

    pub(crate) fn narrows(&self) -> bool {
        !self.words.is_empty()
    }

    pub(crate) fn allows(&self, group: Group) -> bool {
        self.words.iter().all(|word| group.matches(word))
    }

    pub(crate) fn found(&self, category: Category) -> usize {
        category
            .groups()
            .filter(|group| self.allows(*group))
            .count()
    }

    pub(crate) fn anywhere(&self) -> impl Iterator<Item = Group> + '_ {
        let found: Vec<(Group, AnsweredBy)> = Group::ALL
            .into_iter()
            .filter(|group| group.is_offered())
            .filter_map(|group| self.reading_of(group).map(|reading| (group, reading)))
            .collect();
        let best_where = |held: &dyn Fn(Group) -> bool| {
            found
                .iter()
                .filter(|(group, _)| held(*group))
                .map(|(_, reading)| *reading)
                .max()
        };
        let mut ranked: Vec<Ranked> = found
            .iter()
            .map(|(group, reading)| {
                let category = group.category();
                let page = group.page();
                (
                    Reverse(best_where(&|other| other.category() == category)),
                    category as u8,
                    Reverse(best_where(&|other| {
                        other.category() == category && other.page() == page
                    })),
                    page.map(|page| page as usize),
                    Reverse(*reading),
                    *group,
                )
            })
            .collect();
        ranked.sort_by_key(|(best, category, best_on_the_page, page, reading, _)| {
            (*best, *category, *best_on_the_page, *page, *reading)
        });
        ranked.into_iter().map(|(.., group)| group)
    }

    fn reading_of(&self, group: Group) -> Option<AnsweredBy> {
        self.words
            .iter()
            .map(|word| group.read_as(word))
            .try_fold(AnsweredBy::Title, |weakest, reading| {
                reading.map(|reading| weakest.min(reading))
            })
    }
}

pub(crate) const AUTOEQ_HINT: &str = "Corrections measured by AutoEq for a few thousand \
     headphones, fetched one device at a time and kept here as an ordinary profile file. Type a \
     name to see what has been measured and by whom; who measured it and on which rig are shown \
     beside every result, because two rigs disagree about one pair of headphones more than two \
     measurements on one rig do. What is plugged in reads the device in use's own name back \
     against the catalogue, and answers with nothing rather than a guess wherever that name is a \
     socket rather than a product.";

pub(crate) const MEASURERS_HINT: &str = "Whose correction to prefer where several measured one \
     device: one choice for every device and one for the device in use. Either takes the \
     measurement it names before the fixed ranking, and Best ranked leaves it to the ranking.";

pub(crate) const OWN_MEASUREMENT_HINT: &str = "A frequency-response measurement of your own, \
     fitted toward a target: flat, a Harman curve or a file. Up to ten bands are fitted to what \
     lies between them and kept here as a profile with its preamp.";

pub(crate) const ROOM_MEASUREMENT_HINT: &str = "Plays a sweep through the output in use and \
     hears it through the desktop or a microphone, then fits a correction toward the target and \
     keeps it as a profile, the microphone's own calibration taken off where one is given.";

pub(crate) const SPEAKERS_HINT: &str = "Where a set of measured head responses (a SOFA file) \
     places two speakers around you: how far to either side of the front they stand, how far \
     above the ears, and how far your head is turned from them.";

pub(crate) const RESCANNING_HINT: &str = "Rescan reads what was added or changed in the music \
     folders since the last scan; Read every file again re-reads all of them whatever their size \
     and time say, and keeps plays, favourites and playlists.";

pub(crate) const LOOKING_UP_HINT: &str = "Look up asks the reference about what has not been \
     answered lately and Refresh all asks about everything again, at one paced request per album \
     and per artist; Look for missing covers asks the archive again for every album with no \
     picture. A lookup under way says what it has found so far.";

pub(crate) const RETAGGING_HINT: &str = "Writes what a lookup answered back into the files so \
     another player reads the same names. A name this build only guessed at is never written, a \
     field the file already says is left alone, and Preview lists what would be written before \
     Apply writes anything. Nothing is written on its own.";

pub(crate) const DEVICE_HINT: &str = "Where Resonate sends audio. The badge on each device says \
                                      whether the track playing would reach it untouched or be \
                                      converted first, so a device that advertises the file's own \
                                      rate and depth is the one that stays bit-perfect.";

pub(crate) const RESAMPLER_HINT: &str = "Runs only when the device cannot take the file's own sample rate, and never touches a stream \
     the device accepts as it is. It is a windowed-sinc filter: an ideal low-pass is cut to a \
     finite length by a window, and the filter closes on the lower of the two Nyquist limits so \
     nothing folds back as aliasing. Low latency sums a kernel for every output instant, a table \
     holding every phase the two rates can meet, so it answers in about a millisecond. The others \
     convert a block at a time through Fourier transforms, far less work and a longer wait, each \
     through a window designed to fall steadily so the filter rings for as short a time as its \
     depth allows. Very high is built to beat SoX's steep very-high setting: flat as near the top \
     of the band, about 230 dB down from Nyquist on against SoX's 180, and ringing 10.4 ms either \
     side of a sharp sound against SoX's 11.8. Hover a level for the filter it builds. Linear \
     phase rings as much before a transient as after it; minimum phase moves all of that ringing \
     after it and plays a few frames sooner, at the price of delaying the treble a little more \
     than the bass; intermediate sits between the two.";

pub(crate) const TRUE_PEAK_HINT: &str = "A sample can sit under full scale while the wave the \
                                         device rebuilds between two samples passes it, and a \
                                         lossy file often decodes past full scale outright. With \
                                         this on, a track the library has studied is turned down \
                                         by what its measured true peak asks, and wherever the \
                                         sound is processed a guard watches the output at eight \
                                         times its rate and rides the level down under -0.1 dBTP \
                                         just before an over, rather than letting it clip.";

pub(crate) const LOSSY_SOURCES_HINT: &str = "An MP3, AAC or Vorbis file has had a band cut away \
                                            above the encoder's lowpass and, at lower bitrates, \
                                            bands switched off from one moment to the next. Repair \
                                            lifts the droop the encoder's own filter leaves just \
                                            under its cutoff and fills a band that drops out of \
                                            an otherwise steady sound with noise shaped like it. \
                                            Extend also rebuilds the band above the cutoff from \
                                            the octave below it, the way AAC's SBR and Sony's \
                                            DSEE do, at a level that falls away faster than the \
                                            music does. Both touch lossy files alone.";

pub(crate) const CROSSFEED_HINT: &str = "Headphones play each channel into one ear alone, where \
                                         speakers reach both ears with a slightly delayed, \
                                         duller copy of the other side. Crossfeed mixes that \
                                         copy in, so a hard-panned record sits less far apart. \
                                         The level is kept: the mix never peaks above the \
                                         louder channel it was made from. Stereo output only.";

pub(crate) const DITHER_HINT: &str = "Applies wherever the output is shallower than the source or \
                                      anything is processed, on a 16-, 24- or 32-bit device alike. \
                                      Triangular trades a little steady noise for the distortion \
                                      rounding alone would add; None rounds to the nearest step.";

pub(crate) const SHAPING_HINT: &str = "Moves the dither noise above towards where the ear is least sensitive, so it runs only where \
     dither does. Threshold is designed for whatever rate the device runs at, so it shapes at every \
     rate. Lipshitz is a fit to 44.1 kHz: at any rate but 44.1 and 48 kHz its taps would put the \
     notch in the wrong place, so the stream falls back to flat dither rather than shaping it \
     wrongly. `resonate explain` prints the curve that survives beside the target depth.";

pub(crate) const SAMPLE_RATE_HINT: &str = concat!(
    "Which rate the device is opened at. Match the file asks for the file's own rate, so a 96 kHz \
     recording reaches a device that advertises 96 kHz untouched and nothing resamples. Follow the \
     graph stays on whatever rate ",
    sound_system!(),
    " is already running and resamples into it, which never interrupts another client but \
     converts every stream whose rate differs."
);

pub(crate) const GRAPH_RATE_HINT: &str = "Whether the stream asks PipeWire to run the whole graph at its rate. Asking is what carries a \
     rate change through to the hardware; leaving it to the daemon lets the graph stay where it is \
     and resample on the way. The daemon has the last word either way — a rate it refuses to \
     switch to is not in the sink's allowed rates.";

pub(crate) const BUFFER_HINT: &str = "How much decoded audio the player holds ahead of the \
                                      device. A deeper buffer rides out a busy machine or a slow \
                                      disc; a shallower one costs less memory, starts a stream \
                                      sooner and lets a volume or equaliser change be heard \
                                      sooner. A pause and a seek act at once at any depth. It is \
                                      not the device's latency: how much the graph asks for each \
                                      cycle is the sound system's to choose, shared with every other \
                                      application on the device. Changing it reopens the stream.";

pub(crate) const BLUETOOTH_HINT: &str = "Wireless headphones power their radio down when nothing \
     is sent to them, and the first moment of sound after that is lost while the link wakes. \
     With this on, a Bluetooth device is kept fed with silence through a pause so resuming loses \
     nothing, and a stream that starts after the link has slept opens on a short lead of silence \
     before the track, with the clock held at the start until the music is really heard. \
     Experimental: it keeps the headphones' radio busy through a pause, which costs them \
     battery.";

pub(crate) const DEVICE_VOLUME_HINT: &str = "Turns the device's own volume, where its driver says \
     it has one, rather than scaling the samples, so the stream stays bit-perfect at any volume. \
     The slider then is the device's volume: it starts where the device already is, and follows \
     the desktop when the device is turned from there. Everything else playing through that \
     device is turned with it, and a device with no volume of its own is still turned by the \
     stream.";

pub(crate) const DOP_HINT: &str = "Hands a DSD file to the device as DSD carried inside PCM words rather than decimating it to \
     PCM first. Nothing in ALSA or SPA advertises whether a device decodes DoP — only the DAC's \
     own detector knows — so this is a claim you make about your hardware and not one Resonate can \
     check. A DAC that does not decode it plays the markers as full-scale white noise. It is \
     taken only where the device advertises the exact rate and depth the carrier needs and \
     nothing else would touch the stream; `resonate explain` prints which packing was chosen and \
     why the other was not.";

pub(crate) const EQUALISING_HINT: &str = "Whether the curve bound to the playing device is \
     applied. An equaliser is a filter, so turning it on takes the stream out of bit-perfect: \
     every sample is recomputed in floating point before it reaches the device, the playback \
     bar's mode reads converted rather than bit-perfect, and `resonate explain` says so rather \
     than reporting a path it no longer takes. It runs after any resampling, so one profile means \
     one curve whatever rate the device is opened at, and a band above the output's Nyquist is \
     passed over rather than folded back as something it is not. A DSD stream packed as DoP \
     refuses it outright, because rewriting those samples would destroy the markers the DAC \
     reads.";

pub(crate) const BOUND_TO_HINT: &str = "Which curve is applied to which device: a kept profile, \
     or a curve of the device's own that needs no name. A correction is measured for one pair of \
     headphones, so it is bound to the device those headphones are plugged into rather than being \
     in force everywhere: plug in speakers and nothing is applied unless you bound something to \
     them too. The binding every device that names none of its own falls back to is written as \
     default, and the settings file keeps the pairs as an ordinary table you can edit by hand, \
     with true standing for a device's own curve.";

pub(crate) const BANDS_HINT: &str = "What the bound curve does to the sound, drawn at the rate \
     the device is open at, and shaped where it is drawn. Press anywhere on it to add a band there, \
     drag a handle to move one — sideways for its frequency, up and down for its gain — scroll over \
     one to narrow or widen it, and right-press it to take it out. A device with nothing bound is \
     given a curve of its own the first time it is pressed, so shaping the sound needs no profile \
     and no name. A pointer places a band to a tenth of a decibel; a number typed into a row is \
     exact, which a correction measured to the hundredth deserves. The preamp above the rows is \
     what keeps the loudest part of the curve from reaching full scale, and Fit the preamp sets it \
     from the curve itself and then from what the songs in the queue peak at through it. Curves are kept as EqualizerAPO text, so any other player reads what is \
     exported here.";

pub(crate) const REPLAY_GAIN_HINT: &str = "Plays every track at the loudness its tags declare. \
                                           Track levels each track on its own, Album keeps the \
                                           relative levels inside a record. A boost is capped \
                                           where the declared peak would reach full scale, so the \
                                           waveform keeps its shape. A track with no tag that the \
                                           library has studied is played at the gain its measured \
                                           loudness asks for. The pre-amp is added to every gain, \
                                           and a track neither tagged nor studied is played at the \
                                           gain chosen for it instead.";

pub(crate) const FOLDERS_HINT: &str = "The folders a scan walks, and among them the primary \
                                       one: the folder new songs are kept in. What is dragged \
                                       onto the window is copied into the primary folder with \
                                       its covers and lyrics, then filed by the layout under \
                                       Organising unless that is turned off. Adding a folder \
                                       scans it straight away; dropping one forgets every track \
                                       that came from it, and leaves the files alone.";

pub(crate) const VAULT_HINT: &str = "A managed archive this build writes itself. Importing a \
                                     track decodes it, throws every tag and picture away, keeps \
                                     the smallest bit-exact copy it can make and points the \
                                     catalog at it. Covers are kept once each as lossless JXL, \
                                     and the library it was imported from is read and never \
                                     touched.";

pub(crate) const SUBSONIC_HINT: &str = "A server of your own the tracks marked wanted are \
                                        also filled from, matched by the recording's \
                                        MusicBrainz id or its ISRC.";

pub(crate) const JELLYFIN_HINT: &str = "A Jellyfin or Emby server of your own, browsed and played \
                                        where it is in the Services pane.";

pub(crate) const YOUTUBE_MUSIC_HINT: &str = "YouTube Music through the yt-dlp program, on your \
                                             account's cookies: searched and browsed in the \
                                             Services pane, and asked for a song marked wanted \
                                             only once nothing better has it.";

pub(crate) const RESONATE_KEY_HINT: &str = "The key your Resonate server lists. With it the \
                                            tracks marked wanted are also asked of that server, \
                                            which downloads them through its hifi-api and sends \
                                            the lossless FLAC. The key goes to that server \
                                            alone, never to a hifi-api or Monochrome server \
                                            named under TIDAL.";

pub(crate) const SPOTIFY_HINT: &str = "A Spotify account, signed in to with an app you register \
                                       yourself. Its liked songs and playlists become playlists \
                                       here, kept in step or moved once, and what the library \
                                       lacks is wanted. Nothing is offered or asked until a \
                                       client ID is given.";

pub(crate) const TIDAL_HINT: &str = "A TIDAL subscription of your own, signed in to here or \
                                     reached through a hifi-api server you run, the tracks \
                                     marked wanted are also filled from, as lossless FLAC, \
                                     matched by the TIDAL track MusicBrainz names or the ISRC. \
                                     Monochrome is asked too, by the ISRC alone. The hosted \
                                     hifi-api and Monochrome services are asked only where you \
                                     allow them.";

pub(crate) const ABX_HINT: &str = "Hear the equaliser against a bypass of itself, or lossy \
                                   restoration, the resampler or dither against another setting, \
                                   level-matched, and say in each trial whether the hidden one is \
                                   A or B. The score keeps its chance of being luck.";

pub(crate) const ROOM_HINT: &str = "An impulse response measured in the room — a WAV or \
                                    FLAC as REW or DRC writes it — convolved with everything \
                                    played, after the equaliser.";

pub(crate) const INBOX_HINT: &str = "A folder the tracks marked wanted are filled from. A file \
                                     directly inside it whose name is the recording's \
                                     MusicBrainz id, the track's or the ISRC is kept in the \
                                     vault and joins its album as a track; a file that only \
                                     shares a title is never taken. The folder is read and never \
                                     written to. A track is asked about as soon as it is marked \
                                     and then every half hour, no oftener than every six hours \
                                     each; Poll now asks about every one at once, as resonate \
                                     poll --again does.";

pub(crate) const ORGANISING_HINT: &str = "Where the files themselves are kept, written as a \
     template of the fields a track carries: {albumartist}, {artist}, {album}, {title}, {year}, \
     {disc}, {track} and {ext}, with each / a folder. Preview lists what would move and where, \
     and only Apply moves anything. A track nothing names an album for is left where it is, so is \
     one whose destination another file already holds, and a folder left empty is taken away. \
     Nothing is filed on its own: this runs when you press it and at no other time.";

pub(crate) const REPEATING_HINT: &str = "What a skip does while one track is repeating. On, \
                                         pressing next or previous moves on and repeats the \
                                         whole queue from there, the way a streaming player \
                                         does. Off, it moves on and repeats the track it lands \
                                         on instead.";

pub(crate) const PREVIOUS_HINT: &str = "What previous does once a song is under way. On, \
                                         previous starts the song again once it is past its \
                                         first three seconds, and goes back a track while it \
                                         is still in those. Off, previous always goes back a \
                                         track. A song shorter than three seconds is left for \
                                         the one before it.";

pub(crate) const HISTORY_HINT: &str = "How long what was listened to is kept. Forever keeps every \
                                       listen; a span forgets the listens and the skipped time \
                                       older than it as the library opens, once ListenBrainz has \
                                       been told of them. A track's play count and when it was last \
                                       played are kept whatever this says.";

pub(crate) const RESUMING_HINT: &str = "Whether the queue, the row it is on and how far through \
                                        that row are kept in the library for the next run. On, \
                                        opening the window with no files named puts the queue back \
                                        where it was, paused on the row it was playing. Off, what \
                                        was kept is discarded and nothing is written.";

pub(crate) const ONLINE_HINT: &str = "Whether Resonate reaches the network at all. On, a scan ends by asking MusicBrainz about \
     every album and artist it has not yet been answered about, takes a cover from the Cover Art \
     Archive and a portrait from Wikimedia Commons where one is offered, and the lyrics pane asks \
     LRCLIB and Better Lyrics for a sheet the file does not carry, keeping the more finely timed. Off, nothing leaves the machine.";

pub(crate) const AFTER_SCAN_HINT: &str = "Whether a scan that finishes hands over to the reference \
                                          on its own. On, every scan ends by asking about what it \
                                          found; off, the library is asked only when Enrich is \
                                          pressed here or in the Library card, or when resonate \
                                          enrich runs.";

pub(crate) const METERED_HINT: &str = "What a connection NetworkManager reports as metered, a \
                                       phone's hotspot or a capped plan, may spend. Lookups to \
                                       the reference are skipped while it is metered unless \
                                       allowed; a provider's download goes on unless held, and \
                                       waits for an unmetered connection when it is. A song \
                                       played in place is neither. A connection that cannot be \
                                       told is not metered.";

pub(crate) const FLAC_RENDERERS_HINT: &str = "A UPnP renderer that does not state the rates it \
                                              decodes is sent FLAC up to 96 kHz, and what is above \
                                              is resampled down to it. A renderer named here is \
                                              sent 176.4 and 192 kHz FLAC as they are. Nothing \
                                              can ask a renderer what it decodes, so only you can \
                                              say; the name is read when the renderer is next \
                                              found, from the next start.";

pub(crate) const STUDIES_HINT: &str = "Whether a lookup also decodes every track once, on half \
                                       the machine's cores, to judge whether its lossless claim \
                                       holds, measure its loudness and true peak and take its \
                                       print. Off, a lookup asks the reference alone, and a track \
                                       is studied only when the Analysis pane plays it or when \
                                       its print is wanted to name it.";

pub(crate) const LYRICS_HINT: &str = "Whether a lookup also asks LRCLIB and Better Lyrics for \
                                      the words of every track and keeps the more finely timed \
                                      answer, two singers word by word scoring highest, so the \
                                      Lyrics pane opens on them without asking and a search \
                                      reaches them. Plain words and line-synced sets are asked \
                                      about again after a month in case a better set has been \
                                      written since, and a set timed word by word is kept for \
                                      good. Off, words are asked for \
                                      only when a track is played. Choosing by the system's \
                                      language hands a track whose sheets beside it are named for \
                                      several languages the one LANGUAGE or LANG names; off, as \
                                      built, the first of them by name.";

pub(crate) const RECOGNITION_HINT: &str = "An AcoustID client key, which lets the lookup and the \
                                           Analysis pane send a fingerprint of the audio and hear \
                                           back what it is, and an AudD token, which Listen asks \
                                           beside Shazam. Each service gives one to anyone who \
                                           registers; nothing is sent to either without it.";

pub(crate) const LISTENING_HINT: &str = "What Listen records — the desktop's own output or a \
                                         microphone — and for how long, before it asks Shazam \
                                         and whatever else is keyed what the song is. Ctrl+L or \
                                         the ear in the header opens it.";

pub(crate) const LASTFM_HINT: &str = "A Last.fm session, which has every play this library \
                                      counts scrobbled there as it is counted, and the song \
                                      playing told as playing now. Signed in to under the API \
                                      key and secret of an API account made on last.fm; the \
                                      password is sent once and never kept. Nothing is sent \
                                      without a session or while online is off.";

pub(crate) const SUBMITTING_HINT: &str = "A ListenBrainz user token, which has every play this \
                                          library counts told to that service as it is counted — \
                                          its title, its artist, its album and the MusicBrainz \
                                          ids the catalog holds for it. Plays counted before a \
                                          token is given are not sent, and nothing is sent \
                                          without one or while online is off.";

pub(crate) const CONTACT_HINT: &str = "What the User-Agent carries beside the name and the \
                                       version, so a service can say who to write to when a client \
                                       misbehaves. MusicBrainz asks for one; nothing here needs \
                                       it. It is read at start, so what is typed here is sent from \
                                       the next one.";

pub(crate) const NOTIFICATIONS_HINT: &str = "Whether a track starting raises a popup through the \
                                             desktop's notification server. One is never raised \
                                             while this window is the one in front, and a session \
                                             with no notification daemon has none either way.";

pub(crate) const DISCORD_HINT: &str = "Shows what is playing on your Discord profile, under an \
                                       application you register yourself. Off until you switch it \
                                       on and give it that application's id; nothing reaches \
                                       Discord before then.";

pub(crate) const DISCORD_SHOWS_HINT: &str = "How much Discord is told: just that you are \
                                             listening, the track, or the track and its album, \
                                             with the album's cover, your application's icon or \
                                             no picture, and a bar that follows the track.";

pub(crate) const SYNCING_HINT: &str = "Keeps your playlists, favourites, listening history and \
                                       what each device was playing the same on every device that \
                                       shares this pairing code. What travels is sealed on this \
                                       device first; the server holds only what it cannot read.";

pub(crate) const PAIRING_HINT: &str = "A device joins this one by its pairing code: scan the \
                                       code with a phone's camera, or type it in. The code is \
                                       the key to everything your devices share, so it stays \
                                       covered until you show it.";

pub(crate) const SYNC_DEVICES_HINT: &str = "Every device that shares the pairing code, whether it \
                                            is online, on this network or away. One that is away \
                                            can be taken off every device's list.";

pub(crate) const SYNC_CONNECTION_HINT: &str = "How your devices meet: through Resonate's server, \
                                               a server of your own, or only on this network \
                                               with no server at all. Devices on one network \
                                               always meet directly.";

pub(crate) const SYNC_CONTROL_HINT: &str = "A device sharing your pairing code can see and steer \
                                            what plays here, as far as you let it. Nothing \
                                            reaches the server: it goes straight between your \
                                            devices.";

pub(crate) const SYNCED_SONGS_HINT: &str = "Songs never pass through the server: while two of \
                                            your devices are online they are sent from one to the \
                                            other directly. Nothing is sent unless you ask, or \
                                            you choose here what is fetched by itself.";

pub(crate) const COLOUR_HINT: &str = "The palette the whole window is painted in and the one colour it leads with — the play mark, \
     a selected row, a chosen chip, the position rail and every primary control. Every palette is \
     dark, and each draws each accent in its own tones, so the accent stays part of the theme \
     rather than sitting on top of it. A palette's strip shows its sidebar, its panes, what it \
     raises above them and the accent that would be worn.";

pub(crate) const LAYOUT_HINT: &str = "How large the window draws itself. The type scale and every \
                                      measure that holds text — a row, the sidebar, the header, a \
                                      control — are read off one size, so nothing is left behind \
                                      when it changes. The window's own chrome and its corners are \
                                      not, because those belong to the compositor rather than to \
                                      the type.";

pub(crate) const WINDOW_BUTTONS_HINT: &str = "Whether the titlebar this window draws for itself \
     carries a minimise and a maximise button beside the close one. A tiling compositor, or a \
     session where a key already does both, has no use for them. Close always stays, because it \
     is the one way out a pointer has, and a double press on the titlebar still fills the screen \
     and gives the window back either way.";

pub(crate) const MOTION_HINT: &str = "Whether the window moves things about. Reduced, a menu, a \
     sheet or a notice appears at once instead of rising into place, a switch lands without \
     sliding, a song change replaces the last without fading, the scrollbars and the lyrics jump \
     rather than glide.";

pub(crate) const VOLUME_WHEEL_HINT: &str = "Whether turning the wheel over the volume slider \
     moves the volume, a notch at a time. Off, the slider answers only to a press or a drag and \
     the keys.";

pub(crate) const MOUSE_NAVIGATION_HINT: &str = "Whether the mouse side buttons move back and \
     forward through album and artist pages opened in this window.";

pub(crate) const LIVE_SPECTRUM_HINT: &str = "How the visualiser's spectrum reads what is heard: \
     how far the treble is lifted so pink noise stands level, how quiet a band can be and still \
     show, how finely the octaves are cut, and how quickly a bar falls back once its band \
     quietens.";

pub(crate) const SCROLLBARS_HINT: &str = "When the lists and panes that scroll draw a bar \
     down their edge that shows where the view stands and can be dragged: always, only while \
     they are scrolling and for a moment after, or never. Without a bar they still scroll with \
     the wheel and the keys.";

pub(crate) const TABS_HINT: &str = "Which of the collection's panes the sidebar lists. \
     Suggestions offers lists made out of what the catalog holds, and Missing names what the \
     releases are short of and what the artists have put out that the library does not hold. A \
     pane that is hidden is left out of the sidebar and of the keys that step through it, and \
     nothing else leads to it. The counts are the figures drawn beside a tab — how many albums, \
     artists, tracks and plays — and off, the tabs carry their names alone.";

pub(crate) const WINDOW_STATE_HINT: &str = "Whether the window comes back to the tab, size and \
     settings category it last had. Each can be remembered on its own, and turning one off takes \
     its saved value out of the settings file.";

pub(crate) const BUILD_HINT: &str = "What this copy of Resonate is: its version, the faces it \
                                     settled on out of the families this machine has installed, \
                                     and whether the parts that are compiled in behind a feature \
                                     are here at all.";

pub(crate) const PLACES_HINT: &str = "The settings file every control here writes to, and the \
                                      SQLite catalog the library pane reads from. Neither is a \
                                      setting: the settings file is named by --config and the \
                                      catalog by --library or the library key inside it.";

pub(crate) const EVERYTHING_HINT: &str = "Takes every key this pane can write out of the settings \
                                          file, so each setting falls back to what Resonate was \
                                          built with. The music folders are untouched — they live \
                                          in the catalog rather than in the file — and so do the \
                                          files themselves.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_group_stands_in_exactly_one_category_and_every_category_holds_one() {
        for category in Category::ALL {
            assert!(
                category.groups().count() > 0,
                "{} is a category with nothing in it",
                category.label()
            );
        }
        assert_eq!(
            Category::ALL
                .into_iter()
                .map(|category| category.groups().count())
                .sum::<usize>(),
            Group::ALL
                .into_iter()
                .filter(|group| group.is_offered())
                .count()
        );
    }

    #[test]
    fn the_metered_switches_sit_in_online_and_the_renderers_flac_beside_the_device() {
        assert_eq!(Group::Metered.category(), Category::Online);
        assert_eq!(Group::FlacRenderers.category(), Category::Output);
        assert_eq!(
            Group::Metered.keys(),
            &[
                SettingKey::EnrichOnMetered,
                SettingKey::StallDownloadsOnMetered
            ]
        );
        assert_eq!(Group::FlacRenderers.keys(), &[SettingKey::FlacAbove96kFor]);
    }

    #[test]
    fn no_two_groups_share_a_title_and_none_is_written_without_a_hint() {
        for group in Group::ALL {
            let sharing = Group::ALL
                .into_iter()
                .filter(|other| other.title() == group.title())
                .count();
            assert_eq!(
                sharing,
                1,
                "{} is titled as another group is",
                group.title()
            );
            assert!(
                group.hint().len() > 40,
                "{} explains itself in {} characters",
                group.title(),
                group.hint().len()
            );
        }
    }

    #[test]
    fn a_setting_is_written_under_one_group_and_no_key_is_claimed_twice() {
        for group in Group::ALL {
            for key in group.keys() {
                let claiming = Group::ALL
                    .into_iter()
                    .filter(|other| other.keys().contains(key))
                    .count();
                assert_eq!(
                    claiming,
                    1,
                    "{:?} is put back by {} and by another group as well",
                    key,
                    group.title()
                );
            }
        }
    }

    #[test]
    fn an_empty_query_narrows_nothing_and_allows_every_group() {
        let narrowing = Narrowing::of("   ");

        assert!(!narrowing.narrows());
        for group in Group::ALL {
            assert!(narrowing.allows(group));
        }
        assert_eq!(
            narrowing.anywhere().count(),
            Group::ALL
                .into_iter()
                .filter(|group| group.is_offered())
                .count()
        );
    }

    #[test]
    fn a_setting_named_by_the_words_leads_one_whose_hint_merely_mentions_them() {
        for word in [
            "dither", "volume", "lyrics", "cover", "buffer", "discord", "accent",
        ] {
            let found: Vec<Group> = Narrowing::of(word).anywhere().collect();
            let titled = found
                .iter()
                .any(|group| group.title().to_lowercase().contains(word));
            if titled {
                assert!(
                    found[0].title().to_lowercase().contains(word),
                    "{word:?} led with {:?} ahead of a setting called by it",
                    found[0].title()
                );
            }
            let mut seen = Vec::new();
            for group in &found {
                let category = group.category();
                if seen.last() != Some(&category) {
                    assert!(
                        !seen.contains(&category),
                        "{word:?} drew {category:?} under two headings"
                    );
                    seen.push(category);
                }
            }
        }
    }

    #[test]
    fn a_query_reads_a_group_by_its_title_however_it_is_cased() {
        let narrowing = Narrowing::of("DITHER");

        assert!(narrowing.narrows());
        assert!(narrowing.allows(Group::Dither));
        assert!(
            narrowing.allows(Group::NoiseShaping),
            "the shaping hint names dither"
        );
        assert!(!narrowing.allows(Group::Colour));
    }

    #[test]
    fn a_group_is_found_by_a_word_nothing_on_screen_writes() {
        assert!(Narrowing::of("sink").allows(Group::Device));
        assert!(Narrowing::of("latency").allows(Group::Buffer));
        assert!(Narrowing::of("sacd").allows(Group::Dop));
        assert!(Narrowing::of("musicbrainz").allows(Group::Lookups));
        assert!(Narrowing::of("font").allows(Group::Build));
        assert!(Narrowing::of("maximize").allows(Group::WindowButtons));
        assert!(Narrowing::of("titlebar").allows(Group::WindowButtons));
    }

    #[test]
    fn every_word_has_to_land_for_a_group_to_be_allowed() {
        assert!(Narrowing::of("dither noise").allows(Group::Dither));
        assert!(!Narrowing::of("dither folders").allows(Group::Dither));
    }

    #[test]
    fn a_query_is_counted_against_every_category_rather_than_the_one_in_front() {
        let narrowing = Narrowing::of("rate");

        assert!(narrowing.found(Category::Output) >= 2);
        assert_eq!(narrowing.found(Category::Appearance), 0);
        assert_eq!(
            narrowing.anywhere().count(),
            Category::ALL
                .into_iter()
                .map(|category| narrowing.found(category))
                .sum::<usize>()
        );
    }

    #[test]
    fn every_page_holds_a_group_of_its_own_category_and_no_two_pages_share_a_name() {
        for page in Page::ALL {
            assert!(
                page.groups().next().is_some(),
                "{} is a page with nothing on it",
                page.title()
            );
            for group in page.groups() {
                assert_eq!(
                    group.category(),
                    page.category(),
                    "{} sits on {} under another category",
                    group.title(),
                    page.title()
                );
            }
            let sharing = Page::ALL
                .into_iter()
                .filter(|other| other.title() == page.title() || other.id() == page.id())
                .count();
            assert_eq!(sharing, 1, "{} is named as another page is", page.title());
        }
    }

    #[test]
    fn a_category_draws_its_own_groups_and_gathers_its_pages_where_the_first_of_them_stands() {
        assert_eq!(
            Category::Equaliser.drawn(),
            vec![
                Drawn::Group(Group::Equalising),
                Drawn::Group(Group::BoundTo),
                Drawn::Group(Group::Bands),
                Drawn::Pages(vec![
                    Page::HeadphoneCorrection,
                    Page::OwnMeasurement,
                    Page::RoomMeasurement,
                    Page::RoomCorrection,
                    Page::ListeningTest,
                ]),
            ]
        );
        assert_eq!(
            Category::Library.drawn(),
            vec![
                Drawn::Group(Group::Folders),
                Drawn::Pages(vec![Page::Enrichment]),
                Drawn::Group(Group::Organising),
                Drawn::Group(Group::Vault),
                Drawn::Group(Group::Inbox),
                Drawn::Group(Group::History),
            ]
        );
        assert_eq!(
            Category::Sync.drawn(),
            vec![
                Drawn::Group(Group::Syncing),
                Drawn::Group(Group::Pairing),
                Drawn::Pages(vec![Page::Devices]),
                Drawn::Group(Group::SyncConnection),
                Drawn::Group(Group::SyncedSongs),
            ]
        );

        for category in Category::ALL {
            let mut shown: Vec<Group> = category.shown_on(None).collect();
            for drawn in category.drawn() {
                if let Drawn::Pages(pages) = drawn {
                    for page in pages {
                        shown.extend(category.shown_on(Some(page)));
                    }
                }
            }
            shown.sort_by_key(|group| *group as usize);
            assert_eq!(
                shown,
                category.groups().collect::<Vec<_>>(),
                "{} draws a group twice or leaves one out",
                category.label()
            );
        }
    }

    #[test]
    fn a_setting_on_a_page_of_its_own_is_found_by_its_name_and_by_the_pages() {
        assert_eq!(
            Narrowing::of("autoeq").anywhere().next(),
            Some(Group::AutoEq)
        );
        assert!(Narrowing::of("your devices").allows(Group::SyncDevices));
        assert!(Narrowing::of("room correction").allows(Group::Speakers));
        assert!(Narrowing::of("rescan").allows(Group::Rescanning));
        assert_eq!(
            Narrowing::of("listening test").anywhere().next(),
            Some(Group::Abx)
        );
    }

    #[test]
    fn listenbrainz_and_last_fm_are_both_accounts_and_both_answer_to_scrobbling() {
        let found: Vec<Group> = Narrowing::of("scrobble").anywhere().collect();

        assert!(found.contains(&Group::Lastfm));
        assert!(found.contains(&Group::Submitting));
        assert_eq!(Group::Lastfm.category(), Category::Accounts);
        assert_eq!(Group::Submitting.category(), Category::Accounts);
    }

    #[test]
    fn every_category_is_drawn_with_a_mark_of_its_own() {
        for category in Category::ALL {
            let sharing = Category::ALL
                .into_iter()
                .filter(|other| other.icon() == category.icon())
                .count();
            assert_eq!(
                sharing,
                1,
                "{} shares its mark with another category",
                category.label()
            );
        }
    }

    #[test]
    fn a_query_nothing_answers_to_finds_nothing_anywhere() {
        let narrowing = Narrowing::of("chartreuse");

        assert_eq!(narrowing.anywhere().count(), 0);
        for category in Category::ALL {
            assert_eq!(narrowing.found(category), 0);
        }
    }
}
