use realfft::RealFftPlanner;

pub const AUDIBLE_FROM_HZ: f64 = 20.0;
pub const AUDIBLE_TO_HZ: f64 = 20_000.0;
const HARMONICS_COUNTED: usize = 10;
const A_WEIGHT_AT_1_KHZ_DB: f64 = 2.0;

pub struct Spectrum {
    power: Vec<f64>,
    bin_hz: f64,
}

impl Spectrum {
    pub fn of(samples: &[f64], rate: f64) -> Self {
        let frames = samples.len();
        let mut planner = RealFftPlanner::<f64>::new();

        let forward = planner.plan_fft_forward(frames);
        let mut input = samples.to_vec();
        let mut bins = forward.make_output_vec();

        let transformed = forward.process(&mut input, &mut bins);
        let scale = (frames as f64).powi(2);
        let last = bins.len() - 1;

        let power = bins
            .iter()
            .enumerate()
            .map(|(index, bin)| {
                let one_sided = if index == 0 || (index == last && frames.is_multiple_of(2)) {
                    1.0
                } else {
                    2.0
                };
                one_sided * bin.norm_sqr() / scale
            })
            .collect();

        debug_assert!(transformed.is_ok());
        Self {
            power,
            bin_hz: rate / frames as f64,
        }
    }

    pub fn hann_windowed(samples: &[f64], rate: f64) -> Self {
        let frames = samples.len() as f64;
        let windowed: Vec<f64> = samples
            .iter()
            .enumerate()
            .map(|(index, sample)| {
                let along = index as f64 / frames;
                sample * (1.0 - (std::f64::consts::TAU * along).cos())
            })
            .collect();

        Self::of(&windowed, rate)
    }

    pub fn bin_of(&self, hz: f64) -> usize {
        ((hz / self.bin_hz).round() as usize).min(self.power.len() - 1)
    }

    pub fn hz_of(&self, bin: usize) -> f64 {
        bin as f64 * self.bin_hz
    }

    pub fn nyquist(&self) -> f64 {
        self.hz_of(self.power.len() - 1)
    }

    pub fn power_at(&self, hz: f64) -> f64 {
        self.power[self.bin_of(hz)]
    }

    pub fn amplitude_at(&self, hz: f64) -> f64 {
        (2.0 * self.power_at(hz)).sqrt()
    }

    fn within(&self, from_hz: f64, to_hz: f64) -> impl Iterator<Item = (usize, f64)> + '_ {
        let first = (from_hz / self.bin_hz).ceil() as usize;
        let last = ((to_hz / self.bin_hz).floor() as usize).min(self.power.len() - 1);

        (first..=last).map(|bin| (bin, self.power[bin]))
    }

    pub fn band(&self, from_hz: f64, to_hz: f64) -> f64 {
        self.within(from_hz, to_hz).map(|(_, power)| power).sum()
    }

    pub fn band_without(&self, from_hz: f64, to_hz: f64, without: &[f64]) -> f64 {
        let left_out: Vec<usize> = without.iter().map(|&hz| self.bin_of(hz)).collect();

        self.within(from_hz, to_hz)
            .filter(|(bin, _)| !left_out.contains(bin))
            .map(|(_, power)| power)
            .sum()
    }

    pub fn weighted_without(&self, from_hz: f64, to_hz: f64, without: &[f64]) -> f64 {
        let left_out: Vec<usize> = without.iter().map(|&hz| self.bin_of(hz)).collect();

        self.within(from_hz, to_hz)
            .filter(|(bin, _)| !left_out.contains(bin))
            .map(|(bin, power)| power * a_weighting(self.hz_of(bin)))
            .sum()
    }

    pub fn audible_top(&self) -> f64 {
        AUDIBLE_TO_HZ.min(self.nyquist())
    }

    pub fn harmonics_of(&self, hz: f64) -> Vec<f64> {
        (2..=HARMONICS_COUNTED)
            .map(|order| hz * order as f64)
            .filter(|&harmonic| harmonic <= self.audible_top())
            .collect()
    }
}

pub fn a_weighting(hz: f64) -> f64 {
    let squared = hz * hz;
    let response = 12_194.0_f64.powi(2) * squared * squared
        / ((squared + 20.6_f64.powi(2))
            * ((squared + 107.7_f64.powi(2)) * (squared + 737.9_f64.powi(2))).sqrt()
            * (squared + 12_194.0_f64.powi(2)));
    let amplitude = response * 10_f64.powf(A_WEIGHT_AT_1_KHZ_DB / 20.0);

    amplitude * amplitude
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::{on_the_bins, summed};

    const FRAMES: usize = 65_536;
    const RATE: f64 = 48_000.0;

    #[test]
    fn a_tone_on_the_bins_lands_whole_in_one_bin_at_its_own_power() {
        let hz = on_the_bins(997.0, RATE, FRAMES);
        let spectrum = Spectrum::of(&summed(&[(hz, 0.5)], RATE, FRAMES), RATE);

        assert!((spectrum.amplitude_at(hz) - 0.5).abs() < 1e-12);
        assert!(spectrum.band_without(AUDIBLE_FROM_HZ, AUDIBLE_TO_HZ, &[hz]) < 1e-28);
    }

    #[test]
    fn harmonics_of_known_levels_read_at_those_levels() {
        let hz = on_the_bins(1_000.0, RATE, FRAMES);
        let tones = [(hz, 0.5), (2.0 * hz, 0.5e-3), (3.0 * hz, 0.5e-4)];
        let spectrum = Spectrum::of(&summed(&tones, RATE, FRAMES), RATE);

        let harmonics: f64 = spectrum
            .harmonics_of(hz)
            .iter()
            .map(|&harmonic| spectrum.power_at(harmonic))
            .sum();
        let wanted = (1e-6 + 1e-8) / 1.0;

        assert!((harmonics / spectrum.power_at(hz) - wanted).abs() < 1e-12);
    }

    #[test]
    fn the_a_weighting_is_unity_at_a_kilohertz_and_falls_away_at_the_ends() {
        assert!((10.0 * a_weighting(1_000.0).log10()).abs() < 0.01);
        assert!((10.0 * a_weighting(100.0).log10() + 19.1).abs() < 0.1);
        assert!((10.0 * a_weighting(10_000.0).log10() + 2.5).abs() < 0.1);
    }
}
