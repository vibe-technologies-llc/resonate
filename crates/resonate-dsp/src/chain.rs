use std::{mem, sync::Arc};

use resonate_core::{AppliedGain, StreamSpec, Volume, eq::Profile};

use crate::{Error, ProcessCount, Processor, Result};

pub struct Chain {
    stages: Vec<Box<dyn Processor>>,
    widths: Vec<usize>,
    scratch: [Vec<f64>; 2],
    input_channels: usize,
    output_channels: usize,
    max_output_frames: usize,
    max_flush_frames: usize,
    latency: f64,
}

impl Chain {
    pub fn builder(input: StreamSpec) -> ChainBuilder {
        ChainBuilder {
            input,
            stages: Vec::new(),
            max_frames_in: 0,
        }
    }

    #[cfg(test)]
    pub const fn stage_count(&self) -> usize {
        self.stages.len()
    }

    pub const fn is_transparent(&self) -> bool {
        self.stages.is_empty()
    }

    pub const fn latency_frames(&self) -> f64 {
        self.latency
    }

    pub const fn max_output_frames(&self) -> usize {
        self.max_output_frames
    }

    pub const fn max_flush_frames(&self) -> usize {
        self.max_flush_frames
    }

    pub fn reset(&mut self) {
        for stage in &mut self.stages {
            stage.reset();
        }
    }

    pub fn set_gain(&mut self, volume: Volume, replay_gain: AppliedGain) {
        for stage in &mut self.stages {
            stage.set_gain(volume, replay_gain);
        }
    }

    pub fn set_equalisation(&mut self, profile: &Arc<Profile>) {
        for stage in &mut self.stages {
            stage.set_equalisation(profile);
        }
    }

    pub fn gain_amplitude(&self) -> Option<f32> {
        self.stages.iter().find_map(|stage| stage.gain_amplitude())
    }

    pub fn ramp_gain_from(&mut self, amplitude: f32) {
        for stage in &mut self.stages {
            stage.ramp_gain_from(amplitude);
        }
    }

    pub fn is_ramping(&self) -> bool {
        self.stages.iter().any(|stage| stage.is_ramping())
    }

    fn copy(source: &[f64], destination: &mut [f64]) -> usize {
        let samples = source.len().min(destination.len());
        if let (Some(from), Some(into)) = (source.get(..samples), destination.get_mut(..samples)) {
            into.copy_from_slice(from);
        }
        samples
    }

    pub fn process(&mut self, input: &[f64], output: &mut [f64]) -> ProcessCount {
        let widths = &self.widths;
        let [front, back] = &mut self.scratch;

        let Some((last, leading)) = self.stages.split_last_mut() else {
            let frames = Self::copy(input, output) / self.input_channels;
            return ProcessCount {
                frames_in: frames,
                frames_out: frames,
            };
        };

        let mut source = front.as_mut_slice();
        let mut destination = back.as_mut_slice();
        let mut frames_in = None;
        let mut frames = 0;
        let mut channels = self.input_channels;

        for (stage, width) in leading.iter_mut().zip(widths) {
            let taken = match frames_in {
                None => input,
                Some(_) => source.get(..frames * channels).unwrap_or_default(),
            };
            let count = stage.process(taken, destination);
            frames_in.get_or_insert(count.frames_in);
            frames = count.frames_out;
            channels = *width;
            mem::swap(&mut source, &mut destination);
        }

        let taken = match frames_in {
            None => input,
            Some(_) => source.get(..frames * channels).unwrap_or_default(),
        };
        let count = last.process(taken, output);

        ProcessCount {
            frames_in: frames_in.unwrap_or(count.frames_in),
            frames_out: count.frames_out,
        }
    }

    pub fn flush(&mut self, output: &mut [f64]) -> Result<usize> {
        let capacity = output.len() / self.output_channels;
        if capacity < self.max_flush_frames {
            return Err(Error::OutputTooSmall {
                capacity,
                required: self.max_flush_frames,
            });
        }
        Ok(self.drained_into(output))
    }

    fn drained_into(&mut self, output: &mut [f64]) -> usize {
        let widths = &self.widths;
        let [front, back] = &mut self.scratch;

        let Some((last, leading)) = self.stages.split_last_mut() else {
            return 0;
        };
        let Some((first, middle)) = leading.split_first_mut() else {
            return last.flush(output);
        };

        let mut source = front.as_mut_slice();
        let mut destination = back.as_mut_slice();

        let mut frames = first.flush(destination);
        let mut channels = widths.first().copied().unwrap_or(self.input_channels);
        mem::swap(&mut source, &mut destination);

        for (stage, width) in middle.iter_mut().zip(widths.iter().skip(1)) {
            let taken = source.get(..frames * channels).unwrap_or_default();
            frames = Self::drained_through(stage.as_mut(), taken, destination, *width);
            channels = *width;
            mem::swap(&mut source, &mut destination);
        }

        let taken = source.get(..frames * channels).unwrap_or_default();
        let width = widths.last().copied().unwrap_or(channels);
        Self::drained_through(last.as_mut(), taken, output, width)
    }

    fn drained_through(
        stage: &mut dyn Processor,
        taken: &[f64],
        into: &mut [f64],
        width: usize,
    ) -> usize {
        let carried = stage.process(taken, into).frames_out;
        let drained = into
            .get_mut(carried * width..)
            .map_or(0, |tail| stage.flush(tail));
        carried + drained
    }
}

pub struct ChainBuilder {
    input: StreamSpec,
    stages: Vec<Box<dyn Processor>>,
    max_frames_in: usize,
}

impl ChainBuilder {
    #[must_use]
    pub fn push(mut self, stage: Box<dyn Processor>) -> Self {
        self.stages.push(stage);
        self
    }

    #[must_use]
    pub const fn max_frames_in(mut self, frames: usize) -> Self {
        self.max_frames_in = frames;
        self
    }

    pub const fn input(&self) -> StreamSpec {
        self.input
    }

    pub fn build(self) -> Result<Chain> {
        let input_channels = self.input.channel_count().get() as usize;
        let mut spec = self.input;
        let mut frames = self.max_frames_in;
        let mut draining = 0;
        let mut samples = self.max_frames_in.saturating_mul(input_channels);
        let mut latency = 0.0;
        let mut stages: Vec<Box<dyn Processor>> = Vec::new();
        let mut widths: Vec<usize> = Vec::new();

        for mut stage in self.stages {
            if stage.is_transparent() {
                continue;
            }
            let next = stage.output_spec(spec);
            stage.prepare(spec, frames.max(draining))?;
            draining = stage
                .max_output_frames(draining)
                .saturating_add(stage.max_flush_frames());
            frames = stage.max_output_frames(frames);
            let channels = next.channel_count().get() as usize;
            samples = samples.max(frames.max(draining).saturating_mul(channels));
            latency = latency * f64::from(next.rate.hz()) / f64::from(spec.rate.hz())
                + stage.latency_frames();
            spec = next;
            stages.push(stage);
            widths.push(channels);
        }

        Ok(Chain {
            output_channels: widths.last().copied().unwrap_or(input_channels),
            stages,
            widths,
            scratch: [vec![0.0; samples], vec![0.0; samples]],
            input_channels,
            max_output_frames: frames.max(draining),
            max_flush_frames: draining,
            latency,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use resonate_core::{
        BitDepth, ChannelLayout, Decibels, Gain, SampleFormat, SampleRate, Volume,
    };

    use super::*;
    use crate::{
        Dither, DitherKind, FilterPhase, GainConfig, GainStage, NoiseShaping, Quality, Remix,
        Resampler, ResamplerConfig, Restoration, Restore, RestoreConfig, Tuning,
    };

    const BLOCK: usize = 512;

    fn spec(rate: SampleRate) -> StreamSpec {
        StreamSpec::new(rate, ChannelLayout::Stereo, SampleFormat::F32)
    }

    fn resampler(from: SampleRate, to: SampleRate) -> Box<dyn Processor> {
        Box::new(
            Resampler::new(ResamplerConfig {
                input_rate: from,
                output_rate: to,
                channels: ChannelLayout::Stereo,
                quality: Quality::Balanced,
                phase: FilterPhase::Linear,
                max_frames_in: BLOCK,
            })
            .expect("a valid resampler configuration"),
        )
    }

    fn attenuator() -> Box<dyn Processor> {
        Box::new(GainStage::new(GainConfig {
            volume: Volume::new(0.5).expect("in range"),
            ramp: Duration::ZERO,
            ..GainConfig::default()
        }))
    }

    #[test]
    fn a_bit_perfect_path_builds_to_a_chain_of_zero_stages() {
        let chain = Chain::builder(spec(SampleRate::HZ_96000))
            .max_frames_in(BLOCK)
            .push(Box::new(Remix::new(
                ChannelLayout::Stereo,
                ChannelLayout::Stereo,
            )))
            .push(resampler(SampleRate::HZ_96000, SampleRate::HZ_96000))
            .build()
            .expect("a transparent chain always builds");

        assert_eq!(chain.stage_count(), 0);
        assert!(chain.is_transparent());
        assert_eq!(chain.latency_frames(), 0.0);
    }

    #[test]
    fn a_gain_stage_the_builder_is_handed_is_kept_even_at_unity() {
        let cancelled = GainStage::new(GainConfig {
            volume: Volume::new(0.5).expect("in range"),
            replay_gain: AppliedGain {
                gain: Some(Decibels::new(20.0).expect("finite")),
                peak: Some(Gain::new(0.125).expect("in range")),
            },
            ..GainConfig::default()
        });
        assert_eq!(
            cancelled.amplitude(),
            1.0,
            "the slider and the capped boost were meant to cancel exactly"
        );

        for stage in [GainStage::new(GainConfig::default()), cancelled] {
            let chain = Chain::builder(spec(SampleRate::HZ_44100))
                .max_frames_in(BLOCK)
                .push(Box::new(stage))
                .build()
                .expect("a gain stage always builds");

            assert_eq!(
                chain.stage_count(),
                1,
                "the builder dropped a gain stage the plan asked for"
            );
        }
    }

    #[test]
    fn a_transparent_chain_passes_samples_through_untouched() {
        let mut chain = Chain::builder(spec(SampleRate::HZ_44100))
            .max_frames_in(BLOCK)
            .build()
            .expect("an empty chain always builds");

        let input: Vec<f64> = (0..BLOCK * 2).map(|n| n as f64 / 1_000.0).collect();
        let mut output = vec![0.0; input.len()];
        let count = chain.process(&input, &mut output);

        assert_eq!(count.frames_in, BLOCK);
        assert_eq!(count.frames_out, BLOCK);
        assert_eq!(output, input);
    }

    #[test]
    fn only_the_non_transparent_stages_are_kept() {
        let chain = Chain::builder(spec(SampleRate::HZ_44100))
            .max_frames_in(BLOCK)
            .push(resampler(SampleRate::HZ_44100, SampleRate::HZ_48000))
            .push(resampler(SampleRate::HZ_48000, SampleRate::HZ_48000))
            .push(Box::new(GainStage::new(GainConfig::default())))
            .push(Box::new(Dither::new(
                BitDepth::Bits16,
                DitherKind::Triangular,
                NoiseShaping::None,
                1,
            )))
            .build()
            .expect("a valid chain");

        assert_eq!(chain.stage_count(), 3);
        assert!(chain.latency_frames() > 0.0);
    }

    #[test]
    fn a_delay_ahead_of_the_resampler_is_counted_at_the_rate_the_chain_ends_on() {
        let restorer = || {
            Box::new(Restore::new(RestoreConfig {
                restoration: Restoration::Repair,
                tuning: Tuning::Mp3,
                wall: None,
            })) as Box<dyn Processor>
        };
        let alone = |stage: Box<dyn Processor>| {
            Chain::builder(spec(SampleRate::HZ_44100))
                .max_frames_in(BLOCK)
                .push(stage)
                .build()
                .expect("a valid chain")
                .latency_frames()
        };
        let restoring = alone(restorer());
        let resampling = alone(resampler(SampleRate::HZ_44100, SampleRate::HZ_192000));
        assert!(restoring > 0.0 && resampling > 0.0);

        let both = Chain::builder(spec(SampleRate::HZ_44100))
            .max_frames_in(BLOCK)
            .push(restorer())
            .push(resampler(SampleRate::HZ_44100, SampleRate::HZ_192000))
            .build()
            .expect("a valid chain")
            .latency_frames();

        let wanted = restoring * 192_000.0 / 44_100.0 + resampling;
        assert!(
            (both - wanted).abs() < 1e-9,
            "the chain said {both} frames where {wanted} are held"
        );
    }

    #[test]
    fn a_single_stage_chain_applies_that_stage() {
        let mut chain = Chain::builder(spec(SampleRate::HZ_44100))
            .max_frames_in(BLOCK)
            .push(attenuator())
            .build()
            .expect("a valid chain");

        let input = vec![1.0; BLOCK * 2];
        let mut output = vec![0.0; BLOCK * 2];
        chain.process(&input, &mut output);

        for sample in output {
            assert!((sample - 0.125).abs() < 1e-6, "got {sample}");
        }
    }

    #[test]
    fn stages_run_in_order_with_the_resampled_rate_reaching_the_gain() {
        let mut chain = Chain::builder(spec(SampleRate::HZ_44100))
            .max_frames_in(BLOCK)
            .push(resampler(SampleRate::HZ_44100, SampleRate::HZ_88200))
            .push(attenuator())
            .build()
            .expect("a valid chain");

        assert_eq!(chain.stage_count(), 2);

        let input = vec![1.0; BLOCK * 2];
        let mut output = vec![0.0; chain.max_output_frames() * 2];
        let mut produced = 0;
        for _ in 0..8 {
            produced = chain.process(&input, &mut output).frames_out;
        }

        assert!(
            produced > BLOCK,
            "upsampling produced only {produced} frames"
        );
        let settled = output.get(..produced * 2).expect("produced frames fit");
        for sample in settled {
            assert!((sample - 0.125).abs() < 1e-3, "got {sample}");
        }
    }

    #[test]
    fn flushing_a_transparent_chain_produces_nothing() {
        let mut chain = Chain::builder(spec(SampleRate::HZ_44100))
            .max_frames_in(BLOCK)
            .build()
            .expect("an empty chain always builds");

        let mut output = vec![0.0; BLOCK * 2];
        assert_eq!(chain.flush(&mut output), Ok(0));
    }

    fn staged_resampler(
        from: SampleRate,
        to: SampleRate,
        max_frames_in: usize,
    ) -> Box<dyn Processor> {
        Box::new(
            Resampler::new(ResamplerConfig {
                input_rate: from,
                output_rate: to,
                channels: ChannelLayout::Stereo,
                quality: Quality::Balanced,
                phase: FilterPhase::Linear,
                max_frames_in,
            })
            .expect("a valid resampler configuration"),
        )
    }

    #[test]
    fn flushing_drains_every_stage_and_not_just_the_first() {
        let mut chain = Chain::builder(spec(SampleRate::HZ_44100))
            .max_frames_in(BLOCK)
            .push(resampler(SampleRate::HZ_44100, SampleRate::HZ_48000))
            .push(staged_resampler(
                SampleRate::HZ_48000,
                SampleRate::HZ_96000,
                BLOCK * 2,
            ))
            .build()
            .expect("a valid chain");

        let input = vec![1.0; BLOCK * 2];
        let mut output = vec![0.0; chain.max_output_frames() * 2];

        let mut consumed = 0;
        let mut produced = 0;
        while consumed < BLOCK * 16 {
            let count = chain.process(&input, &mut output);
            consumed += count.frames_in;
            produced += count.frames_out;
        }
        produced += chain
            .flush(&mut output)
            .expect("the chain's own bound fits");

        let expected = consumed as f64 * 96_000.0 / 44_100.0;
        assert!(
            (produced as f64 - expected).abs() < 8.0,
            "{produced} frames left the chain where about {expected} were fed in"
        );
    }

    fn surround(rate: SampleRate) -> StreamSpec {
        StreamSpec::new(rate, ChannelLayout::Surround51, SampleFormat::F32)
    }

    #[test]
    fn a_stage_that_narrows_the_channels_carries_the_rest_of_the_chain_with_it() {
        let mut chain = Chain::builder(surround(SampleRate::HZ_44100))
            .max_frames_in(BLOCK)
            .push(Box::new(Remix::new(
                ChannelLayout::Surround51,
                ChannelLayout::Stereo,
            )))
            .push(resampler(SampleRate::HZ_44100, SampleRate::HZ_88200))
            .push(attenuator())
            .build()
            .expect("a valid chain");

        assert_eq!(chain.stage_count(), 3);

        let input = vec![1.0; BLOCK * 6];
        let mut output = vec![0.0; chain.max_output_frames() * 2];
        let mut produced = 0;
        for _ in 0..8 {
            produced = chain.process(&input, &mut output).frames_out;
        }

        assert!(produced > BLOCK, "upsampling produced {produced} frames");
        let settled = output
            .get(..produced * 2)
            .expect("the produced frames are two channels wide");
        for sample in settled {
            assert!((sample - 0.125).abs() < 1e-2, "got {sample}");
        }
    }

    #[test]
    fn a_narrowing_stage_still_lets_the_chain_drain() {
        let mut chain = Chain::builder(surround(SampleRate::HZ_44100))
            .max_frames_in(BLOCK)
            .push(Box::new(Remix::new(
                ChannelLayout::Surround51,
                ChannelLayout::Stereo,
            )))
            .push(resampler(SampleRate::HZ_44100, SampleRate::HZ_48000))
            .build()
            .expect("a valid chain");

        let input = vec![1.0; BLOCK * 6];
        let mut output = vec![0.0; chain.max_output_frames() * 2];
        chain.process(&input, &mut output);

        assert!(
            chain
                .flush(&mut output)
                .expect("the chain's own bound fits")
                > 0
        );
    }

    #[test]
    fn flushing_drains_the_resamplers_history() {
        let mut chain = Chain::builder(spec(SampleRate::HZ_44100))
            .max_frames_in(BLOCK)
            .push(resampler(SampleRate::HZ_44100, SampleRate::HZ_48000))
            .build()
            .expect("a valid chain");

        let input = vec![1.0; BLOCK * 2];
        let mut output = vec![0.0; chain.max_output_frames() * 2];
        chain.process(&input, &mut output);

        assert!(
            chain
                .flush(&mut output)
                .expect("the chain's own bound fits")
                > 0
        );
    }

    #[test]
    fn a_flush_into_less_room_than_the_chain_drains_is_refused_rather_than_cut_short() {
        let mut chain = Chain::builder(spec(SampleRate::HZ_44100))
            .max_frames_in(BLOCK)
            .push(resampler(SampleRate::HZ_44100, SampleRate::HZ_48000))
            .build()
            .expect("a valid chain");

        let input = vec![1.0; BLOCK * 2];
        let mut output = vec![0.0; chain.max_output_frames() * 2];
        chain.process(&input, &mut output);

        let bound = chain.max_flush_frames();
        assert!(bound > 0, "a resampler drains a tail");
        let mut short = vec![0.0; (bound - 1) * 2];
        assert_eq!(
            chain.flush(&mut short),
            Err(Error::OutputTooSmall {
                capacity: bound - 1,
                required: bound,
            })
        );
        assert!(chain.max_output_frames() >= bound);
    }
}
