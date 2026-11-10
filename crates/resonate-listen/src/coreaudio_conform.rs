use std::num::NonZeroUsize;

use resonate_core::{ChannelLayout, SampleRate};
use resonate_dsp::{FilterPhase, Processor, Quality, Resampler, ResamplerConfig};

use crate::session::{HEARD_AT, HEARD_IN_CHANNELS};

const CONFORMED_IN_FRAMES: usize = 4_096;

pub(crate) struct Conforming {
    channels: NonZeroUsize,
    resampler: Option<Resampler>,
    stereo: Vec<f64>,
    scratch: Vec<f64>,
}

impl Conforming {
    pub(crate) fn of(rate: SampleRate, channels: NonZeroUsize) -> resonate_dsp::Result<Self> {
        let resampler = if rate == HEARD_AT {
            None
        } else {
            Some(Resampler::new(ResamplerConfig {
                input_rate: rate,
                output_rate: HEARD_AT,
                channels: ChannelLayout::Stereo,
                quality: Quality::LowLatency,
                phase: FilterPhase::Linear,
                max_frames_in: CONFORMED_IN_FRAMES,
            })?)
        };
        let scratch = resampler.as_ref().map_or(0, |resampler| {
            resampler.max_output_frames(CONFORMED_IN_FRAMES) * HEARD_IN_CHANNELS
        });
        Ok(Self {
            channels,
            resampler,
            stereo: Vec::new(),
            scratch: vec![0.0; scratch],
        })
    }

    pub(crate) fn conformed(&mut self, heard: &[f32]) -> Vec<u8> {
        self.stereo.clear();
        for frame in heard.chunks_exact(self.channels.get()) {
            let left = frame.first().copied().unwrap_or(0.0);
            let right = frame.get(1).copied().unwrap_or(left);
            self.stereo.push(f64::from(left));
            self.stereo.push(f64::from(right));
        }
        let mut bytes = Vec::with_capacity(self.stereo.len() * size_of::<f32>());
        let mut keep = |samples: &[f64]| {
            for sample in samples {
                bytes.extend_from_slice(&(*sample as f32).to_le_bytes());
            }
        };
        match &mut self.resampler {
            None => keep(&self.stereo),
            Some(resampler) => {
                for block in self.stereo.chunks(CONFORMED_IN_FRAMES * HEARD_IN_CHANNELS) {
                    let made = resampler.process(block, &mut self.scratch).frames_out;
                    keep(
                        self.scratch
                            .get(..made * HEARD_IN_CHANNELS)
                            .unwrap_or_default(),
                    );
                }
            }
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples(bytes: &[u8]) -> Vec<f32> {
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| f32::from_le_bytes(*word))
            .collect()
    }

    fn channels(count: usize) -> NonZeroUsize {
        NonZeroUsize::new(count).expect("a channel count")
    }

    #[test]
    fn a_mono_microphone_is_heard_in_both_ears() {
        let mut conforming = Conforming::of(HEARD_AT, channels(1)).expect("conforming");

        let heard = samples(&conforming.conformed(&[0.25, -0.5]));

        assert_eq!(heard, [0.25, 0.25, -0.5, -0.5]);
    }

    #[test]
    fn an_interface_of_many_channels_is_heard_by_its_first_two() {
        let mut conforming = Conforming::of(HEARD_AT, channels(4)).expect("conforming");

        let heard = samples(&conforming.conformed(&[0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8]));

        assert_eq!(heard, [0.1, 0.2, 0.5, 0.6]);
    }

    #[test]
    fn a_microphone_at_another_rate_is_heard_at_the_rate_a_clip_is_kept_at() {
        let mut conforming = Conforming::of(SampleRate::HZ_44100, channels(1)).expect("conforming");
        let second: Vec<f32> = (0..44_100)
            .map(|frame| (frame as f32 * 0.05).sin() * 0.5)
            .collect();

        let heard = samples(&conforming.conformed(&second));

        let frames = heard.len() / HEARD_IN_CHANNELS;
        assert!((47_000..=48_000).contains(&frames), "{frames}");
        assert!(heard.iter().any(|sample| sample.abs() > 0.4));
        assert!(heard.iter().all(|sample| sample.abs() < 0.6));
    }
}
