use std::{
    f64::consts::{PI, TAU},
    sync::Arc,
};

use rustfft::{Fft, FftPlanner, num_complex::Complex};

pub(crate) const HOP: usize = 1_024;
const SPAN: usize = 2 * HOP;
const FOLDED: usize = HOP / 2;
const CONTROL_SHIFT: usize = HOP / 2;

const STEPS_FROM_HZ: f32 = 4_000.0;
const STEPS_TO_HZ: f32 = 16_000.0;
const HIGHEST_AAC_RATE: u32 = 96_000;

const SIXTEEN_BIT_STEPS: f32 = 32_768.0;
const ABOVE_THE_ROUNDING_FLOOR: f32 = 8.0;
const KAISER_BESSEL_ALPHA: f64 = 4.0;

const FRAMES_A_SEARCH: usize = 4;
const SEARCHES: usize = 4;
const SEARCHES_AT_MOST: usize = 8;
const SEARCHED_EVERY_SECONDS: u64 = 2;
const BUSY_SHARE: f64 = 0.2;
const CANDIDATES: usize = 4;
const CONFIRMED_FRAMES: usize = 400;

const MANTISSA_BITS: u32 = 11;
const MANTISSA_SHIFT: u32 = f32::MANTISSA_DIGITS - 1 - MANTISSA_BITS;
const MANTISSA_BINS: usize = 1 << MANTISSA_BITS;
const STEPS_AN_OCTAVE: f64 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Steps {
    pub lattice: f32,
    pub control: f32,
    pub coefficients: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct Tally {
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

const SHAPES: usize = 2;

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

struct Transform {
    fft: Arc<dyn Fft<f32>>,
    windows: [Vec<f32>; SHAPES],
    turning_in: Vec<Complex<f32>>,
    turning_out: Vec<Complex<f32>>,
    windowed: Vec<f32>,
    folded: Vec<f32>,
    buffer: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    phases: Vec<Complex<f32>>,
}

impl Transform {
    fn new() -> Self {
        let fft = FftPlanner::new().plan_fft_forward(FOLDED);
        let scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        let scale = f64::from(SIXTEEN_BIT_STEPS) * (2.0 / HOP as f64).sqrt();
        let turned = |size: f64, angle: f64| {
            let turn = Complex::from_polar(size, angle);
            Complex::new(turn.re as f32, turn.im as f32)
        };
        let turning_in = (0..FOLDED)
            .map(|nth| turned(1.0, -PI * (4 * nth + 1) as f64 / (4 * HOP) as f64))
            .collect();
        let turning_out = (0..FOLDED)
            .map(|nth| turned(scale, -PI * nth as f64 / HOP as f64))
            .collect();
        let phases = (0..MANTISSA_BINS)
            .map(|bin| {
                let mantissa = 1.0 + (bin as f64 + 0.5) / MANTISSA_BINS as f64;
                turned(1.0, TAU * STEPS_AN_OCTAVE * mantissa.log2())
            })
            .collect();
        Self {
            fft,
            windows: [narrowed(sine_window()), narrowed(kaiser_bessel_window())],
            turning_in,
            turning_out,
            windowed: vec![0.0; SPAN],
            folded: vec![0.0; HOP],
            buffer: vec![Complex::default(); FOLDED],
            scratch,
            phases,
        }
    }

    fn coefficients(&mut self, samples: &[f32], shape: usize, into: &mut [f32]) {
        for ((slot, sample), weight) in self
            .windowed
            .iter_mut()
            .zip(samples)
            .zip(&self.windows[shape])
        {
            *slot = sample * weight;
        }
        let (front, back) = self.windowed.split_at(HOP);
        let (first, second) = front.split_at(HOP / 2);
        let (third, fourth) = back.split_at(HOP / 2);
        let (low, high) = self.folded.split_at_mut(HOP / 2);
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
            into[HOP - 1 - 2 * nth] = -turned.im;
        }
    }

    fn phase_of(&self, coefficient: f32) -> (Complex<f32>, u32) {
        let magnitude = coefficient.abs();
        let counted = u32::from(magnitude > ABOVE_THE_ROUNDING_FLOOR);
        let bin = (magnitude.to_bits() >> MANTISSA_SHIFT) as usize & (MANTISSA_BINS - 1);
        (self.phases[bin] * counted as f32, counted)
    }
}

fn narrowed(window: Vec<f64>) -> Vec<f32> {
    window.into_iter().map(|weight| weight as f32).collect()
}

fn sine_window() -> Vec<f64> {
    (0..SPAN)
        .map(|nth| (PI * (nth as f64 + 0.5) / SPAN as f64).sin())
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

fn kaiser_bessel_window() -> Vec<f64> {
    let beta = PI * KAISER_BESSEL_ALPHA;
    let kaiser: Vec<f64> = (0..=HOP)
        .map(|nth| {
            let reach = 2.0 * nth as f64 / HOP as f64 - 1.0;
            bessel_i0(beta * (1.0 - reach * reach).max(0.0).sqrt())
        })
        .collect();
    let whole: f64 = kaiser.iter().sum();
    let mut running = 0.0;
    let rising: Vec<f64> = kaiser[..HOP]
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

pub(crate) struct Stepping {
    channels: usize,
    paired: bool,
    first_bin: usize,
    past_bin: usize,
    every: u64,
    held: [Vec<f32>; 2],
    held_from: u64,
    reached: u64,
    left: Vec<f32>,
    right: Vec<f32>,
    transform: Option<Transform>,
    stage: Stage,
}

impl Stepping {
    pub(crate) fn new(rate: u32, channels: usize) -> Self {
        let nyquist = rate as f32 / 2.0;
        let bin_hz = nyquist / HOP as f32;
        let first_bin = (STEPS_FROM_HZ / bin_hz) as usize;
        let past_bin = ((STEPS_TO_HZ.min(nyquist) / bin_hz) as usize).min(HOP);
        let weighable = rate <= HIGHEST_AAC_RATE && first_bin < past_bin && channels > 0;
        let every = (u64::from(rate) * SEARCHED_EVERY_SECONDS).div_ceil(HOP as u64) * HOP as u64;
        Self {
            channels,
            paired: channels > 1,
            first_bin,
            past_bin,
            every,
            held: [Vec::new(), Vec::new()],
            held_from: 0,
            reached: 0,
            left: vec![0.0; HOP],
            right: vec![0.0; HOP],
            transform: weighable.then(Transform::new),
            stage: if weighable {
                Stage::Searching {
                    next: every,
                    taken: 0,
                    tried: 0,
                    tallies: vec![Tally::default(); SHAPES * HOP],
                }
            } else {
                Stage::Done(None)
            },
        }
    }

    pub(crate) fn unweighed() -> Self {
        Self::new(0, 0)
    }

    pub(crate) fn note(&mut self, interleaved: &[f32]) {
        if matches!(self.stage, Stage::Done(_)) {
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

    fn advance(&mut self) {
        loop {
            match &self.stage {
                Stage::Searching { next, .. } => {
                    let next = *next;
                    let wanted = next + ((FRAMES_A_SEARCH + 2) * HOP) as u64;
                    self.forget_before(next);
                    if self.reached < wanted {
                        return;
                    }
                    self.search_at(next);
                }
                Stage::Confirming { from, .. } => {
                    let from = *from;
                    self.forget_before(from);
                    if self.reached < from + ((CONFIRMED_FRAMES + 2) * HOP) as u64 {
                        return;
                    }
                    self.confirm();
                }
                Stage::Done(_) => {
                    self.held = [Vec::new(), Vec::new()];
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
        let Some(transform) = self.transform.as_mut() else {
            return Tally::default();
        };
        transform.coefficients(&self.held[0][start..start + SPAN], shape, &mut self.left);
        if self.paired {
            transform.coefficients(&self.held[1][start..start + SPAN], shape, &mut self.right);
        }

        let mut sum = Complex::<f32>::default();
        let mut count = 0_u32;
        let mut add = |coefficient: f32| {
            let (phase, counted) = transform.phase_of(coefficient);
            sum += phase;
            count += counted;
        };
        for bin in self.first_bin..self.past_bin {
            let left = self.left[bin];
            add(left);
            if self.paired {
                let right = self.right[bin];
                add(right);
                add((left + right) / 2.0);
                add((left - right) / 2.0);
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
            tally.add(self.tally_frame(start + frame * HOP, shape));
        }
        tally
    }

    fn search_at(&mut self, next: u64) {
        let start = (next - self.held_from) as usize;
        let mut found = vec![Tally::default(); SHAPES * HOP];
        for shape in 0..SHAPES {
            for residue in 0..HOP {
                found[shape * HOP + residue] =
                    self.tally_run(start + residue, FRAMES_A_SEARCH, shape);
            }
        }
        let coefficients: u64 = found.iter().map(|tally| tally.count).sum();
        let offered = (found.len()
            * FRAMES_A_SEARCH
            * (self.past_bin - self.first_bin)
            * if self.paired { 4 } else { 1 }) as f64;
        let busy = coefficients as f64 / offered >= BUSY_SHARE;

        let resume_at = next + ((FRAMES_A_SEARCH + 2) * HOP) as u64;
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

        if *taken >= SEARCHES || *tried >= SEARCHES_AT_MOST {
            let chosen = (*taken > 0).then(|| chosen_from(tallies));
            self.stage = match chosen {
                Some(candidates) => Stage::Confirming {
                    from: resume_at.div_ceil(HOP as u64) * HOP as u64,
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
        let frames = (room.saturating_sub(HOP + CONTROL_SHIFT) / HOP)
            .saturating_sub(1)
            .min(CONFIRMED_FRAMES);
        let steps = candidates
            .iter()
            .map(|candidate| {
                let lattice = self.tally_run(start + candidate.residue, frames, candidate.shape);
                let control = self.tally_run(
                    start + candidate.residue + CONTROL_SHIFT,
                    frames,
                    candidate.shape,
                );
                Steps {
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

fn chosen_from(tallies: &[Tally]) -> Vec<Candidate> {
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
            shape: at / HOP,
            residue: at % HOP,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;
    const SECONDS: usize = 14;

    struct Wandering(u64);

    impl Wandering {
        fn next(&mut self) -> f64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 11) as f64 / (1_u64 << 53) as f64
        }

        fn laplacian(&mut self, scale: f64) -> f64 {
            let drawn = self.next() - 0.5;
            -scale * drawn.signum() * (1.0 - 2.0 * drawn.abs()).max(f64::MIN_POSITIVE).ln()
        }
    }

    fn cosine_at(nth: usize, bin: usize) -> f64 {
        (PI / HOP as f64 * (nth as f64 + 0.5 + HOP as f64 / 2.0) * (bin as f64 + 0.5)).cos()
    }

    fn naive(samples: &[f64]) -> Vec<f64> {
        (0..HOP)
            .map(|bin| {
                samples
                    .iter()
                    .enumerate()
                    .map(|(nth, sample)| sample * cosine_at(nth, bin))
                    .sum()
            })
            .collect()
    }

    fn synthesised(mut coefficients: impl FnMut(usize) -> f64, shape: usize) -> Vec<f32> {
        let window = &Transform::new().windows[shape];
        let frames = SECONDS * RATE as usize / HOP;
        let scale = (2.0 / HOP as f64).sqrt() / f64::from(SIXTEEN_BIT_STEPS);
        let cosines: Vec<f64> = (0..SPAN * HOP)
            .map(|at| cosine_at(at / HOP, at % HOP))
            .collect();

        let mut out = vec![0.0_f64; (frames + 1) * HOP];
        for frame in 0..frames {
            let held: Vec<f64> = (0..HOP).map(&mut coefficients).collect();
            for nth in 0..SPAN {
                let row = &cosines[nth * HOP..(nth + 1) * HOP];
                let sum: f64 = row
                    .iter()
                    .zip(&held)
                    .map(|(cosine, held)| cosine * held)
                    .sum();
                out[frame * HOP + nth] += sum * scale * f64::from(window[nth]);
            }
        }
        out.iter()
            .map(|sample| {
                (sample * f64::from(SIXTEEN_BIT_STEPS)).round() as f32 / SIXTEEN_BIT_STEPS
            })
            .collect()
    }

    fn step_of(bin: usize) -> f64 {
        2_f64.powf((bin / 32 % 8) as f64 / 4.0) * 24.0
    }

    fn quantised(seed: u64) -> impl FnMut(usize) -> f64 {
        let mut wandering = Wandering(seed);
        move |bin| {
            let level = wandering.laplacian(1.2).round();
            level.signum() * level.abs().powf(4.0 / 3.0) * step_of(bin)
        }
    }

    fn unquantised(seed: u64) -> impl FnMut(usize) -> f64 {
        let mut wandering = Wandering(seed);
        move |bin| wandering.laplacian(1.2) * step_of(bin)
    }

    fn stepped(left: &[f32], right: &[f32], skipped: usize) -> Option<Steps> {
        let interleaved: Vec<f32> = left[skipped..]
            .iter()
            .zip(&right[skipped..])
            .flat_map(|(left, right)| [*left, *right])
            .collect();

        let mut stepping = Stepping::new(RATE, 2);
        for block in interleaved.chunks(4_096 * 2) {
            stepping.note(block);
        }
        stepping.finished()
    }

    #[test]
    fn the_folded_transform_is_the_mdct_by_its_definition() {
        let mut wandering = Wandering(7);
        let samples: Vec<f32> = (0..SPAN).map(|_| wandering.next() as f32 - 0.5).collect();
        let mut transform = Transform::new();
        let mut fast = vec![0.0; HOP];
        let scale = f64::from(SIXTEEN_BIT_STEPS) * (2.0 / HOP as f64).sqrt();

        for shape in 0..SHAPES {
            transform.coefficients(&samples, shape, &mut fast);
            let windowed: Vec<f64> = samples
                .iter()
                .zip(&transform.windows[shape])
                .map(|(sample, weight)| f64::from(*sample) * f64::from(*weight))
                .collect();

            let slow = naive(&windowed);
            for (fast, slow) in fast.iter().zip(&slow) {
                assert!((f64::from(*fast) - slow * scale).abs() < 1e-2);
            }
        }
    }

    #[test]
    fn each_window_meets_its_mirror_in_power() {
        let transform = Transform::new();

        for window in &transform.windows {
            for nth in 0..HOP {
                let sum = window[nth] * window[nth] + window[nth + HOP] * window[nth + HOP];
                assert!((sum - 1.0).abs() < 1e-6, "{sum}");
            }
        }
    }

    #[test]
    fn coefficients_on_the_quantiser_steps_are_found_at_any_frame_offset_and_free_ones_are_not() {
        for shape in 0..SHAPES {
            let left = synthesised(quantised(11), shape);
            let right = synthesised(quantised(23), shape);
            for skipped in [0, 300] {
                let steps = stepped(&left, &right, skipped).expect("weighed");

                assert!(steps.lattice > 0.15, "{shape} {skipped} {steps:?}");
                assert!(steps.control < steps.lattice / 10.0, "{steps:?}");
                assert!(steps.coefficients > 100_000, "{steps:?}");
            }

            let left = synthesised(unquantised(11), shape);
            let right = synthesised(unquantised(23), shape);
            let steps = stepped(&left, &right, 0).expect("weighed");

            assert!(steps.lattice < 0.01, "{shape} {steps:?}");
        }
    }

    #[test]
    fn a_rate_no_aac_stream_carries_is_not_weighed() {
        let stepping = Stepping::new(192_000, 2);

        assert!(stepping.finished().is_none());
    }
}
