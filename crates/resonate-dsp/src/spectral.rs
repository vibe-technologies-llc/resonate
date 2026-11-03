use std::{mem, ops::Range, sync::Arc};

use parking_lot::Mutex;
use rustfft::{Fft, FftPlanner, num_complex::Complex};

use crate::resample::bessel_i0;

const LONGEST_TRANSFORM: usize = 1 << 18;
const SAMPLED_POINTS_OVER_THE_BAND: usize = 16;
const KEPT_PLAN_BYTES_AT_MOST: usize = 4 << 20;
const SMOOTH_RADICES: [usize; 4] = [2, 3, 5, 7];
const BINS_ROUNDED_WITHIN: f64 = 1e-9;
const TRANSITION_STEPS: usize = 4_096;
const SILENCE: Complex<f64> = Complex::new(0.0, 0.0);

const fn greatest_common_divisor(a: u64, b: u64) -> u64 {
    if b == 0 {
        a
    } else {
        greatest_common_divisor(b, a % b)
    }
}

fn is_smooth(mut value: usize) -> bool {
    for radix in SMOOTH_RADICES {
        while value.is_multiple_of(radix) {
            value /= radix;
        }
    }
    value == 1
}

fn transform_weight(points: usize) -> f64 {
    points as f64 * (points as f64).log2()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Conversion {
    pub(crate) input_hz: u32,
    pub(crate) output_hz: u32,
    pub(crate) band_hz: f64,
    pub(crate) lead_seconds: f64,
    pub(crate) trail_seconds: f64,
    pub(crate) hop_at_most_seconds: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Blocks {
    unit_in: usize,
    unit_out: usize,
    units: usize,
    trail_units: usize,
    hop_units: usize,
}

impl Blocks {
    pub(crate) fn fitting(conversion: Conversion) -> Option<Self> {
        let common = greatest_common_divisor(
            u64::from(conversion.input_hz),
            u64::from(conversion.output_hz),
        );
        let unit_in = (u64::from(conversion.input_hz) / common) as usize;
        let unit_out = (u64::from(conversion.output_hz) / common) as usize;
        if !is_smooth(unit_in) || !is_smooth(unit_out) {
            return None;
        }

        let unit_seconds = unit_in as f64 / f64::from(conversion.input_hz);
        let trail_units = (conversion.trail_seconds / unit_seconds).ceil() as usize;
        let lead_units = (conversion.lead_seconds / unit_seconds).ceil() as usize;
        let hop_at_most = ((conversion.hop_at_most_seconds / unit_seconds) as usize).max(1);
        let longest_units = LONGEST_TRANSFORM / unit_in.max(unit_out);
        let weight = |units: usize, hop_units: usize| {
            (transform_weight(units * unit_in) + transform_weight(units * unit_out))
                / (hop_units * unit_in) as f64
        };
        let candidates = || {
            (2..=longest_units)
                .step_by(2)
                .filter(|units| is_smooth(*units))
                .filter_map(|units| {
                    units
                        .checked_sub(trail_units + lead_units)
                        .filter(|hop_units| *hop_units > 0)
                        .map(|hop_units| (units, hop_units))
                })
        };

        candidates()
            .filter(|(_, hop_units)| *hop_units <= hop_at_most)
            .min_by(|(units, hop_units), (other, other_hop)| {
                weight(*units, *hop_units).total_cmp(&weight(*other, *other_hop))
            })
            .or_else(|| candidates().next())
            .map(|(units, hop_units)| Self {
                unit_in,
                unit_out,
                units,
                trail_units,
                hop_units,
            })
    }

    const fn input_points(self) -> usize {
        self.units * self.unit_in
    }

    const fn output_points(self) -> usize {
        self.units * self.unit_out
    }

    const fn hop_in(self) -> usize {
        self.hop_units * self.unit_in
    }

    const fn kept_in(self) -> usize {
        self.input_points() - self.hop_in()
    }

    const fn released_from(self) -> usize {
        self.trail_units * self.unit_out
    }

    pub(crate) const fn outputs_held(self) -> usize {
        (self.units - self.trail_units) * self.unit_out
    }

    pub(crate) const fn unit_out(self) -> usize {
        self.unit_out
    }

    pub(crate) const fn trail_units(self) -> usize {
        self.trail_units
    }

    pub(crate) const fn outputs_of(self, frames_in: usize) -> usize {
        match frames_in.checked_sub(1) {
            Some(last) => last * self.unit_out / self.unit_in + 1,
            None => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FlatTop {
    pub(crate) pass_hz: f64,
    pub(crate) stop_hz: f64,
    pub(crate) beta: f64,
}

impl FlatTop {
    pub(crate) fn of(cutoff_hz: f64, width_hz: f64, band_edge_hz: f64, beta: f64) -> Self {
        Self {
            pass_hz: cutoff_hz - width_hz / 2.0,
            stop_hz: (cutoff_hz + width_hz / 2.0).min(band_edge_hz),
            beta,
        }
    }

    fn gains(self, bins: usize, bin_hz: f64, unity: f64) -> Vec<f64> {
        let step = 2.0 / TRANSITION_STEPS as f64;
        let mut still_to_fall = vec![0.0; TRANSITION_STEPS + 1];
        let mut whole = 0.0;
        for (index, slot) in still_to_fall.iter_mut().enumerate().rev().skip(1) {
            let low = -1.0 + index as f64 * step;
            whole += kaiser_between(self.beta, low, low + step);
            *slot = whole;
        }

        (0..bins)
            .map(|bin| {
                let hz = bin as f64 * bin_hz;
                if hz >= self.stop_hz {
                    return 0.0;
                }
                if hz <= self.pass_hz {
                    return unity;
                }
                let along = 2.0 * (hz - self.pass_hz) / (self.stop_hz - self.pass_hz) - 1.0;
                let next = (((along + 1.0) / step).ceil() as usize).min(TRANSITION_STEPS);
                let next_at = -1.0 + next as f64 * step;
                let beyond = still_to_fall.get(next).copied().unwrap_or_default();
                unity * (beyond + kaiser_between(self.beta, along, next_at)) / whole
            })
            .collect()
    }
}

fn kaiser_between(beta: f64, from: f64, to: f64) -> f64 {
    let spread = 2.0 / 7.0 * (6.0_f64 / 5.0).sqrt();
    let inner = (3.0 / 7.0 - spread).sqrt();
    let outer = (3.0 / 7.0 + spread).sqrt();
    let inner_weight = (18.0 + 30.0_f64.sqrt()) / 36.0;
    let outer_weight = (18.0 - 30.0_f64.sqrt()) / 36.0;
    let middle = (from + to) / 2.0;
    let half = (to - from) / 2.0;
    let at = |x: f64| bessel_i0(beta * (1.0 - x * x).max(0.0).sqrt());
    half * (inner_weight * (at(middle - half * inner) + at(middle + half * inner))
        + outer_weight * (at(middle - half * outer) + at(middle + half * outer)))
}

pub(crate) enum Design<At> {
    FlatTop(FlatTop),
    Sampled(At),
}

enum Response {
    Even(Vec<f64>),
    Turned(Vec<Complex<f64>>),
}

impl Response {
    fn of(
        design: Design<impl Fn(f64) -> f64>,
        blocks: Blocks,
        conversion: Conversion,
        planner: &mut FftPlanner<f64>,
    ) -> Self {
        let input_points = blocks.input_points();
        let block_seconds = input_points as f64 / f64::from(conversion.input_hz);
        let bins_in_the_band =
            (conversion.band_hz * block_seconds / 2.0 + BINS_ROUNDED_WITHIN).floor() as usize;
        let bins = bins_in_the_band
            .min(input_points / 2)
            .min(blocks.output_points() / 2)
            .max(1);
        let unity = (input_points as f64).recip();

        match design {
            Design::FlatTop(flat) => Self::Even(flat.gains(bins, block_seconds.recip(), unity)),
            Design::Sampled(at_seconds) => {
                let points = SAMPLED_POINTS_OVER_THE_BAND * 2 * bins;
                let step = block_seconds / points as f64;
                let halfway = points / 2;
                let mut spectrum: Vec<Complex<f64>> = (0..points)
                    .map(|point| {
                        let after = if point < halfway {
                            point as f64
                        } else {
                            point as f64 - points as f64
                        };
                        Complex::new(at_seconds(after * step), 0.0)
                    })
                    .collect();
                planner.plan_fft_forward(points).process(&mut spectrum);
                let direct = spectrum.first().map_or(1.0, |bin| bin.re);
                let scale = unity / direct;
                let held = spectrum.get(..bins).unwrap_or_default();
                Self::Turned(held.iter().map(|bin| bin * scale).collect())
            }
        }
    }

    fn bins(&self) -> usize {
        match self {
            Self::Even(gains) => gains.len(),
            Self::Turned(turns) => turns.len(),
        }
    }

    fn bytes(&self) -> usize {
        match self {
            Self::Even(gains) => gains.capacity() * size_of::<f64>(),
            Self::Turned(turns) => turns.capacity() * size_of::<Complex<f64>>(),
        }
    }

    fn applied(&self, spectrum: &[Complex<f64>], product: &mut [Complex<f64>]) {
        let bins = self.bins();
        let negative_bins = bins - 1;
        let (rising, rest) = product.split_at_mut(bins);
        let (between, falling) = rest.split_at_mut(rest.len() - negative_bins);
        between.fill(SILENCE);
        let heard_rising = spectrum.get(..bins).unwrap_or_default();
        let heard_falling = spectrum
            .get(spectrum.len() - negative_bins..)
            .unwrap_or_default();
        let falling_bins = falling.iter_mut().rev().zip(heard_falling.iter().rev());

        match self {
            Self::Even(gains) => {
                let rising_bins = rising.iter_mut().zip(heard_rising);
                for ((slot, heard), gain) in rising_bins.zip(gains) {
                    *slot = Complex::new(heard.re * gain, heard.im * gain);
                }
                for ((slot, heard), gain) in falling_bins.zip(gains.get(1..).unwrap_or_default()) {
                    *slot = Complex::new(heard.re * gain, heard.im * gain);
                }
            }
            Self::Turned(turns) => {
                for ((slot, heard), turn) in rising.iter_mut().zip(heard_rising).zip(turns) {
                    *slot = heard * turn;
                }
                for ((slot, heard), turn) in falling_bins.zip(turns.get(1..).unwrap_or_default()) {
                    *slot = heard * turn.conj();
                }
            }
        }
    }
}

pub(crate) struct Plan {
    blocks: Blocks,
    forward: Arc<dyn Fft<f64>>,
    inverse: Arc<dyn Fft<f64>>,
    response: Response,
}

impl Plan {
    pub(crate) fn kept_for<Key: PartialEq + Copy>(
        cache: &Mutex<Option<(Key, Arc<Self>)>>,
        wanted: Key,
        conversion: Conversion,
        blocks: Blocks,
        design: Design<impl Fn(f64) -> f64>,
    ) -> Arc<Self> {
        if let Some((_, kept)) = cache
            .lock()
            .as_ref()
            .filter(|(held, kept)| *held == wanted && kept.blocks == blocks)
        {
            return Arc::clone(kept);
        }
        let mut planner = FftPlanner::<f64>::new();
        let built = Arc::new(Self {
            blocks,
            forward: planner.plan_fft_forward(blocks.input_points()),
            inverse: planner.plan_fft_inverse(blocks.output_points()),
            response: Response::of(design, blocks, conversion, &mut planner),
        });
        *cache.lock() = (built.response.bytes() <= KEPT_PLAN_BYTES_AT_MOST)
            .then(|| (wanted, Arc::clone(&built)));
        built
    }
}

struct Lane {
    window: Vec<Complex<f64>>,
    spare: Vec<Complex<f64>>,
    ready: Vec<Complex<f64>>,
}

impl Lane {
    fn new(blocks: Blocks) -> Self {
        Self {
            window: vec![SILENCE; blocks.input_points()],
            spare: vec![SILENCE; blocks.input_points()],
            ready: vec![SILENCE; blocks.output_points()],
        }
    }
}

pub(crate) struct Spectral {
    plan: Arc<Plan>,
    lanes: Vec<Lane>,
    spectrum: Vec<Complex<f64>>,
    product: Vec<Complex<f64>>,
    forward_scratch: Vec<Complex<f64>>,
    inverse_scratch: Vec<Complex<f64>>,
    primed: usize,
    filled: usize,
    released: usize,
    answering: bool,
}

impl Spectral {
    pub(crate) fn new(plan: Arc<Plan>, channels: usize, primed_units: usize) -> Self {
        let blocks = plan.blocks;
        let primed = primed_units * blocks.unit_in;
        Self {
            lanes: (0..channels.div_ceil(2))
                .map(|_| Lane::new(blocks))
                .collect(),
            spectrum: vec![SILENCE; blocks.input_points()],
            product: vec![SILENCE; blocks.output_points()],
            forward_scratch: vec![SILENCE; plan.forward.get_outofplace_scratch_len()],
            inverse_scratch: vec![SILENCE; plan.inverse.get_outofplace_scratch_len()],
            primed,
            filled: primed,
            released: 0,
            answering: false,
            plan,
        }
    }

    pub(crate) fn blocks(&self) -> Blocks {
        self.plan.blocks
    }

    #[cfg(test)]
    pub(crate) const fn plan(&self) -> &Arc<Plan> {
        &self.plan
    }

    pub(crate) fn reset(&mut self) {
        for lane in &mut self.lanes {
            lane.window.fill(SILENCE);
        }
        self.filled = self.primed;
        self.released = 0;
        self.answering = false;
    }

    pub(crate) fn room(&self) -> usize {
        self.plan.blocks.input_points() - self.filled
    }

    pub(crate) fn frames_fitting(&self, room_out: usize) -> usize {
        if !self.answering {
            return self.room();
        }
        let blocks = self.plan.blocks;
        let heard = self.filled - blocks.kept_in();
        let most = ((self.released + room_out + 1) * blocks.unit_in - 1) / blocks.unit_out;
        most.saturating_sub(heard).min(self.room())
    }

    pub(crate) fn gather(&mut self, input: &[f64], channels: usize, frames: usize) {
        let filled = self.filled;
        if let ([lane], 2) = (self.lanes.as_mut_slice(), channels) {
            let (pairs, _) = input.as_chunks::<2>();
            if let Some(run) = lane.window.get_mut(filled..filled + frames) {
                for (slot, [left, right]) in run.iter_mut().zip(pairs) {
                    *slot = Complex::new(*left, *right);
                }
            }
        } else {
            for (pair, lane) in self.lanes.iter_mut().enumerate() {
                let Some(run) = lane.window.get_mut(filled..filled + frames) else {
                    continue;
                };
                for (slot, frame) in run.iter_mut().zip(input.chunks_exact(channels)) {
                    *slot = Complex::new(
                        frame.get(2 * pair).copied().unwrap_or_default(),
                        frame.get(2 * pair + 1).copied().unwrap_or_default(),
                    );
                }
            }
        }
        self.filled += frames;
    }

    pub(crate) fn gather_silence(&mut self, frames: usize) {
        let filled = self.filled;
        for lane in &mut self.lanes {
            if let Some(run) = lane.window.get_mut(filled..filled + frames) {
                run.fill(SILENCE);
            }
        }
        self.filled += frames;
    }

    pub(crate) fn due(&mut self) -> Range<usize> {
        if !self.answering {
            return 0..0;
        }
        let blocks = self.plan.blocks;
        let due = (self.filled - blocks.kept_in()) * blocks.unit_out / blocks.unit_in;
        let run = self.released..due;
        self.released = due;
        run
    }

    pub(crate) fn interleaved_into(&self, run: Range<usize>, output: &mut [f64], channels: usize) {
        let from = self.plan.blocks.released_from();
        let span = from + run.start..from + run.end;
        if let ([lane], 2) = (self.lanes.as_slice(), channels) {
            let (frames, _) = output.as_chunks_mut::<2>();
            let heard = lane.ready.get(span).unwrap_or_default();
            for (frame, sample) in frames.iter_mut().zip(heard) {
                *frame = [sample.re, sample.im];
            }
            return;
        }
        for (pair, lane) in self.lanes.iter().enumerate() {
            let heard = lane.ready.get(span.clone()).unwrap_or_default();
            for (frame, sample) in output.chunks_exact_mut(channels).zip(heard) {
                if let Some(slot) = frame.get_mut(2 * pair) {
                    *slot = sample.re;
                }
                if let Some(slot) = frame.get_mut(2 * pair + 1) {
                    *slot = sample.im;
                }
            }
        }
    }

    pub(crate) fn plane_into(&self, channel: usize, run: Range<usize>, slots: &mut [f64]) {
        let from = self.plan.blocks.released_from();
        let heard = self
            .lanes
            .get(channel / 2)
            .and_then(|lane| lane.ready.get(from + run.start..from + run.end))
            .unwrap_or_default();
        if channel.is_multiple_of(2) {
            for (slot, sample) in slots.iter_mut().zip(heard) {
                *slot = sample.re;
            }
        } else {
            for (slot, sample) in slots.iter_mut().zip(heard) {
                *slot = sample.im;
            }
        }
    }

    pub(crate) fn turn_when_full(&mut self) {
        let blocks = self.plan.blocks;
        if self.filled < blocks.input_points() {
            return;
        }
        let Self {
            plan,
            lanes,
            spectrum,
            product,
            forward_scratch,
            inverse_scratch,
            ..
        } = self;
        for lane in lanes {
            let kept = lane.window.get(blocks.hop_in()..).unwrap_or_default();
            if let Some(start) = lane.spare.get_mut(..kept.len()) {
                start.copy_from_slice(kept);
            }
            plan.forward.process_outofplace_with_scratch(
                &mut lane.window,
                spectrum,
                forward_scratch,
            );
            plan.response.applied(spectrum, product);
            plan.inverse
                .process_outofplace_with_scratch(product, &mut lane.ready, inverse_scratch);
            mem::swap(&mut lane.window, &mut lane.spare);
        }
        self.filled = blocks.kept_in();
        self.released = 0;
        self.answering = true;
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use super::*;

    fn conversion(input_hz: u32, output_hz: u32) -> Conversion {
        let band_hz = f64::from(input_hz.min(output_hz));
        Conversion {
            input_hz,
            output_hz,
            band_hz,
            lead_seconds: 64.0 / band_hz,
            trail_seconds: 64.0 / band_hz,
            hop_at_most_seconds: 512.0 / band_hz,
        }
    }

    #[test]
    fn a_block_is_a_whole_number_of_ratio_cycles_at_both_rates_and_holds_the_kernel() {
        for (input_hz, output_hz) in [
            (44_100, 48_000),
            (48_000, 44_100),
            (96_000, 48_000),
            (48_000, 96_000),
            (192_000, 44_100),
            (8_000, 192_000),
        ] {
            let wanted = conversion(input_hz, output_hz);
            let blocks = Blocks::fitting(wanted).expect("common rates are smooth");
            let block_seconds = blocks.input_points() as f64 / f64::from(input_hz);
            let also_seconds = blocks.output_points() as f64 / f64::from(output_hz);
            assert!((block_seconds - also_seconds).abs() < 1e-12);
            assert!(blocks.input_points().is_multiple_of(2));
            assert!(blocks.output_points().is_multiple_of(2));

            let unit_seconds = blocks.unit_in as f64 / f64::from(input_hz);
            let hop_seconds = blocks.hop_units as f64 * unit_seconds;
            assert!(
                hop_seconds + wanted.lead_seconds + wanted.trail_seconds <= block_seconds + 1e-12,
                "{input_hz} -> {output_hz}: {hop_seconds} s of hop and the kernel fill more than {block_seconds} s"
            );
        }
    }

    #[test]
    fn a_rate_with_a_large_prime_in_its_ratio_is_not_cut_into_blocks() {
        assert_eq!(Blocks::fitting(conversion(44_100, 47_993)), None);
    }

    #[test]
    fn the_flat_top_is_exactly_one_in_its_passband_and_exactly_nothing_past_its_stop() {
        let flat = FlatTop::of(20_000.0, 2_000.0, 21_000.0, 20.0);
        let gains = flat.gains(2_205, 10.0, 1.0);

        assert!(gains[..1_900].iter().all(|gain| *gain == 1.0));
        assert!(gains[2_100..].iter().all(|gain| *gain == 0.0));
        assert!(
            gains[1_900..2_100]
                .windows(2)
                .all(|pair| pair[0] >= pair[1]),
            "the transition rose somewhere"
        );
        let halfway = gains[2_000];
        assert!((halfway - 0.5).abs() < 1e-12, "{halfway} at the cutoff");
        for offset in 1..100 {
            let sum = gains[2_000 - offset] + gains[2_000 + offset];
            assert!(
                (sum - 1.0).abs() < 1e-12,
                "the transition is not odd: {sum}"
            );
        }
    }

    #[test]
    fn a_transition_past_the_band_edge_is_closed_at_it_rather_than_cut_off() {
        let flat = FlatTop::of(20_000.0, 2_000.0, 20_500.0, 20.0);
        let gains = flat.gains(2_205, 10.0, 1.0);

        assert_eq!(gains[2_050], 0.0);
        assert!(gains[2_049] < 1e-7, "{} one bin short", gains[2_049]);
    }

    #[test]
    fn a_block_count_with_a_rounded_band_keeps_the_last_bin() {
        let wanted = conversion(96_000, 48_000);
        let blocks = Blocks {
            unit_in: 2,
            unit_out: 1,
            units: 3_136,
            trail_units: 64,
            hop_units: 3_008,
        };
        let flat = FlatTop::of(23_900.0, 200.0, 24_000.0, 20.0);
        let mut planner = FftPlanner::new();
        let response = Response::of(
            Design::<fn(f64) -> f64>::FlatTop(flat),
            blocks,
            wanted,
            &mut planner,
        );

        assert_eq!(response.bins(), 1_568);
    }

    #[test]
    fn a_stereo_pair_in_one_transform_comes_out_as_two_filtered_channels() {
        let wanted = conversion(44_100, 48_000);
        let blocks = Blocks::fitting(wanted).expect("common rates are smooth");
        let cache = Mutex::new(None);
        let flat = FlatTop::of(18_000.0, 2_000.0, 22_050.0, 12.0);
        let plan = Plan::kept_for(
            &cache,
            0_u8,
            wanted,
            blocks,
            Design::<fn(f64) -> f64>::FlatTop(flat),
        );
        let frames = 40_000;
        let interleaved: Vec<f64> = (0..frames)
            .flat_map(|at| {
                let t = f64::from(at) / 44_100.0;
                [(TAU * 1_000.0 * t).sin(), 0.5 * (TAU * 7_000.0 * t).cos()]
            })
            .collect();

        let mut stereo = Spectral::new(plan, 2, blocks.trail_units());
        let mut output = vec![0.0; 2 * blocks.outputs_of(frames as usize)];
        let mut made = 0;
        let mut taken = 0;
        while taken < frames as usize {
            let run = stereo.room().min(frames as usize - taken);
            stereo.gather(&interleaved[taken * 2..], 2, run);
            let due = stereo.due();
            let to = made + due.len();
            stereo.interleaved_into(due, &mut output[made * 2..to * 2], 2);
            made = to;
            stereo.turn_when_full();
            taken += run;
        }

        assert!(made > 30_000);
        for (index, frame) in output[..made * 2]
            .as_chunks::<2>()
            .0
            .iter()
            .enumerate()
            .skip(4_096)
        {
            let t = index as f64 / 48_000.0;
            let left = (TAU * 1_000.0 * t).sin();
            let right = 0.5 * (TAU * 7_000.0 * t).cos();
            assert!(
                (frame[0] - left).abs() < 1e-6,
                "left {} at {index}",
                frame[0]
            );
            assert!(
                (frame[1] - right).abs() < 1e-6,
                "right {} at {index}",
                frame[1]
            );
        }
    }
}
