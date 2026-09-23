use std::{f64::consts::PI, sync::Arc};

use parking_lot::{Mutex, const_mutex};
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
const CEPSTRUM_PADDING: usize = 16;
const QUIETEST_MAGNITUDE: f64 = 1e-30;
const INTERMEDIATE_LEADS_BY_HALF_WIDTHS: f64 = 1.25;
const INTERMEDIATE_TRAILS_BY_HALF_WIDTHS: f64 = 2.0;
const INTERMEDIATE_SHARE_OF_LINEAR: f64 = 0.5;
const STENCIL: usize = 8;
const STENCIL_REACHES_BACK: usize = STENCIL / 2 - 1;

type Designed = Vec<(SincParams, FilterPhase, Arc<Prototype>)>;

static DESIGNED: Mutex<Designed> = const_mutex(Vec::new());

pub(crate) struct Prototype {
    samples: Vec<f64>,
    peak: usize,
    denominators: [f64; STENCIL],
}

impl Prototype {
    pub(crate) fn lead(&self) -> f64 {
        self.peak as f64 / SAMPLED_PER_TAP as f64
    }

    pub(crate) fn trail(&self) -> f64 {
        self.samples.len().saturating_sub(self.peak + 1) as f64 / SAMPLED_PER_TAP as f64
    }

    pub(crate) fn at(&self, after_peak: f64) -> f64 {
        let position = self.peak as f64 + after_peak * SAMPLED_PER_TAP as f64;
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
    let mut held = DESIGNED.lock();
    if let Some((_, _, prototype)) = held
        .iter()
        .find(|(kept, shaped, _)| *kept == params && *shaped == phase)
    {
        return Some(Arc::clone(prototype));
    }
    let prototype = Arc::new(design(params, phase));
    held.push((params, phase, Arc::clone(&prototype)));
    Some(prototype)
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

fn design(params: SincParams, phase: FilterPhase) -> Prototype {
    let windowed = WindowedSinc::new(params);
    let reach = usize::from(params.half_taps) * SAMPLED_PER_TAP;
    let taps = 2 * reach + 1;
    let size = (taps * CEPSTRUM_PADDING).next_power_of_two();
    let half = size / 2;

    let mut spectrum = vec![Complex::new(0.0, 0.0); size];
    for (index, bin) in spectrum.iter_mut().take(taps).enumerate() {
        let tau = (index as f64 - reach as f64) / SAMPLED_PER_TAP as f64;
        bin.re = windowed.at(tau.abs());
    }

    let mut planner = FftPlanner::<f64>::new();
    let forward = planner.plan_fft_forward(size);
    let inverse = planner.plan_fft_inverse(size);
    let unscaled = (size as f64).recip();

    forward.process(&mut spectrum);
    for bin in &mut spectrum {
        *bin = Complex::new(bin.norm().max(QUIETEST_MAGNITUDE).ln(), 0.0);
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
        peak,
        denominators: stencil_denominators(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Quality;

    const PASSBAND_FLAT_WITHIN_DB: f64 = 1e-4;
    const VERY_HIGH_REJECTS_DB: f64 = -180.0;
    const STOPBAND_STEPS: u32 = 3_000;
    const EARLIEST_NYQUISTS: f64 = 1.0;
    const WIDEST_NYQUISTS: f64 = 3.0;

    fn response(prototype: &Prototype, nyquists: f64) -> f64 {
        let (mut real, mut imaginary, mut sum) = (0.0, 0.0, 0.0);
        for (index, sample) in prototype.samples.iter().enumerate() {
            let angle = PI * nyquists * index as f64 / SAMPLED_PER_TAP as f64;
            real += sample * angle.cos();
            imaginary += sample * angle.sin();
            sum += sample;
        }
        real.hypot(imaginary) / sum
    }

    fn decibels(ratio: f64) -> f64 {
        20.0 * ratio.log10()
    }

    #[test]
    fn a_shaped_very_high_filter_keeps_the_stopband_and_passband_the_linear_one_promises() {
        for phase in [FilterPhase::Minimum, FilterPhase::Intermediate] {
            let prototype =
                designed(Quality::VeryHigh.params(), phase).expect("a shaped phase is designed");
            let at = |nyquists: f64| decibels(response(&prototype, nyquists));

            let flattest = (0..=95)
                .map(|step| at(f64::from(step) / 100.0).abs())
                .fold(0.0, f64::max);
            assert!(
                flattest < PASSBAND_FLAT_WITHIN_DB,
                "{phase:?} strays {flattest:e} dB inside 95 % of Nyquist"
            );

            let loudest = (0..=STOPBAND_STEPS)
                .map(|step| {
                    let across = f64::from(step) / f64::from(STOPBAND_STEPS);
                    at(EARLIEST_NYQUISTS + across * (WIDEST_NYQUISTS - EARLIEST_NYQUISTS))
                })
                .fold(f64::NEG_INFINITY, f64::max);
            assert!(
                loudest < VERY_HIGH_REJECTS_DB,
                "{phase:?} reaches {loudest:.1} dB at or past Nyquist"
            );
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
