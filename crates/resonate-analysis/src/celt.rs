use std::{f64::consts::TAU, ops::Range};

use ahash::AHashMap;
use rustfft::num_complex::Complex;

use crate::steps::{Band, Coding, Grid, Mdct, Taking, Tally, turned};

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
const HEARD_FROM_OCTAVES: f64 = 6.0;
const HEARD_OVER_OCTAVES: f64 = 4.0;
const PHASE_BINS: usize = 1 << 12;
const ENERGIES_KEPT_AT_MOST: usize = 1 << 15;

#[derive(Clone, Copy)]
struct Energies {
    levels: [f64; BANDS],
    weights: [f32; BANDS],
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
        let mut energies = Energies {
            levels: [0.0; BANDS],
            weights: [0.0; BANDS],
        };
        for (band, power) in powers.iter().enumerate() {
            let lines = (BAND_EDGES[band + 1] - BAND_EDGES[band]) * SHORTS;
            let level = 0.5 * power.max(f64::MIN_POSITIVE).log2();
            let rounding = 0.5 * (lines as f64 * ROUNDING_A_LINE * ROUNDING_A_LINE).log2();
            let t = ((level - rounding - HEARD_FROM_OCTAVES) / HEARD_OVER_OCTAVES).clamp(0.0, 1.0);
            energies.levels[band] = level - ENERGY_MEANS[band] - SCALE_OCTAVES;
            energies.weights[band] = (t * t * (3.0 - 2.0 * t)) as f32;
        }
        if self.energies.len() >= ENERGIES_KEPT_AT_MOST {
            self.energies.clear();
        }
        self.energies.insert((lane, at, short), energies);
        energies
    }

    fn phase(&self, fine: f32) -> Complex<f32> {
        let turn = (f64::from(fine) * TURNS_AN_ENERGY_STEP).rem_euclid(1.0);
        self.phases[(turn * PHASE_BINS as f64) as usize % PHASE_BINS]
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
    const FRAMES_A_SEARCH: usize = 8;
    const CONFIRMED_FRAMES: usize = 400;
    const MID_AND_SIDE: bool = false;

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
        let (prediction, decay) = if intra {
            (0.0, INTRA_DECAY)
        } else {
            (INTER_PREDICTION, INTER_DECAY)
        };
        let mut running = 0.0;
        for (band, fine) in into[..BANDS].iter_mut().enumerate() {
            let residual =
                new.levels[band] - prediction * old.levels[band].max(OLDEST_ENERGY) - running;
            let step = residual.round();
            *fine = (residual - step) as f32;
            running += step - decay * step;
        }
        self.weights[taking.lane.min(1)] = new.weights;
        0..BANDS
    }

    fn forget(&mut self) {
        self.energies.clear();
    }
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
