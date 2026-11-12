use std::{array, f64::consts::PI, mem, num::NonZeroU32, sync::Arc};

use parking_lot::Mutex;
use resonate_core::{ChannelLayout, Ratio, SampleRate, StreamSpec};
use wide::f64x8;

use crate::{
    Error, FilterPhase, ProcessCount, Processor, RatioLimits, Result,
    phase::{Prototype, designed},
    spectral::{Blocks, Conversion, Design, Plan, Spectral},
};

const MAX_HALF_TAPS: u16 = 2_048;
const MIN_PHASES: u32 = 8;
const LANES_PER_VECTOR: usize = if cfg!(target_feature = "avx512f") {
    8
} else if cfg!(target_feature = "avx") {
    4
} else {
    2
};
const WIDEST_LANES_PER_VECTOR: usize = 8;
const VECTOR_BYTES: usize = LANES_PER_VECTOR * size_of::<f64>();
const ACCUMULATORS: usize = 8;
const FRAMES_AT_ONCE: usize = 4;
const _: () =
    assert!(ACCUMULATORS.is_power_of_two() && ACCUMULATORS.is_multiple_of(LANES_PER_VECTOR));
const _: () = assert!(LANES_PER_VECTOR <= WIDEST_LANES_PER_VECTOR);
const TABULATED_WEIGHT_BYTES_AT_MOST: usize = 16 << 20;
const KEPT_TABLE_BYTES_AT_MOST: usize = 4 << 20;
const LAGRANGE_NODES: usize = 4;
const SPECTRAL_FROM_HALF_TAPS: u16 = 48;
const STEEP_FROM_RATIO: f64 = 2.0;
const SHAPED_CLEARS_A_SHARE_OF_THE_REACH: u16 = 8;
const SPECTRAL_HOP_SECONDS_AT_MOST: f64 = 0.4;
const OVERSAMPLED: usize = 2;
const INTERPOLATOR_PHASES: u32 = 1_024;
const INTERPOLATOR_CUTOFF: f32 = 1.0;
const INTERPOLATOR_TRANSITION_CYCLES: f64 = 0.5;
const INTERPOLATOR_UNDER_THE_STOPBAND_DB: f64 = 60.0;
const KAISER_BETA_PER_DECIBEL: f64 = 0.1102;
const KAISER_DECIBELS_AT_NO_BETA: f64 = 8.7;
const KAISER_LENGTH_DECIBELS: f64 = 7.95;
const KAISER_LENGTH_PER_DECIBEL: f64 = 14.36;

const BALANCED_WINDOW: [f64; 10] = [
    0.30671382544860293,
    -0.6376531743318821,
    0.5584325253254765,
    -0.3395642911942146,
    0.15328081311843028,
    -0.05294841496722559,
    0.014286134908432835,
    -0.0030933373788818025,
    0.0005199393803197172,
    -9.557476880843463e-05,
];

const HIGH_WINDOW: [f64; 13] = [
    0.24053709856192423,
    -0.5390856442241267,
    0.5625869723081898,
    -0.4488451520698929,
    0.2895782064941611,
    -0.15317479422792973,
    0.0666645101503116,
    -0.023888384626784803,
    0.00704207894887969,
    -0.0017005811900209916,
    0.0003327233059389617,
    -5.070433295871479e-05,
    5.748985880585367e-06,
];

const VERY_HIGH_WINDOW: [f64; 25] = [
    0.15861352245935742,
    -0.3789587164950704,
    0.4598993501671325,
    -0.4672157606244562,
    0.4222783558800702,
    -0.34547974797854814,
    0.25678622511337307,
    -0.17285651405519822,
    0.10468971848162832,
    -0.05663589043696097,
    0.02721201786962143,
    -0.011574395475138928,
    0.004354854447432262,
    -0.0014511993108578117,
    0.00042943980255349945,
    -0.0001132469173140693,
    2.6717495111419625e-05,
    -5.663875087186242e-06,
    1.082325391824068e-06,
    -1.8805154467643742e-07,
    2.918270090163099e-08,
    -4.481586150795191e-09,
    4.159418434679373e-10,
    -1.3466031067999038e-10,
    -1.2254149084345297e-11,
];

fn under_the_stopband(params: SincParams) -> f64 {
    f64::from(params.rejects_db) + INTERPOLATOR_UNDER_THE_STOPBAND_DB
}

fn kaiser_rejecting(rejected_db: f64, transition_cycles: f64, cutoff: f64) -> SincParams {
    let taps = ((rejected_db - KAISER_LENGTH_DECIBELS)
        / (KAISER_LENGTH_PER_DECIBEL * transition_cycles))
        .ceil()
        + 1.0;
    SincParams {
        half_taps: (taps / 2.0).ceil() as u16,
        phases: INTERPOLATOR_PHASES,
        cutoff: cutoff as f32,
        kaiser_beta: (KAISER_BETA_PER_DECIBEL * (rejected_db - KAISER_DECIBELS_AT_NO_BETA)) as f32,
        rejects_db: rejected_db as f32,
        clear_taps: 0,
        hop_taps: 0,
        steep_hop_taps: 0,
        window: &[],
    }
}

fn interpolator_for(params: SincParams) -> SincParams {
    kaiser_rejecting(
        under_the_stopband(params),
        INTERPOLATOR_TRANSITION_CYCLES,
        f64::from(INTERPOLATOR_CUTOFF),
    )
}

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
    LowLatency,
    Balanced,
    #[default]
    High,
    VeryHigh,
}

impl Quality {
    pub const fn params(self) -> SincParams {
        match self {
            Self::LowLatency => SincParams {
                half_taps: 47,
                phases: 512,
                cutoff: 0.940,
                kaiser_beta: 9.0,
                rejects_db: 90.4,
                clear_taps: 0,
                hop_taps: 0,
                steep_hop_taps: 0,
                window: &[],
            },
            Self::Balanced => SincParams {
                half_taps: 110,
                phases: 512,
                cutoff: 0.956_916,
                kaiser_beta: 14.888_727,
                rejects_db: 130.0,
                clear_taps: 0,
                hop_taps: 870,
                steep_hop_taps: 1_600,
                window: &BALANCED_WINDOW,
            },
            Self::High => SincParams {
                half_taps: 170,
                phases: 1024,
                cutoff: 0.955_782,
                kaiser_beta: 23.615_373,
                rejects_db: 171.0,
                clear_taps: 0,
                hop_taps: 1_960,
                steep_hop_taps: 3_600,
                window: &HIGH_WINDOW,
            },
            Self::VeryHigh => SincParams {
                half_taps: 1_200,
                phases: 1024,
                cutoff: 0.991_383,
                kaiser_beta: 32.484_497,
                rejects_db: 218.0,
                clear_taps: 0,
                hop_taps: 3_300,
                steep_hop_taps: 16_000,
                window: &VERY_HIGH_WINDOW,
            },
        }
    }

    pub const fn shaped_params(self) -> SincParams {
        let params = self.params();
        match self {
            Self::LowLatency => params,
            Self::Balanced | Self::High | Self::VeryHigh => SincParams {
                clear_taps: params.half_taps / SHAPED_CLEARS_A_SHARE_OF_THE_REACH,
                ..params
            },
        }
    }

    pub const fn params_for(self, phase: FilterPhase) -> SincParams {
        match phase {
            FilterPhase::Linear => self.params(),
            FilterPhase::Intermediate | FilterPhase::Minimum => self.shaped_params(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SincParams {
    pub half_taps: u16,
    pub phases: u32,
    pub cutoff: f32,
    pub kaiser_beta: f32,
    pub rejects_db: f32,
    pub clear_taps: u16,
    pub hop_taps: u16,
    pub steep_hop_taps: u16,
    pub window: &'static [f64],
}

impl SincParams {
    pub const fn converts_in_blocks(self) -> bool {
        self.half_taps >= SPECTRAL_FROM_HALF_TAPS
    }
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

const fn greatest_common_divisor(a: u64, b: u64) -> u64 {
    if b == 0 {
        a
    } else {
        greatest_common_divisor(b, a % b)
    }
}

pub(crate) fn bessel_i0(x: f64) -> f64 {
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
    series: &'static [f64],
}

fn even_legendre_series(series: &[f64], along: f64) -> f64 {
    let mut even = 1.0;
    let mut odd = along;
    let mut sum = 0.0;
    for (index, coefficient) in series.iter().enumerate() {
        sum += coefficient * even;
        let odd_order = (2 * index + 1) as f64;
        let even_order = odd_order + 1.0;
        let next_even = ((2.0 * odd_order + 1.0) * along * odd - odd_order * even) / even_order;
        odd =
            ((2.0 * even_order + 1.0) * along * next_even - even_order * odd) / (even_order + 1.0);
        even = next_even;
    }
    sum
}

impl WindowedSinc {
    pub(crate) fn new(params: SincParams) -> Self {
        let beta = f64::from(params.kaiser_beta);
        Self {
            half_taps: f64::from(params.half_taps),
            cutoff: f64::from(params.cutoff),
            beta,
            window_peak: bessel_i0(beta),
            series: params.window,
        }
    }

    pub(crate) fn at(&self, tau: f64) -> f64 {
        if tau >= self.half_taps {
            return 0.0;
        }
        let along = tau / self.half_taps;
        let window = if self.series.is_empty() {
            bessel_i0(self.beta * (1.0 - along * along).max(0.0).sqrt()) / self.window_peak
        } else {
            even_legendre_series(self.series, along)
        };
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

static LAST_TABULATED: Mutex<Option<(TableFor, Arc<Tabulated>)>> = Mutex::new(None);
static LAST_PLANNED: Mutex<Option<(TableFor, Arc<Plan>)>> = Mutex::new(None);

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
        padded_row_in_lanes_of(reach, WIDEST_LANES_PER_VECTOR)
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
    padded_row_in_lanes_of(reach, LANES_PER_VECTOR)
}

const fn padded_row_in_lanes_of(reach: Reach, lanes: usize) -> usize {
    (lanes + reach.span() + ACCUMULATORS).next_multiple_of(lanes)
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

#[derive(Clone, Copy)]
struct Read {
    whole: usize,
    phase: u32,
}

fn conversion_for(
    config: ResamplerConfig,
    params: SincParams,
    shape: &Shape,
    output_hz: u32,
) -> Conversion {
    let input_hz = f64::from(config.input_rate.hz());
    let band_hz = f64::from(config.input_rate.hz().min(config.output_rate.hz()));
    let widest_hz = input_hz.max(f64::from(output_hz));
    let clear_taps = f64::from(params.clear_taps);
    let hop_taps = if widest_hz >= STEEP_FROM_RATIO * band_hz {
        params.steep_hop_taps
    } else {
        params.hop_taps
    };
    Conversion {
        input_hz: config.input_rate.hz(),
        output_hz,
        band_hz,
        lead_seconds: (shape.lead() + clear_taps) / band_hz,
        trail_seconds: (shape.trail() + clear_taps) / band_hz,
        hop_at_most_seconds: (f64::from(hop_taps) / band_hz).min(SPECTRAL_HOP_SECONDS_AT_MOST),
    }
}

fn planned(
    config: ResamplerConfig,
    params: SincParams,
    shape: &Shape,
    blocks: Blocks,
    conversion: Conversion,
) -> Arc<Plan> {
    let band_hz = conversion.band_hz;
    let at_seconds = |after: f64| shape.at(after * band_hz);
    let design = match shape {
        Shape::Linear(_) => Design::Even(at_seconds),
        Shape::Shaped(_) => Design::Turned(at_seconds),
    };
    Plan::kept_for(
        &LAST_PLANNED,
        TableFor {
            params,
            phase: config.phase,
            input_rate: config.input_rate,
            output_rate: config.output_rate,
        },
        conversion,
        blocks,
        design,
    )
}

struct Stage {
    spectral: Spectral,
    up: usize,
    preroll: usize,
    skipping: usize,
    taken: usize,
    appended: usize,
    wanted: Option<usize>,
}

impl Stage {
    fn new(config: ResamplerConfig, params: SincParams, preroll: usize) -> Option<Self> {
        let up = if 2 * u64::from(config.output_rate.hz()) <= u64::from(config.input_rate.hz()) {
            1
        } else {
            OVERSAMPLED
        };
        let shape = Shape::of(params, config.phase);
        let oversampled_hz = config.input_rate.hz().checked_mul(up as u32)?;
        let conversion = conversion_for(config, params, &shape, oversampled_hz);
        let blocks = Blocks::fitting(conversion)?;
        let plan = planned(config, params, &shape, blocks, conversion);
        let channels = config.channels.count().get() as usize;
        let prerolled_units = preroll.div_ceil(blocks.unit_out());
        let primed_units = blocks.trail_units() + prerolled_units;

        Some(Self {
            spectral: Spectral::new(plan, channels, primed_units),
            up,
            preroll,
            skipping: prerolled_units * blocks.unit_out() - preroll,
            taken: 0,
            appended: 0,
            wanted: None,
        })
    }

    fn reset(&mut self) {
        self.spectral.reset();
        let unit = self.spectral.blocks().unit_out();
        self.skipping = self.preroll.div_ceil(unit) * unit - self.preroll;
        self.taken = 0;
        self.appended = 0;
        self.wanted = None;
    }

    fn release(&mut self, planes: &mut [f64], stride: usize, filled: &mut usize) {
        let run = self.spectral.due();
        let skipped = self.skipping.min(run.len());
        self.skipping -= skipped;
        let left = self
            .wanted
            .map_or(usize::MAX, |wanted| wanted.saturating_sub(self.appended));
        let kept = (run.len() - skipped).min(left);
        if kept > 0 {
            let start = run.start + skipped;
            for (channel, plane) in planes.chunks_exact_mut(stride).enumerate() {
                if let Some(slots) = plane.get_mut(*filled..*filled + kept) {
                    self.spectral
                        .plane_into(channel, start..start + kept, slots);
                }
            }
        }
        *filled += kept;
        self.appended += kept;
        self.spectral.turn_when_full();
    }
}

struct Whole {
    spectral: Spectral,
    channels: usize,
    taken: usize,
    emitted: usize,
    wanted: Option<usize>,
}

impl Whole {
    fn new(config: ResamplerConfig, params: SincParams) -> Option<Self> {
        let shape = Shape::of(params, config.phase);
        let conversion = conversion_for(config, params, &shape, config.output_rate.hz());
        let blocks = Blocks::fitting(conversion)?;
        let plan = planned(config, params, &shape, blocks, conversion);
        let channels = config.channels.count().get() as usize;
        Some(Self {
            spectral: Spectral::new(plan, channels, blocks.trail_units()),
            channels,
            taken: 0,
            emitted: 0,
            wanted: None,
        })
    }

    fn reset(&mut self) {
        self.spectral.reset();
        self.taken = 0;
        self.emitted = 0;
        self.wanted = None;
    }

    fn release(&mut self, output: &mut [f64], made: &mut usize, left: usize) {
        let run = self.spectral.due();
        let kept = run.len().min(left);
        let channels = self.channels;
        let from = *made * channels;
        if let Some(room) = output.get_mut(from..from + kept * channels) {
            self.spectral
                .interleaved_into(run.start..run.start + kept, room, channels);
        }
        *made += kept;
        self.emitted += kept;
        self.spectral.turn_when_full();
    }

    fn process(&mut self, input: &[f64], output: &mut [f64]) -> ProcessCount {
        let channels = self.channels;
        let frames = input.len() / channels;
        let room = output.len() / channels;
        let mut taken = 0;
        let mut made = 0;
        while taken < frames {
            let run = (frames - taken)
                .min(self.spectral.frames_fitting(room - made))
                .min(self.spectral.room());
            if run == 0 {
                break;
            }
            let rest = input.get(taken * channels..).unwrap_or_default();
            self.spectral.gather(rest, channels, run);
            let left = room - made;
            self.release(output, &mut made, left);
            taken += run;
        }
        self.taken += taken;
        ProcessCount {
            frames_in: taken,
            frames_out: made,
        }
    }

    fn flush(&mut self, output: &mut [f64]) -> usize {
        let channels = self.channels;
        let room = output.len() / channels;
        let wanted = *self
            .wanted
            .get_or_insert(self.spectral.blocks().outputs_of(self.taken));
        let mut made = 0;
        while self.emitted < wanted && made < room {
            let run = self.spectral.frames_fitting(room - made).max(1);
            let run = run.min(self.spectral.room());
            self.spectral.gather_silence(run);
            let left = (room - made).min(wanted - self.emitted);
            self.release(output, &mut made, left);
        }
        made
    }
}

enum Route {
    Whole(Box<Whole>),
    Polyphase(Box<Polyphase>),
}

pub struct Resampler {
    config: ResamplerConfig,
    ratio: f64,
    route: Route,
}

struct Polyphase {
    taps: Taps,
    channels: usize,
    ratio: f64,
    reach: Reach,
    history: History,
    weights: Vec<f64>,
    filled: usize,
    whole: usize,
    phase: u32,
    phases: u32,
    whole_step: usize,
    phase_step: u32,
    stage: Option<Stage>,
}

impl Resampler {
    pub fn new(config: ResamplerConfig) -> Result<Self> {
        Self::with_params(config, config.quality.params_for(config.phase))
    }

    pub fn with_params(config: ResamplerConfig, params: SincParams) -> Result<Self> {
        if params.half_taps == 0 || params.half_taps > MAX_HALF_TAPS {
            return Err(Error::FilterLengthOutOfRange {
                requested: u32::from(params.half_taps),
                max: u32::from(MAX_HALF_TAPS),
            });
        }
        if params.phases < MIN_PHASES {
            return Err(Error::TooFewPhases {
                requested: params.phases,
                min: MIN_PHASES,
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

        let staged = params.converts_in_blocks();
        let route = match staged.then(|| Whole::new(config, params)).flatten() {
            Some(whole) => Route::Whole(Box::new(whole)),
            None => Route::Polyphase(Box::new(Polyphase::new(config, params, ratio, staged))),
        };

        Ok(Self {
            config,
            ratio,
            route,
        })
    }

    pub const fn ratio(&self) -> f64 {
        self.ratio
    }
}

impl Polyphase {
    fn new(config: ResamplerConfig, params: SincParams, ratio: f64, staged: bool) -> Self {
        let scale = ratio.min(1.0);
        let short = interpolator_for(params);
        let reach_of_the_short = Reach::of(&Shape::of(short, FilterPhase::Linear), 1.0);
        let stage = staged
            .then(|| Stage::new(config, params, reach_of_the_short.behind))
            .flatten();
        let (interpolator, interpolated_at, interpolated_phase) = if stage.is_some() {
            (short, 1.0, FilterPhase::Linear)
        } else {
            (params, scale, config.phase)
        };
        let shape = Shape::of(interpolator, interpolated_phase);
        let reach = Reach::of(&shape, interpolated_at);
        let up = stage.as_ref().map_or(1, |stage| stage.up);
        let channels = config.channels.count().get() as usize;
        let cycle = config.input_rate.ratio_to(config.output_rate);
        let advanced = u64::from(cycle.denom.get()) * up as u64;
        let common = greatest_common_divisor(u64::from(cycle.numer.get()), advanced);
        let phases = (u64::from(cycle.numer.get()) / common) as u32;
        let advance = (advanced / common) as u32;
        let tabulated = Tabulated::fits(phases, reach);
        let taps = if tabulated {
            Taps::Tabulated(Tabulated::kept_for(
                TableFor {
                    params: interpolator,
                    phase: interpolated_phase,
                    input_rate: config.input_rate,
                    output_rate: config.output_rate,
                },
                || Tabulated::new(&shape, interpolated_at, reach, phases),
            ))
        } else {
            Taps::Interpolated(Kernel::new(
                &shape,
                interpolator.phases,
                interpolated_at,
                reach,
            ))
        };
        let tapering = if tabulated { 0 } else { padded_row(reach) };

        Self {
            taps,
            channels,
            ratio,
            reach,
            history: History::new(channels, up * config.max_frames_in + reach.span() + 2),
            weights: vec![0.0; tapering],
            filled: if stage.is_some() { 0 } else { reach.behind },
            whole: reach.behind,
            phase: 0,
            phases,
            whole_step: (advance / phases) as usize,
            phase_step: advance % phases,
            stage,
        }
    }

    fn up(&self) -> usize {
        self.stage.as_ref().map_or(1, |stage| stage.up)
    }

    fn append_staged(&mut self, input: &[f64]) -> usize {
        let Self {
            channels,
            history,
            filled,
            stage: Some(stage),
            ..
        } = self
        else {
            return 0;
        };
        let channels = *channels;
        let stride = history.stride;
        let room = (history.capacity - *filled) / stage.up;
        let frames = (input.len() / channels).min(room);
        let planes = history.planes_mut();

        let mut rest = input.get(..frames * channels).unwrap_or_default();
        while !rest.is_empty() {
            let run = stage.spectral.room().min(rest.len() / channels);
            stage.spectral.gather(rest, channels, run);
            stage.release(planes, stride, filled);
            rest = rest.get(run * channels..).unwrap_or_default();
        }
        stage.taken += frames;
        frames
    }

    fn silence_until_wanted(&mut self) -> usize {
        let Self {
            history,
            filled,
            stage: Some(stage),
            ..
        } = self
        else {
            return 0;
        };
        let stride = history.stride;
        let capacity = history.capacity;
        let planes = history.planes_mut();
        let mut stepped = 0;
        while stage.wanted.is_some_and(|wanted| stage.appended < wanted) && *filled < capacity {
            let open = capacity - *filled + stage.skipping;
            let run = stage.spectral.room().min(open / stage.up);
            if run == 0 {
                break;
            }
            stage.spectral.gather_silence(run);
            stage.release(planes, stride, filled);
            stepped += run;
        }
        stepped
    }

    fn flush_staged(&mut self, output: &mut [f64]) -> usize {
        let behind = self.reach.behind;
        let ahead = self.reach.ahead;
        let channels = self.channels;
        if let Some(stage) = self.stage.as_mut() {
            let whole_track = match stage.taken.checked_sub(1) {
                Some(last) => behind + stage.up * last + 1 + ahead,
                None => stage.appended,
            };
            stage.wanted.get_or_insert(whole_track);
        }

        let mut produced = 0;
        loop {
            let stepped = self.silence_until_wanted();
            let made = self.emit(output.get_mut(produced * channels..).unwrap_or_default());
            produced += made;
            self.compact();
            if stepped == 0 && made == 0 {
                return produced;
            }
        }
    }

    fn blocks_of(history: &[f64]) -> &[[f64; ACCUMULATORS]] {
        history.as_chunks::<ACCUMULATORS>().0
    }

    fn dot_one(history: &[f64], weights: &[f64]) -> f64x8 {
        Self::blocks_of(weights)
            .iter()
            .zip(Self::blocks_of(history))
            .fold(f64x8::ZERO, |sum, (batch, block)| {
                f64x8::new(*block).mul_add(f64x8::new(*batch), sum)
            })
    }

    fn dot_two(histories: [&[f64]; 2], weights: &[f64]) -> [f64x8; 2] {
        let [first, second] = histories.map(Self::blocks_of);
        Self::blocks_of(weights).iter().zip(first).zip(second).fold(
            [f64x8::ZERO; 2],
            |[one, two], ((batch, first), second)| {
                let batch = f64x8::new(*batch);
                [
                    f64x8::new(*first).mul_add(batch, one),
                    f64x8::new(*second).mul_add(batch, two),
                ]
            },
        )
    }

    fn dot_four(histories: [&[f64]; FRAMES_AT_ONCE], weights: &[f64]) -> [f64x8; FRAMES_AT_ONCE] {
        let [first, second, third, fourth] = histories.map(Self::blocks_of);
        Self::blocks_of(weights)
            .iter()
            .zip(first)
            .zip(second)
            .zip(third)
            .zip(fourth)
            .fold(
                [f64x8::ZERO; FRAMES_AT_ONCE],
                |[one, two, three, four], ((((batch, first), second), third), fourth)| {
                    let batch = f64x8::new(*batch);
                    [
                        f64x8::new(*first).mul_add(batch, one),
                        f64x8::new(*second).mul_add(batch, two),
                        f64x8::new(*third).mul_add(batch, three),
                        f64x8::new(*fourth).mul_add(batch, four),
                    ]
                },
            )
    }

    fn summed(lanes: f64x8) -> f64 {
        let mut partial = lanes.to_array();
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

    fn histories<const FRAMES: usize>(
        &self,
        firsts: [usize; FRAMES],
        channel: usize,
        taps: usize,
    ) -> [&[f64]; FRAMES] {
        let planes = self.history.planes();
        let offset = channel * self.history.stride;
        firsts.map(|first| {
            planes
                .get(first + offset..first + offset + taps)
                .unwrap_or_default()
        })
    }

    fn convolve_many<const FRAMES: usize>(
        &self,
        mut frames: [&mut [f64]; FRAMES],
        firsts: [usize; FRAMES],
        weights: &[f64],
        dot: fn([&[f64]; FRAMES], &[f64]) -> [f64x8; FRAMES],
    ) {
        for channel in 0..self.channels {
            let sums = dot(self.histories(firsts, channel, weights.len()), weights);
            for (frame, sum) in frames.iter_mut().zip(sums) {
                if let Some(slot) = frame.get_mut(channel) {
                    *slot = Self::summed(sum);
                }
            }
        }
    }

    fn convolve_one(&self, frame: &mut [f64], first: usize, weights: &[f64]) {
        for (channel, slot) in frame.iter_mut().enumerate() {
            let [history] = self.histories([first], channel, weights.len());
            *slot = Self::summed(Self::dot_one(history, weights));
        }
    }

    fn after(&self, read: Read) -> Read {
        let stepped = read.phase + self.phase_step;
        let wraps = stepped >= self.phases;
        Read {
            whole: read.whole + self.whole_step + usize::from(wraps),
            phase: if wraps {
                stepped - self.phases
            } else {
                stepped
            },
        }
    }

    fn fits(&self, read: Read) -> bool {
        let rising = usize::from(read.phase == 0);
        read.whole.saturating_add(self.reach.ahead) + (1 - rising) < self.filled
    }

    fn first_of(&self, read: Read) -> usize {
        let rising = usize::from(read.phase == 0);
        read.whole.saturating_sub(self.reach.behind) + (1 - rising)
    }

    fn weights_of<'row>(&self, phase: u32, row: &'row [f64]) -> &'row [f64] {
        let covered = self
            .reach
            .taps(usize::from(phase == 0))
            .next_multiple_of(ACCUMULATORS);
        row.get(LANES_PER_VECTOR..LANES_PER_VECTOR + covered)
            .unwrap_or_default()
    }

    fn emit(&mut self, output: &mut [f64]) -> usize {
        let capacity = output.len() / self.channels;
        let start = Read {
            whole: self.whole,
            phase: self.phase,
        };

        let mut end = start;
        let mut count = 0;
        while count < capacity && self.fits(end) {
            end = self.after(end);
            count += 1;
        }

        match &self.taps {
            Taps::Tabulated(tabulated) => self.emit_by_phase(tabulated, start, count, output),
            Taps::Interpolated(kernel) => {
                let mut weights = mem::take(&mut self.weights);
                self.emit_tapering(kernel, &mut weights, start, count, output);
                self.weights = weights;
            }
        }

        self.whole = end.whole;
        self.phase = end.phase;
        count
    }

    fn emit_by_phase(&self, tabulated: &Tabulated, start: Read, count: usize, output: &mut [f64]) {
        let channels = self.channels;
        let phases = (self.phases as usize).max(1);
        let cycle = self.whole_step * phases + self.phase_step as usize;

        let mut read = start;
        for group in 0..count.min(phases) {
            let weights = self.weights_of(read.phase, tabulated.row(read.phase));
            let first = self.first_of(read);
            let frames_in_the_group = (count - group).div_ceil(phases);

            let mut taken = 0;
            while taken < frames_in_the_group {
                let index = group + taken * phases;
                let first = first + taken * cycle;
                let left = frames_in_the_group - taken;
                let rest = output.get_mut(index * channels..).unwrap_or_default();
                if left >= FRAMES_AT_ONCE {
                    let frames = Self::every_cycle(rest, channels, phases);
                    let firsts = array::from_fn(|frame| first + frame * cycle);
                    self.convolve_many(frames, firsts, weights, Self::dot_four);
                    taken += FRAMES_AT_ONCE;
                } else if left >= 2 {
                    let frames = Self::every_cycle(rest, channels, phases);
                    self.convolve_many(frames, [first, first + cycle], weights, Self::dot_two);
                    taken += 2;
                } else {
                    let frame = rest.get_mut(..channels).unwrap_or_default();
                    self.convolve_one(frame, first, weights);
                    taken += 1;
                }
            }
            read = self.after(read);
        }
    }

    fn every_cycle<const FRAMES: usize>(
        output: &mut [f64],
        channels: usize,
        phases: usize,
    ) -> [&mut [f64]; FRAMES] {
        let mut rest = output;
        array::from_fn(|_| {
            let taken = mem::take(&mut rest);
            let (frame, after) = taken.split_at_mut(channels.min(taken.len()));
            let skipped = (phases * channels - frame.len()).min(after.len());
            rest = after.get_mut(skipped..).unwrap_or_default();
            frame
        })
    }

    fn emit_tapering(
        &self,
        kernel: &Kernel,
        weights: &mut [f64],
        start: Read,
        count: usize,
        output: &mut [f64],
    ) {
        let channels = self.channels;

        let mut read = start;
        for index in 0..count {
            let rising = usize::from(read.phase == 0);
            let taps_now = self.reach.taps(rising);
            let fraction = f64::from(read.phase) / f64::from(self.phases);
            let shaped = weights
                .get_mut(LANES_PER_VECTOR..LANES_PER_VECTOR + taps_now)
                .unwrap_or_default();
            kernel.taper(shaped, fraction, rising);
            if let Some(stale) = weights.get_mut(LANES_PER_VECTOR + taps_now) {
                *stale = 0.0;
            }
            if let Some(frame) = output.get_mut(index * channels..(index + 1) * channels) {
                let taken = self.weights_of(read.phase, weights);
                self.convolve_one(frame, self.first_of(read), taken);
            }
            read = self.after(read);
        }
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
        if self.stage.is_some() {
            return self.append_staged(input);
        }
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

impl Polyphase {
    fn grow(&mut self, max_frames_in: usize) {
        self.history = History::new(
            self.channels,
            self.up() * max_frames_in + self.reach.span() + 2,
        );
        self.reset();
    }

    fn output_frames_at_most(&self, frames_in: usize) -> usize {
        (frames_in as f64 * self.ratio).ceil() as usize + 1
    }

    fn held_behind_the_output(&self) -> usize {
        self.stage.as_ref().map_or(self.reach.ahead, |stage| {
            stage.spectral.blocks().outputs_held() + self.reach.ahead
        })
    }

    fn max_flush_frames(&self) -> usize {
        match &self.stage {
            Some(stage) => self.output_frames_at_most(
                (self.held_behind_the_output() + stage.up).div_ceil(stage.up),
            ),
            None => self.output_frames_at_most(self.reach.ahead),
        }
    }

    fn latency_frames(&self) -> f64 {
        self.held_behind_the_output() as f64 * self.ratio / self.up() as f64
    }

    fn reset(&mut self) {
        self.history.planes_mut().fill(0.0);
        self.filled = if self.stage.is_some() {
            0
        } else {
            self.reach.behind
        };
        self.whole = self.reach.behind;
        self.phase = 0;
        if let Some(stage) = self.stage.as_mut() {
            stage.reset();
        }
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
        if self.stage.is_some() {
            return self.flush_staged(output);
        }
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
            match &mut self.route {
                Route::Whole(whole) => whole.reset(),
                Route::Polyphase(polyphase) => polyphase.grow(max_frames_in),
            }
        }
        Ok(self.max_output_frames(max_frames_in))
    }

    fn max_output_frames(&self, frames_in: usize) -> usize {
        (frames_in as f64 * self.ratio).ceil() as usize + 1
    }

    fn max_flush_frames(&self) -> usize {
        match &self.route {
            Route::Whole(whole) => whole.spectral.blocks().outputs_held() + 2,
            Route::Polyphase(polyphase) => polyphase.max_flush_frames(),
        }
    }

    fn reset(&mut self) {
        match &mut self.route {
            Route::Whole(whole) => whole.reset(),
            Route::Polyphase(polyphase) => polyphase.reset(),
        }
    }

    fn output_spec(&self, input: StreamSpec) -> StreamSpec {
        StreamSpec::new(self.config.output_rate, input.channels, input.format)
    }

    fn latency_frames(&self) -> f64 {
        match &self.route {
            Route::Whole(whole) => whole.spectral.blocks().outputs_held() as f64,
            Route::Polyphase(polyphase) => polyphase.latency_frames(),
        }
    }

    fn is_transparent(&self) -> bool {
        self.config.input_rate == self.config.output_rate
    }

    fn is_carried_across_a_reshape(&self) -> bool {
        true
    }

    fn process(&mut self, input: &[f64], output: &mut [f64]) -> ProcessCount {
        match &mut self.route {
            Route::Whole(whole) => whole.process(input, output),
            Route::Polyphase(polyphase) => polyphase.process(input, output),
        }
    }

    fn flush(&mut self, output: &mut [f64]) -> usize {
        match &mut self.route {
            Route::Whole(whole) => whole.flush(output),
            Route::Polyphase(polyphase) => polyphase.flush(output),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, f64::consts::TAU};

    use super::*;

    const BLOCK: usize = 1_024;
    const LOW_LATENCY_REJECTS_DB: f64 = -85.0;
    const BALANCED_REJECTS_DB: f64 = -125.0;
    const TWENTY_FOUR_BIT_FLOOR_DB: f64 = -140.0;
    const UNITY_WITHIN: f64 = 1e-9;
    const FOLDS_TO_THREE_KILOHERTZ_AT_EIGHT: f64 = 21_000.0;
    const AGREES_WITH_THE_EXACT_SUM_WITHIN: f64 = 1e-12;
    const BALANCED_AGREES_WITHIN: f64 = 1e-6;
    const HIGH_AGREES_WITHIN: f64 = 1e-8;
    const VERY_HIGH_AGREES_WITHIN: f64 = 3e-11;
    const HIGH_INTERPOLATED_WITHIN: f64 = 1e-8;
    const VERY_HIGH_INTERPOLATED_WITHIN: f64 = 3e-11;
    const RINGS_UNTIL_DOWN_BY: f64 = 1e-6;
    const SOX_STEEP_RINGS_MILLISECONDS: f64 = 11.77;
    const SOX_VHQ_RINGS_MILLISECONDS: f64 = 2.52;
    const SOX_HQ_RINGS_MILLISECONDS: f64 = 2.15;
    const HIGH_REJECTS_DB: f64 = -170.0;
    const VERY_HIGH_REJECTS_DB: f64 = -200.0;
    const SOX_STEEP_PASSBAND_NYQUISTS: f64 = 0.9864;
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

    fn steady<T>(samples: &[T]) -> &[T] {
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

    fn spanned(config: ResamplerConfig) -> f64 {
        let params = config.quality.params_for(config.phase);
        let band = f64::from(config.input_rate.hz().min(config.output_rate.hz()));
        let span = 2.0 * f64::from(params.half_taps) * f64::from(config.output_rate.hz()) / band;
        latency(config) + span
    }

    fn settling_frames(config: ResamplerConfig) -> usize {
        let ratio = f64::from(config.output_rate.hz()) / f64::from(config.input_rate.hz());
        (BLOCK * 16).max((8.0 * spanned(config) / ratio).ceil() as usize)
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
        let config = config(
            SampleRate::HZ_44100,
            SampleRate::HZ_88200,
            Quality::LowLatency,
        );
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
                let output = run(config, &vec![1.0; settling_frames(config)]);

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
    const MINIMUM_RINGS_QUIETER_BY_DB: f64 = 35.0;
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
        for (quality, floor) in [
            (Quality::LowLatency, LOW_LATENCY_REJECTS_DB),
            (Quality::Balanced, BALANCED_REJECTS_DB),
            (Quality::High, TWENTY_FOUR_BIT_FLOOR_DB),
            (Quality::VeryHigh, VERY_HIGH_REJECTS_DB),
        ] {
            for phase in SHAPED {
                for (from, to) in RATE_PAIRS {
                    let config = shaped(from, to, quality, phase);
                    let output = run(config, &vec![1.0; settling_frames(config)]);
                    for sample in settled(&output, spanned(config)) {
                        assert!(
                            (*sample - 1.0).abs() < UNITY_WITHIN,
                            "{from} -> {to} {quality:?} {phase:?} drifted DC to {sample}"
                        );
                    }
                }

                let config = shaped(SampleRate::HZ_44100, SampleRate::HZ_48000, quality, phase);
                let tone_in_the_passband =
                    tone(SampleRate::HZ_44100, 1_000.0, settling_frames(config));
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
                let tone_past_nyquist =
                    tone(SampleRate::HZ_44100, 15_000.0, settling_frames(config));
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
        let half_the_kernel = f64::from(Quality::VeryHigh.params().half_taps) * 48_000.0 / 44_100.0;
        assert!(
            minimum < linear - half_the_kernel * 0.75,
            "minimum phase waits {minimum} frames, linear {linear}"
        );
    }

    fn an_odd_rate() -> SampleRate {
        SampleRate::new(47_993).expect("a rate in range")
    }

    #[test]
    fn a_cycle_too_long_to_tabulate_still_resamples_through_the_interpolated_kernel() {
        let config = config(SampleRate::HZ_44100, an_odd_rate(), Quality::High);
        let resampler = Resampler::new(config).expect("a valid configuration");
        assert!(
            matches!(
                polyphase_of(&resampler).map(|polyphase| &polyphase.taps),
                Some(Taps::Interpolated(_))
            ),
            "44.1 kHz onto 47.993 kHz should exceed the tabulated weight bound"
        );

        let input = tone(SampleRate::HZ_44100, 1_000.0, settling_frames(config));
        let output = run(config, &input);

        let gain = decibels(rms(settled(&output, spanned(config))) / rms(steady(&input)));
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

    fn polyphase_of(resampler: &Resampler) -> Option<&Polyphase> {
        match &resampler.route {
            Route::Polyphase(polyphase) => Some(polyphase),
            Route::Whole(_) => None,
        }
    }

    fn table_of(resampler: &Resampler) -> Option<&Arc<Tabulated>> {
        polyphase_of(resampler).and_then(|polyphase| match &polyphase.taps {
            Taps::Tabulated(table) => Some(table),
            Taps::Interpolated(_) => None,
        })
    }

    fn plan_of(resampler: &Resampler) -> Option<&Arc<Plan>> {
        match &resampler.route {
            Route::Whole(whole) => Some(whole.spectral.plan()),
            Route::Polyphase(_) => None,
        }
    }

    #[test]
    fn a_rebind_at_the_same_rates_takes_the_table_already_built() {
        let wanted = config(
            SampleRate::HZ_44100,
            SampleRate::HZ_48000,
            Quality::LowLatency,
        );
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
            Quality::LowLatency,
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
    fn a_rebind_at_the_same_rates_takes_the_plan_already_built() {
        let wanted = config(SampleRate::HZ_44100, SampleRate::HZ_48000, Quality::High);
        let built_back_to_back = || {
            let first = Resampler::new(wanted).expect("a supported conversion");
            let again = Resampler::new(wanted).expect("a supported conversion");
            plan_of(&first)
                .zip(plan_of(&again))
                .is_some_and(|(first, again)| Arc::ptr_eq(first, again))
        };
        assert!(
            (0..TRIES_WHILE_OTHER_TESTS_BUILD).any(|_| built_back_to_back()),
            "the same conversion planned its transforms twice"
        );
    }

    #[test]
    fn every_rate_pair_is_tabulated_or_converted_in_blocks_at_every_quality() {
        for quality in QUALITIES {
            let mut interpolated = Vec::new();
            for from in EVERY_RATE {
                for to in EVERY_RATE {
                    let config = config(from, to, quality);
                    let Ok(resampler) = Resampler::new(config) else {
                        continue;
                    };
                    let in_blocks = matches!(resampler.route, Route::Whole(_));
                    assert_eq!(
                        in_blocks,
                        quality.params().converts_in_blocks(),
                        "{quality:?} at {from} -> {to}"
                    );
                    if polyphase_of(&resampler)
                        .is_some_and(|polyphase| matches!(polyphase.taps, Taps::Interpolated(_)))
                    {
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
    fn high_quality_rejects_aliases_harder_than_low_latency() {
        let high_down = config(SampleRate::HZ_44100, SampleRate::HZ_22050, Quality::High);
        let input = tone(SampleRate::HZ_44100, 15_000.0, settling_frames(high_down));

        let low_latency = alias_level(
            config(
                SampleRate::HZ_44100,
                SampleRate::HZ_22050,
                Quality::LowLatency,
            ),
            &input,
        );
        let high = alias_level(
            config(SampleRate::HZ_44100, SampleRate::HZ_22050, Quality::High),
            &input,
        );

        assert!(
            high < low_latency,
            "high rejected {high:.1} dB, low latency rejected {low_latency:.1} dB"
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
        let frames = output.as_chunks::<2>().0;

        for frame in steady(frames) {
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
        Quality::LowLatency,
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
            ..config(
                SampleRate::HZ_384000,
                SampleRate::HZ_8000,
                Quality::LowLatency,
            )
        };
        match Resampler::new(config) {
            Err(Error::RatioOutOfRange { .. }) => {}
            Err(other) => panic!("expected RatioOutOfRange, got {other:?}"),
            Ok(_) => panic!("48:1 should be past the supported ratio range"),
        }
    }

    #[test]
    fn each_quality_meets_its_alias_rejection_floor() {
        for (quality, floor) in [
            (Quality::LowLatency, LOW_LATENCY_REJECTS_DB),
            (Quality::Balanced, BALANCED_REJECTS_DB),
            (Quality::High, HIGH_REJECTS_DB),
            (Quality::VeryHigh, VERY_HIGH_REJECTS_DB),
        ] {
            let config = config(SampleRate::HZ_44100, SampleRate::HZ_22050, quality);
            let input = tone(SampleRate::HZ_44100, 15_000.0, settling_frames(config));
            let rejection = alias_level(config, &input);
            assert!(
                rejection < floor,
                "{quality:?} rejected only {rejection:.1} dB, needs better than {floor:.1} dB"
            );
        }
    }

    #[test]
    fn an_extreme_downsample_still_tabulates_the_filter_finely_enough() {
        let config = config(SampleRate::HZ_192000, SampleRate::HZ_8000, Quality::High);
        let input = tone(
            SampleRate::HZ_192000,
            FOLDS_TO_THREE_KILOHERTZ_AT_EIGHT,
            settling_frames(config),
        );

        let rejection = alias_level(config, &input);

        assert!(
            rejection < HIGH_REJECTS_DB,
            "24:1 rejected only {rejection:.1} dB"
        );
    }

    #[test]
    fn every_quality_keeps_the_passband_flat() {
        for quality in QUALITIES {
            let config = config(SampleRate::HZ_44100, SampleRate::HZ_48000, quality);
            let input = tone(SampleRate::HZ_44100, 1_000.0, settling_frames(config));
            let reference = rms(steady(&input));
            let output = run(config, &input);
            let ripple = decibels(rms(settled(&output, spanned(config))) / reference).abs();
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

    fn legendre_of_order(order: usize, along: f64) -> f64 {
        let (mut before, mut at) = (1.0, along);
        if order == 0 {
            return before;
        }
        for degree in 1..order {
            let degree = degree as f64;
            let next = ((2.0 * degree + 1.0) * along * at - degree * before) / (degree + 1.0);
            before = at;
            at = next;
        }
        at
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
        let along = tau / half_taps;
        let window = if params.window.is_empty() {
            modified_bessel_of_order_zero(beta * (1.0 - along * along).sqrt())
                / modified_bessel_of_order_zero(beta)
        } else {
            params
                .window
                .iter()
                .enumerate()
                .map(|(index, coefficient)| coefficient * legendre_of_order(2 * index, along))
                .sum()
        };
        cutoff * sinc * window
    }

    fn dc_of_a_block_kernel(params: SincParams, per_input_tap: f64) -> f64 {
        const POINTS_PER_TAP: i64 = 32;

        let reach = i64::from(params.half_taps) * POINTS_PER_TAP;
        let sum: f64 = (-reach..=reach)
            .map(|point| analytic_tap(params, (point as f64 / POINTS_PER_TAP as f64).abs()))
            .sum();
        sum / POINTS_PER_TAP as f64 / per_input_tap
    }

    struct Oracle {
        tap: Box<dyn Fn(f64) -> f64>,
        phases: i64,
        advance: i64,
        scale: f64,
        reach: i64,
        gain: Option<f64>,
        rows: BTreeMap<i64, Vec<f64>>,
    }

    impl Oracle {
        fn new(config: ResamplerConfig) -> Self {
            let params = config.quality.params_for(config.phase);
            let cycle = config.input_rate.ratio_to(config.output_rate);
            let scale =
                (f64::from(config.output_rate.hz()) / f64::from(config.input_rate.hz())).min(1.0);
            let gain = params
                .converts_in_blocks()
                .then(|| dc_of_a_block_kernel(params, scale));
            Self::of(
                move |tau| analytic_tap(params, tau),
                f64::from(params.half_taps),
                i64::from(cycle.numer.get()),
                i64::from(cycle.denom.get()),
                scale,
                gain,
            )
        }

        fn interpolating(config: ResamplerConfig, up: i64) -> Self {
            let params = interpolator_for(config.quality.params());
            let cycle = config.input_rate.ratio_to(config.output_rate);
            let numer = i64::from(cycle.numer.get());
            let advanced = i64::from(cycle.denom.get()) * up;
            let common = greatest_common_divisor(numer as u64, advanced as u64) as i64;
            Self::of(
                move |tau| analytic_tap(params, tau),
                f64::from(params.half_taps),
                numer / common,
                advanced / common,
                1.0,
                None,
            )
        }

        fn of(
            tap: impl Fn(f64) -> f64 + 'static,
            reach_taps: f64,
            phases: i64,
            advance: i64,
            scale: f64,
            gain: Option<f64>,
        ) -> Self {
            Self {
                tap: Box::new(tap),
                phases,
                advance,
                scale,
                reach: (reach_taps / scale).ceil() as i64 + 1,
                gain,
                rows: BTreeMap::new(),
            }
        }

        fn instant(&mut self, frame: i64) -> (i64, &[f64]) {
            let position = frame * self.advance;
            let first = position / self.phases - self.reach;
            let fraction = position % self.phases;
            let Self {
                tap,
                phases,
                scale,
                reach,
                gain,
                rows,
                ..
            } = self;
            let row = rows.entry(fraction).or_insert_with(|| {
                let raw: Vec<f64> = (-*reach..=*reach)
                    .map(|offset| {
                        let distance = (fraction - offset * *phases).abs() as f64 / *phases as f64;
                        tap(distance * *scale)
                    })
                    .collect();
                let divided_by = gain.unwrap_or_else(|| raw.iter().sum());
                raw.into_iter().map(|weight| weight / divided_by).collect()
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

    struct Prefiltered {
        taps: Vec<f64>,
        lead: i64,
        up: i64,
    }

    impl Prefiltered {
        fn of(config: ResamplerConfig) -> Self {
            let params = config.quality.params();
            let up: i64 = if 2 * config.output_rate.hz() <= config.input_rate.hz() {
                1
            } else {
                2
            };
            let band_hz = f64::from(config.input_rate.hz().min(config.output_rate.hz()));
            let scale = band_hz / f64::from(config.input_rate.hz());
            let reach_taps = f64::from(params.half_taps);
            let lead = (reach_taps * up as f64 / scale).ceil() as i64;
            let mut taps: Vec<f64> = (0..=2 * lead)
                .map(|tap| {
                    let after = (tap - lead) as f64 / up as f64 * scale;
                    analytic_tap(params, after.abs())
                })
                .collect();
            let gain = dc_of_a_block_kernel(params, scale / up as f64) / up as f64;
            for tap in &mut taps {
                *tap /= gain;
            }
            Self { taps, lead, up }
        }

        fn at(&self, input: &[f64], channels: usize, channel: usize, instant: i64) -> f64 {
            let frames = (input.len() / channels) as i64;
            self.taps
                .iter()
                .zip(0..)
                .filter_map(|(tap, offset)| {
                    let stuffed = instant + self.lead - offset;
                    let source = stuffed.div_euclid(self.up);
                    (stuffed.rem_euclid(self.up) == 0 && (0..frames).contains(&source))
                        .then(|| tap * input[source as usize * channels + channel])
                })
                .sum()
        }
    }

    fn assert_matches_the_oracle(from: SampleRate, to: SampleRate, quality: Quality, within: f64) {
        let frames = 3 * BLOCK + 417;
        let wanted = config(from, to, quality);
        let built = Resampler::new(wanted).expect("a valid resampler configuration");
        let staged = polyphase_of(&built).is_some_and(|polyphase| polyphase.stage.is_some());
        let prefiltered = staged.then(|| Prefiltered::of(wanted));
        let mut oracle = match &prefiltered {
            Some(stage) => Oracle::interpolating(wanted, stage.up),
            None => Oracle::new(wanted),
        };

        for layout in [
            ChannelLayout::Mono,
            ChannelLayout::Stereo,
            ChannelLayout::Discrete(resonate_core::ChannelCount::new(3).expect("three channels")),
            ChannelLayout::Surround51,
        ] {
            let config = ResamplerConfig {
                channels: layout,
                ..wanted
            };
            let channels = layout.count().get() as usize;
            let input = oracle_signal(from, channels, frames);
            let output = resampled_and_flushed(config, &input);

            let mut filtered: Vec<BTreeMap<i64, f64>> = vec![BTreeMap::new(); channels];
            for (index, frame) in output.chunks_exact(channels).enumerate() {
                let (first, weights) = oracle.instant(index as i64);
                for (channel, sample) in frame.iter().enumerate() {
                    let exact: f64 = match &prefiltered {
                        Some(stage) => weights
                            .iter()
                            .zip(first..)
                            .map(|(weight, instant)| {
                                weight
                                    * *filtered[channel].entry(instant).or_insert_with(|| {
                                        stage.at(&input, channels, channel, instant)
                                    })
                            })
                            .sum(),
                        None => weights
                            .iter()
                            .zip(first..)
                            .filter(|(_, source)| (0..frames as i64).contains(source))
                            .map(|(weight, source)| {
                                weight * input[source as usize * channels + channel]
                            })
                            .sum(),
                    };
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
            for (quality, within) in [
                (Quality::LowLatency, AGREES_WITH_THE_EXACT_SUM_WITHIN),
                (Quality::Balanced, BALANCED_AGREES_WITHIN),
                (Quality::High, HIGH_AGREES_WITHIN),
                (Quality::VeryHigh, VERY_HIGH_AGREES_WITHIN),
            ] {
                assert_matches_the_oracle(from, to, quality, within);
            }
        }
    }

    #[test]
    fn an_interpolated_kernel_stays_within_the_step_its_window_ends_on() {
        for quality in QUALITIES {
            let resampler = Resampler::new(config(SampleRate::HZ_44100, an_odd_rate(), quality))
                .expect("a valid configuration");
            assert!(
                matches!(
                    polyphase_of(&resampler).map(|polyphase| &polyphase.taps),
                    Some(Taps::Interpolated(_))
                ),
                "{quality:?} tabulated 44.1 kHz onto 47.993 kHz"
            );
        }
        for (from, to) in [
            (SampleRate::HZ_44100, an_odd_rate()),
            (an_odd_rate(), SampleRate::HZ_44100),
        ] {
            for (quality, within) in [
                (Quality::High, HIGH_INTERPOLATED_WITHIN),
                (Quality::VeryHigh, VERY_HIGH_INTERPOLATED_WITHIN),
            ] {
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
    fn very_high_stays_flat_past_the_steepest_sox_recipe_and_is_closed_at_nyquist() {
        let params = Quality::VeryHigh.params();
        let width = f64::from(params.kaiser_beta) / (PI * f64::from(params.half_taps));
        let starts = f64::from(params.cutoff) - width;

        assert!(
            starts > SOX_STEEP_PASSBAND_NYQUISTS - 0.01,
            "the transition starts at {starts} of the band edge"
        );
        assert!(
            (f64::from(params.cutoff) + width - 1.0).abs() < 1e-5,
            "the transition ends away from the band edge"
        );

        let at = |nyquists: f64| continuous_response(params, nyquists);
        let flat_through = decibels(at(starts)).abs();
        let closed_by = decibels(at(1.0 + width / 4.0));
        assert!(flat_through < 1e-7, "{flat_through:e} dB where it starts");
        assert!(closed_by < -190.0, "{closed_by:.1} dB past the edge");
    }

    fn ringing_milliseconds(quality: Quality) -> f64 {
        let wanted = config(SampleRate::HZ_44100, SampleRate::HZ_48000, quality);
        let mut input = vec![0.0; BLOCK * 32];
        let at = BLOCK * 16;
        input[at] = 1.0;
        let output = resampled_and_flushed(wanted, &input);
        let peak = output.iter().map(|sample| sample.abs()).fold(0.0, f64::max);
        let heard = |sample: &f64| sample.abs() > peak * RINGS_UNTIL_DOWN_BY;
        let first = output.iter().position(heard).unwrap_or_default();
        let last = output.iter().rposition(heard).unwrap_or_default();
        (last - first) as f64 / 2.0 / 48.0
    }

    #[test]
    fn each_level_rings_for_less_than_the_sox_recipe_it_answers() {
        for (quality, recipe_rings) in [
            (Quality::Balanced, SOX_HQ_RINGS_MILLISECONDS),
            (Quality::High, SOX_VHQ_RINGS_MILLISECONDS),
            (Quality::VeryHigh, SOX_STEEP_RINGS_MILLISECONDS),
        ] {
            let rings = ringing_milliseconds(quality);
            assert!(
                rings < recipe_rings,
                "{quality:?} rings {rings:.2} ms either side against {recipe_rings} ms"
            );
        }
    }

    #[test]
    fn a_window_stands_at_one_in_the_middle_and_falls_to_almost_nothing_at_its_reach() {
        for quality in [Quality::Balanced, Quality::High, Quality::VeryHigh] {
            let window = quality.params().window;
            let middle = even_legendre_series(window, 0.0);
            let reach = even_legendre_series(window, 1.0);

            assert!(
                (middle - 1.0).abs() < 1e-12,
                "{quality:?} peaks at {middle}"
            );
            assert!(reach.abs() < 1e-3, "{quality:?} ends at {reach}");
        }
    }

    #[test]
    fn the_preset_cutoffs_end_their_transitions_on_nyquist() {
        for quality in [Quality::Balanced, Quality::High, Quality::VeryHigh] {
            let params = quality.params();
            let halfway = f64::from(params.kaiser_beta) / (PI * f64::from(params.half_taps));
            let stop = f64::from(params.cutoff) + halfway;
            assert!(
                (stop - 1.0).abs() < 1e-5,
                "{quality:?} ends its transition at {stop} of Nyquist"
            );
        }
    }

    #[test]
    fn the_interpolator_behind_a_prefilter_is_flat_where_it_passes_and_under_its_stopband() {
        const PASSES_TO_NYQUISTS: f64 = 0.5;
        const REJECTS_FROM_NYQUISTS: f64 = 1.5;
        const QUIETER_BY_DB: f64 = 40.0;
        const FLATTER_BY_DB: f64 = 20.0;

        for quality in [Quality::High, Quality::VeryHigh] {
            let prefilter = quality.params();
            let interpolator = interpolator_for(prefilter);
            let at = |nyquists: f64| decibels(continuous_response(interpolator, nyquists));
            let promised = -f64::from(prefilter.rejects_db);
            let flat_within_db = decibels(1.0 + 10_f64.powf((promised - FLATTER_BY_DB) / 20.0));

            let flattest = (0..=500)
                .map(|step| at(PASSES_TO_NYQUISTS * f64::from(step) / 500.0).abs())
                .fold(0.0, f64::max);
            assert!(
                flattest < flat_within_db,
                "{quality:?}'s interpolator strays {flattest:e} dB where the prefilter passes"
            );

            let loudest = (0..=STOPBAND_STEPS)
                .map(|step| {
                    at(REJECTS_FROM_NYQUISTS + f64::from(step) / f64::from(STOPBAND_STEPS) * 4.0)
                })
                .fold(f64::NEG_INFINITY, f64::max);
            assert!(
                loudest < promised - QUIETER_BY_DB,
                "{quality:?}'s interpolator reaches {loudest:.1} dB where images sit, the prefilter {promised:.1} dB"
            );
        }
    }

    #[test]
    fn a_resampler_given_too_few_phases_says_so_rather_than_naming_the_filter_length() {
        let params = SincParams {
            phases: MIN_PHASES - 1,
            ..Quality::LowLatency.params()
        };

        let built = Resampler::with_params(
            config(
                SampleRate::HZ_44100,
                SampleRate::HZ_48000,
                Quality::LowLatency,
            ),
            params,
        );

        assert!(matches!(
            built,
            Err(Error::TooFewPhases {
                requested,
                min: MIN_PHASES,
            }) if requested == MIN_PHASES - 1
        ));
    }
}
