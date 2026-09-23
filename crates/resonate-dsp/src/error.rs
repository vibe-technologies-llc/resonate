use std::result;

use resonate_core::{ChannelCount, Ratio, SampleRate};
use thiserror::Error;

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
