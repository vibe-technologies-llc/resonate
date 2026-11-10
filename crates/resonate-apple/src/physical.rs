use objc2_core_audio_types::{
    AudioStreamBasicDescription, kAudioFormatFlagIsBigEndian, kAudioFormatFlagIsFloat,
    kAudioFormatFlagIsSignedInteger, kAudioFormatLinearPCM,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Depth {
    Sixteen,
    TwentyFour,
    ThirtyTwo,
    Float,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicalFormat(AudioStreamBasicDescription);

impl PhysicalFormat {
    pub(crate) const fn of(described: AudioStreamBasicDescription) -> Self {
        Self(described)
    }

    pub(crate) const fn described(&self) -> &AudioStreamBasicDescription {
        &self.0
    }

    pub const fn rate(&self) -> f64 {
        self.0.mSampleRate
    }

    pub const fn channels(&self) -> u32 {
        self.0.mChannelsPerFrame
    }

    pub const fn at(&self, hz: f64) -> Self {
        Self(AudioStreamBasicDescription {
            mSampleRate: hz,
            ..self.0
        })
    }

    pub const fn depth(&self) -> Option<Depth> {
        let flags = self.0.mFormatFlags;
        if self.0.mFormatID != kAudioFormatLinearPCM || flags & kAudioFormatFlagIsBigEndian != 0 {
            return None;
        }
        let float = flags & kAudioFormatFlagIsFloat != 0;
        let signed = flags & kAudioFormatFlagIsSignedInteger != 0;
        match (float, signed, self.0.mBitsPerChannel) {
            (true, _, 32) => Some(Depth::Float),
            (false, true, 16) => Some(Depth::Sixteen),
            (false, true, 24) => Some(Depth::TwentyFour),
            (false, true, 32) => Some(Depth::ThirtyTwo),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use objc2_core_audio_types::{
        AudioFormatFlags, kAudioFormatFlagIsPacked, kAudioFormatFlagsNativeFloatPacked,
    };

    use super::*;

    fn physical(flags: AudioFormatFlags, bits: u32) -> PhysicalFormat {
        PhysicalFormat::of(AudioStreamBasicDescription {
            mSampleRate: 96_000.0,
            mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: flags,
            mBytesPerPacket: 8,
            mFramesPerPacket: 1,
            mBytesPerFrame: 8,
            mChannelsPerFrame: 2,
            mBitsPerChannel: bits,
            mReserved: 0,
        })
    }

    #[test]
    fn a_devices_format_says_what_depth_it_carries_and_one_this_player_cannot_fill_says_none() {
        let signed = kAudioFormatFlagIsSignedInteger | kAudioFormatFlagIsPacked;

        assert_eq!(physical(signed, 16).depth(), Some(Depth::Sixteen));
        assert_eq!(physical(signed, 24).depth(), Some(Depth::TwentyFour));
        assert_eq!(physical(signed, 32).depth(), Some(Depth::ThirtyTwo));
        assert_eq!(
            physical(kAudioFormatFlagsNativeFloatPacked, 32).depth(),
            Some(Depth::Float)
        );
        assert_eq!(physical(signed, 20).depth(), None);
        assert_eq!(
            physical(signed | kAudioFormatFlagIsBigEndian, 24).depth(),
            None
        );
        assert_eq!(
            physical(kAudioFormatFlagsNativeFloatPacked, 64).depth(),
            None
        );
    }

    #[test]
    fn a_devices_format_moved_to_another_rate_is_the_devices_own_otherwise() {
        let signed = kAudioFormatFlagIsSignedInteger | kAudioFormatFlagIsPacked;
        let moved = physical(signed, 24).at(44_100.0);

        assert_eq!(moved.rate(), 44_100.0);
        assert_eq!(moved.channels(), 2);
        assert_eq!(moved.depth(), Some(Depth::TwentyFour));
        assert_eq!(moved.described().mBytesPerFrame, 8);
    }
}
