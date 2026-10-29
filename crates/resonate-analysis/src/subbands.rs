use std::ops::Range;

use crate::{
    granules::{POLYPHASE_TAPS, Polyphase, SUBBANDS},
    steps::{Band, Coding, Grid, SIXTEEN_BIT_STEPS, Taking, Tally},
};

const PART: usize = 12;
const HOP: usize = SUBBANDS * PART;
const SPAN: usize = (PART - 1) * SUBBANDS + POLYPHASE_TAPS;
const MP2_FROM_HZ: f32 = 0.0;
const MP2_TO_HZ: f32 = 20_000.0;
const HIGHEST_MP2_RATE: u32 = 48_000;

const LEVELS: [u32; 17] = [
    3, 5, 7, 9, 15, 31, 63, 127, 255, 511, 1_023, 2_047, 4_095, 8_191, 16_383, 32_767, 65_535,
];
const SCALE_FACTORS: usize = 63;
const SCALE_FACTOR_STEPS_AN_OCTAVE: f32 = 3.0;
const LOUDEST_SCALE_FACTOR: f32 = 2.0;
const SCALE_FACTORS_ABOVE_THE_PEAK: f32 = 3.0;
const SCALE_FACTOR_SLACK: f32 = 0.1;
const OFF_THE_STEP_AT_MOST: f32 = 0.1;
const PEAK_COUNTED_FROM: f32 = 32.0;
const SPOKEN_LEVELS_AT_LEAST: usize = 3;

pub(crate) struct Mp2 {
    polyphase: Polyphase,
    steps: Vec<[f32; SCALE_FACTORS]>,
}

impl Mp2 {
    fn on_the_steps(&self, part: &[f32]) -> Option<bool> {
        let peak = part
            .iter()
            .fold(0.0_f32, |peak, value| peak.max(value.abs()));
        if peak < PEAK_COUNTED_FROM {
            return None;
        }
        let full_scale = LOUDEST_SCALE_FACTOR * SIXTEEN_BIT_STEPS;
        let reached = SCALE_FACTOR_STEPS_AN_OCTAVE * (full_scale / peak).log2();
        let highest = (reached + SCALE_FACTOR_SLACK)
            .floor()
            .min((SCALE_FACTORS - 1) as f32);
        let lowest = (reached - SCALE_FACTORS_ABOVE_THE_PEAK).ceil().max(0.0);
        if highest < 0.0 {
            return Some(false);
        }
        let factors = lowest as usize..=highest as usize;
        let found = LEVELS.iter().zip(&self.steps).any(|(levels, steps)| {
            let top = (levels / 2) as f32;
            steps[factors.clone()].iter().any(|step| {
                let reaches = (peak / step).round();
                reaches >= 1.0
                    && reaches <= top
                    && part.iter().all(|value| {
                        let at = value / step;
                        (at - at.round()).abs() <= OFF_THE_STEP_AT_MOST
                    })
                    && part
                        .iter()
                        .filter(|value| (*value / step).abs() >= 0.5)
                        .count()
                        >= SPOKEN_LEVELS_AT_LEAST
            })
        });
        Some(found)
    }
}

impl Grid for Mp2 {
    const CODING: Coding = Coding::Mp2;
    const HOP: usize = HOP;
    const RESIDUES: usize = HOP;
    const SPAN: usize = SPAN;
    const LINES: usize = HOP;
    const SHAPES: usize = 1;
    const CONTROL_SHIFT: usize = SUBBANDS / 2;
    const FRAMES_A_SEARCH: usize = 12;
    const CONFIRMED_FRAMES: usize = 1_200;
    const MID_AND_SIDE: bool = false;

    fn band(rate: u32) -> Option<Band> {
        (rate <= HIGHEST_MP2_RATE).then(|| Band::of(rate, SUBBANDS, MP2_FROM_HZ, MP2_TO_HZ))
    }

    fn new(_: u32, band: &Band) -> Self {
        let full_scale = LOUDEST_SCALE_FACTOR * SIXTEEN_BIT_STEPS;
        let steps = LEVELS
            .iter()
            .map(|levels| {
                std::array::from_fn(|factor| {
                    let scale =
                        full_scale * (-(factor as f32) / SCALE_FACTOR_STEPS_AN_OCTAVE).exp2();
                    2.0 * scale / *levels as f32
                })
            })
            .collect();
        Self {
            polyphase: Polyphase::new(band.first..band.past),
            steps,
        }
    }

    fn tally(&self, lines: Range<usize>, left: &[f32], right: Option<&[f32]>) -> Tally {
        let mut on = 0;
        let mut count = 0;
        for lane in [Some(left), right].into_iter().flatten() {
            for part in lane[lines.clone()].as_chunks::<PART>().0 {
                if let Some(stepped) = self.on_the_steps(part) {
                    count += 1;
                    on += u64::from(stepped);
                }
            }
        }
        Tally::counted(on, count)
    }

    fn coefficients(&mut self, lane: usize, samples: &[f32], at: u64, _: usize, into: &mut [f32]) {
        for nth in 0..PART {
            let from = nth * SUBBANDS;
            let vector = self.polyphase.vector(
                lane,
                &samples[from..from + POLYPHASE_TAPS],
                at + from as u64,
            );
            for (subband, value) in vector.iter().enumerate() {
                into[subband * PART + nth] = *value;
            }
        }
    }

    fn frame(
        &mut self,
        taking: Taking,
        samples: &[f32],
        band: &Band,
        into: &mut [f32],
    ) -> Range<usize> {
        self.coefficients(taking.lane, samples, taking.at, taking.shape, into);
        band.first * PART..band.past * PART
    }

    fn forget(&mut self) {
        self.polyphase.forget();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        granules::tests::Synthesis,
        steps::{
            Stepping,
            tests::{RATE, Wandering, stepped},
        },
    };

    const SECONDS: usize = 20;
    const CODED_SUBBANDS: usize = 27;
    const LEVELS_DRAWN: usize = 8;
    const FACTORS_DRAWN: Range<usize> = 6..24;
    const PARTS_A_FRAME: usize = 3;

    fn synthesised(seed: u64, on_the_steps: bool) -> Vec<f32> {
        let mut wandering = Wandering(seed);
        let frames = SECONDS * RATE as usize / (HOP * PARTS_A_FRAME);
        let mut synthesis = Synthesis::default();
        let mut out = Vec::with_capacity(frames * HOP * PARTS_A_FRAME);
        let mut subbands = vec![[0.0_f64; PART * PARTS_A_FRAME]; SUBBANDS];
        for _ in 0..frames {
            for samples in subbands.iter_mut().take(CODED_SUBBANDS) {
                let levels = LEVELS[(wandering.next() * LEVELS_DRAWN as f64) as usize];
                let top = f64::from(levels / 2);
                for part in 0..PARTS_A_FRAME {
                    let factor = FACTORS_DRAWN.start
                        + (wandering.next() * FACTORS_DRAWN.len() as f64) as usize;
                    let scale = f64::from(LOUDEST_SCALE_FACTOR * SIXTEEN_BIT_STEPS)
                        * (-(factor as f64) / f64::from(SCALE_FACTOR_STEPS_AN_OCTAVE)).exp2();
                    let step = 2.0 * scale / f64::from(levels);
                    for nth in 0..PART {
                        let drawn = wandering.laplacian(top / 2.0);
                        let level = if on_the_steps {
                            drawn.clamp(-top, top).round()
                        } else {
                            drawn
                        };
                        samples[part * PART + nth] = level * step;
                    }
                }
            }
            let vectors: Vec<[f64; SUBBANDS]> = (0..PART * PARTS_A_FRAME)
                .map(|at| std::array::from_fn(|subband| subbands[subband][at]))
                .collect();
            for vector in &vectors {
                synthesis.sounded(vector, &mut out);
            }
        }
        out.iter()
            .map(|sample| sample.round() as f32 / SIXTEEN_BIT_STEPS)
            .collect()
    }

    #[test]
    fn subband_samples_on_a_scale_factors_levels_are_found_at_any_offset_and_free_ones_are_not() {
        let left = synthesised(11, true);
        let right = synthesised(23, true);
        for skipped in [0, 300] {
            let steps = stepped::<Mp2>(&left, &right, skipped).expect("weighed");

            assert!(steps.lattice > 0.5, "{skipped} {steps:?}");
            assert!(steps.control < steps.lattice / 10.0, "{steps:?}");
            assert!(steps.coefficients > 10_000, "{steps:?}");
            assert_eq!(steps.coding, Coding::Mp2);
        }

        let left = synthesised(11, false);
        let right = synthesised(23, false);
        let steps = stepped::<Mp2>(&left, &right, 0).expect("weighed");

        assert!(steps.lattice < 0.01, "{steps:?}");
    }

    #[test]
    fn a_rate_no_mp2_stream_carries_is_not_weighed() {
        let stepping = Stepping::<Mp2>::new(96_000, 2);

        assert!(stepping.finished().is_none());
    }
}
