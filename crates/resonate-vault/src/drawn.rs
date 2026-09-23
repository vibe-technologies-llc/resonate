use std::collections::VecDeque;

use parking_lot::Mutex;
use resonate_codec::CoverArt;

use crate::key::VaultKey;

pub(crate) const DRAWN_BYTES_AT_MOST: usize = 48 << 20;

#[derive(Default)]
pub(crate) struct Drawings {
    held: Mutex<Held>,
}

#[derive(Default)]
struct Held {
    newest_last: VecDeque<(VaultKey, CoverArt)>,
    bytes: usize,
}

impl Drawings {
    pub(crate) fn drawn(&self, key: VaultKey) -> Option<CoverArt> {
        let mut held = self.held.lock();
        let at = held.newest_last.iter().position(|(held, _)| *held == key)?;
        let found = held.newest_last.remove(at)?;
        let art = found.1.clone();
        held.newest_last.push_back(found);
        Some(art)
    }

    pub(crate) fn forget(&self, key: VaultKey) {
        let mut held = self.held.lock();
        if let Some(at) = held.newest_last.iter().position(|(held, _)| *held == key)
            && let Some((_, gone)) = held.newest_last.remove(at)
        {
            held.bytes -= gone.bytes.len();
        }
    }

    pub(crate) fn note(&self, key: VaultKey, art: &CoverArt) {
        let weight = art.bytes.len();
        if weight > DRAWN_BYTES_AT_MOST {
            return;
        }

        self.forget(key);
        let mut held = self.held.lock();
        while held.bytes + weight > DRAWN_BYTES_AT_MOST {
            let Some((_, gone)) = held.newest_last.pop_front() else {
                break;
            };
            held.bytes -= gone.bytes.len();
        }
        held.bytes += weight;
        held.newest_last.push_back((key, art.clone()));
    }
}

#[cfg(test)]
mod tests {
    use resonate_codec::ImageFormat;

    use super::*;

    fn art(weight: usize) -> CoverArt {
        CoverArt {
            format: ImageFormat::Png,
            bytes: vec![0; weight],
        }
    }

    fn key(first: u8) -> VaultKey {
        let mut bytes = [0; 16];
        bytes[0] = first;
        VaultKey::of(bytes)
    }

    #[test]
    fn a_drawing_noted_is_handed_back_and_a_key_never_noted_is_not() {
        let drawings = Drawings::default();
        drawings.note(key(1), &art(10));

        assert_eq!(drawings.drawn(key(1)), Some(art(10)));
        assert_eq!(drawings.drawn(key(2)), None);
    }

    #[test]
    fn the_drawing_asked_for_least_lately_goes_first_once_the_bound_is_reached() {
        let drawings = Drawings::default();
        let third = DRAWN_BYTES_AT_MOST / 3;
        drawings.note(key(1), &art(third));
        drawings.note(key(2), &art(third));
        drawings.note(key(3), &art(third));
        assert!(drawings.drawn(key(1)).is_some());

        drawings.note(key(4), &art(third));

        assert!(drawings.drawn(key(1)).is_some());
        assert!(drawings.drawn(key(2)).is_none());
        assert!(drawings.drawn(key(3)).is_some());
        assert!(drawings.drawn(key(4)).is_some());
    }

    #[test]
    fn a_drawing_heavier_than_the_whole_bound_is_not_held_and_takes_nothing_with_it() {
        let drawings = Drawings::default();
        drawings.note(key(1), &art(10));
        drawings.note(key(2), &art(DRAWN_BYTES_AT_MOST + 1));

        assert!(drawings.drawn(key(1)).is_some());
        assert!(drawings.drawn(key(2)).is_none());
    }

    #[test]
    fn noting_one_key_twice_weighs_it_once() {
        let drawings = Drawings::default();
        let half = DRAWN_BYTES_AT_MOST / 2;
        drawings.note(key(1), &art(half));
        drawings.note(key(1), &art(half));
        drawings.note(key(2), &art(half));

        assert!(drawings.drawn(key(1)).is_some());
        assert!(drawings.drawn(key(2)).is_some());
    }
}
