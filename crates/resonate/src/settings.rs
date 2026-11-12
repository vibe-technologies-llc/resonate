use std::path::PathBuf;

use clap::ValueEnum;
use resonate_core::Trim;
use resonate_engine::{
    DitherKind, FilterPhase, NodeName, NoiseShaping, PreviousRestarts, Quality, ReplayGainMode,
    SkipUnderRepeat,
};
use resonate_eq::Binding;
use resonate_library::HistoryKept;
use resonate_ui::{Setting, SettingChange, Settings};
use toml_edit::{Array, Value};

use crate::{
    ConfigKey,
    cli::{DitherArg, FilterPhaseArg, NoiseShapingArg, QualityArg},
    config::{self, Editing},
    metered, online,
};

const MILLIBELS_PER_DECIBEL: f64 = 100.0;

pub struct File {
    path: PathBuf,
}

impl File {
    pub const fn at(path: PathBuf) -> Self {
        Self { path }
    }
}

fn written(editing: &mut Editing<'_>, key: ConfigKey, value: Option<Value>) {
    match value {
        Some(value) => editing.store(key, value),
        None => editing.clear(key),
    }
}

fn bound(
    editing: &mut Editing<'_>,
    sink: &NodeName,
    binding: Option<&Binding>,
) -> crate::Result<()> {
    match binding {
        Some(binding) => editing.store_in_table(
            ConfigKey::EqualiserFor,
            sink.as_str(),
            config::written(binding),
        ),
        None => {
            editing.clear_in_table(ConfigKey::EqualiserFor, sink.as_str());
            Ok(())
        }
    }
}

fn preferred(
    editing: &mut Editing<'_>,
    sink: &NodeName,
    measurer: Option<&str>,
) -> crate::Result<()> {
    match measurer
        .map(str::trim)
        .filter(|measurer| !measurer.is_empty())
    {
        Some(measurer) => {
            editing.store_in_table(ConfigKey::EqualiserMeasurerFor, sink.as_str(), measurer)
        }
        None => {
            editing.clear_in_table(ConfigKey::EqualiserMeasurerFor, sink.as_str());
            Ok(())
        }
    }
}

impl Settings for File {
    fn apply(&self, changes: &[SettingChange]) -> resonate_ui::Result<()> {
        let Some(first) = changes.first() else {
            return Ok(());
        };
        let mut refused = None;
        let written = config::edit(&self.path, |editing| {
            for change in changes {
                let applied = match change {
                    SettingChange::Store(setting) => stored(editing, setting),
                    SettingChange::Forget(key) => {
                        forgotten(editing, *key);
                        Ok(())
                    }
                };
                if let Err(error) = applied {
                    refused.get_or_insert(error);
                }
            }
            Ok(())
        });
        if let Err(error) = written {
            tracing::error!(%error, "the settings file could not be written");
            return Err(resonate_ui::Error::SettingNotStored { key: first.key() });
        }

        for change in changes {
            match change {
                SettingChange::Store(Setting::Contact(contact)) => {
                    online::introduce(Some(contact.trim()).filter(|contact| !contact.is_empty()));
                }
                SettingChange::Forget(ConfigKey::Contact) => online::introduce(None),
                SettingChange::Store(Setting::Online(on)) => online::reach(*on),
                #[cfg(feature = "online")]
                SettingChange::Store(Setting::EnrichOnMetered(allowed)) => {
                    metered::allow_lookups(*allowed);
                }
                #[cfg(feature = "online")]
                SettingChange::Forget(ConfigKey::EnrichOnMetered) => {
                    metered::allow_lookups(ConfigKey::EnrichOnMetered.switched(None));
                }
                #[cfg(feature = "online")]
                SettingChange::Store(Setting::MusicbrainzMirror(mirrors)) => {
                    crate::musicbrainz::follow(
                        &config::load(Some(&self.path)).unwrap_or_default(),
                        *mirrors,
                    );
                }
                #[cfg(feature = "online")]
                SettingChange::Forget(ConfigKey::MusicbrainzMirror) => {
                    crate::musicbrainz::follow(
                        &config::load(Some(&self.path)).unwrap_or_default(),
                        ConfigKey::MusicbrainzMirror.switched(None),
                    );
                }
                SettingChange::Store(Setting::StallDownloadsOnMetered(stalls)) => {
                    metered::stall_downloads(*stalls);
                }
                SettingChange::Forget(ConfigKey::StallDownloadsOnMetered) => {
                    metered::stall_downloads(ConfigKey::StallDownloadsOnMetered.switched(None));
                }
                _ => {}
            }
        }
        refused.map_or(Ok(()), Err)
    }
}

fn stored(editing: &mut Editing<'_>, setting: &Setting) -> resonate_ui::Result<()> {
    let key = setting.key();
    let value: Option<Value> = match setting {
        Setting::EqualiserFor { sink, binding } => {
            return written_as_a_binding(setting, bound(editing, sink, binding.as_ref()));
        }
        Setting::EqualiserMeasurerFor { sink, measurer } => {
            return written_as_a_binding(setting, preferred(editing, sink, measurer.as_deref()));
        }
        Setting::EqualiserMeasurer(measurer) => given_or_cleared(measurer.as_deref().unwrap_or("")),
        Setting::EqualiserProfile(binding) => binding.as_ref().map(config::written),
        Setting::Sink(name) => name.as_ref().map(|name| name.as_str().into()),
        Setting::Quality(quality) => Some(quality_text(*quality).into()),
        Setting::FilterPhase(phase) => Some(phase_text(*phase).into()),
        Setting::TruePeak(guarded) => Some((*guarded).into()),
        Setting::Restoration(restoration) => Some(restoration.as_str().into()),
        Setting::Crossfeed(crossfeed) => Some(crossfeed.as_str().into()),
        Setting::Dither(dither) => Some(dither_text(*dither).into()),
        Setting::NoiseShaping(shaping) => Some(shaping_text(*shaping).into()),
        Setting::ReplayGain(mode) => Some(replay_gain_text(*mode).into()),
        Setting::PreAmp(trim) => Some(decibels_of(*trim).into()),
        Setting::Untagged(trim) => Some(decibels_of(*trim).into()),
        Setting::BitPerfect(wanted) => Some((*wanted).into()),
        Setting::Dop(marked) => Some((*marked).into()),
        Setting::DsdLikePcm(raised) => Some((*raised).into()),
        Setting::DeviceVolume(handed) => Some((*handed).into()),
        Setting::ForceGraphRate(forced) => Some((*forced).into()),
        Setting::BluetoothWake(on) => Some((*on).into()),
        Setting::BluetoothLead(lead) => {
            let millis = i64::try_from(lead.as_millis())
                .map_err(|_| resonate_ui::Error::SettingNotStored { key: setting.key() })?;
            Some(millis.into())
        }
        Setting::BluetoothAwake(awake) => {
            let seconds = i64::try_from(awake.as_secs())
                .map_err(|_| resonate_ui::Error::SettingNotStored { key: setting.key() })?;
            Some(seconds.into())
        }
        Setting::Buffer(held) => {
            let millis = i64::try_from(held.as_millis())
                .map_err(|_| resonate_ui::Error::SettingNotStored { key: setting.key() })?;
            Some(millis.into())
        }
        Setting::Volume(volume) => Some(as_typed(volume.get()).into()),
        Setting::Theme(theme) => Some(theme.as_str().into()),
        Setting::Accent(accent) => accent.map(|accent| accent.as_str().into()),
        Setting::TextSize(size) => Some(size.as_str().into()),
        Setting::Language(chosen) => Some(chosen.as_str().into()),
        Setting::FollowDesktopScheme(follows) => Some((*follows).into()),
        Setting::FollowDesktopTextSize(follows) => Some((*follows).into()),
        Setting::Online(enabled) => Some((*enabled).into()),
        Setting::EnrichAfterScan(after_scan) => Some((*after_scan).into()),
        Setting::EnrichOnMetered(allowed) => Some((*allowed).into()),
        Setting::MusicbrainzMirror(mirrors) => Some((*mirrors).into()),
        Setting::StallDownloadsOnMetered(stalls) => Some((*stalls).into()),
        Setting::Study(studies) => Some((*studies).into()),
        Setting::FetchLyrics(fetches) => Some((*fetches).into()),
        Setting::LyricsByTheLocale(chooses) => Some((*chooses).into()),
        Setting::IdentifyBySound(hears) => Some((*hears).into()),
        Setting::SkipUnderRepeat(skip) => Some((*skip == SkipUnderRepeat::RepeatsTheQueue).into()),
        Setting::PreviousRestarts(previous) => {
            Some((*previous == PreviousRestarts::RestartsTheTrack).into())
        }
        Setting::ShuffleStyle(style) => Some(style.as_str().into()),
        Setting::ShuffleSpreadsArtists(spreads) => Some((*spreads).into()),
        Setting::ShuffleSpreadsAlbums(spreads) => Some((*spreads).into()),
        Setting::ShuffleFreshFirst(fresh) => Some((*fresh).into()),
        Setting::Contact(contact) => {
            let contact = contact.trim();
            (!contact.is_empty()).then(|| contact.into())
        }
        Setting::AcoustidKey(key) => {
            let key = key.trim();
            (!key.is_empty()).then(|| key.into())
        }
        Setting::AuddToken(token) => {
            let token = token.trim();
            (!token.is_empty()).then(|| token.into())
        }
        Setting::ListenbrainzToken(token) => {
            let token = token.trim();
            (!token.is_empty()).then(|| token.into())
        }
        Setting::LastfmKey(key) => given_or_cleared(key),
        Setting::LastfmSecret(secret) => given_or_cleared(secret),
        Setting::LastfmSession(session) => given_or_cleared(session),
        Setting::ListenFrom(from) => Some(from.written().into()),
        Setting::ListenFor(length) => {
            Some(i64::try_from(length.as_secs()).unwrap_or(i64::MAX).into())
        }
        Setting::Equaliser(on) => Some((*on).into()),
        Setting::Renderers(on) => Some((*on).into()),
        Setting::FlacAbove96kFor(renderers) => (!renderers.is_empty()).then(|| {
            Value::Array(
                renderers
                    .iter()
                    .map(|renderer| renderer.as_str())
                    .collect::<Array>(),
            )
        }),
        Setting::Resume(keeps) => Some((*keeps).into()),
        Setting::HistoryKept(kept) => Some(match kept {
            HistoryKept::Forever => kept.to_string().into(),
            HistoryKept::Days(days) => i64::from(days.get()).into(),
        }),
        Setting::Notify(tells) => Some((*tells).into()),
        Setting::CheckForUpdates(checks) => Some((*checks).into()),
        Setting::UpdateDismissed(version) => given_or_cleared(version),
        Setting::Discord(on) => Some((*on).into()),
        Setting::DiscordApp(app) => app.map(|app| app.to_string().into()),
        Setting::DiscordShows(shown) => Some(shown.as_str().into()),
        Setting::DiscordArt(pictured) => Some(pictured.as_str().into()),
        Setting::DiscordIcon(icon) => icon.as_ref().map(|icon| icon.as_str().into()),
        Setting::DiscordProgress(shown) => Some((*shown).into()),
        Setting::DiscordPaused(stays) => Some((*stays).into()),
        Setting::Sync(on) => Some((*on).into()),
        Setting::SyncNearbyOnly(on) => Some((*on).into()),
        Setting::SyncServer(given) => given_or_cleared(given),
        Setting::SyncKey(given) => given_or_cleared(given),
        Setting::SyncDevice(given) => given_or_cleared(given),
        Setting::SharedName(given) => given_or_cleared(given),
        Setting::SyncSongs(songs) => {
            Some(Value::Array(songs.named().into_iter().collect::<Array>()))
        }
        Setting::SyncControl(trust) => Some(trust.as_str().into()),
        Setting::MinimiseButton(shown) => Some((*shown).into()),
        Setting::MaximiseButton(shown) => Some((*shown).into()),
        Setting::ScrollVolume(scrolls) => Some((*scrolls).into()),
        Setting::MouseNavigation(enabled) => Some((*enabled).into()),
        Setting::ReducedMotion(reduced) => Some((*reduced).into()),
        Setting::FileDropped(files) => Some((*files).into()),
        Setting::Scrollbars(mode) => Some(mode.as_str().into()),
        Setting::SpectrumTilt(tilt) => Some(tilt.as_str().into()),
        Setting::SpectrumFloor(floor) => Some(floor.as_str().into()),
        Setting::SpectrumBands(bands) => Some(bands.as_str().into()),
        Setting::SpectrumFalls(falls) => Some(falls.as_str().into()),
        Setting::SuggestionsTab(shown) => Some((*shown).into()),
        Setting::MissingTab(shown) => Some((*shown).into()),
        Setting::TabCounts(shown) => Some((*shown).into()),
        Setting::RememberTab(remember) => Some((*remember).into()),
        Setting::LastTab(tab) => Some(tab.as_str().into()),
        Setting::ArtistsDrawn(drawn) => Some(drawn.as_str().into()),
        Setting::RememberWindowSize(remember) => Some((*remember).into()),
        Setting::WindowSize(size) => Some(format!("{}x{}", size.width(), size.height()).into()),
        Setting::RememberSettingsCategory(remember) => Some((*remember).into()),
        Setting::LastSettingsCategory(category) => Some(category.as_str().into()),
        Setting::OrganiseAs(template) => Some(template.as_str().into()),
        Setting::Inbox(folder) => {
            let folder = folder
                .to_str()
                .ok_or(resonate_ui::Error::SettingNotStored { key: setting.key() })?;
            Some(folder.into())
        }
        Setting::Vault(folder) => {
            let folder = folder
                .to_str()
                .ok_or(resonate_ui::Error::SettingNotStored { key: setting.key() })?;
            Some(folder.into())
        }
        Setting::MusicExtensions(extensions) => Some(Value::Array(
            extensions
                .selected()
                .map(|extension| format!(".{extension}"))
                .collect::<Array>(),
        )),
        Setting::MinimumLength(length) => Some(i64::from(length.seconds()).into()),
        Setting::MusicFolder(folder) => {
            let folder = folder
                .to_str()
                .ok_or(resonate_ui::Error::SettingNotStored { key: setting.key() })?;
            Some(folder.into())
        }
        Setting::Convolution(held) => {
            let held = held
                .as_deref()
                .map(|path| {
                    path.to_str()
                        .ok_or(resonate_ui::Error::SettingNotStored { key: setting.key() })
                })
                .transpose()?;
            held.map(Into::into)
        }
        Setting::SpeakerAngle(angle) => Some(i64::from(angle.degrees()).into()),
        Setting::SpeakerElevation(elevation) => Some(i64::from(elevation.degrees()).into()),
        Setting::HeadYaw(yaw) => Some(i64::from(yaw.degrees()).into()),
        Setting::Subsonic(given) => given_or_cleared(given),
        Setting::SubsonicUser(given) => given_or_cleared(given),
        Setting::SubsonicPassword(given) => given_or_cleared(given),
        Setting::Jellyfin(given) => given_or_cleared(given),
        Setting::JellyfinUser(given) => given_or_cleared(given),
        Setting::JellyfinPassword(given) => given_or_cleared(given),
        Setting::JellyfinToken(given) => given_or_cleared(given),
        Setting::JellyfinUserId(given) => given_or_cleared(given),
        Setting::JellyfinDevice(given) => given_or_cleared(given),
        Setting::TidalClientId(given) => given_or_cleared(given),
        Setting::TidalClientSecret(given) => given_or_cleared(given),
        Setting::TidalRefreshToken(given) => given_or_cleared(given),
        Setting::SpotifyClientId(given) => given_or_cleared(given),
        Setting::SpotifyRefreshToken(given) => given_or_cleared(given),
        Setting::HifiApi(given) => given_or_cleared(given),
        Setting::HifiSecret(given) => given_or_cleared(given),
        Setting::Monochrome(given) => given_or_cleared(given),
        Setting::HostedServices(allowed) => Some((*allowed).into()),
        Setting::YoutubeMusic(on) => Some((*on).into()),
        Setting::YoutubeMusicCookies(given) => given_or_cleared(given),
        Setting::YtDlp(given) => given_or_cleared(given),
        Setting::YoutubeMusicKeepOrder(on) => Some((*on).into()),
    };

    written(editing, key, value);
    Ok(())
}

fn forgotten(editing: &mut Editing<'_>, key: ConfigKey) {
    match key {
        ConfigKey::RememberTab => {
            editing.clear(ConfigKey::RememberTab);
            editing.clear(ConfigKey::LastTab);
        }
        ConfigKey::RememberWindowSize => {
            editing.clear(ConfigKey::RememberWindowSize);
            editing.clear(ConfigKey::WindowSize);
        }
        ConfigKey::RememberSettingsCategory => {
            editing.clear(ConfigKey::RememberSettingsCategory);
            editing.clear(ConfigKey::LastSettingsCategory);
        }
        _ => editing.clear(key),
    }
}

fn given_or_cleared(given: &str) -> Option<Value> {
    let given = given.trim();
    (!given.is_empty()).then(|| given.into())
}

fn as_typed(single: f32) -> f64 {
    single
        .to_string()
        .parse()
        .unwrap_or_else(|_| f64::from(single))
}

fn written_as_a_binding(setting: &Setting, written: crate::Result<()>) -> resonate_ui::Result<()> {
    written.map_err(|error| {
        tracing::error!(%error, "a binding could not be written");
        resonate_ui::Error::SettingNotStored { key: setting.key() }
    })
}

fn quality_text(quality: Quality) -> String {
    spelt(QualityArg::from(quality))
}

fn phase_text(phase: FilterPhase) -> String {
    spelt(FilterPhaseArg::from(phase))
}

fn dither_text(dither: DitherKind) -> String {
    spelt(DitherArg::from(dither))
}

fn shaping_text(shaping: NoiseShaping) -> String {
    spelt(NoiseShapingArg::from(shaping))
}

fn spelt(value: impl ValueEnum) -> String {
    value
        .to_possible_value()
        .map(|possible| possible.get_name().to_owned())
        .unwrap_or_default()
}

fn decibels_of(trim: Trim) -> f64 {
    f64::from(trim.millibels()) / MILLIBELS_PER_DECIBEL
}

const fn replay_gain_text(mode: ReplayGainMode) -> &'static str {
    match mode {
        ReplayGainMode::Off => "off",
        ReplayGainMode::Track => "track",
        ReplayGainMode::Album => "album",
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, env, fs, process};

    use resonate_core::{
        Accent, AppId, HeadYaw, Pictured, Presence, RemoteTrust, Shown, SpeakerAngle,
        SpeakerElevation, TextSize, Theme, os_bytes,
    };

    use super::*;

    #[test]
    fn music_filters_written_by_the_pane_read_back_and_reset_to_the_current_behavior() {
        let folder = env::temp_dir().join(format!("resonate-settings-filters-{}", process::id()));
        let path = folder.join("config.toml");
        let file = File::at(path.clone());
        let filters = resonate_library::MusicFilters {
            extensions: resonate_library::MusicExtensions::parse(&[".MP3", ".flac", ".m4a"])
                .expect("supported extensions"),
            minimum_length: resonate_library::MinimumLength::new(600).expect("ten minutes"),
        };
        file.apply(&[
            SettingChange::Store(Setting::MusicExtensions(filters.extensions)),
            SettingChange::Store(Setting::MinimumLength(filters.minimum_length)),
        ])
        .expect("a writable config");
        assert_eq!(
            config::load(Some(&path))
                .expect("the config reads")
                .music_filters(),
            filters
        );
        file.apply(&[
            SettingChange::Forget(ConfigKey::MusicExtensions),
            SettingChange::Forget(ConfigKey::MinimumLength),
        ])
        .expect("resetting the filters");
        assert_eq!(
            config::load(Some(&path))
                .expect("the config reads")
                .music_filters(),
            resonate_library::MusicFilters::default()
        );
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_volume_is_written_as_the_figure_it_is_rather_than_the_float_widened() {
        assert_eq!(as_typed(0.7).to_string(), "0.7");
        assert_eq!(as_typed(0.25), 0.25);
        assert_eq!(as_typed(1.0), 1.0);
        assert_ne!(f64::from(0.7_f32).to_string(), "0.7");
    }

    #[test]
    fn every_noise_shaping_the_pane_writes_reads_back_as_itself() {
        let folder = env::temp_dir().join(format!("resonate-settings-{}", process::id()));
        let path = folder.join("config.toml");
        let file = File::at(path.clone());

        for shaping in [
            NoiseShaping::None,
            NoiseShaping::Lipshitz,
            NoiseShaping::Threshold,
        ] {
            file.apply(&[SettingChange::Store(Setting::NoiseShaping(shaping))])
                .expect("a writable temporary directory");
            assert_eq!(
                config::load(Some(&path))
                    .expect("the file reads back")
                    .noise_shaping,
                Some(shaping)
            );
        }
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn every_youtube_music_setting_the_pane_writes_reads_back_as_itself() {
        let folder = env::temp_dir().join(format!("resonate-settings-ytmusic-{}", process::id()));
        let path = folder.join("config.toml");
        let file = File::at(path.clone());

        file.apply(&[
            SettingChange::Store(Setting::YoutubeMusic(true)),
            SettingChange::Store(Setting::YoutubeMusicCookies("firefox".to_owned())),
            SettingChange::Store(Setting::YtDlp("/opt/yt-dlp".to_owned())),
            SettingChange::Store(Setting::YoutubeMusicKeepOrder(false)),
        ])
        .expect("a writable temporary directory");
        let written = config::load(Some(&path)).expect("the file reads back");
        file.apply(&[
            SettingChange::Store(Setting::YoutubeMusicCookies(String::new())),
            SettingChange::Store(Setting::YtDlp(String::new())),
        ])
        .expect("a writable temporary directory");
        let cleared = config::load(Some(&path)).expect("the file reads back");
        let _ = fs::remove_dir_all(folder);

        assert_eq!(written.youtube_music, Some(true));
        assert_eq!(written.youtube_music_cookies.as_deref(), Some("firefox"));
        assert_eq!(written.yt_dlp.as_deref(), Some("/opt/yt-dlp"));
        assert_eq!(written.youtube_music_keep_order, Some(false));
        assert_eq!(cleared.youtube_music_cookies, None);
        assert_eq!(cleared.yt_dlp, None);
    }

    #[test]
    fn a_pre_amp_and_an_untagged_gain_the_pane_writes_read_back_as_themselves() {
        let folder = env::temp_dir().join(format!("resonate-settings-levels-{}", process::id()));
        let path = folder.join("config.toml");
        let file = File::at(path.clone());
        let raised = Trim::from_millibels(300).expect("a trim");
        let lowered = Trim::from_millibels(-650).expect("a trim");

        file.apply(&[SettingChange::Store(Setting::PreAmp(raised))])
            .expect("a writable temporary directory");
        file.apply(&[SettingChange::Store(Setting::Untagged(lowered))])
            .expect("a writable temporary directory");
        let read = config::load(Some(&path)).expect("the file reads back");
        let _ = fs::remove_dir_all(folder);

        assert_eq!(read.pre_amp, Some(raised));
        assert_eq!(read.untagged, Some(lowered));
    }

    #[test]
    fn a_device_given_its_own_curve_by_the_pane_reads_back_bound_to_it() {
        let folder = env::temp_dir().join(format!("resonate-settings-own-{}", process::id()));
        let path = folder.join("config.toml");
        let file = File::at(path.clone());
        let sink = NodeName::new("alsa_output.usb-Topping_E30-00.analog-stereo");

        file.apply(&[SettingChange::Store(Setting::EqualiserFor {
            sink: sink.clone(),
            binding: Some(Binding::Own),
        })])
        .expect("a writable temporary directory");
        file.apply(&[SettingChange::Store(Setting::EqualiserProfile(Some(
            Binding::Own,
        )))])
        .expect("a writable temporary directory");

        let bindings = config::load(Some(&path))
            .expect("the file reads back")
            .equaliser_for
            .expect("a table");
        assert_eq!(
            bindings.for_sink(Some(&sink)),
            Some((Some(&sink), &Binding::Own))
        );
        assert_eq!(bindings.fallback(), Some(&Binding::Own));

        file.apply(&[SettingChange::Store(Setting::EqualiserFor {
            sink: sink.clone(),
            binding: None,
        })])
        .expect("a writable temporary directory");
        let bindings = config::load(Some(&path))
            .expect("the file reads back")
            .equaliser_for
            .expect("the fallback is still there");
        assert_eq!(bindings.for_sink(Some(&sink)), Some((None, &Binding::Own)));

        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_speaker_angle_the_pane_chooses_reads_back_and_one_put_back_is_gone() {
        let folder = env::temp_dir().join(format!("resonate-settings-angle-{}", process::id()));
        let path = folder.join("config.toml");
        let file = File::at(path.clone());
        let wide = SpeakerAngle::from_degrees(45).expect("an angle");

        file.apply(&[SettingChange::Store(Setting::SpeakerAngle(wide))])
            .expect("a writable temporary directory");
        let chosen = config::load(Some(&path)).expect("the file reads back");

        file.apply(&[SettingChange::Forget(ConfigKey::SpeakerAngle)])
            .expect("a writable temporary directory");
        let forgotten = config::load(Some(&path)).expect("the file reads back");
        let _ = fs::remove_dir_all(folder);

        assert_eq!(chosen.speaker_angle(), wide);
        assert_eq!(forgotten.speaker_angle(), SpeakerAngle::STANDARD);
    }

    #[test]
    fn a_speaker_elevation_the_pane_chooses_reads_back_and_one_put_back_is_gone() {
        let folder = env::temp_dir().join(format!("resonate-settings-raised-{}", process::id()));
        let path = folder.join("config.toml");
        let file = File::at(path.clone());
        let lowered = SpeakerElevation::from_degrees(-15).expect("an elevation");

        file.apply(&[SettingChange::Store(Setting::SpeakerElevation(lowered))])
            .expect("a writable temporary directory");
        let chosen = config::load(Some(&path)).expect("the file reads back");

        file.apply(&[SettingChange::Forget(ConfigKey::SpeakerElevation)])
            .expect("a writable temporary directory");
        let forgotten = config::load(Some(&path)).expect("the file reads back");
        let _ = fs::remove_dir_all(folder);

        assert_eq!(chosen.speaker_elevation(), lowered);
        assert_eq!(forgotten.speaker_elevation(), SpeakerElevation::LEVEL);
    }

    #[test]
    fn a_head_yaw_the_pane_chooses_reads_back_and_one_put_back_is_gone() {
        let folder = env::temp_dir().join(format!("resonate-settings-yawed-{}", process::id()));
        let path = folder.join("config.toml");
        let file = File::at(path.clone());
        let turned = HeadYaw::from_degrees(-35).expect("a yaw");

        file.apply(&[SettingChange::Store(Setting::HeadYaw(turned))])
            .expect("a writable temporary directory");
        let chosen = config::load(Some(&path)).expect("the file reads back");

        file.apply(&[SettingChange::Forget(ConfigKey::HeadYaw)])
            .expect("a writable temporary directory");
        let forgotten = config::load(Some(&path)).expect("the file reads back");
        let _ = fs::remove_dir_all(folder);

        assert_eq!(chosen.head_yaw(), turned);
        assert_eq!(forgotten.head_yaw(), HeadYaw::AHEAD);
    }

    #[test]
    fn a_measurer_the_pane_prefers_reads_back_and_one_put_back_is_gone() {
        let folder = env::temp_dir().join(format!("resonate-settings-measurer-{}", process::id()));
        let path = folder.join("config.toml");
        let file = File::at(path.clone());

        file.apply(&[SettingChange::Store(Setting::EqualiserMeasurer(Some(
            "Rtings".to_owned(),
        )))])
        .expect("a writable temporary directory");
        let preferred = config::load(Some(&path)).expect("the file reads back");

        file.apply(&[SettingChange::Forget(ConfigKey::EqualiserMeasurer)])
            .expect("a writable temporary directory");
        let forgotten = config::load(Some(&path)).expect("the file reads back");
        let _ = fs::remove_dir_all(folder);

        assert_eq!(preferred.equaliser_measurer(), Some("Rtings"));
        assert_eq!(forgotten.equaliser_measurer(), None);
    }

    #[test]
    fn a_measurer_the_pane_prefers_for_one_device_reads_back_beside_the_rest_and_goes_when_cleared()
    {
        let folder =
            env::temp_dir().join(format!("resonate-settings-measurer-for-{}", process::id()));
        let path = folder.join("config.toml");
        let file = File::at(path.clone());
        let dac = NodeName::new("alsa_output.dac");

        file.apply(&[
            SettingChange::Store(Setting::EqualiserMeasurer(Some("Rtings".to_owned()))),
            SettingChange::Store(Setting::EqualiserMeasurerFor {
                sink: dac.clone(),
                measurer: Some("crinacle".to_owned()),
            }),
        ])
        .expect("a writable temporary directory");
        let preferred = config::load(Some(&path)).expect("the file reads back");

        file.apply(&[SettingChange::Store(Setting::EqualiserMeasurerFor {
            sink: dac.clone(),
            measurer: None,
        })])
        .expect("a writable temporary directory");
        let cleared = config::load(Some(&path)).expect("the file reads back");
        let text = fs::read_to_string(&path).expect("the file is there");
        let _ = fs::remove_dir_all(folder);

        assert_eq!(
            preferred.equaliser_measurer_for(Some(&dac)),
            Some("crinacle")
        );
        assert_eq!(cleared.equaliser_measurer_for(Some(&dac)), Some("Rtings"));
        assert!(!text.contains("equaliser-measurer-for"), "{text}");
    }

    #[test]
    fn an_inbox_the_pane_names_reads_back_and_one_put_back_is_gone() {
        let folder = env::temp_dir().join(format!("resonate-settings-inbox-{}", process::id()));
        let path = folder.join("config.toml");
        let file = File::at(path.clone());
        let inbox = PathBuf::from("/music/inbox");

        file.apply(&[SettingChange::Store(Setting::Inbox(inbox.clone()))])
            .expect("a writable temporary directory");
        let named = config::load(Some(&path))
            .expect("the file reads back")
            .inbox;
        file.apply(&[SettingChange::Forget(ConfigKey::Inbox)])
            .expect("a writable temporary directory");
        let forgotten = config::load(Some(&path))
            .expect("the file reads back")
            .inbox;
        let _ = fs::remove_dir_all(folder);

        assert_eq!(named, Some(inbox));
        assert_eq!(forgotten, None);
    }

    #[test]
    fn what_other_devices_may_do_reads_back_as_chosen_and_one_forgotten_is_everything_again() {
        let folder = env::temp_dir().join(format!("resonate-settings-control-{}", process::id()));
        let path = folder.join("config.toml");
        let file = File::at(path.clone());

        for trust in RemoteTrust::ALL {
            file.apply(&[SettingChange::Store(Setting::SyncControl(trust))])
                .expect("a writable temporary directory");

            let read = config::load(Some(&path)).expect("the file reads back");

            assert_eq!(read.sync_control, Some(trust));
        }
        file.apply(&[SettingChange::Forget(ConfigKey::SyncControl)])
            .expect("a writable temporary directory");
        let forgotten = config::load(Some(&path)).expect("the file reads back");
        let _ = fs::remove_dir_all(folder);

        assert_eq!(forgotten.sync_control, None);
        assert_eq!(
            forgotten.sync_control.unwrap_or_default(),
            RemoteTrust::Everything
        );
    }

    #[test]
    fn the_metered_switches_read_back_and_one_forgotten_is_the_default_again() {
        let folder = env::temp_dir().join(format!("resonate-settings-metered-{}", process::id()));
        let path = folder.join("config.toml");
        let file = File::at(path.clone());

        file.apply(&[
            SettingChange::Store(Setting::EnrichOnMetered(true)),
            SettingChange::Store(Setting::StallDownloadsOnMetered(true)),
        ])
        .expect("a writable temporary directory");
        let allowed = config::load(Some(&path)).expect("the file reads back");
        file.apply(&[
            SettingChange::Forget(ConfigKey::EnrichOnMetered),
            SettingChange::Forget(ConfigKey::StallDownloadsOnMetered),
        ])
        .expect("a writable temporary directory");
        let forgotten = config::load(Some(&path)).expect("the file reads back");
        let _ = fs::remove_dir_all(folder);

        assert!(allowed.enriches_on_metered());
        assert!(allowed.stalls_downloads_on_metered());
        assert_eq!(forgotten.enrich_on_metered, None);
        assert_eq!(forgotten.stall_downloads_on_metered, None);
        assert!(!forgotten.enriches_on_metered());
        assert!(!forgotten.stalls_downloads_on_metered());
    }

    #[test]
    fn renderers_the_pane_names_for_flac_read_back_as_vouched_and_none_left_clears_the_key() {
        let folder = env::temp_dir().join(format!("resonate-settings-flac-{}", process::id()));
        let path = folder.join("config.toml");
        let file = File::at(path.clone());
        let named: BTreeSet<NodeName> = [
            NodeName::new("upnp:uuid:streamer"),
            NodeName::new("upnp:uuid:amplifier"),
        ]
        .into();

        file.apply(&[SettingChange::Store(Setting::FlacAbove96kFor(
            named.clone(),
        ))])
        .expect("a writable temporary directory");
        let read = config::load(Some(&path)).expect("the file reads back");
        file.apply(&[SettingChange::Store(Setting::FlacAbove96kFor(
            BTreeSet::new(),
        ))])
        .expect("a writable temporary directory");
        let emptied = fs::read_to_string(&path).expect("the file");
        let cleared = config::load(Some(&path)).expect("the file reads back");
        let _ = fs::remove_dir_all(folder);

        assert_eq!(read.flac_above_96k_for, named);
        assert!(cleared.flac_above_96k_for.is_empty());
        assert!(!emptied.contains("flac-above-96k-for"), "{emptied}");
    }

    #[test]
    fn a_vault_the_pane_names_reads_back_for_the_next_start() {
        let folder = env::temp_dir().join(format!("resonate-settings-vault-{}", process::id()));
        let path = folder.join("config.toml");
        let file = File::at(path.clone());
        let vault = PathBuf::from("/archive/vault");

        file.apply(&[SettingChange::Store(Setting::Vault(vault.clone()))])
            .expect("a writable temporary directory");
        let named = config::load(Some(&path))
            .expect("the file reads back")
            .vault;
        let _ = fs::remove_dir_all(folder);

        assert_eq!(named, Some(vault));
    }

    #[test]
    fn a_music_folder_the_pane_names_reads_back_and_one_put_back_is_gone() {
        let folder = env::temp_dir().join(format!("resonate-settings-music-{}", process::id()));
        let path = folder.join("config.toml");
        let file = File::at(path.clone());
        let music = PathBuf::from("/music/library");

        file.apply(&[SettingChange::Store(Setting::MusicFolder(music.clone()))])
            .expect("a writable temporary directory");
        let named = config::load(Some(&path))
            .expect("the file reads back")
            .music_folder;
        file.apply(&[SettingChange::Forget(ConfigKey::MusicFolder)])
            .expect("a writable temporary directory");
        let forgotten = config::load(Some(&path))
            .expect("the file reads back")
            .music_folder;
        let _ = fs::remove_dir_all(folder);

        assert_eq!(named, Some(music));
        assert_eq!(forgotten, None);
    }

    #[test]
    fn a_batch_lands_whole_in_one_write_and_a_refused_setting_leaves_the_rest() {
        let folder = env::temp_dir().join(format!("resonate-settings-batch-{}", process::id()));
        let path = folder.join("config.toml");
        let file = File::at(path.clone());
        let unwritable = PathBuf::from(os_bytes::os_string_from(b"/music/\xed\xa0\x80".to_vec()));

        let refused = file.apply(&[
            SettingChange::Store(Setting::Inbox(PathBuf::from("/music/inbox"))),
            SettingChange::Store(Setting::Inbox(unwritable)),
            SettingChange::Store(Setting::Resume(false)),
            SettingChange::Store(Setting::Notify(false)),
            SettingChange::Forget(ConfigKey::Notify),
        ]);
        let read = config::load(Some(&path)).expect("the file reads back");
        let staged = fs::read_dir(&folder)
            .expect("the folder")
            .flatten()
            .filter(|entry| entry.path() != path)
            .count();
        let _ = fs::remove_dir_all(folder);

        assert!(matches!(
            refused,
            Err(resonate_ui::Error::SettingNotStored {
                key: ConfigKey::Inbox
            })
        ));
        assert_eq!(read.inbox, Some(PathBuf::from("/music/inbox")));
        assert_eq!(read.resume, Some(false));
        assert_eq!(
            read.notify, None,
            "a setting forgotten after it was stored stood"
        );
        assert_eq!(staged, 0, "a write left something beside the file");
    }

    #[test]
    fn every_discord_setting_the_pane_writes_reads_back_as_the_presence_it_named() {
        let folder = env::temp_dir().join(format!("resonate-settings-discord-{}", process::id()));
        let path = folder.join("config.toml");
        let file = File::at(path.clone());
        let named = Presence {
            enabled: true,
            app: AppId::parse("1234567890123456789"),
            shown: Shown::Application,
            pictured: Pictured::Nothing,
            icon: resonate_core::Icon::parse("resonate"),
            progress: false,
            while_paused: true,
        };

        for setting in [
            Setting::Discord(named.enabled),
            Setting::DiscordApp(named.app),
            Setting::DiscordShows(named.shown),
            Setting::DiscordArt(named.pictured),
            Setting::DiscordIcon(named.icon.clone()),
            Setting::DiscordProgress(named.progress),
            Setting::DiscordPaused(named.while_paused),
        ] {
            file.apply(&[SettingChange::Store(setting)])
                .expect("a writable temporary directory");
        }
        let read = config::load(Some(&path))
            .expect("the file reads back")
            .presence();
        file.apply(&[SettingChange::Store(Setting::DiscordApp(None))])
            .expect("a writable temporary directory");
        let cleared = config::load(Some(&path)).expect("the file reads back");
        let _ = fs::remove_dir_all(folder);

        assert_eq!(read, named);
        assert_eq!(cleared.discord_app, None);
        assert!(!cleared.presence().active());
    }

    #[test]
    fn no_two_settings_are_written_under_one_key() {
        let named_by_a_setting: Vec<ConfigKey> = ConfigKey::ALL
            .into_iter()
            .filter(|key| !matches!(key, ConfigKey::Library))
            .collect();

        for key in named_by_a_setting {
            let sharing = ConfigKey::ALL
                .into_iter()
                .filter(|other| other.as_str() == key.as_str())
                .count();
            assert_eq!(sharing, 1, "{key} is written under a name another claims");
        }
    }

    #[test]
    fn the_theme_and_accent_texts_are_the_ones_the_reader_answers_to() {
        for theme in Theme::ALL {
            assert_eq!(Theme::parse(theme.as_str()), Some(theme));
        }
        for accent in Accent::ALL {
            assert_eq!(Accent::parse(accent.as_str()), Some(accent));
        }
        for size in TextSize::ALL {
            assert_eq!(TextSize::parse(size.as_str()), Some(size));
        }
    }
}
