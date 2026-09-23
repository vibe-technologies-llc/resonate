use std::{f64::consts::PI, num::NonZeroUsize};

use resonate_codec::Placement;
use resonate_dsp::TruePeakMeter;

const SHELF_HZ: f64 = 1_681.974_450_955_533;
const SHELF_GAIN_DB: f64 = 3.999_843_853_973_347;
const SHELF_Q: f64 = 0.707_175_236_955_419_6;
const SHELF_BAND_EXPONENT: f64 = 0.499_666_774_154_541_6;
const HIGH_PASS_HZ: f64 = 38.135_470_876_024_44;
const HIGH_PASS_Q: f64 = 0.500_327_037_323_877_3;

const LOUDNESS_OFFSET_DB: f64 = -0.691;
const ABSOLUTE_GATE_LUFS: f64 = -70.0;
const RELATIVE_GATE_LU: f64 = -10.0;
const STEPS_PER_SECOND: u32 = 10;
const STEPS_PER_BLOCK: usize = 4;
const STEPS_PER_SHORT_TERM: usize = 30;
const RANGE_RELATIVE_GATE_LU: f64 = -20.0;
const RANGE_LOW_PERCENTILE: f64 = 0.10;
const RANGE_HIGH_PERCENTILE: f64 = 0.95;

const SURROUND_WEIGHT: f64 = 1.41;
const UNWEIGHTED: f64 = 1.0;

const DR_BLOCK_SECONDS: u32 = 3;
const DR_LOUDEST_SHARE: f64 = 0.2;
const DR_RMS_SCALE: f64 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Loudness {
    pub integrated: Option<f32>,
    pub range: Option<f32>,
    pub momentary_max: Option<f32>,
    pub short_term_max: Option<f32>,
    pub dynamic_range: Option<u8>,
    pub true_peak: f32,
}

#[derive(Clone, Copy, Debug)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn shelf(rate: f64) -> Self {
        let k = (PI * SHELF_HZ / rate).tan();
        let vh = 10_f64.powf(SHELF_GAIN_DB / 20.0);
        let vb = vh.powf(SHELF_BAND_EXPONENT);
        let a0 = 1.0 + k / SHELF_Q + k * k;
        Self::normalised(
            [
                (vh + vb * k / SHELF_Q + k * k) / a0,
                2.0 * (k * k - vh) / a0,
                (vh - vb * k / SHELF_Q + k * k) / a0,
            ],
            [2.0 * (k * k - 1.0) / a0, (1.0 - k / SHELF_Q + k * k) / a0],
        )
    }

    fn high_pass(rate: f64) -> Self {
        let k = (PI * HIGH_PASS_HZ / rate).tan();
        let a0 = 1.0 + k / HIGH_PASS_Q + k * k;
        Self::normalised(
            [1.0, -2.0, 1.0],
            [
                2.0 * (k * k - 1.0) / a0,
                (1.0 - k / HIGH_PASS_Q + k * k) / a0,
            ],
        )
    }

    const fn normalised([b0, b1, b2]: [f64; 3], [a1, a2]: [f64; 2]) -> Self {
        Self {
            b0,
            b1,
            b2,
            a1,
            a2,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn run(&mut self, input: f64) -> f64 {
        let output = self.b0 * input + self.z1;
        self.z1 = self.b1 * input - self.a1 * output + self.z2;
        self.z2 = self.b2 * input - self.a2 * output;
        output
    }
}

#[derive(Clone, Copy, Debug)]
struct Weighting {
    shelf: Biquad,
    high_pass: Biquad,
}

#[derive(Clone, Copy, Debug, Default)]
struct DrBlock {
    squares: f64,
    peak: f64,
}

pub(crate) struct Metering {
    channels: usize,
    weighting: Vec<Weighting>,
    weights: Vec<f64>,
    step_frames: u64,
    in_step: u64,
    step_squares: f64,
    steps: Vec<f64>,
    dr_frames: u64,
    in_dr_block: u64,
    dr_open: Vec<DrBlock>,
    dr_closed: Vec<Vec<DrBlock>>,
    peaking: TruePeakMeter,
}

impl Metering {
    pub(crate) fn new(placements: &[Placement], rate: u32) -> Self {
        let channels = placements.len().max(1);
        let hz = f64::from(rate);
        Self {
            channels,
            weights: weights_of(placements, channels),
            weighting: vec![
                Weighting {
                    shelf: Biquad::shelf(hz),
                    high_pass: Biquad::high_pass(hz),
                };
                channels
            ],
            step_frames: u64::from((rate / STEPS_PER_SECOND).max(1)),
            in_step: 0,
            step_squares: 0.0,
            steps: Vec::new(),
            dr_frames: u64::from(rate * DR_BLOCK_SECONDS).max(1),
            in_dr_block: 0,
            dr_open: vec![DrBlock::default(); channels],
            dr_closed: vec![Vec::new(); channels],
            peaking: TruePeakMeter::new(NonZeroUsize::new(channels).unwrap_or(NonZeroUsize::MIN)),
        }
    }

    pub(crate) fn note(&mut self, interleaved: &[f32]) {
        self.peaking.note(interleaved);
        for frame in interleaved.chunks_exact(self.channels) {
            for (((sample, weighting), weight), block) in frame
                .iter()
                .zip(&mut self.weighting)
                .zip(&self.weights)
                .zip(&mut self.dr_open)
            {
                let sample = f64::from(*sample);
                let weighted = weighting.high_pass.run(weighting.shelf.run(sample));
                self.step_squares += weight * weighted * weighted;
                block.squares += sample * sample;
                block.peak = block.peak.max(sample.abs());
            }
            self.in_step += 1;
            if self.in_step == self.step_frames {
                self.steps.push(self.step_squares / self.step_frames as f64);
                self.step_squares = 0.0;
                self.in_step = 0;
            }
            self.in_dr_block += 1;
            if self.in_dr_block == self.dr_frames {
                self.close_a_dr_block();
            }
        }
    }

    fn close_a_dr_block(&mut self) {
        for (open, closed) in self.dr_open.iter_mut().zip(&mut self.dr_closed) {
            closed.push(DrBlock {
                squares: open.squares / self.in_dr_block as f64,
                peak: open.peak,
            });
            *open = DrBlock::default();
        }
        self.in_dr_block = 0;
    }

    pub(crate) fn finished(mut self) -> Loudness {
        let no_whole_block = self.dr_closed.first().is_none_or(Vec::is_empty);
        if self.in_dr_block > 0 && no_whole_block {
            self.close_a_dr_block();
        }

        let momentary = averaged(&self.steps, STEPS_PER_BLOCK);
        let short_term = averaged(&self.steps, STEPS_PER_SHORT_TERM);
        Loudness {
            integrated: integrated(&momentary),
            range: range(&short_term),
            momentary_max: loudest(&momentary),
            short_term_max: loudest(&short_term),
            dynamic_range: dynamic_range(&self.dr_closed),
            true_peak: self.peaking.finish() as f32,
        }
    }
}

fn weights_of(placements: &[Placement], channels: usize) -> Vec<f64> {
    let beside = placements.contains(&Placement::Side);
    let mut weights: Vec<f64> = placements
        .iter()
        .map(|placement| match placement {
            Placement::Lfe => 0.0,
            Placement::Side => SURROUND_WEIGHT,
            Placement::Rear if !beside => SURROUND_WEIGHT,
            Placement::Front | Placement::Rear | Placement::Raised => UNWEIGHTED,
        })
        .collect();
    weights.resize(channels, UNWEIGHTED);
    weights
}

fn loudness_of(mean_square: f64) -> f64 {
    LOUDNESS_OFFSET_DB + 10.0 * mean_square.log10()
}

fn averaged(steps: &[f64], width: usize) -> Vec<f64> {
    steps
        .windows(width)
        .map(|window| window.iter().sum::<f64>() / width as f64)
        .collect()
}

fn audible(blocks: &[f64]) -> Vec<f64> {
    blocks
        .iter()
        .copied()
        .filter(|block| *block > 0.0 && loudness_of(*block) > ABSOLUTE_GATE_LUFS)
        .collect()
}

fn mean_of(blocks: &[f64]) -> f64 {
    blocks.iter().sum::<f64>() / blocks.len() as f64
}

fn integrated(momentary: &[f64]) -> Option<f32> {
    let blocks = audible(momentary);
    if blocks.is_empty() {
        return None;
    }

    let relative_gate = loudness_of(mean_of(&blocks)) + RELATIVE_GATE_LU;
    let (sum, count) = blocks
        .iter()
        .filter(|block| loudness_of(**block) > relative_gate)
        .fold((0.0, 0_u32), |(sum, count), block| (sum + block, count + 1));
    (count > 0).then(|| loudness_of(sum / f64::from(count)) as f32)
}

fn range(short_term: &[f64]) -> Option<f32> {
    let blocks = audible(short_term);
    if blocks.is_empty() {
        return None;
    }

    let relative_gate = loudness_of(mean_of(&blocks)) + RANGE_RELATIVE_GATE_LU;
    let mut kept: Vec<f64> = blocks
        .iter()
        .map(|block| loudness_of(*block))
        .filter(|loudness| *loudness > relative_gate)
        .collect();
    kept.sort_by(f64::total_cmp);
    let at = |share: f64| kept[((kept.len() - 1) as f64 * share).round() as usize];
    (!kept.is_empty()).then(|| (at(RANGE_HIGH_PERCENTILE) - at(RANGE_LOW_PERCENTILE)) as f32)
}

fn loudest(blocks: &[f64]) -> Option<f32> {
    audible(blocks)
        .into_iter()
        .max_by(f64::total_cmp)
        .map(|block| loudness_of(block) as f32)
}

fn dynamic_range(channels: &[Vec<DrBlock>]) -> Option<u8> {
    let readings: Vec<f64> = channels
        .iter()
        .filter_map(|blocks| dynamic_range_of(blocks))
        .collect();
    if readings.is_empty() {
        return None;
    }

    let mean = readings.iter().sum::<f64>() / readings.len() as f64;
    Some(mean.round().clamp(0.0, f64::from(u8::MAX)) as u8)
}

fn dynamic_range_of(blocks: &[DrBlock]) -> Option<f64> {
    let mut rms: Vec<f64> = blocks
        .iter()
        .map(|block| (DR_RMS_SCALE * block.squares).sqrt())
        .collect();
    rms.sort_by(|one, other| other.total_cmp(one));
    let loudest = ((rms.len() as f64 * DR_LOUDEST_SHARE) as usize).max(1);
    let top = (rms.iter().take(loudest).map(|rms| rms * rms).sum::<f64>() / loudest as f64).sqrt();

    let mut peaks: Vec<f64> = blocks.iter().map(|block| block.peak).collect();
    peaks.sort_by(|one, other| other.total_cmp(one));
    let peak = peaks.get(1).or_else(|| peaks.first()).copied()?;

    (top > 0.0 && peak > 0.0).then(|| 20.0 * (peak / top).log10())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn sine(channels: usize, seconds: u32, amplitude: f32, hz: f32) -> Vec<f32> {
        (0..RATE * seconds)
            .flat_map(|frame| {
                let sample = amplitude
                    * (frame as f32 * 2.0 * std::f32::consts::PI * hz / RATE as f32).sin();
                std::iter::repeat_n(sample, channels)
            })
            .collect()
    }

    fn metered(channels: usize, interleaved: &[f32]) -> Loudness {
        let placements = resonate_codec::Speakers::UNNAMED.placements(channels as u8);
        let mut metering = Metering::new(&placements, RATE);
        for block in interleaved.chunks(4_096 * channels) {
            metering.note(block);
        }
        metering.finished()
    }

    #[test]
    fn the_k_weighting_at_48_khz_is_the_one_the_recommendation_tabulates() {
        let shelf = Biquad::shelf(48_000.0);
        assert!((shelf.b0 - 1.535_124_859_586_97).abs() < 1e-9);
        assert!((shelf.b1 + 2.691_696_189_406_38).abs() < 1e-9);
        assert!((shelf.b2 - 1.198_392_810_852_85).abs() < 1e-9);
        assert!((shelf.a1 + 1.690_659_293_182_41).abs() < 1e-9);
        assert!((shelf.a2 - 0.732_480_774_215_85).abs() < 1e-9);

        let high_pass = Biquad::high_pass(48_000.0);
        assert!((high_pass.a1 + 1.990_047_454_833_98).abs() < 1e-9);
        assert!((high_pass.a2 - 0.990_072_250_366_21).abs() < 1e-9);
    }

    #[test]
    fn a_full_scale_997_hz_sine_in_one_channel_reads_minus_three_lufs() {
        let mono = metered(1, &sine(1, 20, 1.0, 997.0));
        let integrated = mono.integrated.expect("a loudness");
        assert!((integrated + 3.01).abs() < 0.05, "{integrated}");

        let stereo = metered(2, &sine(2, 20, 1.0, 997.0));
        let integrated = stereo.integrated.expect("a loudness");
        assert!(integrated.abs() < 0.05, "{integrated}");
    }

    #[test]
    fn silence_has_no_loudness_and_no_dynamic_range() {
        let silent = metered(2, &vec![0.0; RATE as usize * 2 * 10]);
        assert_eq!(silent.integrated, None);
        assert_eq!(silent.range, None);
        assert_eq!(silent.momentary_max, None);
        assert_eq!(silent.short_term_max, None);
        assert_eq!(silent.dynamic_range, None);
    }

    #[test]
    fn the_lfe_is_left_out_and_the_surrounds_weigh_more() {
        let lfe_alone: Vec<f32> = sine(1, 20, 1.0, 60.0)
            .into_iter()
            .flat_map(|sample| [0.0, 0.0, 0.0, sample, 0.0, 0.0])
            .collect();
        assert_eq!(metered(6, &lfe_alone).integrated, None);

        let everywhere = metered(6, &sine(6, 20, 1.0, 997.0))
            .integrated
            .expect("a loudness");
        let expected = -3.01 + 10.0 * (3.0 + 2.0 * SURROUND_WEIGHT).log10();
        assert!(
            (f64::from(everywhere) - expected).abs() < 0.05,
            "{everywhere}"
        );
    }

    #[test]
    fn a_steady_tone_has_no_range_and_its_maxima_are_its_loudness() {
        let steady = metered(2, &sine(2, 30, 0.5, 997.0));
        let integrated = steady.integrated.expect("a loudness");
        assert!(steady.range.expect("a range") < 0.1);
        let momentary = steady.momentary_max.expect("a momentary maximum");
        let short_term = steady.short_term_max.expect("a short-term maximum");
        assert!((momentary - integrated).abs() < 0.05, "{momentary}");
        assert!((short_term - integrated).abs() < 0.05, "{short_term}");
    }

    #[test]
    fn two_levels_ten_decibels_apart_read_as_ten_loudness_units_of_range() {
        let loud = sine(1, 10, 0.5, 997.0);
        let quiet = sine(1, 10, 0.5 / 10_f32.sqrt(), 997.0);
        let alternating: Vec<f32> = (0..3)
            .flat_map(|_| loud.iter().chain(&quiet).copied())
            .collect();

        let range = metered(1, &alternating).range.expect("a range");
        assert!((range - 10.0).abs() < 0.2, "{range}");
    }

    #[test]
    fn a_steady_sine_has_no_dynamic_range_to_speak_of() {
        let steady = metered(2, &sine(2, 30, 0.5, 440.0));
        assert_eq!(steady.dynamic_range, Some(0));
    }

    #[test]
    fn transient_peaks_over_a_quiet_body_read_as_range() {
        let mut interleaved = sine(1, 30, 0.1, 440.0);
        for spike in interleaved.iter_mut().step_by(RATE as usize) {
            *spike = 1.0;
        }
        let dynamic = metered(1, &interleaved).dynamic_range.expect("a range");
        assert!((19..=21).contains(&dynamic), "{dynamic}");
    }
}
