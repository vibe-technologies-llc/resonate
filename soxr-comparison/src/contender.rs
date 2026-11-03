use std::fmt;

use resonate_core::{ChannelLayout, SampleFormat, SampleRate, StreamSpec};
use resonate_dsp::{FilterPhase, Processor, Quality, Resampler, ResamplerConfig, SincParams};
use soxr::{
    Soxr,
    format::{IoFormat, Mono, Stereo},
    params::{QualityFlags, QualityRecipe, QualitySpec, Rolloff, RuntimeSpec},
};

use crate::error::{Error, Result, SoxrOp};

const SOXR_OUTPUT_FRAMES: usize = 16_384;
const SOXR_THREADS: u32 = 1;
const SOXR_VERY_HIGH_STEEP_PASSBAND_END: f64 = 0.982_27;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pair {
    pub from: SampleRate,
    pub to: SampleRate,
}

impl Pair {
    pub const fn new(from: SampleRate, to: SampleRate) -> Self {
        Self { from, to }
    }

    pub fn input_hz(self) -> f64 {
        f64::from(self.from.hz())
    }

    pub fn output_hz(self) -> f64 {
        f64::from(self.to.hz())
    }

    pub fn output_nyquist(self) -> f64 {
        self.output_hz() / 2.0
    }

    pub fn input_nyquist(self) -> f64 {
        self.input_hz() / 2.0
    }

    pub fn band_edge(self) -> f64 {
        self.input_nyquist().min(self.output_nyquist())
    }

    pub fn is_down(self) -> bool {
        self.to.hz() < self.from.hz()
    }

    pub fn slug(self) -> String {
        format!("{}-to-{}", self.from.kilohertz(), self.to.kilohertz())
    }
}

impl fmt::Display for Pair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} kHz to {} kHz",
            self.from.kilohertz(),
            self.to.kilohertz()
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoxrPhase {
    Linear,
    Intermediate,
    Minimum,
}

impl SoxrPhase {
    const fn response(self) -> f64 {
        match self {
            Self::Linear => 50.0,
            Self::Intermediate => 25.0,
            Self::Minimum => 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Contender {
    Ours {
        quality: Quality,
        phase: FilterPhase,
    },
    Tuned {
        quality: Quality,
        params: SincParams,
    },
    Soxr {
        recipe: QualityRecipe,
        phase: SoxrPhase,
        steep: bool,
    },
}

impl Contender {
    pub const fn is_ours(self) -> bool {
        matches!(self, Self::Ours { .. } | Self::Tuned { .. })
    }
}

impl fmt::Display for Contender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Tuned { quality, params } => write!(
                f,
                "Resonate {}, {} taps a side, cutoff {:.4}",
                level(quality),
                params.half_taps,
                params.cutoff
            ),
            Self::Ours { quality, phase } => {
                let quality = level(quality);
                let phase = match phase {
                    FilterPhase::Linear => "",
                    FilterPhase::Intermediate => ", intermediate",
                    FilterPhase::Minimum => ", minimum",
                };
                write!(f, "Resonate {quality}{phase}")
            }
            Self::Soxr {
                recipe,
                phase,
                steep,
            } => {
                let recipe = match recipe {
                    QualityRecipe::Quick => "QQ",
                    QualityRecipe::Low => "LQ",
                    QualityRecipe::Medium => "MQ",
                    QualityRecipe::Bits16 => "16-bit",
                    QualityRecipe::Bits20 => "HQ",
                    QualityRecipe::Bits24 => "24-bit",
                    QualityRecipe::Bits28 => "VHQ",
                    QualityRecipe::Bits32 => "32-bit",
                };
                let phase = match phase {
                    SoxrPhase::Linear => "",
                    SoxrPhase::Intermediate => ", intermediate",
                    SoxrPhase::Minimum => ", minimum",
                };
                let steep = if steep { ", steep" } else { "" };
                write!(f, "SoXR {recipe}{steep}{phase}")
            }
        }
    }
}

const fn level(quality: Quality) -> &'static str {
    match quality {
        Quality::Fast => "Fast",
        Quality::Balanced => "Balanced",
        Quality::High => "High",
        Quality::VeryHigh => "Very high",
    }
}

const fn ours(quality: Quality, phase: FilterPhase) -> Contender {
    Contender::Ours { quality, phase }
}

const fn soxr(recipe: QualityRecipe, phase: SoxrPhase, steep: bool) -> Contender {
    Contender::Soxr {
        recipe,
        phase,
        steep,
    }
}

pub const CONTENDERS: [Contender; 18] = [
    ours(Quality::Fast, FilterPhase::Linear),
    ours(Quality::Balanced, FilterPhase::Linear),
    ours(Quality::High, FilterPhase::Linear),
    ours(Quality::VeryHigh, FilterPhase::Linear),
    ours(Quality::High, FilterPhase::Minimum),
    ours(Quality::VeryHigh, FilterPhase::Intermediate),
    ours(Quality::VeryHigh, FilterPhase::Minimum),
    soxr(QualityRecipe::Quick, SoxrPhase::Linear, false),
    soxr(QualityRecipe::Low, SoxrPhase::Linear, false),
    soxr(QualityRecipe::Medium, SoxrPhase::Linear, false),
    soxr(QualityRecipe::Bits16, SoxrPhase::Linear, false),
    soxr(QualityRecipe::Bits20, SoxrPhase::Linear, false),
    soxr(QualityRecipe::Bits24, SoxrPhase::Linear, false),
    soxr(QualityRecipe::Bits28, SoxrPhase::Linear, false),
    soxr(QualityRecipe::Bits32, SoxrPhase::Linear, false),
    soxr(QualityRecipe::Bits28, SoxrPhase::Linear, true),
    soxr(QualityRecipe::Bits28, SoxrPhase::Intermediate, false),
    soxr(QualityRecipe::Bits28, SoxrPhase::Minimum, false),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channels {
    Mono,
    Stereo,
}

impl Channels {
    pub const fn count(self) -> usize {
        match self {
            Self::Mono => 1,
            Self::Stereo => 2,
        }
    }

    const fn layout(self) -> ChannelLayout {
        match self {
            Self::Mono => ChannelLayout::Mono,
            Self::Stereo => ChannelLayout::Stereo,
        }
    }
}

pub trait Framed: IoFormat<Sample = f64> {
    const CHANNELS: usize;

    fn framed(samples: &[f64]) -> &Self::Buffer;

    fn framed_mut(samples: &mut [f64]) -> &mut Self::Buffer;
}

impl Framed for Mono<f64> {
    const CHANNELS: usize = 1;

    fn framed(samples: &[f64]) -> &[f64] {
        samples
    }

    fn framed_mut(samples: &mut [f64]) -> &mut [f64] {
        samples
    }
}

impl Framed for Stereo<f64> {
    const CHANNELS: usize = 2;

    fn framed(samples: &[f64]) -> &[[f64; 2]] {
        samples.as_chunks().0
    }

    fn framed_mut(samples: &mut [f64]) -> &mut [[f64; 2]] {
        samples.as_chunks_mut().0
    }
}

pub struct OurStream {
    resampler: Resampler,
    channels: usize,
    most_in: usize,
    scratch: Vec<f64>,
}

impl OurStream {
    fn open(
        quality: Quality,
        phase: FilterPhase,
        params: SincParams,
        pair: Pair,
        channels: Channels,
        block: usize,
    ) -> std::result::Result<Self, resonate_dsp::Error> {
        let mut resampler = Resampler::with_params(
            ResamplerConfig {
                input_rate: pair.from,
                output_rate: pair.to,
                channels: channels.layout(),
                quality,
                phase,
                max_frames_in: block,
            },
            params,
        )?;
        let spec = StreamSpec::new(pair.from, channels.layout(), SampleFormat::F32);
        let most_out = resampler.prepare(spec, block)?;
        let room = most_out.max(resampler.max_flush_frames()) * channels.count();

        Ok(Self {
            resampler,
            channels: channels.count(),
            most_in: block,
            scratch: vec![0.0; room],
        })
    }

    fn push(&mut self, input: &[f64], output: &mut Vec<f64>) {
        for block in input.chunks(self.most_in * self.channels) {
            let mut rest = block;
            while !rest.is_empty() {
                let count = self.resampler.process(rest, &mut self.scratch);
                output.extend_from_slice(&self.scratch[..count.frames_out * self.channels]);
                if count.frames_in == 0 && count.frames_out == 0 {
                    break;
                }
                rest = &rest[count.frames_in * self.channels..];
            }
        }
    }

    fn finish(&mut self, output: &mut Vec<f64>) {
        let flushed = self.resampler.flush(&mut self.scratch);
        output.extend_from_slice(&self.scratch[..flushed * self.channels]);
    }
}

pub struct SoxrStream<F: Framed> {
    soxr: Soxr<F>,
    scratch: Vec<f64>,
}

impl<F: Framed> SoxrStream<F> {
    fn open(
        recipe: QualityRecipe,
        phase: SoxrPhase,
        steep: bool,
        pair: Pair,
    ) -> std::result::Result<Self, soxr::Error> {
        let mut spec = QualitySpec::configure(recipe, Rolloff::Small, QualityFlags::empty())
            .with_phase_response(phase.response());
        if steep {
            spec.set_passband_end(SOXR_VERY_HIGH_STEEP_PASSBAND_END);
        }
        let soxr = Soxr::new_with_params(
            pair.input_hz(),
            pair.output_hz(),
            spec,
            RuntimeSpec::new(SOXR_THREADS),
        )?;

        Ok(Self {
            soxr,
            scratch: vec![0.0; SOXR_OUTPUT_FRAMES * F::CHANNELS],
        })
    }

    fn push(&mut self, input: &[f64], output: &mut Vec<f64>) -> std::result::Result<(), SoxrOp> {
        let mut rest = input;
        loop {
            let done = self
                .soxr
                .process(F::framed(rest), F::framed_mut(&mut self.scratch))
                .map_err(|said| said_by_soxr(SoxrOp::Process, &said))?;
            let produced = done.output_frames * F::CHANNELS;
            output.extend_from_slice(&self.scratch[..produced]);
            rest = &rest[done.input_frames * F::CHANNELS..];
            if rest.is_empty() && produced < self.scratch.len() {
                return Ok(());
            }
            if done.input_frames == 0 && done.output_frames == 0 {
                return Err(SoxrOp::Stall);
            }
        }
    }

    fn finish(&mut self, output: &mut Vec<f64>) -> std::result::Result<(), SoxrOp> {
        loop {
            let drained = self
                .soxr
                .drain(F::framed_mut(&mut self.scratch))
                .map_err(|said| said_by_soxr(SoxrOp::Drain, &said))?;
            if drained == 0 {
                return Ok(());
            }
            output.extend_from_slice(&self.scratch[..drained * F::CHANNELS]);
        }
    }
}

fn said_by_soxr(op: SoxrOp, said: &soxr::Error) -> SoxrOp {
    eprintln!("libsoxr: {said}");
    op
}

pub enum Engine {
    Ours(Box<OurStream>),
    SoxrMono(SoxrStream<Mono<f64>>),
    SoxrStereo(SoxrStream<Stereo<f64>>),
}

pub struct Stream {
    engine: Engine,
    contender: Contender,
    pair: Pair,
}

impl Stream {
    pub fn open(
        contender: Contender,
        pair: Pair,
        channels: Channels,
        block: usize,
    ) -> Result<Self> {
        let engine = match contender {
            Contender::Ours { quality, phase } => {
                OurStream::open(quality, phase, quality.params(), pair, channels, block)
                    .map(|stream| Engine::Ours(Box::new(stream)))
                    .map_err(|source| Error::Ours {
                        contender,
                        pair,
                        source,
                    })?
            }
            Contender::Tuned { quality, params } => {
                OurStream::open(quality, FilterPhase::Linear, params, pair, channels, block)
                    .map(|stream| Engine::Ours(Box::new(stream)))
                    .map_err(|source| Error::Ours {
                        contender,
                        pair,
                        source,
                    })?
            }
            Contender::Soxr {
                recipe,
                phase,
                steep,
            } => match channels {
                Channels::Mono => {
                    SoxrStream::open(recipe, phase, steep, pair).map(Engine::SoxrMono)
                }
                Channels::Stereo => {
                    SoxrStream::open(recipe, phase, steep, pair).map(Engine::SoxrStereo)
                }
            }
            .map_err(|said| Error::Soxr {
                op: said_by_soxr(SoxrOp::Create, &said),
                contender,
                pair,
            })?,
        };

        Ok(Self {
            engine,
            contender,
            pair,
        })
    }

    pub fn push(&mut self, input: &[f64], output: &mut Vec<f64>) -> Result<()> {
        let pushed = match &mut self.engine {
            Engine::Ours(stream) => {
                stream.push(input, output);
                Ok(())
            }
            Engine::SoxrMono(stream) => stream.push(input, output),
            Engine::SoxrStereo(stream) => stream.push(input, output),
        };
        pushed.map_err(|op| self.refused(op))
    }

    pub fn finish(&mut self, output: &mut Vec<f64>) -> Result<()> {
        let finished = match &mut self.engine {
            Engine::Ours(stream) => {
                stream.finish(output);
                Ok(())
            }
            Engine::SoxrMono(stream) => stream.finish(output),
            Engine::SoxrStereo(stream) => stream.finish(output),
        };
        finished.map_err(|op| self.refused(op))
    }

    const fn refused(&self, op: SoxrOp) -> Error {
        Error::Soxr {
            op,
            contender: self.contender,
            pair: self.pair,
        }
    }
}

pub fn resampled(
    contender: Contender,
    pair: Pair,
    input: &[f64],
    block: usize,
) -> Result<Vec<f64>> {
    let mut stream = Stream::open(contender, pair, Channels::Mono, block)?;
    let mut output =
        Vec::with_capacity(input.len() * pair.to.hz() as usize / pair.from.hz() as usize + 4_096);

    stream.push(input, &mut output)?;
    stream.finish(&mut output)?;

    Ok(output)
}
