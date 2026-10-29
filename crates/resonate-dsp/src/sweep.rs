use std::{f64::consts::PI, time::Duration};

use realfft::{RealFftPlanner, num_complex::Complex};
use resonate_core::SampleRate;

use crate::{Error, Result};

pub const SHORTEST_SWEEP: Duration = Duration::from_millis(500);
pub const LONGEST_SWEEP: Duration = Duration::from_secs(60);
pub const FRAMES_HEARD_AT_MOST: usize = 1 << 24;
pub const POINTS_PER_OCTAVE: usize = 48;

const LEVEL: f64 = 0.1;
const FADED_OVER: Duration = Duration::from_millis(20);
const PRE_ROLL: Duration = Duration::from_millis(5);
const GATE_AFTER: Duration = Duration::from_millis(400);
const TAPERED_SHARE: f64 = 0.25;
const REGULARISED_BELOW: f64 = 1e-6;
const SILENT_BELOW: f64 = 1e-12;
const SPECTRUM_BINS_AT_LEAST: usize = 1 << 17;
const FLOOR_DB: f64 = -400.0;

fn frames_of(span: Duration, rate: SampleRate) -> usize {
    (span.as_secs_f64() * f64::from(rate.hz())).round() as usize
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Level {
    pub hertz: f64,
    pub decibels: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sweep {
    rate: SampleRate,
    from_hz: u32,
    to_hz: u32,
    played: Vec<f32>,
}

impl Sweep {
    pub fn new(rate: SampleRate, from_hz: u32, to_hz: u32, length: Duration) -> Result<Self> {
        let below_nyquist = u64::from(to_hz) * 2 < u64::from(rate.hz());
        if from_hz == 0 || to_hz <= from_hz || !below_nyquist {
            return Err(Error::SweepBand {
                from_hz,
                to_hz,
                rate,
            });
        }
        if length < SHORTEST_SWEEP || length > LONGEST_SWEEP {
            return Err(Error::SweepLength {
                frames: frames_of(length, rate),
            });
        }

        let frames = frames_of(length, rate);
        let seconds = frames as f64 / f64::from(rate.hz());
        let from = f64::from(from_hz);
        let octaves_in_e = (f64::from(to_hz) / from).ln();
        let phase_scale = 2.0 * PI * from * seconds / octaves_in_e;
        let fade = frames_of(FADED_OVER, rate).max(1);

        let played = (0..frames)
            .map(|at| {
                let time = at as f64 / f64::from(rate.hz());
                let phase = phase_scale * ((time / seconds * octaves_in_e).exp() - 1.0);
                let rising = (at as f64 / fade as f64).min(1.0);
                let falling = ((frames - 1 - at) as f64 / fade as f64).min(1.0);
                let fading = |share: f64| 0.5 - 0.5 * (PI * share).cos();
                (LEVEL * phase.sin() * fading(rising) * fading(falling)) as f32
            })
            .collect();

        Ok(Self {
            rate,
            from_hz,
            to_hz,
            played,
        })
    }

    pub fn samples(&self) -> &[f32] {
        &self.played
    }

    pub const fn rate(&self) -> SampleRate {
        self.rate
    }

    pub const fn band(&self) -> (u32, u32) {
        (self.from_hz, self.to_hz)
    }

    pub fn length(&self) -> Duration {
        Duration::from_secs_f64(self.played.len() as f64 / f64::from(self.rate.hz()))
    }

    pub fn heard_as(&self, recorded: &[f32]) -> Result<Heard> {
        if recorded.len() < self.played.len() {
            return Err(Error::HeardTooShort {
                heard: recorded.len(),
                played: self.played.len(),
            });
        }
        let size = (recorded.len() + self.played.len()).next_power_of_two();
        if size > FRAMES_HEARD_AT_MOST {
            return Err(Error::HeardTooLong {
                frames: recorded.len(),
            });
        }

        let mut planner = RealFftPlanner::<f64>::new();
        let forward = planner.plan_fft_forward(size);
        let inverse = planner.plan_fft_inverse(size);

        let spectrum_of = |samples: &[f32]| {
            let mut window = forward.make_input_vec();
            for (slot, sample) in window.iter_mut().zip(samples) {
                *slot = f64::from(*sample);
            }
            let mut spectrum = forward.make_output_vec();
            forward
                .process(&mut window, &mut spectrum)
                .map_err(|_| Error::SweepTransform { size })?;
            Ok::<_, Error>(spectrum)
        };
        let played = spectrum_of(&self.played)?;
        let heard = spectrum_of(recorded)?;

        let loudest = played.iter().map(Complex::norm_sqr).fold(0.0_f64, f64::max);
        let regularised = loudest * REGULARISED_BELOW;
        let mut divided: Vec<Complex<f64>> = heard
            .iter()
            .zip(&played)
            .map(|(heard, played)| heard * played.conj() / (played.norm_sqr() + regularised))
            .collect();
        if let Some(first) = divided.first_mut() {
            first.im = 0.0;
        }
        if let Some(last) = divided.last_mut() {
            last.im = 0.0;
        }

        let mut impulse = inverse.make_output_vec();
        inverse
            .process(&mut divided, &mut impulse)
            .map_err(|_| Error::SweepTransform { size })?;
        let scale = 1.0 / size as f64;

        let (peak_at, peak) = impulse
            .iter()
            .enumerate()
            .map(|(at, tap)| (at, tap.abs() * scale))
            .fold(
                (0, 0.0),
                |held: (usize, f64), next| {
                    if next.1 > held.1 { next } else { held }
                },
            );
        if peak < SILENT_BELOW {
            return Err(Error::NothingHeard);
        }

        let before = frames_of(PRE_ROLL, self.rate);
        let after = frames_of(GATE_AFTER, self.rate);
        let taper_from = before + ((after as f64) * (1.0 - TAPERED_SHARE)) as usize;
        let length = before + after;
        let taps = (0..length)
            .map(|at| {
                let source = (peak_at + size + at - before) % size;
                let tap = impulse.get(source).copied().unwrap_or(0.0) * scale;
                let gate = if at < taper_from {
                    1.0
                } else {
                    let share = (at - taper_from) as f64 / (length - taper_from).max(1) as f64;
                    0.5 + 0.5 * (PI * share).cos()
                };
                (tap * gate) as f32
            })
            .collect();

        Ok(Heard {
            rate: self.rate,
            from_hz: self.from_hz,
            to_hz: self.to_hz,
            taps,
            delay: peak_at,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Heard {
    rate: SampleRate,
    from_hz: u32,
    to_hz: u32,
    taps: Vec<f32>,
    delay: usize,
}

impl Heard {
    pub fn taps(&self) -> &[f32] {
        &self.taps
    }

    pub const fn rate(&self) -> SampleRate {
        self.rate
    }

    pub const fn delay_frames(&self) -> usize {
        self.delay
    }

    pub fn response(&self) -> Result<Vec<Level>> {
        let size = self
            .taps
            .len()
            .next_power_of_two()
            .max(SPECTRUM_BINS_AT_LEAST);
        let mut planner = RealFftPlanner::<f64>::new();
        let forward = planner.plan_fft_forward(size);
        let mut window = forward.make_input_vec();
        for (slot, tap) in window.iter_mut().zip(&self.taps) {
            *slot = f64::from(*tap);
        }
        let mut spectrum = forward.make_output_vec();
        forward
            .process(&mut window, &mut spectrum)
            .map_err(|_| Error::SweepTransform { size })?;

        let per_bin = f64::from(self.rate.hz()) / size as f64;
        let octaves = (f64::from(self.to_hz) / f64::from(self.from_hz)).log2();
        let points = (octaves * POINTS_PER_OCTAVE as f64).ceil() as usize;
        Ok((0..=points)
            .map(|point| {
                let hertz = f64::from(self.from_hz)
                    * (f64::from(self.to_hz) / f64::from(self.from_hz))
                        .powf(point as f64 / points as f64);
                let bin = (hertz / per_bin).round() as usize;
                let magnitude = spectrum.get(bin).map_or(0.0, |bin| bin.norm());
                Level {
                    hertz,
                    decibels: (20.0 * magnitude.log10()).max(FLOOR_DB),
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: SampleRate = SampleRate::HZ_48000;
    const SMOOTHING: f64 = 0.3;
    const GAIN: f64 = 0.5;

    fn sweep() -> Sweep {
        Sweep::new(RATE, 20, 20_000, Duration::from_secs(5)).expect("a sweep that fits the rate")
    }

    fn through_a_known_filter(played: &[f32], delay: usize, tail: usize) -> Vec<f32> {
        let mut noise = 0x2545_f491_u32;
        let mut held = 0.0_f64;
        let mut out = vec![0.0_f32; delay];
        for sample in played.iter().copied().chain(std::iter::repeat_n(0.0, tail)) {
            held = SMOOTHING * f64::from(sample) + (1.0 - SMOOTHING) * held;
            noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let hiss = (f64::from(noise >> 8) / f64::from(1_u32 << 24) - 0.5) * 2e-5;
            out.push((GAIN * held + hiss) as f32);
        }
        out
    }

    fn known_db(hertz: f64) -> f64 {
        let turn = 2.0 * PI * hertz / f64::from(RATE.hz());
        let denominator = Complex::new(1.0, 0.0) - Complex::from_polar(1.0 - SMOOTHING, -turn);
        20.0 * (GAIN * SMOOTHING / denominator.norm()).log10()
    }

    fn worst_error_between(levels: &[Level], low: f64, high: f64) -> f64 {
        levels
            .iter()
            .filter(|level| level.hertz >= low && level.hertz <= high)
            .map(|level| (level.decibels - known_db(level.hertz)).abs())
            .fold(0.0, f64::max)
    }

    #[test]
    fn a_sweep_through_a_known_filter_is_deconvolved_to_that_filters_response() {
        let sweep = sweep();
        let heard = sweep
            .heard_as(&through_a_known_filter(sweep.samples(), 137, 48_000))
            .expect("the sweep is heard");

        let levels = heard.response().expect("a response");

        assert!(levels.len() > 400);
        let worst = worst_error_between(&levels, 60.0, 15_000.0);
        assert!(
            worst < 0.5,
            "the response is {worst:.2} dB off the filter's"
        );
        assert!(
            levels
                .iter()
                .any(|level| level.hertz > 10_000.0 && level.decibels < known_db(100.0) - 8.0),
            "the low-pass the filter makes was not measured"
        );
    }

    #[test]
    fn the_delay_between_playing_and_hearing_changes_nothing_but_the_delay() {
        let sweep = sweep();
        let near = sweep
            .heard_as(&through_a_known_filter(sweep.samples(), 10, 48_000))
            .expect("heard");
        let far = sweep
            .heard_as(&through_a_known_filter(sweep.samples(), 9_600, 48_000))
            .expect("heard");

        assert_eq!(far.delay_frames() - near.delay_frames(), 9_590);
        let far = far.response().expect("a response");
        let near = near.response().expect("a response");
        let worst = near
            .iter()
            .zip(&far)
            .filter(|(level, _)| level.hertz > 60.0 && level.hertz < 15_000.0)
            .map(|(near, far)| (near.decibels - far.decibels).abs())
            .fold(0.0, f64::max);
        assert!(worst < 0.1, "a delay moved the response by {worst:.3} dB");
    }

    #[test]
    fn a_sweep_stays_under_its_level_and_fades_in_and_out() {
        let sweep = sweep();
        let samples = sweep.samples();

        let loudest = samples
            .iter()
            .map(|sample| f64::from(sample.abs()))
            .fold(0.0, f64::max);
        assert!(
            loudest <= LEVEL + 1e-6 && loudest > LEVEL * 0.9,
            "{loudest}"
        );
        assert_eq!(samples.first().copied(), Some(0.0));
        assert!(samples.last().is_some_and(|last| last.abs() < 1e-3));
        assert_eq!(sweep.length(), Duration::from_secs(5));
    }

    #[test]
    fn a_band_the_rate_cannot_hold_or_a_length_that_does_not_measure_is_refused() {
        let length = Duration::from_secs(5);
        assert!(matches!(
            Sweep::new(RATE, 20, 24_000, length),
            Err(Error::SweepBand { .. })
        ));
        assert!(matches!(
            Sweep::new(RATE, 0, 20_000, length),
            Err(Error::SweepBand { .. })
        ));
        assert!(matches!(
            Sweep::new(RATE, 2_000, 1_000, length),
            Err(Error::SweepBand { .. })
        ));
        assert!(matches!(
            Sweep::new(RATE, 20, 20_000, Duration::from_millis(100)),
            Err(Error::SweepLength { .. })
        ));
        assert!(matches!(
            Sweep::new(RATE, 20, 20_000, LONGEST_SWEEP + Duration::from_secs(1)),
            Err(Error::SweepLength { .. })
        ));
    }

    #[test]
    fn a_recording_shorter_than_the_sweep_or_with_nothing_in_it_is_refused() {
        let sweep = sweep();

        assert!(matches!(
            sweep.heard_as(&vec![0.0; sweep.samples().len() - 1]),
            Err(Error::HeardTooShort { .. })
        ));
        assert!(matches!(
            sweep.heard_as(&vec![0.0; sweep.samples().len() + 1_000]),
            Err(Error::NothingHeard)
        ));
    }
}
