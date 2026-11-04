const WIDTH: usize = 4;

const STEP_AT: usize = 2;

const FIRST: [u8; WIDTH] = [0x80, 0, 0, 1];

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Place(pub Vec<u8>);

impl Place {
    pub fn first() -> Self {
        Self(FIRST.to_vec())
    }

    pub fn after(low: &Self) -> Self {
        let mut bytes = low.0.clone();
        if bytes.len() < WIDTH {
            bytes.resize(WIDTH, 0);
        }
        bytes.truncate(WIDTH);
        for at in (0..=STEP_AT).rev() {
            let (stepped, carried) = bytes[at].overflowing_add(1);
            bytes[at] = stepped;
            if !carried {
                *bytes.last_mut().unwrap_or(&mut 1) = 1;
                let place = Self(bytes);
                return if place > *low {
                    place
                } else {
                    Self::between(Some(low), None)
                };
            }
        }

        Self::between(Some(low), None)
    }

    pub fn before(high: &Self) -> Self {
        let mut bytes = high.0.clone();
        if bytes.len() < WIDTH {
            bytes.resize(WIDTH, 0);
        }
        bytes.truncate(WIDTH);
        for at in (0..=STEP_AT).rev() {
            let (stepped, borrowed) = bytes[at].overflowing_sub(1);
            bytes[at] = stepped;
            if !borrowed {
                *bytes.last_mut().unwrap_or(&mut 1) = 1;
                let place = Self(bytes);
                return if place < *high && place.0.iter().any(|byte| *byte != 0) {
                    place
                } else {
                    Self::between(None, Some(high))
                };
            }
        }

        Self::between(None, Some(high))
    }

    pub fn between(low: Option<&Self>, high: Option<&Self>) -> Self {
        let low: &[u8] = low.map_or(&[], |place| &place.0);
        let mut high: Option<&[u8]> = high.map(|place| place.0.as_slice());
        let mut bytes = Vec::with_capacity(low.len().max(WIDTH) + 1);

        for at in 0.. {
            let from = u16::from(low.get(at).copied().unwrap_or(0));
            let to = match high {
                Some(high) => u16::from(high.get(at).copied().unwrap_or(0)),
                None => 256,
            };
            if to > from + 1 {
                bytes.push(((from + to) / 2).min(255) as u8);
                return Self(bytes);
            }
            bytes.push(from as u8);
            if to == from + 1 {
                high = None;
            }
        }

        Self(bytes)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn places_after_the_first_keep_their_length_for_a_long_run_of_songs_added_at_the_end() {
        let mut place = Place::first();
        for _ in 0..20_000 {
            let next = Place::after(&place);
            assert!(next > place);
            assert_eq!(next.0.len(), WIDTH);
            place = next;
        }
    }

    #[test]
    fn places_before_the_first_stand_ahead_of_it_and_leave_room_between() {
        let first = Place::first();
        let ahead = Place::before(&first);
        let between = Place::between(Some(&ahead), Some(&first));

        assert!(ahead < first);
        assert!(ahead < between && between < first);
    }

    #[test]
    fn a_place_can_always_be_found_before_the_very_lowest() {
        let mut place = Place::first();
        for _ in 0..1_000 {
            let lower = Place::between(None, Some(&place));
            assert!(lower < place, "{lower:?} is not below {place:?}");
            assert!(lower.0.iter().any(|byte| *byte != 0));
            place = lower;
        }
    }

    proptest! {
        #[test]
        fn a_place_between_two_lies_strictly_between_them(
            one in proptest::collection::vec(any::<u8>(), 1..8),
            other in proptest::collection::vec(any::<u8>(), 1..8),
        ) {
            let mut one = one;
            let mut other = other;
            *one.last_mut().expect("not empty") |= 1;
            *other.last_mut().expect("not empty") |= 1;
            prop_assume!(one != other);
            let (low, high) = if one < other {
                (Place(one), Place(other))
            } else {
                (Place(other), Place(one))
            };

            let between = Place::between(Some(&low), Some(&high));

            prop_assert!(low < between, "{low:?} {between:?} {high:?}");
            prop_assert!(between < high, "{low:?} {between:?} {high:?}");
        }

        #[test]
        fn song_after_song_at_one_spot_keeps_finding_room(count in 1_usize..200) {
            let low = Place(vec![0x40, 0, 0, 1]);
            let high = Place(vec![0x40, 0, 0, 3]);
            let mut last = low.clone();

            for _ in 0..count {
                let next = Place::between(Some(&last), Some(&high));
                prop_assert!(last < next && next < high);
                last = next;
            }
        }
    }
}
