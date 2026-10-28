use std::{
    f64::consts::{PI, TAU},
    num::NonZeroUsize,
};

use resonate_core::{
    SampleRate,
    eq::{Band, BandGain, BandKind, Biquad, Frequency, Q},
};
use resonate_dsp::TruePeakMeter;

use crate::{
    bands::{BANDS, edge_of},
    peaks::{BandPeaks, Heights, NODES, TREE, WHOLE, grouped},
};

const EDGES: usize = BANDS - 1;

const LEVELS: usize = BANDS + 1;

const SECTIONS_AN_EDGE: usize = 4;

const SECTIONS_OF_THE_ORDER: usize = 2 * SECTIONS_AN_EDGE;

const CHUNKS_A_SECOND: usize = 2;

const MARGINS_A_SECOND: usize = 5;

pub(crate) const TILES_A_SECOND: usize = 20;

const TILES_AT_MOST: usize = 1 << 14;

const PROBE_OFFSETS: usize = 64;

const PROBE_FRAMES: isize = 4;

const LEAST_SEEN_OF_A_PEAK: f64 = 0.25;

const LAST_MIDPOINT_FRAMES: usize = 2;

const WIDEST_EDGE_OF_A_BAND_READ: f64 = 0.9;

const OCTAVES_BEYOND_A_BAND_READ: f64 = 2.0 / 3.0;

type Plane = [f64; EDGES];

type Lanes = [f64; BANDS];

type Levels = [f64; LEVELS];

struct Coefficients {
    b0: [Plane; SECTIONS_AN_EDGE],
    b1: [Plane; SECTIONS_AN_EDGE],
    b2: [Plane; SECTIONS_AN_EDGE],
    a1: [Plane; SECTIONS_AN_EDGE],
    a2: [Plane; SECTIONS_AN_EDGE],
}

impl Coefficients {
    fn at(rate: SampleRate) -> Self {
        let mut planes = Self {
            b0: [[1.0; EDGES]; SECTIONS_AN_EDGE],
            b1: [[0.0; EDGES]; SECTIONS_AN_EDGE],
            b2: [[0.0; EDGES]; SECTIONS_AN_EDGE],
            a1: [[0.0; EDGES]; SECTIONS_AN_EDGE],
            a2: [[0.0; EDGES]; SECTIONS_AN_EDGE],
        };
        for edge in 0..EDGES {
            for (section, Biquad { b0, b1, b2, a1, a2 }) in sections_at(edge + 1, rate).enumerate()
            {
                planes.b0[section][edge] = b0;
                planes.b1[section][edge] = b1;
                planes.b2[section][edge] = b2;
                planes.a1[section][edge] = a1;
                planes.a2[section][edge] = a2;
            }
        }
        planes
    }
}

fn sections_at(edge: usize, rate: SampleRate) -> impl Iterator<Item = Biquad> {
    let frequency = Frequency::from_hertz(edge_of(edge)).unwrap_or(Frequency::HIGHEST);
    (0..SECTIONS_AN_EDGE).map(move |section| {
        let q = Q::from_units(butterworth_q(section)).unwrap_or(Q::BUTTERWORTH);
        Band::new(BandKind::LowPass, frequency, BandGain::FLAT, q).design(rate)
    })
}

pub(crate) fn passed_below(edge: usize, hertz: f64, rate: SampleRate) -> f64 {
    if edge == 0 {
        return 0.0;
    }
    if edge >= BANDS {
        return 1.0;
    }
    let decibels: f64 = sections_at(edge, rate)
        .map(|section| section.magnitude_db(hertz, rate))
        .sum();
    10_f64.powf(decibels / 10.0)
}

fn butterworth_q(section: usize) -> f64 {
    let angle = PI * (2 * section + 1) as f64 / (2 * SECTIONS_OF_THE_ORDER) as f64;
    1.0 / (2.0 * angle.cos())
}

#[derive(Clone, Copy)]
struct Cascades {
    first: [Plane; SECTIONS_AN_EDGE],
    second: [Plane; SECTIONS_AN_EDGE],
}

impl Cascades {
    const SILENT: Self = Self {
        first: [[0.0; EDGES]; SECTIONS_AN_EDGE],
        second: [[0.0; EDGES]; SECTIONS_AN_EDGE],
    };

    fn step(&mut self, coefficients: &Coefficients, heard: &mut Plane) {
        for section in 0..SECTIONS_AN_EDGE {
            let b0 = &coefficients.b0[section];
            let b1 = &coefficients.b1[section];
            let b2 = &coefficients.b2[section];
            let a1 = &coefficients.a1[section];
            let a2 = &coefficients.a2[section];
            let first = &mut self.first[section];
            let second = &mut self.second[section];
            for edge in 0..EDGES {
                let input = heard[edge];
                let output = b0[edge] * input + first[edge];
                first[edge] = b1[edge] * input - a1[edge] * output + second[edge];
                second[edge] = b2[edge] * input - a2[edge] * output;
                heard[edge] = output;
            }
        }
    }
}

struct Seen {
    before: [Levels; 3],
}

impl Seen {
    const NOTHING: Self = Self {
        before: [[0.0; LEVELS]; 3],
    };

    fn note(&mut self, levels: &Levels, into: &mut Heights) {
        let [oldest, older, newer] = &self.before;
        let midway: Levels = std::array::from_fn(|level| {
            (9.0 * (older[level] + newer[level]) - oldest[level] - levels[level]) / 16.0
        });
        for (node, height) in TREE.iter().zip(into.iter_mut()).skip(WHOLE + 1) {
            let at = levels[node.to] - levels[node.from];
            let between = midway[node.to] - midway[node.from];
            *height = height.max(at.abs()).max(between.abs());
        }
        self.before = [self.before[1], self.before[2], *levels];
    }
}

struct Tiles {
    frames: usize,
    loudest: Vec<Heights>,
}

impl Tiles {
    fn holding(&mut self, frame: usize) -> &mut Heights {
        let mut tile = frame / self.frames;
        while tile >= self.loudest.len() {
            self.loudest.push([0.0; NODES]);
            if self.loudest.len() > TILES_AT_MOST {
                self.loudest = self
                    .loudest
                    .chunks(2)
                    .map(|pair| {
                        let other = pair.last().unwrap_or(&pair[0]);
                        std::array::from_fn(|node| pair[0][node].max(other[node]))
                    })
                    .collect();
                self.frames *= 2;
                tile = frame / self.frames;
            }
        }
        &mut self.loudest[tile]
    }
}

struct Channel {
    forward: Cascades,
    heard: Vec<f64>,
    passed: Vec<Plane>,
    seen: Seen,
}

impl Channel {
    const fn new() -> Self {
        Self {
            forward: Cascades::SILENT,
            heard: Vec::new(),
            passed: Vec::new(),
            seen: Seen::NOTHING,
        }
    }

    fn pass(&mut self, coefficients: &Coefficients, sample: f64) -> Plane {
        let mut lanes = [sample; EDGES];
        self.forward.step(coefficients, &mut lanes);
        lanes
    }

    fn begin(&mut self, coefficients: &Coefficients, margin: usize) {
        let before = margin.min(self.heard.len().saturating_sub(1));
        for back in (1..=before).rev() {
            self.pass(coefficients, self.heard[back]);
        }
        self.pass_from(coefficients, 0);
    }

    fn pass_from(&mut self, coefficients: &Coefficients, from: usize) {
        for at in from..self.heard.len() {
            let lanes = self.pass(coefficients, self.heard[at]);
            self.passed.push(lanes);
        }
    }
}

pub(crate) struct Splitting {
    chunk: usize,
    margin: usize,
    coefficients: Coefficients,
    channels: Vec<Channel>,
    begun: bool,
    base: usize,
    backward: Vec<Plane>,
    tiles: Tiles,
    meter: TruePeakMeter,
    metered: Vec<f32>,
    metered_from: usize,
    tile_frames: usize,
    noted: usize,
    shortfalls: Lanes,
}

impl Splitting {
    pub(crate) fn new(rate: SampleRate, channels: usize) -> Self {
        let channels = channels.max(1);
        let hz = rate.hz() as usize;
        let tile_frames = (hz / TILES_A_SECOND).max(1);
        Self {
            chunk: (hz / CHUNKS_A_SECOND).max(1),
            margin: (hz / MARGINS_A_SECOND).max(1),
            coefficients: Coefficients::at(rate),
            channels: (0..channels).map(|_| Channel::new()).collect(),
            begun: false,
            base: 0,
            backward: Vec::new(),
            tiles: Tiles {
                frames: tile_frames,
                loudest: Vec::new(),
            },
            meter: TruePeakMeter::at(
                rate,
                NonZeroUsize::new(channels).unwrap_or(NonZeroUsize::MIN),
            ),
            metered: Vec::new(),
            metered_from: 0,
            tile_frames,
            noted: 0,
            shortfalls: shortfalls_at(rate),
        }
    }

    pub(crate) fn note(&mut self, interleaved: &[f32]) {
        let width = self.channels.len();
        for frame in interleaved.chunks_exact(width) {
            for (channel, sample) in self.channels.iter_mut().zip(frame) {
                channel.heard.push(f64::from(*sample));
            }
            self.meter_frame(frame);
        }

        if self.begun {
            for channel in &mut self.channels {
                let from = channel.passed.len();
                channel.pass_from(&self.coefficients, from);
            }
        } else if self.frames_held() > self.margin {
            self.begin();
        }

        while self.begun && self.frames_held() >= self.chunk + self.margin {
            self.settle(self.chunk, false);
        }
    }

    pub(crate) fn finished(mut self) -> BandPeaks {
        if !self.begun {
            self.begin();
        }
        let unsettled = self.frames_held();
        self.settle(unsettled, true);

        let last = self.noted.saturating_sub(1);
        let tile = self.tiles.holding(last);
        for channel in &mut self.channels {
            for _ in 0..LAST_MIDPOINT_FRAMES {
                channel.seen.note(&[0.0; LEVELS], tile);
            }
        }

        if !self.metered.is_empty() {
            let peak = self.meter.take(&self.metered);
            let whole = &mut self.tiles.holding(self.metered_from)[WHOLE];
            *whole = whole.max(peak);
        }
        let ringing = self.meter.finish();
        let whole = &mut self.tiles.holding(last)[WHOLE];
        *whole = whole.max(ringing);

        if self.noted == 0 {
            return BandPeaks::SILENT;
        }
        for heights in &mut self.tiles.loudest {
            for (height, node) in heights.iter_mut().zip(&TREE).skip(WHOLE + 1) {
                *height /= self.shortfalls[node.to - 1];
            }
        }
        grouped(&self.tiles.loudest)
    }

    fn meter_frame(&mut self, frame: &[f32]) {
        if self.metered.is_empty() {
            self.metered_from = self.noted;
        }
        self.metered.extend_from_slice(frame);
        self.noted += 1;
        if self.noted.is_multiple_of(self.tile_frames) {
            let peak = self.meter.take(&self.metered);
            let whole = &mut self.tiles.holding(self.metered_from)[WHOLE];
            *whole = whole.max(peak);
            self.metered.clear();
        }
    }

    fn frames_held(&self) -> usize {
        self.channels
            .first()
            .map_or(0, |channel| channel.heard.len())
    }

    fn begin(&mut self) {
        for channel in &mut self.channels {
            channel.begin(&self.coefficients, self.margin);
        }
        self.begun = true;
    }

    fn settle(&mut self, frames: usize, ending: bool) {
        if frames == 0 {
            return;
        }
        for index in 0..self.channels.len() {
            self.settle_channel(index, frames, ending);
        }
        for channel in &mut self.channels {
            channel.heard.drain(..frames);
            channel.passed.drain(..frames);
        }
        self.base += frames;
    }

    fn settle_channel(&mut self, index: usize, frames: usize, ending: bool) {
        let channel = &mut self.channels[index];
        self.backward.clear();
        if ending {
            let held = channel.heard.len();
            let after = self.margin.min(held.saturating_sub(1));
            self.backward.extend_from_slice(&channel.passed);
            for ahead in 1..=after {
                let sample = channel.heard[held - 1 - ahead];
                let lanes = channel.pass(&self.coefficients, sample);
                self.backward.push(lanes);
            }
        } else {
            self.backward
                .extend_from_slice(&channel.passed[..frames + self.margin]);
        }

        let mut backward = Cascades::SILENT;
        for lanes in self.backward.iter_mut().rev() {
            backward.step(&self.coefficients, lanes);
        }

        for at in 0..frames {
            let mut levels = [0.0; LEVELS];
            levels[1..BANDS].copy_from_slice(&self.backward[at]);
            levels[BANDS] = channel.heard[at];
            let tile = self.tiles.holding(self.base + at);
            channel.seen.note(&levels, tile);
        }
    }
}

fn shortfalls_at(rate: SampleRate) -> Lanes {
    let hz = f64::from(rate.hz());
    let nyquist = hz / 2.0;
    std::array::from_fn(|band| {
        let upper = if band + 1 < BANDS {
            edge_of(band + 1) * 2.0_f64.powf(OCTAVES_BEYOND_A_BAND_READ)
        } else {
            nyquist
        };
        let angle = TAU * upper.min(WIDEST_EDGE_OF_A_BAND_READ * nyquist) / hz;
        shortfall(angle).max(LEAST_SEEN_OF_A_PEAK)
    })
}

fn shortfall(angle: f64) -> f64 {
    (0..PROBE_OFFSETS)
        .map(|step| {
            let offset = angle * step as f64 / PROBE_OFFSETS as f64;
            let tone: Vec<f64> = (-PROBE_FRAMES..=PROBE_FRAMES)
                .map(|frame| (angle * frame as f64 - offset).cos())
                .collect();
            tone.windows(4)
                .flat_map(|four| {
                    let midway = (9.0 * (four[1] + four[2]) - four[0] - four[3]) / 16.0;
                    [four[3].abs(), midway.abs()]
                })
                .fold(0.0, f64::max)
        })
        .fold(f64::INFINITY, f64::min)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: SampleRate = SampleRate::HZ_44100;

    fn faded_tone(hertz: f64, amplitude: f64, channels: usize) -> Vec<f32> {
        let frames = RATE.hz() as usize * 2;
        (0..frames)
            .flat_map(|frame| {
                let position = frame as f64 / frames as f64;
                let envelope = (PI * position).sin().powi(2);
                let sample = (TAU * hertz * frame as f64 / f64::from(RATE.hz())).sin()
                    * amplitude
                    * envelope;
                vec![sample as f32; channels]
            })
            .collect()
    }

    fn peaks_of(signal: &[f32], channels: usize, piece: usize) -> BandPeaks {
        let mut splitting = Splitting::new(RATE, channels);
        for part in signal.chunks(piece * channels) {
            splitting.note(part);
        }
        splitting.finished()
    }

    fn leaves(peaks: &BandPeaks) -> Vec<f64> {
        (0..BANDS)
            .map(|band| {
                let leaf = TREE
                    .iter()
                    .position(|node| node.halves.is_none() && node.from == band)
                    .expect("a leaf");
                peaks
                    .moments()
                    .map(|heights| heights[leaf])
                    .fold(0.0, f64::max)
            })
            .collect()
    }

    fn whole(peaks: &BandPeaks) -> f64 {
        peaks
            .moments()
            .map(|heights| heights[WHOLE])
            .fold(0.0, f64::max)
    }

    #[test]
    fn a_tone_is_held_between_the_bands_around_it_and_they_hold_no_more_than_it_does() {
        let measured = peaks_of(&faded_tone(1_000.0, 0.8, 2), 2, 4_096);
        let peaks = leaves(&measured);

        let together: f64 = peaks.iter().sum();
        let loudest = peaks.iter().copied().fold(0.0, f64::max);

        assert!((0.8..0.8 * 1.05).contains(&together), "{together}");
        assert!(loudest < 0.8 * 0.6, "{loudest}");
        assert!((whole(&measured) - 0.8).abs() < 0.8 * 0.02, "{measured:?}");
    }

    #[test]
    fn a_stream_noted_in_pieces_has_the_peaks_it_has_noted_whole() {
        let signal = faded_tone(250.0, 0.6, 2);

        let whole = peaks_of(&signal, 2, signal.len() / 2);
        let in_blocks = peaks_of(&signal, 2, 1_000);
        let in_odd_blocks = peaks_of(&signal, 2, 7_919);

        assert_eq!(whole, in_blocks);
        assert_eq!(whole, in_odd_blocks);
    }

    #[test]
    fn the_louder_of_two_channels_sets_the_peak_of_each_band_and_silence_has_none() {
        let mut stereo = Vec::new();
        for (loud, quiet) in faded_tone(500.0, 0.7, 1)
            .into_iter()
            .zip(faded_tone(500.0, 0.1, 1))
        {
            stereo.extend([quiet, loud]);
        }
        let silence = vec![0.0_f32; RATE.hz() as usize * 2];

        let heard = peaks_of(&stereo, 2, 4_096);
        let mono_of_the_louder = peaks_of(&faded_tone(500.0, 0.7, 1), 1, 4_096);

        assert_eq!(heard, mono_of_the_louder);
        assert!(peaks_of(&silence, 1, 4_096).is_silent());
    }

    #[test]
    fn a_stream_shorter_than_the_margin_still_has_its_peaks() {
        let signal: Vec<f32> = (0..300)
            .map(|frame| {
                (TAU * 1_000.0 * f64::from(frame) / f64::from(RATE.hz())).sin() as f32 * 0.5
            })
            .collect();

        let peaks = peaks_of(&signal, 1, 100);

        assert!(!peaks.is_silent());
    }

    #[test]
    fn a_burst_is_kept_apart_from_a_tone_heard_long_after_it() {
        let rate = f64::from(RATE.hz());
        let signal: Vec<f32> = (0..RATE.hz() as usize * 3)
            .map(|frame| {
                let seconds = frame as f64 / rate;
                let burst = if (0.5..0.6).contains(&seconds) {
                    (TAU * 60.0 * seconds).sin() * 0.7
                } else {
                    0.0
                };
                let tone = if (2.0..2.5).contains(&seconds) {
                    (TAU * 6_000.0 * seconds).sin() * 0.6
                } else {
                    0.0
                };
                (burst + tone) as f32
            })
            .collect();
        let low = TREE
            .iter()
            .position(|node| node.from == 0 && node.to == BANDS / 2)
            .expect("the lower half");
        let high = TREE
            .iter()
            .position(|node| node.from == BANDS / 2 && node.to == BANDS)
            .expect("the upper half");

        let peaks = peaks_of(&signal, 1, 4_096);

        assert!(peaks.moments().count() >= 2);
        assert!(
            peaks
                .moments()
                .all(|heights| heights[low] < 0.2 || heights[high] < 0.2),
            "{peaks:?}"
        );
    }
}
