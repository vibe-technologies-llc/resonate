use std::{fmt, num::NonZeroU8};

use crate::{Error, Result};

const fn count(n: u8) -> ChannelCount {
    match NonZeroU8::new(n) {
        Some(n) => ChannelCount(n),
        None => panic!("a ChannelCount constant must be non-zero"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChannelCount(NonZeroU8);

impl ChannelCount {
    pub const MONO: Self = count(1);
    pub const STEREO: Self = count(2);
    pub const MAX: Self = count(64);

    pub const fn new(n: u16) -> Result<Self> {
        if n == 0 || n > Self::MAX.get() as u16 {
            return Err(Error::ChannelCountOutOfRange(n));
        }
        match NonZeroU8::new(n as u8) {
            Some(n) => Ok(Self(n)),
            None => Err(Error::ChannelCountOutOfRange(n)),
        }
    }

    pub const fn get(self) -> u8 {
        self.0.get()
    }
}

impl fmt::Display for ChannelCount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.get())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChannelLayout {
    Mono,
    Stereo,
    Quad,
    Surround51,
    Surround71,
    Discrete(ChannelCount),
}

impl ChannelLayout {
    pub const fn count(self) -> ChannelCount {
        match self {
            Self::Mono => ChannelCount::MONO,
            Self::Stereo => ChannelCount::STEREO,
            Self::Quad => count(4),
            Self::Surround51 => count(6),
            Self::Surround71 => count(8),
            Self::Discrete(n) => n,
        }
    }

    pub const fn positions(self) -> &'static [ChannelPosition] {
        use ChannelPosition::{
            FrontCenter, FrontLeft, FrontRight, Lfe, RearLeft, RearRight, SideLeft, SideRight,
        };
        match self {
            Self::Mono => &[FrontCenter],
            Self::Stereo => &[FrontLeft, FrontRight],
            Self::Quad => &[FrontLeft, FrontRight, RearLeft, RearRight],
            Self::Surround51 => &[FrontLeft, FrontRight, FrontCenter, Lfe, RearLeft, RearRight],
            Self::Surround71 => &[
                FrontLeft,
                FrontRight,
                FrontCenter,
                Lfe,
                RearLeft,
                RearRight,
                SideLeft,
                SideRight,
            ],
            Self::Discrete(_) => &[],
        }
    }

    pub const fn from_count(n: ChannelCount) -> Self {
        match n.get() {
            1 => Self::Mono,
            2 => Self::Stereo,
            4 => Self::Quad,
            6 => Self::Surround51,
            8 => Self::Surround71,
            _ => Self::Discrete(n),
        }
    }
}

impl fmt::Display for ChannelLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mono => f.write_str("mono"),
            Self::Stereo => f.write_str("stereo"),
            Self::Quad => f.write_str("quad"),
            Self::Surround51 => f.write_str("5.1"),
            Self::Surround71 => f.write_str("7.1"),
            Self::Discrete(n) => write!(f, "{n}-channel"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChannelPosition {
    FrontLeft,
    FrontRight,
    FrontCenter,
    Lfe,
    RearLeft,
    RearRight,
    SideLeft,
    SideRight,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_layouts_declare_one_position_per_channel() {
        for layout in [
            ChannelLayout::Mono,
            ChannelLayout::Stereo,
            ChannelLayout::Quad,
            ChannelLayout::Surround51,
            ChannelLayout::Surround71,
        ] {
            assert_eq!(
                layout.positions().len(),
                layout.count().get() as usize,
                "{layout} disagrees with its own channel count"
            );
        }
    }

    #[test]
    fn from_count_round_trips_through_named_layouts() {
        let layout = ChannelLayout::from_count(ChannelCount::STEREO);
        assert_eq!(layout, ChannelLayout::Stereo);
        assert_eq!(layout.count(), ChannelCount::STEREO);
    }

    #[test]
    fn unusual_channel_counts_fall_back_to_discrete() {
        let three = ChannelCount::new(3).expect("3 is in range");
        assert_eq!(
            ChannelLayout::from_count(three),
            ChannelLayout::Discrete(three)
        );
    }

    #[test]
    fn channel_counts_outside_the_spa_limit_are_rejected() {
        assert!(ChannelCount::new(0).is_err());
        assert!(ChannelCount::new(65).is_err());
        assert!(ChannelCount::new(64).is_ok());
    }
}
