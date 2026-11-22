use std::f64::consts::TAU;

pub const TONE_AMPLITUDE: f64 = 0.5;
const FREQUENCY_GRID_PER_HZ: f64 = 64.0;
const NOISE_AMPLITUDE: f64 = 0.5;

pub fn on_the_grid(hz: f64) -> f64 {
    (hz * FREQUENCY_GRID_PER_HZ).round() / FREQUENCY_GRID_PER_HZ
}

pub fn phase_at(hz: f64, index: usize, rate: f64) -> f64 {
    TAU * ((hz * index as f64) % rate) / rate
}

pub fn folded(hz: f64, rate: f64) -> f64 {
    let within = hz.rem_euclid(rate);
    if within > rate / 2.0 {
        rate - within
    } else {
        within
    }
}

pub fn tone(hz: f64, rate: f64, frames: usize) -> Vec<f64> {
    (0..frames)
        .map(|index| TONE_AMPLITUDE * phase_at(hz, index, rate).sin())
        .collect()
}

pub struct Multitone {
    pub tones: Vec<(f64, f64)>,
    pub amplitude: f64,
}

impl Multitone {
    pub fn log_spaced(lowest: f64, highest: f64, count: usize, seed: u64) -> Self {
        let mut noise = Noise::seeded(seed);
        let spread = (highest / lowest).ln();

        let tones = (0..count)
            .map(|index| {
                let along = index as f64 / (count - 1) as f64;
                let hz = on_the_grid(lowest * (spread * along).exp());
                (hz, TAU * noise.unit())
            })
            .collect();

        Self {
            tones,
            amplitude: 0.9 / count as f64,
        }
    }

    pub fn at(&self, index: usize, rate: f64, delay: f64) -> f64 {
        self.tones
            .iter()
            .map(|&(hz, phase)| {
                self.amplitude * (phase_at(hz, index, rate) - TAU * hz * delay + phase).sin()
            })
            .sum()
    }

    pub fn rendered(&self, rate: f64, frames: usize) -> Vec<f64> {
        (0..frames).map(|index| self.at(index, rate, 0.0)).collect()
    }
}

pub struct Noise(u64);

impl Noise {
    pub const fn seeded(seed: u64) -> Self {
        Self(seed | 1)
    }

    pub fn unit(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1_u64 << 53) as f64
    }

    pub fn samples(&mut self, count: usize) -> Vec<f64> {
        (0..count)
            .map(|_| NOISE_AMPLITUDE * (2.0 * self.unit() - 1.0))
            .collect()
    }
}

pub fn on_the_bins(hz: f64, rate: f64, frames: usize) -> f64 {
    let width = rate / frames as f64;
    ((hz / width).round().max(1.0)) * width
}

pub fn sine(hz: f64, amplitude: f64, rate: f64, frames: usize) -> Vec<f64> {
    (0..frames)
        .map(|index| amplitude * phase_at(hz, index, rate).sin())
        .collect()
}

pub fn summed(tones: &[(f64, f64)], rate: f64, frames: usize) -> Vec<f64> {
    (0..frames)
        .map(|index| {
            tones
                .iter()
                .map(|&(hz, amplitude)| amplitude * phase_at(hz, index, rate).sin())
                .sum()
        })
        .collect()
}

pub fn phased(tones: &[(f64, f64, f64)], rate: f64, frames: usize) -> Vec<f64> {
    (0..frames)
        .map(|index| {
            tones
                .iter()
                .map(|&(hz, amplitude, phase)| {
                    amplitude * (phase_at(hz, index, rate) + phase).sin()
                })
                .sum()
        })
        .collect()
}

pub fn spread(mono: &[f64], channels: usize) -> Vec<f64> {
    mono.iter()
        .flat_map(|&sample| std::iter::repeat_n(sample, channels))
        .collect()
}

pub fn on_one(mono: &[f64], channel: usize, channels: usize) -> Vec<f64> {
    let mut whole = vec![0.0; mono.len() * channels];

    for (frame, &sample) in whole.chunks_exact_mut(channels).zip(mono) {
        frame[channel] = sample;
    }
    whole
}

pub fn impulse(frames: usize, at: usize) -> Vec<f64> {
    let mut input = vec![0.0; frames];

    input[at] = 1.0;
    input
}

pub fn walled(samples: &[f64], rate: f64, wall_hz: f64) -> Vec<f64> {
    let frames = samples.len();
    let mut planner = realfft::RealFftPlanner::<f64>::new();

    let forward = planner.plan_fft_forward(frames);
    let inverse = planner.plan_fft_inverse(frames);
    let mut input = samples.to_vec();
    let mut bins = forward.make_output_vec();
    let mut output = inverse.make_output_vec();
    let first_cut = (wall_hz * frames as f64 / rate).ceil() as usize;

    if forward.process(&mut input, &mut bins).is_err() {
        return samples.to_vec();
    }
    for bin in bins.iter_mut().skip(first_cut) {
        *bin = realfft::num_complex::Complex::new(0.0, 0.0);
    }
    if let Some(last) = bins.last_mut() {
        last.im = 0.0;
    }
    if let Some(first) = bins.first_mut() {
        first.im = 0.0;
    }
    if inverse.process(&mut bins, &mut output).is_err() {
        return samples.to_vec();
    }
    output.iter().map(|sample| sample / frames as f64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tone_on_the_grid_keeps_its_phase_exact_far_into_a_signal() {
        let hz = on_the_grid(997.3);
        let rate = 44_100.0;
        let index = 10_000_000;

        let reduced = phase_at(hz, index, rate);
        let wrapped = (TAU * hz * index as f64 / rate).rem_euclid(TAU);

        assert!((reduced - wrapped).abs() < 1e-6);
        assert!(reduced < TAU);
    }

    #[test]
    fn a_tone_on_the_bins_lands_on_a_whole_number_of_cycles() {
        let frames = 65_536;
        let hz = on_the_bins(997.0, 48_000.0, frames);

        assert!((hz * frames as f64 / 48_000.0).fract() == 0.0);
        assert!((hz - 997.0).abs() < 0.4);
    }

    #[test]
    fn a_frequency_past_nyquist_folds_back_under_it() {
        assert_eq!(folded(30_000.0, 48_000.0), 18_000.0);
        assert_eq!(folded(43_100.0, 48_000.0), 4_900.0);
        assert_eq!(folded(10_000.0, 48_000.0), 10_000.0);
    }
}
