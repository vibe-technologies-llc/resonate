use std::{mem, ops::Range, sync::Arc};

use parking_lot::Mutex;
use rustfft::{Fft, FftPlanner, num_complex::Complex};

const LONGEST_TRANSFORM: usize = 1 << 18;
const TURNED_POINTS_OVER_THE_BAND: usize = 16;
const EVEN_POINTS_OVER_THE_BAND: usize = 8;
const KEPT_PLAN_BYTES_AT_MOST: usize = 4 << 20;
const SMOOTH_RADICES: [usize; 4] = [2, 3, 5, 7];
const BINS_ROUNDED_WITHIN: f64 = 1e-9;
const SILENCE: Complex<f64> = Complex::new(0.0, 0.0);
const CUBED: u32 = 3;
const AT_LEAST_FOUR_TWOS: u32 = 2;
const SLOW_FIVES_DRAG_STAGES: f64 = 2.0;
const FEW_TWOS_DRAG_STAGES: f64 = 1.5;
const LARGE_TRANSFORM_FROM: usize = 8_192;
const LARGE_TRANSFORM_DRAG_STAGES: f64 = 1.0;

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

fn exponent_of(mut points: usize, radix: usize) -> u32 {
    let mut exponent = 0;
    while points.is_multiple_of(radix) {
        points /= radix;
        exponent += 1;
    }
    exponent
}

fn transform_weight(points: usize) -> f64 {
    let stages = (points as f64).log2();
    let drags = if exponent_of(points, 5) >= CUBED {
        SLOW_FIVES_DRAG_STAGES
    } else {
        0.0
    } + if exponent_of(points, 2) < AT_LEAST_FOUR_TWOS {
        FEW_TWOS_DRAG_STAGES
    } else {
        0.0
    };
    let beyond_the_near_cache = if points >= LARGE_TRANSFORM_FROM {
        LARGE_TRANSFORM_DRAG_STAGES
    } else {
        0.0
    };
    points as f64 * (stages + drags + beyond_the_near_cache)
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
        let shortest_units = trail_units + lead_units + 1;
        let weight = |units: usize| {
            let hop_units = units - trail_units - lead_units;
            (transform_weight(units * unit_in) + transform_weight(units * unit_out))
                / (hop_units * unit_in) as f64
        };
        let usable = |units: &usize| units.is_multiple_of(2) && is_smooth(*units);

        let units = (shortest_units..=longest_units.min(shortest_units - 1 + hop_at_most))
            .filter(usable)
            .min_by(|units, other| weight(*units).total_cmp(&weight(*other)))
            .or_else(|| (shortest_units..=longest_units).find(usable))?;

        Some(Self {
            unit_in,
            unit_out,
            units,
            trail_units,
            hop_units: units - trail_units - lead_units,
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

pub(crate) enum Design<At> {
    Even(At),
    Turned(At),
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
            Design::Even(at_seconds) => {
                let points = EVEN_POINTS_OVER_THE_BAND * 2 * bins;
                let step = block_seconds / points as f64;
                let mut spectrum = vec![SILENCE; points];
                if let Some(first) = spectrum.first_mut() {
                    first.re = at_seconds(0.0);
                }
                let (rising, falling) = spectrum.split_at_mut(points / 2 + 1);
                let mirrored = rising.iter_mut().skip(1).zip(falling.iter_mut().rev());
                for (point, (at, mirror)) in mirrored.enumerate() {
                    let tap = at_seconds((point + 1) as f64 * step);
                    at.re = tap;
                    mirror.re = tap;
                }
                planner.plan_fft_forward(points).process(&mut spectrum);
                let scale = unity / spectrum.first().map_or(1.0, |bin| bin.re);
                let held = spectrum.get(..bins).unwrap_or_default();
                Self::Even(held.iter().map(|bin| bin.re * scale).collect())
            }
            Design::Turned(at_seconds) => {
                let points = TURNED_POINTS_OVER_THE_BAND * 2 * bins;
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
                let scale = unity / spectrum.first().map_or(1.0, |bin| bin.re);
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
    fn a_transform_the_library_runs_slowly_weighs_more_than_a_longer_one_it_runs_well() {
        const SLOW_FIVES: usize = 6_750;
        const SMOOTH_AND_LONGER: usize = 6_912;

        assert!(transform_weight(SLOW_FIVES) > transform_weight(SMOOTH_AND_LONGER));
    }

    #[test]
    fn the_search_never_takes_a_block_longer_than_the_hop_allows_while_one_fits() {
        let wanted = conversion(96_000, 48_000);
        let blocks = Blocks::fitting(wanted).expect("common rates are smooth");
        let hop_seconds = blocks.hop_units as f64 * blocks.unit_in as f64 / 96_000.0;

        assert!(hop_seconds <= wanted.hop_at_most_seconds + 1e-12);
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
        let mut planner = FftPlanner::new();
        let response = Response::of(
            Design::Even(|after: f64| f64::from(after == 0.0)),
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
        let cutoff_hz = 19_000.0_f64;
        let reach_seconds = 64.0 / 44_100.0;
        let plan = Plan::kept_for(
            &cache,
            0_u8,
            wanted,
            blocks,
            Design::Even(|after: f64| {
                let along = after.abs() / reach_seconds;
                if along >= 1.0 {
                    return 0.0;
                }
                let turned = TAU * cutoff_hz * after;
                let sinc = if turned == 0.0 {
                    1.0
                } else {
                    turned.sin() / turned
                };
                let bell = (along * std::f64::consts::FRAC_PI_2).cos().powi(8);
                sinc * bell
            }),
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
                (frame[0] - left).abs() < 1e-4,
                "left {} at {index}",
                frame[0]
            );
            assert!(
                (frame[1] - right).abs() < 1e-4,
                "right {} at {index}",
                frame[1]
            );
        }
    }
}
