const BESSEL_TERMS: u32 = 24;

pub(crate) fn bessel_i0(x: f64) -> f64 {
    let half = x / 2.0;
    let mut term = 1.0;
    let mut sum = 1.0;

    for step in 1..=BESSEL_TERMS {
        let step = f64::from(step);
        term *= (half / step) * (half / step);
        sum += term;
        if term < sum * f64::EPSILON {
            break;
        }
    }
    sum
}

pub(crate) fn beta_for(stopband_db: f64) -> f64 {
    if stopband_db > 50.0 {
        0.1102 * (stopband_db - 8.7)
    } else if stopband_db >= 21.0 {
        0.5842 * (stopband_db - 21.0).powf(0.4) + 0.07886 * (stopband_db - 21.0)
    } else {
        0.0
    }
}

pub(crate) fn kaiser_sinc(taps: usize, cutoff: f64, beta: f64) -> Vec<f64> {
    let last = taps.saturating_sub(1) as f64;
    let middle = last / 2.0;
    let shape = bessel_i0(beta);

    let mut kernel: Vec<f64> = (0..taps)
        .map(|tap| {
            let from = tap as f64 - middle;
            let ideal = if from == 0.0 {
                2.0 * cutoff
            } else {
                (std::f64::consts::TAU * cutoff * from).sin() / (std::f64::consts::PI * from)
            };
            let ratio = if last == 0.0 { 0.0 } else { 2.0 * from / last };
            let window = bessel_i0(beta * (1.0 - ratio * ratio).max(0.0).sqrt()) / shape;
            ideal * window
        })
        .collect();

    let sum: f64 = kernel.iter().sum();
    if sum != 0.0 {
        for tap in &mut kernel {
            *tap /= sum;
        }
    }
    kernel
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gain_at(kernel: &[f64], cycles_per_sample: f64) -> f64 {
        let mut real = 0.0;
        let mut imaginary = 0.0;
        for (tap, weight) in kernel.iter().enumerate() {
            let angle = std::f64::consts::TAU * cycles_per_sample * tap as f64;
            real += weight * angle.cos();
            imaginary -= weight * angle.sin();
        }
        real.hypot(imaginary)
    }

    #[test]
    fn bessel_i0_matches_its_published_values() {
        assert!((bessel_i0(0.0) - 1.0).abs() < 1e-12);
        assert!((bessel_i0(1.0) - 1.266_065_877_75).abs() < 1e-9);
        assert!((bessel_i0(5.0) - 27.239_871_823_6).abs() < 1e-6);
    }

    #[test]
    fn the_designed_kernel_passes_direct_current_untouched() {
        let kernel = kaiser_sinc(512, 45_000.0 / 2_822_400.0, beta_for(96.0));
        assert!((gain_at(&kernel, 0.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn the_designed_kernel_stops_what_would_fold_into_the_band() {
        let kernel = kaiser_sinc(512, 45_000.0 / 2_822_400.0, beta_for(96.0));
        let nyquist = 88_200.0 / 2_822_400.0;

        let rejected = 20.0 * gain_at(&kernel, nyquist).log10();
        assert!(
            rejected < -90.0,
            "the decimator would fold {rejected} dB of ultrasonic noise into the band"
        );
    }
}
