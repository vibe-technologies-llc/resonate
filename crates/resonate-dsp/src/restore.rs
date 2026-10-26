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
const SLICES_REMEMBERED: usize = HOPS_A_FRAME + 1;
const LONG_TERM_SECONDS: f64 = 3.0;
const HOLE_MEMORY_SLICES: usize = 17;
const LOOK_FOR_A_WALL_AFTER_SECONDS: f64 = 0.25;
const LOOK_FOR_A_WALL_EVERY_SECONDS: f64 = 0.25;
const WALL_LOCKS_AFTER_SECONDS: f64 = 1.5;
const WALL_MOVES_PAST_HZ: f64 = 300.0;
const RESTORATION_FADES_OVER_SECONDS: f64 = 0.25;
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
const HOLE_RUN_HZ: f64 = 450.0;
const A_RUN_DEEPER_THAN_ITS_BINS_DB: f64 = 6.0;
const HOLES_WITHIN_DB_OF_THE_LOUDEST: f64 = 60.0;
const HOLES_ONLY_WHILE_THE_SLICE_HOLDS_DB: f64 = 6.0;
const FILL_FADES_OVER_SLICES: usize = 4;
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
    steady: Vec<f64>,
    hollow: Vec<bool>,
    hollow_at: usize,
    holds_a_hole: [bool; SLICES_REMEMBERED],
}

impl Voice {
    fn steady_of(&self, bin: usize, bins: usize) -> f64 {
        let mut kept = [0.0; HOLE_MEMORY_SLICES];
        for (slice, slot) in kept.iter_mut().enumerate() {
            *slot = self
                .heard
                .get(slice * bins + bin)
                .copied()
                .unwrap_or_default();
        }
        let (_, middle, _) = kept.select_nth_unstable_by(HOLE_MEMORY_SLICES / 2, f64::total_cmp);
        *middle
    }

    fn row_aged(&self, age: usize) -> usize {
        (self.hollow_at + SLICES_REMEMBERED - age % SLICES_REMEMBERED) % SLICES_REMEMBERED
    }

    fn holds_a_hole_aged(&self, age: usize) -> bool {
        self.holds_a_hole
            .get(self.row_aged(age))
            .copied()
            .unwrap_or_default()
    }

    fn clear(&mut self) {
        self.history.fill(0.0);
        self.accumulated.fill(0.0);
        self.heard.fill(0.0);
        self.heard_at = 0;
        self.steady.fill(0.0);
        self.hollow.fill(false);
        self.hollow_at = 0;
        self.holds_a_hole = [false; SLICES_REMEMBERED];
    }
}

struct Slicing {
    forward: Arc<dyn Fft<f64>>,
    window: Vec<f64>,
    spectrum: Vec<Complex<f64>>,
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
    slicing: Option<Slicing>,
    spectrum: Vec<Complex<f64>>,
    fill: Vec<Complex<f64>>,
    scratch: Vec<Complex<f64>>,
    voices: Vec<Voice>,
    long_term: Vec<f64>,
    heard_this_hop: Vec<f64>,
    bands: Vec<f64>,
    envelope: Vec<f64>,
    filled: usize,
    hops: u64,
    wall_bin: Option<usize>,
    moving_to: Option<usize>,
    wall_locked: bool,
    presence: f64,
    presence_step: f64,
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
            slicing: None,
            spectrum: Vec::new(),
            fill: Vec::new(),
            scratch: Vec::new(),
            voices: Vec::new(),
            long_term: Vec::new(),
            heard_this_hop: Vec::new(),
            bands: Vec::new(),
            envelope: Vec::new(),
            filled: 0,
            hops: 0,
            wall_bin: None,
            moving_to: None,
            wall_locked: false,
            presence: 0.0,
            presence_step: 1.0,
            noise: 0x9E37_79B9_7F4A_7C15,
            taken: 0,
            emitted: 0,
            preroll: 0,
            long_term_step: 1.0,
            silence: Vec::new(),
        }
    }

    pub fn wall(&self) -> Option<Frequency> {
        self.moving_to
            .or(self.wall_bin)
            .and_then(|bin| Frequency::from_hertz(self.hertz_of(bin)).ok())
    }

    fn hertz_of(&self, bin: usize) -> f64 {
        bin as f64 * self.rate / self.size as f64
    }

    fn bin_of(&self, hertz: f64) -> usize {
        (hertz * self.size as f64 / self.rate).round().max(0.0) as usize
    }

    fn slice_bin_of(&self, hertz: f64) -> usize {
        (hertz * self.hop as f64 / self.rate).round().max(0.0) as usize
    }

    fn slice_bins(&self) -> usize {
        self.hop / 2 + 1
    }

    fn highest_bin(&self) -> usize {
        self.bin_of(HIGHEST_FILLED_NYQUISTS * self.rate / 2.0)
            .min(self.size / 2)
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
        self.heard_this_hop.fill(0.0);
        for channel in 0..self.channels {
            self.look_for_holes(channel);
            self.reshape(channel);
            self.fill_the_holes(channel);
        }
        self.note();
        self.look_for_a_wall();
        self.walk_the_presence();
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

    fn note(&mut self) {
        let step = self.long_term_step.max((self.hops.max(1) as f64).recip());
        let share = (self.channels as f64).recip();
        for (long, heard) in self.long_term.iter_mut().zip(&self.heard_this_hop) {
            *long += (heard * share - *long) * step;
        }
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
        let Some(found) = self.wall_in_what_was_heard() else {
            return;
        };
        let held = self.moving_to.or(self.wall_bin);
        let moves = held.is_none_or(|held| {
            (self.hertz_of(held) - self.hertz_of(found)).abs() > WALL_MOVES_PAST_HZ
        });
        if moves {
            match self.wall_bin {
                None => self.wall_bin = Some(found),
                Some(_) => self.moving_to = Some(found),
            }
        }
        self.wall_locked = heard >= WALL_LOCKS_AFTER_SECONDS;
    }

    fn wall_in_what_was_heard(&mut self) -> Option<usize> {
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
            return None;
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
            })?;
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
        Some((last_loud + 1) * band_bins)
    }

    fn walk_the_presence(&mut self) {
        let wanted = if self.wall_bin.is_some() && self.moving_to.is_none() {
            1.0
        } else {
            0.0
        };
        self.presence = if self.presence < wanted {
            (self.presence + self.presence_step).min(wanted)
        } else {
            (self.presence - self.presence_step).max(wanted)
        };
        if self.presence <= 0.0
            && let Some(moved) = self.moving_to.take()
        {
            self.wall_bin = Some(moved);
        }
    }

    fn shape_the_envelope(&mut self) {
        self.envelope.fill(0.0);
        if self.config.restoration != Restoration::Extend || self.presence <= 0.0 {
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
                *slot = gain * faded * self.presence;
            }
        }
    }

    fn holes_below(&self) -> usize {
        let top = self.highest_bin();
        self.slice_bin_of(self.hertz_of(self.wall_bin.unwrap_or(top).min(top)))
            .min(self.slice_bins())
    }

    fn look_for_holes(&mut self, channel: usize) {
        let from = self.slice_bin_of(HOLES_FROM_HZ);
        let below = self.holes_below();
        let bins = self.slice_bins();
        let run = (HOLE_RUN_HZ * self.hop as f64 / self.rate).ceil().max(1.0) as usize;
        let deeper = power_of(-self.config.tuning.holes().deeper_than_db);
        let deepest =
            power_of(-self.config.tuning.holes().deeper_than_db - A_RUN_DEEPER_THAN_ITS_BINS_DB);
        let (size, hop) = (self.size, self.hop);
        let (Some(slicing), Some(voice)) = (self.slicing.as_mut(), self.voices.get_mut(channel))
        else {
            return;
        };

        let newest = voice.history.get(size - hop..).unwrap_or_default();
        for ((bin, sample), weight) in slicing.spectrum.iter_mut().zip(newest).zip(&slicing.window)
        {
            *bin = Complex::new(sample * weight, 0.0);
        }
        slicing
            .forward
            .process_with_scratch(&mut slicing.spectrum, &mut self.scratch);

        for bin in from..below {
            let steady = voice.steady_of(bin, bins);
            if let Some(slot) = voice.steady.get_mut(bin) {
                *slot = steady;
            }
        }
        voice.hollow_at = (voice.hollow_at + 1) % SLICES_REMEMBERED;
        let row = voice.hollow_at * bins;
        if let Some(cleared) = voice.hollow.get_mut(row..row + bins) {
            cleared.fill(false);
        }
        let mut holds_a_hole = false;
        let heard: f64 = slicing
            .spectrum
            .get(from..below)
            .unwrap_or_default()
            .iter()
            .map(|bin| bin.norm_sqr())
            .sum();
        let steady = voice.steady.get(from..below).unwrap_or_default();
        let expected: f64 = steady.iter().sum();
        let audible =
            steady.iter().copied().fold(0.0, f64::max) * power_of(-HOLES_WITHIN_DB_OF_THE_LOUDEST);
        if heard >= expected * power_of(-HOLES_ONLY_WHILE_THE_SLICE_HOLDS_DB) {
            let mut run_from = None;
            for bin in from..=below {
                let hollow = bin < below && {
                    let now = slicing.spectrum.get(bin).map_or(0.0, |bin| bin.norm_sqr());
                    let steady = voice.steady.get(bin).copied().unwrap_or_default();
                    steady > SILENT_POWER.max(audible) && now < steady * deeper
                };
                match (hollow, run_from) {
                    (true, None) => run_from = Some(bin),
                    (false, Some(start)) => {
                        run_from = None;
                        let now: f64 = slicing
                            .spectrum
                            .get(start..bin)
                            .unwrap_or_default()
                            .iter()
                            .map(|bin| bin.norm_sqr())
                            .sum();
                        let steady: f64 = voice
                            .steady
                            .get(start..bin)
                            .unwrap_or_default()
                            .iter()
                            .sum();
                        let widened = start.saturating_sub(1).max(from)..(bin + 1).min(below);
                        if bin - start >= run
                            && now < steady * deepest
                            && let Some(marked) =
                                voice.hollow.get_mut(row + widened.start..row + widened.end)
                        {
                            marked.fill(true);
                            holds_a_hole = true;
                        }
                    }
                    _ => {}
                }
            }
        }
        if let Some(held) = voice.holds_a_hole.get_mut(voice.hollow_at) {
            *held = holds_a_hole;
        }

        let kept = voice.heard_at * bins;
        for (slot, bin) in voice
            .heard
            .get_mut(kept..kept + bins)
            .unwrap_or_default()
            .iter_mut()
            .zip(&slicing.spectrum)
        {
            *slot = bin.norm_sqr();
        }
        voice.heard_at = (voice.heard_at + 1) % HOLE_MEMORY_SLICES;
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
        for (heard, bin) in self.heard_this_hop.iter_mut().zip(&self.spectrum) {
            *heard += bin.norm_sqr();
        }

        let top = self.highest_bin();
        if let Some(wall) = self.wall_bin {
            self.lift_the_droop(wall, &self.config.tuning.droop());
            self.patch_above(wall, top);
        }
        mirror(&mut self.spectrum);
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
            let lift = gain_of(droop.decibels * across * across * self.presence);
            if let Some(slot) = self.spectrum.get_mut(bin) {
                *slot *= lift;
            }
        }
    }

    fn fill_the_holes(&mut self, channel: usize) {
        let Some(voice) = self.voices.get(channel) else {
            return;
        };
        if !(0..HOPS_A_FRAME).any(|age| voice.holds_a_hole_aged(age)) {
            return;
        }
        for slice in 0..HOPS_A_FRAME {
            self.fill_a_slice(channel, slice);
        }
    }

    fn fill_a_slice(&mut self, channel: usize, slice: usize) {
        let filled = gain_of(-self.config.tuning.holes().filled_below_db);
        let bins = self.slice_bins();
        let (size, hop) = (self.size, self.hop);
        let half = size / 2;
        let widened = (size / hop) as f64;
        let Some(voice) = self.voices.get_mut(channel) else {
            return;
        };
        let age = HOPS_A_FRAME - 1 - slice;
        if !voice.holds_a_hole_aged(age) {
            return;
        }
        let row_index = voice.row_aged(age);
        let reaching = hop / 2;
        let lead = if voice.holds_a_hole_aged(age + 1) {
            0
        } else {
            reaching
        };
        let trail = if age > 0 && voice.holds_a_hole_aged(age - 1) {
            0
        } else {
            reaching
        };
        let row = voice
            .hollow
            .get(row_index * bins..(row_index + 1) * bins)
            .unwrap_or_default();

        self.fill.fill(Complex::new(0.0, 0.0));
        for (slice_bin, _) in row.iter().enumerate().filter(|(_, hollow)| **hollow) {
            let level = (voice.steady.get(slice_bin).copied().unwrap_or_default() * widened).sqrt()
                * filled;
            let from = (slice_bin * HOPS_A_FRAME)
                .saturating_sub(HOPS_A_FRAME / 2)
                .max(1);
            let to = (slice_bin * HOPS_A_FRAME + HOPS_A_FRAME / 2).min(half);
            for bin in from..to {
                let angle = 2.0 * PI * drawn(&mut self.noise);
                if let Some(slot) = self.fill.get_mut(bin) {
                    *slot = Complex::from_polar(level, angle);
                }
            }
        }
        mirror(&mut self.fill);
        if let Some(inverse) = self.inverse.as_ref() {
            inverse.process_with_scratch(&mut self.fill, &mut self.scratch);
        }

        let unscaled = SYNTHESIS_GAIN / size as f64;
        let ramp = (hop / FILL_FADES_OVER_SLICES).max(1);
        let starts = (slice * hop) as f64 - lead as f64;
        let ends = ((slice + 1) * hop + trail) as f64;
        let first = (slice * hop).saturating_sub(lead + ramp / 2);
        let last = ((slice + 1) * hop + trail + ramp / 2).min(size);
        let half_a_ramp = (ramp / 2) as f64;
        for at in first..last {
            let rising = (at as f64 + half_a_ramp + 0.5 - starts) / ramp as f64;
            let falling = (ends + half_a_ramp - at as f64 - 0.5) / ramp as f64;
            let masked = rising.clamp(0.0, 1.0) * falling.clamp(0.0, 1.0);
            let weight = self.window.get(at).copied().unwrap_or_default();
            let noise = self.fill.get(at).map_or(0.0, |bin| bin.re);
            if let Some(held) = voice.accumulated.get_mut(at) {
                *held += noise * weight * masked * unscaled;
            }
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

fn mirror(spectrum: &mut [Complex<f64>]) {
    let size = spectrum.len();
    for bin in 1..size / 2 {
        let mirrored = spectrum.get(bin).copied().unwrap_or_default().conj();
        if let Some(slot) = spectrum.get_mut(size - bin) {
            *slot = mirrored;
        }
    }
}

fn drawn(noise: &mut u64) -> f64 {
    *noise ^= *noise << 13;
    *noise ^= *noise >> 7;
    *noise ^= *noise << 17;
    (*noise >> 11) as f64 / (1_u64 << 53) as f64
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

fn sine_window(size: usize) -> Vec<f64> {
    (0..size)
        .map(|n| (PI * n as f64 / size as f64).sin())
        .collect()
}

fn hann_window(size: usize) -> Vec<f64> {
    (0..size)
        .map(|n| 0.5 - 0.5 * (2.0 * PI * n as f64 / size as f64).cos())
        .collect()
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
        let slice_forward = planner.plan_fft_forward(hop);
        let scratch = forward
            .get_inplace_scratch_len()
            .max(inverse.get_inplace_scratch_len())
            .max(slice_forward.get_inplace_scratch_len());
        let bins = size / 2 + 1;
        let slice_bins = hop / 2 + 1;

        self.channels = channels;
        self.rate = rate;
        self.size = size;
        self.hop = hop;
        self.window = sine_window(size);
        self.forward = Some(forward);
        self.inverse = Some(inverse);
        self.slicing = Some(Slicing {
            forward: slice_forward,
            window: hann_window(hop),
            spectrum: vec![Complex::new(0.0, 0.0); hop],
        });
        self.spectrum = vec![Complex::new(0.0, 0.0); size];
        self.fill = vec![Complex::new(0.0, 0.0); size];
        self.scratch = vec![Complex::new(0.0, 0.0); scratch];
        self.voices = (0..channels)
            .map(|_| Voice {
                history: vec![0.0; size],
                accumulated: vec![0.0; size],
                heard: vec![0.0; HOLE_MEMORY_SLICES * slice_bins],
                heard_at: 0,
                steady: vec![0.0; slice_bins],
                hollow: vec![false; SLICES_REMEMBERED * slice_bins],
                hollow_at: 0,
                holds_a_hole: [false; SLICES_REMEMBERED],
            })
            .collect();
        self.long_term = vec![0.0; bins];
        self.heard_this_hop = vec![0.0; bins];
        self.bands = Vec::with_capacity(bins);
        self.envelope = vec![0.0; bins];
        self.silence = vec![0.0; channels];
        let hops_a_second = rate / hop as f64;
        self.long_term_step = 1.0 - (-1.0 / (LONG_TERM_SECONDS * hops_a_second)).exp();
        self.presence_step = (RESTORATION_FADES_OVER_SECONDS * hops_a_second)
            .max(1.0)
            .recip();
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
            voice.clear();
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
        self.moving_to = None;
        self.wall_locked = self.wall_bin.is_some();
        self.presence = if self.wall_locked { 1.0 } else { 0.0 };
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
    const GRANULE_FRAMES: usize = 576;
    const SHORT_MEASURED: usize = 512;
    const FILL_SPILLS_AT_MOST: usize = 160;
    const STEPPED_EVERY: usize = 1_024;
    const STEPPED_OVER: usize = 4_096;
    const STEPPED_BY_AT_MOST: f64 = 0.2;

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
        decibels_of_power(power / ((bin(to_hz) - bin(from_hz)) * frames) as f64)
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

    fn with_a_band_zeroed(frames: usize, gap: &std::ops::Range<usize>) -> Vec<f64> {
        let mut input = tones(frames, 1_000.0, 5_500.0, 120);
        let band = tones(frames, 8_000.0, 9_000.0, 40);
        for (frame, pair) in input.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            if !gap.contains(&frame) {
                pair[0] += band[frame * 2];
                pair[1] += band[frame * 2 + 1];
            }
        }
        input
    }

    #[test]
    fn a_hole_one_granule_long_is_filled_where_it_falls_and_nowhere_else() {
        let frames = 3 * 44_100;
        let gap = 2 * 44_100..2 * 44_100 + GRANULE_FRAMES;
        let input = with_a_band_zeroed(frames, &gap);
        let (output, _) = through(restored(Restoration::Repair), &input);

        let before = band_db(
            &output,
            gap.start - MEASURED - 2_048,
            8_100.0,
            8_900.0,
            MEASURED,
        );
        let inside = gap.start + 64;
        let taken = band_db(&input, inside, 8_100.0, 8_900.0, SHORT_MEASURED) - before;
        let heard = band_db(&output, inside, 8_100.0, 8_900.0, SHORT_MEASURED) - before;
        assert!(taken < -60.0, "the gap in the input reads {taken:.1} dB");
        assert!(
            heard > -30.0,
            "a hole shorter than the window was left at {heard:.1} dB"
        );

        let touched: Vec<usize> = output
            .iter()
            .zip(&input)
            .enumerate()
            .filter(|(_, (made, taken))| (*made - *taken).abs() > 1e-12)
            .map(|(at, _)| at / 2)
            .collect();
        let first = touched.first().copied().expect("something was filled");
        let last = touched.last().copied().expect("something was filled");
        assert!(
            first + FILL_SPILLS_AT_MOST >= gap.start && last <= gap.end + FILL_SPILLS_AT_MOST,
            "the fill reached from frame {first} to {last} around a hole from {} to {}",
            gap.start,
            gap.end
        );
    }

    fn above_against_below(samples: &[f64], from_frame: usize, frames: usize) -> f64 {
        band_db(samples, from_frame, 17_000.0, 20_000.0, frames)
            - band_db(samples, from_frame, 13_000.0, 15_500.0, frames)
    }

    #[test]
    fn an_unstudied_track_is_extended_from_its_first_half_second() {
        let input = tones(4 * 44_100, 100.0, 16_000.0, 300);
        let (extended, stage) = through(restored(Restoration::Extend), &input);
        let wall = f64::from(stage.wall().expect("a wall").centihertz()) / 100.0;
        assert!(
            (15_900.0..=16_300.0).contains(&wall),
            "the wall was read at {wall} Hz"
        );

        let early = above_against_below(&extended, 44_100 / 2, MEASURED);
        assert!(
            (-45.0..=-3.0).contains(&early),
            "half a second in the extension sits at {early:.1} dB"
        );
    }

    #[test]
    fn the_extension_rises_and_settles_without_a_step() {
        let input = tones(4 * 44_100, 100.0, 16_000.0, 300);
        let (extended, _) = through(restored(Restoration::Extend), &input);
        let settled = above_against_below(&extended, 3 * 44_100, MEASURED);

        let shares: Vec<f64> = (0..3 * 44_100 / STEPPED_EVERY)
            .map(|step| {
                gain_of(
                    above_against_below(&extended, step * STEPPED_EVERY, STEPPED_OVER) - settled,
                )
            })
            .collect();
        let worst = shares
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0, f64::max);
        assert!(
            worst < STEPPED_BY_AT_MOST,
            "the extension stepped by {worst:.2} of its settled level between neighbouring windows"
        );
    }
}
