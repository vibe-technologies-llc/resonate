use resonate_core::{SampleData, SampleFormat};

const CLIPPED_RUN: u32 = 3;

const WITHIN_A_SIXTEEN_BIT_STEP_OF_FULL_SCALE: f32 = 1.0 - 1.0 / 32_768.0;

const SIXTEEN_BIT_GRID: f32 = 32_768.0;

const TWENTY_FOUR_BIT_GRID: f32 = 8_388_608.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stereo {
    pub identical: bool,
    pub correlation: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Levels {
    pub peak: f32,
    pub rms: f32,
    pub dc: f32,
    pub clipped: u64,
    pub clipped_runs: u64,
    pub bits_in_use: Option<u8>,
    pub stereo: Option<Stereo>,
}

#[derive(Clone, Copy, Debug, Default)]
struct Channel {
    peak: f32,
    sum: f64,
    run: u32,
}

#[derive(Clone, Copy, Debug)]
struct Pairing {
    identical: bool,
    left_right: f64,
    left_left: f64,
    right_right: f64,
}

pub(crate) struct Leveling {
    channels: Vec<Channel>,
    squares: f64,
    samples: u64,
    clipped: u64,
    clipped_runs: u64,
    ored: u32,
    on_sixteen: bool,
    on_twenty_four: bool,
    sounded: bool,
    format: SampleFormat,
    pairing: Option<Pairing>,
}

impl Leveling {
    pub(crate) fn new(channels: usize, format: SampleFormat) -> Self {
        Self {
            channels: vec![Channel::default(); channels.max(1)],
            squares: 0.0,
            samples: 0,
            clipped: 0,
            clipped_runs: 0,
            ored: 0,
            on_sixteen: true,
            on_twenty_four: true,
            sounded: false,
            format,
            pairing: (channels == 2).then_some(Pairing {
                identical: true,
                left_right: 0.0,
                left_left: 0.0,
                right_right: 0.0,
            }),
        }
    }

    pub(crate) fn note(&mut self, native: &SampleData, normalised: &[f32]) {
        self.note_the_bits(native);
        self.note_the_levels(normalised);
    }

    fn note_the_bits(&mut self, native: &SampleData) {
        match native {
            SampleData::S16(samples) => self.note_integers(samples, i32::from),
            SampleData::S24(samples) | SampleData::S32(samples) => {
                self.note_integers(samples, |sample| sample);
            }
            SampleData::F32(samples) => {
                for sample in samples {
                    if *sample != 0.0 {
                        self.sounded = true;
                    }
                    self.on_sixteen &= on_the_grid(*sample, SIXTEEN_BIT_GRID);
                    self.on_twenty_four &= on_the_grid(*sample, TWENTY_FOUR_BIT_GRID);
                }
                if let Some(pairing) = &mut self.pairing {
                    pairing.identical &= samples
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .all(|[left, right]| left.to_bits() == right.to_bits());
                }
            }
        }
    }

    fn note_integers<T: Copy>(&mut self, samples: &[T], widened: impl Fn(T) -> i32) {
        let Some(pairing) = &mut self.pairing else {
            for sample in samples {
                self.ored |= widened(*sample).cast_unsigned();
            }
            return;
        };

        let (frames, unpaired) = samples.as_chunks::<2>();
        let mut identical = pairing.identical;
        for [left, right] in frames {
            let (left, right) = (widened(*left), widened(*right));
            self.ored |= (left | right).cast_unsigned();
            identical &= left == right;
        }
        for sample in unpaired {
            self.ored |= widened(*sample).cast_unsigned();
        }
        pairing.identical = identical;
    }

    fn note_the_levels(&mut self, normalised: &[f32]) {
        let width = self.channels.len();
        for frame in normalised.chunks_exact(width) {
            for (channel, sample) in self.channels.iter_mut().zip(frame) {
                let size = sample.abs();
                channel.peak = channel.peak.max(size);
                channel.sum += f64::from(*sample);
                if size >= WITHIN_A_SIXTEEN_BIT_STEP_OF_FULL_SCALE {
                    channel.run += 1;
                    if channel.run == CLIPPED_RUN {
                        self.clipped += u64::from(CLIPPED_RUN);
                        self.clipped_runs += 1;
                    } else if channel.run > CLIPPED_RUN {
                        self.clipped += 1;
                    }
                } else {
                    channel.run = 0;
                }
                self.squares += f64::from(*sample) * f64::from(*sample);
            }
            if let (Some(pairing), [left, right]) = (&mut self.pairing, frame) {
                let (left, right) = (f64::from(*left), f64::from(*right));
                pairing.left_right += left * right;
                pairing.left_left += left * left;
                pairing.right_right += right * right;
            }
            self.samples += width as u64;
        }
    }

    fn bits_in_use(&self) -> Option<u8> {
        if self.format.is_float() {
            if !self.sounded {
                return None;
            }
            return if self.on_sixteen {
                Some(16)
            } else if self.on_twenty_four {
                Some(24)
            } else {
                None
            };
        }
        if self.ored == 0 {
            return None;
        }
        let container = self.format.valid_bits();
        let unused = (self.ored.trailing_zeros() as u8).min(container);
        Some(container - unused)
    }

    pub(crate) fn finished(&self) -> Levels {
        let frames = self.samples / self.channels.len() as u64;
        let dc = if frames == 0 {
            0.0
        } else {
            self.channels
                .iter()
                .map(|channel| (channel.sum / frames as f64).abs())
                .fold(0.0, f64::max) as f32
        };

        Levels {
            peak: self
                .channels
                .iter()
                .map(|channel| channel.peak)
                .fold(0.0, f32::max),
            rms: if self.samples == 0 {
                0.0
            } else {
                (self.squares / self.samples as f64).sqrt() as f32
            },
            dc,
            clipped: self.clipped,
            clipped_runs: self.clipped_runs,
            bits_in_use: self.bits_in_use(),
            stereo: self.pairing.map(|pairing| Stereo {
                identical: pairing.identical && frames > 0,
                correlation: {
                    let energy = (pairing.left_left * pairing.right_right).sqrt();
                    (energy > 0.0).then(|| (pairing.left_right / energy) as f32)
                },
            }),
        }
    }
}

fn on_the_grid(sample: f32, grid: f32) -> bool {
    let scaled = sample * grid;
    sample.abs() <= 1.0 && scaled == scaled.trunc()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normalised(data: &SampleData) -> Vec<f32> {
        let scale = data.format().full_scale();
        match data {
            SampleData::S16(samples) => samples.iter().map(|s| f32::from(*s) / scale).collect(),
            SampleData::S24(samples) | SampleData::S32(samples) => {
                samples.iter().map(|s| *s as f32 / scale).collect()
            }
            SampleData::F32(samples) => samples.clone(),
        }
    }

    fn leveled(channels: usize, data: SampleData) -> Levels {
        let mut leveling = Leveling::new(channels, data.format());
        let normalised = normalised(&data);
        leveling.note(&data, &normalised);
        leveling.finished()
    }

    fn sine(frames: usize, amplitude: f32) -> impl Iterator<Item = f32> {
        (0..frames).map(move |frame| {
            amplitude * (frame as f32 * 2.0 * std::f32::consts::PI * 1_000.0 / 48_000.0).sin()
        })
    }

    #[test]
    fn a_sine_reads_its_peak_and_its_rms() {
        let samples: Vec<f32> = sine(48_000, 0.5).collect();
        let levels = leveled(1, SampleData::F32(samples));

        assert!((levels.peak - 0.5).abs() < 1e-3);
        assert!((levels.rms - 0.5 / 2.0_f32.sqrt()).abs() < 1e-3);
        assert!(levels.dc < 1e-4);
        assert_eq!(levels.clipped, 0);
        assert_eq!(levels.bits_in_use, None);
        assert_eq!(levels.stereo, None);
    }

    #[test]
    fn sixteen_bit_samples_shifted_into_twenty_four_read_as_sixteen_bits_in_use() {
        let samples: Vec<i32> = sine(4_800, 0.8)
            .map(|sample| ((sample * 32_767.0) as i32) << 8)
            .collect();
        assert_eq!(leveled(1, SampleData::S24(samples)).bits_in_use, Some(16));

        let whole: Vec<i32> = sine(4_800, 0.8)
            .map(|sample| (sample * 8_388_607.0) as i32)
            .collect();
        assert_eq!(leveled(1, SampleData::S24(whole)).bits_in_use, Some(24));
    }

    #[test]
    fn float_samples_on_the_sixteen_bit_grid_read_as_sixteen_bits() {
        let samples: Vec<f32> = sine(4_800, 0.8)
            .map(|sample| (sample * 32_768.0).round() / 32_768.0)
            .collect();
        assert_eq!(leveled(1, SampleData::F32(samples)).bits_in_use, Some(16));
        assert_eq!(leveled(1, SampleData::F32(vec![0.0; 64])).bits_in_use, None);
    }

    #[test]
    fn a_run_held_at_full_scale_is_clipping_and_a_lone_peak_is_not() {
        let mut samples = vec![0_i16; 100];
        samples[10] = i16::MAX;
        samples[40..45].fill(i16::MAX);
        samples[60..63].fill(i16::MIN);
        let levels = leveled(1, SampleData::S16(samples));

        assert_eq!(levels.clipped, 8);
        assert_eq!(levels.clipped_runs, 2);
    }

    #[test]
    fn the_same_samples_in_both_channels_are_mono_stored_as_stereo() {
        let mono: Vec<i16> = sine(4_800, 0.5)
            .flat_map(|sample| {
                let held = (sample * 32_767.0) as i16;
                [held, held]
            })
            .collect();
        let levels = leveled(2, SampleData::S16(mono));
        let stereo = levels.stereo.expect("a stereo reading");
        assert!(stereo.identical);
        assert!((stereo.correlation.expect("a correlation") - 1.0).abs() < 1e-6);

        let opposed: Vec<i16> = sine(4_800, 0.5)
            .flat_map(|sample| {
                let held = (sample * 32_767.0) as i16;
                [held, -held]
            })
            .collect();
        let stereo = leveled(2, SampleData::S16(opposed))
            .stereo
            .expect("a stereo reading");
        assert!(!stereo.identical);
        assert!((stereo.correlation.expect("a correlation") + 1.0).abs() < 1e-6);
    }

    #[test]
    fn an_offset_signal_reads_its_offset() {
        let samples: Vec<f32> = sine(48_000, 0.25).map(|sample| sample + 0.1).collect();
        assert!((leveled(1, SampleData::F32(samples)).dc - 0.1).abs() < 1e-3);
    }
}
