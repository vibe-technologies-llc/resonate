use std::{array, f64::consts::PI, num::NonZeroU32, sync::Arc};

use parking_lot::{Mutex, const_mutex};
use resonate_core::{ChannelLayout, Ratio, SampleRate, StreamSpec};

use crate::{
    Error, FilterPhase, ProcessCount, Processor, RatioLimits, Result,
    phase::{Prototype, designed},
};

const MAX_HALF_TAPS: u16 = 1_024;
const MIN_PHASES: u32 = 8;
const LANES_PER_VECTOR: usize = if cfg!(target_feature = "avx512f") {
    8
} else if cfg!(target_feature = "avx") {
    4
} else {
    2
};
const VECTOR_BYTES: usize = LANES_PER_VECTOR * size_of::<f64>();
const ACCUMULATORS: usize = 8;
const _: () =
    assert!(ACCUMULATORS.is_power_of_two() && ACCUMULATORS.is_multiple_of(LANES_PER_VECTOR));
const TABULATED_WEIGHT_BYTES_AT_MOST: usize = 16 << 20;
const KEPT_TABLE_BYTES_AT_MOST: usize = 4 << 20;
const LAGRANGE_NODES: usize = 4;

type Lanes = [f64; ACCUMULATORS];

const fn ratio(numer: u32, denom: u32) -> Ratio {
    match (NonZeroU32::new(numer), NonZeroU32::new(denom)) {
        (Some(numer), Some(denom)) => Ratio { numer, denom },
        _ => panic!("a Ratio constant cannot have a zero term"),
    }
}

const LIMITS: RatioLimits = RatioLimits {
    min: ratio(1, 32),
    max: ratio(32, 1),
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Quality {
    Fast,
    Balanced,
    #[default]
    High,
    VeryHigh,
}

impl Quality {
    pub const fn params(self) -> SincParams {
        match self {
            Self::Fast => SincParams {
                half_taps: 16,
                phases: 128,
                cutoff: 0.900,
                kaiser_beta: 6.0,
            },
            Self::Balanced => SincParams {
                half_taps: 32,
                phases: 512,
                cutoff: 0.940,
                kaiser_beta: 9.0,
            },
            Self::High => SincParams {
                half_taps: 128,
                phases: 2048,
                cutoff: 0.965,
                kaiser_beta: 14.0,
            },
            Self::VeryHigh => SincParams {
                half_taps: 320,
                phases: 1024,
                cutoff: 0.978,
                kaiser_beta: 19.5,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SincParams {
    pub half_taps: u16,
    pub phases: u32,
    pub cutoff: f32,
    pub kaiser_beta: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResamplerConfig {
    pub input_rate: SampleRate,
    pub output_rate: SampleRate,
    pub channels: ChannelLayout,
    pub quality: Quality,
    pub phase: FilterPhase,
    pub max_frames_in: usize,
}

fn bessel_i0(x: f64) -> f64 {
    let half = x / 2.0;
    let mut term = 1.0;
    let mut sum = 1.0;
    for k in 1..96 {
        term *= half / f64::from(k);
        let square = term * term;
        sum += square;
        if square < sum * 1e-18 {
            break;
        }
    }
    sum
}

fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        return 1.0;
    }
    let scaled = PI * x;
    scaled.sin() / scaled
}

fn lagrange_weights(between: f64) -> [f64; LAGRANGE_NODES] {
    let before = between + 1.0;
    let after = between - 1.0;
    let further = between - 2.0;
    [
        -between * after * further / 6.0,
        before * after * further / 2.0,
        -before * between * further / 2.0,
        before * between * after / 6.0,
    ]
}

pub(crate) struct WindowedSinc {
    half_taps: f64,
    cutoff: f64,
    beta: f64,
    window_peak: f64,
}

impl WindowedSinc {
    pub(crate) fn new(params: SincParams) -> Self {
        let beta = f64::from(params.kaiser_beta);
        Self {
            half_taps: f64::from(params.half_taps),
            cutoff: f64::from(params.cutoff),
            beta,
            window_peak: bessel_i0(beta),
        }
    }

    pub(crate) fn at(&self, tau: f64) -> f64 {
        if tau >= self.half_taps {
            return 0.0;
        }
        let shoulder = (1.0 - (tau / self.half_taps).powi(2)).max(0.0).sqrt();
        let window = bessel_i0(self.beta * shoulder) / self.window_peak;
        self.cutoff * sinc(self.cutoff * tau) * window
    }
}

enum Shape {
    Linear(WindowedSinc),
    Shaped(Arc<Prototype>),
}

impl Shape {
    fn of(params: SincParams, phase: FilterPhase) -> Self {
        designed(params, phase)
            .map_or_else(|| Self::Linear(WindowedSinc::new(params)), Self::Shaped)
    }

    fn lead(&self) -> f64 {
        match self {
            Self::Linear(windowed) => windowed.half_taps,
            Self::Shaped(prototype) => prototype.lead(),
        }
    }

    fn trail(&self) -> f64 {
        match self {
            Self::Linear(windowed) => windowed.half_taps,
            Self::Shaped(prototype) => prototype.trail(),
        }
    }

    fn at(&self, after: f64) -> f64 {
        match self {
            Self::Linear(windowed) => windowed.at(after.abs()),
            Self::Shaped(prototype) => prototype.at(after),
        }
    }
}

struct Kernel {
    table: Vec<f64>,
    columns: usize,
    points: f64,
}

impl Kernel {
    fn new(shape: &Shape, phases: u32, scale: f64, reach: Reach) -> Self {
        let points = (f64::from(phases) * scale).round().max(1.0);
        let columns = reach.span() + 1;
        let rows = points as usize + LAGRANGE_NODES;
        let behind = reach.behind as f64;

        let table = (0..rows * columns)
            .map(|entry| {
                let fraction = ((entry / columns) as f64 - 1.0) / points;
                let column = (entry % columns) as f64;
                scale * shape.at((fraction + behind - column) * scale)
            })
            .collect();

        Self {
            table,
            columns,
            points,
        }
    }

    fn taper(&self, weights: &mut [f64], fraction: f64, rising: usize) {
        let offset = fraction * self.points;
        let row = offset as usize;
        let columns = self.columns;
        let skipped = 1 - rising;
        let nodes: [&[f64]; LAGRANGE_NODES] = array::from_fn(|node| {
            let start = (row + node) * columns + skipped;
            self.table
                .get(start..(row + node + 1) * columns)
                .unwrap_or_default()
        });
        let [earlier, low, high, later] = nodes;
        let [a, b, c, d] = lagrange_weights(offset - row as f64);
        let taps = earlier.iter().zip(low).zip(high).zip(later);
        for (weight, (((earlier, low), high), later)) in weights.iter_mut().zip(taps) {
            *weight = a * earlier + b * low + c * high + d * later;
        }

        let gain: f64 = weights.iter().sum();
        if gain.is_normal() {
            for weight in weights {
                *weight /= gain;
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Reach {
    behind: usize,
    ahead: usize,
}

impl Reach {
    fn of(shape: &Shape, scale: f64) -> Self {
        Self {
            behind: (shape.trail() / scale).ceil() as usize,
            ahead: (shape.lead() / scale).ceil() as usize,
        }
    }

    const fn span(self) -> usize {
        self.behind + self.ahead
    }

    const fn taps(self, rising: usize) -> usize {
        self.span() + rising
    }
}

struct Tabulated {
    weights: Aligned,
    stride: usize,
}

#[derive(Clone, Copy, PartialEq)]
struct TableFor {
    params: SincParams,
    phase: FilterPhase,
    input_rate: SampleRate,
    output_rate: SampleRate,
}

static LAST_TABULATED: Mutex<Option<(TableFor, Arc<Tabulated>)>> = const_mutex(None);

impl Tabulated {
    fn kept_for(wanted: TableFor, build: impl FnOnce() -> Self) -> Arc<Self> {
        let mut last = LAST_TABULATED.lock();
        if let Some((_, kept)) = last.as_ref().filter(|(held, _)| *held == wanted) {
            return Arc::clone(kept);
        }
        let built = Arc::new(build());
        *last = (built.bytes() <= KEPT_TABLE_BYTES_AT_MOST).then(|| (wanted, Arc::clone(&built)));
        built
    }

    fn bytes(&self) -> usize {
        self.weights.samples.capacity() * size_of::<f64>()
    }

    fn fits(phases: u32, reach: Reach) -> bool {
        padded_row(reach)
            .checked_mul(phases as usize)
            .and_then(|entries| entries.checked_mul(size_of::<f64>()))
            .is_some_and(|bytes| bytes <= TABULATED_WEIGHT_BYTES_AT_MOST)
    }

    fn new(shape: &Shape, scale: f64, reach: Reach, phases: u32) -> Self {
        let stride = padded_row(reach);
        let count = phases as usize;
        let mut table = Aligned::zeroed(stride * count);
        let weights = table.as_mut_slice();

        match shape {
            Shape::Linear(windowed) => {
                Self::mirrored(weights, windowed, scale, reach.behind, phases, stride);
            }
            Shape::Shaped(_) => Self::walked(weights, shape, scale, reach, phases, stride),
        }

        for (phase, padded) in weights.chunks_exact_mut(stride).enumerate() {
            let taps = reach.taps(usize::from(phase == 0));
            let row = padded
                .get_mut(LANES_PER_VECTOR..LANES_PER_VECTOR + taps)
                .unwrap_or_default();
            let gain: f64 = row.iter().sum();
            if gain.is_normal() {
                for weight in row {
                    *weight /= gain;
                }
            }
        }

        Self {
            weights: table,
            stride,
        }
    }

    fn mirrored(
        weights: &mut [f64],
        windowed: &WindowedSinc,
        scale: f64,
        half_width: usize,
        phases: u32,
        stride: usize,
    ) {
        let count = phases as usize;
        for phase in 0..count {
            let fraction = phase as f64 / f64::from(phases);
            let rising = usize::from(phase == 0);
            let behind = stride * phase + LANES_PER_VECTOR + half_width - 1 + rising;
            let ahead = stride * ((count - phase) % count) + LANES_PER_VECTOR + half_width;

            for tap in 0..half_width + rising {
                let weight = windowed.at((tap as f64 + fraction) * scale);
                if let Some(slot) = weights.get_mut(behind - tap) {
                    *slot = weight;
                }
                if let Some(slot) = weights.get_mut(ahead + tap) {
                    *slot = weight;
                }
            }
        }
    }

    fn walked(
        weights: &mut [f64],
        shape: &Shape,
        scale: f64,
        reach: Reach,
        phases: u32,
        stride: usize,
    ) {
        for (phase, padded) in weights.chunks_exact_mut(stride).enumerate() {
            let fraction = phase as f64 / f64::from(phases);
            let rising = usize::from(phase == 0);
            let nearest = (reach.behind + rising) as f64 - 1.0;
            let row = padded
                .get_mut(LANES_PER_VECTOR..LANES_PER_VECTOR + reach.taps(rising))
                .unwrap_or_default();
            for (tap, slot) in row.iter_mut().enumerate() {
                *slot = shape.at((fraction + nearest - tap as f64) * scale);
            }
        }
    }

    fn row(&self, phase: u32) -> &[f64] {
        self.weights
            .window(self.stride * phase as usize, self.stride)
    }
}

const fn padded_row(reach: Reach) -> usize {
    (LANES_PER_VECTOR + reach.span() + ACCUMULATORS).next_multiple_of(LANES_PER_VECTOR)
}

struct Aligned {
    samples: Vec<f64>,
    origin: usize,
    len: usize,
}

impl Aligned {
    fn zeroed(len: usize) -> Self {
        let samples = vec![0.0; len + LANES_PER_VECTOR - 1];
        let offset = samples.as_ptr().align_offset(VECTOR_BYTES);
        Self {
            origin: if offset < LANES_PER_VECTOR { offset } else { 0 },
            samples,
            len,
        }
    }

    fn window(&self, start: usize, len: usize) -> &[f64] {
        if start + len > self.len {
            return &[];
        }
        let from = self.origin + start;
        self.samples.get(from..from + len).unwrap_or_default()
    }

    fn as_slice(&self) -> &[f64] {
        self.samples
            .get(self.origin..self.origin + self.len)
            .unwrap_or_default()
    }

    fn as_mut_slice(&mut self) -> &mut [f64] {
        self.samples
            .get_mut(self.origin..self.origin + self.len)
            .unwrap_or_default()
    }
}

struct History {
    samples: Aligned,
    stride: usize,
    capacity: usize,
}

impl History {
    fn new(channels: usize, capacity: usize) -> Self {
        let stride = (capacity + ACCUMULATORS).next_multiple_of(LANES_PER_VECTOR);
        Self {
            samples: Aligned::zeroed(stride * channels),
            stride,
            capacity,
        }
    }

    fn planes(&self) -> &[f64] {
        self.samples.as_slice()
    }

    fn planes_mut(&mut self) -> &mut [f64] {
        self.samples.as_mut_slice()
    }
}

enum Taps {
    Tabulated(Arc<Tabulated>),
    Interpolated(Kernel),
}

pub struct Resampler {
    config: ResamplerConfig,
    taps: Taps,
    channels: usize,
    ratio: f64,
    reach: Reach,
    history: History,
    lanes: Aligned,
    weights: Vec<f64>,
    filled: usize,
    whole: usize,
    phase: u32,
    phases: u32,
    whole_step: usize,
    phase_step: u32,
}

impl Resampler {
    pub fn new(config: ResamplerConfig) -> Result<Self> {
        Self::with_params(config, config.quality.params())
    }

    pub fn with_params(config: ResamplerConfig, params: SincParams) -> Result<Self> {
        if params.half_taps == 0 || params.half_taps > MAX_HALF_TAPS {
            return Err(Error::FilterLengthOutOfRange {
                requested: u32::from(params.half_taps),
                max: u32::from(MAX_HALF_TAPS),
            });
        }
        if params.phases < MIN_PHASES {
            return Err(Error::FilterLengthOutOfRange {
                requested: params.phases,
                max: u32::from(MAX_HALF_TAPS),
            });
        }

        let ratio = f64::from(config.output_rate.hz()) / f64::from(config.input_rate.hz());
        if ratio < LIMITS.min.as_f64() || ratio > LIMITS.max.as_f64() {
            return Err(Error::RatioOutOfRange {
                from: config.input_rate,
                to: config.output_rate,
                ratio: config.input_rate.ratio_to(config.output_rate),
                limits: LIMITS,
            });
        }

        let scale = ratio.min(1.0);
        let shape = Shape::of(params, config.phase);
        let reach = Reach::of(&shape, scale);
        let channels = config.channels.count().get() as usize;
        let cycle = config.input_rate.ratio_to(config.output_rate);
        let phases = cycle.numer.get();
        let tabulated = Tabulated::fits(phases, reach);
        let taps = if tabulated {
            Taps::Tabulated(Tabulated::kept_for(
                TableFor {
                    params,
                    phase: config.phase,
                    input_rate: config.input_rate,
                    output_rate: config.output_rate,
                },
                || Tabulated::new(&shape, scale, reach, phases),
            ))
        } else {
            Taps::Interpolated(Kernel::new(&shape, params.phases, scale, reach))
        };
        let tapering = if tabulated { 0 } else { padded_row(reach) };

        Ok(Self {
            config,
            taps,
            channels,
            ratio,
            reach,
            history: History::new(channels, config.max_frames_in + reach.span() + 2),
            lanes: Aligned::zeroed(channels * ACCUMULATORS),
            weights: vec![0.0; tapering],
            filled: reach.behind,
            whole: reach.behind,
            phase: 0,
            phases,
            whole_step: (cycle.denom.get() / phases) as usize,
            phase_step: cycle.denom.get() % phases,
        })
    }

    pub const fn ratio(&self) -> f64 {
        self.ratio
    }

    fn accumulate(lanes: &mut Lanes, block: &Lanes, batch: &Lanes) {
        for ((lane, sample), weight) in lanes.iter_mut().zip(block).zip(batch) {
            *lane += sample * weight;
        }
    }

    fn accumulate_one(lanes: &mut Lanes, history: &[f64], weights: &[f64]) {
        let mut running = [0.0; ACCUMULATORS];

        let (blocks, _) = history.as_chunks::<ACCUMULATORS>();
        let (batches, _) = weights.as_chunks::<ACCUMULATORS>();

        for (block, batch) in blocks.iter().zip(batches) {
            Self::accumulate(&mut running, block, batch);
        }

        *lanes = running;
    }

    fn accumulate_pair(lanes: &mut [Lanes; 2], left: &[f64], right: &[f64], weights: &[f64]) {
        let mut left_running = [0.0; ACCUMULATORS];
        let mut right_running = [0.0; ACCUMULATORS];

        let (left_blocks, _) = left.as_chunks::<ACCUMULATORS>();
        let (right_blocks, _) = right.as_chunks::<ACCUMULATORS>();
        let (batches, _) = weights.as_chunks::<ACCUMULATORS>();

        for ((left_block, right_block), batch) in left_blocks.iter().zip(right_blocks).zip(batches)
        {
            Self::accumulate(&mut left_running, left_block, batch);
            Self::accumulate(&mut right_running, right_block, batch);
        }

        *lanes = [left_running, right_running];
    }

    fn summed(lanes: &Lanes) -> f64 {
        let mut partial = *lanes;
        let mut width = ACCUMULATORS;
        while width > 1 {
            width /= 2;
            let (low, high) = partial.split_at_mut(width);
            for (low, high) in low.iter_mut().zip(high.iter()) {
                *low += *high;
            }
        }
        partial.first().copied().unwrap_or_default()
    }

    fn convolve(
        frame: &mut [f64],
        planes: &[f64],
        stride: usize,
        first: usize,
        weights: &[f64],
        lanes: &mut [Lanes],
    ) {
        let span = first..first + weights.len();
        let (pairs, lone) = lanes.as_chunks_mut::<2>();
        let mut paired = planes.chunks_exact(2 * stride);

        for (pair, both) in pairs.iter_mut().zip(paired.by_ref()) {
            let (left, right) = both.split_at(stride);
            Self::accumulate_pair(
                pair,
                left.get(span.clone()).unwrap_or_default(),
                right.get(span.clone()).unwrap_or_default(),
                weights,
            );
        }

        if let Some(single) = lone.first_mut() {
            let history = paired.remainder().get(span).unwrap_or_default();
            Self::accumulate_one(single, history, weights);
        }

        for (slot, lanes) in frame.iter_mut().zip(lanes.iter()) {
            *slot = Self::summed(lanes);
        }
    }

    fn emit(&mut self, output: &mut [f64]) -> usize {
        let Self {
            taps,
            channels,
            reach,
            history,
            lanes,
            weights,
            filled,
            whole,
            phase,
            phases,
            whole_step,
            phase_step,
            ..
        } = self;
        let channels = *channels;
        let reach = *reach;
        let stride = history.stride;
        let planes = history.planes();
        let lanes = lanes.as_mut_slice().as_chunks_mut::<ACCUMULATORS>().0;
        let capacity = output.len() / channels;
        let mut produced = 0;

        while produced < capacity && *filled > 0 {
            let rising = usize::from(*phase == 0);
            if whole.saturating_add(reach.ahead) + (1 - rising) > *filled - 1 {
                break;
            }

            let first = whole.saturating_sub(reach.behind) + (1 - rising);
            let taps_now = reach.taps(rising);
            let aligned = first - first % LANES_PER_VECTOR;
            let lead = first - aligned;
            let covered = (lead + taps_now).next_multiple_of(ACCUMULATORS);

            let padded = match taps {
                Taps::Tabulated(tabulated) => tabulated.row(*phase),
                Taps::Interpolated(kernel) => {
                    let fraction = f64::from(*phase) / f64::from(*phases);
                    let shaped = weights
                        .get_mut(LANES_PER_VECTOR..LANES_PER_VECTOR + taps_now)
                        .unwrap_or_default();
                    kernel.taper(shaped, fraction, rising);
                    if let Some(stale) = weights.get_mut(LANES_PER_VECTOR + taps_now) {
                        *stale = 0.0;
                    }
                    weights.as_slice()
                }
            };
            let from = LANES_PER_VECTOR - lead;
            let taken = padded.get(from..from + covered).unwrap_or_default();

            if let Some(frame) = output.get_mut(produced * channels..(produced + 1) * channels) {
                Self::convolve(frame, planes, stride, aligned, taken, lanes);
            }

            let stepped = *phase + *phase_step;
            let wraps = stepped >= *phases;
            *phase = if wraps { stepped - *phases } else { stepped };
            *whole += *whole_step + usize::from(wraps);
            produced += 1;
        }

        produced
    }

    fn compact(&mut self) {
        let drop_from = self.whole.saturating_sub(self.reach.behind);
        if drop_from == 0 {
            return;
        }
        let filled = self.filled;
        let kept = filled - drop_from;
        let stride = self.history.stride;
        for plane in self.history.planes_mut().chunks_exact_mut(stride) {
            plane.copy_within(drop_from..filled, 0);
            if let Some(vacated) = plane.get_mut(kept..filled) {
                vacated.fill(0.0);
            }
        }
        self.filled -= drop_from;
        self.whole -= drop_from;
    }

    fn append(&mut self, input: &[f64]) -> usize {
        let channels = self.channels;
        let stride = self.history.stride;
        let room = self.history.capacity - self.filled;
        let frames = (input.len() / channels).min(room);
        let filled = self.filled;

        for (channel, plane) in self
            .history
            .planes_mut()
            .chunks_exact_mut(stride)
            .enumerate()
        {
            let Some(destination) = plane.get_mut(filled..filled + frames) else {
                continue;
            };
            for (slot, sample) in destination
                .iter_mut()
                .zip(input.iter().skip(channel).step_by(channels))
            {
                *slot = *sample;
            }
        }

        self.filled += frames;
        frames
    }
}

impl Processor for Resampler {
    fn prepare(&mut self, spec: StreamSpec, max_frames_in: usize) -> Result<usize> {
        if spec.rate != self.config.input_rate {
            return Err(Error::InputRateMismatch {
                expected: self.config.input_rate,
                actual: spec.rate,
            });
        }
        if spec.channel_count() != self.config.channels.count() {
            return Err(Error::ChannelMismatch {
                expected: self.config.channels.count(),
                actual: spec.channel_count(),
            });
        }
        if max_frames_in > self.config.max_frames_in {
            self.config.max_frames_in = max_frames_in;
            self.history = History::new(self.channels, max_frames_in + self.reach.span() + 2);
            self.reset();
        }
        Ok(self.max_output_frames(max_frames_in))
    }

    fn max_output_frames(&self, frames_in: usize) -> usize {
        (frames_in as f64 * self.ratio).ceil() as usize + 1
    }

    fn max_flush_frames(&self) -> usize {
        self.max_output_frames(self.reach.ahead)
    }

    fn reset(&mut self) {
        self.history.planes_mut().fill(0.0);
        self.filled = self.reach.behind;
        self.whole = self.reach.behind;
        self.phase = 0;
    }

    fn output_spec(&self, input: StreamSpec) -> StreamSpec {
        StreamSpec::new(self.config.output_rate, input.channels, input.format)
    }

    fn latency_frames(&self) -> f64 {
        self.reach.ahead as f64 * self.ratio
    }

    fn is_transparent(&self) -> bool {
        self.config.input_rate == self.config.output_rate
    }

    fn process(&mut self, input: &[f64], output: &mut [f64]) -> ProcessCount {
        let frames_in = self.append(input);
        let frames_out = self.emit(output);
        self.compact();
        ProcessCount {
            frames_in,
            frames_out,
        }
    }

    fn flush(&mut self, output: &mut [f64]) -> usize {
        let stride = self.history.stride;
        let room = self.history.capacity - self.filled;
        let pad = self.reach.ahead.min(room);
        let filled = self.filled;

        for plane in self.history.planes_mut().chunks_exact_mut(stride) {
            if let Some(tail) = plane.get_mut(filled..filled + pad) {
                tail.fill(0.0);
            }
        }

        self.filled += pad;
        let produced = self.emit(output);
        self.compact();
        produced
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, f64::consts::TAU};

    use super::*;

    const BLOCK: usize = 1_024;
    const SIXTEEN_BIT_FLOOR_DB: f64 = -96.0;
    const TWENTY_FOUR_BIT_FLOOR_DB: f64 = -140.0;
    const UNITY_WITHIN: f64 = 1e-9;
    const FOLDS_TO_THREE_KILOHERTZ_AT_EIGHT: f64 = 21_000.0;
    const AGREES_WITH_THE_EXACT_SUM_WITHIN: f64 = 1e-12;
    const HIGH_INTERPOLATED_WITHIN: f64 = 5e-8;
    const VERY_HIGH_INTERPOLATED_WITHIN: f64 = 1e-10;
    const SOX_VERY_HIGH_REJECTS_DB: f64 = -175.0;
    const SOX_VERY_HIGH_BANDWIDTH: f64 = 0.95;
    const VERY_HIGH_REJECTS_DB: f64 = -180.0;
    const PASSBAND_FLAT_WITHIN_DB: f64 = 1e-6;
    const HALF_POWER_DB: f64 = -3.010_299_956_639_812;
    const SAMPLED_PER_TAP: u32 = 16;
    const STOPBAND_STEPS: u32 = 6_000;

    fn config(from: SampleRate, to: SampleRate, quality: Quality) -> ResamplerConfig {
        ResamplerConfig {
            input_rate: from,
            output_rate: to,
            channels: ChannelLayout::Mono,
            quality,
            phase: FilterPhase::Linear,
            max_frames_in: BLOCK,
        }
    }

    fn tone(rate: SampleRate, hz: f64, frames: usize) -> Vec<f64> {
        let step = TAU * hz / f64::from(rate.hz());
        (0..frames).map(|n| (step * n as f64).sin()).collect()
    }

    fn run(config: ResamplerConfig, input: &[f64]) -> Vec<f64> {
        let mut resampler = Resampler::new(config).expect("a valid resampler configuration");
        let channels = config.channels.count().get() as usize;
        let mut scratch = vec![0.0; resampler.max_output_frames(BLOCK) * channels];
        let mut output = Vec::new();

        for block in input.chunks(BLOCK * channels) {
            let count = resampler.process(block, &mut scratch);
            assert_eq!(count.frames_in, block.len() / channels);
            output.extend_from_slice(&scratch[..count.frames_out * channels]);
        }
        output
    }

    fn steady(samples: &[f64]) -> &[f64] {
        let skip = samples.len() / 8;
        &samples[skip..samples.len() - skip]
    }

    fn latency(config: ResamplerConfig) -> f64 {
        Resampler::new(config)
            .expect("a valid resampler configuration")
            .latency_frames()
    }

    fn settled(samples: &[f64], latency: f64) -> &[f64] {
        let skip = samples.len() / 8 + latency.ceil() as usize;
        &samples[skip..samples.len() - skip]
    }

    fn alias_level(config: ResamplerConfig, input: &[f64]) -> f64 {
        let output = run(config, input);
        decibels(rms(settled(&output, latency(config))) / rms(steady(input)))
    }

    fn rms(samples: &[f64]) -> f64 {
        let total: f64 = samples.iter().map(|s| s.powi(2)).sum();
        (total / samples.len() as f64).sqrt()
    }

    fn decibels(ratio: f64) -> f64 {
        20.0 * ratio.log10()
    }

    #[test]
    fn a_unity_ratio_resampler_is_transparent_so_the_chain_drops_it() {
        let resampler = Resampler::new(config(
            SampleRate::HZ_96000,
            SampleRate::HZ_96000,
            Quality::Balanced,
        ))
        .expect("a valid resampler configuration");

        assert!(resampler.is_transparent());
    }

    #[test]
    fn a_changed_rate_is_never_transparent() {
        let resampler = Resampler::new(config(
            SampleRate::HZ_44100,
            SampleRate::HZ_48000,
            Quality::Balanced,
        ))
        .expect("a valid resampler configuration");

        assert!(!resampler.is_transparent());
        assert_eq!(
            resampler
                .output_spec(StreamSpec::new(
                    SampleRate::HZ_44100,
                    ChannelLayout::Mono,
                    resonate_core::SampleFormat::F32,
                ))
                .rate,
            SampleRate::HZ_48000
        );
    }

    #[test]
    fn the_output_frame_count_tracks_the_conversion_ratio() {
        let config = config(SampleRate::HZ_44100, SampleRate::HZ_88200, Quality::Fast);
        let produced = run(config, &vec![0.0; BLOCK * 8]).len();
        let expected = BLOCK * 8 * 2;

        assert!(
            produced.abs_diff(expected) <= 2 * BLOCK,
            "produced {produced}, expected about {expected}"
        );
    }

    #[test]
    fn direct_current_passes_through_at_unity_level() {
        for (from, to) in RATE_PAIRS {
            for quality in QUALITIES {
                let config = config(from, to, quality);
                let output = run(config, &vec![1.0; BLOCK * 8]);

                for sample in settled(&output, latency(config)) {
                    assert!(
                        (*sample - 1.0).abs() < UNITY_WITHIN,
                        "{from} -> {to} {quality:?} drifted DC to {sample}"
                    );
                }
            }
        }
    }

    const SHAPED: [FilterPhase; 2] = [FilterPhase::Minimum, FilterPhase::Intermediate];
    const EVERY_PHASE: [FilterPhase; 3] = [
        FilterPhase::Linear,
        FilterPhase::Minimum,
        FilterPhase::Intermediate,
    ];
    const RINGS_BEFORE_WITHIN_FRAMES: usize = 3;
    const MINIMUM_RINGS_QUIETER_BY_DB: f64 = 40.0;
    const INTERMEDIATE_RINGS_QUIETER_BY_DB: f64 = 10.0;

    fn shaped(
        from: SampleRate,
        to: SampleRate,
        quality: Quality,
        phase: FilterPhase,
    ) -> ResamplerConfig {
        ResamplerConfig {
            phase,
            ..config(from, to, quality)
        }
    }

    fn impulse_through(config: ResamplerConfig) -> Vec<f64> {
        let mut input = vec![0.0; BLOCK * 4];
        if let Some(slot) = input.get_mut(BLOCK * 2) {
            *slot = 1.0;
        }
        resampled_and_flushed(config, &input)
    }

    fn energy(samples: &[f64]) -> f64 {
        samples.iter().map(|sample| sample * sample).sum()
    }

    fn spanned(config: ResamplerConfig) -> f64 {
        let resampler = Resampler::new(config).expect("a valid resampler configuration");
        resampler.reach.span() as f64 * resampler.ratio
    }

    fn resampled_and_flushed_once(config: ResamplerConfig, input: &[f64]) -> usize {
        let mut resampler = Resampler::new(config).expect("a valid resampler configuration");
        let channels = config.channels.count().get() as usize;
        let mut scratch = vec![0.0; resampler.max_output_frames(BLOCK) * channels];
        let mut frames = 0;
        for block in input.chunks(BLOCK * channels) {
            frames += resampler.process(block, &mut scratch).frames_out;
        }
        let mut tail = vec![0.0; resampler.max_flush_frames() * channels];
        frames + resampler.flush(&mut tail)
    }

    #[test]
    fn a_minimum_phase_filter_rings_only_after_what_it_answers() {
        for quality in [Quality::High, Quality::VeryHigh] {
            let mut before_the_peak = Vec::new();
            for phase in EVERY_PHASE {
                let config = shaped(SampleRate::HZ_44100, SampleRate::HZ_48000, quality, phase);
                let output = impulse_through(config);
                let peak = output
                    .iter()
                    .enumerate()
                    .max_by(|(_, left), (_, right)| left.abs().total_cmp(&right.abs()))
                    .map_or(0, |(index, _)| index);
                let early = peak.saturating_sub(RINGS_BEFORE_WITHIN_FRAMES);
                let ringing = energy(&output[..early]) / energy(&output);
                before_the_peak.push((phase, decibels(ringing.sqrt())));

                let expected = (BLOCK * 2) as f64 * 48_000.0 / 44_100.0;
                assert!(
                    (peak as f64 - expected).abs() <= 2.0,
                    "{quality:?} {phase:?} answered an impulse at {peak} where it sits at {expected}"
                );
            }

            let [(_, linear), (_, minimum), (_, intermediate)] = before_the_peak[..] else {
                panic!("three phases were measured");
            };
            assert!(
                minimum < linear - MINIMUM_RINGS_QUIETER_BY_DB,
                "{quality:?} minimum phase rang at {minimum:.1} dB before the peak, linear {linear:.1}"
            );
            assert!(
                intermediate < linear - INTERMEDIATE_RINGS_QUIETER_BY_DB && intermediate > minimum,
                "{quality:?} intermediate rang at {intermediate:.1} dB, linear {linear:.1}, minimum {minimum:.1}"
            );
        }
    }

    #[test]
    fn every_phase_passes_direct_current_keeps_the_passband_and_rejects_aliases() {
        let tone_in_the_passband = tone(SampleRate::HZ_44100, 1_000.0, BLOCK * 16);
        let tone_past_nyquist = tone(SampleRate::HZ_44100, 15_000.0, BLOCK * 16);
        for (quality, floor) in [
            (Quality::High, TWENTY_FOUR_BIT_FLOOR_DB),
            (Quality::VeryHigh, SOX_VERY_HIGH_REJECTS_DB),
        ] {
            for phase in SHAPED {
                for (from, to) in RATE_PAIRS {
                    let config = shaped(from, to, quality, phase);
                    let output = run(config, &vec![1.0; BLOCK * 16]);
                    for sample in settled(&output, spanned(config)) {
                        assert!(
                            (*sample - 1.0).abs() < UNITY_WITHIN,
                            "{from} -> {to} {quality:?} {phase:?} drifted DC to {sample}"
                        );
                    }
                }

                let config = shaped(SampleRate::HZ_44100, SampleRate::HZ_48000, quality, phase);
                let output = run(config, &tone_in_the_passband);
                let ripple = decibels(
                    rms(settled(&output, spanned(config))) / rms(steady(&tone_in_the_passband)),
                )
                .abs();
                assert!(
                    ripple < 0.01,
                    "{quality:?} {phase:?} rippled {ripple:.4} dB"
                );

                let config = shaped(SampleRate::HZ_44100, SampleRate::HZ_22050, quality, phase);
                let output = run(config, &tone_past_nyquist);
                let rejection = decibels(
                    rms(settled(&output, spanned(config))) / rms(steady(&tone_past_nyquist)),
                );
                assert!(
                    rejection < floor,
                    "{quality:?} {phase:?} rejected only {rejection:.1} dB"
                );
            }
        }
    }

    #[test]
    fn a_track_keeps_its_length_whatever_the_phase() {
        let input = oracle_signal(SampleRate::HZ_44100, 2, 3 * BLOCK + 417);
        for quality in [Quality::High, Quality::VeryHigh] {
            let lengths: Vec<usize> = EVERY_PHASE
                .iter()
                .map(|phase| {
                    let config = ResamplerConfig {
                        channels: ChannelLayout::Stereo,
                        ..shaped(SampleRate::HZ_44100, SampleRate::HZ_48000, quality, *phase)
                    };
                    resampled_and_flushed_once(config, &input)
                })
                .collect();
            assert!(
                lengths.windows(2).all(|pair| pair[0] == pair[1]),
                "{quality:?} came out {lengths:?} long across the phases"
            );
        }
    }

    #[test]
    fn a_minimum_phase_filter_waits_for_a_few_frames_rather_than_half_its_length() {
        let linear = latency(shaped(
            SampleRate::HZ_44100,
            SampleRate::HZ_48000,
            Quality::VeryHigh,
            FilterPhase::Linear,
        ));
        let minimum = latency(shaped(
            SampleRate::HZ_44100,
            SampleRate::HZ_48000,
            Quality::VeryHigh,
            FilterPhase::Minimum,
        ));
        assert!(minimum < 12.0, "minimum phase waits {minimum} frames");
        assert!(linear > 300.0);
    }

    fn an_odd_rate() -> SampleRate {
        SampleRate::new(47_993).expect("a rate in range")
    }

    #[test]
    fn a_cycle_too_long_to_tabulate_still_resamples_through_the_interpolated_kernel() {
        let config = config(SampleRate::HZ_44100, an_odd_rate(), Quality::High);
        let resampler = Resampler::new(config).expect("a valid configuration");
        assert!(
            matches!(resampler.taps, Taps::Interpolated(_)),
            "44.1 kHz onto 47.993 kHz should exceed the tabulated weight bound"
        );

        let input = tone(SampleRate::HZ_44100, 1_000.0, BLOCK * 8);
        let output = run(config, &input);

        let gain = decibels(rms(steady(&output)) / rms(steady(&input)));
        assert!(gain.abs() < 0.2, "1 kHz changed level by {gain:.3} dB");
    }

    const EVERY_RATE: [SampleRate; 10] = [
        SampleRate::HZ_8000,
        SampleRate::HZ_22050,
        SampleRate::HZ_44100,
        SampleRate::HZ_48000,
        SampleRate::HZ_88200,
        SampleRate::HZ_96000,
        SampleRate::HZ_176400,
        SampleRate::HZ_192000,
        SampleRate::HZ_352800,
        SampleRate::HZ_384000,
    ];

    const TRIES_WHILE_OTHER_TESTS_BUILD: usize = 64;

    fn table_of(resampler: &Resampler) -> Option<&Arc<Tabulated>> {
        match &resampler.taps {
            Taps::Tabulated(table) => Some(table),
            Taps::Interpolated(_) => None,
        }
    }

    #[test]
    fn a_rebind_at_the_same_rates_takes_the_table_already_built() {
        let wanted = config(SampleRate::HZ_44100, SampleRate::HZ_48000, Quality::High);
        let built_back_to_back = || {
            let first = Resampler::new(wanted).expect("a supported conversion");
            let again = Resampler::new(wanted).expect("a supported conversion");
            table_of(&first)
                .zip(table_of(&again))
                .is_some_and(|(first, again)| Arc::ptr_eq(first, again))
        };
        assert!(
            (0..TRIES_WHILE_OTHER_TESTS_BUILD).any(|_| built_back_to_back()),
            "the same conversion built its table twice"
        );

        let first = Resampler::new(wanted).expect("a supported conversion");

        let elsewhere = Resampler::new(config(
            SampleRate::HZ_48000,
            SampleRate::HZ_44100,
            Quality::High,
        ))
        .expect("a supported conversion");
        assert!(
            table_of(&first)
                .zip(table_of(&elsewhere))
                .is_some_and(|(first, elsewhere)| !Arc::ptr_eq(first, elsewhere)),
            "another conversion was handed a table built for this one"
        );
    }

    #[test]
    fn every_rate_pair_tabulates_its_phases_at_every_quality() {
        for quality in QUALITIES {
            let mut interpolated = Vec::new();
            for from in EVERY_RATE {
                for to in EVERY_RATE {
                    let config = config(from, to, quality);
                    let Ok(resampler) = Resampler::new(config) else {
                        continue;
                    };
                    if matches!(resampler.taps, Taps::Interpolated(_)) {
                        interpolated.push((from.hz(), to.hz()));
                    }
                }
            }

            assert!(
                interpolated.is_empty(),
                "{quality:?} left {interpolated:?} to the interpolated kernel"
            );
        }
    }

    #[test]
    fn a_tone_well_inside_the_passband_keeps_its_level() {
        let config = config(
            SampleRate::HZ_44100,
            SampleRate::HZ_48000,
            Quality::Balanced,
        );
        let input = tone(SampleRate::HZ_44100, 1_000.0, BLOCK * 16);
        let output = run(config, &input);

        let gain = decibels(rms(steady(&output)) / rms(steady(&input)));
        assert!(gain.abs() < 0.1, "1 kHz changed level by {gain:.3} dB");
    }

    #[test]
    fn a_tone_above_the_output_nyquist_is_rejected() {
        let input = tone(SampleRate::HZ_44100, 15_000.0, BLOCK * 16);
        let rejection = alias_level(
            config(
                SampleRate::HZ_44100,
                SampleRate::HZ_22050,
                Quality::Balanced,
            ),
            &input,
        );

        assert!(
            rejection < -80.0,
            "15 kHz survived downsampling at {rejection:.1} dB"
        );
    }

    #[test]
    fn high_quality_rejects_aliases_harder_than_fast() {
        let input = tone(SampleRate::HZ_44100, 15_000.0, BLOCK * 16);

        let fast = alias_level(
            config(SampleRate::HZ_44100, SampleRate::HZ_22050, Quality::Fast),
            &input,
        );
        let high = alias_level(
            config(SampleRate::HZ_44100, SampleRate::HZ_22050, Quality::High),
            &input,
        );

        assert!(
            high < fast,
            "high rejected {high:.1} dB, fast rejected {fast:.1} dB"
        );
    }

    #[test]
    fn channels_are_resampled_independently() {
        let config = ResamplerConfig {
            channels: ChannelLayout::Stereo,
            ..config(
                SampleRate::HZ_44100,
                SampleRate::HZ_48000,
                Quality::Balanced,
            )
        };
        let mut input = vec![0.0; BLOCK * 8 * 2];
        for (index, sample) in input.iter_mut().enumerate() {
            *sample = if index % 2 == 0 { 1.0 } else { -1.0 };
        }
        let output = run(config, &input);

        for frame in steady(&output).as_chunks::<2>().0 {
            assert!((frame[0] - 1.0).abs() < 1e-3, "left was {}", frame[0]);
            assert!((frame[1] + 1.0).abs() < 1e-3, "right was {}", frame[1]);
        }
    }

    const RATE_PAIRS: [(SampleRate, SampleRate); 6] = [
        (SampleRate::HZ_44100, SampleRate::HZ_48000),
        (SampleRate::HZ_48000, SampleRate::HZ_44100),
        (SampleRate::HZ_44100, SampleRate::HZ_192000),
        (SampleRate::HZ_192000, SampleRate::HZ_44100),
        (SampleRate::HZ_88200, SampleRate::HZ_96000),
        (SampleRate::HZ_96000, SampleRate::HZ_88200),
    ];

    const QUALITIES: [Quality; 4] = [
        Quality::Fast,
        Quality::Balanced,
        Quality::High,
        Quality::VeryHigh,
    ];

    fn drained(config: ResamplerConfig, blocks: usize) -> Resampler {
        let mut resampler = Resampler::new(config).expect("a valid resampler configuration");
        let mut scratch = vec![0.0; resampler.max_output_frames(BLOCK)];
        let input = tone(config.input_rate, 1_000.0, BLOCK * blocks);

        for block in input.chunks(BLOCK) {
            let count = resampler.process(block, &mut scratch);
            assert_eq!(
                count.frames_in,
                block.len(),
                "the block was not taken whole"
            );
        }
        resampler
    }

    #[test]
    fn a_flush_never_produces_more_than_the_bound_it_advertises() {
        for (from, to) in RATE_PAIRS {
            for quality in QUALITIES {
                let mut resampler = drained(config(from, to, quality), 8);
                let bound = resampler.max_flush_frames();

                let mut tail = vec![0.0; bound * 4 + BLOCK];
                let produced = resampler.flush(&mut tail);
                assert!(
                    produced <= bound,
                    "{from} -> {to} {quality:?} flushed {produced} against a bound of {bound}"
                );
            }
        }
    }

    #[test]
    fn the_flush_bound_follows_the_filter_rather_than_the_block_size() {
        for (from, to) in RATE_PAIRS {
            for quality in QUALITIES {
                let small = Resampler::new(config(from, to, quality))
                    .expect("a valid resampler configuration");
                let large = Resampler::new(ResamplerConfig {
                    max_frames_in: BLOCK * 16,
                    ..config(from, to, quality)
                })
                .expect("a valid resampler configuration");

                assert_eq!(
                    small.max_flush_frames(),
                    large.max_flush_frames(),
                    "{from} -> {to} {quality:?} sized its flush by the caller's block"
                );
            }
        }
    }

    #[test]
    fn a_flush_bounded_to_its_own_advertised_room_still_drains_the_history() {
        for (from, to) in RATE_PAIRS {
            let config = config(from, to, Quality::High);
            let mut resampler = drained(config, 8);
            let bound = resampler.max_flush_frames();

            let mut tail = vec![0.0; bound];
            let produced = resampler.flush(&mut tail);
            assert!(
                produced > 0,
                "{from} -> {to} emitted nothing from a full history"
            );

            let mut again = vec![0.0; bound * 4];
            let second = resampler.flush(&mut again);
            assert!(
                second <= bound,
                "{from} -> {to} left {second} frames behind a bounded flush"
            );
        }
    }

    #[test]
    fn a_ratio_beyond_the_supported_range_is_rejected() {
        let config = ResamplerConfig {
            max_frames_in: BLOCK,
            ..config(SampleRate::HZ_384000, SampleRate::HZ_8000, Quality::Fast)
        };
        match Resampler::new(config) {
            Err(Error::RatioOutOfRange { .. }) => {}
            Err(other) => panic!("expected RatioOutOfRange, got {other:?}"),
            Ok(_) => panic!("48:1 should be past the supported ratio range"),
        }
    }

    #[test]
    fn each_quality_meets_its_alias_rejection_floor() {
        let input = tone(SampleRate::HZ_44100, 15_000.0, BLOCK * 16);

        for (quality, floor) in [
            (Quality::Fast, -65.0),
            (Quality::Balanced, SIXTEEN_BIT_FLOOR_DB),
            (Quality::High, TWENTY_FOUR_BIT_FLOOR_DB),
            (Quality::VeryHigh, SOX_VERY_HIGH_REJECTS_DB),
        ] {
            let rejection = alias_level(
                config(SampleRate::HZ_44100, SampleRate::HZ_22050, quality),
                &input,
            );
            assert!(
                rejection < floor,
                "{quality:?} rejected only {rejection:.1} dB, needs better than {floor:.1} dB"
            );
        }
    }

    #[test]
    fn an_extreme_downsample_still_tabulates_the_filter_finely_enough() {
        let input = tone(
            SampleRate::HZ_192000,
            FOLDS_TO_THREE_KILOHERTZ_AT_EIGHT,
            BLOCK * 32,
        );

        let rejection = alias_level(
            config(SampleRate::HZ_192000, SampleRate::HZ_8000, Quality::High),
            &input,
        );

        assert!(
            rejection < TWENTY_FOUR_BIT_FLOOR_DB,
            "24:1 rejected only {rejection:.1} dB"
        );
    }

    #[test]
    fn every_quality_keeps_the_passband_flat() {
        let input = tone(SampleRate::HZ_44100, 1_000.0, BLOCK * 16);
        let reference = rms(steady(&input));

        for quality in QUALITIES {
            let output = run(
                config(SampleRate::HZ_44100, SampleRate::HZ_48000, quality),
                &input,
            );
            let ripple = decibels(rms(steady(&output)) / reference).abs();
            assert!(ripple < 0.01, "{quality:?} rippled {ripple:.4} dB at 1 kHz");
        }
    }

    const ORACLE_PAIRS: [(SampleRate, SampleRate); 4] = [
        (SampleRate::HZ_44100, SampleRate::HZ_48000),
        (SampleRate::HZ_48000, SampleRate::HZ_44100),
        (SampleRate::HZ_96000, SampleRate::HZ_48000),
        (SampleRate::HZ_192000, SampleRate::HZ_44100),
    ];

    fn modified_bessel_of_order_zero(x: f64) -> f64 {
        let quarter_square = x * x / 4.0;
        let mut term = 1.0;
        let mut sum = 1.0;
        let mut k = 1.0;
        while term > sum * f64::EPSILON {
            term *= quarter_square / (k * k);
            sum += term;
            k += 1.0;
        }
        sum
    }

    fn analytic_tap(params: SincParams, tau: f64) -> f64 {
        let half_taps = f64::from(params.half_taps);
        if tau >= half_taps {
            return 0.0;
        }
        let cutoff = f64::from(params.cutoff);
        let beta = f64::from(params.kaiser_beta);
        let argument = PI * cutoff * tau;
        let sinc = if argument == 0.0 {
            1.0
        } else {
            argument.sin() / argument
        };
        let shoulder = (1.0 - (tau / half_taps).powi(2)).sqrt();
        let window =
            modified_bessel_of_order_zero(beta * shoulder) / modified_bessel_of_order_zero(beta);
        cutoff * sinc * window
    }

    struct Oracle {
        params: SincParams,
        phases: i64,
        advance: i64,
        scale: f64,
        reach: i64,
        rows: BTreeMap<i64, Vec<f64>>,
    }

    impl Oracle {
        fn new(config: ResamplerConfig) -> Self {
            let params = config.quality.params();
            let cycle = config.input_rate.ratio_to(config.output_rate);
            let scale =
                (f64::from(config.output_rate.hz()) / f64::from(config.input_rate.hz())).min(1.0);
            Self {
                params,
                phases: i64::from(cycle.numer.get()),
                advance: i64::from(cycle.denom.get()),
                scale,
                reach: (f64::from(params.half_taps) / scale).ceil() as i64 + 1,
                rows: BTreeMap::new(),
            }
        }

        fn instant(&mut self, frame: i64) -> (i64, &[f64]) {
            let position = frame * self.advance;
            let first = position / self.phases - self.reach;
            let fraction = position % self.phases;
            let Self {
                params,
                phases,
                scale,
                reach,
                rows,
                ..
            } = self;
            let row = rows.entry(fraction).or_insert_with(|| {
                let raw: Vec<f64> = (-*reach..=*reach)
                    .map(|offset| {
                        let distance = (fraction - offset * *phases).abs() as f64 / *phases as f64;
                        analytic_tap(*params, distance * *scale)
                    })
                    .collect();
                let gain: f64 = raw.iter().sum();
                raw.into_iter().map(|weight| weight / gain).collect()
            });
            (first, row)
        }
    }

    fn oracle_signal(rate: SampleRate, channels: usize, frames: usize) -> Vec<f64> {
        let hz = f64::from(rate.hz());
        let mut noise: u64 = 0x5265_736F_6E61_7465;
        (0..frames * channels)
            .map(|index| {
                let frame = (index / channels) as f64;
                let channel = (index % channels) as f64;
                noise ^= noise << 13;
                noise ^= noise >> 7;
                noise ^= noise << 17;
                let hiss = (noise >> 40) as f64 / 8_388_608.0 - 1.0;
                let low = (TAU * (440.0 + 330.0 * channel) * frame / hz + channel).sin();
                let high = (TAU * 0.41 * frame + 2.0 * channel).sin();
                0.45 * low + 0.3 * high + 0.2 * hiss
            })
            .collect()
    }

    fn resampled_and_flushed(config: ResamplerConfig, input: &[f64]) -> Vec<f64> {
        let mut resampler = Resampler::new(config).expect("a valid resampler configuration");
        let channels = config.channels.count().get() as usize;
        let mut output = Vec::new();
        let mut scratch = vec![0.0; resampler.max_output_frames(BLOCK) * channels];

        for block in input.chunks(BLOCK * channels) {
            let count = resampler.process(block, &mut scratch);
            assert_eq!(count.frames_in, block.len() / channels);
            output.extend_from_slice(&scratch[..count.frames_out * channels]);
        }
        for _ in 0..2 {
            let mut tail = vec![0.0; resampler.max_flush_frames() * channels];
            let produced = resampler.flush(&mut tail);
            output.extend_from_slice(&tail[..produced * channels]);
        }
        output
    }

    fn assert_matches_the_oracle(from: SampleRate, to: SampleRate, quality: Quality, within: f64) {
        let frames = 3 * BLOCK + 417;
        let mut oracle = Oracle::new(config(from, to, quality));

        for layout in [
            ChannelLayout::Mono,
            ChannelLayout::Stereo,
            ChannelLayout::Discrete(resonate_core::ChannelCount::new(3).expect("three channels")),
            ChannelLayout::Surround51,
        ] {
            let config = ResamplerConfig {
                channels: layout,
                ..config(from, to, quality)
            };
            let channels = layout.count().get() as usize;
            let input = oracle_signal(from, channels, frames);
            let output = resampled_and_flushed(config, &input);

            for (index, frame) in output.chunks_exact(channels).enumerate() {
                let (first, weights) = oracle.instant(index as i64);
                for (channel, sample) in frame.iter().enumerate() {
                    let exact: f64 = weights
                        .iter()
                        .zip(first..)
                        .filter(|(_, source)| (0..frames as i64).contains(source))
                        .map(|(weight, source)| {
                            weight * input[source as usize * channels + channel]
                        })
                        .sum();
                    assert!(
                        (*sample - exact).abs() < within,
                        "{from} -> {to} {quality:?} {layout:?} frame {index} channel {channel}: {sample} against {exact}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_frame_is_the_exact_windowed_sinc_normalised_at_its_instant() {
        for (from, to) in ORACLE_PAIRS {
            for quality in QUALITIES {
                assert_matches_the_oracle(from, to, quality, AGREES_WITH_THE_EXACT_SUM_WITHIN);
            }
        }
    }

    #[test]
    fn an_interpolated_kernel_stays_within_the_step_its_window_ends_on() {
        for (from, to) in [
            (SampleRate::HZ_44100, an_odd_rate()),
            (an_odd_rate(), SampleRate::HZ_44100),
        ] {
            for (quality, within) in [
                (Quality::High, HIGH_INTERPOLATED_WITHIN),
                (Quality::VeryHigh, VERY_HIGH_INTERPOLATED_WITHIN),
            ] {
                let resampler =
                    Resampler::new(config(from, to, quality)).expect("a valid configuration");
                assert!(matches!(resampler.taps, Taps::Interpolated(_)));
                assert_matches_the_oracle(from, to, quality, within);
            }
        }
    }

    fn continuous_response(params: SincParams, nyquists: f64) -> f64 {
        let points = f64::from(SAMPLED_PER_TAP);
        let reach = i64::from(params.half_taps) * i64::from(SAMPLED_PER_TAP);
        let (mut real, mut imaginary, mut sum) = (0.0, 0.0, 0.0);
        for index in -reach..=reach {
            let tau = index as f64 / points;
            let weight = analytic_tap(params, tau.abs());
            let angle = PI * nyquists * tau;
            real += weight * angle.cos();
            imaginary += weight * angle.sin();
            sum += weight;
        }
        real.hypot(imaginary) / sum
    }

    #[test]
    fn very_high_beats_sox_very_high_on_paper() {
        let params = Quality::VeryHigh.params();
        let at = |nyquists: f64| decibels(continuous_response(params, nyquists));

        let flattest = (0..=950)
            .map(|step| at(f64::from(step) / 1_000.0).abs())
            .fold(0.0, f64::max);
        assert!(
            flattest < PASSBAND_FLAT_WITHIN_DB,
            "the passband strays {flattest:e} dB before 95 % of Nyquist"
        );

        let half_power = (9_500..10_000)
            .map(|step| f64::from(step) / 10_000.0)
            .find(|&nyquists| at(nyquists) < HALF_POWER_DB)
            .unwrap_or(1.0);
        assert!(
            half_power >= SOX_VERY_HIGH_BANDWIDTH,
            "the response is 3 dB down at {half_power} of Nyquist"
        );

        let loudest = (0..=STOPBAND_STEPS)
            .map(|step| at(1.0 + f64::from(step) / f64::from(STOPBAND_STEPS) * 3.0))
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            loudest < VERY_HIGH_REJECTS_DB,
            "the stopband reaches {loudest:.1} dB at or past Nyquist"
        );
    }
}
