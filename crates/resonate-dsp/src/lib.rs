mod chain;
mod dither;
mod eq;
mod error;
mod gain;
mod peak;
mod phase;
mod remix;
mod resample;
mod restore;

pub use crate::{
    chain::{Chain, ChainBuilder},
    dither::{Dither, DitherKind, NoiseShaping},
    eq::Equaliser,
    error::{Error, RatioLimits, Result},
    gain::{GainConfig, GainStage, ReplayGainMode},
    peak::{TruePeak, TruePeakMeter},
    phase::FilterPhase,
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

    fn is_transparent(&self) -> bool;

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

    fn ramp_gain_from(&mut self, _amplitude: f32) {}

    fn is_ramping(&self) -> bool {
        false
    }

    fn process(&mut self, input: &[f64], output: &mut [f64]) -> ProcessCount;

    fn flush(&mut self, output: &mut [f64]) -> usize;
}
