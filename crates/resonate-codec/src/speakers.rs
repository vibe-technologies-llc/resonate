use symphonia::core::audio::{Channels, Position};

const WAVE_POSITIONS: u32 = 18;
const WIDEST_FLAC: u32 = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Speakers(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Placement {
    Front,
    Side,
    Rear,
    Raised,
    Lfe,
}

impl Placement {
    fn of(position: Position) -> Self {
        if position.intersects(Position::LFE1 | Position::LFE2) {
            Self::Lfe
        } else if position.intersects(Position::SIDE_LEFT | Position::SIDE_RIGHT) {
            Self::Side
        } else if position
            .intersects(Position::REAR_LEFT | Position::REAR_RIGHT | Position::REAR_CENTER)
        {
            Self::Rear
        } else if position.intersects(
            Position::TOP_CENTER
                | Position::TOP_FRONT_LEFT
                | Position::TOP_FRONT_CENTER
                | Position::TOP_FRONT_RIGHT
                | Position::TOP_REAR_LEFT
                | Position::TOP_REAR_CENTER
                | Position::TOP_REAR_RIGHT
                | Position::TOP_SIDE_LEFT
                | Position::TOP_SIDE_RIGHT
                | Position::BOTTOM_FRONT_CENTER
                | Position::BOTTOM_FRONT_LEFT
                | Position::BOTTOM_FRONT_RIGHT,
        ) {
            Self::Raised
        } else {
            Self::Front
        }
    }
}

impl Speakers {
    pub const UNNAMED: Self = Self(0);

    pub(crate) fn of(channels: Option<&Channels>) -> Self {
        match channels {
            Some(Channels::Positioned(positions)) => Self(positions.bits()),
            _ => Self::UNNAMED,
        }
    }

    pub const fn is_named(self) -> bool {
        self.0 != 0
    }

    pub const fn wave_mask(self) -> Option<u32> {
        if self.0 >> WAVE_POSITIONS == 0 {
            Some(self.0 as u32)
        } else {
            None
        }
    }

    pub fn in_flac_order(channels: u8) -> Option<Self> {
        let positions = match u32::from(channels) {
            1 => Position::FRONT_LEFT,
            2 => Position::FRONT_LEFT | Position::FRONT_RIGHT,
            3 => Position::FRONT_LEFT | Position::FRONT_RIGHT | Position::FRONT_CENTER,
            4 => {
                Position::FRONT_LEFT
                    | Position::FRONT_RIGHT
                    | Position::REAR_LEFT
                    | Position::REAR_RIGHT
            }
            5 => {
                Position::FRONT_LEFT
                    | Position::FRONT_RIGHT
                    | Position::FRONT_CENTER
                    | Position::REAR_LEFT
                    | Position::REAR_RIGHT
            }
            6 => {
                Position::FRONT_LEFT
                    | Position::FRONT_RIGHT
                    | Position::FRONT_CENTER
                    | Position::LFE1
                    | Position::REAR_LEFT
                    | Position::REAR_RIGHT
            }
            7 => {
                Position::FRONT_LEFT
                    | Position::FRONT_RIGHT
                    | Position::FRONT_CENTER
                    | Position::LFE1
                    | Position::REAR_CENTER
                    | Position::SIDE_LEFT
                    | Position::SIDE_RIGHT
            }
            WIDEST_FLAC => {
                Position::FRONT_LEFT
                    | Position::FRONT_RIGHT
                    | Position::FRONT_CENTER
                    | Position::LFE1
                    | Position::REAR_LEFT
                    | Position::REAR_RIGHT
                    | Position::SIDE_LEFT
                    | Position::SIDE_RIGHT
            }
            _ => return None,
        };
        Some(Self(positions.bits()))
    }

    pub fn in_wave_order(channels: u8) -> Option<Self> {
        Position::from_wave_channel_count(u32::from(channels))
            .map(|positions| Self(positions.bits()))
    }

    pub fn placements(self, channels: u8) -> Vec<Placement> {
        let named = if self.is_named() {
            Some(self)
        } else {
            Self::in_wave_order(channels)
        };
        let mut bits = named.map_or(0, |speakers| speakers.0);
        (0..channels)
            .map(|_| {
                if bits == 0 {
                    return Placement::Front;
                }
                let lowest = bits.isolate_lowest_one();
                bits &= !lowest;
                Placement::of(Position::from_bits_truncate(lowest))
            })
            .collect()
    }

    pub fn held_by(self, read_back: Self) -> bool {
        !self.is_named() || self == read_back
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_layout_is_carried_by_flac_only_where_it_is_the_order_flac_assigns() {
        let quad = Speakers::of(Some(&Channels::Positioned(
            Position::FRONT_LEFT
                | Position::FRONT_RIGHT
                | Position::REAR_LEFT
                | Position::REAR_RIGHT,
        )));
        let two_point_one = Speakers::of(Some(&Channels::Positioned(
            Position::FRONT_LEFT | Position::FRONT_RIGHT | Position::LFE1,
        )));

        assert_eq!(Speakers::in_flac_order(4), Some(quad));
        assert_ne!(Speakers::in_flac_order(3), Some(two_point_one));
        assert_eq!(Speakers::in_flac_order(9), None);
    }

    #[test]
    fn a_speaker_is_placed_by_the_position_its_channel_names() {
        let side = Speakers::of(Some(&Channels::Positioned(
            Position::FRONT_LEFT
                | Position::FRONT_RIGHT
                | Position::FRONT_CENTER
                | Position::LFE1
                | Position::SIDE_LEFT
                | Position::SIDE_RIGHT,
        )));

        assert_eq!(
            side.placements(6),
            [
                Placement::Front,
                Placement::Front,
                Placement::Front,
                Placement::Lfe,
                Placement::Side,
                Placement::Side,
            ]
        );
        assert_eq!(
            Speakers::UNNAMED.placements(6)[3..],
            [Placement::Lfe, Placement::Rear, Placement::Rear]
        );
        assert_eq!(Speakers::UNNAMED.placements(20).len(), 20);
    }

    #[test]
    fn positions_past_what_a_wave_mask_names_have_no_mask() {
        let wide = Speakers::of(Some(&Channels::Positioned(
            Position::FRONT_LEFT | Position::FRONT_LEFT_WIDE,
        )));

        assert_eq!(wide.wave_mask(), None);
        assert_eq!(Speakers::UNNAMED.wave_mask(), Some(0));
    }

    #[test]
    fn a_source_that_names_no_positions_is_held_by_any_reading() {
        let quad = Speakers::in_flac_order(4).expect("a quad layout");

        assert!(Speakers::UNNAMED.held_by(quad));
        assert!(quad.held_by(quad));
        assert!(!quad.held_by(Speakers::in_wave_order(4).expect("four wave positions")));
    }
}
