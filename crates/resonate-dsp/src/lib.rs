mod chain;
mod convolve;
mod crossfeed;
mod dither;
mod eq;
mod error;
mod fused;
mod gain;
mod overs;
mod peak;
mod phase;
mod recall;
mod remix;
mod resample;
mod restore;

pub use crate::{
    chain::{Back, Chain, ChainBuilder, Front},
    convolve::{CROSSED_RESPONSES, Convolver, Impulse, Speaker, partition_frames_at},
    crossfeed::{Crossfeed, Crossfeeder},
    dither::{Dither, DitherKind, NoiseShaping},
    eq::{Easing, Equaliser},
    error::{Error, RatioLimits, Result},
    gain::{GainConfig, GainStage, ReplayGainMode},
    overs::Overs,
    peak::{TruePeak, TruePeakMeter},
    phase::FilterPhase,
    recall::Recall,
    remix::Remix,
    resample::{Quality, Resampler, ResamplerConfig, SincParams},
    restore::{Restoration, Restore, RestoreConfig, Tuning},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProcessCount {
    pub frames_in: usize,
    pub frames_out: usize,
}

pub trait Processor: Send {
    fn prepare(&mut self, spec: resonate_core::StreamSpec, max_frames_in: usize) -> Result<usize>;

    fn max_output_frames(&self, frames_in: usize) -> usize {
        frames_in
    }

    fn max_flush_frames(&self) -> usize {
        0
    }

    fn reset(&mut self);

    fn output_spec(&self, input: resonate_core::StreamSpec) -> resonate_core::StreamSpec {
        input
    }

    fn latency_frames(&self) -> f64;

    fn frames_held(&self) -> usize {
        0
    }

    fn is_transparent(&self) -> bool;

    fn is_carried_across_a_reshape(&self) -> bool {
        false
    }

    fn set_gain(
        &mut self,
        _volume: resonate_core::Volume,
        _replay_gain: resonate_core::AppliedGain,
    ) {
    }

    fn set_equalisation(&mut self, _profile: &std::sync::Arc<resonate_core::eq::Profile>) {}

    fn gain_amplitude(&self) -> Option<f32> {
        None
    }

    fn is_ramping(&self) -> bool {
        false
    }

    fn overs(&self) -> u64 {
        0
    }

    fn ease_equalisation(&mut self, _easing: Easing) {}

    fn process(&mut self, input: &[f64], output: &mut [f64]) -> ProcessCount;

    fn flush(&mut self, output: &mut [f64]) -> usize;
}
