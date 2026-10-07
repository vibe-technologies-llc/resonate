use std::{num::NonZeroUsize, time::Duration};

use resonate_engine::TruePeakMeter;

use crate::spectrum::{Bar, FLOOR_DB, LONGEST_STEP, QUIETEST_DB, height_of};

const SILENT_ENERGY: f32 = 1e-12;
const CORRELATION_SETTLES_OVER: Duration = Duration::from_millis(300);
const HALF: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Meter {
    pub(crate) level: f32,
    pub(crate) peak: f32,
    pub(crate) peak_db: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Metered {
    pub(crate) left: Meter,
    pub(crate) right: Meter,
    pub(crate) correlation: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Heard<'a> {
    pub(crate) left: &'a [f32],
    pub(crate) right: &'a [f32],
    pub(crate) heard: u64,
}

impl Heard<'_> {
    const fn centre(&self) -> usize {
        let frames = if self.left.len() < self.right.len() {
            self.left.len()
        } else {
            self.right.len()
        };
        frames - frames / 2
    }
}

struct TruePeaks {
    left: TruePeakMeter,
    right: TruePeakMeter,
    reached: Option<u64>,
    read: (f32, f32),
}

impl TruePeaks {
    fn new() -> Self {
        Self {
            left: TruePeakMeter::new(NonZeroUsize::MIN),
            right: TruePeakMeter::new(NonZeroUsize::MIN),
            reached: None,
            read: (QUIETEST_DB, QUIETEST_DB),
        }
    }

    fn read(&mut self, window: Heard<'_>) -> (f32, f32) {
        let centre = window.centre();
        let fresh = match self.reached {
            Some(reached) if window.heard >= reached => {
                usize::try_from(window.heard - reached).unwrap_or(usize::MAX)
            }
            _ => usize::MAX,
        };
        self.reached = Some(window.heard);
        if fresh == 0 {
            return self.read;
        }
        if fresh > centre {
            *self = Self {
                reached: self.reached,
                ..Self::new()
            };
        }

        let from = centre.saturating_sub(fresh);
        let taken = |meter: &mut TruePeakMeter, samples: &[f32]| {
            samples
                .get(from..centre)
                .map_or(QUIETEST_DB, |fresh| decibels_of(meter.take(fresh) as f32))
        };
        self.read = (
            taken(&mut self.left, window.left),
            taken(&mut self.right, window.right),
        );
        self.read
    }
}

pub(crate) struct Stereo {
    left: Bar,
    right: Bar,
    left_peak: Bar,
    right_peak: Bar,
    true_peaks: TruePeaks,
    correlation: Option<f32>,
}

impl Default for Stereo {
    fn default() -> Self {
        Self {
            left: Bar::METERED,
            right: Bar::METERED,
            left_peak: Bar::METERED,
            right_peak: Bar::METERED,
            true_peaks: TruePeaks::new(),
            correlation: None,
        }
    }
}

impl Stereo {
    pub(crate) fn take(&mut self, window: Heard<'_>, step: Duration) {
        let Heard { left, right, .. } = window;
        let step = step.min(LONGEST_STEP);
        let (left_peak, right_peak) = self.true_peaks.read(window);
        self.left.fold(mean_square_db(left), step);
        self.right.fold(mean_square_db(right), step);
        self.left_peak.fold(left_peak, step);
        self.right_peak.fold(right_peak, step);

        self.correlation = match (correlation(left, right), self.correlation) {
            (Some(read), Some(held)) => Some(settled_towards(held, read, step)),
            (read, None) => read,
            (None, held) => held.map(|held| settled_towards(held, 0.0, step)),
        };
    }

    pub(crate) fn is_at_rest(&self) -> bool {
        self.left.is_at_rest()
            && self.right.is_at_rest()
            && self.left_peak.is_at_rest()
            && self.right_peak.is_at_rest()
            && self
                .correlation
                .is_none_or(|held| held.abs() < f32::EPSILON)
    }

    pub(crate) fn metered(&self) -> Metered {
        let meter = |level: Bar, peak: Bar| Meter {
            level: height_of(level.level),
            peak: height_of(peak.peak),
            peak_db: peak.peak.max(FLOOR_DB),
        };
        Metered {
            left: meter(self.left, self.left_peak),
            right: meter(self.right, self.right_peak),
            correlation: self.correlation,
        }
    }
}

fn settled_towards(held: f32, read: f32, step: Duration) -> f32 {
    let share = 1.0 - (-step.as_secs_f32() / CORRELATION_SETTLES_OVER.as_secs_f32()).exp();
    (read - held).mul_add(share, held)
}

pub(crate) fn mean_square_db(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return QUIETEST_DB;
    }
    let energy = samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32;
    if energy <= SILENT_ENERGY {
        return QUIETEST_DB;
    }
    (10.0 * energy.log10()).max(QUIETEST_DB)
}

fn decibels_of(peak: f32) -> f32 {
    if peak * peak <= SILENT_ENERGY {
        return QUIETEST_DB;
    }
    (20.0 * peak.log10()).max(QUIETEST_DB)
}

pub(crate) fn correlation(left: &[f32], right: &[f32]) -> Option<f32> {
    let (mut both, mut on_the_left, mut on_the_right) = (0.0_f64, 0.0_f64, 0.0_f64);
    for (left, right) in left.iter().zip(right) {
        let (left, right) = (f64::from(*left), f64::from(*right));
        both += left * right;
        on_the_left += left * left;
        on_the_right += right * right;
    }
    let apart = (on_the_left * on_the_right).sqrt();
    (apart > f64::from(SILENT_ENERGY)).then(|| (both / apart).clamp(-1.0, 1.0) as f32)
}

pub(crate) fn sides_and_mids(left: &[f32], right: &[f32]) -> impl Iterator<Item = (f32, f32)> {
    left.iter()
        .zip(right)
        .map(|(left, right)| ((left - right) * HALF, (left + right) * HALF))
}

#[cfg(test)]
mod tests {
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, TAU};

    use super::*;

    fn tone(frames: usize, amplitude: f32) -> Vec<f32> {
        (0..frames)
            .map(|at| amplitude * (TAU * at as f32 / 64.0).sin())
            .collect()
    }

    #[test]
    fn a_full_scale_sine_meters_three_decibels_under_full_scale_and_peaks_at_it() {
        let sine = tone(4_096, 1.0);

        assert!((mean_square_db(&sine) + 3.01).abs() < 0.05);
        assert_eq!(mean_square_db(&vec![0.0; 64]), QUIETEST_DB);
    }

    #[test]
    fn the_meters_hold_the_peak_between_the_samples_and_read_each_frame_once() {
        let straddled: Vec<f32> = (0..4_096)
            .map(|at| (FRAC_PI_2 * at as f32 + FRAC_PI_4).sin())
            .collect();
        let sampled = straddled
            .iter()
            .fold(0.0_f32, |loudest, sample| loudest.max(sample.abs()));
        let mut stereo = Stereo::default();
        let window = |heard| Heard {
            left: &straddled,
            right: &straddled,
            heard,
        };

        stereo.take(window(10_000), Duration::from_millis(16));
        let first = stereo.metered();
        stereo.take(window(10_000), Duration::from_millis(16));
        let again = stereo.metered();
        stereo.take(window(10_800), Duration::from_millis(16));
        let on = stereo.metered();

        assert!(decibels_of(sampled) < -2.9);
        assert!(first.left.peak_db.abs() < 0.1, "{}", first.left.peak_db);
        assert_eq!(again, first);
        assert!(on.right.peak_db.abs() < 0.1, "{}", on.right.peak_db);
    }

    #[test]
    fn the_same_signal_correlates_wholly_an_inverted_one_wholly_against_and_silence_not_at_all() {
        let sine = tone(4_096, 0.5);
        let inverted: Vec<f32> = sine.iter().map(|sample| -sample).collect();
        let quarter_turned: Vec<f32> = (0..4_096)
            .map(|at| 0.5 * (TAU * at as f32 / 64.0).cos())
            .collect();

        assert!((correlation(&sine, &sine).expect("a reading") - 1.0).abs() < 1e-4);
        assert!((correlation(&sine, &inverted).expect("a reading") + 1.0).abs() < 1e-4);
        assert!(
            correlation(&sine, &quarter_turned)
                .expect("a reading")
                .abs()
                < 1e-3
        );
        assert_eq!(correlation(&[0.0; 64], &sine), None);
    }

    #[test]
    fn a_mono_signal_lies_on_the_mid_and_one_side_alone_on_a_diagonal() {
        let read: Vec<(f32, f32)> = sides_and_mids(&[0.5, 0.25], &[0.5, -0.25]).collect();

        assert_eq!(read, [(0.0, 0.5), (0.25, 0.0)]);
    }

    #[test]
    fn the_meters_rise_at_once_and_settle_back_to_rest_with_the_correlation() {
        let sine = tone(4_096, 1.0);
        let silence = vec![0.0; 4_096];
        let mut stereo = Stereo::default();

        stereo.take(
            Heard {
                left: &sine,
                right: &sine,
                heard: 0,
            },
            Duration::from_millis(16),
        );
        let lit = stereo.metered();
        assert!(lit.left.level > 0.9 && lit.right.level > 0.9);
        assert_eq!(lit.correlation, Some(1.0));

        for heard in (1..=400).map(|step| step * 4_096) {
            stereo.take(
                Heard {
                    left: &silence,
                    right: &silence,
                    heard,
                },
                Duration::from_millis(50),
            );
        }
        assert!(stereo.is_at_rest());
    }
}
