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
    Surround50,
    Surround51,
    Surround61,
    Surround71,
    Placed(Places),
    Discrete(ChannelCount),
}

impl ChannelLayout {
    pub const fn count(self) -> ChannelCount {
        match self {
            Self::Mono => ChannelCount::MONO,
            Self::Stereo => ChannelCount::STEREO,
            Self::Quad => count(4),
            Self::Surround50 => count(5),
            Self::Surround51 => count(6),
            Self::Surround61 => count(7),
            Self::Surround71 => count(8),
            Self::Placed(places) => places.count(),
            Self::Discrete(n) => n,
        }
    }

    pub const NAMED: [Self; 7] = [
        Self::Mono,
        Self::Stereo,
        Self::Quad,
        Self::Surround50,
        Self::Surround51,
        Self::Surround61,
        Self::Surround71,
    ];

    pub fn placed(places: Places) -> Option<Self> {
        if places.is_empty() {
            return None;
        }
        let named = Self::NAMED
            .into_iter()
            .find(|named| Places::of(named.positions().iter().copied()).stands_for(places));
        Some(match named {
            Some(named) => named,
            None if places.len() >= PLACED_FROM_CHANNELS => Self::Placed(places),
            None => Self::Discrete(places.count()),
        })
    }

    pub const fn positions(self) -> &'static [ChannelPosition] {
        use ChannelPosition::{
            FrontCenter, FrontLeft, FrontRight, Lfe, RearCenter, RearLeft, RearRight, SideLeft,
            SideRight,
        };
        match self {
            Self::Mono => &[FrontCenter],
            Self::Stereo => &[FrontLeft, FrontRight],
            Self::Quad => &[FrontLeft, FrontRight, RearLeft, RearRight],
            Self::Surround50 => &[FrontLeft, FrontRight, FrontCenter, RearLeft, RearRight],
            Self::Surround51 => &[FrontLeft, FrontRight, FrontCenter, Lfe, RearLeft, RearRight],
            Self::Surround61 => &[
                FrontLeft,
                FrontRight,
                FrontCenter,
                Lfe,
                RearCenter,
                SideLeft,
                SideRight,
            ],
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
            Self::Placed(places) => places.positions(),
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
            Self::Surround50 => f.write_str("5.0"),
            Self::Surround51 => f.write_str("5.1"),
            Self::Surround61 => f.write_str("6.1"),
            Self::Surround71 => f.write_str("7.1"),
            Self::Placed(places) => {
                let low = usize::from(places.holds(ChannelPosition::Lfe));
                write!(f, "{}.{low}", places.len() - low)
            }
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
    RearCenter,
    SideLeft,
    SideRight,
}

impl ChannelPosition {
    pub const IN_WAVE_ORDER: [Self; 9] = [
        Self::FrontLeft,
        Self::FrontRight,
        Self::FrontCenter,
        Self::Lfe,
        Self::RearLeft,
        Self::RearRight,
        Self::RearCenter,
        Self::SideLeft,
        Self::SideRight,
    ];

    const fn bit(self) -> u16 {
        1 << self as u16
    }
}

const PLACED_FROM_CHANNELS: usize = 3;
const EVERY_SET_OF_PLACES: usize = 1 << ChannelPosition::IN_WAVE_ORDER.len();
const REAR_PAIR: u16 = ChannelPosition::RearLeft.bit() | ChannelPosition::RearRight.bit();
const SIDE_PAIR: u16 = ChannelPosition::SideLeft.bit() | ChannelPosition::SideRight.bit();

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Places(u16);

impl Places {
    pub fn of(positions: impl IntoIterator<Item = ChannelPosition>) -> Self {
        Self(
            positions
                .into_iter()
                .fold(0, |held, position| held | position.bit()),
        )
    }

    pub fn in_order(positions: impl IntoIterator<Item = ChannelPosition>) -> Option<Self> {
        positions.into_iter().try_fold(Self(0), |held, position| {
            (position.bit() > held.0).then_some(Self(held.0 | position.bit()))
        })
    }

    pub const fn holds(self, position: ChannelPosition) -> bool {
        self.0 & position.bit() != 0
    }

    pub const fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    const fn count(self) -> ChannelCount {
        match NonZeroU8::new(self.0.count_ones() as u8) {
            Some(n) => ChannelCount(n),
            None => ChannelCount::MONO,
        }
    }

    const fn positions(self) -> &'static [ChannelPosition] {
        let laid = &LAID_IN_WAVE_ORDER[self.0 as usize % EVERY_SET_OF_PLACES];
        match laid.positions.split_at_checked(laid.len) {
            Some((held, _)) => held,
            None => &[],
        }
    }

    const fn stands_for(self, given: Self) -> bool {
        let sides_for_rears =
            self.0 & SIDE_PAIR == 0 && given.0 & REAR_PAIR == 0 && given.0 & SIDE_PAIR == SIDE_PAIR;
        let given = if sides_for_rears {
            given.0 & !SIDE_PAIR | REAR_PAIR
        } else {
            given.0
        };
        self.0 == given
    }
}

#[derive(Clone, Copy)]
struct Laid {
    positions: [ChannelPosition; ChannelPosition::IN_WAVE_ORDER.len()],
    len: usize,
}

static LAID_IN_WAVE_ORDER: [Laid; EVERY_SET_OF_PLACES] = laid_in_wave_order();

const fn laid_in_wave_order() -> [Laid; EVERY_SET_OF_PLACES] {
    let mut every = [Laid {
        positions: [ChannelPosition::FrontLeft; ChannelPosition::IN_WAVE_ORDER.len()],
        len: 0,
    }; EVERY_SET_OF_PLACES];
    let mut set = 0;
    while set < EVERY_SET_OF_PLACES {
        let mut place = 0;
        while place < ChannelPosition::IN_WAVE_ORDER.len() {
            if set & (1 << place) != 0 {
                every[set].positions[every[set].len] = ChannelPosition::IN_WAVE_ORDER[place];
                every[set].len += 1;
            }
            place += 1;
        }
        set += 1;
    }
    every
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
            ChannelLayout::Surround50,
            ChannelLayout::Surround51,
            ChannelLayout::Surround61,
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

    fn placed(positions: &[ChannelPosition]) -> Option<ChannelLayout> {
        ChannelLayout::placed(Places::of(positions.iter().copied()))
    }

    #[test]
    fn a_placed_set_is_named_where_a_named_layout_holds_it_and_kept_placed_where_none_does() {
        use ChannelPosition::{
            FrontCenter, FrontLeft, FrontRight, Lfe, RearCenter, RearLeft, RearRight, SideLeft,
            SideRight,
        };

        assert_eq!(
            placed(&[FrontRight, FrontLeft, FrontCenter, Lfe, SideLeft, SideRight]),
            Some(ChannelLayout::Surround51),
            "a 5.1 with its surrounds on the sides was not read as 5.1"
        );
        assert_eq!(
            placed(&[FrontLeft, FrontRight, SideLeft, SideRight]),
            Some(ChannelLayout::Quad)
        );
        assert_eq!(placed(&[FrontCenter]), Some(ChannelLayout::Mono));

        let three = placed(&[FrontLeft, FrontRight, FrontCenter]).expect("three places");
        assert!(matches!(three, ChannelLayout::Placed(_)));
        assert_eq!(three.positions(), [FrontLeft, FrontRight, FrontCenter]);
        assert_eq!(three.to_string(), "3.0");

        let seven = placed(&[
            SideRight,
            SideLeft,
            RearRight,
            RearLeft,
            FrontCenter,
            FrontRight,
            FrontLeft,
        ])
        .expect("seven places");
        assert_eq!(seven.count().get(), 7);
        assert_eq!(
            seven.positions(),
            [
                FrontLeft,
                FrontRight,
                FrontCenter,
                RearLeft,
                RearRight,
                SideLeft,
                SideRight
            ],
            "a 7.0 was not laid out in the order its channels arrive"
        );
        assert_eq!(seven.to_string(), "7.0");

        let four_one =
            placed(&[FrontLeft, FrontRight, FrontCenter, Lfe, RearCenter]).expect("five places");
        assert_eq!(four_one.to_string(), "4.1");
        assert_eq!(
            four_one.positions(),
            [FrontLeft, FrontRight, FrontCenter, Lfe, RearCenter]
        );

        assert_eq!(
            placed(&[FrontLeft]),
            Some(ChannelLayout::Discrete(ChannelCount::MONO)),
            "one channel placed off the centre was named"
        );
        assert_eq!(placed(&[]), None);
    }

    #[test]
    fn places_in_order_refuse_a_set_laid_out_of_wave_order() {
        use ChannelPosition::{FrontCenter, FrontLeft, FrontRight, Lfe};

        assert_eq!(
            Places::in_order([FrontLeft, FrontRight, FrontCenter, Lfe]),
            Some(Places::of([Lfe, FrontCenter, FrontRight, FrontLeft]))
        );
        assert_eq!(Places::in_order([FrontLeft, FrontCenter, FrontRight]), None);
        assert_eq!(Places::in_order([FrontLeft, FrontLeft]), None);
    }

    #[test]
    fn every_named_layout_lays_its_positions_out_in_wave_order() {
        for layout in ChannelLayout::NAMED {
            assert_eq!(
                Places::in_order(layout.positions().iter().copied()),
                Some(Places::of(layout.positions().iter().copied())),
                "{layout} is not laid out in the order its channels arrive"
            );
        }
    }

    #[test]
    fn channel_counts_outside_the_spa_limit_are_rejected() {
        assert!(ChannelCount::new(0).is_err());
        assert!(ChannelCount::new(65).is_err());
        assert!(ChannelCount::new(64).is_ok());
    }
}
