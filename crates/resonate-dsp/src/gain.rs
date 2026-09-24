use std::time::Duration;

use resonate_core::{AppliedGain, Gain, StreamSpec, Volume};

use crate::{ProcessCount, Processor, Result};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ReplayGainMode {
    #[default]
    Off,
    Track,
    Album,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GainConfig {
    pub volume: Volume,
    pub replay_gain: AppliedGain,
    pub prevent_clipping: bool,
    pub ramp: Duration,
}

impl Default for GainConfig {
    fn default() -> Self {
        Self {
            volume: Volume::MAX,
            replay_gain: AppliedGain::default(),
            prevent_clipping: true,
            ramp: Duration::from_millis(20),
        }
    }
}

impl GainConfig {
    fn amplitude(&self) -> f32 {
        let replay_gain = if self.prevent_clipping {
            self.replay_gain.applied()
        } else {
            self.replay_gain.requested()
        };
        self.volume.to_gain().get() * replay_gain.get()
    }

    fn limits_every_sample(&self) -> bool {
        self.prevent_clipping
            && self.replay_gain.peak.is_none()
            && self.amplitude() > Gain::UNITY.get()
    }
}

pub struct GainStage {
    config: GainConfig,
    channels: usize,
    current: f32,
    target: f32,
    ramp_frames: usize,
    remaining: usize,
    increment: f32,
    limits: bool,
}

impl GainStage {
    pub fn new(config: GainConfig) -> Self {
        let amplitude = config.amplitude();
        let limits = config.limits_every_sample();
        Self {
            config,
            channels: 1,
            current: amplitude,
            target: amplitude,
            ramp_frames: 0,
            remaining: 0,
            increment: 0.0,
            limits,
        }
    }

    #[cfg(test)]
    pub fn set_volume(&mut self, volume: Volume) {
        self.config.volume = volume;
        self.retarget();
    }

    pub const fn amplitude(&self) -> f32 {
        self.current
    }

    fn retarget(&mut self) {
        self.target = self.config.amplitude();
        self.limits = self.config.limits_every_sample();
        if self.ramp_frames == 0 || self.target == self.current {
            self.current = self.target;
            self.remaining = 0;
            return;
        }
        self.remaining = self.ramp_frames;
        self.increment = (self.target - self.current) / self.ramp_frames as f32;
    }

    fn ramp(&mut self, input: &[f64], output: &mut [f64], frames: usize) {
        let channels = self.channels;
        let limits = self.limits;

        for frame in 0..frames {
            let amplitude = f64::from(self.advance());
            for channel in 0..channels {
                let index = frame * channels + channel;
                let sample = input.get(index).copied().unwrap_or_default() * amplitude;
                if let Some(slot) = output.get_mut(index) {
                    *slot = if limits {
                        sample.clamp(-1.0, 1.0)
                    } else {
                        sample
                    };
                }
            }
        }
    }

    fn hold(&mut self, input: &[f64], output: &mut [f64], from: usize, frames: usize) {
        if from >= frames {
            return;
        }
        let channels = self.channels;
        let amplitude = f64::from(self.advance());
        let from = from * channels;
        let until = frames * channels;
        let (Some(taken), Some(made)) = (input.get(from..until), output.get_mut(from..until))
        else {
            return;
        };

        if self.limits {
            for (sample, slot) in taken.iter().zip(made.iter_mut()) {
                *slot = (sample * amplitude).clamp(-1.0, 1.0);
            }
        } else {
            for (sample, slot) in taken.iter().zip(made.iter_mut()) {
                *slot = sample * amplitude;
            }
        }
    }

    fn advance(&mut self) -> f32 {
        if self.remaining == 0 {
            self.current = self.target;
            return self.current;
        }
        self.current += self.increment;
        self.remaining -= 1;
        if self.remaining == 0 {
            self.current = self.target;
        }
        self.current
    }
}

impl Processor for GainStage {
    fn prepare(&mut self, spec: StreamSpec, max_frames_in: usize) -> Result<usize> {
        self.channels = spec.channel_count().get() as usize;
        self.ramp_frames =
            (self.config.ramp.as_secs_f64() * f64::from(spec.rate.hz())).round() as usize;
        Ok(max_frames_in)
    }

    fn reset(&mut self) {
        self.current = self.target;
        self.remaining = 0;
    }

    fn latency_frames(&self) -> f64 {
        0.0
    }

    fn is_transparent(&self) -> bool {
        false
    }

    fn set_gain(&mut self, volume: Volume, replay_gain: AppliedGain) {
        self.config.volume = volume;
        self.config.replay_gain = replay_gain;
        self.retarget();
    }

    fn gain_amplitude(&self) -> Option<f32> {
        Some(self.current)
    }

    fn ramp_gain_from(&mut self, amplitude: f32) {
        self.current = amplitude;
        self.retarget();
    }

    fn is_ramping(&self) -> bool {
        self.remaining > 0
    }

    fn process(&mut self, input: &[f64], output: &mut [f64]) -> ProcessCount {
        let channels = self.channels;
        let frames = (input.len() / channels).min(output.len() / channels);
        let steady = self.remaining.min(frames);

        self.ramp(input, output, steady);
        self.hold(input, output, steady, frames);

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
    use resonate_core::{ChannelLayout, Decibels, SampleFormat, SampleRate};

    use super::*;

    fn spec(channels: ChannelLayout) -> StreamSpec {
        StreamSpec::new(SampleRate::HZ_48000, channels, SampleFormat::F32)
    }

    fn prepared(config: GainConfig) -> GainStage {
        let mut stage = GainStage::new(config);
        stage
            .prepare(spec(ChannelLayout::Stereo), 512)
            .expect("gain always prepares");
        stage
    }

    fn fitting_under(peak: f32) -> GainConfig {
        GainConfig {
            replay_gain: AppliedGain {
                gain: None,
                peak: Some(resonate_core::Gain::new(peak).expect("in range")),
            },
            ramp: Duration::ZERO,
            ..GainConfig::default()
        }
    }

    #[test]
    fn a_gain_stage_is_never_transparent_so_the_plan_alone_decides_whether_it_runs() {
        let attenuated = GainConfig {
            volume: Volume::new(0.5).expect("in range"),
            ..GainConfig::default()
        };

        for config in [GainConfig::default(), fitting_under(0.98), attenuated] {
            assert!(
                !GainStage::new(config).is_transparent(),
                "{config:?} offered the builder a reason to drop a stage the plan asked for"
            );
        }
    }

    #[test]
    fn a_declared_peak_that_fits_passes_the_samples_through_unchanged() {
        let mut stage = prepared(fitting_under(0.98));

        let input = [0.98, -0.98, 0.25, -0.5];
        let mut output = [0.0; 4];
        stage.process(&input, &mut output);

        assert_eq!(output, input);
    }

    #[test]
    fn a_half_volume_slider_attenuates_by_its_cubic_curve() {
        let config = GainConfig {
            volume: Volume::new(0.5).expect("in range"),
            ramp: Duration::ZERO,
            ..GainConfig::default()
        };
        let mut stage = prepared(config);

        let input = [1.0, 1.0, 1.0, 1.0];
        let mut output = [0.0; 4];
        stage.process(&input, &mut output);

        for sample in output {
            assert!((sample - 0.125).abs() < 1e-6, "got {sample}");
        }
    }

    #[test]
    fn a_volume_change_ramps_rather_than_stepping() {
        let mut stage = prepared(GainConfig {
            ramp: Duration::from_millis(10),
            ..GainConfig::default()
        });
        stage.set_volume(Volume::MUTE);

        let input = vec![1.0; 64];
        let mut output = vec![0.0; 64];
        stage.process(&input, &mut output);

        let first = output.first().copied().expect("non-empty");
        let last = output.last().copied().expect("non-empty");
        assert!(first < 1.0 && first > 0.9, "ramp jumped to {first}");
        assert!(last < first, "ramp did not descend: {first} -> {last}");
        assert!(last > 0.0, "ramp collapsed to {last} in one block");
    }

    #[test]
    fn a_ramp_split_across_blocks_is_the_ramp_heard_in_one() {
        let ramped = || {
            let mut stage = prepared(GainConfig {
                ramp: Duration::from_millis(10),
                ..GainConfig::default()
            });
            stage.set_volume(Volume::MUTE);
            stage
        };
        let input = vec![1.0; 2 * 960];

        let mut whole = vec![0.0; input.len()];
        ramped().process(&input, &mut whole);

        let mut stage = ramped();
        let mut split = vec![0.0; input.len()];
        for (taken, made) in input.chunks(2 * 64).zip(split.chunks_mut(2 * 64)) {
            stage.process(taken, made);
        }

        assert_eq!(split, whole);
    }

    #[test]
    fn a_zero_ramp_applies_the_new_volume_immediately() {
        let mut stage = prepared(GainConfig {
            ramp: Duration::ZERO,
            ..GainConfig::default()
        });
        stage.set_volume(Volume::MUTE);

        let input = [1.0, 1.0];
        let mut output = [0.0; 2];
        stage.process(&input, &mut output);

        assert_eq!(output, [0.0, 0.0]);
    }

    fn boost(db: f32, peak: Option<f32>) -> AppliedGain {
        AppliedGain {
            gain: Some(Decibels::new(db).expect("finite")),
            peak: peak.map(|peak| resonate_core::Gain::new(peak).expect("in range")),
        }
    }

    #[test]
    fn a_boost_with_no_declared_peak_is_limited_sample_by_sample() {
        let mut stage = prepared(GainConfig {
            replay_gain: boost(12.0, None),
            ramp: Duration::ZERO,
            prevent_clipping: true,
            ..GainConfig::default()
        });

        let input = [0.9, -0.9];
        let mut output = [0.0; 2];
        stage.process(&input, &mut output);

        assert_eq!(output, [1.0, -1.0]);
    }

    #[test]
    fn a_stage_at_unity_or_below_passes_an_over_already_in_the_signal_through() {
        let unity = prepared(GainConfig {
            ramp: Duration::ZERO,
            ..GainConfig::default()
        });
        let attenuating = prepared(GainConfig {
            volume: Volume::new(0.5).expect("in range"),
            ramp: Duration::ZERO,
            ..GainConfig::default()
        });

        for (mut stage, fed) in [(unity, 1.2), (attenuating, 9.6)] {
            let mut output = [0.0; 2];
            stage.process(&[fed, -fed], &mut output);

            assert!(
                (output[0] - 1.2).abs() < 1e-6 && (output[1] + 1.2).abs() < 1e-6,
                "a stage that boosts nothing clamped an over it did not make: {output:?}"
            );
        }
    }

    #[test]
    fn a_declared_peak_pulls_the_boost_down_instead_of_clipping_the_waveform() {
        let mut stage = prepared(GainConfig {
            replay_gain: boost(12.0, Some(0.9)),
            ramp: Duration::ZERO,
            prevent_clipping: true,
            ..GainConfig::default()
        });

        let input = [0.9, -0.9, 0.45, -0.45];
        let mut output = [0.0; 4];
        stage.process(&input, &mut output);

        assert!((output[0] - 1.0).abs() < 1e-6, "got {}", output[0]);
        assert!((output[1] + 1.0).abs() < 1e-6, "got {}", output[1]);
        assert!(
            (output[2] - 0.5).abs() < 1e-6,
            "a quiet sample was distorted along with the loud one: {}",
            output[2]
        );
    }

    #[test]
    fn turning_clip_prevention_off_lets_the_boost_through_whole() {
        let mut stage = prepared(GainConfig {
            replay_gain: boost(12.0, Some(0.9)),
            ramp: Duration::ZERO,
            prevent_clipping: false,
            ..GainConfig::default()
        });

        let input = [0.9, 0.9];
        let mut output = [0.0; 2];
        stage.process(&input, &mut output);

        assert!(
            output[0] > 3.5,
            "the boost was capped anyway: {}",
            output[0]
        );
    }

    #[test]
    fn a_source_that_already_passes_full_scale_is_attenuated_rather_than_clipped() {
        let config = GainConfig {
            replay_gain: AppliedGain {
                gain: None,
                peak: Some(resonate_core::Gain::new(1.25).expect("in range")),
            },
            ramp: Duration::ZERO,
            prevent_clipping: true,
            ..GainConfig::default()
        };
        assert!(!GainStage::new(config).is_transparent());

        let mut stage = prepared(config);
        let input = [1.25, -1.25];
        let mut output = [0.0; 2];
        stage.process(&input, &mut output);

        assert!((output[0] - 1.0).abs() < 1e-6, "got {}", output[0]);
        assert!((output[1] + 1.0).abs() < 1e-6, "got {}", output[1]);
    }

    #[test]
    fn a_stage_handed_where_the_last_chain_left_off_ramps_from_there_to_its_own_target() {
        let ramp = Duration::from_millis(10);
        let mut stage = prepared(GainConfig {
            volume: Volume::new(0.5).expect("in range"),
            ramp,
            ..GainConfig::default()
        });
        let target = stage.amplitude();
        let left_off_at = 1.0;
        stage.ramp_gain_from(left_off_at);

        assert_eq!(stage.gain_amplitude(), Some(left_off_at));
        assert!(stage.is_ramping(), "a stage handed a start held its target");

        let frames = (ramp.as_secs_f64() * f64::from(SampleRate::HZ_48000.hz())).round() as usize;
        let input = vec![1.0; frames * 2];
        let mut output = vec![0.0; frames * 2];
        stage.process(&input, &mut output);

        let levels: Vec<f64> = output
            .as_chunks::<2>()
            .0
            .iter()
            .map(|frame| frame[0])
            .collect();
        let (left_off_at, target) = (f64::from(left_off_at), f64::from(target));
        let step = (left_off_at - target) / frames as f64;
        let slack = step / 16.0;
        let first = levels.first().copied().expect("the ramp is not empty");
        assert!(
            first <= left_off_at && left_off_at - first <= step + slack,
            "the first frame stepped from {left_off_at} to {first} where the ramp moves {step} a frame"
        );
        assert_eq!(levels.last().copied(), Some(target));
        for pair in levels.windows(2) {
            let fall = pair[0] - pair[1];
            assert!(
                (-slack..=step + slack).contains(&fall),
                "the ramp moved {fall} in one frame where it moves {step}"
            );
        }
        assert!(!stage.is_ramping(), "the ramp outlived its length");
    }

    #[test]
    fn asking_a_stage_for_the_amplitude_it_already_holds_leaves_nothing_to_ramp() {
        let mut stage = prepared(GainConfig {
            ramp: Duration::from_millis(10),
            ..GainConfig::default()
        });
        stage.set_gain(Volume::MAX, AppliedGain::default());

        assert!(!stage.is_ramping());
        assert_eq!(stage.gain_amplitude(), Some(1.0));
    }

    #[test]
    fn the_ramp_advances_once_per_frame_not_once_per_sample() {
        let mut stereo = prepared(GainConfig {
            ramp: Duration::from_millis(10),
            ..GainConfig::default()
        });
        stereo.set_volume(Volume::MUTE);

        let input = vec![1.0; 8];
        let mut output = vec![0.0; 8];
        stereo.process(&input, &mut output);

        for pair in output.as_chunks::<2>().0 {
            assert_eq!(pair[0], pair[1], "channels within a frame diverged");
        }
    }
}
