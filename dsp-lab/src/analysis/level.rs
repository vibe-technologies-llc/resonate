use std::num::NonZeroUsize;

use resonate_core::SampleRate;
use resonate_dsp::TruePeakMeter;

const QUIETEST_AMPLITUDE: f64 = 1e-30;
const QUIETEST_POWER: f64 = 1e-60;

pub fn decibels(ratio: f64) -> f64 {
    20.0 * ratio.max(QUIETEST_AMPLITUDE).log10()
}

pub fn power_decibels(ratio: f64) -> f64 {
    10.0 * ratio.max(QUIETEST_POWER).log10()
}

pub fn rms(samples: &[f64]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|sample| sample * sample).sum::<f64>() / samples.len() as f64).sqrt()
}

pub fn loudest(samples: &[f64]) -> usize {
    samples
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.abs().total_cmp(&b.abs()))
        .map_or(0, |(index, _)| index)
}

pub fn true_peak(interleaved: &[f64], channels: usize, rate: SampleRate) -> f64 {
    let Some(channels) = NonZeroUsize::new(channels) else {
        return 0.0;
    };
    let narrowed: Vec<f32> = interleaved.iter().map(|&sample| sample as f32).collect();
    let mut meter = TruePeakMeter::at(rate, channels);

    meter.note(&narrowed);
    meter.finish()
}

pub fn null_db(heard: &[f64], wanted: &[f64]) -> f64 {
    let (error, ideal) =
        heard
            .iter()
            .zip(wanted)
            .fold((0.0, 0.0), |(error, ideal), (&heard, &wanted)| {
                (error + (heard - wanted).powi(2), ideal + wanted * wanted)
            });
    let missing: f64 = wanted
        .iter()
        .skip(heard.len())
        .map(|sample| sample * sample)
        .sum();

    if ideal == 0.0 {
        return f64::NAN;
    }
    if error + missing == 0.0 {
        return f64::NEG_INFINITY;
    }
    power_decibels((error + missing) / (ideal + missing.min(0.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signal_against_itself_nulls_to_nothing_and_a_tenth_off_to_minus_twenty() {
        let wanted: Vec<f64> = (0..100)
            .map(|index| (f64::from(index) * 0.1).sin())
            .collect();
        let off: Vec<f64> = wanted.iter().map(|sample| sample * 1.1).collect();

        assert_eq!(null_db(&wanted, &wanted), f64::NEG_INFINITY);
        assert!((null_db(&off, &wanted) + 20.0).abs() < 1e-9);
    }

    #[test]
    fn a_signal_cut_short_does_not_null() {
        let wanted = vec![0.5; 100];

        assert!(null_db(&wanted[..50], &wanted) > -4.0);
    }
}
