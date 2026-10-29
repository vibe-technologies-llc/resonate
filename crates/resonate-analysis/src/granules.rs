use std::{f64::consts::PI, ops::Range};

use ahash::AHashMap;

use crate::steps::{Band, Coding, Grid, Phases, QUARTER_OCTAVE_STEPS, Ramp, Taking, Tally, phased};

pub(crate) const SUBBANDS: usize = 32;
pub(crate) const POLYPHASE_TAPS: usize = 512;
const POLYPHASE_ROWS: usize = 2 * SUBBANDS;
const LINES_A_SUBBAND: usize = 18;
const GRANULE: usize = SUBBANDS * LINES_A_SUBBAND;
const VECTORS_A_BLOCK: usize = 2 * LINES_A_SUBBAND;
const SPAN: usize = (VECTORS_A_BLOCK - 1) * SUBBANDS + POLYPHASE_TAPS;
const BUTTERFLIES: usize = 8;
const SHORT_BLOCK: usize = 12;
const SHORT_LINES: usize = 6;
const SHORT_WINDOWS: usize = 3;
const SHORT_FIRST: usize = 6;
const SHORT_GAIN: f32 = 3.0;
const SHORT_KIND: usize = 3;
const MP3_RAMP: Ramp = Ramp {
    unit: 0.535,
    from: 2.0,
    to: 6.0,
};
const WINDOW_UNITS_A_ONE: f64 = 65_536.0;

const MP3_FROM_HZ: f32 = 6_000.0;
const MP3_TO_HZ: f32 = 20_000.0;
const HIGHEST_MP3_RATE: u32 = 48_000;
const VECTORS_KEPT_AT_MOST: usize = 1 << 16;

const ALIAS_COEFFICIENTS: [f64; BUTTERFLIES] = [
    -0.6, -0.535, -0.33, -0.185, -0.095, -0.041, -0.0142, -0.0037,
];

const SYNTHESIS_WINDOW: [i32; POLYPHASE_TAPS] = [
    0, -1, -1, -1, -1, -1, -1, -2, -2, -2, -2, -3, -3, -4, -4, -5, -5, -6, -7, -7, -8, -9, -10,
    -11, -13, -14, -16, -17, -19, -21, -24, -26, -29, -31, -35, -38, -41, -45, -49, -53, -58, -63,
    -68, -73, -79, -85, -91, -97, -104, -111, -117, -125, -132, -139, -147, -154, -161, -169, -176,
    -183, -190, -196, -202, -208, 213, 218, 222, 225, 227, 228, 228, 227, 224, 221, 215, 208, 200,
    189, 177, 163, 146, 127, 106, 83, 57, 29, -2, -36, -72, -111, -153, -197, -244, -294, -347,
    -401, -459, -519, -581, -645, -711, -779, -848, -919, -991, -1064, -1137, -1210, -1283, -1356,
    -1428, -1498, -1567, -1634, -1698, -1759, -1817, -1870, -1919, -1962, -2001, -2032, -2057,
    -2075, -2085, -2087, -2080, -2063, 2037, 2000, 1952, 1893, 1822, 1739, 1644, 1535, 1414, 1280,
    1131, 970, 794, 605, 402, 185, -45, -288, -545, -814, -1095, -1388, -1692, -2006, -2330, -2663,
    -3004, -3351, -3705, -4063, -4425, -4788, -5153, -5517, -5879, -6237, -6589, -6935, -7271,
    -7597, -7910, -8209, -8491, -8755, -8998, -9219, -9416, -9585, -9727, -9838, -9916, -9959,
    -9966, -9935, -9863, -9750, -9592, -9389, -9139, -8840, -8492, -8092, -7640, -7134, 6574, 5959,
    5288, 4561, 3776, 2935, 2037, 1082, 70, -998, -2122, -3300, -4533, -5818, -7154, -8540, -9975,
    -11455, -12980, -14548, -16155, -17799, -19478, -21189, -22929, -24694, -26482, -28289, -30112,
    -31947, -33791, -35640, -37489, -39336, -41176, -43006, -44821, -46617, -48390, -50137, -51853,
    -53534, -55178, -56778, -58333, -59838, -61289, -62684, -64019, -65290, -66494, -67629, -68692,
    -69679, -70590, -71420, -72169, -72835, -73415, -73908, -74313, -74630, -74856, -74992, 75038,
    74992, 74856, 74630, 74313, 73908, 73415, 72835, 72169, 71420, 70590, 69679, 68692, 67629,
    66494, 65290, 64019, 62684, 61289, 59838, 58333, 56778, 55178, 53534, 51853, 50137, 48390,
    46617, 44821, 43006, 41176, 39336, 37489, 35640, 33791, 31947, 30112, 28289, 26482, 24694,
    22929, 21189, 19478, 17799, 16155, 14548, 12980, 11455, 9975, 8540, 7154, 5818, 4533, 3300,
    2122, 998, -70, -1082, -2037, -2935, -3776, -4561, -5288, -5959, 6574, 7134, 7640, 8092, 8492,
    8840, 9139, 9389, 9592, 9750, 9863, 9935, 9966, 9959, 9916, 9838, 9727, 9585, 9416, 9219, 8998,
    8755, 8491, 8209, 7910, 7597, 7271, 6935, 6589, 6237, 5879, 5517, 5153, 4788, 4425, 4063, 3705,
    3351, 3004, 2663, 2330, 2006, 1692, 1388, 1095, 814, 545, 288, 45, -185, -402, -605, -794,
    -970, -1131, -1280, -1414, -1535, -1644, -1739, -1822, -1893, -1952, -2000, 2037, 2063, 2080,
    2087, 2085, 2075, 2057, 2032, 2001, 1962, 1919, 1870, 1817, 1759, 1698, 1634, 1567, 1498, 1428,
    1356, 1283, 1210, 1137, 1064, 991, 919, 848, 779, 711, 645, 581, 519, 459, 401, 347, 294, 244,
    197, 153, 111, 72, 36, 2, -29, -57, -83, -106, -127, -146, -163, -177, -189, -200, -208, -215,
    -221, -224, -227, -228, -228, -227, -225, -222, -218, 213, 208, 202, 196, 190, 183, 176, 169,
    161, 154, 147, 139, 132, 125, 117, 111, 104, 97, 91, 85, 79, 73, 68, 63, 58, 53, 49, 45, 41,
    38, 35, 31, 29, 26, 24, 21, 19, 17, 16, 14, 13, 11, 10, 9, 8, 7, 7, 6, 5, 5, 4, 4, 3, 3, 2, 2,
    2, 2, 1, 1, 1, 1, 1, 1,
];

pub(crate) type Vector = [f32; SUBBANDS];

pub(crate) struct Polyphase {
    analysis_window: Vec<f32>,
    matrixing: Vec<f32>,
    vectors: AHashMap<(usize, u64), Vector>,
    subbands: Range<usize>,
}

impl Polyphase {
    pub(crate) fn new(subbands: Range<usize>) -> Self {
        Self {
            analysis_window: analysis_window(),
            matrixing: matrixing(),
            vectors: AHashMap::new(),
            subbands,
        }
    }

    pub(crate) fn vector(&mut self, lane: usize, samples: &[f32], at: u64) -> Vector {
        if let Some(vector) = self.vectors.get(&(lane, at)) {
            return *vector;
        }
        let mut rows = [0.0_f32; POLYPHASE_ROWS];
        for (tap, (sample, weight)) in samples.iter().rev().zip(&self.analysis_window).enumerate() {
            rows[tap % POLYPHASE_ROWS] += sample * weight;
        }
        let mut vector = [0.0_f32; SUBBANDS];
        let folded = folded_rows(&rows);
        for subband in self.subbands.clone() {
            let row = &self.matrixing[subband * SUBBANDS..(subband + 1) * SUBBANDS];
            vector[subband] = row
                .iter()
                .zip(&folded)
                .map(|(cosine, value)| cosine * value)
                .sum();
        }
        if self.vectors.len() >= VECTORS_KEPT_AT_MOST {
            self.vectors.clear();
        }
        self.vectors.insert((lane, at), vector);
        vector
    }

    pub(crate) fn forget(&mut self) {
        self.vectors.clear();
    }
}

pub(crate) struct Mp3 {
    polyphase: Polyphase,
    block_windows: [[f32; VECTORS_A_BLOCK]; 3],
    short_window: [f32; SHORT_BLOCK],
    short_turning: Vec<f32>,
    turning: Vec<f32>,
    butterflies: [(f32, f32); BUTTERFLIES],
    block: Vec<Vector>,
    folded: [f32; LINES_A_SUBBAND],
    lines: [[f32; LINES_A_SUBBAND]; SUBBANDS],
    subbands: Range<usize>,
    phases: Phases,
}

impl Mp3 {
    fn transformed_short(&mut self, subband: usize) {
        for window in 0..SHORT_WINDOWS {
            let mut windowed = [0.0_f32; SHORT_BLOCK];
            for (nth, slot) in windowed.iter_mut().enumerate() {
                let at = SHORT_FIRST + SHORT_LINES * window + nth;
                let inverted = subband % 2 == 1 && at % 2 == 1;
                let value = self.block[at][subband] * self.short_window[nth];
                *slot = if inverted { -value } else { value };
            }
            for line in 0..SHORT_LINES {
                let row = &self.short_turning[line * SHORT_BLOCK..(line + 1) * SHORT_BLOCK];
                self.lines[subband][window * SHORT_LINES + line] = SHORT_GAIN
                    * row
                        .iter()
                        .zip(&windowed)
                        .map(|(cosine, value)| cosine * value)
                        .sum::<f32>();
            }
        }
    }

    fn transformed(&mut self, subband: usize, kind: usize) {
        let mut windowed = [0.0_f32; VECTORS_A_BLOCK];
        for (nth, slot) in windowed.iter_mut().enumerate() {
            let inverted = subband % 2 == 1 && nth % 2 == 1;
            let value = self.block[nth][subband] * self.block_windows[kind][nth];
            *slot = if inverted { -value } else { value };
        }
        let quarter = LINES_A_SUBBAND / 2;
        let (front, back) = windowed.split_at(LINES_A_SUBBAND);
        let (first, second) = front.split_at(quarter);
        let (third, fourth) = back.split_at(quarter);
        let (low, high) = self.folded.split_at_mut(quarter);
        for ((slot, third), fourth) in low.iter_mut().zip(third.iter().rev()).zip(fourth) {
            *slot = -third - fourth;
        }
        for ((slot, first), second) in high.iter_mut().zip(first).zip(second.iter().rev()) {
            *slot = first - second;
        }
        for (line, slot) in self.lines[subband].iter_mut().enumerate() {
            let row = &self.turning[line * LINES_A_SUBBAND..(line + 1) * LINES_A_SUBBAND];
            *slot = row
                .iter()
                .zip(&self.folded)
                .map(|(cosine, value)| cosine * value)
                .sum();
        }
    }

    fn transform(&mut self, lane: usize, samples: &[f32], at: u64, kind: usize, into: &mut [f32]) {
        for nth in 0..VECTORS_A_BLOCK {
            let from = nth * SUBBANDS;
            self.block[nth] = self.polyphase.vector(
                lane,
                &samples[from..from + POLYPHASE_TAPS],
                at + from as u64,
            );
        }
        for subband in self.subbands.clone() {
            if kind == SHORT_KIND {
                self.transformed_short(subband);
            } else {
                self.transformed(subband, kind);
            }
        }
        if kind != SHORT_KIND {
            self.unaliased();
        }
        for (subband, lines) in self.lines.iter().enumerate() {
            into[subband * LINES_A_SUBBAND..(subband + 1) * LINES_A_SUBBAND].copy_from_slice(lines);
        }
    }

    fn unaliased(&mut self) {
        for subband in self.subbands.start + 1..self.subbands.end {
            for (nth, (sine, cosine)) in self.butterflies.iter().enumerate() {
                let below = self.lines[subband - 1][LINES_A_SUBBAND - 1 - nth];
                let above = self.lines[subband][nth];
                self.lines[subband - 1][LINES_A_SUBBAND - 1 - nth] = below * cosine + above * sine;
                self.lines[subband][nth] = above * cosine - below * sine;
            }
        }
    }
}

fn long_slope(nth: usize) -> f64 {
    (PI / VECTORS_A_BLOCK as f64 * (nth as f64 + 0.5)).sin()
}

fn short_slope(nth: usize) -> f64 {
    (PI / SHORT_BLOCK as f64 * (nth as f64 + 0.5)).sin()
}

fn block_windows() -> [[f32; VECTORS_A_BLOCK]; 3] {
    let normal = std::array::from_fn(|nth| long_slope(nth) as f32);
    let start = std::array::from_fn(|nth| match nth {
        0..18 => long_slope(nth) as f32,
        18..24 => 1.0,
        24..30 => short_slope(nth - 18) as f32,
        _ => 0.0,
    });
    let stop = std::array::from_fn(|nth| match nth {
        0..6 => 0.0,
        6..12 => short_slope(nth - 6) as f32,
        12..18 => 1.0,
        _ => long_slope(nth) as f32,
    });
    [normal, start, stop]
}

fn analysis_window() -> Vec<f32> {
    SYNTHESIS_WINDOW
        .iter()
        .map(|units| (f64::from(*units) / WINDOW_UNITS_A_ONE / SUBBANDS as f64) as f32)
        .collect()
}

const ROWS_BEFORE_THE_CENTRE: usize = 16;

fn folded_rows(rows: &[f32; POLYPHASE_ROWS]) -> [f32; SUBBANDS] {
    let mut folded = [0.0_f32; SUBBANDS];
    for (row, value) in rows.iter().enumerate() {
        if row < ROWS_BEFORE_THE_CENTRE {
            folded[ROWS_BEFORE_THE_CENTRE - row] += value;
        } else if row < ROWS_BEFORE_THE_CENTRE + SUBBANDS {
            folded[row - ROWS_BEFORE_THE_CENTRE] += value;
        } else if row > ROWS_BEFORE_THE_CENTRE + SUBBANDS {
            folded[POLYPHASE_ROWS + ROWS_BEFORE_THE_CENTRE - row] -= value;
        }
    }
    folded
}

fn matrix_angle(subband: usize, row: f64) -> f64 {
    (2.0 * subband as f64 + 1.0) * row * PI / POLYPHASE_ROWS as f64
}

fn matrixing() -> Vec<f32> {
    (0..SUBBANDS * SUBBANDS)
        .map(|at| matrix_angle(at / SUBBANDS, (at % SUBBANDS) as f64).cos() as f32)
        .collect()
}

fn turning() -> Vec<f32> {
    let lines = LINES_A_SUBBAND as f64;
    (0..LINES_A_SUBBAND * LINES_A_SUBBAND)
        .map(|at| {
            let line = (at / LINES_A_SUBBAND) as f64;
            let nth = (at % LINES_A_SUBBAND) as f64;
            (PI / lines * (nth + 0.5) * (line + 0.5)).cos() as f32
        })
        .collect()
}

fn butterflies() -> [(f32, f32); BUTTERFLIES] {
    ALIAS_COEFFICIENTS.map(|coefficient| {
        let length = (1.0 + coefficient * coefficient).sqrt();
        ((coefficient / length) as f32, (1.0 / length) as f32)
    })
}

impl Grid for Mp3 {
    const CODING: Coding = Coding::Mp3;
    const HOP: usize = GRANULE;
    const RESIDUES: usize = GRANULE;
    const SPAN: usize = SPAN;
    const LINES: usize = GRANULE;
    const SHAPES: usize = 1;
    const KINDS: usize = 4;
    const CONTROL_SHIFT: usize = GRANULE / 2;
    const FRAMES_A_SEARCH: usize = 4;
    const CONFIRMED_FRAMES: usize = 400;
    const MID_AND_SIDE: bool = true;

    fn band(rate: u32) -> Option<Band> {
        (rate <= HIGHEST_MP3_RATE).then(|| Band::of(rate, GRANULE, MP3_FROM_HZ, MP3_TO_HZ))
    }

    fn new(_: u32, band: &Band) -> Self {
        let lowest = (band.first / LINES_A_SUBBAND).saturating_sub(1);
        let highest = (band.past.saturating_sub(1) / LINES_A_SUBBAND + 1).min(SUBBANDS - 1);
        let block_windows = block_windows();
        let short_window = std::array::from_fn(|nth| short_slope(nth) as f32);
        let short_turning = (0..SHORT_LINES * SHORT_BLOCK)
            .map(|at| {
                let line = (at / SHORT_BLOCK) as f64;
                let nth = (at % SHORT_BLOCK) as f64;
                (PI / (2 * SHORT_BLOCK) as f64
                    * (2.0 * nth + 1.0 + SHORT_LINES as f64)
                    * (2.0 * line + 1.0))
                    .cos() as f32
            })
            .collect();
        Self {
            polyphase: Polyphase::new(lowest..highest + 1),
            block_windows,
            short_window,
            short_turning,
            turning: turning(),
            butterflies: butterflies(),
            block: vec![[0.0; SUBBANDS]; VECTORS_A_BLOCK],
            folded: [0.0; LINES_A_SUBBAND],
            lines: [[0.0; LINES_A_SUBBAND]; SUBBANDS],
            subbands: lowest..highest + 1,
            phases: Phases::new(QUARTER_OCTAVE_STEPS, MP3_RAMP),
        }
    }

    fn tally(&self, lines: Range<usize>, left: &[f32], right: Option<&[f32]>) -> Tally {
        phased(&self.phases, lines, left, right, Self::MID_AND_SIDE)
    }

    fn coefficients(&mut self, lane: usize, samples: &[f32], at: u64, _: usize, into: &mut [f32]) {
        self.transform(lane, samples, at, 0, into);
    }

    fn frame(
        &mut self,
        taking: Taking,
        samples: &[f32],
        band: &Band,
        into: &mut [f32],
    ) -> Range<usize> {
        self.transform(taking.lane, samples, taking.at, taking.kind, into);
        if taking.kind == SHORT_KIND {
            self.subbands.start * LINES_A_SUBBAND..(self.subbands.end) * LINES_A_SUBBAND
        } else {
            band.first..band.past
        }
    }

    fn forget(&mut self) {
        self.polyphase.forget();
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::steps::{
        SIXTEEN_BIT_STEPS, Stepping,
        tests::{RATE, Wandering, quantised, stepped, unquantised},
    };

    const SECONDS: usize = 14;
    const SYNTHESIS_HISTORY: usize = 1_024;

    const BLOCK_SEQUENCE: [usize; 6] = [0, 0, 1, SHORT_KIND, SHORT_KIND, 2];

    pub(crate) struct Synthesis {
        history: Vec<f64>,
    }

    impl Default for Synthesis {
        fn default() -> Self {
            Self {
                history: vec![0.0; SYNTHESIS_HISTORY],
            }
        }
    }

    impl Synthesis {
        pub(crate) fn sounded(&mut self, samples: &[f64; SUBBANDS], out: &mut Vec<f64>) {
            self.history.rotate_right(POLYPHASE_ROWS);
            for (row, slot) in self.history[..POLYPHASE_ROWS].iter_mut().enumerate() {
                *slot = samples
                    .iter()
                    .enumerate()
                    .map(|(subband, value)| {
                        let angle = (ROWS_BEFORE_THE_CENTRE as f64 + row as f64)
                            * (2.0 * subband as f64 + 1.0)
                            * PI
                            / POLYPHASE_ROWS as f64;
                        value * angle.cos()
                    })
                    .sum();
            }
            for nth in 0..SUBBANDS {
                let sum: f64 = (0..POLYPHASE_TAPS / SUBBANDS)
                    .map(|step| {
                        let tap = nth + SUBBANDS * step;
                        let slot = tap / POLYPHASE_ROWS * 2 * POLYPHASE_ROWS
                            + if tap % POLYPHASE_ROWS < SUBBANDS {
                                tap % POLYPHASE_ROWS
                            } else {
                                tap % POLYPHASE_ROWS + POLYPHASE_ROWS
                            };
                        self.history[slot] * f64::from(SYNTHESIS_WINDOW[tap]) / WINDOW_UNITS_A_ONE
                    })
                    .sum();
                out.push(sum);
            }
        }
    }

    fn synthesised(coefficients: impl FnMut(usize) -> f64) -> Vec<f32> {
        synthesised_blocks(coefficients, |_| 0)
    }

    fn synthesised_blocks(
        mut coefficients: impl FnMut(usize) -> f64,
        kind_of: impl Fn(usize) -> usize,
    ) -> Vec<f32> {
        let granules = SECONDS * RATE as usize / GRANULE;
        let windows = block_windows();
        let butterflies = butterflies();
        let mut overlap = vec![[0.0_f64; LINES_A_SUBBAND]; SUBBANDS];
        let mut synthesis = Synthesis::default();
        let mut out = Vec::with_capacity(granules * GRANULE);

        for granule in 0..granules {
            let kind = kind_of(granule);
            let mut lines: Vec<[f64; LINES_A_SUBBAND]> = (0..SUBBANDS)
                .map(|subband| {
                    std::array::from_fn(|line| coefficients(subband * LINES_A_SUBBAND + line))
                })
                .collect();
            for subband in 1..SUBBANDS {
                if kind == SHORT_KIND {
                    break;
                }
                for (nth, (sine, cosine)) in butterflies.iter().enumerate() {
                    let (sine, cosine) = (f64::from(*sine), f64::from(*cosine));
                    let below = lines[subband - 1][LINES_A_SUBBAND - 1 - nth];
                    let above = lines[subband][nth];
                    lines[subband - 1][LINES_A_SUBBAND - 1 - nth] = below * cosine - above * sine;
                    lines[subband][nth] = above * cosine + below * sine;
                }
            }
            let mut subband_samples = vec![[0.0_f64; SUBBANDS]; LINES_A_SUBBAND];
            for (subband, held) in lines.iter().enumerate() {
                let block: Vec<f64> = if kind == SHORT_KIND {
                    let mut block = vec![0.0_f64; VECTORS_A_BLOCK];
                    for window in 0..SHORT_WINDOWS {
                        for nth in 0..SHORT_BLOCK {
                            let sum: f64 = (0..SHORT_LINES)
                                .map(|line| {
                                    let angle = PI / (2.0 * SHORT_BLOCK as f64)
                                        * (2.0 * nth as f64 + 1.0 + SHORT_LINES as f64)
                                        * (2.0 * line as f64 + 1.0);
                                    held[window * SHORT_LINES + line] * angle.cos()
                                })
                                .sum();
                            block[SHORT_FIRST + SHORT_LINES * window + nth] +=
                                sum * short_slope(nth) * 2.0 / LINES_A_SUBBAND as f64;
                        }
                    }
                    block
                } else {
                    (0..VECTORS_A_BLOCK)
                        .map(|nth| {
                            let sum: f64 = held
                                .iter()
                                .enumerate()
                                .map(|(line, value)| {
                                    let angle = PI / (2.0 * VECTORS_A_BLOCK as f64)
                                        * (2.0 * nth as f64 + 1.0 + LINES_A_SUBBAND as f64)
                                        * (2.0 * line as f64 + 1.0);
                                    value * angle.cos()
                                })
                                .sum();
                            sum * f64::from(windows[kind][nth]) * 2.0 / LINES_A_SUBBAND as f64
                        })
                        .collect()
                };
                for nth in 0..LINES_A_SUBBAND {
                    let value = block[nth] + overlap[subband][nth];
                    let inverted = subband % 2 == 1 && nth % 2 == 1;
                    subband_samples[nth][subband] = if inverted { -value } else { value };
                    overlap[subband][nth] = block[nth + LINES_A_SUBBAND];
                }
            }
            for samples in &subband_samples {
                synthesis.sounded(samples, &mut out);
            }
        }
        out.iter()
            .map(|sample| sample.round() as f32 / SIXTEEN_BIT_STEPS)
            .collect()
    }

    #[test]
    fn lines_on_the_quantiser_steps_are_found_through_the_hybrid_filterbank_at_any_offset() {
        let left = synthesised(quantised(11));
        let right = synthesised(quantised(23));
        for skipped in [0, 300] {
            let steps = stepped::<Mp3>(&left, &right, skipped).expect("weighed");

            assert!(steps.lattice > 0.15, "{skipped} {steps:?}");
            assert!(steps.control < steps.lattice / 10.0, "{steps:?}");
            assert!(steps.coefficients > 100_000, "{steps:?}");
            assert_eq!(steps.coding, Coding::Mp3);
        }

        let left = synthesised(unquantised(11));
        let right = synthesised(unquantised(23));
        let steps = stepped::<Mp3>(&left, &right, 0).expect("weighed");

        assert!(steps.lattice < 0.01, "{steps:?}");
    }

    #[test]
    fn lines_on_the_steps_are_found_whatever_block_types_the_granules_run_through() {
        let kind_of = |granule: usize| BLOCK_SEQUENCE[granule % BLOCK_SEQUENCE.len()];
        let left = synthesised_blocks(quantised(11), kind_of);
        let right = synthesised_blocks(quantised(23), kind_of);
        for skipped in [0, 300] {
            let steps = stepped::<Mp3>(&left, &right, skipped).expect("weighed");

            assert!(steps.lattice > 0.15, "{skipped} {steps:?}");
            assert!(steps.control < steps.lattice / 10.0, "{steps:?}");
            assert_eq!(steps.coding, Coding::Mp3);
        }

        let left = synthesised_blocks(unquantised(11), kind_of);
        let right = synthesised_blocks(unquantised(23), kind_of);
        let steps = stepped::<Mp3>(&left, &right, 0).expect("weighed");

        assert!(steps.lattice < 0.01, "{steps:?}");
    }

    #[test]
    fn a_rate_no_mp3_stream_carries_is_not_weighed() {
        let stepping = Stepping::<Mp3>::new(96_000, 2);

        assert!(stepping.finished().is_none());
    }

    #[test]
    fn the_folded_matrixing_is_the_standards_matrix() {
        let mut wandering = Wandering(5);
        let rows: [f32; POLYPHASE_ROWS] = std::array::from_fn(|_| wandering.next() as f32 - 0.5);
        let matrixing = matrixing();
        let folded = folded_rows(&rows);

        for subband in 0..SUBBANDS {
            let fast: f32 = matrixing[subband * SUBBANDS..(subband + 1) * SUBBANDS]
                .iter()
                .zip(&folded)
                .map(|(cosine, value)| cosine * value)
                .sum();
            let slow: f64 = rows
                .iter()
                .enumerate()
                .map(|(row, value)| {
                    let centred = row as f64 - ROWS_BEFORE_THE_CENTRE as f64;
                    f64::from(*value) * matrix_angle(subband, centred).cos()
                })
                .sum();
            assert!(
                (f64::from(fast) - slow).abs() < 1e-4,
                "{subband} {fast} {slow}"
            );
        }
    }

    #[test]
    fn the_folded_block_transform_is_the_standards_mdct() {
        let mut mp3 = Mp3::new(44_100, &Band::of(44_100, GRANULE, 0.0, 22_050.0));
        let mut wandering = Wandering(9);
        for vector in &mut mp3.block {
            *vector = std::array::from_fn(|_| wandering.next() as f32 - 0.5);
        }
        let subband = 4;

        mp3.transformed(subband, 0);
        for line in 0..LINES_A_SUBBAND {
            let slow: f64 = (0..VECTORS_A_BLOCK)
                .map(|nth| {
                    let angle = PI / (2.0 * VECTORS_A_BLOCK as f64)
                        * (2.0 * nth as f64 + 1.0 + LINES_A_SUBBAND as f64)
                        * (2.0 * line as f64 + 1.0);
                    f64::from(mp3.block[nth][subband])
                        * f64::from(mp3.block_windows[0][nth])
                        * angle.cos()
                })
                .sum();
            let fast = f64::from(mp3.lines[subband][line]);
            assert!((fast - slow).abs() < 1e-4, "{line} {fast} {slow}");
        }
    }
}
