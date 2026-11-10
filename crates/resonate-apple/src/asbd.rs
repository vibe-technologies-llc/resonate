use std::num::NonZeroU32;

use objc2_core_audio_types::{
    AudioFormatFlags, AudioStreamBasicDescription, kAudioFormatFlagIsPacked,
    kAudioFormatFlagIsSignedInteger, kAudioFormatFlagsNativeFloatPacked, kAudioFormatLinearPCM,
};

const BITS_A_BYTE: u32 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Word {
    Float32,
    Signed16,
    Signed32,
}

impl Word {
    const fn bytes(self) -> NonZeroU32 {
        let bytes = match self {
            Self::Signed16 => 2,
            Self::Float32 | Self::Signed32 => 4,
        };
        match NonZeroU32::new(bytes) {
            Some(bytes) => bytes,
            None => NonZeroU32::MIN,
        }
    }

    const fn flags(self) -> AudioFormatFlags {
        match self {
            Self::Float32 => kAudioFormatFlagsNativeFloatPacked,
            Self::Signed16 | Self::Signed32 => {
                kAudioFormatFlagIsSignedInteger | kAudioFormatFlagIsPacked
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Format {
    hz: f64,
    channels: NonZeroU32,
    word: Word,
}

impl Format {
    pub const fn new(hz: f64, channels: NonZeroU32, word: Word) -> Self {
        Self { hz, channels, word }
    }

    pub const fn at(self, hz: f64) -> Self {
        Self { hz, ..self }
    }

    pub const fn channels(self) -> NonZeroU32 {
        self.channels
    }

    pub const fn frame_bytes(self) -> NonZeroU32 {
        match self.channels.checked_mul(self.word.bytes()) {
            Some(bytes) => bytes,
            None => NonZeroU32::MAX,
        }
    }

    pub(crate) const fn described(self) -> AudioStreamBasicDescription {
        let frame = self.frame_bytes().get();
        AudioStreamBasicDescription {
            mSampleRate: self.hz,
            mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: self.word.flags(),
            mBytesPerPacket: frame,
            mFramesPerPacket: 1,
            mBytesPerFrame: frame,
            mChannelsPerFrame: self.channels.get(),
            mBitsPerChannel: self.word.bytes().get() * BITS_A_BYTE,
            mReserved: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo() -> NonZeroU32 {
        NonZeroU32::MIN.saturating_add(1)
    }

    #[test]
    fn float_stereo_is_asked_for_interleaved_and_packed_at_the_rate_given() {
        let format = Format::new(44_100.0, stereo(), Word::Float32).described();

        assert_eq!(format.mSampleRate, 44_100.0);
        assert_eq!(format.mFormatID, kAudioFormatLinearPCM);
        assert_eq!(format.mFormatFlags, kAudioFormatFlagsNativeFloatPacked);
        assert_eq!(format.mChannelsPerFrame, 2);
        assert_eq!(format.mBitsPerChannel, 32);
        assert_eq!(format.mBytesPerFrame, 8);
        assert_eq!(format.mBytesPerPacket, 8);
        assert_eq!(format.mFramesPerPacket, 1);
    }

    #[test]
    fn a_signed_word_is_packed_into_as_many_bytes_as_it_has() {
        let sixteen = Format::new(48_000.0, stereo(), Word::Signed16).described();
        let thirty_two = Format::new(96_000.0, NonZeroU32::MIN, Word::Signed32).described();

        assert_eq!(
            sixteen.mFormatFlags,
            kAudioFormatFlagIsSignedInteger | kAudioFormatFlagIsPacked
        );
        assert_eq!(sixteen.mBitsPerChannel, 16);
        assert_eq!(sixteen.mBytesPerFrame, 4);
        assert_eq!(thirty_two.mBitsPerChannel, 32);
        assert_eq!(thirty_two.mBytesPerFrame, 4);
        assert_eq!(thirty_two.mChannelsPerFrame, 1);
    }

    #[test]
    fn a_format_moved_to_another_rate_keeps_its_shape() {
        let moved = Format::new(48_000.0, stereo(), Word::Signed16).at(44_100.0);

        assert_eq!(moved, Format::new(44_100.0, stereo(), Word::Signed16));
        assert_eq!(moved.frame_bytes().get(), 4);
    }
}
