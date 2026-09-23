use std::{
    array,
    f64::consts::{LN_10, PI},
    num::NonZeroUsize,
};

use resonate_core::{BitDepth, SampleRate, StreamSpec};

use crate::{ProcessCount, Processor, Result};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DitherKind {
    None,
    Rectangular,
    #[default]
    Triangular,
}

impl DitherKind {
    const fn peak_steps(self) -> f64 {
        match self {
            Self::None => 0.0,
            Self::Rectangular => A_DRAW_REACHES_STEPS,
            Self::Triangular => A_DRAW_REACHES_STEPS + A_DRAW_REACHES_STEPS,
        }
    }

    const fn largest_error_steps(self) -> f64 {
        self.peak_steps() + ROUNDING_REACHES_STEPS
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NoiseShaping {
    #[default]
    None,
    Lipshitz,
    Threshold,
}

const LIPSHITZ_1992_E_WEIGHTED: [f64; 5] = [2.033, -2.165, 1.959, -1.590, 0.6149];
const LIPSHITZ_1992_TAPS: usize = LIPSHITZ_1992_E_WEIGHTED.len();
const LIPSHITZ_1992_DESIGN_RATES: [SampleRate; 2] = [SampleRate::HZ_44100, SampleRate::HZ_48000];

const THRESHOLD_ORDER: usize = 12;
const THRESHOLD_LAGS: usize = THRESHOLD_ORDER + 1;
const THRESHOLD_RANGE_DB: f64 = 42.0;
const THRESHOLD_HELD_BELOW_KHZ: f64 = 1.0;
const THRESHOLD_GRID_POINTS: usize = 2_048;
const POINTS_TOGETHER: usize = 4;
const POINT_GROUPS: usize = THRESHOLD_GRID_POINTS / POINTS_TOGETHER;

type Points = [f64; POINTS_TOGETHER];

const TERHARDT_1979_LOW_RISE_DB: f64 = 3.64;
const TERHARDT_1979_LOW_RISE_POWER: f64 = -0.8;
const TERHARDT_1979_RESONANCE_DB: f64 = 6.5;
const TERHARDT_1979_RESONANCE_KHZ: f64 = 3.3;
const TERHARDT_1979_RESONANCE_SHARPNESS: f64 = 0.6;
const TERHARDT_1979_HIGH_RISE_DB: f64 = 1e-3;
const TERHARDT_1979_HIGH_RISE_POWER: i32 = 4;

const DECIBELS_PER_BEL: f64 = 10.0;
const HERTZ_PER_KILOHERTZ: f64 = 1_000.0;

const EVERY_F64_IS_WHOLE_FROM: f64 = (1_u64 << (f64::MANTISSA_DIGITS - 1)) as f64;
const ROUNDS_A_SIGNED_F64_TO_WHOLE: f64 = 1.5 * EVERY_F64_IS_WHOLE_FROM;

const HISTORY_HELD_TWICE: usize = 2;

const DRAWN_BITS: u32 = f64::MANTISSA_DIGITS;
const UNITS_PER_DRAW: f64 = 1.0 / (1_u64 << DRAWN_BITS) as f64;
const A_DRAW_REACHES_STEPS: f64 = 0.5;
const ROUNDING_REACHES_STEPS: f64 = 0.5;

impl NoiseShaping {
    pub fn applies_at(self, rate: SampleRate) -> bool {
        match self {
            Self::None => false,
            Self::Lipshitz => LIPSHITZ_1992_DESIGN_RATES.contains(&rate),
            Self::Threshold => true,
        }
    }

    pub fn at(self, rate: SampleRate) -> Self {
        if self.applies_at(rate) {
            self
        } else {
            Self::None
        }
    }

    fn coefficients_at(self, rate: SampleRate) -> Vec<f64> {
        match self {
            Self::None => Vec::new(),
            Self::Lipshitz => LIPSHITZ_1992_E_WEIGHTED.to_vec(),
            Self::Threshold => threshold_coefficients(rate),
        }
    }
}

fn threshold_coefficients(rate: SampleRate) -> Vec<f64> {
    prediction_error_filter(&weighted_autocorrelation(rate))
        .iter()
        .skip(1)
        .map(|predicted| -predicted)
        .collect()
}

fn threshold_in_quiet(khz: f64) -> f64 {
    let from_resonance = khz - TERHARDT_1979_RESONANCE_KHZ;
    TERHARDT_1979_LOW_RISE_DB * khz.powf(TERHARDT_1979_LOW_RISE_POWER)
        - TERHARDT_1979_RESONANCE_DB
            * (-TERHARDT_1979_RESONANCE_SHARPNESS * from_resonance * from_resonance).exp()
        + TERHARDT_1979_HIGH_RISE_DB * khz.powi(TERHARDT_1979_HIGH_RISE_POWER)
}

fn power_ratio(decibels: f64) -> f64 {
    (decibels / DECIBELS_PER_BEL * LN_10).exp()
}

fn midpoint(group: usize, lane: usize) -> f64 {
    (group * POINTS_TOGETHER + lane) as f64 + 0.5
}

fn weighted_autocorrelation(rate: SampleRate) -> [f64; THRESHOLD_LAGS] {
    let nyquist_khz = f64::from(rate.hz()) / 2.0 / HERTZ_PER_KILOHERTZ;
    let spacing_khz = nyquist_khz / THRESHOLD_GRID_POINTS as f64;
    let held = threshold_in_quiet(THRESHOLD_HELD_BELOW_KHZ);
    let thresholds: [Points; POINT_GROUPS] = array::from_fn(|group| {
        array::from_fn(|lane| {
            let khz = midpoint(group, lane) * spacing_khz;
            if khz < THRESHOLD_HELD_BELOW_KHZ {
                held
            } else {
                threshold_in_quiet(khz)
            }
        })
    });
    let ceiling = thresholds
        .as_flattened()
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min)
        + THRESHOLD_RANGE_DB;

    let mut lags = [[0.0; POINTS_TOGETHER]; THRESHOLD_LAGS];
    for (group, thresholds) in thresholds.iter().enumerate() {
        let weights = thresholds.map(|threshold| power_ratio(-threshold.min(ceiling)));
        let cosines = array::from_fn(|lane| {
            (PI * midpoint(group, lane) / THRESHOLD_GRID_POINTS as f64).cos()
        });
        accumulate_lags(&mut lags, weights, cosines);
    }
    lags.map(|lanes| lanes.iter().sum::<f64>() / THRESHOLD_GRID_POINTS as f64)
}

fn accumulate_lags(lags: &mut [Points; THRESHOLD_LAGS], weights: Points, cosines: Points) {
    let (mut previous, mut current) = (cosines, [1.0; POINTS_TOGETHER]);
    for lag in lags {
        for ((sum, weight), cosine) in lag.iter_mut().zip(weights).zip(current) {
            *sum += weight * cosine;
        }
        let mut next = [0.0; POINTS_TOGETHER];
        for (((slot, first), now), before) in
            next.iter_mut().zip(cosines).zip(current).zip(previous)
        {
            *slot = 2.0 * first * now - before;
        }
        previous = current;
        current = next;
    }
}

fn prediction_error_filter(autocorrelation: &[f64; THRESHOLD_LAGS]) -> [f64; THRESHOLD_LAGS] {
    let mut filter = [0.0; THRESHOLD_LAGS];
    filter[0] = 1.0;
    let [mut error, ..] = *autocorrelation;

    for order in 1..THRESHOLD_LAGS {
        let correlation: f64 = filter
            .iter()
            .take(order)
            .zip(autocorrelation.iter().take(order + 1).skip(1).rev())
            .map(|(tap, lag)| tap * lag)
            .sum();
        let reflection = -correlation / error;
        let previous = filter;
        for ((tap, own), mirrored) in filter
            .iter_mut()
            .zip(previous)
            .zip(previous.iter().take(order + 1).rev())
        {
            *tap = own + reflection * mirrored;
        }
        error *= 1.0 - reflection * reflection;
    }
    filter
}

struct Lfsr(u64);

impl Lfsr {
    fn new(seed: u64) -> Self {
        Self(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    fn draw(&mut self, kind: DitherKind) -> f64 {
        match kind {
            DitherKind::None => 0.0,
            DitherKind::Rectangular => self.next_unit(),
            DitherKind::Triangular => self.next_unit() + self.next_unit(),
        }
    }

    fn next_unit(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> (u64::BITS - DRAWN_BITS)) as f64 * UNITS_PER_DRAW - A_DRAW_REACHES_STEPS
    }
}

#[derive(Clone, Copy)]
struct Grid {
    steps_per_unit: f64,
    step: f64,
}

impl Grid {
    fn of(target: BitDepth) -> Self {
        let steps_per_unit = f64::from(1_u32 << (target.bits().get() - 1));
        Self {
            steps_per_unit,
            step: 1.0 / steps_per_unit,
        }
    }

    fn steps(self, sample: f64) -> f64 {
        sample * self.steps_per_unit
    }

    fn sample(self, steps: f64) -> f64 {
        in_range(steps, self.steps_per_unit) * self.step
    }
}

fn on_the_grid(steps: f64) -> f64 {
    (steps + ROUNDS_A_SIGNED_F64_TO_WHOLE) - ROUNDS_A_SIGNED_F64_TO_WHOLE
}

fn in_range(steps: f64, steps_per_unit: f64) -> f64 {
    steps.clamp(-steps_per_unit, steps_per_unit - 1.0)
}

pub struct Dither {
    kind: DitherKind,
    shaping: NoiseShaping,
    applied: NoiseShaping,
    grid: Grid,
    channels: NonZeroUsize,
    noise: Lfsr,
    coefficients: Vec<f64>,
    history: Vec<f64>,
    newest_at: usize,
}

impl Dither {
    pub fn new(target: BitDepth, kind: DitherKind, shaping: NoiseShaping, seed: u64) -> Self {
        Self {
            kind,
            shaping,
            applied: NoiseShaping::None,
            grid: Grid::of(target),
            channels: NonZeroUsize::MIN,
            noise: Lfsr::new(seed),
            coefficients: Vec::new(),
            history: Vec::new(),
            newest_at: 0,
        }
    }

    pub const fn step(&self) -> f64 {
        self.grid.step
    }

    fn requantise(&mut self, input: &[f64], output: &mut [f64], samples: usize) {
        let grid = self.grid;

        for (sample, slot) in input.iter().zip(output.iter_mut()).take(samples) {
            let dithered = grid.steps(*sample) + self.noise.draw(self.kind);
            *slot = grid.sample(on_the_grid(dithered));
        }
    }

    fn requantise_shaped<const TAPS: usize>(
        &mut self,
        input: &[f64],
        output: &mut [f64],
        frames: usize,
    ) {
        let channels = self.channels.get();
        let grid = self.grid;
        let largest_error = self.kind.largest_error_steps();
        let Ok(&coefficients) = <&[f64; TAPS]>::try_from(self.coefficients.as_slice()) else {
            return;
        };
        let (rows, _) = self.history.as_chunks_mut::<TAPS>();
        let (histories, _) = rows.as_chunks_mut::<HISTORY_HELD_TWICE>();
        let frames_in = input.chunks_exact(channels);
        let frames_out = output.chunks_exact_mut(channels);
        let mut newest_at = self.newest_at;

        for (frame_in, frame_out) in frames_in.zip(frames_out).take(frames) {
            let written_at = newest_at.checked_sub(1).unwrap_or(TAPS - 1);
            let lanes = frame_in.iter().zip(frame_out.iter_mut());
            for ((sample, slot), held) in lanes.zip(histories.iter_mut()) {
                let errors = held.as_flattened_mut();
                let Some(recent) = errors.get(newest_at..newest_at + TAPS) else {
                    continue;
                };
                let mut feedback = 0.0;
                for (coefficient, error) in coefficients.iter().zip(recent).rev() {
                    feedback += coefficient * error;
                }

                let shaped = grid.steps(*sample) + feedback;
                let quantised = on_the_grid(shaped + self.noise.draw(self.kind));

                let error = shaped - quantised;
                let fed_back = if error.abs() <= largest_error {
                    error
                } else {
                    0.0
                };
                for copy in [written_at, written_at + TAPS] {
                    if let Some(held) = errors.get_mut(copy) {
                        *held = fed_back;
                    }
                }
                *slot = grid.sample(quantised);
            }
            newest_at = written_at;
        }
        self.newest_at = newest_at;
    }
}

impl Processor for Dither {
    fn prepare(&mut self, spec: StreamSpec, max_frames_in: usize) -> Result<usize> {
        self.channels =
            NonZeroUsize::new(usize::from(spec.channel_count().get())).unwrap_or(NonZeroUsize::MIN);
        self.applied = self.shaping.at(spec.rate);
        self.coefficients = self.applied.coefficients_at(spec.rate);
        self.history =
            vec![0.0; self.channels.get() * HISTORY_HELD_TWICE * self.coefficients.len()];
        self.newest_at = 0;
        Ok(max_frames_in)
    }

    fn reset(&mut self) {
        self.history.fill(0.0);
    }

    fn latency_frames(&self) -> f64 {
        0.0
    }

    fn is_transparent(&self) -> bool {
        false
    }

    fn process(&mut self, input: &[f64], output: &mut [f64]) -> ProcessCount {
        let channels = self.channels.get();
        let frames = (input.len() / channels).min(output.len() / channels);

        match self.applied {
            NoiseShaping::None => self.requantise(input, output, frames * channels),
            NoiseShaping::Lipshitz => {
                self.requantise_shaped::<LIPSHITZ_1992_TAPS>(input, output, frames);
            }
            NoiseShaping::Threshold => {
                self.requantise_shaped::<THRESHOLD_ORDER>(input, output, frames);
            }
        }

        ProcessCount {
            frames_in: frames,
            frames_out: frames,
        }
    }

    fn flush(&mut self, _output: &mut [f64]) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use resonate_core::{ChannelLayout, SampleData, SampleFormat, SampleRate};

    use super::*;

    const SEED: u64 = 0x5265_736F_6E61_7465;
    const SHAPED_CURVES: [NoiseShaping; 2] = [NoiseShaping::Lipshitz, NoiseShaping::Threshold];

    const EVALUATION_POINTS: usize = 8_192;
    const LOWEST_EVALUATED_HZ: f64 = 1.0;
    const AUDIBLE_FROM_HZ: f64 = 20.0;
    const AUDIBLE_TO_HZ: f64 = 20_000.0;
    const PEAK_CEILING_DB: f64 = 30.0;
    const HIGH_RATES_QUIETER_THAN_FLAT_BY_DB: f64 = 20.0;
    const PROTOTYPE_AGREES_WITHIN: f64 = 1e-9;

    const PROTOTYPE_AT_44_1_KHZ: [f64; THRESHOLD_ORDER] = [
        2.585357311322852,
        -3.395101376526047,
        2.451720223848863,
        -0.5216001338122793,
        -0.9324211142317529,
        0.9595467346799502,
        -0.16568129500837298,
        -0.4888253438954888,
        0.5014571676700149,
        -0.18400739334173832,
        -0.026394027311332974,
        0.03846863711108699,
    ];
    const PROTOTYPE_AT_96_KHZ: [f64; THRESHOLD_ORDER] = [
        1.81945291267273,
        -0.3702732298113269,
        -0.7112710306334691,
        -0.21327061130027705,
        0.2678825131191851,
        0.3382456390047739,
        0.07858707586704027,
        -0.18718351206466813,
        -0.20212311009142497,
        0.017880193585524385,
        0.18507279648406733,
        -0.07537324287094997,
    ];

    const QUIET_SINE_STEPS: f64 = 3.3;
    const QUIET_SINE_HZ: f64 = 441.0;
    const MEASURED_BLOCK: usize = 4_096;
    const MEASURED_BLOCKS: usize = 32;
    const NEAR_NYQUIST_HZ: f64 = 40_000.0;
    const MIDRANGE_QUIETER_BY_DB: f64 = 10.0;
    const NEAR_NYQUIST_LOUDER_BY_DB: f64 = 3.0;

    const CLIPPING_SINE_FULL_SCALES: f64 = 1.5;
    const CLIPPING_SINE_HZ: f64 = 997.0;

    const WILD_SAMPLES: [f64; 4] = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e30];
    const WILD_STREAM: usize = 4_096;
    const WILD_AT: usize = WILD_STREAM / 2 + 5;
    const EVERY_KIND: [DitherKind; 3] = [
        DitherKind::None,
        DitherKind::Rectangular,
        DitherKind::Triangular,
    ];

    const UNBIASED_WITHIN_STEPS: f64 = 0.02;
    const BIAS_SAMPLES: usize = 1 << 17;

    fn rate(hz: u32) -> SampleRate {
        SampleRate::new(hz).expect("a rate in range")
    }

    fn every_rate() -> [SampleRate; 13] {
        [
            SampleRate::HZ_8000,
            rate(11_025),
            rate(16_000),
            SampleRate::HZ_22050,
            rate(32_000),
            SampleRate::HZ_44100,
            SampleRate::HZ_48000,
            SampleRate::HZ_88200,
            SampleRate::HZ_96000,
            SampleRate::HZ_176400,
            SampleRate::HZ_192000,
            SampleRate::HZ_352800,
            SampleRate::HZ_384000,
        ]
    }

    fn prepared(target: BitDepth, kind: DitherKind, shaping: NoiseShaping) -> Dither {
        prepared_at(SampleRate::HZ_44100, target, kind, shaping)
    }

    fn prepared_at(
        rate: SampleRate,
        target: BitDepth,
        kind: DitherKind,
        shaping: NoiseShaping,
    ) -> Dither {
        let mut stage = Dither::new(target, kind, shaping, SEED);
        stage
            .prepare(
                StreamSpec::new(rate, ChannelLayout::Mono, SampleFormat::F32),
                4_096,
            )
            .expect("dither always prepares");
        stage
    }

    fn ramp(frames: usize) -> Vec<f64> {
        (0..frames)
            .map(|n| (n as f64 / frames as f64) * 1.8 - 0.9)
            .collect()
    }

    fn sine(rate: SampleRate, hz: f64, amplitude: f64, frames: usize) -> Vec<f64> {
        let cycles_per_sample = hz / f64::from(rate.hz());
        (0..frames)
            .map(|n| amplitude * (TAU * cycles_per_sample * n as f64).sin())
            .collect()
    }

    fn decibels(power_ratio: f64) -> f64 {
        DECIBELS_PER_BEL * power_ratio.log10()
    }

    struct Figures {
        weighted_db: f64,
        peak_db: f64,
    }

    fn noise_power_gain(coefficients: &[f64], cycles_per_sample: f64) -> f64 {
        let (mut real, mut imaginary) = (1.0, 0.0);
        for (delay, coefficient) in (1..).zip(coefficients) {
            let angle = TAU * cycles_per_sample * f64::from(delay);
            real -= coefficient * angle.cos();
            imaginary += coefficient * angle.sin();
        }
        real * real + imaginary * imaginary
    }

    fn figures(coefficients: &[f64], rate: SampleRate) -> Figures {
        let hz = f64::from(rate.hz());
        let nyquist = hz / 2.0;
        let audible = AUDIBLE_FROM_HZ..=AUDIBLE_TO_HZ.min(nyquist);
        let (mut weighted, mut weights, mut peak) = (0.0, 0.0, 0.0_f64);

        for point in 0..EVALUATION_POINTS {
            let along = point as f64 / (EVALUATION_POINTS - 1) as f64;
            let frequency = LOWEST_EVALUATED_HZ + (nyquist - LOWEST_EVALUATED_HZ) * along;
            let gain = noise_power_gain(coefficients, frequency / hz);
            peak = peak.max(gain);
            if audible.contains(&frequency) {
                let threshold = threshold_in_quiet(frequency / HERTZ_PER_KILOHERTZ);
                let weight = 10.0_f64.powf(-threshold / DECIBELS_PER_BEL);
                weighted += weight * gain;
                weights += weight;
            }
        }

        Figures {
            weighted_db: decibels(weighted / weights),
            peak_db: decibels(peak),
        }
    }

    fn reflections(coefficients: &[f64]) -> Vec<f64> {
        let mut filter: Vec<f64> = coefficients
            .iter()
            .map(|coefficient| -coefficient)
            .collect();
        let mut reflections = Vec::new();
        while let Some(&last) = filter.last() {
            reflections.push(last);
            let order = filter.len();
            let scale = 1.0 - last * last;
            filter = (0..order - 1)
                .map(|tap| (filter[tap] - last * filter[order - 2 - tap]) / scale)
                .collect();
        }
        reflections
    }

    fn goertzel_power(samples: &[f64], cycles_per_sample: f64) -> f64 {
        let coefficient = 2.0 * (TAU * cycles_per_sample).cos();
        let length = samples.len() as f64;
        let (mut newer, mut older) = (0.0, 0.0);
        for (n, sample) in samples.iter().enumerate() {
            let hann = 0.5 - 0.5 * (TAU * n as f64 / length).cos();
            let next = sample * hann + coefficient * newer - older;
            older = newer;
            newer = next;
        }
        newer * newer + older * older - coefficient * newer * older
    }

    fn error_power_at(rate: SampleRate, shaping: NoiseShaping, hz: f64) -> f64 {
        let mut stage = prepared_at(rate, BitDepth::Bits16, DitherKind::Triangular, shaping);
        let step = stage.step();
        let input = sine(
            rate,
            QUIET_SINE_HZ,
            QUIET_SINE_STEPS * step,
            MEASURED_BLOCK * MEASURED_BLOCKS,
        );
        let mut output = vec![0.0; input.len()];
        stage.process(&input, &mut output);

        let error: Vec<f64> = output
            .iter()
            .zip(&input)
            .map(|(out, raw)| (*out - *raw) / step)
            .collect();
        let cycles_per_sample = hz / f64::from(rate.hz());
        let (blocks, _) = error.as_chunks::<MEASURED_BLOCK>();
        blocks
            .iter()
            .map(|block| goertzel_power(block, cycles_per_sample))
            .sum::<f64>()
            / MEASURED_BLOCKS as f64
    }

    #[test]
    fn output_lands_exactly_on_the_target_lsb_grid() {
        let mut stage = prepared(BitDepth::Bits16, DitherKind::Triangular, NoiseShaping::None);
        let input = ramp(4_096);
        let mut output = vec![0.0; input.len()];
        stage.process(&input, &mut output);

        let step = stage.step();
        for sample in output {
            let steps = sample / step;
            assert_eq!(steps, steps.round(), "{sample} is {steps} steps");
        }
    }

    #[test]
    fn undithered_half_steps_round_to_the_even_neighbour() {
        let mut stage = prepared(BitDepth::Bits16, DitherKind::None, NoiseShaping::None);
        let step = stage.step();
        let halves = [0.5, 1.5, 2.5, 3.5, -0.5, -1.5, -2.5, -3.5];
        let input: Vec<f64> = halves.iter().map(|half| half * step).collect();
        let mut output = vec![0.0; input.len()];
        stage.process(&input, &mut output);

        let evens: Vec<f64> = output.iter().map(|sample| *sample / step).collect();
        assert_eq!(evens, [0.0, 2.0, 2.0, 4.0, 0.0, -2.0, -2.0, -4.0]);
    }

    #[test]
    fn a_24_bit_word_near_full_scale_is_dithered_without_bias() {
        let mut stage = prepared(BitDepth::Bits24, DitherKind::Triangular, NoiseShaping::None);
        let step = stage.step();
        let between_two_steps = ((0.9 / step).floor() + 0.5) * step;
        let input = vec![between_two_steps; BIAS_SAMPLES];
        assert_eq!(input[0], between_two_steps);
        let mut output = vec![0.0; input.len()];
        stage.process(&input, &mut output);

        let bias = output
            .iter()
            .map(|sample| (*sample - between_two_steps) / step)
            .sum::<f64>()
            / BIAS_SAMPLES as f64;
        assert!(
            bias.abs() < UNBIASED_WITHIN_STEPS,
            "the dither left a bias of {bias} steps"
        );
    }

    #[test]
    fn undithered_quantisation_is_deterministic_and_dither_is_not() {
        let input = ramp(1_024);
        let mut plain = vec![0.0; input.len()];
        prepared(BitDepth::Bits16, DitherKind::None, NoiseShaping::None)
            .process(&input, &mut plain);

        let mut again = vec![0.0; input.len()];
        prepared(BitDepth::Bits16, DitherKind::None, NoiseShaping::None)
            .process(&input, &mut again);
        assert_eq!(plain, again);

        let mut dithered = vec![0.0; input.len()];
        prepared(BitDepth::Bits16, DitherKind::Triangular, NoiseShaping::None)
            .process(&input, &mut dithered);
        assert_ne!(plain, dithered);
    }

    #[test]
    fn dither_error_stays_within_a_couple_of_least_significant_bits() {
        let mut stage = prepared(BitDepth::Bits16, DitherKind::Triangular, NoiseShaping::None);
        let input = ramp(4_096);
        let mut output = vec![0.0; input.len()];
        stage.process(&input, &mut output);

        let step = stage.step();
        for (sample, quantised) in input.iter().zip(&output) {
            let error = (*sample - *quantised).abs() / step;
            assert!(error < 2.0, "error of {error} LSB at {sample}");
        }
    }

    #[test]
    fn a_curve_runs_only_at_the_rates_it_was_designed_for() {
        for rate in [SampleRate::HZ_44100, SampleRate::HZ_48000] {
            assert!(NoiseShaping::Lipshitz.applies_at(rate), "{rate}");
            assert_eq!(NoiseShaping::Lipshitz.at(rate), NoiseShaping::Lipshitz);
        }
        for rate in [
            SampleRate::HZ_88200,
            SampleRate::HZ_96000,
            SampleRate::HZ_192000,
        ] {
            assert!(!NoiseShaping::Lipshitz.applies_at(rate), "{rate}");
            assert_eq!(NoiseShaping::Lipshitz.at(rate), NoiseShaping::None);
        }
        for rate in every_rate() {
            assert!(NoiseShaping::Threshold.applies_at(rate), "{rate}");
            assert_eq!(NoiseShaping::Threshold.at(rate), NoiseShaping::Threshold);
            assert!(!NoiseShaping::None.applies_at(rate), "{rate}");
        }
    }

    #[test]
    fn a_rate_the_curve_was_not_designed_for_is_dithered_flat_instead() {
        let input = ramp(4_096);

        let mut shaped = vec![0.0; input.len()];
        prepared_at(
            SampleRate::HZ_96000,
            BitDepth::Bits16,
            DitherKind::Triangular,
            NoiseShaping::Lipshitz,
        )
        .process(&input, &mut shaped);

        let mut flat = vec![0.0; input.len()];
        prepared_at(
            SampleRate::HZ_96000,
            BitDepth::Bits16,
            DitherKind::Triangular,
            NoiseShaping::None,
        )
        .process(&input, &mut flat);
        assert_eq!(shaped, flat, "a curve designed for 44.1 kHz ran at 96 kHz");

        let mut at_design_rate = vec![0.0; input.len()];
        prepared(
            BitDepth::Bits16,
            DitherKind::Triangular,
            NoiseShaping::Lipshitz,
        )
        .process(&input, &mut at_design_rate);
        assert_ne!(
            at_design_rate, flat,
            "the curve did not run at the rate it was designed for"
        );
    }

    #[test]
    fn noise_shaping_pushes_error_out_of_the_audible_band() {
        let input = ramp(8_192);

        let mut flat = vec![0.0; input.len()];
        let mut flat_stage = prepared(BitDepth::Bits16, DitherKind::Triangular, NoiseShaping::None);
        let step = flat_stage.step();
        flat_stage.process(&input, &mut flat);

        let energy = |samples: &[f64]| -> f64 {
            samples
                .iter()
                .zip(&input)
                .map(|(out, raw)| ((*out - *raw) / step).powi(2))
                .sum::<f64>()
        };

        for shaping in SHAPED_CURVES {
            let mut shaped = vec![0.0; input.len()];
            prepared(BitDepth::Bits16, DitherKind::Triangular, shaping)
                .process(&input, &mut shaped);

            let shaping_trades_total_power_for_a_quieter_audible_band =
                energy(&shaped) > energy(&flat);
            assert!(
                shaping_trades_total_power_for_a_quieter_audible_band,
                "{shaping:?} did not redistribute error energy"
            );
            assert_ne!(flat, shaped);
        }
    }

    #[test]
    fn a_shaped_channels_error_never_reaches_the_channel_beside_it() {
        for shaping in SHAPED_CURVES {
            let mut stage = Dither::new(BitDepth::Bits16, DitherKind::None, shaping, SEED);
            stage
                .prepare(
                    StreamSpec::new(
                        SampleRate::HZ_44100,
                        ChannelLayout::Stereo,
                        SampleFormat::F32,
                    ),
                    4_096,
                )
                .expect("dither always prepares");

            let loud = ramp(2_048);
            let mut input = vec![0.0; loud.len() * 2];
            for (frame, sample) in loud.iter().enumerate() {
                if let Some(slot) = input.get_mut(frame * 2) {
                    *slot = *sample;
                }
            }
            let mut output = vec![0.0; input.len()];
            stage.process(&input, &mut output);

            let (frames, _) = output.as_chunks::<2>();
            for (frame, [_, right]) in frames.iter().enumerate() {
                assert_eq!(
                    *right, 0.0,
                    "{shaping:?}: silence picked up the other channel's error at frame {frame}"
                );
            }
            assert!(
                frames.iter().any(|[left, _]| *left != 0.0),
                "{shaping:?}: the shaped channel produced nothing to leak"
            );
        }
    }

    #[test]
    fn the_designed_curve_is_the_prototypes_at_44_1_and_96_khz() {
        for (rate, prototype) in [
            (SampleRate::HZ_44100, PROTOTYPE_AT_44_1_KHZ),
            (SampleRate::HZ_96000, PROTOTYPE_AT_96_KHZ),
        ] {
            let designed = threshold_coefficients(rate);
            assert_eq!(designed.len(), THRESHOLD_ORDER);
            for (tap, (ours, theirs)) in designed.iter().zip(prototype).enumerate() {
                assert!(
                    (ours - theirs).abs() < PROTOTYPE_AGREES_WITHIN,
                    "{rate} tap {tap}: {ours} against the prototype's {theirs}"
                );
            }
        }
    }

    #[test]
    fn the_threshold_curve_is_at_least_as_quiet_as_lipshitz_at_lipshitz_s_own_rates() {
        for rate in LIPSHITZ_1992_DESIGN_RATES {
            let lipshitz = figures(&LIPSHITZ_1992_E_WEIGHTED, rate);
            let threshold = figures(&threshold_coefficients(rate), rate);
            assert!(
                threshold.weighted_db <= lipshitz.weighted_db,
                "{rate}: threshold {:.1} dB against lipshitz {:.1} dB",
                threshold.weighted_db,
                lipshitz.weighted_db
            );
        }
    }

    #[test]
    fn the_threshold_curve_is_twenty_db_quieter_than_flat_at_the_high_rates() {
        for rate in [
            SampleRate::HZ_88200,
            SampleRate::HZ_96000,
            SampleRate::HZ_176400,
            SampleRate::HZ_192000,
        ] {
            let threshold = figures(&threshold_coefficients(rate), rate);
            assert!(
                threshold.weighted_db <= -HIGH_RATES_QUIETER_THAN_FLAT_BY_DB,
                "{rate}: only {:.1} dB against flat",
                threshold.weighted_db
            );
        }
    }

    #[test]
    fn no_designed_curve_lifts_the_noise_past_its_ceiling_at_any_rate() {
        for rate in every_rate() {
            let threshold = figures(&threshold_coefficients(rate), rate);
            assert!(
                threshold.peak_db < PEAK_CEILING_DB,
                "{rate}: the curve peaks at {:.1} dB",
                threshold.peak_db
            );
        }
    }

    #[test]
    fn every_designed_curve_is_minimum_phase() {
        for rate in every_rate() {
            for (order, reflection) in reflections(&threshold_coefficients(rate))
                .iter()
                .enumerate()
            {
                assert!(
                    reflection.abs() < 1.0,
                    "{rate}: reflection {order} is {reflection}"
                );
            }
        }
    }

    #[test]
    fn threshold_shaping_moves_a_quiet_sines_error_from_the_midrange_to_near_nyquist() {
        let rate = SampleRate::HZ_96000;
        let against_flat = |hz: f64| {
            decibels(
                error_power_at(rate, NoiseShaping::Threshold, hz)
                    / error_power_at(rate, NoiseShaping::None, hz),
            )
        };

        for hz in [1_000.0, 4_000.0] {
            let moved = against_flat(hz);
            assert!(
                moved < -MIDRANGE_QUIETER_BY_DB,
                "{hz} Hz is only {moved:.1} dB against flat"
            );
        }
        let moved = against_flat(NEAR_NYQUIST_HZ);
        assert!(
            moved > NEAR_NYQUIST_LOUDER_BY_DB,
            "{NEAR_NYQUIST_HZ} Hz is {moved:.1} dB against flat"
        );
    }

    #[test]
    fn a_clipping_sine_stays_on_the_grid_and_in_range_and_its_fed_back_error_stays_bounded() {
        for rate in [SampleRate::HZ_44100, SampleRate::HZ_96000] {
            let mut stage = prepared_at(
                rate,
                BitDepth::Bits16,
                DitherKind::Triangular,
                NoiseShaping::Threshold,
            );
            let step = stage.step();
            let input = sine(
                rate,
                CLIPPING_SINE_HZ,
                CLIPPING_SINE_FULL_SCALES,
                rate.hz() as usize,
            );
            let mut output = vec![0.0; input.len()];

            for (block, landed) in input
                .chunks(THRESHOLD_ORDER)
                .zip(output.chunks_mut(THRESHOLD_ORDER))
            {
                stage.process(block, landed);
                for error in &stage.history {
                    assert!(
                        error.abs() <= DitherKind::Triangular.largest_error_steps(),
                        "{rate}: fed back {error} steps"
                    );
                }
            }

            for sample in output {
                let steps = sample / step;
                assert_eq!(steps, steps.round(), "{rate}: {sample} is off the grid");
                assert!(
                    (-1.0..=1.0 - step).contains(&sample),
                    "{rate}: {sample} is out of range"
                );
            }
        }
    }

    fn unguarded_error_feedback(
        rate: SampleRate,
        kind: DitherKind,
        shaping: NoiseShaping,
        input: &[f64],
    ) -> Vec<f64> {
        let coefficients = shaping.at(rate).coefficients_at(rate);
        let grid = Grid::of(BitDepth::Bits16);
        let mut noise = Lfsr::new(SEED);
        let mut errors = vec![0.0; coefficients.len()];

        input
            .iter()
            .map(|sample| {
                let feedback = coefficients
                    .iter()
                    .zip(&errors)
                    .rev()
                    .fold(0.0, |sum, (coefficient, error)| sum + coefficient * error);
                let shaped = grid.steps(*sample) + feedback;
                let quantised = on_the_grid(shaped + noise.draw(kind));
                errors.rotate_right(1);
                if let Some(newest) = errors.first_mut() {
                    *newest = shaped - quantised;
                }
                grid.sample(quantised)
            })
            .collect()
    }

    fn shaped_streams() -> [(NoiseShaping, SampleRate); 3] {
        [
            (NoiseShaping::Threshold, SampleRate::HZ_44100),
            (NoiseShaping::Threshold, SampleRate::HZ_96000),
            (NoiseShaping::Lipshitz, SampleRate::HZ_44100),
        ]
    }

    #[test]
    fn an_error_is_fed_back_whole_wherever_the_input_is_a_real_sample() {
        for (shaping, rate) in shaped_streams() {
            for kind in EVERY_KIND {
                let input = sine(
                    rate,
                    CLIPPING_SINE_HZ,
                    CLIPPING_SINE_FULL_SCALES,
                    rate.hz() as usize,
                );
                let mut output = vec![0.0; input.len()];
                prepared_at(rate, BitDepth::Bits16, kind, shaping).process(&input, &mut output);

                assert!(
                    output == unguarded_error_feedback(rate, kind, shaping, &input),
                    "{shaping:?} at {rate} under {kind:?} dropped an error a real sample made"
                );
            }
        }
    }

    #[test]
    fn one_wild_sample_is_forgotten_rather_than_fed_back_for_the_rest_of_the_stream() {
        let reach = DitherKind::Triangular.largest_error_steps();

        for (shaping, rate) in shaped_streams() {
            let step = prepared_at(rate, BitDepth::Bits16, DitherKind::Triangular, shaping).step();
            let quiet = sine(rate, QUIET_SINE_HZ, QUIET_SINE_STEPS * step, WILD_STREAM);
            let mut clean = vec![0.0; quiet.len()];
            prepared_at(rate, BitDepth::Bits16, DitherKind::Triangular, shaping)
                .process(&quiet, &mut clean);

            let streams = WILD_SAMPLES.map(|wild| {
                let mut input = quiet.clone();
                input[WILD_AT] = wild;
                let mut stage =
                    prepared_at(rate, BitDepth::Bits16, DitherKind::Triangular, shaping);
                let mut output = vec![0.0; input.len()];
                for (block, landed) in input
                    .chunks(THRESHOLD_ORDER)
                    .zip(output.chunks_mut(THRESHOLD_ORDER))
                {
                    stage.process(block, landed);
                    for error in &stage.history {
                        assert!(
                            error.abs() <= reach,
                            "{shaping:?} at {rate}: {wild} left {error} steps to feed back"
                        );
                    }
                }
                output
            });

            for (wild, output) in WILD_SAMPLES.iter().zip(&streams) {
                assert_eq!(output[..WILD_AT], clean[..WILD_AT], "{wild} reached back");
                for sample in &output[WILD_AT + 1..] {
                    let sample = *sample;
                    let steps = sample / step;
                    assert!(sample.is_finite(), "{wild} rode the feedback to {sample}");
                    assert_eq!(steps, steps.round(), "{wild}: {sample} is off the grid");
                    assert!(
                        (-1.0..=1.0 - step).contains(&sample),
                        "{wild}: {sample} is out of range"
                    );
                }
                assert!(
                    output[WILD_AT + 1..] == streams[0][WILD_AT + 1..],
                    "{shaping:?} at {rate}: {wild} was not forgotten the way a NaN is"
                );
            }

            let [not_a_number, above, below, huge] = streams.map(|output| output[WILD_AT]);
            let mut word = SampleData::S16(vec![i16::MAX]);
            word.write_f64(&[not_a_number]);
            assert_eq!(
                word,
                SampleData::S16(vec![0]),
                "a NaN the stage passes on is silence once the conversion reads it"
            );
            assert_eq!(above, 1.0 - step);
            assert_eq!(below, -1.0);
            assert_eq!(huge, 1.0 - step);
        }
    }

    #[test]
    fn dither_is_never_transparent_because_it_always_requantises() {
        let stage = prepared(BitDepth::Bits16, DitherKind::None, NoiseShaping::None);
        assert!(!stage.is_transparent());
    }

    #[test]
    fn a_32_bit_word_is_dithered_on_its_own_grid_without_bias() {
        let mut stage = prepared(BitDepth::Bits32, DitherKind::Triangular, NoiseShaping::None);
        let step = stage.step();
        assert_eq!(step, 1.0 / 2_147_483_648.0);
        let between_two_steps = ((0.9 / step).floor() + 0.5) * step;
        let input = vec![between_two_steps; BIAS_SAMPLES];
        let mut output = vec![0.0; input.len()];
        stage.process(&input, &mut output);

        for sample in &output {
            let steps = sample / step;
            assert_eq!(steps, steps.round(), "{sample} is {steps} steps");
        }
        let bias = output
            .iter()
            .map(|sample| (sample - between_two_steps) / step)
            .sum::<f64>()
            / BIAS_SAMPLES as f64;
        assert!(
            bias.abs() < UNBIASED_WITHIN_STEPS,
            "the dither left a bias of {bias} steps"
        );

        let mut word = SampleData::S32(vec![0; output.len()]);
        word.write_f64(&output);
        let SampleData::S32(words) = word else {
            panic!("the word changed its format");
        };
        for (word, sample) in words.iter().zip(&output) {
            assert_eq!(
                f64::from(*word) * step,
                *sample,
                "the conversion moved a grid value"
            );
        }
    }

    #[test]
    fn a_narrower_target_uses_a_coarser_grid() {
        let wide = prepared(BitDepth::Bits24, DitherKind::None, NoiseShaping::None);
        let narrow = prepared(BitDepth::Bits16, DitherKind::None, NoiseShaping::None);

        assert!(narrow.step() > wide.step());
        assert_eq!(narrow.step() / wide.step(), 256.0);
    }
}
