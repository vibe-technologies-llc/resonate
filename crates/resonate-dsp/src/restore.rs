use std::{f64::consts::PI, sync::Arc};

use resonate_core::{StreamSpec, eq::Frequency};
use rustfft::{Fft, FftPlanner, num_complex::Complex};

use crate::{ProcessCount, Processor, Result};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Restoration {
    #[default]
    Off,
    Repair,
    Extend,
}

impl Restoration {
    pub const ALL: [Self; 3] = [Self::Off, Self::Repair, Self::Extend];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Repair => "repair",
            Self::Extend => "extend",
        }
    }

    pub fn named(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tuning {
    Mp3,
    Aac,
    Vorbis,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RestoreConfig {
    pub restoration: Restoration,
    pub tuning: Tuning,
    pub wall: Option<Frequency>,
}

struct Droop {
    decibels: f64,
    across_hz: f64,
}

struct Holes {
    deeper_than_db: f64,
    filled_below_db: f64,
}

impl Tuning {
    const fn plausible_walls_hz(self) -> (f64, f64) {
        match self {
            Self::Mp3 => (11_000.0, 20_500.0),
            Self::Aac | Self::Vorbis => (13_000.0, 21_000.0),
        }
    }

    const fn droop(self) -> Droop {
        match self {
            Self::Mp3 => Droop {
                decibels: 2.0,
                across_hz: 1_500.0,
            },
            Self::Aac => Droop {
                decibels: 1.0,
                across_hz: 1_000.0,
            },
            Self::Vorbis => Droop {
                decibels: 1.5,
                across_hz: 1_200.0,
            },
        }
    }

    const fn holes(self) -> Holes {
        match self {
            Self::Mp3 => Holes {
                deeper_than_db: 20.0,
                filled_below_db: 12.0,
            },
            Self::Aac => Holes {
                deeper_than_db: 24.0,
                filled_below_db: 18.0,
            },
            Self::Vorbis => Holes {
                deeper_than_db: 20.0,
                filled_below_db: 14.0,
            },
        }
    }
}

const FRAME_AT_48_KHZ: usize = 1_024;
const HOPS_A_FRAME: usize = 4;
const SYNTHESIS_GAIN: f64 = 2.0 / HOPS_A_FRAME as f64;
const LONG_TERM_SECONDS: f64 = 3.0;
const HOLE_MEMORY_FRAMES: usize = 9;
const LOOK_FOR_A_WALL_AFTER_SECONDS: f64 = 1.5;
const LOOK_FOR_A_WALL_EVERY_SECONDS: f64 = 0.5;
const BAND_HZ: f64 = 100.0;
const BELOW_A_WALL_HZ: f64 = 1_000.0;
const ABOVE_A_WALL_FROM_HZ: f64 = 200.0;
const WALL_DB: f64 = 30.0;
const EDGE_WITHIN_DB: f64 = 10.0;
const BELOW_A_WALL_WITHIN_DB: f64 = 50.0;
const REFERENCE_FROM_HZ: f64 = 2_000.0;
const REFERENCE_TO_HZ: f64 = 10_000.0;
const QUIETEST_REFERENCE_DB: f64 = -100.0;
const HIGHEST_FILLED_NYQUISTS: f64 = 0.97;
const HOLES_FROM_HZ: f64 = 4_000.0;
const HOLE_RUN_BINS: usize = 4;
const HOLES_ONLY_WHILE_THE_FRAME_HOLDS_DB: f64 = 6.0;
const EXTRA_ROLLOFF_DB_PER_OCTAVE: f64 = 12.0;
const UNDER_THE_EDGE_DB: f64 = 6.0;
const STEEPEST_SLOPE_DB_PER_OCTAVE: f64 = -24.0;
const PATCH_UNDER_ITS_SOURCE_DB: f64 = 3.0;
const CROSSFADE_HZ: f64 = 500.0;
const SLOPE_FROM_WALLS: f64 = 0.5;
const SLOPE_TO_WALLS: f64 = 0.95;
const EDGE_FROM_WALLS: f64 = 0.9;
const EDGE_TO_WALLS: f64 = 0.97;
const SILENT_POWER: f64 = 1e-30;

struct Voice {
    history: Vec<f64>,
    accumulated: Vec<f64>,
    heard: Vec<f64>,
    heard_at: usize,
}

impl Voice {
    fn steady(&self, bin: usize, bins: usize) -> f64 {
        let mut kept = [0.0; HOLE_MEMORY_FRAMES];
        for (frame, slot) in kept.iter_mut().enumerate() {
            *slot = self
                .heard
                .get(frame * bins + bin)
                .copied()
                .unwrap_or_default();
        }
        let (_, middle, _) = kept.select_nth_unstable_by(HOLE_MEMORY_FRAMES / 2, f64::total_cmp);
        *middle
    }
}

pub struct Restore {
    config: RestoreConfig,
    channels: usize,
    rate: f64,
    size: usize,
    hop: usize,
    window: Vec<f64>,
    forward: Option<Arc<dyn Fft<f64>>>,
    inverse: Option<Arc<dyn Fft<f64>>>,
    spectrum: Vec<Complex<f64>>,
    scratch: Vec<Complex<f64>>,
    voices: Vec<Voice>,
    long_term: Vec<f64>,
    bands: Vec<f64>,
    envelope: Vec<f64>,
    hollow: Vec<bool>,
    steady: Vec<f64>,
    filled: usize,
    hops: u64,
    wall_bin: Option<usize>,
    wall_locked: bool,
    noise: u64,
    taken: u64,
    emitted: u64,
    preroll: u64,
    long_term_step: f64,
    silence: Vec<f64>,
}

impl Restore {
    pub fn new(config: RestoreConfig) -> Self {
        Self {
            config,
            channels: 1,
            rate: 48_000.0,
            size: 0,
            hop: 0,
            window: Vec::new(),
            forward: None,
            inverse: None,
            spectrum: Vec::new(),
            scratch: Vec::new(),
            voices: Vec::new(),
            long_term: Vec::new(),
            bands: Vec::new(),
            envelope: Vec::new(),
            hollow: Vec::new(),
            steady: Vec::new(),
            filled: 0,
            hops: 0,
            wall_bin: None,
            wall_locked: false,
            noise: 0x9E37_79B9_7F4A_7C15,
            taken: 0,
            emitted: 0,
            preroll: 0,
            long_term_step: 1.0,
            silence: Vec::new(),
        }
    }

    pub fn wall(&self) -> Option<Frequency> {
        self.wall_bin
            .and_then(|bin| Frequency::from_hertz(self.hertz_of(bin)).ok())
    }

    fn hertz_of(&self, bin: usize) -> f64 {
        bin as f64 * self.rate / self.size as f64
    }

    fn bin_of(&self, hertz: f64) -> usize {
        (hertz * self.size as f64 / self.rate).round().max(0.0) as usize
    }

    fn highest_bin(&self) -> usize {
        self.bin_of(HIGHEST_FILLED_NYQUISTS * self.rate / 2.0)
            .min(self.size / 2)
    }

    fn draw(&mut self) -> f64 {
        self.noise ^= self.noise << 13;
        self.noise ^= self.noise >> 7;
        self.noise ^= self.noise << 17;
        (self.noise >> 11) as f64 / (1_u64 << 53) as f64
    }

    fn push(&mut self, frame: &[f64], output: &mut [f64], produced: &mut usize) {
        let at = self.size - self.hop + self.filled;
        for (voice, sample) in self.voices.iter_mut().zip(frame) {
            if let Some(slot) = voice.history.get_mut(at) {
                *slot = *sample;
            }
        }
        self.filled += 1;
        if self.filled == self.hop {
            self.run_a_hop(output, produced);
        }
    }

    fn run_a_hop(&mut self, output: &mut [f64], produced: &mut usize) {
        self.filled = 0;
        self.hops += 1;
        for channel in 0..self.channels {
            self.reshape(channel);
        }
        self.look_for_a_wall();
        self.shape_the_envelope();

        let (hop, channels) = (self.hop, self.channels);
        for offset in 0..hop {
            self.taken_out(offset, output, produced, channels);
        }
        for voice in &mut self.voices {
            voice.history.copy_within(hop.., 0);
            voice.accumulated.copy_within(hop.., 0);
            let size = voice.accumulated.len();
            if let Some(vacated) = voice.accumulated.get_mut(size - hop..) {
                vacated.fill(0.0);
            }
        }
    }

    fn taken_out(
        &mut self,
        offset: usize,
        output: &mut [f64],
        produced: &mut usize,
        channels: usize,
    ) {
        if self.preroll > 0 {
            self.preroll -= 1;
            return;
        }
        if self.emitted >= self.taken {
            return;
        }
        let Some(slot) = output.get_mut(*produced * channels..(*produced + 1) * channels) else {
            return;
        };
        for (sample, voice) in slot.iter_mut().zip(&self.voices) {
            *sample = voice.accumulated.get(offset).copied().unwrap_or_default();
        }
        *produced += 1;
        self.emitted += 1;
    }

    fn note(&mut self, channel: usize) {
        let slow = self.long_term_step;
        let share = (self.channels as f64).recip();
        let bins = self.long_term.len();
        let Some(voice) = self.voices.get_mut(channel) else {
            return;
        };
        let row = voice.heard_at * bins;
        let heard = voice.heard.get_mut(row..row + bins).unwrap_or_default();
        for ((bin, long), kept) in self
            .spectrum
            .iter()
            .zip(self.long_term.iter_mut())
            .zip(heard.iter_mut())
        {
            let power = bin.norm_sqr();
            *long += (power - *long) * slow * share;
            *kept = power;
        }
        voice.heard_at = (voice.heard_at + 1) % HOLE_MEMORY_FRAMES;
    }

    fn look_for_a_wall(&mut self) {
        if self.wall_locked || self.config.restoration == Restoration::Off {
            return;
        }
        let heard = self.hops as f64 * self.hop as f64 / self.rate;
        let every = (LOOK_FOR_A_WALL_EVERY_SECONDS * self.rate / self.hop as f64).max(1.0) as u64;
        if heard < LOOK_FOR_A_WALL_AFTER_SECONDS || !self.hops.is_multiple_of(every) {
            return;
        }

        let band_bins = self.bin_of(BAND_HZ).max(1);
        let top = self.highest_bin();
        self.bands.clear();
        let mut start = 0;
        while start + band_bins <= top {
            let power: f64 = self
                .long_term
                .get(start..start + band_bins)
                .unwrap_or_default()
                .iter()
                .sum::<f64>()
                / band_bins as f64;
            self.bands.push(decibels_of_power(power));
            start += band_bins;
        }

        let band_hz = self.hertz_of(band_bins);
        let band_of = |hertz: f64| (hertz / band_hz) as usize;
        let reference = self
            .bands
            .get(band_of(REFERENCE_FROM_HZ)..band_of(REFERENCE_TO_HZ).min(self.bands.len()))
            .unwrap_or_default()
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        if reference < QUIETEST_REFERENCE_DB {
            return;
        }

        let (lowest, highest) = self.config.tuning.plausible_walls_hz();
        let below = band_of(BELOW_A_WALL_HZ);
        let gap = band_of(ABOVE_A_WALL_FROM_HZ);
        let cliff = (band_of(lowest)..band_of(highest).min(self.bands.len()))
            .rev()
            .find(|&band| {
                let under = self
                    .bands
                    .get(band.saturating_sub(below)..band)
                    .unwrap_or_default();
                let over = self.bands.get(band + gap..).unwrap_or_default();
                if under.is_empty() || over.is_empty() {
                    return false;
                }
                let under_level = under.iter().sum::<f64>() / under.len() as f64;
                let over_level = over.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                under_level - over_level >= WALL_DB
                    && under_level >= reference - BELOW_A_WALL_WITHIN_DB
            });
        let Some(cliff) = cliff else {
            return;
        };
        let under = self
            .bands
            .get(cliff.saturating_sub(below)..cliff)
            .unwrap_or_default();
        let under_level = under.iter().sum::<f64>() / under.len().max(1) as f64;
        let last_loud = (cliff.saturating_sub(below)..=cliff)
            .rev()
            .find(|band| {
                self.bands
                    .get(*band)
                    .is_some_and(|level| *level >= under_level - EDGE_WITHIN_DB)
            })
            .unwrap_or(cliff);
        self.wall_bin = Some((last_loud + 1) * band_bins);
        self.wall_locked = true;
    }

    fn shape_the_envelope(&mut self) {
        self.envelope.fill(0.0);
        if self.config.restoration != Restoration::Extend {
            return;
        }
        let Some(wall) = self.wall_bin else {
            return;
        };
        let top = self.highest_bin();
        if wall >= top {
            return;
        }
        let width = top - wall;
        let Some(source_from) = wall.checked_sub(width) else {
            return;
        };

        let wall_hz = self.hertz_of(wall);
        let fit_from = self.bin_of(wall_hz * SLOPE_FROM_WALLS).max(1);
        let fit_to = self.bin_of(wall_hz * SLOPE_TO_WALLS);
        let (mut sx, mut sy, mut sxx, mut sxy, mut count) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for bin in fit_from..fit_to {
            let x = (self.hertz_of(bin) / wall_hz).log2();
            let y = decibels_of_power(self.long_term.get(bin).copied().unwrap_or_default());
            sx += x;
            sy += y;
            sxx += x * x;
            sxy += x * y;
            count += 1.0;
        }
        let spread = count * sxx - sx * sx;
        if count < 2.0 || spread.abs() < f64::EPSILON {
            return;
        }
        let slope = ((count * sxy - sx * sy) / spread).clamp(STEEPEST_SLOPE_DB_PER_OCTAVE, 0.0);

        let edge_from = self.bin_of(wall_hz * EDGE_FROM_WALLS);
        let edge_to = self.bin_of(wall_hz * EDGE_TO_WALLS).max(edge_from + 1);
        let edge_db = decibels_of_power(
            self.long_term
                .get(edge_from..edge_to)
                .unwrap_or_default()
                .iter()
                .sum::<f64>()
                / (edge_to - edge_from) as f64,
        ) - UNDER_THE_EDGE_DB;

        let crossfade = self.bin_of(CROSSFADE_HZ).max(1);
        for bin in wall..top {
            let source = source_from + (bin - wall);
            let hertz = self.hertz_of(bin);
            let wanted_db =
                edge_db + (slope - EXTRA_ROLLOFF_DB_PER_OCTAVE) * (hertz / wall_hz).log2();
            let source_db =
                decibels_of_power(self.long_term.get(source).copied().unwrap_or_default());
            let gain = gain_of(wanted_db - source_db).min(gain_of(-PATCH_UNDER_ITS_SOURCE_DB));
            let into = (bin - wall) as f64 / crossfade as f64;
            let faded = if into < 1.0 {
                0.5 - 0.5 * (PI * into).cos()
            } else {
                1.0
            };
            if let Some(slot) = self.envelope.get_mut(bin) {
                *slot = gain * faded;
            }
        }
    }

    fn reshape(&mut self, channel: usize) {
        let Some(voice) = self.voices.get(channel) else {
            return;
        };
        for ((bin, sample), weight) in self
            .spectrum
            .iter_mut()
            .zip(&voice.history)
            .zip(&self.window)
        {
            *bin = Complex::new(sample * weight, 0.0);
        }
        if let Some(forward) = self.forward.as_ref() {
            forward.process_with_scratch(&mut self.spectrum, &mut self.scratch);
        }

        let half = self.size / 2;
        let top = self.highest_bin();
        let wall = self.wall_bin;
        let tuning = self.config.tuning;
        let holes = tuning.holes();
        self.find_the_holes(channel, wall.unwrap_or(top).min(top), &holes);
        self.note(channel);
        if let Some(wall) = wall {
            self.lift_the_droop(wall, &tuning.droop());
        }
        self.fill_the_holes(&holes);
        if let Some(wall) = wall {
            self.patch_above(wall, top);
        }

        for bin in 1..half {
            let mirrored = self.spectrum.get(bin).copied().unwrap_or_default().conj();
            if let Some(slot) = self.spectrum.get_mut(self.size - bin) {
                *slot = mirrored;
            }
        }
        if let Some(inverse) = self.inverse.as_ref() {
            inverse.process_with_scratch(&mut self.spectrum, &mut self.scratch);
        }
        let unscaled = SYNTHESIS_GAIN / self.size as f64;
        let Some(voice) = self.voices.get_mut(channel) else {
            return;
        };
        for ((held, bin), weight) in voice
            .accumulated
            .iter_mut()
            .zip(&self.spectrum)
            .zip(&self.window)
        {
            *held += bin.re * weight * unscaled;
        }
    }

    fn lift_the_droop(&mut self, wall: usize, droop: &Droop) {
        let wall_hz = self.hertz_of(wall);
        let from = self.bin_of(wall_hz - droop.across_hz);
        for bin in from..wall {
            let across = (self.hertz_of(bin) - (wall_hz - droop.across_hz)) / droop.across_hz;
            let lift = gain_of(droop.decibels * across * across);
            if let Some(slot) = self.spectrum.get_mut(bin) {
                *slot *= lift;
            }
        }
    }

    fn find_the_holes(&mut self, channel: usize, below: usize, holes: &Holes) {
        self.hollow.fill(false);
        let from = self.bin_of(HOLES_FROM_HZ);
        let deeper = power_of(-holes.deeper_than_db);
        let Some(voice) = self.voices.get(channel) else {
            return;
        };
        let bins = self.long_term.len();
        for bin in from..below {
            if let Some(slot) = self.steady.get_mut(bin) {
                *slot = voice.steady(bin, bins);
            }
        }
        let heard: f64 = self
            .spectrum
            .get(from..below)
            .unwrap_or_default()
            .iter()
            .map(|bin| bin.norm_sqr())
            .sum();
        let expected: f64 = self
            .steady
            .get(from..below)
            .unwrap_or_default()
            .iter()
            .sum();
        if heard < expected * power_of(-HOLES_ONLY_WHILE_THE_FRAME_HOLDS_DB) {
            return;
        }
        let mut run_from = None;
        for bin in from..=below {
            let hollow = bin < below && {
                let now = self.spectrum.get(bin).map_or(0.0, |bin| bin.norm_sqr());
                let steady = self.steady.get(bin).copied().unwrap_or_default();
                steady > SILENT_POWER && now < steady * deeper
            };
            match (hollow, run_from) {
                (true, None) => run_from = Some(bin),
                (false, Some(start)) => {
                    run_from = None;
                    if bin - start >= HOLE_RUN_BINS
                        && let Some(run) = self.hollow.get_mut(start..bin)
                    {
                        run.fill(true);
                    }
                }
                _ => {}
            }
        }
    }

    fn fill_the_holes(&mut self, holes: &Holes) {
        let filled = gain_of(-holes.filled_below_db);
        for bin in 0..self.hollow.len() {
            if self.hollow.get(bin).copied().unwrap_or_default() {
                self.fill(bin, filled);
            }
        }
    }

    fn fill(&mut self, bin: usize, filled: f64) {
        let steady = self.steady.get(bin).copied().unwrap_or_default();
        let angle = 2.0 * PI * self.draw();
        if let Some(slot) = self.spectrum.get_mut(bin) {
            *slot = Complex::from_polar(steady.sqrt() * filled, angle);
        }
    }

    fn patch_above(&mut self, wall: usize, top: usize) {
        if wall >= top {
            return;
        }
        let width = top - wall;
        let Some(source_from) = wall.checked_sub(width) else {
            return;
        };
        for bin in wall..top {
            let gain = self.envelope.get(bin).copied().unwrap_or_default();
            if gain == 0.0 {
                continue;
            }
            let patch = self
                .spectrum
                .get(source_from + (bin - wall))
                .copied()
                .unwrap_or_default();
            if let Some(slot) = self.spectrum.get_mut(bin) {
                *slot += patch * gain;
            }
        }
    }
}

fn decibels_of_power(power: f64) -> f64 {
    10.0 * power.max(SILENT_POWER).log10()
}

fn gain_of(decibels: f64) -> f64 {
    10_f64.powf(decibels / 20.0)
}

fn power_of(decibels: f64) -> f64 {
    10_f64.powf(decibels / 10.0)
}

impl Processor for Restore {
    fn prepare(&mut self, spec: StreamSpec, max_frames_in: usize) -> Result<usize> {
        let channels = usize::from(spec.channel_count().get());
        let rate = f64::from(spec.rate.hz());
        let size = ((FRAME_AT_48_KHZ as f64 * rate / 48_000.0).round() as usize)
            .next_power_of_two()
            .max(HOPS_A_FRAME * 2);
        let hop = size / HOPS_A_FRAME;

        let mut planner = FftPlanner::<f64>::new();
        let forward = planner.plan_fft_forward(size);
        let inverse = planner.plan_fft_inverse(size);
        let scratch = forward
            .get_inplace_scratch_len()
            .max(inverse.get_inplace_scratch_len());

        self.channels = channels;
        self.rate = rate;
        self.size = size;
        self.hop = hop;
        self.window = (0..size)
            .map(|n| (PI * n as f64 / size as f64).sin())
            .collect();
        self.forward = Some(forward);
        self.inverse = Some(inverse);
        self.spectrum = vec![Complex::new(0.0, 0.0); size];
        self.scratch = vec![Complex::new(0.0, 0.0); scratch];
        self.voices = (0..channels)
            .map(|_| Voice {
                history: vec![0.0; size],
                accumulated: vec![0.0; size],
                heard: vec![0.0; HOLE_MEMORY_FRAMES * (size / 2 + 1)],
                heard_at: 0,
            })
            .collect();
        self.long_term = vec![0.0; size / 2 + 1];
        self.bands = Vec::with_capacity(size / 2 + 1);
        self.envelope = vec![0.0; size / 2 + 1];
        self.hollow = vec![false; size / 2 + 1];
        self.steady = vec![0.0; size / 2 + 1];
        self.silence = vec![0.0; channels];
        let hops_a_second = rate / hop as f64;
        self.long_term_step = 1.0 - (-1.0 / (LONG_TERM_SECONDS * hops_a_second)).exp();
        self.reset();
        Ok(self.max_output_frames(max_frames_in))
    }

    fn max_output_frames(&self, frames_in: usize) -> usize {
        frames_in + self.hop
    }

    fn max_flush_frames(&self) -> usize {
        self.size
    }

    fn reset(&mut self) {
        for voice in &mut self.voices {
            voice.history.fill(0.0);
            voice.accumulated.fill(0.0);
            voice.heard.fill(0.0);
            voice.heard_at = 0;
        }
        self.long_term.fill(0.0);
        self.envelope.fill(0.0);
        self.filled = 0;
        self.hops = 0;
        self.wall_bin = self.config.wall.and_then(|wall| {
            let hertz = f64::from(wall.centihertz()) / 100.0;
            let (lowest, highest) = self.config.tuning.plausible_walls_hz();
            (lowest..=highest)
                .contains(&hertz)
                .then(|| self.bin_of(hertz))
        });
        self.wall_locked = self.wall_bin.is_some();
        self.taken = 0;
        self.emitted = 0;
        self.preroll = (self.size - self.hop) as u64;
    }

    fn latency_frames(&self) -> f64 {
        (self.size - self.hop) as f64
    }

    fn is_transparent(&self) -> bool {
        self.config.restoration == Restoration::Off
    }

    fn process(&mut self, input: &[f64], output: &mut [f64]) -> ProcessCount {
        let channels = self.channels;
        let mut produced = 0;
        let mut taken = 0;
        for frame in input.chunks_exact(channels) {
            self.taken += 1;
            self.push(frame, output, &mut produced);
            taken += 1;
        }
        ProcessCount {
            frames_in: taken,
            frames_out: produced,
        }
    }

    fn flush(&mut self, output: &mut [f64]) -> usize {
        let silence = std::mem::take(&mut self.silence);
        let mut produced = 0;
        let mut guard = self.size * 2;
        while self.emitted < self.taken && guard > 0 {
            self.push(&silence, output, &mut produced);
            guard -= 1;
        }
        self.silence = silence;
        produced
    }
}

#[cfg(test)]
mod tests {
    use resonate_core::{ChannelLayout, SampleFormat, SampleRate};

    use super::*;

    const RATE: f64 = 44_100.0;
    const BLOCK: usize = 1_024;
    const MEASURED: usize = 8_192;
    const DROPOUT_FRAMES: usize = 2_048;
    const FILLED_FRAMES: usize = 1_024;

    fn restored(restoration: Restoration) -> RestoreConfig {
        RestoreConfig {
            restoration,
            tuning: Tuning::Mp3,
            wall: None,
        }
    }

    fn tones(frames: usize, lowest: f64, highest: f64, count: usize) -> Vec<f64> {
        let mut seed: u64 = 0x5265_736F_6E61_7465;
        let mut draw = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1_u64 << 53) as f64
        };
        let voices: Vec<(f64, f64)> = (0..count)
            .map(|_| (lowest + draw() * (highest - lowest), draw() * 2.0 * PI))
            .collect();
        let level = 0.5 / (count as f64).sqrt();
        (0..frames)
            .flat_map(|n| {
                let t = n as f64 / RATE;
                let sample: f64 = voices
                    .iter()
                    .map(|(hz, phase)| (2.0 * PI * hz * t + phase).sin())
                    .sum::<f64>()
                    * level;
                [sample, sample]
            })
            .collect()
    }

    fn through(config: RestoreConfig, input: &[f64]) -> (Vec<f64>, Restore) {
        let mut stage = Restore::new(config);
        let most = stage
            .prepare(
                StreamSpec::new(
                    SampleRate::HZ_44100,
                    ChannelLayout::Stereo,
                    SampleFormat::F32,
                ),
                BLOCK,
            )
            .expect("the stage prepares");
        let mut scratch = vec![0.0; most * 2];
        let mut output = Vec::new();
        for block in input.chunks(BLOCK * 2) {
            let count = stage.process(block, &mut scratch);
            assert_eq!(count.frames_in, block.len() / 2, "a frame was left untaken");
            output.extend_from_slice(&scratch[..count.frames_out * 2]);
        }
        let mut tail = vec![0.0; stage.max_flush_frames() * 2];
        let drained = stage.flush(&mut tail);
        output.extend_from_slice(&tail[..drained * 2]);
        (output, stage)
    }

    fn band_db(samples: &[f64], from_frame: usize, from_hz: f64, to_hz: f64, frames: usize) -> f64 {
        let mut planner = FftPlanner::<f64>::new();
        let fft = planner.plan_fft_forward(frames);
        let mut spectrum: Vec<Complex<f64>> = (0..frames)
            .map(|n| {
                let left = samples
                    .get((from_frame + n) * 2)
                    .copied()
                    .unwrap_or_default();
                let hann = 0.5 - 0.5 * (2.0 * PI * n as f64 / frames as f64).cos();
                Complex::new(left * hann, 0.0)
            })
            .collect();
        fft.process(&mut spectrum);
        let bin = |hz: f64| (hz * frames as f64 / RATE) as usize;
        let power: f64 = spectrum[bin(from_hz)..bin(to_hz)]
            .iter()
            .map(|bin| bin.norm_sqr())
            .sum();
        decibels_of_power(power / (bin(to_hz) - bin(from_hz)) as f64)
    }

    #[test]
    fn with_nothing_to_restore_the_stage_hands_back_what_it_took_exactly_and_whole() {
        let input = tones(3 * 44_100, 100.0, 21_900.0, 300);
        let (output, stage) = through(restored(Restoration::Extend), &input);
        assert_eq!(
            output.len(),
            input.len(),
            "the stage changed the stream's length"
        );
        assert_eq!(stage.wall(), None, "a full-band stream was read as cut");
        let worst = output
            .iter()
            .zip(&input)
            .map(|(made, taken)| (made - taken).abs())
            .fold(0.0, f64::max);
        assert!(worst < 1e-12, "the overlap-add strayed by {worst:e}");
    }

    #[test]
    fn a_wall_is_found_where_an_encoder_cut() {
        let input = tones(3 * 44_100, 100.0, 16_000.0, 300);
        let (_, stage) = through(restored(Restoration::Repair), &input);
        let wall = f64::from(stage.wall().expect("a wall").centihertz()) / 100.0;
        assert!(
            (15_900.0..=16_300.0).contains(&wall),
            "the wall was read at {wall} Hz"
        );
    }

    #[test]
    fn a_studied_wall_is_taken_from_the_start_and_an_implausible_one_is_not() {
        let studied = |hertz: f64| RestoreConfig {
            wall: Frequency::from_hertz(hertz).ok(),
            ..restored(Restoration::Extend)
        };
        let mut stage = Restore::new(studied(16_000.0));
        stage
            .prepare(
                StreamSpec::new(
                    SampleRate::HZ_44100,
                    ChannelLayout::Stereo,
                    SampleFormat::F32,
                ),
                BLOCK,
            )
            .expect("the stage prepares");
        assert!(stage.wall().is_some());

        let mut implausible = Restore::new(studied(6_000.0));
        implausible
            .prepare(
                StreamSpec::new(
                    SampleRate::HZ_44100,
                    ChannelLayout::Stereo,
                    SampleFormat::F32,
                ),
                BLOCK,
            )
            .expect("the stage prepares");
        assert_eq!(implausible.wall(), None);
    }

    #[test]
    fn extending_fills_the_band_above_the_wall_under_the_band_below_it() {
        let input = tones(4 * 44_100, 100.0, 16_000.0, 300);
        let late = 3 * 44_100;
        let (repaired, _) = through(restored(Restoration::Repair), &input);
        let (extended, _) = through(restored(Restoration::Extend), &input);

        let below = band_db(&extended, late, 13_000.0, 15_500.0, MEASURED);
        let above_repaired = band_db(&repaired, late, 17_000.0, 20_000.0, MEASURED) - below;
        let above_extended = band_db(&extended, late, 17_000.0, 20_000.0, MEASURED) - below;
        assert!(
            above_repaired < -60.0,
            "repairing alone put {above_repaired:.1} dB above the wall"
        );
        assert!(
            (-45.0..=-3.0).contains(&above_extended),
            "the extension sits at {above_extended:.1} dB against the band below the wall"
        );
    }

    #[test]
    fn a_transient_is_not_taken_for_a_hole_in_what_follows_it() {
        let mut input = tones(3 * 44_100, 100.0, 21_900.0, 200);
        let struck = 2 * 44_100;
        for frame in struck..struck + 3 {
            input[frame * 2] += 0.8;
            input[frame * 2 + 1] += 0.8;
        }
        let (output, _) = through(restored(Restoration::Repair), &input);
        let worst = output
            .iter()
            .zip(&input)
            .map(|(made, taken)| (made - taken).abs())
            .fold(0.0, f64::max);
        assert!(
            worst < 1e-12,
            "the strike was followed by {worst:e} of fill"
        );
    }

    #[test]
    fn a_band_an_encoder_zeroed_is_filled_and_a_steady_band_is_left_alone() {
        let frames = 3 * 44_100;
        let mut input = tones(frames, 1_000.0, 5_500.0, 120);
        let band = tones(frames, 8_000.0, 9_000.0, 40);
        let gap = 2 * 44_100..2 * 44_100 + DROPOUT_FRAMES;
        for (frame, pair) in input.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            if !gap.contains(&frame) {
                pair[0] += band[frame * 2];
                pair[1] += band[frame * 2 + 1];
            }
        }
        let (output, _) = through(restored(Restoration::Repair), &input);

        let before = band_db(
            &output,
            gap.start - MEASURED - 2_048,
            8_100.0,
            8_900.0,
            MEASURED,
        );
        let taken = band_db(&input, gap.start, 8_100.0, 8_900.0, FILLED_FRAMES) - before;
        let heard = band_db(&output, gap.start, 8_100.0, 8_900.0, FILLED_FRAMES) - before;
        assert!(taken < -60.0, "the gap in the input reads {taken:.1} dB");
        assert!(
            heard > -40.0,
            "the gap was left at {heard:.1} dB rather than filled"
        );
    }
}
