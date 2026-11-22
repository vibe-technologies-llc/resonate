use std::fmt;

use resonate_core::ChannelLayout;
use resonate_dsp::{FilterPhase, Quality, Resampler, ResamplerConfig, SincParams};

use crate::{
    error::{Error, Result},
    lab::{
        pair::Pair,
        subject::{Family, Running, Subject, SubjectName},
    },
    subjects::stage::Driven,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Resonate {
    Level {
        quality: Quality,
        phase: FilterPhase,
    },
    Tuned {
        quality: Quality,
        params: SincParams,
    },
}

pub const fn level(quality: Quality) -> &'static str {
    match quality {
        Quality::LowLatency => "Low latency",
        Quality::Balanced => "Balanced",
        Quality::High => "High",
        Quality::VeryHigh => "Very high",
    }
}

impl Resonate {
    pub const fn at(quality: Quality, phase: FilterPhase) -> Self {
        Self::Level { quality, phase }
    }

    const fn quality(self) -> Quality {
        match self {
            Self::Level { quality, .. } | Self::Tuned { quality, .. } => quality,
        }
    }

    const fn phase(self) -> FilterPhase {
        match self {
            Self::Level { phase, .. } => phase,
            Self::Tuned { .. } => FilterPhase::Linear,
        }
    }

    const fn params(self) -> SincParams {
        match self {
            Self::Level { quality, phase } => quality.params_for(phase),
            Self::Tuned { params, .. } => params,
        }
    }
}

impl fmt::Display for Resonate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Tuned { quality, params } => write!(
                f,
                "Resonate {}: {} taps, cut {:.5}, beta {:.2}, clear {}, hop {}, steep {}, window {:+.6}/{}",
                level(quality),
                params.half_taps,
                params.cutoff,
                params.kaiser_beta,
                params.clear_taps,
                params.hop_taps,
                params.steep_hop_taps,
                params.window.get(1).copied().unwrap_or_default(),
                params.window.len()
            ),
            Self::Level { quality, phase } => {
                let quality = level(quality);
                let phase = match phase {
                    FilterPhase::Linear => "",
                    FilterPhase::Intermediate => ", intermediate",
                    FilterPhase::Minimum => ", minimum",
                };
                write!(f, "Resonate {quality}{phase}")
            }
        }
    }
}

impl Subject for Resonate {
    fn name(&self) -> SubjectName {
        SubjectName(self.to_string())
    }

    fn family(&self) -> Family {
        Family::Ours
    }

    fn supports(&self, pair: Pair) -> bool {
        pair.converts()
    }

    fn open(&self, pair: Pair, layout: ChannelLayout, block: usize) -> Result<Box<dyn Running>> {
        let refused = |source| Error::Ours {
            subject: self.name(),
            pair,
            source,
        };
        let resampler = Resampler::with_params(
            ResamplerConfig {
                input_rate: pair.from,
                output_rate: pair.to,
                channels: layout,
                quality: self.quality(),
                phase: self.phase(),
                max_frames_in: block,
            },
            self.params(),
        )
        .map_err(refused)?;

        Driven::open(Box::new(resampler), pair.from, layout, block)
            .map(|driven| Box::new(driven) as Box<dyn Running>)
            .map_err(refused)
    }
}
