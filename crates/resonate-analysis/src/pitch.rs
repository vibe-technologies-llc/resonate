pub(crate) const LONGEST_PERIOD: usize = 1_024;
pub(crate) const SHORTEST_PERIOD: usize = 15;
pub(crate) const LONGEST_CODED_PERIOD: usize = LONGEST_PERIOD - 2;

const LPC_ORDER: usize = 4;
const NOISE_FLOOR: f32 = 1.0001;
const LAG_WINDOW: f32 = 0.008;
const LPC_STOPS_UNDER: f32 = 1e-3;
const LPC_BANDWIDTH: f32 = 0.9;
const ADDED_ZERO: f32 = 0.8;
const INTERPOLATED_OVER: f32 = 0.7;
const SEARCHED_SHORT_OF: usize = 3 * SHORTEST_PERIOD;
const NEIGHBOURS_KEPT: usize = 2;
const SUBMULTIPLES_TRIED: usize = 15;
const SECOND_CHECK: [usize; SUBMULTIPLES_TRIED + 1] =
    [0, 0, 3, 2, 3, 2, 5, 2, 3, 2, 3, 2, 5, 2, 3, 2];
const KEPT_OVER: f32 = 0.7;
const KEPT_AT_LEAST: f32 = 0.3;
const SHORT_KEPT_OVER: f32 = 0.85;
const SHORT_KEPT_AT_LEAST: f32 = 0.4;
const ENCODER_GAIN_SHARE: f32 = 0.7;

pub(crate) struct Pitch {
    frame: usize,
    period: usize,
    gain: f32,
    buffer: Vec<f32>,
    lookup: Vec<f32>,
    coarse: Vec<f32>,
    fine: Vec<f32>,
}

impl Pitch {
    pub(crate) fn new(frame: usize) -> Self {
        Self {
            frame,
            period: SHORTEST_PERIOD,
            gain: 0.0,
            buffer: Vec::with_capacity((LONGEST_PERIOD + frame) / 2),
            lookup: vec![0.0; LONGEST_PERIOD / 2 + 1],
            coarse: Vec::with_capacity(LONGEST_PERIOD / 4),
            fine: Vec::with_capacity(LONGEST_PERIOD / 2),
        }
    }

    pub(crate) fn forget(&mut self) {
        self.period = SHORTEST_PERIOD;
        self.gain = 0.0;
    }

    pub(crate) fn heard(&mut self, gain: f32) {
        self.gain = gain;
    }

    pub(crate) fn period(&mut self, lanes: &[&[f32]]) -> (usize, f32) {
        self.downsample(lanes);
        let found = self.search();
        let (period, gain) = self.undoubled(LONGEST_PERIOD - found);
        self.period = period.min(LONGEST_CODED_PERIOD);
        (self.period, ENCODER_GAIN_SHARE * gain)
    }

    fn downsample(&mut self, lanes: &[&[f32]]) {
        let half = lanes[0].len() / 2;
        self.buffer.clear();
        self.buffer.resize(half, 0.0);
        for lane in lanes {
            self.buffer[0] += 0.5 * (0.5 * lane[1] + lane[0]);
            for (nth, slot) in self.buffer.iter_mut().enumerate().skip(1) {
                *slot += 0.5 * (0.5 * (lane[2 * nth - 1] + lane[2 * nth + 1]) + lane[2 * nth]);
            }
        }
        let whitening = whitening(&self.buffer);
        let mut held = [0.0_f32; LPC_ORDER + 1];
        for value in &mut self.buffer {
            let sample = *value;
            *value = sample
                + whitening
                    .iter()
                    .zip(&held)
                    .map(|(tap, before)| tap * before)
                    .sum::<f32>();
            held.rotate_right(1);
            held[0] = sample;
        }
    }

    fn search(&mut self) -> usize {
        let length = self.frame;
        let reach = LONGEST_PERIOD - SEARCHED_SHORT_OF;
        let x = &self.buffer[LONGEST_PERIOD / 2..];
        let y = &self.buffer;
        self.coarse.clear();
        self.coarse.extend((0..reach / 4).map(|lag| {
            (0..length / 4)
                .map(|nth| x[2 * nth] * y[2 * (lag + nth)])
                .sum::<f32>()
        }));
        let every_other: Vec<f32> = y.iter().step_by(2).copied().collect();
        let best = best_of(&self.coarse, &every_other, length / 4);
        self.fine.clear();
        self.fine.extend((0..reach / 2).map(|lag| {
            if lag.abs_diff(2 * best[0]) > NEIGHBOURS_KEPT
                && lag.abs_diff(2 * best[1]) > NEIGHBOURS_KEPT
            {
                0.0
            } else {
                inner(x, &y[lag..], length / 2).max(-1.0)
            }
        }));
        let best = best_of(&self.fine, y, length / 2)[0];
        let offset = if best > 0 && best < reach / 2 - 1 {
            leaning(self.fine[best - 1], self.fine[best], self.fine[best + 1])
        } else {
            0
        };
        (2 * best as isize - offset) as usize
    }

    fn undoubled(&mut self, period: usize) -> (usize, f32) {
        let longest = LONGEST_PERIOD / 2;
        let shortest = SHORTEST_PERIOD / 2;
        let length = self.frame / 2;
        let buffer = &self.buffer;
        let back = |lag: usize| &buffer[longest - lag..longest - lag + length];
        let x = back(0);
        let first = (period / 2).clamp(1, longest - 1);
        let before = self.period / 2;

        let xx = inner(x, x, length);
        let mut yy = xx;
        self.lookup[0] = xx;
        for lag in 1..=longest {
            let entering = buffer[longest - lag];
            let leaving = buffer[longest + length - lag];
            yy += entering * entering - leaving * leaving;
            self.lookup[lag] = yy.max(0.0);
        }
        let first_correlation = inner(x, back(first), length);
        let first_gain = gain_of(first_correlation, xx, self.lookup[first]);
        let mut chosen = first;
        let mut chosen_gain = first_gain;
        let mut chosen_correlation = first_correlation;
        let mut chosen_energy = self.lookup[first];
        for (k, second) in SECOND_CHECK.iter().enumerate().skip(2) {
            let lag = (2 * first + k) / (2 * k);
            if lag < shortest {
                break;
            }
            let other = match k {
                2 if lag + first > longest => first,
                2 => first + lag,
                _ => (2 * second * first + k) / (2 * k),
            };
            let xy = 0.5 * (inner(x, back(lag), length) + inner(x, back(other), length));
            let yy = 0.5 * (self.lookup[lag] + self.lookup[other]);
            let gain = gain_of(xy, xx, yy);
            let continued = if lag.abs_diff(before) <= 1 {
                self.gain
            } else if lag.abs_diff(before) <= 2 && 5 * k * k < first {
                0.5 * self.gain
            } else {
                0.0
            };
            let threshold = if lag < 3 * shortest {
                (SHORT_KEPT_OVER * first_gain - continued).max(SHORT_KEPT_AT_LEAST)
            } else {
                (KEPT_OVER * first_gain - continued).max(KEPT_AT_LEAST)
            };
            if gain > threshold {
                chosen = lag;
                chosen_gain = gain;
                chosen_correlation = xy;
                chosen_energy = yy;
            }
        }
        let correlation = chosen_correlation.max(0.0);
        let held = if chosen_energy <= correlation {
            1.0
        } else {
            correlation / (chosen_energy + 1.0)
        };
        let around: [f32; 3] = [0, 1, 2].map(|step| inner(x, back(chosen + step - 1), length));
        let offset = leaning(around[0], around[1], around[2]);
        let period = ((2 * chosen) as isize + offset).max(SHORTEST_PERIOD as isize) as usize;
        (period, held.min(chosen_gain))
    }
}

fn whitening(signal: &[f32]) -> [f32; LPC_ORDER + 1] {
    let mut correlation = [0.0_f32; LPC_ORDER + 1];
    for (lag, slot) in correlation.iter_mut().enumerate() {
        *slot = inner(signal, &signal[lag..], signal.len() - lag);
    }
    correlation[0] *= NOISE_FLOOR;
    for (lag, slot) in correlation.iter_mut().enumerate().skip(1) {
        let windowed = LAG_WINDOW * lag as f32;
        *slot -= *slot * windowed * windowed;
    }
    let mut lpc = levinson(&correlation);
    let mut shrink = 1.0;
    for tap in &mut lpc {
        shrink *= LPC_BANDWIDTH;
        *tap *= shrink;
    }
    [
        lpc[0] + ADDED_ZERO,
        lpc[1] + ADDED_ZERO * lpc[0],
        lpc[2] + ADDED_ZERO * lpc[1],
        lpc[3] + ADDED_ZERO * lpc[2],
        ADDED_ZERO * lpc[3],
    ]
}

fn levinson(correlation: &[f32; LPC_ORDER + 1]) -> [f32; LPC_ORDER] {
    let mut lpc = [0.0_f32; LPC_ORDER];
    let mut error = correlation[0];
    if correlation[0] <= 1e-10 {
        return lpc;
    }
    for order in 0..LPC_ORDER {
        let reflected = correlation[order + 1]
            + (0..order)
                .map(|nth| lpc[nth] * correlation[order - nth])
                .sum::<f32>();
        let reflection = -reflected / error;
        lpc[order] = reflection;
        for nth in 0..order.div_ceil(2) {
            let (one, other) = (lpc[nth], lpc[order - 1 - nth]);
            lpc[nth] = one + reflection * other;
            lpc[order - 1 - nth] = other + reflection * one;
        }
        error -= reflection * reflection * error;
        if error < correlation[0] * LPC_STOPS_UNDER {
            break;
        }
    }
    lpc
}

fn inner(one: &[f32], other: &[f32], length: usize) -> f32 {
    one[..length]
        .iter()
        .zip(&other[..length])
        .map(|(a, b)| a * b)
        .sum()
}

fn gain_of(xy: f32, xx: f32, yy: f32) -> f32 {
    xy / (1.0 + xx * yy).sqrt()
}

fn leaning(before: f32, at: f32, after: f32) -> isize {
    if after - before > INTERPOLATED_OVER * (at - before) {
        1
    } else if before - after > INTERPOLATED_OVER * (at - after) {
        -1
    } else {
        0
    }
}

fn best_of(correlations: &[f32], signal: &[f32], length: usize) -> [usize; 2] {
    let mut energy = 1.0_f32 + inner(signal, signal, length);
    let mut numerators = [-1.0_f32; 2];
    let mut denominators = [0.0_f32; 2];
    let mut best = [0, 1];
    for (lag, correlation) in correlations.iter().enumerate() {
        if *correlation > 0.0 {
            let scaled = correlation * 1e-12;
            let numerator = scaled * scaled;
            if numerator * denominators[1] > numerators[1] * energy {
                if numerator * denominators[0] > numerators[0] * energy {
                    numerators[1] = numerators[0];
                    denominators[1] = denominators[0];
                    best[1] = best[0];
                    numerators[0] = numerator;
                    denominators[0] = energy;
                    best[0] = lag;
                } else {
                    numerators[1] = numerator;
                    denominators[1] = energy;
                    best[1] = lag;
                }
            }
        }
        energy += signal[lag + length] * signal[lag + length] - signal[lag] * signal[lag];
        energy = energy.max(1.0);
    }
    best
}
