use std::{
    hash::Hash,
    time::{Duration, Instant},
};

use ahash::{AHashMap, AHashSet};

use crate::serial::Serial;

pub(crate) const DECODES_AT_ONCE: usize = 4;

pub(crate) const FETCHES_AT_ONCE: usize = 6;

pub(crate) const FETCHES_FROM_ONE_SERVER_AT_ONCE: usize = 4;

pub(crate) const A_FAILED_READ_IS_ASKED_AGAIN_AFTER: Duration = Duration::from_secs(30);

pub(crate) const A_FAILED_FETCH_IS_ASKED_AGAIN_AFTER: Duration = Duration::from_secs(60);

pub(crate) struct Underway<K> {
    at_once: usize,
    asked_again_after: Duration,
    underway: AHashSet<K>,
    failed: AHashMap<K, Instant>,
    forgotten: Serial,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Asked {
    forgotten: Serial,
}

impl<K: Clone + Eq + Hash> Underway<K> {
    pub(crate) fn decoding() -> Self {
        Self::new(DECODES_AT_ONCE, A_FAILED_READ_IS_ASKED_AGAIN_AFTER)
    }

    pub(crate) fn fetching() -> Self {
        Self::new(FETCHES_AT_ONCE, A_FAILED_FETCH_IS_ASKED_AGAIN_AFTER)
    }

    pub(crate) fn fetching_from_one_server() -> Self {
        Self::new(
            FETCHES_FROM_ONE_SERVER_AT_ONCE,
            A_FAILED_FETCH_IS_ASKED_AGAIN_AFTER,
        )
    }

    fn new(at_once: usize, asked_again_after: Duration) -> Self {
        Self {
            at_once,
            asked_again_after,
            underway: AHashSet::new(),
            failed: AHashMap::new(),
            forgotten: Serial::default(),
        }
    }

    pub(crate) fn holds(&self, key: &K) -> bool {
        self.underway.contains(key)
    }

    #[cfg(test)]
    pub(crate) fn is_idle(&self) -> bool {
        self.underway.is_empty()
    }

    pub(crate) fn has_room_beyond(&self, kept: usize) -> bool {
        self.underway.len() + kept < self.at_once
    }

    pub(crate) fn may_ask(&self, key: &K) -> bool {
        !self.holds(key)
            && self.has_room_beyond(0)
            && !self
                .failed
                .get(key)
                .is_some_and(|failed| failed.elapsed() < self.asked_again_after)
    }

    pub(crate) fn begin(&mut self, key: K) -> Asked {
        self.underway.insert(key);
        Asked {
            forgotten: self.forgotten,
        }
    }

    pub(crate) fn answered(&mut self, key: &K, asked: Asked) -> bool {
        let current = self.left(key, asked);
        if current {
            self.failed.remove(key);
        }
        current
    }

    pub(crate) fn failed(&mut self, key: K, asked: Asked) -> bool {
        let current = self.left(&key, asked);
        if current {
            self.failed.insert(key, Instant::now());
        }
        current
    }

    pub(crate) fn left(&mut self, key: &K, asked: Asked) -> bool {
        let current = asked.forgotten == self.forgotten;
        if current {
            self.underway.remove(key);
        }
        current
    }

    pub(crate) fn forget(&mut self) {
        self.underway.clear();
        self.failed.clear();
        self.forgotten.moved();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_from_before_a_reset_neither_lands_nor_frees_the_key_a_newer_ask_holds() {
        let mut underway = Underway::fetching_from_one_server();

        let before = underway.begin("al-1");
        underway.forget();
        let after = underway.begin("al-1");

        assert!(!underway.answered(&"al-1", before));
        assert!(underway.holds(&"al-1"));

        assert!(underway.answered(&"al-1", after));
        assert!(!underway.holds(&"al-1"));
        assert!(underway.is_idle());
    }

    #[test]
    fn no_more_are_asked_at_once_than_the_limit_and_a_key_underway_is_not_asked_twice() {
        let mut underway = Underway::decoding();

        for key in 0..DECODES_AT_ONCE {
            assert!(underway.may_ask(&key));
            underway.begin(key);
        }

        assert!(!underway.may_ask(&0));
        assert!(!underway.may_ask(&DECODES_AT_ONCE));
        assert!(!underway.has_room_beyond(0));
    }

    #[test]
    fn a_failure_is_not_asked_again_until_its_wait_is_out_and_an_answer_clears_it() {
        let mut underway = Underway::fetching();

        let asked = underway.begin("cover");
        assert!(underway.failed("cover", asked));

        assert!(!underway.may_ask(&"cover"));
        assert!(underway.may_ask(&"another"));

        let asked = underway.begin("cover");
        assert!(underway.answered(&"cover", asked));
        assert!(underway.may_ask(&"cover"));
    }

    #[test]
    fn a_reset_forgets_the_failures_too() {
        let mut underway = Underway::fetching();

        let asked = underway.begin("cover");
        underway.failed("cover", asked);
        underway.forget();

        assert!(underway.may_ask(&"cover"));
    }
}
