use std::{f64::consts::PI, sync::Arc};

use resonate_core::SampleRate;
use rustfft::{Fft, FftPlanner, num_complex::Complex};

use crate::bands::{BANDS, edge_of};

pub(crate) const GROUPS: usize = 3;

pub(crate) const GROUP_EDGES: [usize; GROUPS + 1] = [0, 7, 21, BANDS];

const STEPS_OF_A_WEIGHT: usize = 3;

const HALF_A_TABLE: usize = STEPS_OF_A_WEIGHT * STEPS_OF_A_WEIGHT;

const WEIGHINGS_AND_NONE: usize = HALF_A_TABLE * HALF_A_TABLE;

pub(crate) const WEIGHINGS: usize = WEIGHINGS_AND_NONE - 1;

const MARGINS_A_SECOND: usize = 5;

const MARGIN_AT_LEAST: usize = 2 * READ_REACH;

const EDGE_ORDER: i32 = 16;

const ANSWER_HELD_FROM: f64 = 1e-5;

const POINTS_A_BIN_AT_LEAST: usize = 2;

const HELD_FOR_SECONDS: [f64; GROUPS] = [1e-3, 2.5e-4, 5e-5];

const SHARPENED_BY_AT_MOST: [f32; GROUPS] = [1.0, 1.0, 1.12];

const HEARD_AROUND_HZ: [f64; GROUPS] = [60.0, 640.0, 8_000.0];

const HEARD_ACROSS_OCTAVES: [f64; GROUPS] = [1.2, 1.5, 1.2];

const HELD_TOGETHER_WITHIN_SECONDS: [f64; GROUPS] = [8e-3, 1e-3, 2.5e-4];

const COMPACT_WHILE_APART_AT_MOST: f64 = 0.5;

const COMPACT_BINS_A_TAP: usize = 2;

const READ_TWICE_OVER_UP_TO_HZ: u32 = 48_000;

const READ_ONCE_OVER_UP_TO_HZ: u32 = 96_000;

const READ_REACH: usize = 16;

const READ_TAPS: usize = 2 * READ_REACH;

const READ_LANES: usize = 8;

fn offsets_read_between(rate: SampleRate) -> &'static [f64] {
    match rate.hz() {
        hz if hz <= READ_TWICE_OVER_UP_TO_HZ => &[-0.5, -0.25, 0.25],
        hz if hz <= READ_ONCE_OVER_UP_TO_HZ => &[-0.5],
        _ => &[],
    }
}

fn read_at(offset: f64) -> [f32; READ_TAPS] {
    let reach = READ_REACH as f64 + 0.5;
    let taps: [f64; READ_TAPS] = std::array::from_fn(|tap| {
        let apart = tap as f64 + 1.0 - READ_REACH as f64 - offset;
        let sinc = if apart == 0.0 {
            1.0
        } else {
            (PI * apart).sin() / (PI * apart)
        };
        sinc * (0.5 + 0.5 * (PI * apart / reach).cos())
    });
    let whole: f64 = taps.iter().sum();
    taps.map(|tap| (tap / whole) as f32)
}

fn dotted(heard: &[f32; READ_TAPS], read: &[f32; READ_TAPS]) -> f32 {
    let mut lanes = [0.0_f32; READ_LANES];
    for (heard, read) in heard
        .as_chunks::<READ_LANES>()
        .0
        .iter()
        .zip(read.as_chunks::<READ_LANES>().0)
    {
        for (lane, (heard, read)) in lanes.iter_mut().zip(heard.iter().zip(read)) {
            *lane += heard * read;
        }
    }
    lanes.iter().sum()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Weighing {
    pub(crate) whole: f64,
    pub(crate) groups: [f64; GROUPS],
}

pub(crate) fn weighing(index: usize) -> Weighing {
    let at = index + 1;
    let weight = |digit: usize| digit as f64 / (STEPS_OF_A_WEIGHT - 1) as f64;
    Weighing {
        whole: weight(at / (HALF_A_TABLE * STEPS_OF_A_WEIGHT) % STEPS_OF_A_WEIGHT),
        groups: [
            weight(at / HALF_A_TABLE % STEPS_OF_A_WEIGHT),
            weight(at / STEPS_OF_A_WEIGHT % STEPS_OF_A_WEIGHT),
            weight(at % STEPS_OF_A_WEIGHT),
        ],
    }
}

pub(crate) fn group_passes(group: usize, hertz: f64, rate: SampleRate) -> f64 {
    let below = |edge: usize| {
        if edge == 0 {
            0.0
        } else if edge >= BANDS {
            1.0
        } else {
            passed_at(edge, hertz, rate)
        }
    };
    below(GROUP_EDGES[group + 1]) - below(GROUP_EDGES[group])
}

fn passed_at(edge: usize, hertz: f64, rate: SampleRate) -> f64 {
    let angle_of = |hertz: f64| (PI * hertz / f64::from(rate.hz())).tan();
    let nyquist = f64::from(rate.hz()) / 2.0;
    if hertz >= nyquist {
        return 0.0;
    }
    let turned = angle_of(hertz) / angle_of(edge_of(edge).min(nyquist * (1.0 - 1e-6)));
    1.0 / (1.0 + turned.powi(EDGE_ORDER))
}

struct Group {
    first: usize,
    answers: Vec<f32>,
    inverse: Arc<dyn Fft<f32>>,
    heard: Vec<Complex<f32>>,
    sizes: Vec<f32>,
    settled: Vec<f32>,
    held_for: usize,
    sharpened_by: f32,
    held: Vec<f32>,
}

impl Group {
    fn over(
        group: usize,
        size: usize,
        hop: usize,
        rate: SampleRate,
        planner: &mut FftPlanner<f32>,
    ) -> Self {
        let bins_a_hertz = size as f64 / f64::from(rate.hz());
        let last_bin = size / 2;
        let answers: Vec<f64> = (0..=last_bin)
            .map(|bin| group_passes(group, bin as f64 / bins_a_hertz, rate).max(0.0))
            .collect();
        let first = answers
            .iter()
            .position(|answer| *answer >= ANSWER_HELD_FROM)
            .unwrap_or(0);
        let last = answers
            .iter()
            .rposition(|answer| *answer >= ANSWER_HELD_FROM)
            .unwrap_or(first);
        let bins = last - first + 1;
        let points = (bins * POINTS_A_BIN_AT_LEAST).next_power_of_two().min(size);
        let scale = 1.0 / size as f64;
        Self {
            first,
            answers: answers[first..=last]
                .iter()
                .enumerate()
                .map(|(into, answer)| {
                    let bin = first + into;
                    let sides = if bin == 0 || bin == last_bin {
                        1.0
                    } else {
                        2.0
                    };
                    (answer * sides * scale) as f32
                })
                .collect(),
            inverse: planner.plan_fft_inverse(points),
            heard: vec![Complex::new(0.0, 0.0); points],
            sizes: vec![0.0; points],
            settled: vec![0.0; points],
            held_for: (HELD_FOR_SECONDS[group] * f64::from(rate.hz()) * points as f64 / size as f64)
                .round() as usize,
            sharpened_by: SHARPENED_BY_AT_MOST[group],
            held: vec![0.0; hop],
        }
    }

    fn hear(&mut self, spectrum: &[Complex<f32>], from: usize, scratch: &mut Vec<Complex<f32>>) {
        self.heard.fill(Complex::new(0.0, 0.0));
        let bins = &spectrum[self.first..self.first + self.answers.len()];
        for ((into, bin), answer) in self.heard.iter_mut().zip(bins).zip(&self.answers) {
            *into = bin * answer;
        }
        scratch.resize(
            self.inverse.get_inplace_scratch_len(),
            Complex::new(0.0, 0.0),
        );
        self.inverse.process_with_scratch(&mut self.heard, scratch);
        for (size, heard) in self.sizes.iter_mut().zip(&self.heard) {
            *size = heard.norm();
        }
        let points = self.sizes.len();
        let window = spectrum.len();
        for _ in 0..self.held_for {
            self.settled.copy_from_slice(&self.sizes);
            for (settled, three) in self.settled[1..].iter_mut().zip(self.sizes.windows(3)) {
                *settled = three[0].max(three[1]).max(three[2]);
            }
            std::mem::swap(&mut self.sizes, &mut self.settled);
        }
        for (into, held) in self.held.iter_mut().enumerate() {
            let point = (from + into) * points / window;
            *held = self.sizes[point].max(self.sizes[(point + 1) % points]) * self.sharpened_by;
        }
    }
}

struct Wholes {
    reads: Vec<[f32; READ_TAPS]>,
    held: Vec<f32>,
}

impl Wholes {
    fn over(hop: usize, rate: SampleRate) -> Self {
        Self {
            reads: offsets_read_between(rate)
                .iter()
                .map(|offset| read_at(*offset))
                .collect(),
            held: vec![0.0; hop],
        }
    }

    fn hear(&mut self, channel: &[f32], from: usize, frames: usize) {
        for (into, held) in self.held[..frames].iter_mut().enumerate() {
            let at = from + into;
            let heard = channel[at + 1 - READ_REACH..at + 1 + READ_REACH]
                .first_chunk::<READ_TAPS>()
                .unwrap_or(&[0.0; READ_TAPS]);
            *held = self.reads.iter().fold(channel[at].abs(), |loudest, read| {
                loudest.max(dotted(heard, read).abs())
            });
        }
    }
}

pub(crate) struct Coinciding {
    hop: usize,
    size: usize,
    pad: usize,
    forward: Arc<dyn Fft<f32>>,
    wholes: Wholes,
    groups: [Group; GROUPS],
    channels: Vec<Vec<f32>>,
    begun: bool,
    spectrum: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    heard: Box<[f32; WEIGHINGS_AND_NONE]>,
    sampled: f32,
    noted: usize,
    settled: usize,
}

impl Coinciding {
    pub(crate) fn new(rate: SampleRate, channels: usize) -> Self {
        let pad = (rate.hz() as usize / MARGINS_A_SECOND).max(MARGIN_AT_LEAST);
        let size = (rate.hz() as usize).max(4 * pad).next_power_of_two();
        let hop = size - 2 * pad;
        let mut planner = FftPlanner::new();
        Self {
            hop,
            size,
            pad,
            forward: planner.plan_fft_forward(size),
            wholes: Wholes::over(hop, rate),
            groups: std::array::from_fn(|group| Group::over(group, size, hop, rate, &mut planner)),
            channels: vec![Vec::new(); channels.max(1)],
            begun: false,
            spectrum: vec![Complex::new(0.0, 0.0); size],
            scratch: Vec::new(),
            heard: Box::new([0.0; WEIGHINGS_AND_NONE]),
            sampled: 0.0,
            noted: 0,
            settled: 0,
        }
    }

    pub(crate) fn note(&mut self, interleaved: &[f32]) {
        let width = self.channels.len();
        for (index, channel) in self.channels.iter_mut().enumerate() {
            channel.extend(interleaved.iter().skip(index).step_by(width));
        }
        self.noted += interleaved.len() / width;
        if !self.begun && self.channels[0].len() > self.hop + self.pad {
            self.begin();
        }
        while self.begun && self.channels[0].len() >= self.size {
            self.settle(self.hop);
        }
    }

    pub(crate) fn finished(mut self, true_peak: f64) -> Option<[f64; WEIGHINGS]> {
        if !self.begun {
            self.begin();
        }
        while self.settled < self.noted {
            let ending = self.pad + self.noted - self.settled;
            for channel in &mut self.channels {
                mirrored_after(channel, ending, self.size);
            }
            self.settle((self.noted - self.settled).min(self.hop));
        }
        if self.sampled <= 0.0 {
            return None;
        }
        let between = (true_peak - f64::from(self.sampled)).max(0.0);
        Some(std::array::from_fn(|index| {
            f64::from(self.heard[index + 1]) + weighing(index).whole * between
        }))
    }

    fn begin(&mut self) {
        for channel in &mut self.channels {
            let before: Vec<f32> = (1..=self.pad)
                .rev()
                .map(|back| channel.get(back).copied().unwrap_or(0.0))
                .collect();
            channel.splice(0..0, before);
        }
        self.begun = true;
    }

    fn settle(&mut self, frames: usize) {
        for index in 0..self.channels.len() {
            self.settle_channel(index, frames);
        }
        for channel in &mut self.channels {
            channel.drain(..self.hop.min(channel.len()));
        }
        self.settled += frames;
    }

    fn settle_channel(&mut self, index: usize, frames: usize) {
        let channel = &self.channels[index];
        for (into, sample) in self.spectrum.iter_mut().zip(channel) {
            *into = Complex::new(*sample, 0.0);
        }
        self.scratch.resize(
            self.forward.get_inplace_scratch_len(),
            Complex::new(0.0, 0.0),
        );
        self.forward
            .process_with_scratch(&mut self.spectrum, &mut self.scratch);
        self.wholes.hear(channel, self.pad, frames);
        for group in &mut self.groups {
            group.hear(&self.spectrum, self.pad, &mut self.scratch);
        }

        let [low, middle, high] = &self.groups;
        let heard = self.heard.as_mut();
        let mut sampled = self.sampled;
        for (at, whole) in self.wholes.held[..frames].iter().copied().enumerate() {
            sampled = sampled.max(whole);
            let front = table_of(whole, low.held[at]);
            let back = table_of(middle.held[at], high.held[at]);
            for (row, before) in heard
                .as_chunks_mut::<HALF_A_TABLE>()
                .0
                .iter_mut()
                .zip(&front)
            {
                for (held, after) in row.iter_mut().zip(&back) {
                    *held = held.max(before + after);
                }
            }
        }
        self.sampled = sampled;
    }
}

pub(crate) fn stays_together(responses: &[(Vec<f64>, usize)], rate: SampleRate) -> bool {
    let longest = responses
        .iter()
        .map(|(taps, _)| taps.len())
        .max()
        .unwrap_or(0)
        .max(1);
    let size = (longest * COMPACT_BINS_A_TAP).next_power_of_two();
    let mut planner = FftPlanner::<f64>::new();
    let forward = planner.plan_fft_forward(size);
    let inverse = planner.plan_fft_inverse(size);
    let hertz_a_bin = f64::from(rate.hz()) / size as f64;
    responses.iter().all(|(taps, centre)| {
        let mut spectrum = vec![Complex::new(0.0, 0.0); size];
        for (into, tap) in taps.iter().enumerate() {
            spectrum[(into + size - centre % size) % size] = Complex::new(*tap, 0.0);
        }
        forward.process(&mut spectrum);
        (0..GROUPS).all(|group| {
            let mut heard = spectrum.clone();
            for (bin, value) in heard.iter_mut().enumerate() {
                *value *= heard_across(group, bin.min(size - bin) as f64 * hertz_a_bin);
            }
            inverse.process(&mut heard);
            let within = (HELD_TOGETHER_WITHIN_SECONDS[group] * f64::from(rate.hz())) as usize;
            let (mut apart, mut whole) = (0.0, 0.0);
            for (at, value) in heard.iter().enumerate() {
                let energy = value.norm_sqr();
                whole += energy;
                if at.min(size - at) > within {
                    apart += energy;
                }
            }
            whole <= 0.0 || (apart / whole).sqrt() <= COMPACT_WHILE_APART_AT_MOST
        })
    })
}

fn heard_across(group: usize, hertz: f64) -> f64 {
    let octaves = (hertz.max(1.0) / HEARD_AROUND_HZ[group]).log2() / HEARD_ACROSS_OCTAVES[group];
    (-0.5 * octaves * octaves).exp()
}

fn mirrored_after(channel: &mut Vec<f32>, ending: usize, size: usize) {
    for at in channel.len()..size {
        let mirrored = (2 * ending).checked_sub(at + 2);
        let sample = mirrored.and_then(|into| channel.get(into)).copied();
        channel.push(sample.unwrap_or(0.0));
    }
}

fn table_of(first: f32, second: f32) -> [f32; HALF_A_TABLE] {
    let halves = |height: f32| [0.0, height / 2.0, height];
    let firsts = halves(first);
    let seconds = halves(second);
    std::array::from_fn(|at| firsts[at / STEPS_OF_A_WEIGHT] + seconds[at % STEPS_OF_A_WEIGHT])
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use super::*;
    use crate::split::passed_below;

    const RATE: SampleRate = SampleRate::HZ_44100;

    const FADE_SECONDS: f64 = 0.5;

    fn index_of(whole: f64, groups: [f64; GROUPS]) -> usize {
        (0..WEIGHINGS)
            .find(|index| {
                let found = weighing(*index);
                found.whole == whole && found.groups == groups
            })
            .expect("a weighing")
    }

    fn tones(tones: &[(f64, f64, f64)]) -> Vec<f32> {
        (0..RATE.hz() as usize * 4)
            .map(|frame| {
                let seconds = frame as f64 / f64::from(RATE.hz());
                let faded = (seconds / FADE_SECONDS)
                    .min((4.0 - seconds) / FADE_SECONDS)
                    .min(1.0);
                tones
                    .iter()
                    .map(|(hertz, height, phase)| (TAU * hertz * seconds + phase).cos() * height)
                    .sum::<f64>() as f32
                    * faded as f32
            })
            .collect()
    }

    fn heard(samples: &[f32], piece: usize) -> [f64; WEIGHINGS] {
        let mut coinciding = Coinciding::new(RATE, 1);
        for part in samples.chunks(piece) {
            coinciding.note(part);
        }
        let peak = samples
            .iter()
            .fold(0.0_f32, |held, sample| held.max(sample.abs()));
        coinciding
            .finished(f64::from(peak))
            .expect("something heard")
    }

    #[test]
    fn every_weighing_is_a_different_one_and_none_of_them_weighs_nothing() {
        let mut seen: Vec<[u8; 4]> = (0..WEIGHINGS)
            .map(|index| {
                let found = weighing(index);
                [
                    found.whole,
                    found.groups[0],
                    found.groups[1],
                    found.groups[2],
                ]
                .map(|weight| (weight * 2.0) as u8)
            })
            .collect();

        assert!(
            seen.iter()
                .all(|weights| weights.iter().any(|weight| *weight > 0))
        );
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), WEIGHINGS);
    }

    #[test]
    fn a_groups_answer_is_the_bands_own_filters_and_the_groups_sum_to_the_signal() {
        for hertz in [30.0, 126.0, 500.0, 3_200.0, 9_000.0, 20_000.0] {
            let bank = |edge: usize| passed_below(edge, hertz, RATE);
            for group in 0..GROUPS {
                let wanted = bank(GROUP_EDGES[group + 1]) - bank(GROUP_EDGES[group]);
                let answer = group_passes(group, hertz, RATE);

                assert!(
                    (answer - wanted).abs() < 1e-3,
                    "{group} at {hertz}: {answer} {wanted}"
                );
            }
            let together: f64 = (0..GROUPS)
                .map(|group| group_passes(group, hertz, RATE))
                .sum();

            assert!((together - 1.0).abs() < 1e-9, "{together}");
        }
    }

    #[test]
    fn a_tone_in_one_group_is_held_at_its_height_whatever_its_phase() {
        let height = 0.5;
        for phase in [0.0, 0.7, 2.1] {
            let figures = heard(&tones(&[(60.0, height, phase)]), 4_096);
            let lone = figures[index_of(0.0, [1.0, 0.0, 0.0])];
            let elsewhere = figures[index_of(0.0, [0.0, 0.0, 1.0])];

            assert!((lone - height).abs() < height * 0.01, "{lone}");
            assert!(elsewhere < height * 0.001, "{elsewhere}");
        }
    }

    #[test]
    fn tones_heard_at_once_add_where_their_groups_are_weighed_together() {
        let figures = heard(
            &tones(&[(60.0, 0.5, 0.0), (800.0, 0.3, 0.0), (8_000.0, 0.1, 0.0)]),
            4_096,
        );

        let together = figures[index_of(0.0, [1.0, 1.0, 1.0])];
        let halved = figures[index_of(0.0, [0.5, 1.0, 0.5])];

        let high_as_read = 0.1 * f64::from(SHARPENED_BY_AT_MOST[2]);
        assert!((together - (0.8 + high_as_read)).abs() < 0.01, "{together}");
        assert!(
            (halved - (0.55 + high_as_read / 2.0)).abs() < 0.01,
            "{halved}"
        );
    }

    #[test]
    fn a_figure_never_reads_under_the_sample_it_was_weighed_at() {
        let samples = tones(&[(90.0, 0.4, 0.3), (1_500.0, 0.3, 1.1), (6_000.0, 0.2, 2.0)]);
        let figures = heard(&samples, 4_096);
        let peak = samples
            .iter()
            .fold(0.0_f32, |held, sample| held.max(sample.abs()));

        let whole = figures[index_of(1.0, [0.0, 0.0, 0.0])];

        assert!(whole >= f64::from(peak) - 1e-5, "{whole} {peak}");
        assert!(whole <= f64::from(peak) * 1.02, "{whole} {peak}");
        for (index, figure) in figures.iter().enumerate() {
            let least = weighing(index).whole * f64::from(peak);

            assert!(*figure >= least - 1e-5, "{index}");
        }
    }

    #[test]
    fn a_stream_noted_in_pieces_has_the_figures_it_has_noted_whole() {
        let samples = tones(&[(70.0, 0.4, 0.0), (2_000.0, 0.2, 0.5)]);

        let whole = heard(&samples, samples.len());
        let pieces = heard(&samples, 1_000);
        let odd = heard(&samples, 7_919);

        assert_eq!(whole, pieces);
        assert_eq!(whole, odd);
    }

    #[test]
    fn a_stream_shorter_than_a_hop_is_still_heard_and_silence_is_nothing() {
        let short: Vec<f32> = (0..300)
            .map(|frame| {
                (TAU * 1_000.0 * f64::from(frame) / f64::from(RATE.hz())).sin() as f32 * 0.5
            })
            .collect();
        let mut silent = Coinciding::new(RATE, 2);
        silent.note(&[0.0; 4_096]);

        let figures = heard(&short, 100);

        assert!(figures[index_of(1.0, [0.0, 0.0, 0.0])] > 0.4);
        assert_eq!(silent.finished(0.0), None);
    }
}
