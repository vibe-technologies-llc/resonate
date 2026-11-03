use std::{f64::consts::PI, sync::Arc};

use parking_lot::Mutex;
use rustfft::{FftPlanner, num_complex::Complex};

use crate::resample::{SincParams, WindowedSinc};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FilterPhase {
    #[default]
    Linear,
    Intermediate,
    Minimum,
}

const SAMPLED_PER_TAP: usize = 64;
const SAMPLED_PER_TAP_OF_A_LONG_KERNEL: usize = 16;
const LONG_KERNEL_FROM_HALF_TAPS: u16 = 512;
const CEPSTRUM_PADDING: usize = 32;
const KAISER_BETA_PER_DECIBEL: f64 = 0.1102;
const KAISER_DECIBELS_AT_NO_BETA: f64 = 8.7;
const LOG_FLOOR_UNDER_THE_STOPBAND: f64 = 0.01;
const INTERMEDIATE_LEADS_BY_HALF_WIDTHS: f64 = 1.25;
const INTERMEDIATE_TRAILS_BY_HALF_WIDTHS: f64 = 2.0;
const INTERMEDIATE_SHARE_OF_LINEAR: f64 = 0.5;
const STENCIL: usize = 8;
const STENCIL_REACHES_BACK: usize = STENCIL / 2 - 1;

type Designed = Vec<(SincParams, FilterPhase, Arc<Prototype>)>;

static DESIGNED: Mutex<Designed> = Mutex::new(Vec::new());

pub(crate) struct Prototype {
    samples: Vec<f64>,
    per_tap: usize,
    peak: usize,
    denominators: [f64; STENCIL],
}

impl Prototype {
    pub(crate) fn lead(&self) -> f64 {
        self.peak as f64 / self.per_tap as f64
    }

    pub(crate) fn trail(&self) -> f64 {
        self.samples.len().saturating_sub(self.peak + 1) as f64 / self.per_tap as f64
    }

    pub(crate) fn at(&self, after_peak: f64) -> f64 {
        let position = self.peak as f64 + after_peak * self.per_tap as f64;
        let base = position.floor();
        let between = position - base;
        let first = base as i64 - STENCIL_REACHES_BACK as i64;

        let offsets: [f64; STENCIL] =
            std::array::from_fn(|node| between - (node as f64 - STENCIL_REACHES_BACK as f64));
        let mut rising = [1.0; STENCIL];
        let mut falling = [1.0; STENCIL];
        for node in 1..STENCIL {
            rising[node] = rising[node - 1] * offsets[node - 1];
            falling[STENCIL - 1 - node] = falling[STENCIL - node] * offsets[STENCIL - node];
        }

        (0..STENCIL)
            .map(|node| {
                let sample = usize::try_from(first + node as i64)
                    .ok()
                    .and_then(|index| self.samples.get(index))
                    .copied()
                    .unwrap_or_default();
                sample * rising[node] * falling[node] * self.denominators[node]
            })
            .sum()
    }
}

pub(crate) fn designed(params: SincParams, phase: FilterPhase) -> Option<Arc<Prototype>> {
    if phase == FilterPhase::Linear {
        return None;
    }
    Some(designed_in(&DESIGNED, params, phase, design))
}

fn kept(held: &Designed, params: SincParams, phase: FilterPhase) -> Option<Arc<Prototype>> {
    held.iter()
        .find(|(kept, shaped, _)| *kept == params && *shaped == phase)
        .map(|(_, _, prototype)| Arc::clone(prototype))
}

fn designed_in(
    cache: &Mutex<Designed>,
    params: SincParams,
    phase: FilterPhase,
    make: impl FnOnce(SincParams, FilterPhase) -> Prototype,
) -> Arc<Prototype> {
    if let Some(prototype) = kept(&cache.lock(), params, phase) {
        return prototype;
    }
    let prototype = Arc::new(make(params, phase));
    let mut held = cache.lock();
    if let Some(raced) = kept(&held, params, phase) {
        return raced;
    }
    held.push((params, phase, Arc::clone(&prototype)));
    prototype
}

fn stencil_denominators() -> [f64; STENCIL] {
    std::array::from_fn(|node| {
        let product: f64 = (0..STENCIL)
            .filter(|other| *other != node)
            .map(|other| node as f64 - other as f64)
            .product();
        product.recip()
    })
}

fn stopband(params: SincParams) -> f64 {
    let decibels =
        f64::from(params.kaiser_beta) / KAISER_BETA_PER_DECIBEL + KAISER_DECIBELS_AT_NO_BETA;
    10_f64.powf(-decibels / 20.0)
}

fn design(params: SincParams, phase: FilterPhase) -> Prototype {
    let windowed = WindowedSinc::new(params);
    let per_tap = if params.half_taps >= LONG_KERNEL_FROM_HALF_TAPS {
        SAMPLED_PER_TAP_OF_A_LONG_KERNEL
    } else {
        SAMPLED_PER_TAP
    };
    let reach = usize::from(params.half_taps) * per_tap;
    let taps = 2 * reach + 1;
    let size = (taps * CEPSTRUM_PADDING).next_power_of_two();
    let half = size / 2;

    let mut spectrum = vec![Complex::new(0.0, 0.0); size];
    for (index, bin) in spectrum.iter_mut().take(taps).enumerate() {
        let tau = (index as f64 - reach as f64) / per_tap as f64;
        bin.re = windowed.at(tau.abs());
    }

    let mut planner = FftPlanner::<f64>::new();
    let forward = planner.plan_fft_forward(size);
    let inverse = planner.plan_fft_inverse(size);
    let unscaled = (size as f64).recip();

    forward.process(&mut spectrum);
    let floor = spectrum[0].norm() * stopband(params) * LOG_FLOOR_UNDER_THE_STOPBAND;
    for bin in &mut spectrum {
        *bin = Complex::new(bin.norm().max(floor).ln(), 0.0);
    }
    inverse.process(&mut spectrum);
    for (index, bin) in spectrum.iter_mut().enumerate() {
        let folded = match index {
            0 => 1.0,
            _ if index < half => 2.0,
            _ if index == half => 1.0,
            _ => 0.0,
        };
        *bin = Complex::new(bin.re * unscaled * folded, 0.0);
    }
    forward.process(&mut spectrum);

    let delay = reach as f64;
    for (index, bin) in spectrum.iter_mut().enumerate() {
        let radians = if index <= half {
            2.0 * PI * index as f64 / size as f64
        } else {
            2.0 * PI * (index as f64 - size as f64) / size as f64
        };
        let angle = match phase {
            FilterPhase::Linear | FilterPhase::Minimum => bin.im,
            FilterPhase::Intermediate => {
                (1.0 - INTERMEDIATE_SHARE_OF_LINEAR) * bin.im
                    - INTERMEDIATE_SHARE_OF_LINEAR * radians * delay
            }
        };
        *bin = Complex::from_polar(bin.re.exp(), angle);
    }
    inverse.process(&mut spectrum);

    let response: Vec<f64> = spectrum.iter().map(|bin| bin.re * unscaled).collect();
    let loudest = response
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| left.abs().total_cmp(&right.abs()))
        .map_or(0, |(index, _)| index);

    let (start, length) = match phase {
        FilterPhase::Linear | FilterPhase::Minimum => (0, taps),
        FilterPhase::Intermediate => {
            let lead = (INTERMEDIATE_LEADS_BY_HALF_WIDTHS * reach as f64).round() as usize;
            let trail = (INTERMEDIATE_TRAILS_BY_HALF_WIDTHS * reach as f64).round() as usize;
            ((loudest + size - lead) % size, lead + trail + 1)
        }
    };
    let samples: Vec<f64> = (0..length)
        .map(|offset| {
            response
                .get((start + offset) % size)
                .copied()
                .unwrap_or_default()
        })
        .collect();
    let peak = (loudest + size - start) % size;

    Prototype {
        samples,
        per_tap,
        peak,
        denominators: stencil_denominators(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Quality;

    const STOPBAND_STEPS: u32 = 3_000;
    const EARLIEST_NYQUISTS: f64 = 1.0;
    const WIDEST_NYQUISTS: f64 = 3.0;

    fn response(prototype: &Prototype, nyquists: f64) -> f64 {
        let (mut real, mut imaginary, mut sum) = (0.0, 0.0, 0.0);
        for (index, sample) in prototype.samples.iter().enumerate() {
            let angle = PI * nyquists * index as f64 / prototype.per_tap as f64;
            real += sample * angle.cos();
            imaginary += sample * angle.sin();
            sum += sample;
        }
        real.hypot(imaginary) / sum
    }

    fn decibels(ratio: f64) -> f64 {
        20.0 * ratio.log10()
    }

    const SMALL: SincParams = SincParams {
        half_taps: 4,
        phases: 16,
        cutoff: 0.95,
        kaiser_beta: 8.0,
    };

    #[test]
    fn a_design_is_made_with_the_cache_unlocked_and_kept_for_the_next_asking() {
        let cache = Mutex::new(Vec::new());
        let mut made = 0;

        let first = designed_in(&cache, SMALL, FilterPhase::Minimum, |params, phase| {
            assert!(!cache.is_locked(), "the design ran under the lock");
            made += 1;
            design(params, phase)
        });
        let again = designed_in(&cache, SMALL, FilterPhase::Minimum, |_, _| {
            unreachable!("a kept design was made again")
        });

        assert_eq!(made, 1);
        assert!(Arc::ptr_eq(&first, &again));
    }

    #[test]
    fn an_asking_that_loses_a_race_to_the_same_design_returns_the_one_held() {
        let cache = Mutex::new(Vec::new());

        let first = designed_in(&cache, SMALL, FilterPhase::Intermediate, |params, phase| {
            let raced = designed_in(&cache, params, phase, design);
            assert_eq!(cache.lock().len(), 1);
            drop(raced);
            design(params, phase)
        });

        assert_eq!(cache.lock().len(), 1);
        let kept = designed_in(&cache, SMALL, FilterPhase::Intermediate, design);
        assert!(Arc::ptr_eq(&first, &kept));
    }

    #[test]
    fn a_shaped_filter_keeps_the_stopband_and_passband_the_linear_one_promises() {
        let promised = [
            (Quality::High, 0.90, 2e-6, -160.0),
            (Quality::VeryHigh, 0.90, 2e-6, -190.0),
        ];
        for (quality, flat_until, flat_within_db, rejects_db) in promised {
            for phase in [FilterPhase::Minimum, FilterPhase::Intermediate] {
                let prototype =
                    designed(quality.params(), phase).expect("a shaped phase is designed");
                let at = |nyquists: f64| decibels(response(&prototype, nyquists));

                let flattest = (0..=100)
                    .map(|step| at(flat_until * f64::from(step) / 100.0).abs())
                    .fold(0.0, f64::max);
                assert!(
                    flattest < flat_within_db,
                    "{quality:?} {phase:?} strays {flattest:e} dB inside {flat_until} of Nyquist"
                );

                let loudest = (0..=STOPBAND_STEPS)
                    .map(|step| {
                        let across = f64::from(step) / f64::from(STOPBAND_STEPS);
                        at(EARLIEST_NYQUISTS + across * (WIDEST_NYQUISTS - EARLIEST_NYQUISTS))
                    })
                    .fold(f64::NEG_INFINITY, f64::max);
                assert!(
                    loudest < rejects_db,
                    "{quality:?} {phase:?} reaches {loudest:.1} dB at or past Nyquist"
                );
            }
        }
    }

    fn stopband_energy_db(samples: &[f64], per_tap: usize) -> f64 {
        let size = (samples.len() * 4).next_power_of_two();
        let mut spectrum = vec![Complex::new(0.0, 0.0); size];
        for (bin, sample) in spectrum.iter_mut().zip(samples) {
            bin.re = *sample;
        }
        FftPlanner::<f64>::new()
            .plan_fft_forward(size)
            .process(&mut spectrum);

        let direct = spectrum[0].norm();
        let bins_a_nyquist = size as f64 / (2.0 * per_tap as f64);
        let leaked: f64 = spectrum
            .iter()
            .take(size / 2)
            .enumerate()
            .filter(|(bin, _)| *bin as f64 >= bins_a_nyquist)
            .map(|(_, level)| (level.norm() / direct).powi(2))
            .sum();
        10.0 * (leaked / bins_a_nyquist).log10()
    }

    #[test]
    fn a_shaped_filter_leaks_hardly_more_into_the_stopband_than_the_linear_one() {
        const LEAKS_MORE_BY_AT_MOST_DB: f64 = 3.0;

        for quality in [Quality::Balanced, Quality::High] {
            let params = quality.params();
            let windowed = WindowedSinc::new(params);
            let per_tap = designed(params, FilterPhase::Minimum)
                .expect("a shaped phase is designed")
                .per_tap;
            let reach = usize::from(params.half_taps) * per_tap;
            let linear: Vec<f64> = (0..=2 * reach)
                .map(|index| windowed.at((index as f64 - reach as f64).abs() / per_tap as f64))
                .collect();
            let promised = stopband_energy_db(&linear, per_tap);

            for phase in [FilterPhase::Minimum, FilterPhase::Intermediate] {
                let prototype = designed(params, phase).expect("a shaped phase is designed");
                let leaked = stopband_energy_db(&prototype.samples, per_tap);
                assert!(
                    leaked < promised + LEAKS_MORE_BY_AT_MOST_DB,
                    "{quality:?} {phase:?} leaks {leaked:.1} dB against the linear {promised:.1} dB"
                );
            }
        }
    }

    #[test]
    fn a_minimum_phase_filter_peaks_within_a_few_taps_and_an_intermediate_one_between() {
        let params = Quality::VeryHigh.params();
        let minimum = designed(params, FilterPhase::Minimum).expect("designed");
        let intermediate = designed(params, FilterPhase::Intermediate).expect("designed");
        let half_taps = f64::from(params.half_taps);

        assert!(
            minimum.lead() < 10.0,
            "the peak sits {} taps in",
            minimum.lead()
        );
        assert!(minimum.trail() <= 2.0 * half_taps);
        assert!(intermediate.lead() > minimum.lead());
        assert!(intermediate.lead() + intermediate.trail() > 2.0 * half_taps);
    }

    #[test]
    fn a_prototype_is_designed_once_and_handed_out_again() {
        let params = Quality::Balanced.params();
        let first = designed(params, FilterPhase::Minimum).expect("designed");
        let again = designed(params, FilterPhase::Minimum).expect("designed");
        assert!(Arc::ptr_eq(&first, &again));
        assert!(designed(params, FilterPhase::Linear).is_none());
    }

    #[test]
    fn the_stencil_reads_a_polynomial_of_its_own_order_exactly() {
        let prototype = Prototype {
            samples: (0..64).map(|index| (index as f64 / 8.0).powi(7)).collect(),
            per_tap: SAMPLED_PER_TAP,
            peak: 32,
            denominators: stencil_denominators(),
        };
        for step in 0..40 {
            let after = f64::from(step) / 97.0 - 0.2;
            let position = 32.0 + after * SAMPLED_PER_TAP as f64;
            let exact = (position / 8.0).powi(7);
            let read = prototype.at(after);
            assert!(
                (read - exact).abs() < 1e-9 * exact.abs().max(1.0),
                "{read} against {exact} at {position}"
            );
        }
    }
}
