use std::{f32::consts::PI, sync::Arc, time::Duration};

use rustfft::{Fft, FftPlanner, num_complex::Complex};

pub const FLOOR_DB: f32 = -160.0;

const SILENT_BELOW_DB: f32 = -60.0;

pub(crate) const BAND_HZ: f32 = 100.0;

const TYPICAL_QUANTILE: f64 = 0.25;

const DECIBELS_A_BUCKET: f32 = 1.0;

const BUCKETS: usize = (-FLOOR_DB / DECIBELS_A_BUCKET) as usize + 1;

const POINTS_AT_48_KHZ: usize = 4_096;
const POINTS_AT_96_KHZ: usize = 8_192;
const POINTS_AT_192_KHZ: usize = 16_384;
const POINTS_ABOVE: usize = 32_768;

pub(crate) const fn points_for(rate: u32) -> usize {
    if rate <= 48_000 {
        POINTS_AT_48_KHZ
    } else if rate <= 96_000 {
        POINTS_AT_96_KHZ
    } else if rate <= 192_000 {
        POINTS_AT_192_KHZ
    } else {
        POINTS_ABOVE
    }
}

fn decibels(power: f64) -> f32 {
    if power > 0.0 {
        (10.0 * power.log10()).max(f64::from(FLOOR_DB)) as f32
    } else {
        FLOOR_DB
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spectrum {
    bin_hz: f32,
    levels: Vec<f32>,
    typical: Vec<f32>,
    heard: Duration,
}

impl Spectrum {
    pub fn new(bin_hz: f32, levels: Vec<f32>, heard: Duration) -> Self {
        Self {
            bin_hz,
            levels,
            typical: Vec::new(),
            heard,
        }
    }

    pub fn with_typical(self, typical: Vec<f32>) -> Self {
        Self { typical, ..self }
    }

    pub fn typical(&self) -> &[f32] {
        &self.typical
    }

    pub const fn bin_hz(&self) -> f32 {
        self.bin_hz
    }

    pub fn levels(&self) -> &[f32] {
        &self.levels
    }

    pub const fn heard(&self) -> Duration {
        self.heard
    }

    pub fn nyquist_hz(&self) -> f32 {
        self.bin_hz * self.levels.len().saturating_sub(1) as f32
    }

    pub fn band_db(&self, from_hz: f32, to_hz: f32) -> Option<f32> {
        let first = (from_hz / self.bin_hz).ceil().max(0.0) as usize;
        let past = ((to_hz / self.bin_hz).ceil().max(0.0) as usize).min(self.levels.len());
        if first >= past {
            return None;
        }
        let power: f64 = self.levels[first..past]
            .iter()
            .map(|level| 10_f64.powf(f64::from(*level) / 10.0))
            .sum();
        Some(decibels(power / (past - first) as f64))
    }
}

pub(crate) struct Transforming {
    points: usize,
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    gathered: Vec<f32>,
    buffer: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    powers: Vec<f32>,
    sums: Vec<f64>,
    bands: Vec<(usize, usize)>,
    counted: Vec<u32>,
    loud: u64,
    rate: u32,
    full_scale_power: f64,
}

impl Transforming {
    pub(crate) fn new(rate: u32) -> Self {
        let points = points_for(rate);
        let fft = FftPlanner::new().plan_fft_forward(points);
        let scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        let window = (0..points)
            .map(|nth| 0.5 - 0.5 * (2.0 * PI * nth as f32 / points as f32).cos())
            .collect();
        let quarter = points as f64 / 4.0;
        let bin_hz = rate as f32 / points as f32;
        let bins = points / 2 + 1;
        let bands: Vec<(usize, usize)> = (0..)
            .map(|band| band as f32 * BAND_HZ)
            .take_while(|from| *from < rate as f32 / 2.0)
            .map(|from| {
                let first = ((from / bin_hz).ceil() as usize).min(bins);
                let past = (((from + BAND_HZ) / bin_hz).ceil() as usize).min(bins);
                (first, past.max(first))
            })
            .collect();
        let counted = vec![0; bands.len() * BUCKETS];
        Self {
            points,
            fft,
            window,
            gathered: Vec::with_capacity(points),
            buffer: vec![Complex::default(); points],
            scratch,
            powers: vec![0.0; points / 2 + 1],
            sums: vec![0.0; points / 2 + 1],
            bands,
            counted,
            loud: 0,
            rate,
            full_scale_power: quarter * quarter,
        }
    }

    pub(crate) const fn points(&self) -> usize {
        self.points
    }

    pub(crate) fn note(&mut self, mono: &[f32], mut each: impl FnMut(&[f32])) {
        let mut rest = mono;
        while !rest.is_empty() {
            let room = self.points - self.gathered.len();
            let (taken, left) = rest.split_at(room.min(rest.len()));
            self.gathered.extend_from_slice(taken);
            rest = left;
            if self.gathered.len() == self.points {
                self.transform();
                each(&self.powers);
                self.gathered.clear();
            }
        }
    }

    fn transform(&mut self) {
        let squares: f64 = self
            .gathered
            .iter()
            .map(|sample| f64::from(*sample) * f64::from(*sample))
            .sum();
        let loud = decibels(squares / self.points as f64) >= SILENT_BELOW_DB;

        for ((slot, sample), weight) in self.buffer.iter_mut().zip(&self.gathered).zip(&self.window)
        {
            *slot = Complex::new(sample * weight, 0.0);
        }
        self.fft
            .process_with_scratch(&mut self.buffer, &mut self.scratch);

        for ((power, sum), bin) in self.powers.iter_mut().zip(&mut self.sums).zip(&self.buffer) {
            let heard = f64::from(bin.norm_sqr()) / self.full_scale_power;
            *power = heard as f32;
            if loud {
                *sum += heard;
            }
        }
        if loud {
            self.loud += 1;
            self.count_the_bands();
        }
    }

    fn count_the_bands(&mut self) {
        for (band, (first, past)) in self.bands.iter().enumerate() {
            let Some(held) = self
                .powers
                .get(*first..*past)
                .filter(|held| !held.is_empty())
            else {
                continue;
            };
            let power = held.iter().map(|power| f64::from(*power)).sum::<f64>() / held.len() as f64;
            let bucket = ((decibels(power) - FLOOR_DB) / DECIBELS_A_BUCKET) as usize;
            self.counted[band * BUCKETS + bucket.min(BUCKETS - 1)] += 1;
        }
    }

    fn typical(&self) -> Vec<f32> {
        let wanted = ((self.loud as f64 * TYPICAL_QUANTILE).ceil() as u64).max(1);
        self.counted
            .as_chunks::<BUCKETS>()
            .0
            .iter()
            .map(|buckets| {
                let mut reached = 0_u64;
                buckets
                    .iter()
                    .position(|count| {
                        reached += u64::from(*count);
                        reached >= wanted
                    })
                    .map_or(FLOOR_DB, |bucket| {
                        FLOOR_DB + bucket as f32 * DECIBELS_A_BUCKET
                    })
            })
            .collect()
    }

    pub(crate) fn finished(self) -> Spectrum {
        let levels = self
            .sums
            .iter()
            .map(|sum| {
                if self.loud == 0 {
                    FLOOR_DB
                } else {
                    decibels(sum / self.loud as f64)
                }
            })
            .collect();
        let heard_frames = self.loud * self.points as u64;
        let typical = if self.loud == 0 {
            Vec::new()
        } else {
            self.typical()
        };

        Spectrum {
            bin_hz: self.rate as f32 / self.points as f32,
            levels,
            typical,
            heard: Duration::from_secs_f64(heard_frames as f64 / f64::from(self.rate)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn transformed(signal: &[f32]) -> Spectrum {
        let mut transforming = Transforming::new(RATE);
        for block in signal.chunks(1_000) {
            transforming.note(block, |_| {});
        }
        transforming.finished()
    }

    fn sine(hz: f32, amplitude: f32, seconds: u32) -> Vec<f32> {
        (0..RATE * seconds)
            .map(|frame| amplitude * (frame as f32 * 2.0 * PI * hz / RATE as f32).sin())
            .collect()
    }

    #[test]
    fn a_full_scale_sine_on_a_bin_reads_zero_decibels_there() {
        let bin_hz = RATE as f32 / points_for(RATE) as f32;
        let spectrum = transformed(&sine(bin_hz * 100.0, 1.0, 2));

        assert_eq!(spectrum.bin_hz(), bin_hz);
        assert!(
            (spectrum.levels()[100]).abs() < 0.01,
            "{}",
            spectrum.levels()[100]
        );
        assert!(spectrum.levels()[400] < -100.0);
        assert!((spectrum.nyquist_hz() - 24_000.0).abs() < 1e-3);
    }

    #[test]
    fn silence_is_left_out_of_the_average_and_heard_counts_only_what_was_loud() {
        let mut signal = vec![0.0; RATE as usize * 4];
        signal.extend(sine(1_000.0, 0.5, 2));
        let spectrum = transformed(&signal);

        let heard = spectrum.heard().as_secs_f32();
        assert!((1.5..=2.2).contains(&heard), "{heard}");
        let at_1k = spectrum.band_db(950.0, 1_050.0).expect("a band");
        assert!(at_1k > -30.0, "{at_1k}");
    }

    #[test]
    fn the_transform_widens_with_the_rate() {
        assert_eq!(points_for(44_100), 4_096);
        assert_eq!(points_for(96_000), 8_192);
        assert_eq!(points_for(176_400), 16_384);
        assert_eq!(points_for(384_000), 32_768);
    }
}
