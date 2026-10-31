use std::{
    f64::consts::{LN_2, PI, TAU},
    ops::Range,
};

use ahash::AHashMap;
use rustfft::num_complex::Complex;

use crate::{
    pitch::{LONGEST_CODED_PERIOD, LONGEST_PERIOD, Pitch, SHORTEST_PERIOD},
    steps::{Band, Coding, Grid, Mdct, Taking, Tally, turned},
};

const OPUS_RATE: u32 = 48_000;
const FRAME: usize = 960;
const OVERLAP: usize = 120;
const SHORTS: usize = 8;
const QUIET: usize = (FRAME - OVERLAP) / 2;
const BANDS: usize = 21;
const BAND_EDGES: [usize; BANDS + 1] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 34, 40, 48, 60, 78, 100,
];
const ENERGY_MEANS: [f64; BANDS] = [
    6.4375, 6.25, 5.75, 5.3125, 5.0625, 4.8125, 4.5, 4.375, 4.875, 4.6875, 4.5625, 4.4375, 4.875,
    4.625, 4.3125, 4.5, 4.375, 4.625, 4.75, 4.4375, 3.75,
];
const EMPHASIS: f32 = 0.85;
const SCALE_OCTAVES: f64 = 1.25;
const OLDEST_ENERGY: f64 = -9.0;
const INTER_PREDICTION: f64 = 0.5;
const INTER_DECAY: f64 = 0.2;
const INTRA_DECAY: f64 = 0.15;
const TURNS_AN_ENERGY_STEP: f64 = 32.0;
const ROUNDING_A_LINE: f64 = 0.4;
const ROUNDING_NOISE_TURNS: f64 = 0.25;
const PHASE_BINS: usize = 1 << 12;
const ENERGIES_KEPT_AT_MOST: usize = 1 << 15;
const WINDOW: usize = 2 * FRAME;
const FIRST_SAMPLE: usize = FRAME + 1;
const OWN_FILTER_FROM: usize = QUIET + OVERLAP;
const TAPSETS: [[f32; 3]; 3] = [
    [0.306_640_63, 0.217_041_02, 0.129_638_67],
    [0.463_867_2, 0.268_066_4, 0.0],
    [0.799_804_7, 0.100_097_656, 0.0],
];
const GAIN_STEP: f32 = 0.093_75;
const GAINS: usize = 8;
const PERIODS_EITHER_SIDE: usize = 2;
const TAP_REACH: usize = 2;
const LAGS: usize = 2 * (PERIODS_EITHER_SIDE + TAP_REACH) + 1;
const GAIN_STEPS_OVER_THE_ESTIMATE: usize = 1;
const ENCODER_FILTERS_FROM: f32 = 0.2;

#[derive(Clone, Copy)]
struct Energies {
    levels: [f64; BANDS],
    weights: [f32; BANDS],
}

impl Energies {
    fn of(powers: &[f64; BANDS]) -> Self {
        let mut energies = Self {
            levels: [0.0; BANDS],
            weights: [0.0; BANDS],
        };
        for (band, power) in powers.iter().enumerate() {
            let lines = (BAND_EDGES[band + 1] - BAND_EDGES[band]) * SHORTS;
            let level = 0.5 * power.max(f64::MIN_POSITIVE).log2();
            let rounding = 0.5 * (lines as f64 * ROUNDING_A_LINE * ROUNDING_A_LINE).log2();
            let rounding_in_turns =
                ROUNDING_NOISE_TURNS * TURNS_AN_ENERGY_STEP * (-(level - rounding)).exp2()
                    / (LN_2 * (lines as f64).sqrt());
            energies.levels[band] = level - ENERGY_MEANS[band] - SCALE_OCTAVES;
            energies.weights[band] =
                (-2.0 * PI * PI * rounding_in_turns * rounding_in_turns).exp() as f32;
        }
        energies
    }

    fn of_lines(coefficients: &[f32]) -> Self {
        let mut powers = [0.0_f64; BANDS];
        for (power, edges) in powers.iter_mut().zip(BAND_EDGES.windows(2)) {
            *power = coefficients[edges[0] * SHORTS..edges[1] * SHORTS]
                .iter()
                .map(|value| f64::from(*value) * f64::from(*value))
                .sum();
        }
        Self::of(&powers)
    }
}

#[derive(Clone, Copy)]
enum Block {
    Long,
    Short,
}

#[derive(Clone, Copy)]
struct Kind {
    before: Block,
    now: Block,
    intra: bool,
}

const KINDS: [Kind; 8] = {
    let blocks = [Block::Long, Block::Short];
    let mut kinds = [Kind {
        before: Block::Long,
        now: Block::Long,
        intra: false,
    }; 8];
    let mut at = 0;
    while at < 8 {
        kinds[at] = Kind {
            before: blocks[at & 1],
            now: blocks[(at >> 1) & 1],
            intra: at >> 2 == 1,
        };
        at += 1;
    }
    kinds
};

pub(crate) struct Opus {
    long: Mdct,
    short: Mdct,
    coefficients: Vec<f32>,
    emphasised: Vec<f32>,
    energies: AHashMap<(usize, u64, bool), Energies>,
    weights: [[f32; BANDS]; 2],
    phases: Vec<Complex<f32>>,
    undoing: Undoing,
}

impl Opus {
    fn energies(&mut self, lane: usize, samples: &[f32], at: u64, block: Block) -> Energies {
        let short = matches!(block, Block::Short);
        if let Some(energies) = self.energies.get(&(lane, at, short)) {
            return *energies;
        }
        let mut powers = [0.0_f64; BANDS];
        let mut add = |coefficients: &[f32], lines_a_unit: usize| {
            for (band, edges) in BAND_EDGES.windows(2).enumerate() {
                powers[band] += coefficients[edges[0] * lines_a_unit..edges[1] * lines_a_unit]
                    .iter()
                    .map(|value| f64::from(*value) * f64::from(*value))
                    .sum::<f64>();
            }
        };
        self.emphasised.clear();
        self.emphasised
            .extend(samples.iter().enumerate().map(|(nth, sample)| {
                let before = if nth > 0 { samples[nth - 1] } else { 0.0 };
                sample - EMPHASIS * before
            }));
        match block {
            Block::Long => {
                self.long
                    .coefficients(&self.emphasised[1..], 0, &mut self.coefficients);
                add(&self.coefficients, SHORTS);
            }
            Block::Short => {
                for nth in 0..SHORTS {
                    let from = 1 + QUIET + nth * OVERLAP;
                    self.short.coefficients(
                        &self.emphasised[from..from + 2 * OVERLAP],
                        0,
                        &mut self.coefficients[..OVERLAP],
                    );
                    add(&self.coefficients[..OVERLAP], 1);
                }
            }
        }
        let energies = Energies::of(&powers);
        if self.energies.len() >= ENERGIES_KEPT_AT_MOST {
            self.energies.clear();
        }
        self.energies.insert((lane, at, short), energies);
        energies
    }

    fn phase(&self, fine: f32) -> Complex<f32> {
        phase_of(&self.phases, fine)
    }
}

fn window(span: usize, quiet: usize) -> Vec<f64> {
    let rising = |nth: usize| {
        let inner = (TAU / 4.0 * (nth as f64 + 0.5) / OVERLAP as f64).sin();
        (TAU / 4.0 * inner * inner).sin()
    };
    (0..span)
        .map(|nth| match nth {
            nth if nth < quiet => 0.0,
            nth if nth < quiet + OVERLAP => rising(nth - quiet),
            nth if nth < span - quiet - OVERLAP => 1.0,
            nth if nth < span - quiet => rising(span - quiet - 1 - nth),
            _ => 0.0,
        })
        .collect()
}

impl Grid for Opus {
    const CODING: Coding = Coding::Opus;
    const HOP: usize = FRAME;
    const RESIDUES: usize = FRAME;
    const SPAN: usize = 3 * FRAME + 1;
    const LINES: usize = BANDS;
    const SHAPES: usize = 1;
    const KINDS: usize = KINDS.len();
    const CONTROL_SHIFT: usize = FRAME / 2 + OVERLAP / 2;
    const FRAMES_A_SEARCH: usize = 16;
    const CONFIRMED_FRAMES: usize = 400;
    const MID_AND_SIDE: bool = false;
    const UNDOES: bool = true;

    fn searched(kind: usize) -> bool {
        let Kind { before, now, intra } = KINDS[kind];
        matches!((before, now, intra), (Block::Long, Block::Long, false))
    }

    fn band(rate: u32) -> Option<Band> {
        (rate == OPUS_RATE).then_some(Band {
            first: 0,
            past: BANDS,
        })
    }

    fn new(_: u32, _: &Band) -> Self {
        Self {
            long: Mdct::new(FRAME, vec![window(2 * FRAME, QUIET)]),
            short: Mdct::new(OVERLAP, vec![window(2 * OVERLAP, 0)]),
            coefficients: vec![0.0; FRAME],
            emphasised: Vec::with_capacity(2 * FRAME + 1),
            energies: AHashMap::new(),
            weights: [[0.0; BANDS]; 2],
            phases: (0..PHASE_BINS)
                .map(|bin| turned(1.0, TAU * (bin as f64 + 0.5) / PHASE_BINS as f64))
                .collect(),
            undoing: Undoing::new(),
        }
    }

    fn tally(&self, lines: Range<usize>, left: &[f32], right: Option<&[f32]>) -> Tally {
        let mut tally = Tally::default();
        for (lane, fines) in [Some(left), right].into_iter().flatten().enumerate() {
            for band in lines.clone() {
                let weight = self.weights[lane][band];
                tally.add(Tally::weighed(self.phase(fines[band]) * weight, weight));
            }
        }
        tally
    }

    fn coefficients(&mut self, _: usize, _: &[f32], _: u64, _: usize, _: &mut [f32]) {}

    fn frame(
        &mut self,
        taking: Taking,
        samples: &[f32],
        _: &Band,
        into: &mut [f32],
    ) -> Range<usize> {
        let Kind { before, now, intra } = KINDS[taking.kind];
        let old = self.energies(taking.lane, &samples[..=2 * FRAME], taking.at, before);
        let new = self.energies(
            taking.lane,
            &samples[FRAME..],
            taking.at + FRAME as u64,
            now,
        );
        into[..BANDS].copy_from_slice(&fines(&old, &new, intra));
        self.weights[taking.lane.min(1)] = new.weights;
        0..BANDS
    }

    fn forget(&mut self) {
        self.energies.clear();
        self.undoing.next = None;
    }

    fn undone(&mut self, at: u64, lanes: &[&[f32]]) -> Option<Tally> {
        let Self {
            long,
            undoing,
            phases,
            ..
        } = self;
        Some(undoing.undone(long, phases, at, lanes))
    }
}

fn fines(old: &Energies, new: &Energies, intra: bool) -> [f32; BANDS] {
    let (prediction, decay) = if intra {
        (0.0, INTRA_DECAY)
    } else {
        (INTER_PREDICTION, INTER_DECAY)
    };
    let mut running = 0.0;
    let mut fines = [0.0; BANDS];
    for (band, fine) in fines.iter_mut().enumerate() {
        let residual =
            new.levels[band] - prediction * old.levels[band].max(OLDEST_ENERGY) - running;
        let step = residual.round();
        *fine = (residual - step) as f32;
        running += step - decay * step;
    }
    fines
}

fn phase_of(phases: &[Complex<f32>], fine: f32) -> Complex<f32> {
    let turn = (f64::from(fine) * TURNS_AN_ENERGY_STEP).rem_euclid(1.0);
    phases[(turn * PHASE_BINS as f64) as usize % PHASE_BINS]
}

fn weighed(phases: &[Complex<f32>], old: &Energies, new: &Energies) -> Tally {
    let mut tally = Tally::default();
    for (fine, weight) in fines(old, new, false).iter().zip(&new.weights) {
        tally.add(Tally::weighed(phase_of(phases, *fine) * *weight, *weight));
    }
    tally
}

#[derive(Clone, Copy)]
struct PostFilter {
    period: usize,
    gain: f32,
    tapset: usize,
}

const NO_POST_FILTER: PostFilter = PostFilter {
    period: SHORTEST_PERIOD,
    gain: 0.0,
    tapset: 0,
};

impl PostFilter {
    fn combed(self, lane: &[f32], at: usize) -> f32 {
        let taps = TAPSETS[self.tapset];
        let centre = at - self.period;
        taps[0] * lane[centre]
            + taps[1] * (lane[centre - 1] + lane[centre + 1])
            + taps[2] * (lane[centre - 2] + lane[centre + 2])
    }
}

fn own_share(sample: usize) -> f32 {
    match sample {
        sample if sample < OWN_FILTER_FROM => 0.0,
        sample if sample < OWN_FILTER_FROM + OVERLAP => {
            let inner =
                (TAU / 4.0 * ((sample - OWN_FILTER_FROM) as f64 + 0.5) / OVERLAP as f64).sin();
            let rising = (TAU / 4.0 * inner * inner).sin() as f32;
            rising * rising
        }
        _ => 1.0,
    }
}

#[derive(Clone, Copy)]
struct Crossed {
    own: [f64; BANDS],
    shared: [f64; BANDS],
    combed: [f64; BANDS],
}

struct Undoing {
    pitch: Pitch,
    next: Option<u64>,
    before: PostFilter,
    old: [Option<Energies>; 2],
    emphasised: [Vec<f32>; 2],
    taken: Vec<f32>,
    plain: [Vec<f32>; 2],
    kept: [Vec<f32>; 2],
    shifted: [Vec<Vec<f32>>; 2],
    combed: Vec<f32>,
}

impl Undoing {
    fn new() -> Self {
        Self {
            pitch: Pitch::new(FRAME),
            next: None,
            before: NO_POST_FILTER,
            old: [None; 2],
            emphasised: [Vec::new(), Vec::new()],
            taken: vec![0.0; WINDOW],
            plain: [vec![0.0; FRAME], vec![0.0; FRAME]],
            kept: [vec![0.0; FRAME], vec![0.0; FRAME]],
            shifted: [vec![vec![0.0; FRAME]; LAGS], vec![vec![0.0; FRAME]; LAGS]],
            combed: vec![0.0; FRAME],
        }
    }

    fn undone(
        &mut self,
        long: &mut Mdct,
        phases: &[Complex<f32>],
        at: u64,
        lanes: &[&[f32]],
    ) -> Tally {
        if self.next != Some(at) {
            self.before = NO_POST_FILTER;
            self.old = [None; 2];
            self.pitch.forget();
        }
        let lanes = &lanes[..lanes.len().min(2)];
        for (emphasised, samples) in self.emphasised.iter_mut().zip(lanes) {
            emphasised.clear();
            emphasised.push(samples[0]);
            emphasised.extend(samples.windows(2).map(|pair| pair[1] - EMPHASIS * pair[0]));
        }
        let heard_from = FIRST_SAMPLE + OWN_FILTER_FROM - LONGEST_PERIOD;
        let heard_to = FIRST_SAMPLE + OWN_FILTER_FROM + FRAME;
        let heard: Vec<&[f32]> = self.emphasised[..lanes.len()]
            .iter()
            .map(|emphasised| &emphasised[heard_from..heard_to])
            .collect();
        let (period, estimated_gain) = self.pitch.period(&heard);
        let steps_at_most = (estimated_gain / GAIN_STEP + 0.5).floor().max(1.0) as usize
            + GAIN_STEPS_OVER_THE_ESTIMATE;
        let lowest_lag = period - PERIODS_EITHER_SIDE - TAP_REACH;

        let mut plain_old = [None; 2];
        for (lane, plain_old) in plain_old.iter_mut().enumerate().take(lanes.len()) {
            let emphasised = &self.emphasised[lane];
            long.coefficients(
                &emphasised[FIRST_SAMPLE..FIRST_SAMPLE + WINDOW],
                0,
                &mut self.plain[lane],
            );
            if self.old[lane].is_none() || self.before.gain > 0.0 {
                long.coefficients(
                    &emphasised[FIRST_SAMPLE - FRAME..FIRST_SAMPLE + FRAME],
                    0,
                    &mut self.combed,
                );
                *plain_old = Some(Energies::of_lines(&self.combed));
            }
            if self.before.gain > 0.0 {
                let before = self.before;
                for (sample, slot) in self.taken.iter_mut().enumerate() {
                    *slot = if sample < QUIET {
                        0.0
                    } else {
                        let at = FIRST_SAMPLE + sample;
                        emphasised[at]
                            - (1.0 - own_share(sample))
                                * before.gain
                                * before.combed(emphasised, at)
                    };
                }
                long.coefficients(&self.taken, 0, &mut self.kept[lane]);
            }
            for (nth, shifted) in self.shifted[lane].iter_mut().enumerate() {
                let lag = lowest_lag + nth;
                for (sample, slot) in self.taken.iter_mut().enumerate() {
                    *slot = if sample < OWN_FILTER_FROM {
                        0.0
                    } else {
                        own_share(sample) * emphasised[FIRST_SAMPLE + sample - lag]
                    };
                }
                long.coefficients(&self.taken, 0, shifted);
            }
        }

        let mut histories: Vec<(bool, [Energies; 2])> = Vec::with_capacity(2);
        let chained = [0, 1].map(|lane| {
            self.old[lane]
                .or(plain_old[lane])
                .unwrap_or(Energies::of(&[0.0; BANDS]))
        });
        histories.push((self.before.gain > 0.0, chained));
        if self.before.gain > 0.0 {
            histories.push((
                false,
                [0, 1].map(|lane| plain_old[lane].unwrap_or(chained[lane])),
            ));
        }

        let mut best: Option<(f64, Tally, PostFilter, [Energies; 2])> = None;
        let mut consider = |tally: Tally, filter: PostFilter, news: [Energies; 2]| {
            let strength = tally.rayleigh();
            if best.as_ref().is_none_or(|(held, ..)| strength > *held) {
                best = Some((strength, tally, filter, news));
            }
        };
        for (kept, olds) in &histories {
            let own: Vec<[f64; BANDS]> = (0..lanes.len())
                .map(|lane| {
                    let spectrum = if *kept {
                        &self.kept[lane]
                    } else {
                        &self.plain[lane]
                    };
                    powers(spectrum, spectrum)
                })
                .collect();
            let mut news = [Energies::of(&[0.0; BANDS]); 2];
            let mut tally = Tally::default();
            for lane in 0..lanes.len() {
                news[lane] = Energies::of(&own[lane]);
                tally.add(weighed(phases, &olds[lane], &news[lane]));
            }
            consider(tally, NO_POST_FILTER, news);

            for offset in 0..=2 * PERIODS_EITHER_SIDE {
                let candidate = lowest_lag + TAP_REACH + offset;
                if estimated_gain < ENCODER_FILTERS_FROM
                    || !(SHORTEST_PERIOD..=LONGEST_CODED_PERIOD).contains(&candidate)
                {
                    continue;
                }
                let centre = TAP_REACH + offset;
                for (tapset, taps) in TAPSETS.iter().enumerate() {
                    let crossed: Vec<Crossed> = (0..lanes.len())
                        .map(|lane| {
                            let shifted = &self.shifted[lane];
                            for (line, slot) in self.combed.iter_mut().enumerate() {
                                *slot = taps[0] * shifted[centre][line]
                                    + taps[1]
                                        * (shifted[centre - 1][line] + shifted[centre + 1][line])
                                    + taps[2]
                                        * (shifted[centre - 2][line] + shifted[centre + 2][line]);
                            }
                            let spectrum = if *kept {
                                &self.kept[lane]
                            } else {
                                &self.plain[lane]
                            };
                            Crossed {
                                own: own[lane],
                                shared: powers(spectrum, &self.combed),
                                combed: powers(&self.combed, &self.combed),
                            }
                        })
                        .collect();
                    for step in 1..=GAINS.min(steps_at_most) {
                        let gain = GAIN_STEP * step as f32;
                        let filter = PostFilter {
                            period: candidate,
                            gain,
                            tapset,
                        };
                        let mut news = [Energies::of(&[0.0; BANDS]); 2];
                        let mut tally = Tally::default();
                        for (lane, crossed) in crossed.iter().enumerate() {
                            let g = f64::from(gain);
                            let filtered: [f64; BANDS] = std::array::from_fn(|band| {
                                (crossed.own[band] - 2.0 * g * crossed.shared[band]
                                    + g * g * crossed.combed[band])
                                    .max(0.0)
                            });
                            news[lane] = Energies::of(&filtered);
                            tally.add(weighed(phases, &olds[lane], &news[lane]));
                        }
                        consider(tally, filter, news);
                    }
                }
            }
        }

        let (_, tally, filter, news) = best.unwrap_or((
            0.0,
            Tally::default(),
            NO_POST_FILTER,
            [Energies::of(&[0.0; BANDS]); 2],
        ));
        self.before = filter;
        for (old, new) in self.old.iter_mut().zip(news).take(lanes.len()) {
            *old = Some(new);
        }
        self.pitch.heard(filter.gain);
        self.next = Some(at + FRAME as u64);
        tally
    }
}

fn powers(one: &[f32], other: &[f32]) -> [f64; BANDS] {
    let mut powers = [0.0_f64; BANDS];
    for (power, edges) in powers.iter_mut().zip(BAND_EDGES.windows(2)) {
        *power = one[edges[0] * SHORTS..edges[1] * SHORTS]
            .iter()
            .zip(&other[edges[0] * SHORTS..edges[1] * SHORTS])
            .map(|(a, b)| f64::from(*a) * f64::from(*b))
            .sum();
    }
    powers
}

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;

    use super::*;
    use crate::steps::{SIXTEEN_BIT_STEPS, Stepping, Steps, tests::Wandering};

    const SECONDS: usize = 16;
    const FINE_BITS_AT_MOST: f64 = 4.0;
    const LEVEL: f64 = 3.0;

    fn synthesised(seed: u64, on_the_steps: bool) -> Vec<f32> {
        let mut wandering = Wandering(seed);
        let frames = SECONDS * OPUS_RATE as usize / FRAME;
        let window = window(2 * FRAME, QUIET);
        let cosines: Vec<f64> = (0..2 * FRAME * FRAME)
            .map(|at| {
                let (nth, line) = (at / FRAME, at % FRAME);
                (PI / FRAME as f64 * (nth as f64 + 0.5 + FRAME as f64 / 2.0) * (line as f64 + 0.5))
                    .cos()
            })
            .collect();
        let scale = (2.0 / FRAME as f64).sqrt();
        let mut old = [LEVEL; BANDS];
        let mut out = vec![0.0_f64; (frames + 2) * FRAME];
        for frame in 0..frames {
            let mut running = 0.0;
            let mut levels = [0.0; BANDS];
            for band in 0..BANDS {
                let predicted = INTER_PREDICTION * old[band].max(OLDEST_ENERGY) + running;
                let wanted = LEVEL + wandering.laplacian(1.0);
                let step = (wanted - predicted).round();
                let fine = if on_the_steps {
                    let bits = (wandering.next() * FINE_BITS_AT_MOST).floor();
                    let levels_a_step = bits.exp2();
                    ((wandering.next() * levels_a_step).floor() + 0.5) / levels_a_step - 0.5
                } else {
                    wandering.next() - 0.5
                };
                levels[band] = predicted + step + fine;
                running += step - INTER_DECAY * step;
            }
            old = levels;
            let mut lines = vec![0.0_f64; FRAME];
            for band in 0..BANDS {
                let range = BAND_EDGES[band] * SHORTS..BAND_EDGES[band + 1] * SHORTS;
                let shape: Vec<f64> = range.clone().map(|_| wandering.laplacian(1.0)).collect();
                let norm = shape.iter().map(|value| value * value).sum::<f64>().sqrt();
                let gain = (levels[band] + ENERGY_MEANS[band] + SCALE_OCTAVES).exp2();
                for (line, value) in range.zip(&shape) {
                    lines[line] = value / norm * gain;
                }
            }
            for nth in 0..2 * FRAME {
                let row = &cosines[nth * FRAME..(nth + 1) * FRAME];
                let sum: f64 = row
                    .iter()
                    .zip(&lines)
                    .map(|(cosine, line)| cosine * line)
                    .sum();
                out[frame * FRAME + nth] += sum * scale * window[nth];
            }
        }
        let mut before = 0.0;
        out.iter()
            .map(|sample| {
                before = sample + f64::from(EMPHASIS) * before;
                before.round() as f32 / SIXTEEN_BIT_STEPS
            })
            .collect()
    }

    fn stepped(left: &[f32], right: &[f32], skipped: usize) -> Option<Steps> {
        let interleaved: Vec<f32> = left[skipped..]
            .iter()
            .zip(&right[skipped..])
            .flat_map(|(left, right)| [*left, *right])
            .collect();
        let mut stepping = Stepping::<Opus>::new(OPUS_RATE, 2);
        for block in interleaved.chunks(4_096 * 2) {
            stepping.note(block);
        }
        stepping.finished()
    }

    #[test]
    fn band_energies_on_the_coarse_and_fine_steps_of_their_prediction_are_found_and_free_ones_are_not()
     {
        let left = synthesised(11, true);
        let right = synthesised(23, true);
        for skipped in [0, 300] {
            let steps = stepped(&left, &right, skipped).expect("weighed");

            assert!(steps.lattice > 0.3, "{skipped} {steps:?}");
            assert!(steps.control < steps.lattice / 10.0, "{steps:?}");
            assert!(steps.coefficients > 5_000, "{steps:?}");
            assert_eq!(steps.coding, Coding::Opus);
        }

        let left = synthesised(11, false);
        let right = synthesised(23, false);
        let steps = stepped(&left, &right, 0).expect("weighed");

        assert!(steps.lattice < 0.03, "{steps:?}");
    }

    #[test]
    fn a_rate_no_opus_stream_is_decoded_at_is_not_weighed() {
        let stepping = Stepping::<Opus>::new(44_100, 2);

        assert!(stepping.finished().is_none());
    }
}
