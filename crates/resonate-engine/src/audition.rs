use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use resonate_audio::NodeName;
use resonate_codec::{DecodeStatus, Decoder, Placement, Sources};
use resonate_core::{
    AppliedGain, AudioBuffer, Frames, SampleData, SampleFormat, StreamSpec, Trim, Volume, abx::Arm,
};
use resonate_dsp::{Chain, DitherKind, FilterPhase, NoiseShaping, Quality, Restoration};

use crate::{
    Decoded, EngineConfig, Error, LoudnessMeter, OutputPlan, QueueItem, Result,
    pipeline::{Attenuator, plan_for},
};

const MEASURED_IN_BLOCKS_OF: usize = 4_096;
const SETTLING_LEFT_UNMEASURED: Duration = Duration::from_secs(4);
const LEVELS_APART_AT_MOST_MILLIBELS: i32 = Trim::WIDEST_MILLIBELS;
const MILLIBELS_A_DECIBEL: f64 = 100.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Resampling {
    pub quality: Quality,
    pub phase: FilterPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Dithering {
    pub kind: DitherKind,
    pub shaping: NoiseShaping,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Contest {
    Restoration { a: Restoration, b: Restoration },
    Resampling { a: Resampling, b: Resampling },
    Dithering { a: Dithering, b: Dithering },
}

impl Contest {
    pub fn apply(self, arm: Arm, config: &mut EngineConfig) {
        match self {
            Self::Restoration { a, b } => {
                config.restoration = match arm {
                    Arm::A => a,
                    Arm::B => b,
                };
            }
            Self::Resampling { a, b } => {
                let chosen = match arm {
                    Arm::A => a,
                    Arm::B => b,
                };
                config.quality = chosen.quality;
                config.filter_phase = chosen.phase;
            }
            Self::Dithering { a, b } => {
                let chosen = match arm {
                    Arm::A => a,
                    Arm::B => b,
                };
                config.dither = chosen.kind;
                config.noise_shaping = chosen.shaping;
            }
        }
    }

    pub fn applied(self, arm: Arm, config: &EngineConfig) -> EngineConfig {
        let mut config = config.clone();
        self.apply(arm, &mut config);
        config
    }

    pub(crate) fn renders_alike(self, a: &OutputPlan, b: &OutputPlan) -> bool {
        if !a.renders_alike(b) {
            return false;
        }
        match self {
            Self::Restoration { .. } => true,
            Self::Resampling { a: one, b: other } => !a.resamples() || one == other,
            Self::Dithering { a: one, b: other } => !a.dithers() || one.kind == other.kind,
        }
    }

    pub const fn is_flat_in_level(self) -> bool {
        !matches!(self, Self::Restoration { .. })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Contested {
    restoration: Restoration,
    quality: Quality,
    filter_phase: FilterPhase,
    dither: DitherKind,
    noise_shaping: NoiseShaping,
}

impl Contested {
    pub(crate) const fn of(config: &EngineConfig) -> Self {
        Self {
            restoration: config.restoration,
            quality: config.quality,
            filter_phase: config.filter_phase,
            dither: config.dither,
            noise_shaping: config.noise_shaping,
        }
    }

    pub(crate) const fn put_back(self, config: &mut EngineConfig) {
        config.restoration = self.restoration;
        config.quality = self.quality;
        config.filter_phase = self.filter_phase;
        config.dither = self.dither;
        config.noise_shaping = self.noise_shaping;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Lowered {
    pub arm: Arm,
    pub by: Trim,
}

impl Lowered {
    pub const NOTHING: Self = Self {
        arm: Arm::A,
        by: Trim::NONE,
    };

    pub fn gain_of(self, arm: Arm) -> f64 {
        if arm != self.arm || self.by.is_none() {
            return 1.0;
        }
        let decibels = -f64::from(self.by.millibels().abs()) / MILLIBELS_A_DECIBEL;
        10.0_f64.powf(decibels / 20.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Audition {
    pub contest: Contest,
    pub heard: Arm,
    pub lowered: Lowered,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContestLevels {
    pub a_lufs: f64,
    pub b_lufs: f64,
}

impl ContestLevels {
    pub fn apart_db(self) -> f64 {
        self.a_lufs - self.b_lufs
    }

    pub fn lowered(self) -> Result<Lowered> {
        let apart = self.apart_db();
        let millibels = (apart.abs() * MILLIBELS_A_DECIBEL).round();
        if !millibels.is_finite() || millibels > f64::from(LEVELS_APART_AT_MOST_MILLIBELS) {
            return Err(Error::LevelsTooFarApart {
                millibels: millibels.clamp(0.0, f64::from(i32::MAX)) as i32,
            });
        }
        let by = Trim::from_millibels(millibels as i32).map_err(|_| Error::LevelsTooFarApart {
            millibels: millibels as i32,
        })?;
        let arm = if apart >= 0.0 { Arm::A } else { Arm::B };
        Ok(Lowered { arm, by })
    }
}

pub(crate) fn plans_of(
    decoded: Decoded,
    sink: &NodeName,
    stream: StreamSpec,
    config: &EngineConfig,
    replay_gain: AppliedGain,
    attenuator: Attenuator,
    contest: Contest,
) -> (OutputPlan, OutputPlan) {
    let plan = |arm| {
        plan_for(
            decoded,
            sink,
            stream,
            &contest.applied(arm, config),
            replay_gain,
            attenuator,
        )
    };
    (plan(Arm::A), plan(Arm::B))
}

struct Measured {
    chain: Chain,
    out: Vec<f64>,
    narrowed: Vec<f32>,
    meter: LoudnessMeter,
    unmeasured: usize,
}

impl Measured {
    fn take(&mut self, frames: usize, channels: usize) {
        let passed_over = frames.min(self.unmeasured);
        self.unmeasured -= passed_over;
        let Some(made) = self.out.get(passed_over * channels..frames * channels) else {
            return;
        };
        self.narrowed.clear();
        self.narrowed
            .extend(made.iter().map(|&sample| sample as f32));
        self.meter.note(&self.narrowed);
    }

    fn run(&mut self, input: &[f64], channels_in: usize, channels_out: usize) {
        let mut rest = input;
        while !rest.is_empty() {
            let count = self.chain.process(rest, &mut self.out);
            self.take(count.frames_out, channels_out);
            if count.frames_in == 0 {
                return;
            }
            rest = rest
                .get(count.frames_in * channels_in..)
                .unwrap_or_default();
        }
    }

    fn finish(mut self, channels_out: usize) -> Option<f64> {
        let mut tail = vec![0.0; self.chain.max_flush_frames().max(1) * channels_out];
        if let Ok(frames) = self.chain.flush(&mut tail) {
            self.out = tail;
            self.take(frames, channels_out);
        }
        self.meter.finished().integrated.map(f64::from)
    }
}

pub fn levels_of(
    sources: &Sources,
    item: &QueueItem,
    stream: StreamSpec,
    config: &EngineConfig,
    contest: Contest,
    stop: &AtomicBool,
) -> Result<ContestLevels> {
    let track = item.id;
    let opened = match item.span {
        Some(span) => Decoder::open_span(sources, &item.location, span),
        None => Decoder::open(sources, &item.location),
    };
    let (mut decoder, info) = opened.map_err(|source| Error::Decode { track, source })?;
    let hints = sources.hints(&item.location, item.span);
    let decoded = Decoded::of(&info, hints.lowpass);
    let source = info.spec;
    decoder.set_output_format(SampleFormat::F32);

    let flat = EngineConfig {
        volume: Volume::MAX,
        ..config.clone()
    };
    let (plan_a, plan_b) = plans_of(
        decoded,
        &NodeName::new(""),
        stream,
        &flat,
        AppliedGain::default(),
        Attenuator::Stream,
        contest,
    );
    if contest.renders_alike(&plan_a, &plan_b) {
        return Err(Error::ArmsAlike);
    }

    let channels_in = source.channel_count().get() as usize;
    let channels_out = stream.channel_count().get() as usize;
    let placements = vec![Placement::Front; channels_out];
    let settling = Frames::from_duration(SETTLING_LEFT_UNMEASURED, stream.rate);
    let unmeasured = info
        .duration
        .map_or(settling, |length| {
            settling.min(Frames(length.rescaled(source.rate, stream.rate).get() / 2))
        })
        .get() as usize;
    let arm = |plan: &OutputPlan, arm: Arm| -> Result<Measured> {
        let chain = plan
            .build_chain(source, &contest.applied(arm, &flat), MEASURED_IN_BLOCKS_OF)
            .map_err(|error| Error::Convert {
                track,
                source_spec: source,
                sink_spec: stream,
                source: error,
            })?;
        Ok(Measured {
            out: vec![0.0; chain.max_output_frames().max(1) * channels_out],
            narrowed: Vec::new(),
            meter: LoudnessMeter::new(&placements, stream.rate),
            unmeasured,
            chain,
        })
    };
    let mut a = arm(&plan_a, Arm::A)?;
    let mut b = arm(&plan_b, Arm::B)?;

    let mut block = AudioBuffer::empty(StreamSpec::new(
        source.rate,
        source.channels,
        SampleFormat::F32,
    ));
    let mut widened = Vec::new();
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err(Error::LevelsUnmeasured);
        }
        match decoder.next_block(&mut block) {
            Ok(DecodeStatus::EndOfStream | DecodeStatus::Reformatted(_)) => break,
            Ok(DecodeStatus::Decoded) => {
                let SampleData::F32(samples) = block.data() else {
                    continue;
                };
                for piece in samples.chunks(MEASURED_IN_BLOCKS_OF * channels_in) {
                    widened.clear();
                    widened.extend(piece.iter().map(|&sample| f64::from(sample)));
                    a.run(&widened, channels_in, channels_out);
                    b.run(&widened, channels_in, channels_out);
                }
            }
            Err(source) => return Err(Error::Decode { track, source }),
        }
    }

    match (a.finish(channels_out), b.finish(channels_out)) {
        (Some(a_lufs), Some(b_lufs)) => Ok(ContestLevels { a_lufs, b_lufs }),
        _ => Err(Error::LevelsUnmeasured),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_louder_arm_is_lowered_by_how_far_apart_the_two_were_measured() {
        let levels = ContestLevels {
            a_lufs: -14.0,
            b_lufs: -15.25,
        };

        let lowered = levels.lowered().expect("within the widest trim");

        assert_eq!(lowered.arm, Arm::A);
        assert_eq!(lowered.by.millibels(), 125);
        assert!((lowered.gain_of(Arm::A) - 10.0_f64.powf(-1.25 / 20.0)).abs() < 1e-12);
        assert!((lowered.gain_of(Arm::B) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn arms_further_apart_than_the_widest_trim_are_refused() {
        let levels = ContestLevels {
            a_lufs: -40.0,
            b_lufs: -10.0,
        };

        assert!(matches!(
            levels.lowered(),
            Err(Error::LevelsTooFarApart { millibels: 3_000 })
        ));
    }

    #[test]
    fn each_arm_writes_its_own_settings_and_putting_back_restores_the_rest() {
        let mut config = EngineConfig::default();
        let before = Contested::of(&config);
        let contest = Contest::Resampling {
            a: Resampling {
                quality: Quality::Fast,
                phase: FilterPhase::Minimum,
            },
            b: Resampling {
                quality: Quality::VeryHigh,
                phase: FilterPhase::Linear,
            },
        };

        contest.apply(Arm::B, &mut config);
        assert_eq!(config.quality, Quality::VeryHigh);
        assert_eq!(config.filter_phase, FilterPhase::Linear);
        contest.apply(Arm::A, &mut config);
        assert_eq!(config.quality, Quality::Fast);
        before.put_back(&mut config);

        assert_eq!(Contested::of(&config), before);
    }
}
