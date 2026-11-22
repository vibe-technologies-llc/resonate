use std::fmt;

use resonate_core::ChannelLayout;
use rubato::{
    Async, Fft, FixedAsync, FixedSync, Indexing, PolynomialDegree, Resampler,
    SincInterpolationParameters, SincInterpolationType, WindowFunction,
    audioadapter_buffers::direct::InterleavedSlice,
};

use crate::{
    error::{Error, Result},
    lab::{
        pair::Pair,
        subject::{Family, Running, Subject, SubjectName},
    },
};

const RATIO_HELD: f64 = 1.0;
const OVERSAMPLED_BY: usize = 256;
const EMPTY_PUMPS_AT_MOST: usize = 4_096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rubato {
    Fft,
    Sinc {
        taps: usize,
        interpolation: Interpolation,
    },
    Polynomial,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interpolation {
    Cubic,
    Linear,
}

pub const SETTINGS: [Rubato; 4] = [
    Rubato::Fft,
    Rubato::Sinc {
        taps: 256,
        interpolation: Interpolation::Cubic,
    },
    Rubato::Sinc {
        taps: 128,
        interpolation: Interpolation::Linear,
    },
    Rubato::Polynomial,
];

impl fmt::Display for Rubato {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fft => f.write_str("rubato FFT"),
            Self::Sinc {
                taps,
                interpolation,
            } => {
                let interpolation = match interpolation {
                    Interpolation::Cubic => "cubic",
                    Interpolation::Linear => "linear",
                };
                write!(f, "rubato sinc {taps}, {interpolation}")
            }
            Self::Polynomial => f.write_str("rubato polynomial, cubic"),
        }
    }
}

impl Rubato {
    fn built(
        self,
        pair: Pair,
        channels: usize,
        block: usize,
    ) -> std::result::Result<Box<dyn Resampler<f64>>, rubato::ResamplerConstructionError> {
        let ratio = pair.output_hz() / pair.input_hz();

        Ok(match self {
            Self::Fft => Box::new(Fft::<f64>::new(
                pair.from.hz() as usize,
                pair.to.hz() as usize,
                block,
                channels,
                FixedSync::Input,
            )?),
            Self::Sinc {
                taps,
                interpolation,
            } => {
                let interpolation = match interpolation {
                    Interpolation::Cubic => SincInterpolationType::Cubic,
                    Interpolation::Linear => SincInterpolationType::Linear,
                };
                let parameters =
                    SincInterpolationParameters::new(taps, WindowFunction::BlackmanHarris2)
                        .oversampling_factor(OVERSAMPLED_BY)
                        .interpolation(interpolation);

                Box::new(Async::<f64>::new_sinc(
                    ratio,
                    RATIO_HELD,
                    &parameters,
                    block,
                    channels,
                    FixedAsync::Input,
                )?)
            }
            Self::Polynomial => Box::new(Async::<f64>::new_poly(
                ratio,
                RATIO_HELD,
                PolynomialDegree::Cubic,
                block,
                channels,
                FixedAsync::Input,
            )?),
        })
    }
}

impl Subject for Rubato {
    fn name(&self) -> SubjectName {
        SubjectName(self.to_string())
    }

    fn family(&self) -> Family {
        Family::Reference
    }

    fn supports(&self, pair: Pair) -> bool {
        pair.converts()
    }

    fn open(&self, pair: Pair, layout: ChannelLayout, block: usize) -> Result<Box<dyn Running>> {
        let channels = usize::from(layout.count().get());
        let resampler = self
            .built(pair, channels, block)
            .map_err(|source| Error::RubatoBuilt {
                subject: self.name(),
                pair,
                source,
            })?;
        let to_trim = resampler.output_delay();

        let room = resampler.output_frames_max() * channels;

        Ok(Box::new(Streamed {
            resampler,
            subject: self.name(),
            pair,
            channels,
            pending: Vec::new(),
            scratch: vec![0.0; room],
            to_trim,
            frames_in: 0,
            emitted: 0,
        }))
    }
}

struct Streamed {
    resampler: Box<dyn Resampler<f64>>,
    subject: SubjectName,
    pair: Pair,
    channels: usize,
    pending: Vec<f64>,
    scratch: Vec<f64>,
    to_trim: usize,
    frames_in: usize,
    emitted: usize,
}

impl Streamed {
    fn unframed(&self) -> Error {
        Error::RubatoBuffer {
            subject: self.subject.clone(),
            pair: self.pair,
        }
    }

    fn chunk(&mut self, partial: Option<usize>) -> Result<(usize, usize)> {
        let held = self.pending.len() / self.channels;
        let input = InterleavedSlice::new(&self.pending[..], self.channels, held)
            .map_err(|_| self.unframed())?;
        let capacity = self.scratch.len() / self.channels;
        let mut output = InterleavedSlice::new_mut(&mut self.scratch[..], self.channels, capacity)
            .map_err(|_| Error::RubatoBuffer {
                subject: self.subject.clone(),
                pair: self.pair,
            })?;
        let indexing = partial.map(|frames| Indexing::new().partial_len(frames));

        self.resampler
            .process_into_buffer(&input, &mut output, indexing.as_ref())
            .map_err(|source| Error::RubatoRan {
                subject: self.subject.clone(),
                pair: self.pair,
                source,
            })
    }

    fn emit(&mut self, frames: usize, wanted: Option<usize>, output: &mut Vec<f64>) {
        let trimmed = frames.min(self.to_trim);
        let mut kept = frames - trimmed;

        self.to_trim -= trimmed;
        if let Some(wanted) = wanted {
            kept = kept.min(wanted.saturating_sub(self.emitted));
        }
        output.extend_from_slice(
            &self.scratch[trimmed * self.channels..(trimmed + kept) * self.channels],
        );
        self.emitted += kept;
    }
}

impl Running for Streamed {
    fn push(&mut self, input: &[f64], output: &mut Vec<f64>) -> Result<()> {
        self.pending.extend_from_slice(input);
        self.frames_in += input.len() / self.channels;

        while self.pending.len() / self.channels >= self.resampler.input_frames_next() {
            let (taken, made) = self.chunk(None)?;

            self.pending.drain(..taken * self.channels);
            self.emit(made, None, output);
        }
        Ok(())
    }

    fn finish(&mut self, output: &mut Vec<f64>) -> Result<()> {
        let wanted =
            (self.frames_in as f64 * self.pair.output_hz() / self.pair.input_hz()).ceil() as usize;
        let held = self.pending.len() / self.channels;

        if held > 0 {
            let (_, made) = self.chunk(Some(held))?;

            self.pending.clear();
            self.emit(made, Some(wanted), output);
        }

        let mut empty_in_a_row = 0;

        while self.emitted < wanted {
            let (_, made) = self.chunk(Some(0))?;

            empty_in_a_row = if made == 0 { empty_in_a_row + 1 } else { 0 };
            if empty_in_a_row > EMPTY_PUMPS_AT_MOST {
                return Err(Error::RubatoStalled {
                    subject: self.subject.clone(),
                    pair: self.pair,
                });
            }
            self.emit(made, Some(wanted), output);
        }
        Ok(())
    }

    fn channels_out(&self) -> usize {
        self.channels
    }
}

#[cfg(test)]
mod tests {
    use resonate_core::SampleRate;

    use super::*;
    use crate::lab::subject::resampled;

    #[test]
    fn every_setting_gives_as_many_frames_as_the_ratio_owes() {
        let input: Vec<f64> = (0..48_000)
            .map(|index| (f64::from(index) * 0.01).sin())
            .collect();
        let pairs = [
            Pair::new(SampleRate::HZ_48000, SampleRate::HZ_44100),
            Pair::new(SampleRate::HZ_44100, SampleRate::HZ_48000),
            Pair::new(SampleRate::HZ_192000, SampleRate::HZ_48000),
        ];

        for setting in SETTINGS {
            for pair in pairs {
                for block in [1, 1_024] {
                    let heard = resampled(&setting, pair, &input, block);
                    let owed = (input.len() as f64 * pair.output_hz() / pair.input_hz()).ceil();

                    assert_eq!(
                        heard
                            .map(|heard| heard.len())
                            .map_err(|error| error.to_string()),
                        Ok(owed as usize),
                        "{setting} at {pair}, blocks of {block}"
                    );
                }
            }
        }
    }
}
