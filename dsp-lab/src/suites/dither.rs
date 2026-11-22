use resonate_core::{BitDepth, ChannelLayout, SampleRate};
use resonate_dsp::{Dither, DitherKind, NoiseShaping};

use crate::{
    analysis::{
        fidelity::{self, BLOCK, Feed, LOUD_DBFS, MIDDLE_DBFS, amplitude_of, steady, tone_hz},
        level::power_decibels,
        spectrum::{AUDIBLE_FROM_HZ, Spectrum},
    },
    error::Result,
    lab::{
        metric::{Better, Metric, Reading, Shown, Tolerance},
        pair::Pair,
        subject::{Subject, ran},
        suite::{Outcome, Suite},
    },
    signal,
    subjects::stage::Stage,
};

const SEED: u64 = 0x6469_7468_6572;
const FULL_SCALE_SINE_POWER: f64 = 0.5;
const SILENCE_AFTER_SECONDS: f64 = 1.0;
const HEARD_SECONDS: f64 = 0.5;
const SHAPED_LOW_HZ: (f64, f64) = (1_000.0, 5_000.0);
const SHAPED_HIGH_HZ: (f64, f64) = (15_000.0, 20_000.0);

const KINDS: [DitherKind; 3] = [
    DitherKind::None,
    DitherKind::Rectangular,
    DitherKind::Triangular,
];
const SHAPINGS: [NoiseShaping; 3] = [
    NoiseShaping::None,
    NoiseShaping::Lipshitz,
    NoiseShaping::Threshold,
];

pub const NOISE: Metric = Metric::new(
    "noise-dbfs",
    "Noise to Nyquist (dBFS)",
    Better::Lower,
    Tolerance::DECIBELS,
    Shown::Decibels,
);
pub const NOISE_A: Metric = Metric::new(
    "noise-a-dbfs",
    "Noise, A-weighted (dBFS)",
    Better::Lower,
    Tolerance::DECIBELS,
    Shown::Decibels,
);
pub const NOISE_LOW: Metric = Metric::new(
    "noise-1-5-khz-dbfs",
    "Noise 1–5 kHz (dBFS)",
    Better::Lower,
    Tolerance::DECIBELS,
    Shown::Decibels,
);
pub const NOISE_HIGH: Metric = Metric::new(
    "noise-15-20-khz-dbfs",
    "Noise 15–20 kHz (dBFS)",
    Better::Same,
    Tolerance::DECIBELS,
    Shown::Decibels,
);
pub const STEP_HARMONICS: Metric = Metric::new(
    "harmonics-of-a-step-db",
    "Harmonics of a one-step tone (dB)",
    Better::Lower,
    Tolerance::DECIBELS,
    Shown::Decibels,
);
pub const MODULATION: Metric = Metric::new(
    "noise-modulation-db",
    "Noise modulation (dB)",
    Better::Lower,
    Tolerance::DECIBELS,
    Shown::Digits(2),
);
pub const ON_THE_GRID: Metric = Metric::new(
    "on-the-grid",
    "Every sample on the grid",
    Better::Higher,
    Tolerance::EXACT,
    Shown::YesOrNo,
);
pub const SILENT_AFTER: Metric = Metric::new(
    "silent-after-frames",
    "Frames to silence",
    Better::Lower,
    Tolerance::within(1.0),
    Shown::Count,
);

const METRICS: [&Metric; 8] = [
    &NOISE,
    &NOISE_A,
    &NOISE_LOW,
    &NOISE_HIGH,
    &STEP_HARMONICS,
    &MODULATION,
    &ON_THE_GRID,
    &SILENT_AFTER,
];

pub struct Dithering {
    pub depth: BitDepth,
}

const fn bits(depth: BitDepth) -> u8 {
    match depth {
        BitDepth::Bits8 => 8,
        BitDepth::Bits16 => 16,
        BitDepth::Bits18 => 18,
        BitDepth::Bits20 => 20,
        BitDepth::Bits24 => 24,
        BitDepth::Bits32 => 32,
    }
}

fn kind_named(kind: DitherKind) -> &'static str {
    match kind {
        DitherKind::None => "no dither",
        DitherKind::Rectangular => "rectangular",
        DitherKind::Triangular => "triangular",
    }
}

fn shaping_named(shaping: NoiseShaping) -> &'static str {
    match shaping {
        NoiseShaping::None => "flat",
        NoiseShaping::Lipshitz => "Lipshitz",
        NoiseShaping::Threshold => "threshold",
    }
}

impl Dithering {
    fn step(&self) -> f64 {
        1.0 / f64::from(1_u32 << (bits(self.depth) - 1))
    }

    fn noise(&self, subject: &dyn Subject, pair: Pair, dbfs: f64) -> Result<(Spectrum, f64)> {
        let hz = tone_hz(pair, fidelity::TONE_HZ);
        let amplitude = amplitude_of(dbfs);

        let heard = steady(
            subject,
            pair,
            Feed::every(ChannelLayout::Mono),
            |rate, frames| signal::sine(hz, amplitude, rate, frames),
        )?;
        let spectrum = heard.spectra().remove(0);

        Ok((spectrum, hz))
    }

    fn after_the_music(&self, subject: &dyn Subject, pair: Pair) -> Result<(bool, f64)> {
        let rate = pair.input_hz();
        let sounding = (HEARD_SECONDS * rate) as usize;
        let silent = (SILENCE_AFTER_SECONDS * rate) as usize;

        let mut input = signal::sine(fidelity::TONE_HZ, amplitude_of(MIDDLE_DBFS), rate, sounding);

        input.resize(sounding + silent, 0.0);

        let heard = ran(subject, pair, ChannelLayout::Mono, &input, BLOCK)?;
        let step = self.step();

        let on_the_grid = heard
            .output
            .iter()
            .all(|sample| (sample / step).fract() == 0.0);
        let last_sounding = heard
            .output
            .iter()
            .rposition(|sample| *sample != 0.0)
            .map_or(0, |at| at + 1);

        Ok((on_the_grid, last_sounding.saturating_sub(sounding) as f64))
    }
}

impl Suite for Dithering {
    fn name(&self) -> &'static str {
        match self.depth {
            BitDepth::Bits16 => "dither-16",
            _ => "dither-24",
        }
    }

    fn title(&self) -> &'static str {
        match self.depth {
            BitDepth::Bits16 => "Dither to 16 bits",
            _ => "Dither to 24 bits",
        }
    }

    fn about(&self) -> &'static str {
        "each dither kind and noise shaping: the noise it leaves, where shaping moves it, the distortion it removes and whether it holds the grid"
    }

    fn subjects(&self) -> Result<Vec<Box<dyn Subject>>> {
        let depth = self.depth;

        Ok(KINDS
            .iter()
            .flat_map(|&kind| SHAPINGS.iter().map(move |&shaping| (kind, shaping)))
            .filter(|&(kind, shaping)| kind != DitherKind::None || shaping == NoiseShaping::None)
            .map(|(kind, shaping)| {
                Box::new(Stage::new(
                    format!("Dither {}, {}", kind_named(kind), shaping_named(shaping)),
                    move |_, _| Ok(Box::new(Dither::new(depth, kind, shaping, SEED))),
                )) as Box<dyn Subject>
            })
            .collect())
    }

    fn pairs(&self) -> Vec<Pair> {
        [
            SampleRate::HZ_44100,
            SampleRate::HZ_48000,
            SampleRate::HZ_96000,
        ]
        .map(Pair::at)
        .to_vec()
    }

    fn metrics(&self) -> &'static [&'static Metric] {
        &METRICS
    }

    fn measure(&self, subject: &dyn Subject, pair: Pair) -> Result<Outcome> {
        let (spectrum, hz) = self.noise(subject, pair, MIDDLE_DBFS)?;
        let nyquist = spectrum.nyquist();
        let relative = |power: f64| power_decibels(power / FULL_SCALE_SINE_POWER);

        let noise = relative(spectrum.band_without(AUDIBLE_FROM_HZ, nyquist, &[hz]));
        let noise_a =
            relative(spectrum.weighted_without(AUDIBLE_FROM_HZ, spectrum.audible_top(), &[hz]));
        let low = relative(spectrum.band_without(SHAPED_LOW_HZ.0, SHAPED_LOW_HZ.1, &[hz]));
        let high =
            relative(spectrum.band_without(SHAPED_HIGH_HZ.0, SHAPED_HIGH_HZ.1.min(nyquist), &[hz]));

        let step_dbfs = 20.0 * self.step().log10();
        let (quiet, quiet_hz) = self.noise(subject, pair, step_dbfs)?;

        let harmonics: f64 = quiet
            .harmonics_of(quiet_hz)
            .iter()
            .map(|&harmonic| quiet.power_at(harmonic))
            .sum();
        let step_harmonics = power_decibels(harmonics / quiet.power_at(quiet_hz));

        let (loud, loud_hz) = self.noise(subject, pair, LOUD_DBFS)?;

        let levels = [
            noise,
            relative(loud.band_without(AUDIBLE_FROM_HZ, nyquist, &[loud_hz])),
            relative(quiet.band_without(AUDIBLE_FROM_HZ, nyquist, &[quiet_hz])),
        ];
        let modulation = levels.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - levels.iter().copied().fold(f64::INFINITY, f64::min);

        let (on_the_grid, silent_after) = self.after_the_music(subject, pair)?;

        Ok(Outcome::of(vec![
            Reading::of(&NOISE, noise),
            Reading::of(&NOISE_A, noise_a),
            Reading::of(&NOISE_LOW, low),
            Reading::of(&NOISE_HIGH, high),
            Reading::of(&STEP_HARMONICS, step_harmonics),
            Reading::of(&MODULATION, modulation),
            Reading::of(&ON_THE_GRID, f64::from(u8::from(on_the_grid))),
            Reading::of(&SILENT_AFTER, silent_after),
        ]))
    }

    fn timed_layout(&self) -> ChannelLayout {
        ChannelLayout::Stereo
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lab::suite::measured_as;

    const SIXTEEN: Dithering = Dithering {
        depth: BitDepth::Bits16,
    };
    const AT: Pair = Pair::at(SampleRate::HZ_48000);

    #[test]
    fn rounding_alone_distorts_a_quiet_tone_and_triangular_dither_takes_it_away() {
        let rounded = measured_as(&SIXTEEN, "Dither no dither, flat", AT);
        let dithered = measured_as(&SIXTEEN, "Dither triangular, flat", AT);

        assert_eq!(rounded.value(&ON_THE_GRID), 1.0);
        assert_eq!(dithered.value(&ON_THE_GRID), 1.0);
        assert!(rounded.value(&STEP_HARMONICS) > -20.0);
        assert!(dithered.value(&STEP_HARMONICS) < rounded.value(&STEP_HARMONICS) - 20.0);
        assert!(dithered.value(&MODULATION) < 0.5);
    }

    #[test]
    fn shaping_moves_the_noise_out_of_the_band_the_ear_hears_best() {
        let flat = measured_as(&SIXTEEN, "Dither triangular, flat", AT);
        let shaped = measured_as(&SIXTEEN, "Dither triangular, threshold", AT);

        assert!(shaped.value(&NOISE_LOW) < flat.value(&NOISE_LOW) - 10.0);
        assert!(shaped.value(&NOISE_HIGH) > flat.value(&NOISE_HIGH));
    }
}
