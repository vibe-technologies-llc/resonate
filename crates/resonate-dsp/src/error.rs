use std::result;

use resonate_core::{ChannelCount, Ratio, SampleRate};
use thiserror::Error;

use crate::convolve::Speaker;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RatioLimits {
    pub min: Ratio,
    pub max: Ratio,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum Error {
    #[error("converting {from} to {to} needs ratio {ratio}, outside {min}..={max}", min = limits.min, max = limits.max)]
    RatioOutOfRange {
        from: SampleRate,
        to: SampleRate,
        ratio: Ratio,
        limits: RatioLimits,
    },

    #[error("stage was built for {expected} channels and was given {actual}")]
    ChannelMismatch {
        expected: ChannelCount,
        actual: ChannelCount,
    },

    #[error("stage was built for {expected} input and was given {actual}")]
    InputRateMismatch {
        expected: SampleRate,
        actual: SampleRate,
    },

    #[error("output holds {capacity} frames, {required} required")]
    OutputTooSmall { capacity: usize, required: usize },

    #[error("filter half-length {requested} exceeds the maximum {max}")]
    FilterLengthOutOfRange { requested: u32, max: u32 },

    #[error("{requested} filter phases are fewer than the {min} a resampler needs")]
    TooFewPhases { requested: u32, min: u32 },

    #[error("a response needing {bytes} bytes is more than the {at_most} a convolver may hold")]
    ResponseTooLarge { bytes: u64, at_most: u64 },

    #[error("the {speaker} speaker's response holds {channels} channels; a pair is stereo files")]
    SpeakerNotStereo { speaker: Speaker, channels: usize },

    #[error("the {speaker} speaker's response is already a two-by-two one")]
    SpeakerAlreadyCrossed { speaker: Speaker },

    #[error("the left speaker's response is at {left} and the right's at {right}")]
    SpeakerRatesDiffer { left: SampleRate, right: SampleRate },

    #[error("the left speaker's response is {left} frames and the right's {right}")]
    SpeakerLengthsDiffer { left: usize, right: usize },

    #[error("the pair of responses holds no usable taps")]
    SpeakersUnusable,

    #[error("a sweep from {from_hz} Hz to {to_hz} Hz does not fit below half of {rate}")]
    SweepBand {
        from_hz: u32,
        to_hz: u32,
        rate: SampleRate,
    },

    #[error("a sweep of {frames} frames is outside the length a measurement takes")]
    SweepLength { frames: usize },

    #[error("{heard} frames were heard and the sweep alone was {played}")]
    HeardTooShort { heard: usize, played: usize },

    #[error("{frames} frames heard are more than a measurement is deconvolved from")]
    HeardTooLong { frames: usize },

    #[error("nothing of the sweep was heard")]
    NothingHeard,

    #[error("the transform of {size} points failed")]
    SweepTransform { size: usize },
}

pub type Result<T> = result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_is_copy_and_never_allocates() {
        assert!(!std::mem::needs_drop::<Error>());
        assert!(size_of::<Error>() <= 128, "{}", size_of::<Error>());
    }
}
