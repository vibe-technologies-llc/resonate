use std::fmt;

use resonate_core::ChannelLayout;
use soxr::{
    Soxr,
    format::{IoFormat, Mono, Stereo},
    params::{QualityFlags, QualityRecipe, QualitySpec, Rolloff, RuntimeSpec},
};

use crate::{
    error::{Error, Result, SoxrOp},
    lab::{
        pair::Pair,
        subject::{Family, Running, Subject, SubjectName},
    },
};

const SOXR_OUTPUT_FRAMES: usize = 16_384;
const SOXR_THREADS: u32 = 1;
const SOXR_VERY_HIGH_STEEP_PASSBAND_END: f64 = 0.982_27;

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
pub struct Recipe {
    pub recipe: QualityRecipe,
    pub phase: SoxrPhase,
    pub steep: bool,
}

impl Recipe {
    pub const fn new(recipe: QualityRecipe, phase: SoxrPhase, steep: bool) -> Self {
        Self {
            recipe,
            phase,
            steep,
        }
    }
}

impl fmt::Display for Recipe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let recipe = match self.recipe {
            QualityRecipe::Quick => "QQ",
            QualityRecipe::Low => "LQ",
            QualityRecipe::Medium => "MQ",
            QualityRecipe::Bits16 => "16-bit",
            QualityRecipe::Bits20 => "HQ",
            QualityRecipe::Bits24 => "24-bit",
            QualityRecipe::Bits28 => "VHQ",
            QualityRecipe::Bits32 => "32-bit",
        };
        let phase = match self.phase {
            SoxrPhase::Linear => "",
            SoxrPhase::Intermediate => ", intermediate",
            SoxrPhase::Minimum => ", minimum",
        };
        let steep = if self.steep { ", steep" } else { "" };
        write!(f, "SoXR {recipe}{steep}{phase}")
    }
}

pub const RECIPES: [Recipe; 11] = [
    Recipe::new(QualityRecipe::Quick, SoxrPhase::Linear, false),
    Recipe::new(QualityRecipe::Low, SoxrPhase::Linear, false),
    Recipe::new(QualityRecipe::Medium, SoxrPhase::Linear, false),
    Recipe::new(QualityRecipe::Bits16, SoxrPhase::Linear, false),
    Recipe::new(QualityRecipe::Bits20, SoxrPhase::Linear, false),
    Recipe::new(QualityRecipe::Bits24, SoxrPhase::Linear, false),
    Recipe::new(QualityRecipe::Bits28, SoxrPhase::Linear, false),
    Recipe::new(QualityRecipe::Bits32, SoxrPhase::Linear, false),
    Recipe::new(QualityRecipe::Bits28, SoxrPhase::Linear, true),
    Recipe::new(QualityRecipe::Bits28, SoxrPhase::Intermediate, false),
    Recipe::new(QualityRecipe::Bits28, SoxrPhase::Minimum, false),
];

pub const PLOTTED: [Recipe; 2] = [
    Recipe::new(QualityRecipe::Bits28, SoxrPhase::Linear, false),
    Recipe::new(QualityRecipe::Bits28, SoxrPhase::Linear, true),
];

pub fn named(word: &str) -> Option<Recipe> {
    let (recipe, steep) = match word {
        "soxr-hq" => (QualityRecipe::Bits20, false),
        "soxr-24" => (QualityRecipe::Bits24, false),
        "soxr-vhq" => (QualityRecipe::Bits28, false),
        "soxr-32" => (QualityRecipe::Bits32, false),
        "soxr-steep" => (QualityRecipe::Bits28, true),
        _ => return None,
    };
    Some(Recipe::new(recipe, SoxrPhase::Linear, steep))
}

trait Framed: IoFormat<Sample = f64> {
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

struct SoxrStream<F: Framed> {
    soxr: Soxr<F>,
    scratch: Vec<f64>,
    subject: SubjectName,
    pair: Pair,
}

fn said_by_soxr(op: SoxrOp, said: &soxr::Error) -> SoxrOp {
    eprintln!("libsoxr: {said}");
    op
}

impl<F: Framed> SoxrStream<F> {
    fn open(recipe: Recipe, pair: Pair) -> std::result::Result<Self, soxr::Error> {
        let mut spec = QualitySpec::configure(recipe.recipe, Rolloff::Small, QualityFlags::empty())
            .with_phase_response(recipe.phase.response());

        if recipe.steep {
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
            subject: SubjectName(recipe.to_string()),
            pair,
        })
    }

    fn refused(&self, op: SoxrOp) -> Error {
        Error::Soxr {
            op,
            subject: self.subject.clone(),
            pair: self.pair,
        }
    }
}

impl<F: Framed> Running for SoxrStream<F> {
    fn push(&mut self, input: &[f64], output: &mut Vec<f64>) -> Result<()> {
        let mut rest = input;
        loop {
            let done = self
                .soxr
                .process(F::framed(rest), F::framed_mut(&mut self.scratch))
                .map_err(|said| self.refused(said_by_soxr(SoxrOp::Process, &said)))?;
            let produced = done.output_frames * F::CHANNELS;

            output.extend_from_slice(&self.scratch[..produced]);
            rest = &rest[done.input_frames * F::CHANNELS..];
            if rest.is_empty() && produced < self.scratch.len() {
                return Ok(());
            }
            if done.input_frames == 0 && done.output_frames == 0 {
                return Err(self.refused(SoxrOp::Stall));
            }
        }
    }

    fn finish(&mut self, output: &mut Vec<f64>) -> Result<()> {
        loop {
            let drained = self
                .soxr
                .drain(F::framed_mut(&mut self.scratch))
                .map_err(|said| self.refused(said_by_soxr(SoxrOp::Drain, &said)))?;

            if drained == 0 {
                return Ok(());
            }
            output.extend_from_slice(&self.scratch[..drained * F::CHANNELS]);
        }
    }

    fn channels_out(&self) -> usize {
        F::CHANNELS
    }
}

impl Subject for Recipe {
    fn name(&self) -> SubjectName {
        SubjectName(self.to_string())
    }

    fn family(&self) -> Family {
        Family::Reference
    }

    fn supports(&self, pair: Pair) -> bool {
        pair.converts()
    }

    fn open(&self, pair: Pair, layout: ChannelLayout, _block: usize) -> Result<Box<dyn Running>> {
        let refused = |said: soxr::Error| Error::Soxr {
            op: said_by_soxr(SoxrOp::Create, &said),
            subject: self.name(),
            pair,
        };

        match layout {
            ChannelLayout::Mono => SoxrStream::<Mono<f64>>::open(*self, pair)
                .map(|stream| Box::new(stream) as Box<dyn Running>)
                .map_err(refused),
            ChannelLayout::Stereo => SoxrStream::<Stereo<f64>>::open(*self, pair)
                .map(|stream| Box::new(stream) as Box<dyn Running>)
                .map_err(refused),
            _ => Err(Error::LayoutUnsupported {
                subject: self.name(),
                layout,
            }),
        }
    }
}
