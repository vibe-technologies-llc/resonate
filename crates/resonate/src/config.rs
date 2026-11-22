use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::PathBuf,
    time::Duration,
};

use resonate_cast::Given;
use resonate_core::{
    Accent, AppId, ArtistsDrawn, HeadYaw, Icon, Pictured, Presence, RemoteTrust, ScrollbarMode,
    Shown, SpeakerAngle, SpeakerElevation, Speakers, SpectrumBands, SpectrumFalls, SpectrumFloor,
    SpectrumTilt, Switch, Switches, SyncedSongs, TextSize, Theme, Trim, Volume,
};
#[cfg(any(feature = "ui", test))]
use resonate_core::{Appearance, Following};
use resonate_engine::{
    Crossfeed, DitherKind, FilterPhase, Learnt, NodeName, NoiseShaping, PreviousRestarts, Quality,
    ReplayGainMode, Restoration, ShuffleStyle, SkipUnderRepeat, SmartShuffle,
};
use resonate_eq::Binding;
use resonate_library::{HistoryKept, Layout, MinimumLength, MusicExtensions, MusicFilters};
use resonate_listen::Listening;
use resonate_upnp::{Located, ServerName, Udn, Vouched};
use resonate_words::{Language, LanguageChoice, RegionChoice};

mod places;
mod reading;
mod writing;

#[cfg(feature = "ui")]
pub use places::analyses_dir;
#[cfg(feature = "online")]
pub use places::data_dir;
#[cfg(all(feature = "ui", unix, not(target_os = "macos")))]
pub use places::icon_theme_dir;
#[cfg(feature = "sync")]
pub use places::private_store;
pub use places::{config_path, equaliser_dir, library_path, private_data_dir, vault_dir};
#[cfg(feature = "online")]
pub use reading::a_path;
#[cfg(feature = "online")]
pub use reading::{contact_in, spotify_in, submitting_in, tidal_refresh_token_in};
pub use reading::{load, spoken_before_the_grammar};
#[cfg(feature = "ui")]
pub use writing::Editing;
#[cfg(any(feature = "online", feature = "ui"))]
pub use writing::edit;
pub use writing::{clear, clear_in_table, learnt_written, store, store_in_table, written};

#[cfg(test)]
mod tests;

#[derive(Clone, Default, PartialEq)]
pub struct Config {
    pub sink: Option<NodeName>,
    pub library: Option<PathBuf>,
    pub vault: Option<PathBuf>,
    pub quality: Option<Quality>,
    pub filter_phase: Option<FilterPhase>,
    pub true_peak: Option<bool>,
    pub restoration: Option<Restoration>,
    pub crossfeed: Option<Crossfeed>,
    pub dither: Option<DitherKind>,
    pub noise_shaping: Option<NoiseShaping>,
    pub replay_gain: Option<ReplayGainMode>,
    pub pre_amp: Option<Trim>,
    pub untagged: Option<Trim>,
    pub bit_perfect: Option<bool>,
    pub dop: Option<bool>,
    pub dsd_like_pcm: Option<bool>,
    pub device_volume: Option<bool>,
    pub force_graph_rate: Option<bool>,
    pub bluetooth_wake: Option<bool>,
    pub bluetooth_lead: Option<Duration>,
    pub bluetooth_awake: Option<Duration>,
    pub volume: Option<Volume>,
    pub buffer: Option<Duration>,
    pub theme: Option<Theme>,
    pub accent: Option<Accent>,
    pub text_size: Option<TextSize>,
    pub language: Option<LanguageChoice>,
    pub region: Option<RegionChoice>,
    pub follow_desktop_scheme: Option<bool>,
    pub follow_desktop_text_size: Option<bool>,
    pub switches: Switches,
    pub musicbrainz_replication_token: Option<String>,
    pub skip_repeats_queue: Option<bool>,
    pub previous_restarts: Option<bool>,
    pub shuffle_style: Option<ShuffleStyle>,
    pub shuffle_spreads_artists: Option<bool>,
    pub shuffle_spreads_albums: Option<bool>,
    pub shuffle_fresh_first: Option<bool>,
    pub contact: Option<String>,
    pub read_from: Option<PathBuf>,
    pub acoustid_key: Option<String>,
    pub audd_token: Option<String>,
    pub listenbrainz_token: Option<String>,
    pub lastfm_key: Option<String>,
    pub lastfm_secret: Option<String>,
    pub lastfm_session: Option<String>,
    pub listen_from: Option<Listening>,
    pub listen_for: Option<Duration>,
    pub equaliser_for: Option<Bindings>,
    pub equaliser_measurer: Option<String>,
    pub equaliser_measurer_for: BTreeMap<NodeName, String>,
    pub flac_above_96k_for: BTreeSet<NodeName>,
    pub formats_learnt_for: Learnt,
    pub renderers_at: BTreeSet<Located>,
    pub cast_at: BTreeSet<Given>,
    pub offer_name: Option<ServerName>,
    pub history_kept: Option<HistoryKept>,
    pub organise_as: Option<Layout>,
    pub update_dismissed: Option<String>,
    pub minimise_button: Option<bool>,
    pub maximise_button: Option<bool>,
    pub reduced_motion: Option<bool>,
    pub scrollbars: Option<ScrollbarMode>,
    pub spectrum_tilt: Option<SpectrumTilt>,
    pub spectrum_floor: Option<SpectrumFloor>,
    pub spectrum_bands: Option<SpectrumBands>,
    pub spectrum_falls: Option<SpectrumFalls>,
    pub suggestions_tab: Option<bool>,
    pub missing_tab: Option<bool>,
    pub tab_counts: Option<bool>,
    pub last_tab: Option<String>,
    pub artists_drawn: Option<ArtistsDrawn>,
    pub window_size: Option<(u32, u32)>,
    pub last_settings_category: Option<String>,
    pub inbox: Option<PathBuf>,
    pub music_folder: Option<PathBuf>,
    pub music_extensions: Option<MusicExtensions>,
    pub minimum_length: Option<MinimumLength>,
    pub convolution: Option<PathBuf>,
    pub speaker_angle: Option<SpeakerAngle>,
    pub speaker_elevation: Option<SpeakerElevation>,
    pub head_yaw: Option<HeadYaw>,
    pub subsonic: Option<String>,
    pub subsonic_user: Option<String>,
    pub subsonic_password: Option<String>,
    pub jellyfin: Option<String>,
    pub jellyfin_user: Option<String>,
    pub jellyfin_password: Option<String>,
    pub jellyfin_token: Option<String>,
    pub jellyfin_user_id: Option<String>,
    pub jellyfin_device: Option<String>,
    pub tidal_client_id: Option<String>,
    pub tidal_client_secret: Option<String>,
    pub tidal_refresh_token: Option<String>,
    pub spotify_client_id: Option<String>,
    pub spotify_refresh_token: Option<String>,
    pub hifi_api: Option<String>,
    pub hifi_secret: Option<String>,
    pub monochrome: Option<String>,
    pub youtube_music_cookies: Option<String>,
    pub yt_dlp: Option<String>,
    pub discord: Option<bool>,
    pub discord_app: Option<AppId>,
    pub discord_shows: Option<Shown>,
    pub discord_art: Option<Pictured>,
    pub discord_icon: Option<Icon>,
    pub discord_progress: Option<bool>,
    pub discord_paused: Option<bool>,
    pub sync_server: Option<String>,
    pub sync_server_certificate: Option<PathBuf>,
    pub sync_stun: Option<String>,
    pub sync_key: Option<String>,
    pub sync_device: Option<String>,
    pub shared_name: Option<String>,
    pub sync_songs: Option<SyncedSongs>,
    pub sync_control: Option<RemoteTrust>,
    pub party_time_server: Option<String>,
}

struct Withheld;

impl fmt::Debug for Withheld {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<withheld>")
    }
}

fn withheld(secret: Option<&String>) -> Option<Withheld> {
    secret.map(|_| Withheld)
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            sink,
            library,
            vault,
            quality,
            filter_phase,
            true_peak,
            restoration,
            crossfeed,
            dither,
            noise_shaping,
            replay_gain,
            pre_amp,
            untagged,
            bit_perfect,
            dop,
            dsd_like_pcm,
            device_volume,
            force_graph_rate,
            bluetooth_wake,
            bluetooth_lead,
            bluetooth_awake,
            volume,
            buffer,
            theme,
            accent,
            text_size,
            language,
            region,
            follow_desktop_scheme,
            follow_desktop_text_size,
            switches,
            musicbrainz_replication_token,
            skip_repeats_queue,
            previous_restarts,
            shuffle_style,
            shuffle_spreads_artists,
            shuffle_spreads_albums,
            shuffle_fresh_first,
            contact,
            read_from,
            acoustid_key,
            audd_token,
            listenbrainz_token,
            lastfm_key,
            lastfm_secret,
            lastfm_session,
            listen_from,
            listen_for,
            equaliser_for,
            equaliser_measurer,
            equaliser_measurer_for,
            flac_above_96k_for,
            formats_learnt_for,
            renderers_at,
            cast_at,
            offer_name,
            history_kept,
            organise_as,
            update_dismissed,
            minimise_button,
            maximise_button,
            reduced_motion,
            scrollbars,
            spectrum_tilt,
            spectrum_floor,
            spectrum_bands,
            spectrum_falls,
            suggestions_tab,
            missing_tab,
            tab_counts,
            last_tab,
            artists_drawn,
            window_size,
            last_settings_category,
            inbox,
            music_folder,
            music_extensions,
            minimum_length,
            convolution,
            speaker_angle,
            speaker_elevation,
            head_yaw,
            subsonic,
            subsonic_user,
            subsonic_password,
            jellyfin,
            jellyfin_user,
            jellyfin_password,
            jellyfin_token,
            jellyfin_user_id,
            jellyfin_device,
            tidal_client_id,
            tidal_client_secret,
            tidal_refresh_token,
            spotify_client_id,
            spotify_refresh_token,
            hifi_api,
            hifi_secret,
            monochrome,
            youtube_music_cookies,
            yt_dlp,
            discord,
            discord_app,
            discord_shows,
            discord_art,
            discord_icon,
            discord_progress,
            discord_paused,
            sync_server,
            sync_server_certificate,
            sync_stun,
            sync_key,
            sync_device,
            shared_name,
            sync_songs,
            sync_control,
            party_time_server,
        } = self;

        f.debug_struct("Config")
            .field("sink", sink)
            .field("library", library)
            .field("vault", vault)
            .field("quality", quality)
            .field("filter_phase", filter_phase)
            .field("true_peak", true_peak)
            .field("restoration", restoration)
            .field("crossfeed", crossfeed)
            .field("dither", dither)
            .field("noise_shaping", noise_shaping)
            .field("replay_gain", replay_gain)
            .field("pre_amp", pre_amp)
            .field("untagged", untagged)
            .field("bit_perfect", bit_perfect)
            .field("dop", dop)
            .field("dsd_like_pcm", dsd_like_pcm)
            .field("device_volume", device_volume)
            .field("force_graph_rate", force_graph_rate)
            .field("bluetooth_wake", bluetooth_wake)
            .field("bluetooth_lead", bluetooth_lead)
            .field("bluetooth_awake", bluetooth_awake)
            .field("volume", volume)
            .field("buffer", buffer)
            .field("theme", theme)
            .field("accent", accent)
            .field("text_size", text_size)
            .field("language", language)
            .field("region", region)
            .field("follow_desktop_scheme", follow_desktop_scheme)
            .field("follow_desktop_text_size", follow_desktop_text_size)
            .field("switches", switches)
            .field(
                "musicbrainz_replication_token",
                &withheld(musicbrainz_replication_token.as_ref()),
            )
            .field("skip_repeats_queue", skip_repeats_queue)
            .field("previous_restarts", previous_restarts)
            .field("shuffle_style", shuffle_style)
            .field("shuffle_spreads_artists", shuffle_spreads_artists)
            .field("shuffle_spreads_albums", shuffle_spreads_albums)
            .field("shuffle_fresh_first", shuffle_fresh_first)
            .field("contact", &withheld(contact.as_ref()))
            .field("read_from", read_from)
            .field("acoustid_key", &withheld(acoustid_key.as_ref()))
            .field("audd_token", &withheld(audd_token.as_ref()))
            .field("listenbrainz_token", &withheld(listenbrainz_token.as_ref()))
            .field("lastfm_key", &withheld(lastfm_key.as_ref()))
            .field("lastfm_secret", &withheld(lastfm_secret.as_ref()))
            .field("lastfm_session", &withheld(lastfm_session.as_ref()))
            .field("listen_from", listen_from)
            .field("listen_for", listen_for)
            .field("equaliser_for", equaliser_for)
            .field("equaliser_measurer", equaliser_measurer)
            .field("equaliser_measurer_for", equaliser_measurer_for)
            .field("flac_above_96k_for", flac_above_96k_for)
            .field("formats_learnt_for", formats_learnt_for)
            .field("renderers_at", renderers_at)
            .field("cast_at", cast_at)
            .field("offer_name", offer_name)
            .field("offer_name", offer_name)
            .field("history_kept", history_kept)
            .field("organise_as", organise_as)
            .field("update_dismissed", update_dismissed)
            .field("minimise_button", minimise_button)
            .field("maximise_button", maximise_button)
            .field("reduced_motion", reduced_motion)
            .field("scrollbars", scrollbars)
            .field("spectrum_tilt", spectrum_tilt)
            .field("spectrum_floor", spectrum_floor)
            .field("spectrum_bands", spectrum_bands)
            .field("spectrum_falls", spectrum_falls)
            .field("suggestions_tab", suggestions_tab)
            .field("missing_tab", missing_tab)
            .field("tab_counts", tab_counts)
            .field("last_tab", last_tab)
            .field("artists_drawn", artists_drawn)
            .field("window_size", window_size)
            .field("last_settings_category", last_settings_category)
            .field("inbox", inbox)
            .field("music_folder", music_folder)
            .field("music_extensions", music_extensions)
            .field("minimum_length", minimum_length)
            .field("convolution", convolution)
            .field("speaker_angle", speaker_angle)
            .field("speaker_elevation", speaker_elevation)
            .field("head_yaw", head_yaw)
            .field("subsonic", subsonic)
            .field("subsonic_user", subsonic_user)
            .field("subsonic_password", &withheld(subsonic_password.as_ref()))
            .field("jellyfin", jellyfin)
            .field("jellyfin_user", jellyfin_user)
            .field("jellyfin_password", &withheld(jellyfin_password.as_ref()))
            .field("jellyfin_token", &withheld(jellyfin_token.as_ref()))
            .field("jellyfin_user_id", jellyfin_user_id)
            .field("jellyfin_device", jellyfin_device)
            .field("tidal_client_id", tidal_client_id)
            .field(
                "tidal_client_secret",
                &withheld(tidal_client_secret.as_ref()),
            )
            .field(
                "tidal_refresh_token",
                &withheld(tidal_refresh_token.as_ref()),
            )
            .field("spotify_client_id", spotify_client_id)
            .field(
                "spotify_refresh_token",
                &withheld(spotify_refresh_token.as_ref()),
            )
            .field("hifi_api", hifi_api)
            .field("hifi_secret", &withheld(hifi_secret.as_ref()))
            .field("monochrome", monochrome)
            .field(
                "youtube_music_cookies",
                &withheld(youtube_music_cookies.as_ref()),
            )
            .field("yt_dlp", yt_dlp)
            .field("discord", discord)
            .field("discord_app", discord_app)
            .field("discord_shows", discord_shows)
            .field("discord_art", discord_art)
            .field("discord_icon", discord_icon)
            .field("discord_progress", discord_progress)
            .field("discord_paused", discord_paused)
            .field("sync_server", sync_server)
            .field("sync_server_certificate", sync_server_certificate)
            .field("sync_stun", sync_stun)
            .field("sync_key", &withheld(sync_key.as_ref()))
            .field("sync_device", sync_device)
            .field("shared_name", shared_name)
            .field("sync_songs", sync_songs)
            .field("sync_control", sync_control)
            .field("party_time_server", party_time_server)
            .finish()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bindings {
    fallback: Option<Binding>,
    by_sink: BTreeMap<NodeName, Binding>,
}

impl Bindings {
    pub const fn fallback(&self) -> Option<&Binding> {
        self.fallback.as_ref()
    }

    pub fn by_sink(&self) -> impl Iterator<Item = (&NodeName, &Binding)> {
        self.by_sink.iter()
    }

    pub fn of(&self, owner: Option<&NodeName>) -> Option<&Binding> {
        match owner {
            Some(sink) => self.by_sink.get(sink),
            None => self.fallback(),
        }
    }

    pub fn for_sink(&self, sink: Option<&NodeName>) -> Option<(Option<&NodeName>, &Binding)> {
        sink.and_then(|sink| self.by_sink.get_key_value(sink))
            .map(|(owner, bound)| (Some(owner), bound))
            .or_else(|| self.fallback().map(|bound| (None, bound)))
    }

    pub fn is_empty(&self) -> bool {
        self.fallback.is_none() && self.by_sink.is_empty()
    }
}

impl Config {
    pub fn language(&self) -> LanguageChoice {
        self.language.unwrap_or_default()
    }

    pub fn region(&self) -> RegionChoice {
        self.region.unwrap_or_default()
    }

    pub fn language_spoken(&self) -> Language {
        self.language()
            .language(&resonate_words::languages_the_listener_reads())
    }

    #[cfg(test)]
    pub fn appearance(&self) -> Appearance {
        self.appearance_where_the_desktop_asks(false)
    }

    #[cfg(any(feature = "ui", test))]
    pub fn appearance_where_the_desktop_asks(&self, for_more_contrast: bool) -> Appearance {
        let worn = Appearance::default();
        let unasked = if for_more_contrast {
            Theme::HighContrast
        } else {
            worn.theme
        };

        Appearance {
            theme: self.theme.unwrap_or(unasked),
            accent: self.accent.or(worn.accent),
            text_size: self.text_size.unwrap_or(worn.text_size),
            follows: Following {
                scheme: self
                    .follow_desktop_scheme
                    .unwrap_or_else(|| self.theme.is_none()),
                text: self
                    .follow_desktop_text_size
                    .unwrap_or_else(|| self.text_size.is_none()),
            },
        }
    }

    #[cfg(any(feature = "sync", test))]
    pub fn party_time_server(&self) -> Option<String> {
        if !self.is_on(Switch::Online) {
            return None;
        }
        match self.party_time_server.as_deref() {
            None => Some(resonate_core::PARTY_TIME_SERVER.to_owned()),
            Some("") => None,
            Some(server) => Some(server.to_owned()),
        }
    }

    #[cfg(any(feature = "sync", test))]
    pub fn syncs(&self) -> bool {
        self.is_on(Switch::Sync)
            && (self.is_on(Switch::Online) || self.is_on(Switch::SyncNearbyOnly))
    }

    #[cfg(feature = "online")]
    pub fn submits_to(&self) -> Accounts {
        if !self.is_on(Switch::Online) {
            return Accounts::default();
        }
        Accounts {
            listenbrainz: self.listenbrainz_token.clone(),
            lastfm: LastfmAccount::of(
                self.lastfm_key.clone(),
                self.lastfm_secret.clone(),
                self.lastfm_session.clone(),
            ),
        }
    }

    #[cfg(feature = "online")]
    pub fn musicbrainz_replication_token(&self) -> Option<&str> {
        self.musicbrainz_replication_token.as_deref()
    }

    pub fn skip_under_repeat(&self) -> SkipUnderRepeat {
        match self.skip_repeats_queue {
            Some(false) => SkipUnderRepeat::KeepsRepeatingTheTrack,
            Some(true) | None => SkipUnderRepeat::RepeatsTheQueue,
        }
    }

    pub fn smart_shuffle(&self) -> SmartShuffle {
        let defaults = SmartShuffle::default();
        SmartShuffle {
            style: self.shuffle_style.unwrap_or(defaults.style),
            spreads_artists: self
                .shuffle_spreads_artists
                .unwrap_or(defaults.spreads_artists),
            spreads_albums: self
                .shuffle_spreads_albums
                .unwrap_or(defaults.spreads_albums),
            fresh_first: self.shuffle_fresh_first.unwrap_or(defaults.fresh_first),
        }
    }

    pub fn previous_restarts(&self) -> PreviousRestarts {
        match self.previous_restarts {
            Some(false) => PreviousRestarts::AlwaysGoesBack,
            Some(true) | None => PreviousRestarts::RestartsTheTrack,
        }
    }

    pub fn equaliser_measurer(&self) -> Option<&str> {
        self.equaliser_measurer.as_deref()
    }

    pub fn equaliser_measurer_for(&self, sink: Option<&NodeName>) -> Option<&str> {
        sink.and_then(|sink| self.equaliser_measurer_for.get(sink))
            .map(String::as_str)
            .or_else(|| self.equaliser_measurer())
    }

    pub fn renderers_vouched_for(&self) -> Vouched {
        Vouched::flac_at_any_rate(self.flac_above_96k_for.iter().filter_map(Udn::from_node))
    }

    pub const fn formats_learnt_for(&self) -> &Learnt {
        &self.formats_learnt_for
    }

    pub const fn renderers_at(&self) -> &BTreeSet<Located> {
        &self.renderers_at
    }

    pub const fn cast_at(&self) -> &BTreeSet<Given> {
        &self.cast_at
    }

    pub const fn offer_name(&self) -> Option<&ServerName> {
        self.offer_name.as_ref()
    }

    pub fn holds_receiver_addresses(&self) -> bool {
        !(self.renderers_at.is_empty() && self.cast_at.is_empty())
    }

    pub fn history_kept(&self) -> HistoryKept {
        self.history_kept.unwrap_or_default()
    }

    #[cfg(feature = "ui")]
    pub fn window_buttons(&self) -> resonate_ui::WindowButtons {
        let shown = resonate_ui::WindowButtons::SHOWN;

        resonate_ui::WindowButtons {
            minimise: self.minimise_button.unwrap_or(shown.minimise),
            maximise: self.maximise_button.unwrap_or(shown.maximise),
        }
    }

    #[cfg(any(feature = "ui", test))]
    pub fn reduces_motion_where_the_desktop_asks(&self, desktop_reduces: bool) -> bool {
        self.reduced_motion.unwrap_or(desktop_reduces)
    }

    pub fn speaker_angle(&self) -> SpeakerAngle {
        self.speaker_angle.unwrap_or_default()
    }

    pub fn speaker_elevation(&self) -> SpeakerElevation {
        self.speaker_elevation.unwrap_or_default()
    }

    pub fn head_yaw(&self) -> HeadYaw {
        self.head_yaw.unwrap_or_default()
    }

    pub fn speakers(&self) -> Speakers {
        Speakers {
            angle: self.speaker_angle(),
            elevation: self.speaker_elevation(),
            yaw: self.head_yaw(),
        }
    }

    #[cfg(feature = "ui")]
    pub fn scrollbars(&self) -> ScrollbarMode {
        self.scrollbars.unwrap_or_default()
    }

    #[cfg(feature = "ui")]
    pub fn spectral(&self) -> resonate_core::Spectral {
        resonate_core::Spectral {
            tilt: self.spectrum_tilt.unwrap_or_default(),
            floor: self.spectrum_floor.unwrap_or_default(),
            bands: self.spectrum_bands.unwrap_or_default(),
            falls: self.spectrum_falls.unwrap_or_default(),
        }
    }

    #[cfg(feature = "ui")]
    pub fn artists_drawn(&self) -> ArtistsDrawn {
        self.artists_drawn.unwrap_or_default()
    }

    #[cfg(feature = "ui")]
    pub fn tabs(&self) -> resonate_ui::Tabs {
        let built = resonate_ui::Tabs::AS_BUILT;

        resonate_ui::Tabs {
            suggestions: self.suggestions_tab.unwrap_or(built.suggestions),
            missing: self.missing_tab.unwrap_or(built.missing),
            counts: self.tab_counts.unwrap_or(built.counts),
        }
    }

    #[cfg(feature = "ui")]
    pub fn remembered_tab(&self) -> Option<resonate_ui::Pane> {
        self.last_tab.as_deref().and_then(resonate_ui::Pane::parse)
    }

    #[cfg(feature = "ui")]
    pub fn window_size(&self) -> Option<resonate_ui::WindowSize> {
        let (width, height) = self.window_size?;
        resonate_ui::WindowSize::new(width, height)
    }

    #[cfg(feature = "ui")]
    pub fn remembered_settings_category(&self) -> resonate_ui::SettingsCategory {
        self.last_settings_category
            .as_deref()
            .and_then(resonate_ui::SettingsCategory::parse)
            .unwrap_or_default()
    }

    pub fn presence(&self) -> Presence {
        let off = Presence::OFF;

        Presence {
            enabled: self.discord.unwrap_or(off.enabled),
            app: self.discord_app.or(off.app),
            shown: self.discord_shows.unwrap_or(off.shown),
            pictured: self.discord_art.unwrap_or(off.pictured),
            icon: self.discord_icon.clone().or(off.icon),
            progress: self.discord_progress.unwrap_or(off.progress),
            while_paused: self.discord_paused.unwrap_or(off.while_paused),
        }
    }

    pub fn organise_as(&self) -> Layout {
        self.organise_as.clone().unwrap_or_default()
    }
}

#[cfg(feature = "online")]
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Accounts {
    pub listenbrainz: Option<String>,
    pub lastfm: Option<LastfmAccount>,
}

#[cfg(feature = "online")]
#[derive(Clone, PartialEq, Eq)]
pub struct LastfmAccount {
    pub key: String,
    pub secret: String,
    pub session: String,
}

#[cfg(feature = "online")]
impl LastfmAccount {
    fn of(key: Option<String>, secret: Option<String>, session: Option<String>) -> Option<Self> {
        Some(Self {
            key: key?,
            secret: secret?,
            session: session?,
        })
    }
}

#[cfg(feature = "online")]
impl fmt::Debug for LastfmAccount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LastfmAccount")
            .field("key", &Withheld)
            .field("secret", &Withheld)
            .field("session", &Withheld)
            .finish()
    }
}

#[cfg(feature = "online")]
impl fmt::Debug for Accounts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Accounts")
            .field("listenbrainz", &withheld(self.listenbrainz.as_ref()))
            .field("lastfm", &self.lastfm.as_ref().map(|_| Withheld))
            .finish()
    }
}

impl Config {
    pub const fn is_on(&self, switch: Switch) -> bool {
        self.switches.is_on(switch)
    }

    pub fn music_filters(&self) -> MusicFilters {
        MusicFilters {
            extensions: self.music_extensions.unwrap_or_default(),
            minimum_length: self.minimum_length.unwrap_or_default(),
        }
    }
}

impl Config {
    pub fn spotify_signed_in(&self) -> bool {
        let given =
            |held: &Option<String>| held.as_deref().is_some_and(|held| !held.trim().is_empty());
        given(&self.spotify_client_id) && given(&self.spotify_refresh_token)
    }
}
