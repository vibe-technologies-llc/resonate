use std::result;

use thiserror::Error;

use crate::{
    ChannelCount, SampleRate, Trim,
    eq::{BandGain, Frequency, MAX_BANDS, Q},
};

#[derive(Clone, Copy, Debug, PartialEq, Error)]
pub enum Error {
    #[error("{0} Hz is outside the supported range {min} Hz..={max} Hz", min = SampleRate::MIN_HZ, max = SampleRate::MAX_HZ)]
    SampleRateOutOfRange(u32),

    #[error("{0} channels is outside the supported range 1..={max}", max = ChannelCount::MAX.get())]
    ChannelCountOutOfRange(u16),

    #[error("{0} bits is not a representable bit depth")]
    BitDepthNotRepresentable(u8),

    #[error("{0} is not a finite decibel value")]
    DecibelsNotFinite(f32),

    #[error("a trim of {0} millibels is outside -{max}..={max}", max = Trim::WIDEST_MILLIBELS)]
    TrimOutOfRange(i32),

    #[error("gain {0} is not a finite, non-negative linear multiplier")]
    GainNotFinite(f32),

    #[error("volume {0} is outside 0.0..=1.0")]
    VolumeOutOfRange(f32),

    #[error("{0} centihertz is outside the band range {min}..={max}", min = Frequency::LOWEST_CENTIHERTZ, max = Frequency::HIGHEST_CENTIHERTZ)]
    BandFrequencyOutOfRange(u32),

    #[error("{0} millibels is outside the band gain range -{max}..={max}", max = BandGain::WIDEST_MILLIBELS)]
    BandGainOutOfRange(i32),

    #[error("a Q of {0} milli-units is outside {min}..={max}", min = Q::WIDEST_MILLI, max = Q::NARROWEST_MILLI)]
    BandQOutOfRange(u32),

    #[error("a preamp of {0} millibels is outside -{max}..={max}", max = BandGain::WIDEST_MILLIBELS)]
    PreampOutOfRange(i32),

    #[error("a profile of {0} bands is more than the {MAX_BANDS} a chain carries")]
    TooManyBands(usize),

    #[error("an identifier must be non-zero")]
    ZeroId,

    #[error("a media source is named in lowercase letters, digits, dashes and underscores")]
    SourceNameNotUsable,

    #[error("not a MusicBrainz identifier")]
    NotAnMbid,

    #[error("not an ISRC")]
    NotAnIsrc,

    #[error("not a Chromaprint written in URL-safe base64")]
    NotAChromaprint,
}

pub type Result<T> = result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_error_stays_copy_and_small_enough_for_result_large_err() {
        assert!(!std::mem::needs_drop::<Error>());
        assert!(size_of::<Error>() <= 128, "{}", size_of::<Error>());
    }
}
