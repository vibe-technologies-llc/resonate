use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs, io,
    ops::RangeInclusive,
    path::{Path, PathBuf},
    time::Duration,
};

use clap::ValueEnum as _;
use resonate_cast::Given;
use resonate_core::{
    Accent, AppId, ArtistsDrawn, HeadYaw, Icon, Pictured, RemoteTrust, ScrollbarMode, Shown,
    Sorted, SpeakerAngle, SpeakerElevation, SpectrumBands, SpectrumFalls, SpectrumFloor,
    SpectrumTilt, Switch, SyncedSongs, TextSize, Theme, Trim, Valued, Volume,
};
use resonate_engine::{
    Crossfeed, DitherKind, FilterPhase, Learnt, NodeName, NoiseShaping, Plays, Quality,
    ReplayGainMode, Restoration, ShuffleStyle, Trial,
};
use resonate_eq::{Binding, ProfileName};
use resonate_library::{Ceiling, HistoryKept, Layout, MinimumLength, MusicExtensions};
use resonate_listen::Listening;
use resonate_upnp::{Located, ServerName};
use resonate_words::{Language, LanguageChoice, RegionChoice};
use toml_edit::{DocumentMut, Item};

#[cfg(feature = "online")]
use super::{Accounts, LastfmAccount};
use super::{Config, config_path, places::home_dir};
use crate::{
    ConfigKey, Error, Result, ValueKind,
    cli::{DitherArg, FilterPhaseArg, LISTENS_FOR_SECONDS, NoiseShapingArg, QualityArg},
    vocabulary::Meaning,
};

const CONFIG_FLAG: &str = "--config";

const BLUETOOTH_LEAD_MS: RangeInclusive<u64> = 0..=5_000;

const BLUETOOTH_AWAKE_S: RangeInclusive<u64> = 0..=3 * 60 * 60;

const SPEAKER_ANGLE_DEGREES: RangeInclusive<u64> =
    SpeakerAngle::NARROWEST_DEGREES as u64..=SpeakerAngle::WIDEST_DEGREES as u64;

pub fn spoken_before_the_grammar(arguments: &[OsString]) -> Language {
    let chosen = named_on_the_command_line(arguments)
        .or_else(|| config_path().ok())
        .and_then(|path| item_in(&path, ConfigKey::Language).ok().flatten())
        .and_then(|item| item.as_str().and_then(LanguageChoice::parse))
        .unwrap_or_default();
    chosen.language(&resonate_words::languages_the_listener_reads())
}

pub(super) fn named_on_the_command_line(arguments: &[OsString]) -> Option<PathBuf> {
    let given = format!("{CONFIG_FLAG}=");
    let mut words = arguments.iter().skip(1).take_while(|word| *word != "--");

    while let Some(word) = words.next() {
        if *word == *CONFIG_FLAG {
            return words.next().map(PathBuf::from);
        }
        if let Some(path) = word.to_str().and_then(|word| word.strip_prefix(&given)) {
            return Some(PathBuf::from(path));
        }
    }
    None
}

pub fn load(explicit: Option<&Path>) -> Result<Config> {
    let (path, demanded) = match explicit {
        Some(path) => (path.to_path_buf(), true),
        None => (config_path()?, false),
    };

    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(source) if source.kind() == io::ErrorKind::NotFound && !demanded => {
            return Ok(Config {
                read_from: Some(path),
                ..Config::default()
            });
        }
        Err(source) => return Err(Error::ReadConfig { path, source }),
    };
    Ok(Config {
        read_from: Some(path.clone()),
        ..parse(&path, &text, |error| {
            tracing::warn!(%error, "leaving a setting that will not read at its default");
        })?
    })
}

#[cfg(feature = "online")]
pub fn spotify_in(path: &Path) -> Result<Option<(String, String)>> {
    let read = |key: ConfigKey| -> Result<Option<String>> {
        match item_in(path, key)? {
            Some(value) => Ok(given(At { path, key }.string(&value)?)),
            None => Ok(None),
        }
    };

    Ok(read(ConfigKey::SpotifyClientId)?.zip(read(ConfigKey::SpotifyRefreshToken)?))
}

#[cfg(feature = "online")]
pub fn tidal_refresh_token_in(path: &Path) -> Result<Option<String>> {
    let key = ConfigKey::TidalRefreshToken;
    match item_in(path, key)? {
        Some(value) => Ok(given(At { path, key }.string(&value)?)),
        None => Ok(None),
    }
}

#[cfg(feature = "online")]
pub fn submitting_in(path: &Path) -> Result<Accounts> {
    let online = ConfigKey::Online;
    let switched_on = match item_in(path, online)? {
        Some(value) => At { path, key: online }.boolean(&value)?,
        None => true,
    };

    if !switched_on {
        return Ok(Accounts::default());
    }

    let read = |key: ConfigKey| -> Result<Option<String>> {
        match item_in(path, key)? {
            Some(value) => Ok(given(At { path, key }.string(&value)?)),
            None => Ok(None),
        }
    };

    Ok(Accounts {
        listenbrainz: read(ConfigKey::ListenbrainzToken)?,
        lastfm: LastfmAccount::of(
            read(ConfigKey::LastfmKey)?,
            read(ConfigKey::LastfmSecret)?,
            read(ConfigKey::LastfmSession)?,
        ),
    })
}

fn item_in(path: &Path, key: ConfigKey) -> Result<Option<Item>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(Error::ReadConfig {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    Ok(document(path, &text)?.get(key.as_str()).cloned())
}

pub(super) fn document(path: &Path, text: &str) -> Result<DocumentMut> {
    text.parse().map_err(|source| Error::ConfigSyntax {
        path: path.to_path_buf(),
        source: Box::new(source),
    })
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Retired {
    spelt: &'static str,
    became: Switch,
    reads_as: fn(bool) -> bool,
}

pub(super) const REMOVED_CONTACT: &str = "contact";

const RETIRED: [Retired; 1] = [Retired {
    spelt: "stall-downloads-on-metered",
    became: Switch::DownloadOnMetered,
    reads_as: std::ops::Not::not,
}];

impl Retired {
    fn spelt(name: &str) -> Option<Self> {
        RETIRED.into_iter().find(|retired| retired.spelt == name)
    }

    pub(super) fn spellings_of(key: ConfigKey) -> impl Iterator<Item = &'static str> {
        RETIRED
            .into_iter()
            .filter(move |retired| retired.became.key() == key)
            .map(|retired| retired.spelt)
    }

    fn take_unless_said_anew(self, config: &mut Config, path: &Path, value: &Item) {
        if config.switches.said(self.became).is_some() {
            return;
        }
        match value.as_bool() {
            Some(said) => config.switches.say(self.became, (self.reads_as)(said)),
            None => tracing::warn!(
                key = self.spelt,
                path = %path.display(),
                "ignoring a retired setting that is not true or false"
            ),
        }
    }
}

pub(super) fn parse(path: &Path, text: &str, mut refused: impl FnMut(Error)) -> Result<Config> {
    let document = document(path, text)?;

    let mut config = Config::default();
    let mut retired = Vec::new();

    for (name, value) in document.iter() {
        if name == REMOVED_CONTACT {
            continue;
        }

        let Some(key) = ConfigKey::parse(name) else {
            match Retired::spelt(name) {
                Some(spelling) => retired.push((spelling, value)),
                None => {
                    tracing::warn!(key = name, path = %path.display(), "ignoring an unknown setting");
                }
            }
            continue;
        };
        if let Err(error) = config.take(At { path, key }, value) {
            refused(error);
        }
    }
    for (spelling, value) in retired {
        spelling.take_unless_said_anew(&mut config, path, value);
    }
    Ok(config)
}

impl Config {
    fn take(&mut self, at: At<'_>, value: &Item) -> Result<()> {
        let config = self;
        let key = match at.key.sorted() {
            Sorted::Switch(switch) => {
                config.switches.say(switch, at.boolean(value)?);
                return Ok(());
            }
            Sorted::Valued(key) => key,
        };

        match key {
            Valued::Sink => config.sink = Some(NodeName::new(at.string(value)?)),
            Valued::Library => config.library = a_path(at.string(value)?),
            Valued::Vault => config.vault = a_path(at.string(value)?),
            Valued::Quality => config.quality = Some(at.one_of(value, quality)?),
            Valued::FilterPhase => {
                config.filter_phase = Some(at.one_of(value, filter_phase)?);
            }
            Valued::Dither => config.dither = Some(at.one_of(value, dither)?),
            Valued::NoiseShaping => {
                config.noise_shaping = Some(at.one_of(value, noise_shaping)?);
            }
            Valued::ReplayGain => config.replay_gain = Some(at.one_of(value, replay_gain)?),
            Valued::ReplayGainPreAmp => config.pre_amp = Some(at.trim(value)?),
            Valued::ReplayGainUntagged => config.untagged = Some(at.trim(value)?),
            Valued::BitPerfect => config.bit_perfect = Some(at.boolean(value)?),
            Valued::Dop => config.dop = Some(at.boolean(value)?),
            Valued::DsdLikePcm => config.dsd_like_pcm = Some(at.boolean(value)?),
            Valued::DeviceVolume => config.device_volume = Some(at.boolean(value)?),
            Valued::TruePeak => config.true_peak = Some(at.boolean(value)?),
            Valued::RestoreLossy => {
                config.restoration = Some(at.one_of(value, Restoration::named)?);
            }
            Valued::Crossfeed => {
                config.crossfeed = Some(at.one_of(value, Crossfeed::named)?);
            }
            Valued::ForceGraphRate => config.force_graph_rate = Some(at.boolean(value)?),
            Valued::BluetoothWake => config.bluetooth_wake = Some(at.boolean(value)?),
            Valued::BluetoothLeadMs => {
                let millis = at.within(value, &BLUETOOTH_LEAD_MS)?;

                config.bluetooth_lead = Some(Duration::from_millis(millis));
            }
            Valued::BluetoothAwakeS => {
                let seconds = at.within(value, &BLUETOOTH_AWAKE_S)?;

                config.bluetooth_awake = Some(Duration::from_secs(seconds));
            }
            Valued::Volume => {
                let position = at.float(value)?;
                let volume = Volume::new(position as f32).map_err(|_| at.rejected())?;

                config.volume = Some(volume);
            }
            Valued::Theme => config.theme = Some(at.one_of(value, Theme::parse)?),
            Valued::Accent => config.accent = Some(at.one_of(value, Accent::parse)?),
            Valued::TextSize => {
                config.text_size = Some(at.one_of(value, TextSize::parse)?);
            }
            Valued::Language => {
                config.language = Some(at.one_of(value, LanguageChoice::parse)?);
            }
            Valued::Region => config.region = Some(at.one_of(value, RegionChoice::parse)?),
            Valued::FollowDesktopScheme => {
                config.follow_desktop_scheme = Some(at.boolean(value)?);
            }
            Valued::FollowDesktopTextSize => {
                config.follow_desktop_text_size = Some(at.boolean(value)?);
            }
            Valued::MusicbrainzReplicationToken => {
                config.musicbrainz_replication_token = given(at.string(value)?);
            }
            Valued::SkipRepeatsQueue => {
                config.skip_repeats_queue = Some(at.boolean(value)?);
            }
            Valued::ShuffleStyle => {
                config.shuffle_style = Some(at.one_of(value, ShuffleStyle::parse)?);
            }
            Valued::ShuffleSpreadsArtists => {
                config.shuffle_spreads_artists = Some(at.boolean(value)?);
            }
            Valued::ShuffleSpreadsAlbums => {
                config.shuffle_spreads_albums = Some(at.boolean(value)?);
            }
            Valued::ShuffleFreshFirst => {
                config.shuffle_fresh_first = Some(at.boolean(value)?);
            }
            Valued::PreviousRestarts => {
                config.previous_restarts = Some(at.boolean(value)?);
            }
            Valued::AcoustidKey => config.acoustid_key = given(at.string(value)?),
            Valued::AuddToken => config.audd_token = given(at.string(value)?),
            Valued::ListenbrainzToken => config.listenbrainz_token = given(at.string(value)?),
            Valued::LastfmKey => config.lastfm_key = given(at.string(value)?),
            Valued::LastfmSecret => config.lastfm_secret = given(at.string(value)?),
            Valued::LastfmSession => config.lastfm_session = given(at.string(value)?),
            Valued::ListenFrom => {
                config.listen_from = Some(Listening::named(at.string(value)?));
            }
            Valued::ListenFor => {
                let seconds = at.within(value, &LISTENS_FOR_SECONDS)?;

                config.listen_for = Some(Duration::from_secs(seconds));
            }
            Valued::Inbox => config.inbox = a_path(at.string(value)?),
            Valued::MusicFolder => config.music_folder = a_path(at.string(value)?),
            Valued::MusicExtensions => {
                let array = value
                    .as_array()
                    .ok_or_else(|| at.mistyped(ValueKind::Array))?;
                let extensions = array
                    .iter()
                    .map(|extension| {
                        extension
                            .as_str()
                            .ok_or_else(|| at.mistyped(ValueKind::Text))
                    })
                    .collect::<Result<Vec<_>>>()?;

                config.music_extensions =
                    Some(MusicExtensions::parse(&extensions).ok_or_else(|| at.rejected())?);
            }
            Valued::MinimumLength => {
                let seconds = at.within(value, &(0..=u64::from(MinimumLength::MAX_SECONDS)))?;

                config.minimum_length = Some(
                    MinimumLength::new(u16::try_from(seconds).map_err(|_| at.rejected())?)
                        .ok_or_else(|| at.rejected())?,
                );
            }
            Valued::Convolution => config.convolution = a_path(at.string(value)?),
            Valued::SpeakerAngle => {
                let degrees = at.within(value, &SPEAKER_ANGLE_DEGREES)?;
                let degrees = u8::try_from(degrees).map_err(|_| at.rejected())?;

                config.speaker_angle =
                    Some(SpeakerAngle::from_degrees(degrees).map_err(|_| at.rejected())?);
            }
            Valued::SpeakerElevation => {
                let degrees = i16::try_from(at.integer(value)?).map_err(|_| at.rejected())?;

                config.speaker_elevation =
                    Some(SpeakerElevation::from_degrees(degrees).map_err(|_| at.rejected())?);
            }
            Valued::HeadYaw => {
                let degrees = i16::try_from(at.integer(value)?).map_err(|_| at.rejected())?;

                config.head_yaw = Some(HeadYaw::from_degrees(degrees).map_err(|_| at.rejected())?);
            }
            Valued::Subsonic => config.subsonic = given(at.string(value)?),
            Valued::SubsonicUser => config.subsonic_user = given(at.string(value)?),
            Valued::SubsonicPassword => {
                config.subsonic_password = given(at.string(value)?);
            }
            Valued::Jellyfin => config.jellyfin = given(at.string(value)?),
            Valued::JellyfinUser => config.jellyfin_user = given(at.string(value)?),
            Valued::JellyfinPassword => {
                config.jellyfin_password = given(at.string(value)?);
            }
            Valued::JellyfinToken => config.jellyfin_token = given(at.string(value)?),
            Valued::JellyfinUserId => config.jellyfin_user_id = given(at.string(value)?),
            Valued::JellyfinDevice => config.jellyfin_device = given(at.string(value)?),
            Valued::TidalClientId => config.tidal_client_id = given(at.string(value)?),
            Valued::TidalClientSecret => {
                config.tidal_client_secret = given(at.string(value)?);
            }
            Valued::TidalRefreshToken => {
                config.tidal_refresh_token = given(at.string(value)?);
            }
            Valued::SpotifyClientId => config.spotify_client_id = given(at.string(value)?),
            Valued::SpotifyRefreshToken => {
                config.spotify_refresh_token = given(at.string(value)?);
            }
            Valued::HifiApi => config.hifi_api = given(at.string(value)?),
            Valued::HifiSecret => config.hifi_secret = given(at.string(value)?),
            Valued::Monochrome => config.monochrome = given(at.string(value)?),
            Valued::YoutubeMusicCookies => {
                config.youtube_music_cookies = given(at.string(value)?);
            }
            Valued::YtDlp => config.yt_dlp = given(at.string(value)?),
            Valued::FlacAbove96kFor => config.flac_above_96k_for = node_names(at, value)?,
            Valued::FormatsLearntFor => config.formats_learnt_for = learnt(at, value)?,
            Valued::RenderersAt => config.renderers_at = renderer_addresses(at, value)?,
            Valued::CastAt => config.cast_at = cast_addresses(at, value)?,
            Valued::OfferName => config.offer_name = ServerName::new(at.string(value)?),
            Valued::HistoryKept => {
                config.history_kept = Some(match value.as_integer() {
                    Some(days) => HistoryKept::for_days(days).ok_or_else(|| at.rejected())?,
                    None => at.one_of(value, HistoryKept::parse)?,
                });
            }
            Valued::UpdateDismissed => config.update_dismissed = given(at.string(value)?),
            Valued::MinimiseButton => config.minimise_button = Some(at.boolean(value)?),
            Valued::MaximiseButton => config.maximise_button = Some(at.boolean(value)?),
            Valued::ReducedMotion => config.reduced_motion = Some(at.boolean(value)?),
            Valued::SpectrumTilt => {
                config.spectrum_tilt = Some(at.one_of(value, SpectrumTilt::parse)?);
            }
            Valued::SpectrumFloor => {
                config.spectrum_floor = Some(at.one_of(value, SpectrumFloor::parse)?);
            }
            Valued::SpectrumBands => {
                config.spectrum_bands = Some(at.one_of(value, SpectrumBands::parse)?);
            }
            Valued::SpectrumFalls => {
                config.spectrum_falls = Some(at.one_of(value, SpectrumFalls::parse)?);
            }
            Valued::Scrollbars => {
                config.scrollbars = Some(match value.as_bool() {
                    Some(drawn) => ScrollbarMode::of_a_switch(drawn),
                    None => at.one_of(value, ScrollbarMode::parse)?,
                });
            }
            Valued::SuggestionsTab => config.suggestions_tab = Some(at.boolean(value)?),
            Valued::MissingTab => config.missing_tab = Some(at.boolean(value)?),
            Valued::TabCounts => config.tab_counts = Some(at.boolean(value)?),
            Valued::LastTab => config.last_tab = Some(at.one_of(value, tab)?.to_owned()),
            Valued::ArtistsDrawn => {
                config.artists_drawn = Some(at.one_of(value, ArtistsDrawn::parse)?);
            }
            Valued::WindowSize => config.window_size = Some(at.one_of(value, window_size)?),
            Valued::LastSettingsCategory => {
                config.last_settings_category =
                    Some(at.one_of(value, settings_category)?.to_owned());
            }
            Valued::OrganiseAs => config.organise_as = Some(at.one_of(value, layout)?),
            Valued::RecompressTo => {
                config.recompress_to = Some(at.one_of(value, Ceiling::parse)?);
            }
            Valued::EqualiserFor => {
                config.equaliser_for.get_or_insert_default().by_sink = bindings(at, value)?;
            }
            Valued::EqualiserMeasurer => {
                config.equaliser_measurer = given(at.string(value)?);
            }
            Valued::EqualiserMeasurerFor => {
                config.equaliser_measurer_for = measurers(at, value)?;
            }
            Valued::EqualiserProfile => {
                config.equaliser_for.get_or_insert_default().fallback = at.binding(value)?;
            }
            Valued::Discord => config.discord = Some(at.boolean(value)?),
            Valued::DiscordApp => config.discord_app = at.unless_blank(value, AppId::parse)?,
            Valued::DiscordShows => {
                config.discord_shows = Some(at.one_of(value, Shown::parse)?);
            }
            Valued::DiscordArt => config.discord_art = Some(at.one_of(value, Pictured::parse)?),
            Valued::DiscordIcon => config.discord_icon = at.unless_blank(value, Icon::parse)?,
            Valued::DiscordProgress => config.discord_progress = Some(at.boolean(value)?),
            Valued::DiscordPaused => config.discord_paused = Some(at.boolean(value)?),
            Valued::SyncServer => config.sync_server = given(at.string(value)?),
            Valued::SyncServerCertificate => {
                config.sync_server_certificate = a_path(at.string(value)?);
            }
            Valued::SyncStun => config.sync_stun = given(at.string(value)?),
            Valued::SyncKey => config.sync_key = given(at.string(value)?),
            Valued::SyncDevice => config.sync_device = given(at.string(value)?),
            Valued::SharedName => config.shared_name = given(at.string(value)?),
            Valued::SyncSongs => {
                let array = value
                    .as_array()
                    .ok_or_else(|| at.mistyped(ValueKind::Array))?;
                let named = array
                    .iter()
                    .map(|name| name.as_str().ok_or_else(|| at.mistyped(ValueKind::Text)))
                    .collect::<Result<Vec<_>>>()?;

                config.sync_songs = Some(SyncedSongs::parse(&named).ok_or_else(|| at.rejected())?);
            }
            Valued::SyncControl => {
                config.sync_control = Some(at.one_of(value, RemoteTrust::parse)?);
            }
            Valued::PartyTimeServer => {
                config.party_time_server = Some(at.string(value)?.trim().to_owned());
            }
            Valued::BufferMs => {
                let millis = u64::try_from(at.integer(value)?).map_err(|_| at.rejected())?;

                config.buffer = Some(Duration::from_millis(millis));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct At<'a> {
    path: &'a Path,
    key: ConfigKey,
}

impl At<'_> {
    fn mistyped(self, expected: ValueKind) -> Error {
        Error::ConfigType {
            path: self.path.to_path_buf(),
            key: self.key,
            expected,
        }
    }

    fn rejected(self) -> Error {
        Error::ConfigValue {
            path: self.path.to_path_buf(),
            key: self.key,
        }
    }

    fn string(self, value: &Item) -> Result<&str> {
        value.as_str().ok_or_else(|| self.mistyped(ValueKind::Text))
    }

    fn boolean(self, value: &Item) -> Result<bool> {
        value
            .as_bool()
            .ok_or_else(|| self.mistyped(ValueKind::Boolean))
    }

    fn integer(self, value: &Item) -> Result<i64> {
        value
            .as_integer()
            .ok_or_else(|| self.mistyped(ValueKind::Integer))
    }

    fn within(self, value: &Item, range: &RangeInclusive<u64>) -> Result<u64> {
        u64::try_from(self.integer(value)?)
            .ok()
            .filter(|held| range.contains(held))
            .ok_or_else(|| self.rejected())
    }

    fn float(self, value: &Item) -> Result<f64> {
        value
            .as_float()
            .or_else(|| value.as_integer().map(|whole| whole as f64))
            .ok_or_else(|| self.mistyped(ValueKind::Number))
    }

    fn trim(self, value: &Item) -> Result<Trim> {
        Trim::from_decibels(self.float(value)?).map_err(|_| self.rejected())
    }

    fn one_of<T>(self, value: &Item, read: impl Fn(&str) -> Option<T>) -> Result<T> {
        read(self.string(value)?).ok_or_else(|| self.rejected())
    }

    fn unless_blank<T>(self, value: &Item, read: impl Fn(&str) -> Option<T>) -> Result<Option<T>> {
        given(self.string(value)?)
            .map(|text| read(&text).ok_or_else(|| self.rejected()))
            .transpose()
    }

    fn binding(self, value: &Item) -> Result<Option<Binding>> {
        if let Some(own) = value.as_bool() {
            return own
                .then_some(Some(Binding::Own))
                .ok_or_else(|| self.rejected());
        }

        let Some(named) = given(self.string(value)?) else {
            return Ok(None);
        };
        ProfileName::standing(&named)
            .map(|name| Some(Binding::Profile(name)))
            .map_err(|_| self.rejected())
    }

    fn table(self, value: &Item) -> Result<&dyn toml_edit::TableLike> {
        value
            .as_table_like()
            .ok_or_else(|| self.mistyped(ValueKind::Table))
    }
}

fn bindings(at: At<'_>, value: &Item) -> Result<BTreeMap<NodeName, Binding>> {
    let mut by_sink = BTreeMap::new();

    for (named, bound) in at.table(value)?.iter() {
        let Ok(Some(binding)) = at.binding(bound) else {
            tracing::warn!(
                sink = named,
                path = %at.path.display(),
                "ignoring a binding that names neither a profile nor the device's own curve"
            );
            continue;
        };

        by_sink.insert(NodeName::new(named), binding);
    }
    Ok(by_sink)
}

fn node_names(at: At<'_>, value: &Item) -> Result<BTreeSet<NodeName>> {
    let array = value
        .as_array()
        .ok_or_else(|| at.mistyped(ValueKind::Array))?;
    array
        .iter()
        .map(|named| {
            named
                .as_str()
                .map(|named| NodeName::new(named.trim()))
                .ok_or_else(|| at.mistyped(ValueKind::Text))
        })
        .collect()
}

fn learnt(at: At<'_>, value: &Item) -> Result<Learnt> {
    let mut learnt = Learnt::default();

    for (named, plays) in at.table(value)?.iter() {
        let Some(spellings) = plays.as_array() else {
            tracing::warn!(
                device = named,
                path = %at.path.display(),
                "ignoring what was learnt of a device that is not a list of formats"
            );
            continue;
        };
        let mut played = Vec::new();

        for spelling in spellings {
            match spelling.as_str().and_then(Trial::spelled) {
                Some(trial) => played.push(trial),
                None => tracing::warn!(
                    device = named,
                    path = %at.path.display(),
                    "ignoring a format learnt of a device that this build does not know"
                ),
            }
        }
        learnt.insert(NodeName::new(named.trim()), Plays::from_iter(played));
    }
    Ok(learnt)
}

fn renderer_addresses(at: At<'_>, value: &Item) -> Result<BTreeSet<Located>> {
    let array = value
        .as_array()
        .ok_or_else(|| at.mistyped(ValueKind::Array))?;
    let mut addresses = BTreeSet::new();

    for named in array {
        let address = named.as_str().ok_or_else(|| at.mistyped(ValueKind::Text))?;
        match Located::parse(address) {
            Ok(located) => {
                addresses.insert(located);
            }
            Err(error) if address.trim().is_empty() => {
                tracing::debug!(%error, "ignoring a blank renderer address");
            }
            Err(error) => {
                tracing::warn!(%error, address, path = %at.path.display(), "ignoring a renderer address that is not an http address");
            }
        }
    }
    Ok(addresses)
}

fn cast_addresses(at: At<'_>, value: &Item) -> Result<BTreeSet<Given>> {
    let array = value
        .as_array()
        .ok_or_else(|| at.mistyped(ValueKind::Array))?;
    let mut addresses = BTreeSet::new();

    for named in array {
        let address = named.as_str().ok_or_else(|| at.mistyped(ValueKind::Text))?;
        match Given::parse(address) {
            Some(receiver) => {
                addresses.insert(receiver);
            }
            None if address.trim().is_empty() => {
                tracing::debug!("ignoring a blank Cast receiver address");
            }
            None => {
                tracing::warn!(address, path = %at.path.display(), "ignoring a Cast receiver address that is neither an IP address nor a host name");
            }
        }
    }
    Ok(addresses)
}

fn measurers(at: At<'_>, value: &Item) -> Result<BTreeMap<NodeName, String>> {
    let mut by_sink = BTreeMap::new();

    for (named, preferred) in at.table(value)?.iter() {
        let Some(measurer) = preferred.as_str().and_then(given) else {
            tracing::warn!(
                sink = named,
                path = %at.path.display(),
                "ignoring a preferred measurer that is not a name"
            );
            continue;
        };

        by_sink.insert(NodeName::new(named), measurer);
    }
    Ok(by_sink)
}

fn given(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

const HOME: &str = "~";

pub fn a_path(text: &str) -> Option<PathBuf> {
    let text = given(text)?;
    let under_home = match text.strip_prefix(HOME) {
        Some("") => Some(""),
        Some(rest) => rest
            .strip_prefix('/')
            .or_else(|| separated_by_the_platform(rest)),
        None => None,
    };
    let home = home_dir();
    match (under_home, home) {
        (Some(rest), Some(home)) => Some(home.join(rest)),
        _ => Some(PathBuf::from(text)),
    }
}

#[cfg(windows)]
fn separated_by_the_platform(rest: &str) -> Option<&str> {
    rest.strip_prefix('\\')
}

#[cfg(unix)]
fn separated_by_the_platform(_: &str) -> Option<&str> {
    None
}

fn layout(text: &str) -> Option<Layout> {
    Layout::read(text).ok()
}

pub(super) fn tab(text: &str) -> Option<&'static str> {
    match text {
        "albums" => Some("albums"),
        "artists" => Some("artists"),
        "tracks" => Some("tracks"),
        "statistics" => Some("statistics"),
        "queue" => Some("queue"),
        "playlists" => Some("playlists"),
        "party" => Some("party"),
        "favourites" => Some("favourites"),
        "suggestions" => Some("suggestions"),
        "missing" => Some("missing"),
        "server" => Some("server"),
        "lyrics" => Some("lyrics"),
        "inspector" => Some("inspector"),
        "visualiser" => Some("visualiser"),
        "analysis" => Some("analysis"),
        "settings" => Some("settings"),
        _ => None,
    }
}

fn window_size(text: &str) -> Option<(u32, u32)> {
    let (width, height) = text.split_once('x')?;

    if height.contains('x') {
        return None;
    }

    let (width, height) = (width.parse::<u32>().ok()?, height.parse::<u32>().ok()?);
    (width >= 720 && height >= 520 && width <= 8192 && height <= 8192).then_some((width, height))
}

pub(super) const SETTINGS_CATEGORIES: [&str; 14] = [
    "output",
    "processing",
    "equaliser",
    "playback",
    "library",
    "filters",
    "online",
    "recognition",
    "accounts",
    "sync",
    "desktop",
    "appearance",
    "window",
    "about",
];

pub(super) const SETTINGS_CATEGORIES_SPELT_BEFORE: [&str; 2] = ["enrichment", "scrobbling"];

pub(super) fn settings_category(text: &str) -> Option<&'static str> {
    SETTINGS_CATEGORIES
        .into_iter()
        .chain(SETTINGS_CATEGORIES_SPELT_BEFORE)
        .find(|category| *category == text)
}

fn quality(text: &str) -> Option<Quality> {
    QualityArg::from_str(text, false).ok().map(Meaning::meaning)
}

fn filter_phase(text: &str) -> Option<FilterPhase> {
    FilterPhaseArg::from_str(text, false)
        .ok()
        .map(Meaning::meaning)
}

fn dither(text: &str) -> Option<DitherKind> {
    DitherArg::from_str(text, false).ok().map(Meaning::meaning)
}

fn noise_shaping(text: &str) -> Option<NoiseShaping> {
    NoiseShapingArg::from_str(text, false)
        .ok()
        .map(Meaning::meaning)
}

fn replay_gain(text: &str) -> Option<ReplayGainMode> {
    match text {
        "off" => Some(ReplayGainMode::Off),
        "track" => Some(ReplayGainMode::Track),
        "album" => Some(ReplayGainMode::Album),
        _ => None,
    }
}
