use resonate_core::StreamSpec;

use crate::{ProcessCount, Processor, Result};

const FULL_SCALE: f64 = 1.0;

fn past_full_scale(samples: &[f64]) -> u64 {
    samples
        .iter()
        .filter(|sample| sample.abs() > FULL_SCALE)
        .count() as u64
}

pub struct Overs {
    channels: usize,
    counted: u64,
}

impl Overs {
    pub const fn new() -> Self {
        Self {
            channels: 1,
            counted: 0,
        }
    }
}

impl Default for Overs {
    fn default() -> Self {
        Self::new()
    }
}

impl Processor for Overs {
    fn prepare(&mut self, spec: StreamSpec, max_frames_in: usize) -> Result<usize> {
        self.channels = spec.channel_count().get() as usize;
        Ok(max_frames_in)
    }

    fn reset(&mut self) {}

    fn latency_frames(&self) -> f64 {
        0.0
    }

    fn is_transparent(&self) -> bool {
        false
    }

    fn overs(&self) -> u64 {
        self.counted
    }

    fn process(&mut self, input: &[f64], output: &mut [f64]) -> ProcessCount {
        let channels = self.channels;
        let frames = (input.len() / channels).min(output.len() / channels);
        let samples = frames * channels;
        let (Some(taken), Some(made)) = (input.get(..samples), output.get_mut(..samples)) else {
            return ProcessCount {
                frames_in: 0,
                frames_out: 0,
            };
        };

        made.copy_from_slice(taken);
        self.counted = self.counted.saturating_add(past_full_scale(taken));

        ProcessCount {
            frames_in: frames,
            frames_out: frames,
        }
    }

    fn flush(&mut self, _output: &mut [f64]) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use resonate_core::{ChannelLayout, SampleFormat, SampleRate};

    use super::*;

    fn prepared() -> Overs {
        let mut stage = Overs::new();
        stage
            .prepare(
                StreamSpec::new(
                    SampleRate::HZ_48000,
                    ChannelLayout::Stereo,
                    SampleFormat::F32,
                ),
                512,
            )
            .expect("a counter always prepares");
        stage
    }

    #[test]
    fn samples_past_full_scale_are_counted_and_handed_on_unchanged() {
        let mut stage = prepared();

        let input = [0.5, -1.0, 1.0, 1.25, -2.0, 0.0];
        let mut output = [0.0; 6];
        let count = stage.process(&input, &mut output);

        assert_eq!(output, input);
        assert_eq!(count.frames_out, 3);
        assert_eq!(stage.overs(), 2);
    }

    #[test]
    fn the_count_keeps_growing_across_blocks_and_a_reset() {
        let mut stage = prepared();
        let mut output = [0.0; 2];

        stage.process(&[1.5, 0.0], &mut output);
        stage.reset();
        stage.process(&[0.0, -1.5], &mut output);

        assert_eq!(stage.overs(), 2);
    }

    #[test]
    fn a_stream_that_stays_inside_full_scale_counts_nothing() {
        let mut stage = prepared();
        let mut output = [0.0; 4];

        stage.process(&[1.0, -1.0, 0.99, f64::NAN], &mut output);

        assert_eq!(stage.overs(), 0);
    }
}
