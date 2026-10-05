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
    pub guarded_after: bool,
}

impl Default for GainConfig {
    fn default() -> Self {
        Self {
            volume: Volume::MAX,
            replay_gain: AppliedGain::default(),
            prevent_clipping: true,
            guarded_after: false,
        }
    }
}

impl GainConfig {
    pub fn amplitude(&self) -> f32 {
        let replay_gain = if self.prevent_clipping {
            self.replay_gain.applied()
        } else {
            self.replay_gain.requested()
        };
        self.volume.to_gain().get() * replay_gain.get()
    }

    fn limits_every_sample(&self) -> bool {
        self.prevent_clipping
            && !self.guarded_after
            && self.replay_gain.peak.is_none()
            && self.amplitude() > Gain::UNITY.get()
    }
}

pub struct GainStage {
    config: GainConfig,
    channels: usize,
    amplitude: f32,
    limits: bool,
}

impl GainStage {
    pub fn new(config: GainConfig) -> Self {
        Self {
            config,
            channels: 1,
            amplitude: config.amplitude(),
            limits: config.limits_every_sample(),
        }
    }

    #[cfg(test)]
    pub fn set_volume(&mut self, volume: Volume) {
        self.config.volume = volume;
        self.retarget();
    }

    pub const fn amplitude(&self) -> f32 {
        self.amplitude
    }

    fn retarget(&mut self) {
        self.amplitude = self.config.amplitude();
        self.limits = self.config.limits_every_sample();
    }
}

impl Processor for GainStage {
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

    fn set_gain(&mut self, volume: Volume, replay_gain: AppliedGain) {
        self.config.volume = volume;
        self.config.replay_gain = replay_gain;
        self.retarget();
    }

    fn gain_amplitude(&self) -> Option<f32> {
        Some(self.amplitude)
    }

    fn process(&mut self, input: &[f64], output: &mut [f64]) -> ProcessCount {
        let channels = self.channels;
        let frames = (input.len() / channels).min(output.len() / channels);
        let samples = frames * channels;
        let amplitude = f64::from(self.amplitude);
        let (Some(taken), Some(made)) = (input.get(..samples), output.get_mut(..samples)) else {
            return ProcessCount {
                frames_in: 0,
                frames_out: 0,
            };
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
    fn a_new_volume_applies_from_the_next_frame() {
        let mut stage = prepared(GainConfig::default());
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
            prevent_clipping: true,
            ..GainConfig::default()
        });

        let input = [0.9, -0.9];
        let mut output = [0.0; 2];
        stage.process(&input, &mut output);

        assert_eq!(output, [1.0, -1.0]);
    }

    #[test]
    fn a_boost_with_no_declared_peak_leaves_its_overs_to_a_guard_that_follows() {
        let mut stage = prepared(GainConfig {
            replay_gain: boost(12.0, None),
            prevent_clipping: true,
            guarded_after: true,
            ..GainConfig::default()
        });

        let input = [0.9, -0.9];
        let mut output = [0.0; 2];
        stage.process(&input, &mut output);

        assert!(
            output[0] > 3.5 && output[1] < -3.5,
            "the overs were clipped before the guard could ride them: {output:?}"
        );
    }

    #[test]
    fn a_stage_at_unity_or_below_passes_an_over_already_in_the_signal_through() {
        let unity = prepared(GainConfig {
            ..GainConfig::default()
        });
        let attenuating = prepared(GainConfig {
            volume: Volume::new(0.5).expect("in range"),
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
}
