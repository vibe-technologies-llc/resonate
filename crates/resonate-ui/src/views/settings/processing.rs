use gpui::{Context, Div, SharedString, prelude::*};
use resonate_core::Trim;
use resonate_engine::{
    Command, DitherKind, FilterPhase, Levelling, NoiseShaping, Quality, ReplayGainMode,
    Restoration, SincParams,
};

use crate::{
    Setting,
    views::{
        kit,
        root::RootView,
        settings::{Choice, switch_row},
    },
};

impl Choice for Quality {
    const ALL: &'static [Self] = &[Self::Fast, Self::Balanced, Self::High, Self::VeryHigh];

    fn label(self) -> &'static str {
        match self {
            Self::Fast => "Fast",
            Self::Balanced => "Balanced",
            Self::High => "High",
            Self::VeryHigh => "Very high",
        }
    }

    fn detail(self) -> Option<SharedString> {
        Some(filter(self.params()))
    }
}

impl Choice for FilterPhase {
    const ALL: &'static [Self] = &[Self::Linear, Self::Intermediate, Self::Minimum];

    fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::Intermediate => "Intermediate",
            Self::Minimum => "Minimum",
        }
    }
}

impl Choice for Restoration {
    const ALL: &'static [Self] = &[Self::Off, Self::Repair, Self::Extend];

    fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Repair => "Repair",
            Self::Extend => "Repair and extend",
        }
    }
}

impl Choice for DitherKind {
    const ALL: &'static [Self] = &[Self::None, Self::Rectangular, Self::Triangular];

    fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Rectangular => "Rectangular",
            Self::Triangular => "Triangular",
        }
    }
}

impl Choice for NoiseShaping {
    const ALL: &'static [Self] = &[Self::None, Self::Lipshitz, Self::Threshold];

    fn label(self) -> &'static str {
        match self {
            Self::None => "Flat",
            Self::Lipshitz => "Lipshitz",
            Self::Threshold => "Threshold",
        }
    }

    fn detail(self) -> Option<SharedString> {
        match self {
            Self::None => None,
            Self::Lipshitz => Some(SharedString::new_static(
                "An E-weighted fit to the ear's sensitivity, designed at 44.1 kHz. It runs at \
                 44.1 and 48 kHz and falls back to flat dither at every other rate.",
            )),
            Self::Threshold => Some(SharedString::new_static(
                "Designed at the device's own rate from the threshold of hearing, so it shapes \
                 at every rate. The noise it leaves where the ear can hear it is 7 dB quieter \
                 than Lipshitz's at 44.1 kHz, and 26 dB quieter than flat dither at 96 kHz.",
            )),
        }
    }
}

impl Choice for ReplayGainMode {
    const ALL: &'static [Self] = &[Self::Off, Self::Track, Self::Album];

    fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Track => "Track",
            Self::Album => "Album",
        }
    }
}

const PRE_AMPS: [(Trim, &str); 5] = [
    (trimmed(-600), "−6 dB"),
    (trimmed(-300), "−3 dB"),
    (Trim::NONE, "None"),
    (trimmed(300), "+3 dB"),
    (trimmed(600), "+6 dB"),
];

const UNTAGGED: [(Trim, &str); 4] = [
    (Trim::NONE, "None"),
    (trimmed(-300), "−3 dB"),
    (trimmed(-600), "−6 dB"),
    (trimmed(-900), "−9 dB"),
];

const fn trimmed(millibels: i32) -> Trim {
    match Trim::from_millibels(millibels) {
        Ok(trim) => trim,
        Err(_) => Trim::NONE,
    }
}

fn labelled(table: &[(Trim, &'static str)], trim: Trim) -> &'static str {
    table
        .iter()
        .find_map(|(held, label)| (*held == trim).then_some(*label))
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

    fn detail(self) -> Option<SharedString> {
        Some(SharedString::new_static(
            "Added to every gain a ReplayGain tag asks for. Clip prevention still holds the \
             result under the peak the tag declares.",
        ))
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

    fn detail(self) -> Option<SharedString> {
        Some(SharedString::new_static(
            "The gain a track carrying no ReplayGain tag is played at, so it sits nearer the \
             tagged tracks around it.",
        ))
    }
}

impl RootView {
    pub(super) fn resampler_group(&mut self, cx: &mut Context<Self>) -> Div {
        let settings = self.player.read(cx).output_settings();
        let (quality, phase) = (settings.quality, settings.filter_phase);

        kit::section_body()
            .child(self.choices(
                "quality",
                Some(quality),
                cx,
                |this, quality: Quality, cx| {
                    this.send(Command::SetQuality(quality), cx);
                    this.store(&Setting::Quality(quality), cx);
                },
            ))
            .child(kit::field(
                "Phase",
                self.choices(
                    "filter-phase",
                    Some(phase),
                    cx,
                    |this, phase: FilterPhase, cx| {
                        this.send(Command::SetFilterPhase(phase), cx);
                        this.store(&Setting::FilterPhase(phase), cx);
                    },
                ),
            ))
    }

    pub(super) fn true_peak_group(&mut self, cx: &mut Context<Self>) -> Div {
        let guarded = self.player.read(cx).output_settings().true_peak;

        kit::section_body().child(self.in_the_ring(
            "true-peak",
            switch_row(
                "Keep true peaks under full scale",
                "Turn a track down by its measured peak and guard what is processed",
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

    pub(super) fn lossy_sources_group(&mut self, cx: &mut Context<Self>) -> Div {
        let restoration = self.player.read(cx).output_settings().restoration;

        kit::section_body().child(self.choices(
            "restore-lossy",
            Some(restoration),
            cx,
            |this, restoration: Restoration, cx| {
                this.send(Command::SetRestoration(restoration), cx);
                this.store(&Setting::Restoration(restoration), cx);
            },
        ))
    }

    pub(super) fn dither_group(&mut self, cx: &mut Context<Self>) -> Div {
        let dither = self.player.read(cx).output_settings().dither;

        kit::section_body().child(self.choices(
            "dither",
            Some(dither),
            cx,
            |this, dither: DitherKind, cx| {
                this.send(Command::SetDither(dither), cx);
                this.store(&Setting::Dither(dither), cx);
            },
        ))
    }

    pub(super) fn noise_shaping_group(&mut self, cx: &mut Context<Self>) -> Div {
        let shaping = self.player.read(cx).output_settings().noise_shaping;

        kit::section_body().child(self.choices(
            "noise-shaping",
            Some(shaping),
            cx,
            |this, shaping: NoiseShaping, cx| {
                this.send(Command::SetNoiseShaping(shaping), cx);
                this.store(&Setting::NoiseShaping(shaping), cx);
            },
        ))
    }

    pub(super) fn replay_gain_group(&mut self, cx: &mut Context<Self>) -> Div {
        let settings = self.player.read(cx).output_settings();
        let mode = settings.replay_gain;
        let levelling = settings.levelling;

        kit::section_body()
            .child(self.choices(
                "replay-gain",
                Some(mode),
                cx,
                |this, mode: ReplayGainMode, cx| {
                    this.send(Command::SetReplayGain(mode), cx);
                    this.store(&Setting::ReplayGain(mode), cx);
                },
            ))
            .child(kit::field(
                "Pre-amp",
                self.choices(
                    "pre-amp",
                    Some(PreAmp(levelling.pre_amp)),
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
                "Without tags",
                self.choices(
                    "untagged",
                    Some(Untagged(levelling.untagged)),
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

    format!(
        "{} source samples either side of each output instant — {taps} taps where the \
         device's rate is at or above the file's, and more in proportion below it. {} phases in \
         the table those weights are read from. Cutoff {:.3} of Nyquist, Kaiser β {:.1}.",
        params.half_taps, params.phases, params.cutoff, params.kaiser_beta,
    )
    .into()
}
