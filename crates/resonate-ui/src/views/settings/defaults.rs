use std::{
    sync::{Arc, LazyLock},
    time::Duration,
};

use resonate_core::{Appearance, Presence};
use resonate_engine::{Command, EngineConfig, OutputSettings, SkipUnderRepeat};
use resonate_listen::{CLIP_BY_DEFAULT, Listening};

use crate::{WindowButtons, views::settings::find::Group};

pub(crate) const ONLINE: bool = true;
pub(crate) const ENRICH_AFTER_SCAN: bool = true;
pub(crate) const STUDY: bool = true;
pub(crate) const RESUME: bool = true;
pub(crate) const NOTIFY: bool = true;
pub(crate) const WINDOW_BUTTONS: WindowButtons = WindowButtons::SHOWN;
pub(crate) const SCROLL_VOLUME: bool = true;
pub(crate) const SCROLLBARS: bool = true;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Standing {
    pub(crate) output: OutputSettings,
    pub(crate) appearance: Appearance,
    pub(crate) online: bool,
    pub(crate) after_scan: bool,
    pub(crate) studies: bool,
    pub(crate) contact_given: bool,
    pub(crate) key_given: bool,
    pub(crate) token_given: bool,
    pub(crate) listening_from: Listening,
    pub(crate) listening_for: Duration,
    pub(crate) resume: bool,
    pub(crate) skip_under_repeat: SkipUnderRepeat,
    pub(crate) notify: bool,
    pub(crate) window_buttons: WindowButtons,
    pub(crate) scroll_volume: bool,
    pub(crate) scrollbars: bool,
    pub(crate) presence: Presence,
    pub(crate) template_given: bool,
    pub(crate) inbox_given: bool,
}

impl Standing {
    pub(crate) fn as_built() -> Self {
        Self {
            output: OutputSettings::of(&EngineConfig::default()),
            appearance: Appearance::DEFAULT,
            online: ONLINE,
            after_scan: ENRICH_AFTER_SCAN,
            studies: STUDY,
            contact_given: false,
            key_given: false,
            token_given: false,
            listening_from: Listening::Desktop,
            listening_for: CLIP_BY_DEFAULT,
            resume: RESUME,
            skip_under_repeat: SkipUnderRepeat::default(),
            notify: NOTIFY,
            window_buttons: WINDOW_BUTTONS,
            scroll_volume: SCROLL_VOLUME,
            scrollbars: SCROLLBARS,
            presence: Presence::OFF,
            template_given: false,
            inbox_given: false,
        }
    }
}

static AS_BUILT: LazyLock<Standing> = LazyLock::new(Standing::as_built);

pub(crate) fn differs(group: Group, standing: &Standing) -> bool {
    let (worn, built) = (&standing.output, &AS_BUILT.output);

    match group {
        Group::Device => worn.sink != built.sink,
        Group::SampleRate => worn.prefer_bit_perfect != built.prefer_bit_perfect,
        Group::GraphRate => worn.force_graph_rate != built.force_graph_rate,
        Group::Buffer => worn.buffer != built.buffer,
        Group::Dop => worn.dop != built.dop,
        Group::Bluetooth => worn.bluetooth != built.bluetooth,
        Group::Resampler => {
            worn.quality != built.quality || worn.filter_phase != built.filter_phase
        }
        Group::Dither => worn.dither != built.dither,
        Group::NoiseShaping => worn.noise_shaping != built.noise_shaping,
        Group::ReplayGain => {
            worn.replay_gain != built.replay_gain || worn.levelling != built.levelling
        }
        Group::TruePeak => worn.true_peak != built.true_peak,
        Group::LossySources => worn.restoration != built.restoration,
        Group::Equalising => worn.equaliser.enabled != built.equaliser.enabled,
        Group::BoundTo => worn.equaliser.binds_anything(),
        Group::Lookups => standing.online != ONLINE,
        Group::AfterScan => standing.after_scan != ENRICH_AFTER_SCAN,
        Group::Studies => standing.studies != STUDY,
        Group::Resuming => standing.resume != RESUME,
        Group::Repeating => standing.skip_under_repeat != SkipUnderRepeat::default(),
        Group::Notifications => standing.notify != NOTIFY,
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
        Group::Contact => standing.contact_given,
        Group::Recognition => standing.key_given || standing.token_given,
        Group::Listening => {
            standing.listening_from != Listening::Desktop
                || standing.listening_for != CLIP_BY_DEFAULT
        }
        Group::Organising => standing.template_given,
        Group::Inbox => standing.inbox_given,
        Group::Colour => {
            standing.appearance.theme != Appearance::DEFAULT.theme
                || standing.appearance.accent != Appearance::DEFAULT.accent
        }
        Group::Layout => standing.appearance.text_size != Appearance::DEFAULT.text_size,
        Group::WindowButtons => standing.window_buttons != WINDOW_BUTTONS,
        Group::VolumeWheel => standing.scroll_volume != SCROLL_VOLUME,
        Group::Scrollbars => standing.scrollbars != SCROLLBARS,
        Group::Bands
        | Group::Measured
        | Group::Folders
        | Group::Scanning
        | Group::Refreshing
        | Group::Tagging
        | Group::Vault
        | Group::LookUpNow
        | Group::Build
        | Group::Places
        | Group::Everything => false,
    }
}

pub(crate) fn puts_back(group: Group) -> Vec<Command> {
    let built = OutputSettings::of(&EngineConfig::default());

    match group {
        Group::Device => vec![Command::SetSink(built.sink)],
        Group::SampleRate => vec![Command::SetBitPerfect(built.prefer_bit_perfect)],
        Group::GraphRate => vec![Command::SetForceGraphRate(built.force_graph_rate)],
        Group::Buffer => vec![Command::SetBuffer(built.buffer)],
        Group::Dop => vec![Command::SetDop(built.dop)],
        Group::Bluetooth => vec![Command::SetBluetoothWake(built.bluetooth)],
        Group::Resampler => vec![
            Command::SetQuality(built.quality),
            Command::SetFilterPhase(built.filter_phase),
        ],
        Group::Dither => vec![Command::SetDither(built.dither)],
        Group::NoiseShaping => vec![Command::SetNoiseShaping(built.noise_shaping)],
        Group::ReplayGain => vec![
            Command::SetReplayGain(built.replay_gain),
            Command::SetLevelling(built.levelling),
        ],
        Group::TruePeak => vec![Command::SetTruePeak(built.true_peak)],
        Group::LossySources => vec![Command::SetRestoration(built.restoration)],
        Group::Repeating => vec![Command::SetSkipUnderRepeat(SkipUnderRepeat::default())],
        Group::Equalising | Group::BoundTo => {
            vec![Command::SetEqualisation(Arc::clone(&built.equaliser))]
        }
        Group::Bands
        | Group::Measured
        | Group::Lookups
        | Group::AfterScan
        | Group::Studies
        | Group::Contact
        | Group::Recognition
        | Group::Listening
        | Group::Colour
        | Group::Layout
        | Group::WindowButtons
        | Group::VolumeWheel
        | Group::Scrollbars
        | Group::Folders
        | Group::Scanning
        | Group::Refreshing
        | Group::Tagging
        | Group::Organising
        | Group::Vault
        | Group::Inbox
        | Group::Resuming
        | Group::Notifications
        | Group::Discord
        | Group::DiscordShows
        | Group::LookUpNow
        | Group::Build
        | Group::Places
        | Group::Everything => Vec::new(),
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
                    | Group::SampleRate
                    | Group::GraphRate
                    | Group::Buffer
                    | Group::Dop
                    | Group::Bluetooth
                    | Group::Resampler
                    | Group::Dither
                    | Group::NoiseShaping
                    | Group::ReplayGain
                    | Group::TruePeak
                    | Group::LossySources
                    | Group::Equalising
                    | Group::BoundTo
                    | Group::Repeating
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
                quality: Quality::Fast,
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
            Group::Organising,
            &Standing {
                template_given: true,
                ..built.clone()
            }
        ));
        assert!(differs(
            Group::Lookups,
            &Standing {
                online: false,
                ..built.clone()
            }
        ));
        assert!(differs(
            Group::AfterScan,
            &Standing {
                after_scan: false,
                ..built.clone()
            }
        ));
        assert!(differs(
            Group::Studies,
            &Standing {
                studies: false,
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
        assert_eq!(puts_back(Group::Dop), vec![Command::SetDop(false)]);
    }
}
