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

const STEPS_WAITING_AT_MOST: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Step {
    at: u64,
    volume: Volume,
    replay_gain: AppliedGain,
}

pub struct GainStage {
    config: GainConfig,
    channels: usize,
    amplitude: f32,
    limits: bool,
    clamped: u64,
    passed: u64,
    steps: [Option<Step>; STEPS_WAITING_AT_MOST],
}

impl GainStage {
    pub fn new(config: GainConfig) -> Self {
        Self {
            config,
            channels: 1,
            amplitude: config.amplitude(),
            limits: config.limits_every_sample(),
            clamped: 0,
            passed: 0,
            steps: [None; STEPS_WAITING_AT_MOST],
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

    fn take_the_step(&mut self, step: Step) {
        self.config.volume = step.volume;
        self.config.replay_gain = step.replay_gain;
        self.retarget();
    }

    fn next_step(&self) -> Option<Step> {
        self.steps.first().copied().flatten()
    }

    fn last_step(&self) -> Option<Step> {
        self.steps.iter().rev().find_map(|step| *step)
    }

    fn step_off_the_first(&mut self) -> Option<Step> {
        let first = self.next_step()?;
        self.steps.rotate_left(1);
        if let Some(last) = self.steps.last_mut() {
            *last = None;
        }
        Some(first)
    }

    fn take_the_steps_due(&mut self) {
        while self.next_step().is_some_and(|step| step.at <= self.passed) {
            if let Some(step) = self.step_off_the_first() {
                self.take_the_step(step);
            }
        }
    }

    fn take_every_step(&mut self) {
        while let Some(step) = self.step_off_the_first() {
            self.take_the_step(step);
        }
    }

    fn scaled(&mut self, taken: &[f64], made: &mut [f64]) {
        let amplitude = f64::from(self.amplitude);
        if self.limits {
            let mut past = 0_u64;
            for (sample, slot) in taken.iter().zip(made.iter_mut()) {
                let scaled = sample * amplitude;
                past += u64::from(scaled.abs() > 1.0);
                *slot = scaled.clamp(-1.0, 1.0);
            }
            self.clamped = self.clamped.saturating_add(past);
        } else {
            for (sample, slot) in taken.iter().zip(made.iter_mut()) {
                *slot = sample * amplitude;
            }
        }
    }
}

impl Processor for GainStage {
    fn prepare(&mut self, spec: StreamSpec, max_frames_in: usize) -> Result<usize> {
        self.channels = spec.channel_count().get() as usize;
        Ok(max_frames_in)
    }

    fn reset(&mut self) {
        self.take_every_step();
    }

    fn latency_frames(&self) -> f64 {
        0.0
    }

    fn is_transparent(&self) -> bool {
        false
    }

    fn set_gain(&mut self, volume: Volume, replay_gain: AppliedGain) {
        match self.steps.iter_mut().rev().find_map(Option::as_mut) {
            Some(last) => {
                last.volume = volume;
                last.replay_gain = replay_gain;
            }
            None => self.take_the_step(Step {
                at: self.passed,
                volume,
                replay_gain,
            }),
        }
    }

    fn set_gain_after(&mut self, volume: Volume, replay_gain: AppliedGain, frames: usize) {
        let step = Step {
            at: self.passed.saturating_add(frames as u64),
            volume,
            replay_gain,
        };
        for waiting in &mut self.steps {
            if waiting.is_some_and(|waiting| waiting.at >= step.at) {
                *waiting = None;
            }
        }
        if self.steps.iter().all(Option::is_some)
            && let Some(oldest) = self.step_off_the_first()
        {
            self.take_the_step(oldest);
        }
        if let Some(free) = self.steps.iter_mut().find(|slot| slot.is_none()) {
            *free = Some(step);
        }
        self.take_the_steps_due();
    }

    fn gain_amplitude(&self) -> Option<f32> {
        Some(self.last_step().map_or(self.amplitude, |step| {
            GainConfig {
                volume: step.volume,
                replay_gain: step.replay_gain,
                ..self.config
            }
            .amplitude()
        }))
    }

    fn frames_before_the_gain_settles(&self) -> usize {
        self.last_step()
            .map_or(0, |step| step.at.saturating_sub(self.passed) as usize)
    }

    fn overs(&self) -> u64 {
        self.clamped
    }

    fn process(&mut self, input: &[f64], output: &mut [f64]) -> ProcessCount {
        let channels = self.channels;
        let frames = (input.len() / channels).min(output.len() / channels);
        let samples = frames * channels;
        let (Some(mut taken), Some(mut made)) = (input.get(..samples), output.get_mut(..samples))
        else {
            return ProcessCount {
                frames_in: 0,
                frames_out: 0,
            };
        };

        while !taken.is_empty() {
            let left = taken.len() / channels;
            let run = self.next_step().map_or(left, |step| {
                (step.at.saturating_sub(self.passed) as usize).clamp(1, left)
            });
            let (Some((now, later)), Some((into, rest))) = (
                taken.split_at_checked(run * channels),
                made.split_at_mut_checked(run * channels),
            ) else {
                break;
            };
            self.scaled(now, into);
            self.passed = self.passed.saturating_add(run as u64);
            self.take_the_steps_due();
            taken = later;
            made = rest;
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

    fn halved() -> AppliedGain {
        AppliedGain {
            gain: Some(Decibels::new(-6.020_6).expect("finite")),
            peak: None,
        }
    }

    fn left_of(output: &[f64]) -> Vec<f64> {
        output
            .as_chunks::<2>()
            .0
            .iter()
            .map(|frame| (frame[0] * 1_000.0).round() / 1_000.0)
            .collect()
    }

    #[test]
    fn a_gain_set_after_what_the_chain_holds_steps_on_that_frame_across_blocks() {
        let mut stage = prepared(GainConfig::default());
        stage.set_gain_after(Volume::MAX, halved(), 3);

        assert_eq!(stage.frames_before_the_gain_settles(), 3);
        assert!((stage.gain_amplitude().expect("a gain") - 0.5).abs() < 1e-4);

        let mut first = [0.0; 4];
        stage.process(&[1.0; 4], &mut first);
        let mut second = [0.0; 6];
        stage.process(&[1.0; 6], &mut second);

        assert_eq!(left_of(&first), [1.0, 1.0]);
        assert_eq!(left_of(&second), [1.0, 0.5, 0.5]);
        assert_eq!(stage.frames_before_the_gain_settles(), 0);
    }

    #[test]
    fn a_volume_asked_while_a_step_waits_joins_the_step_and_leaves_the_frames_before_it() {
        let mut stage = prepared(GainConfig::default());
        stage.set_gain_after(Volume::MAX, halved(), 2);
        stage.set_gain(Volume::MUTE, halved());

        let mut output = [1.0; 8];
        stage.process(&[1.0; 8], &mut output);

        assert_eq!(left_of(&output), [1.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn steps_set_one_behind_another_each_land_on_their_own_frame() {
        let mut stage = prepared(GainConfig::default());
        stage.set_gain_after(Volume::MAX, halved(), 1);
        stage.set_gain_after(Volume::MAX, AppliedGain::default(), 3);

        let mut output = [0.0; 10];
        stage.process(&[1.0; 10], &mut output);

        assert_eq!(left_of(&output), [1.0, 0.5, 0.5, 1.0, 1.0]);
    }

    #[test]
    fn a_reset_takes_every_waiting_step_at_once() {
        let mut stage = prepared(GainConfig::default());
        stage.set_gain_after(Volume::MAX, halved(), 100);
        stage.reset();

        let mut output = [0.0; 2];
        stage.process(&[1.0; 2], &mut output);

        assert_eq!(left_of(&output), [0.5]);
        assert_eq!(stage.frames_before_the_gain_settles(), 0);
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
    fn a_boost_that_clamps_counts_the_samples_it_clamped() {
        let mut stage = prepared(GainConfig {
            replay_gain: boost(12.0, None),
            prevent_clipping: true,
            ..GainConfig::default()
        });

        let mut output = [0.0; 4];
        stage.process(&[0.9, -0.9, 0.1, 0.0], &mut output);

        assert_eq!(stage.overs(), 2);
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
