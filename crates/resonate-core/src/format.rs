use std::{
    fmt,
    num::{NonZeroU8, NonZeroU32},
};

use crate::{ChannelCount, ChannelLayout, Error, Frames, Result};

const fn rate(hz: u32) -> SampleRate {
    match NonZeroU32::new(hz) {
        Some(hz) => SampleRate(hz),
        None => panic!("a SampleRate constant must be non-zero"),
    }
}

const fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SampleRate(NonZeroU32);

impl SampleRate {
    pub const MIN_HZ: u32 = 8_000;
    pub const MAX_HZ: u32 = 768_000;

    pub const HZ_8000: Self = rate(8_000);
    pub const HZ_16000: Self = rate(16_000);
    pub const HZ_22050: Self = rate(22_050);
    pub const HZ_44100: Self = rate(44_100);
    pub const HZ_48000: Self = rate(48_000);
    pub const HZ_88200: Self = rate(88_200);
    pub const HZ_96000: Self = rate(96_000);
    pub const HZ_176400: Self = rate(176_400);
    pub const HZ_192000: Self = rate(192_000);
    pub const HZ_352800: Self = rate(352_800);
    pub const HZ_384000: Self = rate(384_000);

    pub const fn new(hz: u32) -> Result<Self> {
        if hz < Self::MIN_HZ || hz > Self::MAX_HZ {
            return Err(Error::SampleRateOutOfRange(hz));
        }
        match NonZeroU32::new(hz) {
            Some(hz) => Ok(Self(hz)),
            None => Err(Error::SampleRateOutOfRange(hz)),
        }
    }

    pub const fn hz(self) -> u32 {
        self.0.get()
    }

    pub const fn family(self) -> RateFamily {
        let hz = self.hz();
        if hz.is_multiple_of(11_025) {
            RateFamily::Base44100
        } else if hz.is_multiple_of(8_000) {
            RateFamily::Base48000
        } else {
            RateFamily::Other
        }
    }

    pub const fn is_integer_multiple_of(self, base: Self) -> bool {
        self.hz().is_multiple_of(base.hz())
    }

    pub const fn ratio_to(self, target: Self) -> Ratio {
        let divisor = gcd(self.hz(), target.hz());
        match (
            NonZeroU32::new(target.hz() / divisor),
            NonZeroU32::new(self.hz() / divisor),
        ) {
            (Some(numer), Some(denom)) => Ratio { numer, denom },
            _ => panic!("a reduced rate ratio cannot have a zero term"),
        }
    }
}

const HZ_A_KILOHERTZ: u32 = 1_000;
const DIGITS_OF_A_KILOHERTZ: usize = 3;

impl fmt::Display for SampleRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hz = self.hz();
        let mut fraction = hz % HZ_A_KILOHERTZ;
        if fraction == 0 {
            return write!(f, "{} kHz", hz / HZ_A_KILOHERTZ);
        }
        let mut digits = DIGITS_OF_A_KILOHERTZ;
        while fraction.is_multiple_of(10) {
            fraction /= 10;
            digits -= 1;
        }
        write!(f, "{}.{fraction:0digits$} kHz", hz / HZ_A_KILOHERTZ)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RateFamily {
    Base44100,
    Base48000,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ratio {
    pub numer: NonZeroU32,
    pub denom: NonZeroU32,
}

impl Ratio {
    pub const fn as_f64(self) -> f64 {
        self.numer.get() as f64 / self.denom.get() as f64
    }
}

impl fmt::Display for Ratio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.numer, self.denom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BitDepth {
    Bits8,
    Bits16,
    Bits18,
    Bits20,
    Bits24,
    Bits32,
}

impl BitDepth {
    pub const fn bits(self) -> NonZeroU8 {
        let bits = match self {
            Self::Bits8 => 8,
            Self::Bits16 => 16,
            Self::Bits18 => 18,
            Self::Bits20 => 20,
            Self::Bits24 => 24,
            Self::Bits32 => 32,
        };
        match NonZeroU8::new(bits) {
            Some(bits) => bits,
            None => panic!("a BitDepth is never zero bits"),
        }
    }

    pub const fn from_bits(bits: u8) -> Result<Self> {
        match bits {
            8 => Ok(Self::Bits8),
            16 => Ok(Self::Bits16),
            18 => Ok(Self::Bits18),
            20 => Ok(Self::Bits20),
            24 => Ok(Self::Bits24),
            32 => Ok(Self::Bits32),
            other => Err(Error::BitDepthNotRepresentable(other)),
        }
    }
}

impl fmt::Display for BitDepth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-bit", self.bits())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SampleFormat {
    S16,
    S24,
    S32,
    F32,
}

impl SampleFormat {
    const F32_SIGNIFICANT_BITS: u8 = 24;

    pub const ALL: [Self; 4] = [Self::S16, Self::S24, Self::S32, Self::F32];

    pub const MAX_BYTES: usize = 4;

    pub const fn bytes_per_sample(self) -> NonZeroU8 {
        let bytes = match self {
            Self::S16 => 2,
            Self::S24 | Self::S32 | Self::F32 => 4,
        };
        match NonZeroU8::new(bytes) {
            Some(bytes) => bytes,
            None => panic!("a sample is never zero bytes wide"),
        }
    }

    pub const fn valid_bits(self) -> u8 {
        match self {
            Self::S16 => 16,
            Self::S24 | Self::F32 => Self::F32_SIGNIFICANT_BITS,
            Self::S32 => 32,
        }
    }

    pub const fn bit_depth(self) -> BitDepth {
        match self {
            Self::S16 => BitDepth::Bits16,
            Self::S24 | Self::F32 => BitDepth::Bits24,
            Self::S32 => BitDepth::Bits32,
        }
    }

    pub const fn is_integer(self) -> bool {
        !self.is_float()
    }

    pub const fn is_float(self) -> bool {
        matches!(self, Self::F32)
    }

    pub const fn full_scale(self) -> f32 {
        match self {
            Self::S16 => 32_768.0,
            Self::S24 => 8_388_608.0,
            Self::S32 => 2_147_483_648.0,
            Self::F32 => 1.0,
        }
    }

    pub const fn losslessly_holds(self, source: Self) -> bool {
        match (self, source) {
            (Self::F32, Self::F32) => true,
            (Self::F32, source) => source.valid_bits() <= Self::F32_SIGNIFICANT_BITS,
            (_, Self::F32) => false,
            (dst, source) => source.valid_bits() <= dst.valid_bits(),
        }
    }
}

impl fmt::Display for SampleFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::S16 => "S16LE",
            Self::S24 => "S24",
            Self::S32 => "S32LE",
            Self::F32 => "F32LE",
        };
        f.write_str(name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StreamSpec {
    pub rate: SampleRate,
    pub channels: ChannelLayout,
    pub format: SampleFormat,
}

impl StreamSpec {
    pub const fn new(rate: SampleRate, channels: ChannelLayout, format: SampleFormat) -> Self {
        Self {
            rate,
            channels,
            format,
        }
    }

    pub const fn channel_count(self) -> ChannelCount {
        self.channels.count()
    }

    pub const fn bytes_per_frame(self) -> NonZeroU32 {
        let bytes = self.format.bytes_per_sample().get() as u32 * self.channel_count().get() as u32;
        match NonZeroU32::new(bytes) {
            Some(bytes) => bytes,
            None => panic!("a frame is never zero bytes wide"),
        }
    }

    pub const fn frames_to_bytes(self, frames: Frames) -> u64 {
        frames.0.saturating_mul(self.bytes_per_frame().get() as u64)
    }

    pub const fn bytes_to_frames(self, bytes: u64) -> Frames {
        Frames(bytes / self.bytes_per_frame().get() as u64)
    }

    pub const fn losslessly_holds(self, source: Self) -> bool {
        self.rate.hz() == source.rate.hz()
            && self.channels.count().get() == source.channels.count().get()
            && self.format.losslessly_holds(source.format)
    }
}

impl fmt::Display for StreamSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}", self.rate, self.format, self.channels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rate_is_written_in_kilohertz_to_every_digit_it_holds() {
        for (hz, written) in [
            (8_000, "8 kHz"),
            (11_025, "11.025 kHz"),
            (22_050, "22.05 kHz"),
            (44_100, "44.1 kHz"),
            (48_000, "48 kHz"),
            (352_800, "352.8 kHz"),
            (705_600, "705.6 kHz"),
        ] {
            let rate = SampleRate::new(hz).expect("a supported rate");
            assert_eq!(rate.to_string(), written);
        }
    }

    #[test]
    fn f32_cannot_losslessly_hold_s32() {
        assert!(!SampleFormat::F32.losslessly_holds(SampleFormat::S32));
        assert!(SampleFormat::F32.losslessly_holds(SampleFormat::S24));
        assert!(SampleFormat::F32.losslessly_holds(SampleFormat::S16));
    }

    #[test]
    fn integer_formats_widen_losslessly_but_do_not_narrow() {
        assert!(SampleFormat::S32.losslessly_holds(SampleFormat::S24));
        assert!(SampleFormat::S24.losslessly_holds(SampleFormat::S16));
        assert!(!SampleFormat::S24.losslessly_holds(SampleFormat::S32));
        assert!(!SampleFormat::S16.losslessly_holds(SampleFormat::S24));
    }

    #[test]
    fn no_integer_format_holds_f32() {
        assert!(!SampleFormat::S32.losslessly_holds(SampleFormat::F32));
        assert!(!SampleFormat::S24.losslessly_holds(SampleFormat::F32));
    }

    #[test]
    fn every_format_holds_itself() {
        for format in [
            SampleFormat::S16,
            SampleFormat::S24,
            SampleFormat::S32,
            SampleFormat::F32,
        ] {
            assert!(format.losslessly_holds(format), "{format} lost itself");
        }
    }

    #[test]
    fn rates_group_into_families() {
        assert_eq!(SampleRate::HZ_44100.family(), RateFamily::Base44100);
        assert_eq!(SampleRate::HZ_352800.family(), RateFamily::Base44100);
        assert_eq!(SampleRate::HZ_48000.family(), RateFamily::Base48000);
        assert_eq!(SampleRate::HZ_384000.family(), RateFamily::Base48000);
    }

    #[test]
    fn rate_ratios_are_reduced() {
        let ratio = SampleRate::HZ_44100.ratio_to(SampleRate::HZ_88200);
        assert_eq!(ratio.numer.get(), 2);
        assert_eq!(ratio.denom.get(), 1);

        let ratio = SampleRate::HZ_44100.ratio_to(SampleRate::HZ_48000);
        assert_eq!(ratio.numer.get(), 160);
        assert_eq!(ratio.denom.get(), 147);
    }

    #[test]
    fn rates_outside_the_hardware_range_are_rejected() {
        assert!(SampleRate::new(0).is_err());
        assert!(SampleRate::new(SampleRate::MIN_HZ - 1).is_err());
        assert!(SampleRate::new(SampleRate::MAX_HZ + 1).is_err());
        assert!(SampleRate::new(SampleRate::MAX_HZ).is_ok());
    }

    #[test]
    fn no_format_is_wider_than_the_widest_a_sample_word_holds() {
        for format in SampleFormat::ALL {
            assert!(
                usize::from(format.bytes_per_sample().get()) <= SampleFormat::MAX_BYTES,
                "{format} outgrew a sample word"
            );
        }
    }

    #[test]
    fn error_stays_small_enough_for_result_large_err() {
        assert!(size_of::<Error>() <= 128);
    }
}
