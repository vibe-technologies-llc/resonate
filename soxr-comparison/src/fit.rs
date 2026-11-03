use crate::signal::phase_at;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sinusoid {
    pub sine: f64,
    pub cosine: f64,
}

impl Sinusoid {
    pub fn amplitude(self) -> f64 {
        self.sine.hypot(self.cosine)
    }

    pub fn phase(self) -> f64 {
        self.cosine.atan2(self.sine)
    }
}

pub struct Window<'a> {
    pub samples: &'a [f64],
    pub first: usize,
    pub rate: f64,
}

impl Window<'_> {
    fn basis(&self, hz: &[f64], offset: usize, row: &mut [[f64; 2]]) {
        for (pair, &frequency) in row.iter_mut().zip(hz) {
            let (sine, cosine) = phase_at(frequency, self.first + offset, self.rate).sin_cos();
            *pair = [sine, cosine];
        }
    }

    pub fn fitted(&self, hz: &[f64]) -> Vec<Sinusoid> {
        let width = 2 * hz.len();
        let mut normal = vec![0.0; width * width];
        let mut projected = vec![0.0; width];
        let mut row = vec![[0.0; 2]; hz.len()];

        for (offset, &sample) in self.samples.iter().enumerate() {
            self.basis(hz, offset, &mut row);
            let flat = row.as_flattened();
            for (across, &left) in flat.iter().enumerate() {
                projected[across] += left * sample;
                for (down, &right) in flat.iter().enumerate() {
                    normal[across * width + down] += left * right;
                }
            }
        }

        solved(&mut normal, &mut projected, width)
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&[sine, cosine]| Sinusoid { sine, cosine })
            .collect()
    }

    pub fn rms_without(&self, hz: &[f64], fitted: &[Sinusoid]) -> f64 {
        let mut row = vec![[0.0; 2]; hz.len()];
        let mut energy = 0.0;

        for (offset, &sample) in self.samples.iter().enumerate() {
            self.basis(hz, offset, &mut row);
            let modelled: f64 = row
                .iter()
                .zip(fitted)
                .map(|(&[sine, cosine], sinusoid)| sine * sinusoid.sine + cosine * sinusoid.cosine)
                .sum();
            energy += (sample - modelled).powi(2);
        }

        (energy / self.samples.len() as f64).sqrt()
    }

    pub fn rms(&self) -> f64 {
        let energy: f64 = self.samples.iter().map(|sample| sample * sample).sum();
        (energy / self.samples.len() as f64).sqrt()
    }
}

fn solved(normal: &mut [f64], projected: &mut [f64], width: usize) -> Vec<f64> {
    for pivot in 0..width {
        let largest = (pivot..width)
            .max_by(|&a, &b| {
                normal[a * width + pivot]
                    .abs()
                    .total_cmp(&normal[b * width + pivot].abs())
            })
            .unwrap_or(pivot);
        if largest != pivot {
            for column in 0..width {
                normal.swap(pivot * width + column, largest * width + column);
            }
            projected.swap(pivot, largest);
        }
        let lead = normal[pivot * width + pivot];
        for below in pivot + 1..width {
            let factor = normal[below * width + pivot] / lead;
            for column in pivot..width {
                normal[below * width + column] -= factor * normal[pivot * width + column];
            }
            projected[below] -= factor * projected[pivot];
        }
    }

    let mut solution = vec![0.0; width];
    for row in (0..width).rev() {
        let known: f64 = (row + 1..width)
            .map(|column| normal[row * width + column] * solution[column])
            .sum();
        solution[row] = (projected[row] - known) / normal[row * width + row];
    }
    solution
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{PI, TAU};

    use super::*;

    #[test]
    fn two_sinusoids_are_recovered_with_their_phases() {
        let rate = 48_000.0;
        let samples: Vec<f64> = (0..20_000)
            .map(|index| {
                0.25 * (phase_at(1_000.0, index, rate) + 0.5).sin()
                    + 1e-6 * (phase_at(4_900.0, index, rate) - 1.0).sin()
            })
            .collect();
        let window = Window {
            samples: &samples[5_000..15_000],
            first: 5_000,
            rate,
        };

        let fitted = window.fitted(&[1_000.0, 4_900.0]);

        assert!((fitted[0].amplitude() - 0.25).abs() < 1e-14);
        assert!((fitted[0].phase() - 0.5).abs() < 1e-12);
        assert!((fitted[1].amplitude() - 1e-6).abs() < 1e-14);
        assert!(((fitted[1].phase() + 1.0 + PI).rem_euclid(TAU) - PI).abs() < 1e-6);
        assert!(window.rms_without(&[1_000.0, 4_900.0], &fitted) < 1e-15);
    }
}
