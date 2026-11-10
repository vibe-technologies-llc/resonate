use gpui::{Context, SharedString, prelude::*};
use resonate_core::Trim;
use resonate_engine::{
    Command, Crossfeed, DitherKind, FilterPhase, Levelling, NoiseShaping, Quality, ReplayGainMode,
    Restoration, SincParams,
};

use crate::{
    Setting,
    views::{
        kit,
        root::RootView,
        settings::{Choice, Rows, switch_row},
    },
    words,
};

impl Choice for Quality {
    const ALL: &'static [Self] = &[Self::LowLatency, Self::Balanced, Self::High, Self::VeryHigh];

    fn label(self) -> &'static str {
        match self {
            Self::LowLatency => words::settings_processing::low_latency(),
            Self::Balanced => words::settings_processing::balanced(),
            Self::High => words::settings_processing::high(),
            Self::VeryHigh => words::settings_processing::very_high(),
        }
    }

    fn detail(self) -> Option<SharedString> {
        Some(filter(self.params()))
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self {
            Self::LowLatency => words::settings_processing::answers_about_a_millisecond_after_the(),
            Self::Balanced => words::settings_processing::the_least_work_of_the_four(),
            Self::High => words::settings_processing::a_sharper_filter_whose_errors_sit(),
            Self::VeryHigh => words::settings_processing::the_sharpest_filter_keeping_the_treble(),
        })
    }
}

impl Choice for FilterPhase {
    const ALL: &'static [Self] = &[Self::Linear, Self::Intermediate, Self::Minimum];

    fn label(self) -> &'static str {
        match self {
            Self::Linear => words::settings_processing::linear(),
            Self::Intermediate => words::settings_processing::intermediate(),
            Self::Minimum => words::settings_processing::minimum(),
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self {
            Self::Linear => words::settings_processing::every_frequency_is_delayed_alike_so(),
            Self::Intermediate => {
                words::settings_processing::halfway_between_the_two_less_ringing()
            }
            Self::Minimum => words::settings_processing::nothing_rings_before_a_sharp_sound(),
        })
    }
}

impl Choice for Restoration {
    const ALL: &'static [Self] = &[Self::Off, Self::Repair, Self::Extend];

    fn label(self) -> &'static str {
        match self {
            Self::Off => words::settings_processing::off(),
            Self::Repair => words::settings_processing::repair(),
            Self::Extend => words::settings_processing::repair_and_extend(),
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self {
            Self::Off => words::settings_processing::mp3_aac_and_vorbis_files_play(),
            Self::Repair => words::settings_processing::undoes_the_damage_an_encoder_leaves(),
            Self::Extend => words::settings_processing::repairs_as_above_then_rebuilds_the(),
        })
    }
}

impl Choice for DitherKind {
    const ALL: &'static [Self] = &[Self::None, Self::Rectangular, Self::Triangular];

    fn label(self) -> &'static str {
        match self {
            Self::None => words::settings_processing::none(),
            Self::Rectangular => words::settings_processing::rectangular(),
            Self::Triangular => words::settings_processing::triangular(),
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self {
            Self::None => words::settings_processing::where_the_bits_have_to_be(),
            Self::Rectangular => words::settings_processing::a_whisper_of_noise_is_added(),
            Self::Triangular => words::settings_processing::twice_the_whisper_which_leaves_only(),
        })
    }
}

impl Choice for NoiseShaping {
    const ALL: &'static [Self] = &[Self::None, Self::Lipshitz, Self::Threshold];

    fn label(self) -> &'static str {
        match self {
            Self::None => words::settings_processing::flat(),
            Self::Lipshitz => "Lipshitz",
            Self::Threshold => words::settings_processing::threshold(),
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self {
            Self::None => words::settings_processing::the_dithers_hiss_is_spread_evenly(),
            Self::Lipshitz => words::settings_processing::an_e_weighted_fit_to_the(),
            Self::Threshold => words::settings_processing::designed_at_the_devices_own_rate(),
        })
    }
}

impl Choice for Crossfeed {
    const ALL: &'static [Self] = &[Self::Off, Self::Light, Self::Moderate, Self::Strong];

    fn label(self) -> &'static str {
        match self {
            Self::Off => words::settings_processing::off(),
            Self::Light => words::settings_processing::light(),
            Self::Moderate => words::settings_processing::moderate(),
            Self::Strong => words::settings_processing::strong(),
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self {
            Self::Off => words::settings_processing::each_channel_stays_in_its_own(),
            Self::Light => words::settings_processing::a_faint_low_copy_of_each(),
            Self::Moderate => words::settings_processing::the_classic_setting_700_hz_6(),
            Self::Strong => words::settings_processing::the_most_of_the_other_side(),
        })
    }
}

impl Choice for ReplayGainMode {
    const ALL: &'static [Self] = &[Self::Off, Self::Track, Self::Album];

    fn label(self) -> &'static str {
        match self {
            Self::Off => words::settings_processing::off(),
            Self::Track => words::settings_processing::track(),
            Self::Album => words::settings_processing::album(),
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self {
            Self::Off => words::settings_processing::every_track_plays_at_the_level(),
            Self::Track => words::settings_processing::each_track_is_brought_to_one(),
            Self::Album => words::settings_processing::each_album_is_brought_to_one(),
        })
    }
}

type Said = fn() -> &'static str;

const PRE_AMPS: [(Trim, Said); 5] = [
    (trimmed(-600), words::settings_processing::lowered_6_db),
    (trimmed(-300), words::settings_processing::lowered_3_db),
    (Trim::NONE, words::settings_processing::none),
    (trimmed(300), words::settings_processing::raised_3_db),
    (trimmed(600), words::settings_processing::raised_6_db),
];

const UNTAGGED: [(Trim, Said); 4] = [
    (Trim::NONE, words::settings_processing::none),
    (trimmed(-300), words::settings_processing::lowered_3_db),
    (trimmed(-600), words::settings_processing::lowered_6_db),
    (trimmed(-900), words::settings_processing::lowered_9_db),
];

const fn trimmed(millibels: i32) -> Trim {
    match Trim::from_millibels(millibels) {
        Ok(trim) => trim,
        Err(_) => Trim::NONE,
    }
}

fn labelled(table: &[(Trim, Said)], trim: Trim) -> &'static str {
    table
        .iter()
        .find_map(|(held, said)| (*held == trim).then(said))
        .unwrap_or_default()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PreAmp(Trim);

impl Choice for PreAmp {
    const ALL: &'static [Self] = &[
        Self(PRE_AMPS[0].0),
        Self(PRE_AMPS[1].0),
        Self(PRE_AMPS[2].0),
        Self(PRE_AMPS[3].0),
        Self(PRE_AMPS[4].0),
    ];

    fn label(self) -> &'static str {
        labelled(&PRE_AMPS, self.0)
    }

    fn in_force(self) -> SharedString {
        SharedString::from(self.0.to_string())
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(words::settings_processing::added_to_every_gain_a_replaygain())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Untagged(Trim);

impl Choice for Untagged {
    const ALL: &'static [Self] = &[
        Self(UNTAGGED[0].0),
        Self(UNTAGGED[1].0),
        Self(UNTAGGED[2].0),
        Self(UNTAGGED[3].0),
    ];

    fn label(self) -> &'static str {
        labelled(&UNTAGGED, self.0)
    }

    fn in_force(self) -> SharedString {
        SharedString::from(self.0.to_string())
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(words::settings_processing::the_gain_a_track_carrying_no())
    }
}

impl RootView {
    pub(super) fn resampler_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let settings = self.player.read(cx).output_settings();
        let (quality, phase) = (settings.quality, settings.filter_phase);

        kit::rows()
            .child(
                self.choices("quality", quality, cx, |this, quality: Quality, cx| {
                    this.send(Command::SetQuality(quality), cx);
                    this.store(&Setting::Quality(quality), cx);
                }),
            )
            .child(kit::field(
                words::settings_processing::phase(),
                self.choices("filter-phase", phase, cx, |this, phase: FilterPhase, cx| {
                    this.send(Command::SetFilterPhase(phase), cx);
                    this.store(&Setting::FilterPhase(phase), cx);
                }),
            ))
    }

    pub(super) fn true_peak_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let guarded = self.player.read(cx).output_settings().true_peak;

        kit::rows().child(self.in_the_ring(
            "true-peak",
            switch_row(
                words::settings_processing::keep_true_peaks_under_full_scale(),
                words::settings_processing::turn_a_track_down_by_its(),
                guarded,
                "true-peak",
            ),
            move |this, _, cx| {
                this.send(Command::SetTruePeak(!guarded), cx);
                this.store(&Setting::TruePeak(!guarded), cx);
            },
            cx,
        ))
    }

    pub(super) fn lossy_sources_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let restoration = self.player.read(cx).output_settings().restoration;

        kit::rows()
            .child(self.choices(
                "restore-lossy",
                restoration,
                cx,
                |this, restoration: Restoration, cx| {
                    this.send(Command::SetRestoration(restoration), cx);
                    this.store(&Setting::Restoration(restoration), cx);
                },
            ))
            .note(words::settings_processing::lossy_only())
    }

    pub(super) fn crossfeed_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let crossfeed = self.player.read(cx).output_settings().crossfeed;

        kit::rows()
            .child(self.choices(
                "crossfeed",
                crossfeed,
                cx,
                |this, crossfeed: Crossfeed, cx| {
                    this.send(Command::SetCrossfeed(crossfeed), cx);
                    this.store(&Setting::Crossfeed(crossfeed), cx);
                },
            ))
            .note(words::settings_processing::crossfeed_stereo_only())
    }

    pub(super) fn dither_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let dither = self.player.read(cx).output_settings().dither;

        kit::rows().child(
            self.choices("dither", dither, cx, |this, dither: DitherKind, cx| {
                this.send(Command::SetDither(dither), cx);
                this.store(&Setting::Dither(dither), cx);
            }),
        )
    }

    pub(super) fn noise_shaping_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let shaping = self.player.read(cx).output_settings().noise_shaping;

        kit::rows().child(self.choices(
            "noise-shaping",
            shaping,
            cx,
            |this, shaping: NoiseShaping, cx| {
                this.send(Command::SetNoiseShaping(shaping), cx);
                this.store(&Setting::NoiseShaping(shaping), cx);
            },
        ))
    }

    pub(super) fn replay_gain_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let settings = self.player.read(cx).output_settings();
        let mode = settings.replay_gain;
        let levelling = settings.levelling;

        kit::rows()
            .child(
                self.choices("replay-gain", mode, cx, |this, mode: ReplayGainMode, cx| {
                    this.send(Command::SetReplayGain(mode), cx);
                    this.store(&Setting::ReplayGain(mode), cx);
                }),
            )
            .child(kit::field(
                words::settings_processing::pre_amp(),
                self.choices(
                    "pre-amp",
                    PreAmp(levelling.pre_amp),
                    cx,
                    |this, step: PreAmp, cx| {
                        let levelling = Levelling {
                            pre_amp: step.0,
                            ..this.player.read(cx).output_settings().levelling
                        };
                        this.send(Command::SetLevelling(levelling), cx);
                        this.store(&Setting::PreAmp(step.0), cx);
                    },
                ),
            ))
            .child(kit::field(
                words::settings_processing::without_tags(),
                self.choices(
                    "untagged",
                    Untagged(levelling.untagged),
                    cx,
                    |this, step: Untagged, cx| {
                        let levelling = Levelling {
                            untagged: step.0,
                            ..this.player.read(cx).output_settings().levelling
                        };
                        this.send(Command::SetLevelling(levelling), cx);
                        this.store(&Setting::Untagged(step.0), cx);
                    },
                ),
            ))
    }
}

fn filter(params: SincParams) -> SharedString {
    let taps = 2 * u32::from(params.half_taps) + 1;

    if params.converts_in_blocks() {
        return words::settings_processing::filter_in_blocks(
            format!("{:.3}", params.cutoff),
            params.half_taps,
        )
        .into();
    }

    words::settings_processing::filter_in_phases(
        format!("{:.1}", params.kaiser_beta),
        format!("{:.3}", params.cutoff),
        params.half_taps,
        params.phases,
        taps,
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::settings::said_under;

    #[test]
    fn a_trim_off_the_table_says_what_is_in_force() {
        let off = Trim::from_millibels(150).expect("a trim");

        assert_eq!(
            said_under(PreAmp(off)),
            "+1.5 dB is in force, which is none of these."
        );
        assert_eq!(
            said_under(Untagged(off)),
            "+1.5 dB is in force, which is none of these."
        );
        assert_eq!(said_under(PreAmp(Trim::NONE)), PreAmp(Trim::NONE).meaning());
    }
}
