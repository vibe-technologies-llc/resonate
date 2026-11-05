use std::{
    f64::consts::{PI, TAU},
    num::NonZeroUsize,
};

use rayon::{
    iter::{IndexedParallelIterator, IntoParallelIterator, ParallelIterator},
    slice::ParallelSliceMut,
};
use resonate_core::{
    SampleRate,
    eq::{Band, BandGain, BandKind, Biquad, Frequency, Q},
};
use resonate_dsp::TruePeakMeter;

use crate::{
    bands::{BANDS, edge_of},
    coinciding::Coinciding,
    crew::Noting,
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

const HISTORY: usize = 3;

const FRAMES_READ_AT_ONCE: usize = 64;

const READ_LANES: usize = 8;

const WIDEST_EDGE_OF_A_BAND_READ: f64 = 0.9;

const OCTAVES_BEYOND_A_BAND_READ: f64 = 2.0 / 3.0;

type Plane = [f64; EDGES];

type Lanes = [f64; BANDS];

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

struct Tiles<const WIDTH: usize> {
    frames: usize,
    loudest: Vec<[f64; WIDTH]>,
}

impl<const WIDTH: usize> Tiles<WIDTH> {
    const fn each(frames: usize) -> Self {
        Self {
            frames,
            loudest: Vec::new(),
        }
    }

    fn holding(&mut self, frame: usize) -> &mut [f64; WIDTH] {
        let mut tile = frame / self.frames;
        while tile >= self.loudest.len() {
            self.loudest.push([0.0; WIDTH]);
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

    fn frames_left_of_the_tile_holding(&mut self, frame: usize) -> usize {
        self.holding(frame);
        self.frames - frame % self.frames
    }
}

fn loudest_apart(
    start: f64,
    to: &[f64],
    from: &[f64],
    midway_to: &[f64],
    midway_from: &[f64],
) -> f64 {
    let mut lanes = [start; READ_LANES];
    let chunks = to
        .as_chunks::<READ_LANES>()
        .0
        .iter()
        .zip(from.as_chunks::<READ_LANES>().0)
        .zip(midway_to.as_chunks::<READ_LANES>().0)
        .zip(midway_from.as_chunks::<READ_LANES>().0);
    for (((to, from), midway_to), midway_from) in chunks {
        for lane in 0..READ_LANES {
            let at = (to[lane] - from[lane]).abs();
            let between = (midway_to[lane] - midway_from[lane]).abs();
            lanes[lane] = larger(lanes[lane], larger(at, between));
        }
    }
    let mut loudest = lanes.into_iter().fold(start, larger);
    let read = to.len() / READ_LANES * READ_LANES;
    for at in read..to.len() {
        loudest = larger(loudest, (to[at] - from[at]).abs());
        loudest = larger(loudest, (midway_to[at] - midway_from[at]).abs());
    }
    loudest
}

const fn larger(one: f64, other: f64) -> f64 {
    if one > other { one } else { other }
}

struct Reading {
    levels: Box<[[f64; HISTORY + FRAMES_READ_AT_ONCE]; LEVELS]>,
    midway: Box<[[f64; FRAMES_READ_AT_ONCE]; LEVELS]>,
}

impl Reading {
    fn silent() -> Self {
        Self {
            levels: Box::new([[0.0; HISTORY + FRAMES_READ_AT_ONCE]; LEVELS]),
            midway: Box::new([[0.0; FRAMES_READ_AT_ONCE]; LEVELS]),
        }
    }

    fn take_in(&mut self, passed: &[Plane], heard: &[f64]) {
        for (frame, plane) in passed.iter().enumerate() {
            for (column, lane) in self.levels[1..BANDS].iter_mut().zip(plane) {
                column[HISTORY + frame] = *lane;
            }
        }
        self.levels[BANDS][HISTORY..HISTORY + heard.len()].copy_from_slice(heard);
    }

    fn take_in_silence(&mut self, frames: usize) {
        for column in &mut self.levels[1..] {
            column[HISTORY..HISTORY + frames].fill(0.0);
        }
    }

    fn read_into(&mut self, frames: usize, into: &mut Heights) {
        for (midway, column) in self.midway[1..].iter_mut().zip(&self.levels[1..]) {
            let oldest = &column[..frames];
            let older = &column[1..=frames];
            let newer = &column[2..frames + 2];
            let levels = &column[HISTORY..HISTORY + frames];
            let reads = oldest.iter().zip(older).zip(newer).zip(levels);
            for (slot, (((oldest, older), newer), level)) in midway[..frames].iter_mut().zip(reads)
            {
                *slot = (9.0 * (older + newer) - oldest - level) / 16.0;
            }
        }

        for (node, height) in TREE.iter().zip(into.iter_mut()).skip(WHOLE + 1) {
            *height = loudest_apart(
                *height,
                &self.levels[node.to][HISTORY..HISTORY + frames],
                &self.levels[node.from][HISTORY..HISTORY + frames],
                &self.midway[node.to][..frames],
                &self.midway[node.from][..frames],
            );
        }

        for column in &mut self.levels[1..] {
            column.copy_within(frames..frames + HISTORY, 0);
        }
    }
}

struct Bank {
    index: usize,
    width: usize,
    chunk: usize,
    margin: usize,
    coefficients: Coefficients,
    forward: Cascades,
    heard: Vec<f64>,
    passed: Vec<Plane>,
    begun: bool,
    base: usize,
    reading: Reading,
    tiles: Tiles<NODES>,
}

impl Noting for Bank {
    fn note(&mut self, interleaved: &[f32]) {
        Self::note(self, interleaved);
    }

    fn hears_any_pieces_alike(&self) -> bool {
        true
    }
}

impl Noting for Metered {
    fn note(&mut self, interleaved: &[f32]) {
        Self::note(self, interleaved);
    }

    fn hears_any_pieces_alike(&self) -> bool {
        true
    }
}

impl Bank {
    fn new(index: usize, width: usize, rate: SampleRate) -> Self {
        let hz = rate.hz() as usize;
        Self {
            index,
            width,
            chunk: (hz / CHUNKS_A_SECOND).max(1),
            margin: (hz / MARGINS_A_SECOND).max(1),
            coefficients: Coefficients::at(rate),
            forward: Cascades::SILENT,
            heard: Vec::new(),
            passed: Vec::new(),
            begun: false,
            base: 0,
            reading: Reading::silent(),
            tiles: Tiles::each(tile_frames_at(hz)),
        }
    }

    fn note(&mut self, interleaved: &[f32]) {
        let width = self.width;
        self.heard.extend(
            interleaved
                .iter()
                .skip(self.index)
                .step_by(width)
                .map(|sample| f64::from(*sample)),
        );

        if !self.begun {
            if self.heard.len() <= self.margin {
                return;
            }
            self.begin();
        }
        self.pass_and_settle();
    }

    fn begin(&mut self) {
        let before = self.margin.min(self.heard.len().saturating_sub(1));
        for back in (1..=before).rev() {
            let mut lanes = [self.heard[back]; EDGES];
            self.forward.step(&self.coefficients, &mut lanes);
        }
        self.begun = true;
    }

    fn pass_and_settle(&mut self) {
        let passed = self.passed.len();
        let chunks = passed.saturating_sub(self.margin) / self.chunk;
        self.passed.resize(self.heard.len(), [0.0; EDGES]);
        let (settling, fresh) = self.passed.split_at_mut(passed);

        let Self {
            coefficients,
            forward,
            heard,
            reading,
            tiles,
            chunk,
            margin,
            base,
            ..
        } = self;
        together(
            || {
                for (lanes, sample) in fresh.iter_mut().zip(&heard[passed..]) {
                    *lanes = [*sample; EDGES];
                    forward.step(coefficients, lanes);
                }
            },
            || {
                settle_chunks(coefficients, settling, chunks, *chunk, *margin);
                let frames = chunks * *chunk;
                read_settled(reading, tiles, &settling[..frames], &heard[..frames], *base);
            },
        );

        let frames = chunks * self.chunk;
        self.heard.drain(..frames);
        self.passed.drain(..frames);
        self.base += frames;
    }

    fn finished(mut self) -> Tiles<NODES> {
        if !self.begun {
            self.begin();
        }
        self.pass_and_settle();
        self.pass_and_settle();

        let held = self.heard.len();
        let after = self.margin.min(held.saturating_sub(1));
        self.passed.reserve(after);
        for ahead in 1..=after {
            let mut lanes = [self.heard[held - 1 - ahead]; EDGES];
            self.forward.step(&self.coefficients, &mut lanes);
            self.passed.push(lanes);
        }
        if held > 0 {
            let (settled, ahead) = self.passed.split_at_mut(held);
            backward_over(&self.coefficients, settled, ahead);
            read_settled(
                &mut self.reading,
                &mut self.tiles,
                settled,
                &self.heard,
                self.base,
            );
        }

        let last = (self.base + held).saturating_sub(1);
        self.reading.take_in_silence(LAST_MIDPOINT_FRAMES);
        self.reading
            .read_into(LAST_MIDPOINT_FRAMES, self.tiles.holding(last));
        self.tiles
    }
}

fn on_the_crew() -> bool {
    rayon::current_thread_index().is_some()
}

fn together(one: impl FnOnce() + Send, other: impl FnOnce() + Send) {
    if on_the_crew() {
        rayon::join(one, other);
    } else {
        one();
        other();
    }
}

fn state_before(coefficients: &Coefficients, ahead: &[Plane]) -> Cascades {
    let mut backward = Cascades::SILENT;
    for lanes in ahead.iter().rev() {
        let mut lanes = *lanes;
        backward.step(coefficients, &mut lanes);
    }
    backward
}

fn backward_over(coefficients: &Coefficients, settled: &mut [Plane], ahead: &[Plane]) {
    let backward = state_before(coefficients, ahead);
    finish_backward(coefficients, settled, backward);
}

fn finish_backward(coefficients: &Coefficients, settled: &mut [Plane], mut backward: Cascades) {
    for lanes in settled.iter_mut().rev() {
        backward.step(coefficients, lanes);
    }
}

fn settle_chunks(
    coefficients: &Coefficients,
    passed: &mut [Plane],
    chunks: usize,
    chunk: usize,
    margin: usize,
) {
    if chunks == 0 {
        return;
    }
    if chunks > 1 && on_the_crew() {
        let held: &[Plane] = passed;
        let states: Vec<Cascades> = (1..=chunks)
            .into_par_iter()
            .map(|nth| state_before(coefficients, &held[nth * chunk..nth * chunk + margin]))
            .collect();
        passed[..chunks * chunk]
            .par_chunks_mut(chunk)
            .zip(states)
            .for_each(|(settled, state)| finish_backward(coefficients, settled, state));
    } else {
        for nth in 0..chunks {
            let (settled, ahead) = passed[nth * chunk..].split_at_mut(chunk);
            backward_over(coefficients, settled, &ahead[..margin]);
        }
    }
}

fn read_settled(
    reading: &mut Reading,
    tiles: &mut Tiles<NODES>,
    settled: &[Plane],
    heard: &[f64],
    base: usize,
) {
    let frames = settled.len();
    let mut at = 0;
    while at < frames {
        let frame = base + at;
        let run = tiles
            .frames_left_of_the_tile_holding(frame)
            .min(FRAMES_READ_AT_ONCE)
            .min(frames - at);
        reading.take_in(&settled[at..at + run], &heard[at..at + run]);
        reading.read_into(run, tiles.holding(frame));
        at += run;
    }
}

struct Metered {
    width: usize,
    meter: TruePeakMeter,
    metered: Vec<f32>,
    metered_from: usize,
    tile_frames: usize,
    noted: usize,
    tiles: Tiles<1>,
}

impl Metered {
    fn new(rate: SampleRate, width: usize) -> Self {
        let tile_frames = tile_frames_at(rate.hz() as usize);
        Self {
            width,
            meter: TruePeakMeter::at(rate, NonZeroUsize::new(width).unwrap_or(NonZeroUsize::MIN)),
            metered: Vec::new(),
            metered_from: 0,
            tile_frames,
            noted: 0,
            tiles: Tiles::each(tile_frames),
        }
    }

    fn note(&mut self, interleaved: &[f32]) {
        let mut left = interleaved;
        while left.len() >= self.width {
            if self.metered.is_empty() {
                self.metered_from = self.noted;
            }
            let room = self.tile_frames - self.noted % self.tile_frames;
            let taken = room.min(left.len() / self.width);
            let (these, rest) = left.split_at(taken * self.width);
            self.metered.extend_from_slice(these);
            self.noted += taken;
            left = rest;
            if self.noted.is_multiple_of(self.tile_frames) {
                self.read_what_was_metered();
            }
        }
    }

    fn read_what_was_metered(&mut self) {
        let peak = self.meter.take(&self.metered);
        let [whole] = self.tiles.holding(self.metered_from);
        *whole = whole.max(peak);
        self.metered.clear();
    }

    fn finished(mut self) -> Tiles<1> {
        if !self.metered.is_empty() {
            self.read_what_was_metered();
        }
        let ringing = self.meter.finish();
        let [whole] = self.tiles.holding(self.noted.saturating_sub(1));
        *whole = whole.max(ringing);
        self.tiles
    }
}

pub(crate) struct Split {
    pub(crate) peaks: BandPeaks,
    pub(crate) true_peak: f64,
}

pub(crate) struct Splitting {
    banks: Vec<Bank>,
    metered: Metered,
    shortfalls: Lanes,
    coinciding: Coinciding,
}

impl Splitting {
    pub(crate) fn new(rate: SampleRate, channels: usize) -> Self {
        let channels = channels.max(1);
        Self {
            banks: (0..channels)
                .map(|index| Bank::new(index, channels, rate))
                .collect(),
            metered: Metered::new(rate, channels),
            shortfalls: shortfalls_at(rate),
            coinciding: Coinciding::new(rate, channels),
        }
    }

    #[cfg(test)]
    pub(crate) fn note(&mut self, interleaved: &[f32]) {
        self.coinciding.note(interleaved);
        self.metered.note(interleaved);
        for bank in &mut self.banks {
            bank.note(interleaved);
        }
    }

    pub(crate) fn stages(&mut self) -> impl Iterator<Item = &mut dyn Noting> {
        self.banks
            .iter_mut()
            .map(|bank| bank as &mut dyn Noting)
            .chain(std::iter::once(&mut self.metered as &mut dyn Noting))
            .chain(self.coinciding.stages())
    }

    pub(crate) fn finished(self) -> Split {
        if self.metered.noted == 0 {
            return Split {
                peaks: BandPeaks::SILENT,
                true_peak: 0.0,
            };
        }

        let mut tiles = Vec::new();
        for bank in self.banks {
            let finished = bank.finished();
            if tiles.is_empty() {
                tiles = finished.loudest;
            } else {
                for (held, more) in tiles.iter_mut().zip(&finished.loudest) {
                    let held: &mut Heights = held;
                    for (held, more) in held.iter_mut().zip(more) {
                        *held = held.max(*more);
                    }
                }
            }
        }
        let metered = self.metered.finished().loudest;
        let metered_peak = metered
            .iter()
            .fold(0.0, |loudest, [whole]| f64::max(loudest, *whole));
        for (heights, [whole]) in tiles.iter_mut().zip(metered) {
            heights[WHOLE] = whole;
        }

        for heights in &mut tiles {
            for (height, node) in heights.iter_mut().zip(&TREE).skip(WHOLE + 1) {
                *height /= self.shortfalls[node.to - 1];
            }
        }
        let true_peak = tiles
            .iter()
            .fold(0.0, |loudest, heights| f64::max(loudest, heights[WHOLE]));
        Split {
            peaks: grouped(&tiles).held_together(self.coinciding.finished(true_peak).as_ref()),
            true_peak: metered_peak,
        }
    }
}

fn tile_frames_at(hz: usize) -> usize {
    (hz / TILES_A_SECOND).max(1)
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
    use crate::crew::Crew;

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
        splitting.finished().peaks
    }

    fn peaks_on_a_crew(signal: &[f32], channels: usize, crew: Crew) -> BandPeaks {
        let mut splitting = Splitting::new(RATE, channels);
        crew.fanned(
            RATE.hz() as usize * channels,
            splitting.stages().collect(),
            &|| false,
            |feed| {
                for part in signal.chunks(4_096 * channels) {
                    feed(part);
                }
            },
        );
        splitting.finished().peaks
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

    fn loud_and_bright(channels: usize) -> Vec<f32> {
        let frames = RATE.hz() as usize * 3;
        let mut state = 0x2545_f491_u32;
        (0..frames)
            .flat_map(|frame| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                let hiss = f64::from(state) / f64::from(u32::MAX) - 0.5;
                let swell = 0.2 + 0.7 * (PI * frame as f64 / frames as f64).sin().powi(2);
                (0..channels).map(move |channel| {
                    let tone = (TAU * (9_000.0 + 2_300.0 * channel as f64) * frame as f64
                        / f64::from(RATE.hz()))
                    .sin();
                    (swell * (0.7 * tone + 0.3 * hiss)) as f32
                })
            })
            .collect()
    }

    #[test]
    fn the_peak_of_the_tiles_is_the_one_a_true_peak_meter_reads_over_the_whole_song() {
        for channels in [1, 2, 6] {
            let signal = loud_and_bright(channels);
            let mut meter =
                TruePeakMeter::at(RATE, NonZeroUsize::new(channels).expect("a channel"));
            meter.note(&signal);
            let read_whole = meter.finish();
            let sampled = signal.iter().fold(0.0_f64, |loudest, sample| {
                loudest.max(f64::from(sample.abs()))
            });
            assert!(read_whole > sampled, "{read_whole} against {sampled}");

            for piece in [signal.len() / channels, 1_000, 7_919] {
                let mut splitting = Splitting::new(RATE, channels);
                for part in signal.chunks(piece * channels) {
                    splitting.note(part);
                }

                assert_eq!(
                    splitting.finished().true_peak.to_bits(),
                    read_whole.to_bits(),
                    "{channels} channels in pieces of {piece}",
                );
            }
        }
    }

    #[test]
    fn a_song_with_no_frames_has_no_true_peak() {
        assert_eq!(Splitting::new(RATE, 2).finished().true_peak, 0.0);
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
    fn a_stream_heard_on_a_crew_has_the_peaks_it_has_heard_alone() {
        let signal = faded_tone(250.0, 0.6, 2);
        let long: Vec<f32> = signal
            .iter()
            .chain(&signal)
            .chain(&signal)
            .copied()
            .collect();

        let alone = peaks_of(&long, 2, 4_096);

        assert_eq!(alone, peaks_on_a_crew(&long, 2, Crew::Background));
        assert_eq!(alone, peaks_on_a_crew(&long, 2, Crew::AsTheMachineAllows));
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
