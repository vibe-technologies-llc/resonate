use std::path::PathBuf;

use clap::ValueEnum;
use resonate_core::Trim;
use resonate_engine::{
    DitherKind, FilterPhase, NodeName, NoiseShaping, Quality, ReplayGainMode, SkipUnderRepeat,
};
use resonate_eq::Binding;
use resonate_ui::{Setting, SettingKey, Settings};
use toml_edit::Value;

use crate::{
    ConfigKey,
    cli::{DitherArg, FilterPhaseArg, NoiseShapingArg, QualityArg},
    config,
};

const MILLIBELS_PER_DECIBEL: f64 = 100.0;

pub struct File {
    path: PathBuf,
}

impl File {
    pub const fn at(path: PathBuf) -> Self {
        Self { path }
    }

    fn written(&self, key: ConfigKey, value: Option<Value>) -> crate::Result<()> {
        match value {
            Some(value) => config::store(&self.path, key, value),
            None => config::clear(&self.path, key),
        }
    }

    fn bound(&self, sink: &NodeName, binding: Option<&Binding>) -> crate::Result<()> {
        match binding {
            Some(binding) => config::store_in_table(
                &self.path,
                ConfigKey::EqualiserFor,
                sink.as_str(),
                config::written(binding),
            ),
            None => config::clear_in_table(&self.path, ConfigKey::EqualiserFor, sink.as_str()),
        }
    }

    fn bound_by_the_rest(&self, binding: Option<&Binding>) -> crate::Result<()> {
        self.written(ConfigKey::EqualiserProfile, binding.map(config::written))
    }
}

impl Settings for File {
    fn store(&self, setting: &Setting) -> resonate_ui::Result<()> {
        let (key, value): (ConfigKey, Option<Value>) = match setting {
            Setting::EqualiserFor { sink, binding } => {
                return written_as_a_binding(setting, self.bound(sink, binding.as_ref()));
            }
            Setting::EqualiserProfile(binding) => {
                return written_as_a_binding(setting, self.bound_by_the_rest(binding.as_ref()));
            }
            Setting::Sink(name) => (
                ConfigKey::Sink,
                name.as_ref().map(|name| name.as_str().into()),
            ),
            Setting::Quality(quality) => (ConfigKey::Quality, Some(quality_text(*quality).into())),
            Setting::FilterPhase(phase) => {
                (ConfigKey::FilterPhase, Some(phase_text(*phase).into()))
            }
            Setting::TruePeak(guarded) => (ConfigKey::TruePeak, Some((*guarded).into())),
            Setting::Restoration(restoration) => {
                (ConfigKey::RestoreLossy, Some(restoration.as_str().into()))
            }
            Setting::Dither(dither) => (ConfigKey::Dither, Some(dither_text(*dither).into())),
            Setting::NoiseShaping(shaping) => {
                (ConfigKey::NoiseShaping, Some(shaping_text(*shaping).into()))
            }
            Setting::ReplayGain(mode) => {
                (ConfigKey::ReplayGain, Some(replay_gain_text(*mode).into()))
            }
            Setting::PreAmp(trim) => (ConfigKey::ReplayGainPreAmp, Some(decibels_of(*trim).into())),
            Setting::Untagged(trim) => (
                ConfigKey::ReplayGainUntagged,
                Some(decibels_of(*trim).into()),
            ),
            Setting::BitPerfect(wanted) => (ConfigKey::BitPerfect, Some((*wanted).into())),
            Setting::Dop(marked) => (ConfigKey::Dop, Some((*marked).into())),
            Setting::ForceGraphRate(forced) => (ConfigKey::ForceGraphRate, Some((*forced).into())),
            Setting::BluetoothWake(on) => (ConfigKey::BluetoothWake, Some((*on).into())),
            Setting::BluetoothLead(lead) => {
                let millis = i64::try_from(lead.as_millis())
                    .map_err(|_| resonate_ui::Error::SettingNotStored { key: setting.key() })?;
                (ConfigKey::BluetoothLeadMs, Some(millis.into()))
            }
            Setting::BluetoothAwake(awake) => {
                let seconds = i64::try_from(awake.as_secs())
                    .map_err(|_| resonate_ui::Error::SettingNotStored { key: setting.key() })?;
                (ConfigKey::BluetoothAwakeS, Some(seconds.into()))
            }
            Setting::Buffer(held) => {
                let millis = i64::try_from(held.as_millis())
                    .map_err(|_| resonate_ui::Error::SettingNotStored { key: setting.key() })?;
                (ConfigKey::BufferMs, Some(millis.into()))
            }
            Setting::Volume(volume) => (ConfigKey::Volume, Some(f64::from(volume.get()).into())),
            Setting::Theme(theme) => (ConfigKey::Theme, Some(theme.as_str().into())),
            Setting::Accent(accent) => (
                ConfigKey::Accent,
                accent.map(|accent| accent.as_str().into()),
            ),
            Setting::TextSize(size) => (ConfigKey::TextSize, Some(size.as_str().into())),
            Setting::Online(enabled) => (ConfigKey::Online, Some((*enabled).into())),
            Setting::EnrichAfterScan(after_scan) => {
                (ConfigKey::EnrichAfterScan, Some((*after_scan).into()))
            }
            Setting::Study(studies) => (ConfigKey::Study, Some((*studies).into())),
            Setting::SkipUnderRepeat(skip) => (
                ConfigKey::SkipRepeatsQueue,
                Some((*skip == SkipUnderRepeat::RepeatsTheQueue).into()),
            ),
            Setting::Contact(contact) => {
                let contact = contact.trim();
                (
                    ConfigKey::Contact,
                    (!contact.is_empty()).then(|| contact.into()),
                )
            }
            Setting::AcoustidKey(key) => {
                let key = key.trim();
                (
                    ConfigKey::AcoustidKey,
                    (!key.is_empty()).then(|| key.into()),
                )
            }
            Setting::AuddToken(token) => {
                let token = token.trim();
                (
                    ConfigKey::AuddToken,
                    (!token.is_empty()).then(|| token.into()),
                )
            }
            Setting::ListenFrom(from) => (ConfigKey::ListenFrom, Some(from.written().into())),
            Setting::ListenFor(length) => (
                ConfigKey::ListenFor,
                Some(i64::try_from(length.as_secs()).unwrap_or(i64::MAX).into()),
            ),
            Setting::Equaliser(on) => (ConfigKey::Equaliser, Some((*on).into())),
            Setting::Resume(keeps) => (ConfigKey::Resume, Some((*keeps).into())),
            Setting::Notify(tells) => (ConfigKey::Notify, Some((*tells).into())),
            Setting::Discord(on) => (ConfigKey::Discord, Some((*on).into())),
            Setting::DiscordApp(app) => {
                (ConfigKey::DiscordApp, app.map(|app| app.to_string().into()))
            }
            Setting::DiscordShows(shown) => (ConfigKey::DiscordShows, Some(shown.as_str().into())),
            Setting::DiscordArt(pictured) => {
                (ConfigKey::DiscordArt, Some(pictured.as_str().into()))
            }
            Setting::DiscordIcon(icon) => (
                ConfigKey::DiscordIcon,
                icon.as_ref().map(|icon| icon.as_str().into()),
            ),
            Setting::DiscordProgress(shown) => (ConfigKey::DiscordProgress, Some((*shown).into())),
            Setting::DiscordPaused(stays) => (ConfigKey::DiscordPaused, Some((*stays).into())),
            Setting::MinimiseButton(shown) => (ConfigKey::MinimiseButton, Some((*shown).into())),
            Setting::MaximiseButton(shown) => (ConfigKey::MaximiseButton, Some((*shown).into())),
            Setting::ScrollVolume(scrolls) => (ConfigKey::ScrollVolume, Some((*scrolls).into())),
            Setting::Scrollbars(drawn) => (ConfigKey::Scrollbars, Some((*drawn).into())),
            Setting::OrganiseAs(template) => {
                (ConfigKey::OrganiseAs, Some(template.as_str().into()))
            }
            Setting::Inbox(folder) => {
                let folder = folder
                    .to_str()
                    .ok_or(resonate_ui::Error::SettingNotStored { key: setting.key() })?;
                (ConfigKey::Inbox, Some(folder.into()))
            }
        };

        self.written(key, value).map_err(|error| {
            tracing::error!(%error, %key, "the settings file could not be written");
            resonate_ui::Error::SettingNotStored { key: setting.key() }
        })
    }

    fn forget(&self, key: SettingKey) -> resonate_ui::Result<()> {
        let named = named(key);

        config::clear(&self.path, named).map_err(|error| {
            tracing::error!(%error, key = %named, "the setting could not be taken out of the file");
            resonate_ui::Error::SettingNotStored { key }
        })
    }
}

fn written_as_a_binding(setting: &Setting, written: crate::Result<()>) -> resonate_ui::Result<()> {
    written.map_err(|error| {
        tracing::error!(%error, "a binding could not be written");
        resonate_ui::Error::SettingNotStored { key: setting.key() }
    })
}

const fn named(key: SettingKey) -> ConfigKey {
    match key {
        SettingKey::Sink => ConfigKey::Sink,
        SettingKey::Quality => ConfigKey::Quality,
        SettingKey::FilterPhase => ConfigKey::FilterPhase,
        SettingKey::TruePeak => ConfigKey::TruePeak,
        SettingKey::Restoration => ConfigKey::RestoreLossy,
        SettingKey::Dither => ConfigKey::Dither,
        SettingKey::NoiseShaping => ConfigKey::NoiseShaping,
        SettingKey::ReplayGain => ConfigKey::ReplayGain,
        SettingKey::PreAmp => ConfigKey::ReplayGainPreAmp,
        SettingKey::Untagged => ConfigKey::ReplayGainUntagged,
        SettingKey::BitPerfect => ConfigKey::BitPerfect,
        SettingKey::Dop => ConfigKey::Dop,
        SettingKey::ForceGraphRate => ConfigKey::ForceGraphRate,
        SettingKey::BluetoothWake => ConfigKey::BluetoothWake,
        SettingKey::BluetoothLead => ConfigKey::BluetoothLeadMs,
        SettingKey::BluetoothAwake => ConfigKey::BluetoothAwakeS,
        SettingKey::Buffer => ConfigKey::BufferMs,
        SettingKey::Volume => ConfigKey::Volume,
        SettingKey::Theme => ConfigKey::Theme,
        SettingKey::Accent => ConfigKey::Accent,
        SettingKey::TextSize => ConfigKey::TextSize,
        SettingKey::Online => ConfigKey::Online,
        SettingKey::EnrichAfterScan => ConfigKey::EnrichAfterScan,
        SettingKey::Study => ConfigKey::Study,
        SettingKey::SkipUnderRepeat => ConfigKey::SkipRepeatsQueue,
        SettingKey::Contact => ConfigKey::Contact,
        SettingKey::AcoustidKey => ConfigKey::AcoustidKey,
        SettingKey::AuddToken => ConfigKey::AuddToken,
        SettingKey::ListenFrom => ConfigKey::ListenFrom,
        SettingKey::ListenFor => ConfigKey::ListenFor,
        SettingKey::Equaliser => ConfigKey::Equaliser,
        SettingKey::EqualiserFor => ConfigKey::EqualiserFor,
        SettingKey::EqualiserProfile => ConfigKey::EqualiserProfile,
        SettingKey::Resume => ConfigKey::Resume,
        SettingKey::OrganiseAs => ConfigKey::OrganiseAs,
        SettingKey::Notify => ConfigKey::Notify,
        SettingKey::MinimiseButton => ConfigKey::MinimiseButton,
        SettingKey::MaximiseButton => ConfigKey::MaximiseButton,
        SettingKey::ScrollVolume => ConfigKey::ScrollVolume,
        SettingKey::Scrollbars => ConfigKey::Scrollbars,
        SettingKey::Inbox => ConfigKey::Inbox,
        SettingKey::Discord => ConfigKey::Discord,
        SettingKey::DiscordApp => ConfigKey::DiscordApp,
        SettingKey::DiscordShows => ConfigKey::DiscordShows,
        SettingKey::DiscordArt => ConfigKey::DiscordArt,
        SettingKey::DiscordIcon => ConfigKey::DiscordIcon,
        SettingKey::DiscordProgress => ConfigKey::DiscordProgress,
        SettingKey::DiscordPaused => ConfigKey::DiscordPaused,
    }
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
    use std::{env, fs, process};

    use resonate_core::{Accent, AppId, Pictured, Presence, Shown, TextSize, Theme};

    use super::*;

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
            file.store(&Setting::NoiseShaping(shaping))
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
    fn a_pre_amp_and_an_untagged_gain_the_pane_writes_read_back_as_themselves() {
        let folder = env::temp_dir().join(format!("resonate-settings-levels-{}", process::id()));
        let path = folder.join("config.toml");
        let file = File::at(path.clone());
        let raised = Trim::from_millibels(300).expect("a trim");
        let lowered = Trim::from_millibels(-650).expect("a trim");

        file.store(&Setting::PreAmp(raised))
            .expect("a writable temporary directory");
        file.store(&Setting::Untagged(lowered))
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

        file.store(&Setting::EqualiserFor {
            sink: sink.clone(),
            binding: Some(Binding::Own),
        })
        .expect("a writable temporary directory");
        file.store(&Setting::EqualiserProfile(Some(Binding::Own)))
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

        file.store(&Setting::EqualiserFor {
            sink: sink.clone(),
            binding: None,
        })
        .expect("a writable temporary directory");
        let bindings = config::load(Some(&path))
            .expect("the file reads back")
            .equaliser_for
            .expect("the fallback is still there");
        assert_eq!(bindings.for_sink(Some(&sink)), Some((None, &Binding::Own)));

        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn an_inbox_the_pane_names_reads_back_and_one_put_back_is_gone() {
        let folder = env::temp_dir().join(format!("resonate-settings-inbox-{}", process::id()));
        let path = folder.join("config.toml");
        let file = File::at(path.clone());
        let inbox = PathBuf::from("/music/inbox");

        file.store(&Setting::Inbox(inbox.clone()))
            .expect("a writable temporary directory");
        let named = config::load(Some(&path))
            .expect("the file reads back")
            .inbox;
        file.forget(SettingKey::Inbox)
            .expect("a writable temporary directory");
        let forgotten = config::load(Some(&path))
            .expect("the file reads back")
            .inbox;
        let _ = fs::remove_dir_all(folder);

        assert_eq!(named, Some(inbox));
        assert_eq!(forgotten, None);
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
            file.store(&setting)
                .expect("a writable temporary directory");
        }
        let read = config::load(Some(&path))
            .expect("the file reads back")
            .presence();
        file.store(&Setting::DiscordApp(None))
            .expect("a writable temporary directory");
        let cleared = config::load(Some(&path)).expect("the file reads back");
        let _ = fs::remove_dir_all(folder);

        assert_eq!(read, named);
        assert_eq!(cleared.discord_app, None);
        assert!(!cleared.presence().active());
    }

    #[test]
    fn every_setting_the_pane_can_write_names_a_key_the_file_reads_back() {
        for key in SettingKey::ALL {
            assert_eq!(ConfigKey::parse(named(key).as_str()), Some(named(key)));
        }
    }

    #[test]
    fn no_two_keys_the_pane_writes_land_in_one_place_in_the_file() {
        for key in SettingKey::ALL {
            let sharing = SettingKey::ALL
                .into_iter()
                .filter(|other| named(*other) == named(key))
                .count();
            assert_eq!(
                sharing,
                1,
                "{} is written where another setting is",
                named(key)
            );
        }
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
