use std::{
    sync::{Arc, LazyLock},
    time::Duration,
};

use resonate_core::{
    Appearance, Presence, ScrollbarMode, Spectral, SpectrumBands, SpectrumFalls, SpectrumFloor,
    SpectrumTilt, Switches,
};
use resonate_engine::{
    Command, EngineConfig, OutputSettings, PreviousRestarts, SkipUnderRepeat, SmartShuffle,
};
use resonate_library::{HistoryKept, MinimumLength, MusicExtensions, MusicFilters};
use resonate_listen::{CLIP_BY_DEFAULT, Listening};
use resonate_words::LanguageChoice;

use crate::{Synchronised, Tabs, WindowButtons, views::settings::find::Group};

pub(crate) const HISTORY_KEPT: HistoryKept = HistoryKept::Forever;
pub(crate) const WINDOW_BUTTONS: WindowButtons = WindowButtons::SHOWN;
pub(crate) const REDUCED_MOTION: bool = false;

pub(crate) const SCROLLBARS: ScrollbarMode = ScrollbarMode::AutoHidden;
pub(crate) const LANGUAGE: LanguageChoice = LanguageChoice::System;
pub(crate) const SPECTRAL: Spectral = Spectral {
    tilt: SpectrumTilt::Pink,
    floor: SpectrumFloor::Middling,
    bands: SpectrumBands::Sixths,
    falls: SpectrumFalls::Middling,
};
pub(crate) const TABS: Tabs = Tabs::AS_BUILT;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Standing {
    pub(crate) output: OutputSettings,
    pub(crate) appearance: Appearance,
    pub(crate) switches: Switches,
    pub(crate) flac_renderers_named: bool,
    pub(crate) formats_learnt: bool,
    pub(crate) contact_given: bool,
    pub(crate) measurer_given: bool,
    pub(crate) key_given: bool,
    pub(crate) token_given: bool,
    pub(crate) submitting_given: bool,
    pub(crate) lastfm_given: bool,
    pub(crate) listening_from: Listening,
    pub(crate) listening_for: Duration,
    pub(crate) history_kept: HistoryKept,
    pub(crate) music_filters: MusicFilters,
    pub(crate) skip_under_repeat: SkipUnderRepeat,
    pub(crate) previous_restarts: PreviousRestarts,
    pub(crate) smart_shuffle: SmartShuffle,
    pub(crate) window_buttons: WindowButtons,
    pub(crate) reduced_motion: bool,
    pub(crate) scrollbars: ScrollbarMode,
    pub(crate) language: LanguageChoice,
    pub(crate) spectral: Spectral,
    pub(crate) tabs: Tabs,
    pub(crate) presence: Presence,
    pub(crate) synchronised: Synchronised,
    pub(crate) template_given: bool,
    pub(crate) inbox_given: bool,
    pub(crate) music_folder_given: bool,
    pub(crate) subsonic_given: bool,
    pub(crate) jellyfin_given: bool,
    pub(crate) tidal_given: bool,
    pub(crate) spotify_given: bool,
    pub(crate) resonate_key_given: bool,
    pub(crate) youtube_music_given: bool,
    pub(crate) convolving: bool,
    pub(crate) speakers_moved: bool,
}

impl Standing {
    pub(crate) fn as_built() -> Self {
        Self {
            output: OutputSettings::of(&EngineConfig::default()),
            appearance: Appearance::DEFAULT,
            switches: Switches::UNSAID,
            flac_renderers_named: false,
            formats_learnt: false,
            contact_given: false,
            measurer_given: false,
            key_given: false,
            token_given: false,
            submitting_given: false,
            lastfm_given: false,
            listening_from: Listening::default(),
            listening_for: CLIP_BY_DEFAULT,
            history_kept: HISTORY_KEPT,
            music_filters: MusicFilters::default(),
            skip_under_repeat: SkipUnderRepeat::default(),
            previous_restarts: PreviousRestarts::default(),
            smart_shuffle: SmartShuffle::default(),
            window_buttons: WINDOW_BUTTONS,
            reduced_motion: REDUCED_MOTION,
            scrollbars: SCROLLBARS,
            language: LANGUAGE,
            spectral: SPECTRAL,
            tabs: TABS,
            presence: Presence::OFF,
            synchronised: Synchronised::default(),
            template_given: false,
            inbox_given: false,
            music_folder_given: false,
            subsonic_given: false,
            jellyfin_given: false,
            tidal_given: false,
            spotify_given: false,
            resonate_key_given: false,
            youtube_music_given: false,
            convolving: false,
            speakers_moved: false,
        }
    }
}

static AS_BUILT: LazyLock<Standing> = LazyLock::new(Standing::as_built);

pub(crate) fn differs(group: Group, standing: &Standing) -> bool {
    switched_from_as_built(group, &standing.switches)
        || differs_but_for_its_switches(group, standing)
}

pub(crate) fn switched_from_as_built(group: Group, worn: &Switches) -> bool {
    group
        .switches()
        .any(|switch| worn.is_on(switch) != switch.on_unless_said())
}

fn differs_but_for_its_switches(group: Group, standing: &Standing) -> bool {
    let (worn, built) = (&standing.output, &AS_BUILT.output);

    match group {
        Group::Device => worn.sink != built.sink,
        Group::DeviceVolume => worn.device_volume != built.device_volume,
        Group::SampleRate => worn.prefer_bit_perfect != built.prefer_bit_perfect,
        Group::GraphRate => worn.force_graph_rate != built.force_graph_rate,
        Group::Buffer => worn.buffer != built.buffer,
        Group::Dop => worn.dop != built.dop || worn.dsd_like_pcm != built.dsd_like_pcm,
        Group::Bluetooth => worn.bluetooth != built.bluetooth,
        Group::Resampler => {
            worn.quality != built.quality || worn.filter_phase != built.filter_phase
        }
        Group::Dither => worn.dither != built.dither,
        Group::NoiseShaping => worn.noise_shaping != built.noise_shaping,
        Group::Crossfeed => worn.crossfeed != built.crossfeed,
        Group::ReplayGain => {
            worn.replay_gain != built.replay_gain || worn.levelling != built.levelling
        }
        Group::TruePeak => worn.true_peak != built.true_peak,
        Group::LossySources => worn.restoration != built.restoration,
        Group::BoundTo => worn.equaliser.binds_anything(),
        Group::FlacRenderers => standing.flac_renderers_named,
        Group::LearntFormats => standing.formats_learnt,
        Group::History => standing.history_kept != HISTORY_KEPT,
        Group::MusicExtensions => standing.music_filters.extensions != MusicExtensions::default(),
        Group::MinimumLength => standing.music_filters.minimum_length != MinimumLength::default(),
        Group::Repeating => standing.skip_under_repeat != SkipUnderRepeat::default(),
        Group::PreviousButton => standing.previous_restarts != PreviousRestarts::default(),
        Group::Shuffling => standing.smart_shuffle != SmartShuffle::default(),
        Group::Discord => {
            let (worn, off) = (&standing.presence, &Presence::OFF);
            worn.enabled != off.enabled || worn.app != off.app
        }
        Group::DiscordShows => {
            let (worn, off) = (&standing.presence, &Presence::OFF);
            worn.shown != off.shown
                || worn.pictured != off.pictured
                || worn.icon != off.icon
                || worn.progress != off.progress
                || worn.while_paused != off.while_paused
        }
        Group::SyncConnection => {
            let (worn, built) = (&standing.synchronised, &AS_BUILT.synchronised);
            worn.nearby_only != built.nearby_only
                || worn.server != built.server
                || worn.device != built.device
        }
        Group::SyncedSongs => standing.synchronised.songs != AS_BUILT.synchronised.songs,
        Group::SyncControl => standing.synchronised.control != AS_BUILT.synchronised.control,
        Group::Contact => standing.contact_given,
        Group::Measurers => standing.measurer_given,
        Group::Recognition => standing.key_given || standing.token_given,
        Group::Submitting => standing.submitting_given,
        Group::Lastfm => standing.lastfm_given,
        Group::Listening => {
            standing.listening_from != Listening::default()
                || standing.listening_for != CLIP_BY_DEFAULT
        }
        Group::Organising => standing.template_given,
        Group::Inbox => standing.inbox_given,
        Group::Folders => standing.music_folder_given,
        Group::ResonateKey => standing.resonate_key_given,
        Group::RoomCorrection => standing.convolving,
        Group::Speakers => standing.speakers_moved,
        Group::Subsonic => standing.subsonic_given,
        Group::Jellyfin => standing.jellyfin_given,
        Group::Tidal => standing.tidal_given,
        Group::Spotify => standing.spotify_given,
        Group::YoutubeMusic => standing.youtube_music_given,
        Group::Colour => {
            standing.appearance.theme != Appearance::DEFAULT.theme
                || standing.appearance.accent != Appearance::DEFAULT.accent
                || standing.appearance.follows.scheme != Appearance::DEFAULT.follows.scheme
        }
        Group::Layout => {
            standing.appearance.text_size != Appearance::DEFAULT.text_size
                || standing.appearance.follows.text != Appearance::DEFAULT.follows.text
        }
        Group::WindowButtons => standing.window_buttons != WINDOW_BUTTONS,
        Group::Motion => standing.reduced_motion != REDUCED_MOTION,
        Group::Scrollbars => standing.scrollbars != SCROLLBARS,
        Group::Language => standing.language != LANGUAGE,
        Group::LiveSpectrum => standing.spectral != SPECTRAL,
        Group::Tabs => standing.tabs != TABS,
        Group::Equalising
        | Group::Syncing
        | Group::Lookups
        | Group::AfterScan
        | Group::Metered
        | Group::Studies
        | Group::Lyrics
        | Group::Resuming
        | Group::Notifications
        | Group::Updates
        | Group::VolumeWheel
        | Group::MouseNavigation
        | Group::WindowState
        | Group::Bands
        | Group::AutoEq
        | Group::OwnMeasurement
        | Group::RoomMeasurement
        | Group::Abx
        | Group::Hidden
        | Group::Pairing
        | Group::SyncDevices
        | Group::Rescanning
        | Group::LookingUp
        | Group::Retagging
        | Group::Identical
        | Group::Vault
        | Group::Build
        | Group::Places
        | Group::Everything => false,
    }
}

pub(crate) fn puts_back(group: Group) -> Vec<Command> {
    let built = OutputSettings::of(&EngineConfig::default());

    match group {
        Group::Device => vec![Command::SetSink(built.sink)],
        Group::DeviceVolume => vec![Command::SetDeviceVolume(built.device_volume)],
        Group::SampleRate => vec![Command::SetBitPerfect(built.prefer_bit_perfect)],
        Group::GraphRate => vec![Command::SetForceGraphRate(built.force_graph_rate)],
        Group::Buffer => vec![Command::SetBuffer(built.buffer)],
        Group::Dop => vec![
            Command::SetDop(built.dop),
            Command::SetDsdLikePcm(built.dsd_like_pcm),
        ],
        Group::Bluetooth => vec![Command::SetBluetoothWake(built.bluetooth)],
        Group::Resampler => vec![
            Command::SetQuality(built.quality),
            Command::SetFilterPhase(built.filter_phase),
        ],
        Group::Dither => vec![Command::SetDither(built.dither)],
        Group::NoiseShaping => vec![Command::SetNoiseShaping(built.noise_shaping)],
        Group::Crossfeed => vec![Command::SetCrossfeed(built.crossfeed)],
        Group::ReplayGain => vec![
            Command::SetReplayGain(built.replay_gain),
            Command::SetLevelling(built.levelling),
        ],
        Group::TruePeak => vec![Command::SetTruePeak(built.true_peak)],
        Group::LossySources => vec![Command::SetRestoration(built.restoration)],
        Group::Repeating => vec![Command::SetSkipUnderRepeat(SkipUnderRepeat::default())],
        Group::PreviousButton => {
            vec![Command::SetPreviousRestarts(PreviousRestarts::default())]
        }
        Group::Shuffling => vec![Command::SetSmartShuffle(SmartShuffle::default())],
        Group::Equalising | Group::BoundTo => {
            vec![Command::SetEqualisation(Arc::clone(&built.equaliser))]
        }
        _ => Vec::new(),
    }
}

pub(crate) fn can_be_put_back(group: Group) -> bool {
    !group.keys().is_empty()
}

#[cfg(test)]
mod tests {
    use resonate_core::{Accent, TextSize, Theme};
    use resonate_engine::{DitherKind, NodeName, Quality};

    use super::*;

    #[test]
    fn a_build_as_it_left_the_workshop_differs_from_nothing() {
        let standing = Standing::as_built();

        for group in Group::ALL {
            assert!(
                !differs(group, &standing),
                "{} says it differs from the default it is the default of",
                group.title()
            );
        }
    }

    #[test]
    fn every_switch_is_put_back_and_weighed_by_the_one_group_holding_it() {
        for switch in resonate_core::Switch::ALL {
            let holding: Vec<_> = Group::ALL
                .into_iter()
                .filter(|group| group.switches().any(|held| held == switch))
                .collect();

            assert_eq!(
                holding.len(),
                1,
                "{switch} is held by {} groups",
                holding.len()
            );

            let turned = Switches::UNSAID.with(switch, !switch.on_unless_said());

            assert!(switched_from_as_built(holding[0], &turned));
        }
    }

    #[test]
    fn a_group_that_can_be_put_back_is_one_that_names_a_key() {
        for group in Group::ALL {
            assert_eq!(
                can_be_put_back(group),
                !group.keys().is_empty(),
                "{} offers to put back a setting it names no key for",
                group.title()
            );
        }
    }

    #[test]
    fn every_engine_setting_answers_with_the_command_that_puts_it_back() {
        let built = Standing::as_built();

        for group in Group::ALL {
            let commands = puts_back(group);
            let engine = matches!(
                group,
                Group::Device
                    | Group::DeviceVolume
                    | Group::SampleRate
                    | Group::GraphRate
                    | Group::Buffer
                    | Group::Dop
                    | Group::Bluetooth
                    | Group::Resampler
                    | Group::Dither
                    | Group::NoiseShaping
                    | Group::Crossfeed
                    | Group::ReplayGain
                    | Group::TruePeak
                    | Group::LossySources
                    | Group::Equalising
                    | Group::BoundTo
                    | Group::Shuffling
                    | Group::Repeating
                    | Group::PreviousButton
            );

            assert_eq!(
                !commands.is_empty(),
                engine,
                "{} sends {} commands to put itself back",
                group.title(),
                commands.len()
            );
            assert!(!differs(group, &built));
        }
    }

    #[test]
    fn a_setting_moved_off_its_default_says_so_and_nothing_beside_it_does() {
        let moved = Standing {
            output: OutputSettings {
                quality: Quality::LowLatency,
                ..Standing::as_built().output
            },
            ..Standing::as_built()
        };

        assert!(differs(Group::Resampler, &moved));
        assert!(!differs(Group::Dither, &moved));
        assert!(!differs(Group::Colour, &moved));
    }

    #[test]
    fn a_named_device_a_palette_and_a_size_are_each_read_as_a_change() {
        let built = Standing::as_built();

        assert!(differs(
            Group::Device,
            &Standing {
                output: OutputSettings {
                    sink: Some(NodeName::new("auto_null")),
                    ..built.output.clone()
                },
                ..built.clone()
            }
        ));
        assert!(differs(
            Group::Colour,
            &Standing {
                appearance: Appearance {
                    accent: Some(Accent::Teal),
                    ..Appearance::DEFAULT
                },
                ..built.clone()
            }
        ));
        assert!(differs(
            Group::Colour,
            &Standing {
                appearance: Appearance {
                    theme: Theme::Nord,
                    ..Appearance::DEFAULT
                },
                ..built.clone()
            }
        ));
        assert!(differs(
            Group::Layout,
            &Standing {
                appearance: Appearance {
                    text_size: TextSize::Large,
                    ..Appearance::DEFAULT
                },
                ..built.clone()
            }
        ));
        assert!(differs(
            Group::Contact,
            &Standing {
                contact_given: true,
                ..built.clone()
            }
        ));
        assert!(differs(
            Group::Measurers,
            &Standing {
                measurer_given: true,
                ..built.clone()
            }
        ));
        assert!(differs(
            Group::Organising,
            &Standing {
                template_given: true,
                ..built.clone()
            }
        ));
        assert!(differs(
            Group::FlacRenderers,
            &Standing {
                flac_renderers_named: true,
                ..built.clone()
            }
        ));
        assert!(differs(
            Group::LearntFormats,
            &Standing {
                formats_learnt: true,
                ..built.clone()
            }
        ));
    }

    #[test]
    fn either_window_button_hidden_is_read_as_a_change_and_nothing_beside_it_is() {
        let built = Standing::as_built();

        for hidden in [
            WindowButtons {
                minimise: false,
                ..WINDOW_BUTTONS
            },
            WindowButtons {
                maximise: false,
                ..WINDOW_BUTTONS
            },
        ] {
            let moved = Standing {
                window_buttons: hidden,
                ..built.clone()
            };

            assert!(differs(Group::WindowButtons, &moved));
            assert!(!differs(Group::Layout, &moved));
            assert!(!differs(Group::Colour, &moved));
        }
    }

    #[test]
    fn what_puts_the_dither_back_is_the_kind_the_build_was_made_with() {
        let commands = puts_back(Group::Dither);

        assert_eq!(commands, vec![Command::SetDither(DitherKind::Triangular)]);
        assert_eq!(
            puts_back(Group::Dop),
            vec![Command::SetDop(false), Command::SetDsdLikePcm(false)]
        );
    }
}
