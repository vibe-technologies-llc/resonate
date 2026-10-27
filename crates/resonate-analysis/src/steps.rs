use std::{
    f64::consts::{PI, TAU},
    sync::Arc,
};

use rustfft::{Fft, FftPlanner, num_complex::Complex};

pub(crate) const SIXTEEN_BIT_STEPS: f32 = 32_768.0;
const SILENT_COEFFICIENT_UP_TO: f32 = 1.0 / 4_096.0;
pub(crate) const QUARTER_OCTAVE_STEPS: f64 = 4.0;

const SEARCHES: usize = 4;
const SEARCHED_EVERY_SECONDS: u64 = 2;
const TRIED_FOR_EACH_SEARCH_AT_MOST: usize = 2;
const BUSY_SHARE: f64 = 0.2;
const CANDIDATES: usize = 4;

const MANTISSA_BITS: u32 = 11;
const MANTISSA_SHIFT: u32 = f32::MANTISSA_DIGITS - 1 - MANTISSA_BITS;
const MANTISSA_BINS: usize = 1 << MANTISSA_BITS;
const EXPONENTS: usize = 256;
const EXPONENT_BIAS: i32 = 127;

const AAC_HOP: usize = 1_024;
const AAC_FROM_HZ: f32 = 4_000.0;
const AAC_TO_HZ: f32 = 16_000.0;
const HIGHEST_AAC_RATE: u32 = 96_000;
const KAISER_BESSEL_ALPHA: f64 = 4.0;

const VORBIS_LONG_HOP: usize = 1_024;
const VORBIS_SHORT_HOP: usize = 128;
const VORBIS_FROM_HZ: f32 = 4_000.0;
const VORBIS_TO_HZ: f32 = 16_000.0;
const HIGHEST_VORBIS_RATE: u32 = 48_000;
const VORBIS_FLOOR_OCTAVES_A_STEP: f64 = 0.090_833_967_8;
const VORBIS_TURNS_A_FLOOR_STEP: f64 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Coding {
    Aac,
    Mp3,
    Vorbis,
}

impl Coding {
    pub const ALL: [Self; 3] = [Self::Aac, Self::Mp3, Self::Vorbis];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Aac => "AAC",
            Self::Mp3 => "MP3",
            Self::Vorbis => "Vorbis",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Steps {
    pub coding: Coding,
    pub rate: u32,
    pub lattice: f32,
    pub control: f32,
    pub coefficients: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Tally {
    sum: Complex<f64>,
    count: u64,
}

impl Tally {
    fn add(&mut self, other: Self) {
        self.sum += other.sum;
        self.count += other.count;
    }

    fn resultant(self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.sum.norm() / self.count as f64
        }
    }

    fn rayleigh(self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.sum.norm_sqr() / self.count as f64
        }
    }
}

pub(crate) struct Phases {
    mantissas: Vec<Complex<f32>>,
    exponents: Vec<Complex<f32>>,
}

impl Phases {
    pub(crate) fn new(steps_an_octave: f64) -> Self {
        let mantissas = (0..MANTISSA_BINS)
            .map(|bin| {
                let mantissa = 1.0 + (bin as f64 + 0.5) / MANTISSA_BINS as f64;
                turned(1.0, TAU * steps_an_octave * mantissa.log2())
            })
            .collect();
        let exponents = (0..EXPONENTS)
            .map(|exponent| {
                let octave = f64::from(exponent as i32 - EXPONENT_BIAS);
                turned(1.0, TAU * (steps_an_octave * octave).fract())
            })
            .collect();
        Self {
            mantissas,
            exponents,
        }
    }

    fn of(&self, coefficient: f32) -> (Complex<f32>, u32) {
        let magnitude = coefficient.abs();
        let counted = u32::from(magnitude > SILENT_COEFFICIENT_UP_TO);
        let bits = magnitude.to_bits();
        let bin = (bits >> MANTISSA_SHIFT) as usize & (MANTISSA_BINS - 1);
        let exponent = (bits >> (f32::MANTISSA_DIGITS - 1)) as usize & (EXPONENTS - 1);
        (
            self.mantissas[bin] * self.exponents[exponent] * counted as f32,
            counted,
        )
    }
}

pub(crate) fn turned(size: f64, angle: f64) -> Complex<f32> {
    let turn = Complex::from_polar(size, angle);
    Complex::new(turn.re as f32, turn.im as f32)
}

pub(crate) struct Band {
    pub(crate) first: usize,
    pub(crate) past: usize,
}

impl Band {
    pub(crate) fn of(rate: u32, lines: usize, from_hz: f32, to_hz: f32) -> Self {
        let nyquist = rate as f32 / 2.0;
        let line_hz = nyquist / lines as f32;
        Self {
            first: (from_hz / line_hz) as usize,
            past: ((to_hz.min(nyquist) / line_hz) as usize).min(lines),
        }
    }

    fn is_empty(&self) -> bool {
        self.first >= self.past
    }

    fn width(&self) -> usize {
        self.past - self.first
    }
}

pub(crate) trait Grid {
    const CODING: Coding;
    const HOP: usize;
    const RESIDUES: usize;
    const SPAN: usize;
    const LINES: usize;
    const SHAPES: usize;
    const CONTROL_SHIFT: usize;
    const FRAMES_A_SEARCH: usize;
    const CONFIRMED_FRAMES: usize;
    const MID_AND_SIDE: bool;

    fn band(rate: u32) -> Option<Band>;

    fn new(band: &Band) -> Self;

    fn phases(&self) -> &Phases;

    fn coefficients(
        &mut self,
        lane: usize,
        samples: &[f32],
        at: u64,
        shape: usize,
        into: &mut [f32],
    );

    fn forget(&mut self) {}
}

#[derive(Clone, Copy, Debug)]
struct Candidate {
    shape: usize,
    residue: usize,
}

enum Stage {
    Searching {
        next: u64,
        taken: usize,
        tried: usize,
        tallies: Vec<Tally>,
    },
    Confirming {
        from: u64,
        candidates: Vec<Candidate>,
    },
    Done(Option<Steps>),
}

pub(crate) struct Stepping<G: Grid> {
    searches: usize,
    rate: u32,
    channels: usize,
    paired: bool,
    band: Band,
    every: u64,
    held: [Vec<f32>; 2],
    held_from: u64,
    reached: u64,
    left: Vec<f32>,
    right: Vec<f32>,
    grid: Option<G>,
    stage: Stage,
}

impl<G: Grid> Stepping<G> {
    pub(crate) fn new(rate: u32, channels: usize) -> Self {
        Self::searching(rate, channels, SEARCHES)
    }

    pub(crate) fn searching(rate: u32, channels: usize, searches: usize) -> Self {
        let band = G::band(rate)
            .filter(|band| !band.is_empty() && channels > 0)
            .unwrap_or(Band { first: 0, past: 0 });
        let weighable = !band.is_empty();
        let hop = G::HOP as u64;
        let every = (u64::from(rate) * SEARCHED_EVERY_SECONDS).div_ceil(hop) * hop;
        let grid = weighable.then(|| G::new(&band));
        Self {
            searches,
            rate,
            channels,
            paired: channels > 1,
            band,
            every,
            held: [Vec::new(), Vec::new()],
            held_from: 0,
            reached: 0,
            left: vec![0.0; G::LINES],
            right: vec![0.0; G::LINES],
            grid,
            stage: if weighable {
                Stage::Searching {
                    next: every,
                    taken: 0,
                    tried: 0,
                    tallies: vec![Tally::default(); G::SHAPES * G::RESIDUES],
                }
            } else {
                Stage::Done(None)
            },
        }
    }

    pub(crate) fn unweighed() -> Self {
        Self::new(0, 0)
    }

    pub(crate) fn is_done(&self) -> bool {
        matches!(self.stage, Stage::Done(_))
    }

    pub(crate) fn note(&mut self, interleaved: &[f32]) {
        if self.is_done() {
            return;
        }
        for frame in interleaved.chunks_exact(self.channels) {
            self.held[0].push(frame[0] * SIXTEEN_BIT_STEPS);
            if self.paired {
                self.held[1].push(frame[1] * SIXTEEN_BIT_STEPS);
            }
        }
        self.reached += (interleaved.len() / self.channels) as u64;
        self.advance();
    }

    const fn searched_through() -> u64 {
        (G::RESIDUES + (G::FRAMES_A_SEARCH - 1) * G::HOP + G::SPAN) as u64
    }

    const fn confirmed_through() -> u64 {
        (G::RESIDUES + G::CONTROL_SHIFT + (G::CONFIRMED_FRAMES - 1) * G::HOP + G::SPAN) as u64
    }

    fn advance(&mut self) {
        loop {
            match &self.stage {
                Stage::Searching { next, .. } => {
                    let next = *next;
                    self.forget_before(next);
                    if self.reached < next + Self::searched_through() {
                        return;
                    }
                    self.search_at(next);
                }
                Stage::Confirming { from, .. } => {
                    let from = *from;
                    self.forget_before(from);
                    if self.reached < from + Self::confirmed_through() {
                        return;
                    }
                    self.confirm();
                }
                Stage::Done(_) => {
                    self.held = [Vec::new(), Vec::new()];
                    self.grid = None;
                    return;
                }
            }
        }
    }

    fn forget_before(&mut self, position: u64) {
        let gone = position.saturating_sub(self.held_from) as usize;
        let gone = gone.min(self.held[0].len());
        for lane in &mut self.held {
            lane.drain(..gone.min(lane.len()));
        }
        self.held_from += gone as u64;
    }

    fn tally_frame(&mut self, start: usize, shape: usize) -> Tally {
        let Some(grid) = self.grid.as_mut() else {
            return Tally::default();
        };
        let at = self.held_from + start as u64;
        grid.coefficients(
            0,
            &self.held[0][start..start + G::SPAN],
            at,
            shape,
            &mut self.left,
        );
        if self.paired {
            grid.coefficients(
                1,
                &self.held[1][start..start + G::SPAN],
                at,
                shape,
                &mut self.right,
            );
        }

        let phases = grid.phases();
        let mut sum = Complex::<f32>::default();
        let mut count = 0_u32;
        let mut add = |coefficient: f32| {
            let (phase, counted) = phases.of(coefficient);
            sum += phase;
            count += counted;
        };
        for line in self.band.first..self.band.past {
            let left = self.left[line];
            add(left);
            if self.paired {
                let right = self.right[line];
                add(right);
                if G::MID_AND_SIDE {
                    add((left + right) / 2.0);
                    add((left - right) / 2.0);
                }
            }
        }
        Tally {
            sum: Complex::new(f64::from(sum.re), f64::from(sum.im)),
            count: u64::from(count),
        }
    }

    fn tally_run(&mut self, start: usize, frames: usize, shape: usize) -> Tally {
        let mut tally = Tally::default();
        for frame in 0..frames {
            tally.add(self.tally_frame(start + frame * G::HOP, shape));
        }
        tally
    }

    fn lanes(&self) -> usize {
        match (self.paired, G::MID_AND_SIDE) {
            (false, _) => 1,
            (true, false) => 2,
            (true, true) => 4,
        }
    }

    fn search_at(&mut self, next: u64) {
        let start = (next - self.held_from) as usize;
        let mut found = vec![Tally::default(); G::SHAPES * G::RESIDUES];
        for shape in 0..G::SHAPES {
            for residue in 0..G::RESIDUES {
                found[shape * G::RESIDUES + residue] =
                    self.tally_run(start + residue, G::FRAMES_A_SEARCH, shape);
            }
        }
        if let Some(grid) = self.grid.as_mut() {
            grid.forget();
        }
        let coefficients: u64 = found.iter().map(|tally| tally.count).sum();
        let offered = (found.len() * G::FRAMES_A_SEARCH * self.band.width() * self.lanes()) as f64;
        let busy = coefficients as f64 / offered >= BUSY_SHARE;

        let resume_at = next + Self::searched_through();
        let searches = self.searches;
        let Stage::Searching {
            next,
            taken,
            tried,
            tallies,
        } = &mut self.stage
        else {
            return;
        };
        *tried += 1;
        if busy {
            *taken += 1;
            for (held, more) in tallies.iter_mut().zip(&found) {
                held.add(*more);
            }
        }
        *next += self.every;

        if *taken >= searches || *tried >= TRIED_FOR_EACH_SEARCH_AT_MOST * searches {
            let chosen = (*taken > 0).then(|| chosen_from::<G>(tallies));
            let hop = G::HOP as u64;
            self.stage = match chosen {
                Some(candidates) => Stage::Confirming {
                    from: resume_at.div_ceil(hop) * hop,
                    candidates,
                },
                None => Stage::Done(None),
            };
        }
    }

    fn confirm(&mut self) {
        let Stage::Confirming { from, candidates } = &self.stage else {
            return;
        };
        let start = (*from - self.held_from) as usize;
        let candidates = candidates.clone();
        let room = self.held[0].len().saturating_sub(start);
        let frames = room
            .checked_sub(G::RESIDUES + G::CONTROL_SHIFT + G::SPAN)
            .map_or(0, |left| left / G::HOP + 1)
            .min(G::CONFIRMED_FRAMES);
        let rate = self.rate;
        let steps = candidates
            .iter()
            .map(|candidate| {
                let lattice = self.tally_run(start + candidate.residue, frames, candidate.shape);
                if let Some(grid) = self.grid.as_mut() {
                    grid.forget();
                }
                let control = self.tally_run(
                    start + candidate.residue + G::CONTROL_SHIFT,
                    frames,
                    candidate.shape,
                );
                if let Some(grid) = self.grid.as_mut() {
                    grid.forget();
                }
                Steps {
                    coding: G::CODING,
                    rate,
                    lattice: lattice.resultant() as f32,
                    control: control.resultant() as f32,
                    coefficients: lattice.count,
                }
            })
            .max_by(|one, other| one.lattice.total_cmp(&other.lattice))
            .filter(|steps| steps.coefficients > 0);
        self.stage = Stage::Done(steps);
    }

    pub(crate) fn finished(mut self) -> Option<Steps> {
        if matches!(self.stage, Stage::Confirming { .. }) {
            self.confirm();
        }
        match self.stage {
            Stage::Done(steps) => steps,
            Stage::Searching { .. } | Stage::Confirming { .. } => None,
        }
    }
}

fn chosen_from<G: Grid>(tallies: &[Tally]) -> Vec<Candidate> {
    let mut ranked: Vec<(usize, f64)> = tallies
        .iter()
        .map(|tally| tally.rayleigh())
        .enumerate()
        .collect();
    ranked.sort_by(|one, other| other.1.total_cmp(&one.1));
    ranked
        .into_iter()
        .take(CANDIDATES)
        .map(|(at, _)| Candidate {
            shape: at / G::RESIDUES,
            residue: at % G::RESIDUES,
        })
        .collect()
}

pub(crate) struct Mdct {
    hop: usize,
    fft: Arc<dyn Fft<f32>>,
    windows: Vec<Vec<f32>>,
    turning_in: Vec<Complex<f32>>,
    turning_out: Vec<Complex<f32>>,
    windowed: Vec<f32>,
    folded: Vec<f32>,
    buffer: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
}

impl Mdct {
    pub(crate) fn new(hop: usize, windows: Vec<Vec<f64>>) -> Self {
        let folded = hop / 2;
        let fft = FftPlanner::new().plan_fft_forward(folded);
        let scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        let scale = (2.0 / hop as f64).sqrt();
        let turning_in = (0..folded)
            .map(|nth| turned(1.0, -PI * (4 * nth + 1) as f64 / (4 * hop) as f64))
            .collect();
        let turning_out = (0..folded)
            .map(|nth| turned(scale, -PI * nth as f64 / hop as f64))
            .collect();
        Self {
            hop,
            fft,
            windows: windows.into_iter().map(narrowed).collect(),
            turning_in,
            turning_out,
            windowed: vec![0.0; 2 * hop],
            folded: vec![0.0; hop],
            buffer: vec![Complex::default(); folded],
            scratch,
        }
    }

    pub(crate) fn coefficients(&mut self, samples: &[f32], shape: usize, into: &mut [f32]) {
        let hop = self.hop;
        for ((slot, sample), weight) in self
            .windowed
            .iter_mut()
            .zip(samples)
            .zip(&self.windows[shape])
        {
            *slot = sample * weight;
        }
        let (front, back) = self.windowed.split_at(hop);
        let (first, second) = front.split_at(hop / 2);
        let (third, fourth) = back.split_at(hop / 2);
        let (low, high) = self.folded.split_at_mut(hop / 2);
        for ((slot, third), fourth) in low.iter_mut().zip(third.iter().rev()).zip(fourth) {
            *slot = -third - fourth;
        }
        for ((slot, first), second) in high.iter_mut().zip(first).zip(second.iter().rev()) {
            *slot = first - second;
        }
        for (((slot, even), odd), turn) in self
            .buffer
            .iter_mut()
            .zip(self.folded.iter().step_by(2))
            .zip(self.folded.iter().rev().step_by(2))
            .zip(&self.turning_in)
        {
            *slot = Complex::new(*even, *odd) * turn;
        }
        self.fft
            .process_with_scratch(&mut self.buffer, &mut self.scratch);
        for (nth, (value, turn)) in self.buffer.iter().zip(&self.turning_out).enumerate() {
            let turned = value * turn;
            into[2 * nth] = turned.re;
            into[hop - 1 - 2 * nth] = -turned.im;
        }
    }

    #[cfg(test)]
    pub(crate) fn window(&self, shape: usize) -> &[f32] {
        &self.windows[shape]
    }
}

fn narrowed(window: Vec<f64>) -> Vec<f32> {
    window.into_iter().map(|weight| weight as f32).collect()
}

pub(crate) fn sine_window(span: usize) -> Vec<f64> {
    (0..span)
        .map(|nth| (PI * (nth as f64 + 0.5) / span as f64).sin())
        .collect()
}

fn bessel_i0(value: f64) -> f64 {
    let quarter_square = value * value / 4.0;
    let mut term = 1.0;
    let mut sum = 1.0;
    for order in 1..64 {
        term *= quarter_square / (order * order) as f64;
        sum += term;
        if term < sum * f64::EPSILON {
            break;
        }
    }
    sum
}

fn kaiser_bessel_window(hop: usize) -> Vec<f64> {
    let beta = PI * KAISER_BESSEL_ALPHA;
    let kaiser: Vec<f64> = (0..=hop)
        .map(|nth| {
            let reach = 2.0 * nth as f64 / hop as f64 - 1.0;
            bessel_i0(beta * (1.0 - reach * reach).max(0.0).sqrt())
        })
        .collect();
    let whole: f64 = kaiser.iter().sum();
    let mut running = 0.0;
    let rising: Vec<f64> = kaiser[..hop]
        .iter()
        .map(|value| {
            running += value;
            (running / whole).sqrt()
        })
        .collect();
    rising
        .iter()
        .copied()
        .chain(rising.iter().rev().copied())
        .collect()
}

fn vorbis_window(span: usize) -> Vec<f64> {
    (0..span)
        .map(|nth| {
            let rising = (PI * (nth as f64 + 0.5) / span as f64).sin();
            (PI / 2.0 * rising * rising).sin()
        })
        .collect()
}

pub(crate) struct Aac {
    mdct: Mdct,
    phases: Phases,
}

impl Grid for Aac {
    const CODING: Coding = Coding::Aac;
    const HOP: usize = AAC_HOP;
    const RESIDUES: usize = AAC_HOP;
    const SPAN: usize = 2 * AAC_HOP;
    const LINES: usize = AAC_HOP;
    const SHAPES: usize = 2;
    const CONTROL_SHIFT: usize = AAC_HOP / 2;
    const FRAMES_A_SEARCH: usize = 4;
    const CONFIRMED_FRAMES: usize = 400;
    const MID_AND_SIDE: bool = true;

    fn band(rate: u32) -> Option<Band> {
        (rate <= HIGHEST_AAC_RATE).then(|| Band::of(rate, AAC_HOP, AAC_FROM_HZ, AAC_TO_HZ))
    }

    fn new(_: &Band) -> Self {
        Self {
            mdct: Mdct::new(
                AAC_HOP,
                vec![sine_window(2 * AAC_HOP), kaiser_bessel_window(AAC_HOP)],
            ),
            phases: Phases::new(QUARTER_OCTAVE_STEPS),
        }
    }

    fn phases(&self) -> &Phases {
        &self.phases
    }

    fn coefficients(&mut self, _: usize, samples: &[f32], _: u64, shape: usize, into: &mut [f32]) {
        self.mdct.coefficients(samples, shape, into);
    }
}

pub(crate) struct Vorbis {
    mdct: Mdct,
    phases: Phases,
}

impl Grid for Vorbis {
    const CODING: Coding = Coding::Vorbis;
    const HOP: usize = VORBIS_SHORT_HOP;
    const RESIDUES: usize = VORBIS_SHORT_HOP;
    const SPAN: usize = 2 * VORBIS_LONG_HOP;
    const LINES: usize = VORBIS_LONG_HOP;
    const SHAPES: usize = 1;
    const CONTROL_SHIFT: usize = VORBIS_SHORT_HOP / 2;
    const FRAMES_A_SEARCH: usize = 32;
    const CONFIRMED_FRAMES: usize = 1_600;
    const MID_AND_SIDE: bool = false;

    fn band(rate: u32) -> Option<Band> {
        (rate <= HIGHEST_VORBIS_RATE)
            .then(|| Band::of(rate, VORBIS_LONG_HOP, VORBIS_FROM_HZ, VORBIS_TO_HZ))
    }

    fn new(_: &Band) -> Self {
        Self {
            mdct: Mdct::new(VORBIS_LONG_HOP, vec![vorbis_window(2 * VORBIS_LONG_HOP)]),
            phases: Phases::new(VORBIS_TURNS_A_FLOOR_STEP / VORBIS_FLOOR_OCTAVES_A_STEP),
        }
    }

    fn phases(&self) -> &Phases {
        &self.phases
    }

    fn coefficients(&mut self, _: usize, samples: &[f32], _: u64, shape: usize, into: &mut [f32]) {
        self.mdct.coefficients(samples, shape, into);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const RATE: u32 = 44_100;
    const SECONDS: usize = 14;

    pub(crate) struct Wandering(pub(crate) u64);

    impl Wandering {
        pub(crate) fn next(&mut self) -> f64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 11) as f64 / (1_u64 << 53) as f64
        }

        pub(crate) fn laplacian(&mut self, scale: f64) -> f64 {
            let drawn = self.next() - 0.5;
            -scale * drawn.signum() * (1.0 - 2.0 * drawn.abs()).max(f64::MIN_POSITIVE).ln()
        }
    }

    fn cosine_at(nth: usize, bin: usize) -> f64 {
        (PI / AAC_HOP as f64 * (nth as f64 + 0.5 + AAC_HOP as f64 / 2.0) * (bin as f64 + 0.5)).cos()
    }

    fn naive(samples: &[f64]) -> Vec<f64> {
        (0..AAC_HOP)
            .map(|bin| {
                samples
                    .iter()
                    .enumerate()
                    .map(|(nth, sample)| sample * cosine_at(nth, bin))
                    .sum()
            })
            .collect()
    }

    fn synthesised(coefficients: impl FnMut(usize) -> f64, shape: usize) -> Vec<f32> {
        let aac = Aac::new(&Band { first: 0, past: 0 });
        synthesised_under(aac.mdct.window(shape), coefficients)
    }

    fn synthesised_under(window: &[f32], mut coefficients: impl FnMut(usize) -> f64) -> Vec<f32> {
        let frames = SECONDS * RATE as usize / AAC_HOP;
        let scale = (2.0 / AAC_HOP as f64).sqrt() / f64::from(SIXTEEN_BIT_STEPS);
        let cosines: Vec<f64> = (0..2 * AAC_HOP * AAC_HOP)
            .map(|at| cosine_at(at / AAC_HOP, at % AAC_HOP))
            .collect();

        let mut out = vec![0.0_f64; (frames + 1) * AAC_HOP];
        for frame in 0..frames {
            let held: Vec<f64> = (0..AAC_HOP).map(&mut coefficients).collect();
            for nth in 0..2 * AAC_HOP {
                let row = &cosines[nth * AAC_HOP..(nth + 1) * AAC_HOP];
                let sum: f64 = row
                    .iter()
                    .zip(&held)
                    .map(|(cosine, held)| cosine * held)
                    .sum();
                out[frame * AAC_HOP + nth] += sum * scale * f64::from(window[nth]);
            }
        }
        out.iter()
            .map(|sample| {
                (sample * f64::from(SIXTEEN_BIT_STEPS)).round() as f32 / SIXTEEN_BIT_STEPS
            })
            .collect()
    }

    pub(crate) fn step_of(bin: usize) -> f64 {
        2_f64.powf((bin / 32 % 8) as f64 / 4.0) * 24.0
    }

    pub(crate) fn quantised(seed: u64) -> impl FnMut(usize) -> f64 {
        let mut wandering = Wandering(seed);
        move |bin| {
            let level = wandering.laplacian(1.2).round();
            level.signum() * level.abs().powf(4.0 / 3.0) * step_of(bin)
        }
    }

    pub(crate) fn unquantised(seed: u64) -> impl FnMut(usize) -> f64 {
        let mut wandering = Wandering(seed);
        move |bin| wandering.laplacian(1.2) * step_of(bin)
    }

    pub(crate) fn stepped<G: Grid>(left: &[f32], right: &[f32], skipped: usize) -> Option<Steps> {
        let interleaved: Vec<f32> = left[skipped..]
            .iter()
            .zip(&right[skipped..])
            .flat_map(|(left, right)| [*left, *right])
            .collect();

        let mut stepping = Stepping::<G>::new(RATE, 2);
        for block in interleaved.chunks(4_096 * 2) {
            stepping.note(block);
        }
        stepping.finished()
    }

    #[test]
    fn the_folded_transform_is_the_mdct_by_its_definition() {
        let mut wandering = Wandering(7);
        let samples: Vec<f32> = (0..2 * AAC_HOP)
            .map(|_| wandering.next() as f32 - 0.5)
            .collect();
        let mut aac = Aac::new(&Band { first: 0, past: 0 });
        let mut fast = vec![0.0; AAC_HOP];
        let scale = (2.0 / AAC_HOP as f64).sqrt();

        for shape in 0..Aac::SHAPES {
            aac.mdct.coefficients(&samples, shape, &mut fast);
            let windowed: Vec<f64> = samples
                .iter()
                .zip(aac.mdct.window(shape))
                .map(|(sample, weight)| f64::from(*sample) * f64::from(*weight))
                .collect();

            let slow = naive(&windowed);
            for (fast, slow) in fast.iter().zip(&slow) {
                assert!((f64::from(*fast) - slow * scale).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn each_window_meets_its_mirror_in_power() {
        let aac = Aac::new(&Band { first: 0, past: 0 });
        let vorbis = Vorbis::new(&Band { first: 0, past: 0 });
        let windows = [
            aac.mdct.window(0),
            aac.mdct.window(1),
            vorbis.mdct.window(0),
        ];

        for window in windows {
            let hop = window.len() / 2;
            for nth in 0..hop {
                let sum = window[nth] * window[nth] + window[nth + hop] * window[nth + hop];
                assert!((sum - 1.0).abs() < 1e-6, "{sum}");
            }
        }
    }

    #[test]
    fn a_phase_is_the_same_whole_turns_apart_and_opposite_half_a_turn_apart() {
        let quarter = Phases::new(QUARTER_OCTAVE_STEPS);
        let floor = Phases::new(VORBIS_TURNS_A_FLOOR_STEP / VORBIS_FLOOR_OCTAVES_A_STEP);
        let apart = |phases: &Phases, octaves: f64| {
            let value = 300.0_f32;
            let moved = (f64::from(value) * octaves.exp2()) as f32;
            (phases.of(value).0 - phases.of(moved).0).norm()
        };

        assert!(apart(&quarter, 0.5) < 0.1);
        assert!(apart(&quarter, 0.125) > 1.9);
        assert!(apart(&floor, VORBIS_FLOOR_OCTAVES_A_STEP * 1.5) < 0.1);
        assert!(apart(&floor, VORBIS_FLOOR_OCTAVES_A_STEP * 0.25) > 1.9);
        assert_eq!(quarter.of(0.0).1, 0);
    }

    #[test]
    fn coefficients_on_the_quantiser_steps_are_found_at_any_frame_offset_and_free_ones_are_not() {
        for shape in 0..Aac::SHAPES {
            let left = synthesised(quantised(11), shape);
            let right = synthesised(quantised(23), shape);
            for skipped in [0, 300] {
                let steps = stepped::<Aac>(&left, &right, skipped).expect("weighed");

                assert!(steps.lattice > 0.15, "{shape} {skipped} {steps:?}");
                assert!(steps.control < steps.lattice / 10.0, "{steps:?}");
                assert!(steps.coefficients > 100_000, "{steps:?}");
                assert_eq!(steps.coding, Coding::Aac);
            }

            let left = synthesised(unquantised(11), shape);
            let right = synthesised(unquantised(23), shape);
            let steps = stepped::<Aac>(&left, &right, 0).expect("weighed");

            assert!(steps.lattice < 0.01, "{shape} {steps:?}");
        }
    }

    fn on_vorbis_floor(seed: u64) -> impl FnMut(usize) -> f64 {
        let mut wandering = Wandering(seed);
        move |bin| {
            let floor = (bin / 16 % 24) as f64 * VORBIS_FLOOR_OCTAVES_A_STEP;
            wandering.laplacian(1.2).round() * floor.exp2() * 24.0
        }
    }

    fn off_vorbis_floor(seed: u64) -> impl FnMut(usize) -> f64 {
        let mut wandering = Wandering(seed);
        move |bin| {
            let floor = (bin / 16 % 24) as f64 * VORBIS_FLOOR_OCTAVES_A_STEP;
            wandering.laplacian(1.2) * floor.exp2() * 24.0
        }
    }

    #[test]
    fn residues_on_vorbis_floor_are_found_on_any_short_block_grid_and_free_ones_are_not() {
        let vorbis = Vorbis::new(&Band { first: 0, past: 0 });
        let window = vorbis.mdct.window(0);
        let left = synthesised_under(window, on_vorbis_floor(11));
        let right = synthesised_under(window, on_vorbis_floor(23));
        for skipped in [0, 300] {
            let steps = stepped::<Vorbis>(&left, &right, skipped).expect("weighed");

            assert!(steps.lattice > 0.05, "{skipped} {steps:?}");
            assert!(steps.control < steps.lattice / 10.0, "{steps:?}");
            assert!(steps.coefficients > 100_000, "{steps:?}");
            assert_eq!(steps.coding, Coding::Vorbis);
        }

        let left = synthesised_under(window, off_vorbis_floor(11));
        let right = synthesised_under(window, off_vorbis_floor(23));
        let steps = stepped::<Vorbis>(&left, &right, 0).expect("weighed");

        assert!(steps.lattice < 0.01, "{steps:?}");
    }

    #[test]
    fn a_rate_no_aac_stream_carries_is_not_weighed() {
        let stepping = Stepping::<Aac>::new(192_000, 2);

        assert!(stepping.finished().is_none());
    }
}
