#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use std::mem;

use resonate_audio::AudioSource;

const SOURCES_HANDED_AT_ONCE: usize = 1;
const SOURCES_SPENT_AT_ONCE: usize = 2;

pub(crate) struct Fed {
    source: Box<dyn AudioSource>,
    handed: rtrb::Consumer<Box<dyn AudioSource>>,
    spent: rtrb::Producer<Box<dyn AudioSource>>,
}

impl Fed {
    pub(crate) fn source(&mut self) -> &mut dyn AudioSource {
        self.take_what_was_handed();
        self.source.as_mut()
    }

    fn take_what_was_handed(&mut self) {
        if self.spent.is_full() {
            return;
        }
        let Ok(handed) = self.handed.pop() else {
            return;
        };
        let spent = mem::replace(&mut self.source, handed);
        let kept_off_this_thread = self.spent.push(spent);
        debug_assert!(kept_off_this_thread.is_ok());
    }
}

pub(crate) struct Handing {
    handing: rtrb::Producer<Box<dyn AudioSource>>,
    spent: rtrb::Consumer<Box<dyn AudioSource>>,
}

impl Handing {
    pub(crate) fn has_room(&self) -> bool {
        !self.handing.is_full()
    }

    pub(crate) fn hand(
        &mut self,
        source: Box<dyn AudioSource>,
    ) -> Result<(), Box<dyn AudioSource>> {
        self.handing
            .push(source)
            .map_err(|rtrb::PushError::Full(source)| source)
    }

    pub(crate) fn let_go_of_the_spent(&mut self) {
        while let Ok(spent) = self.spent.pop() {
            drop(spent);
        }
    }
}

pub(crate) fn fed_from(source: Box<dyn AudioSource>) -> (Fed, Handing) {
    let (handing, handed) = rtrb::RingBuffer::new(SOURCES_HANDED_AT_ONCE);
    let (spending, spent) = rtrb::RingBuffer::new(SOURCES_SPENT_AT_ONCE);
    (
        Fed {
            source,
            handed,
            spent: spending,
        },
        Handing { handing, spent },
    )
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    struct Marked {
        mark: u8,
        dropped: Arc<AtomicUsize>,
    }

    impl AudioSource for Marked {
        fn fill(&mut self, dst: &mut [u8]) -> usize {
            dst.fill(self.mark);
            dst.len()
        }
    }

    impl Drop for Marked {
        fn drop(&mut self) {
            self.dropped.fetch_add(1, Ordering::AcqRel);
        }
    }

    fn marked(mark: u8, dropped: &Arc<AtomicUsize>) -> Box<dyn AudioSource> {
        Box::new(Marked {
            mark,
            dropped: Arc::clone(dropped),
        })
    }

    fn mark_read(fed: &mut Fed) -> u8 {
        let mut bytes = [0_u8; 4];
        fed.source().fill(&mut bytes);
        bytes.first().copied().unwrap_or_default()
    }

    #[test]
    fn a_source_handed_over_is_read_from_the_next_fill_on() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let (mut fed, mut handing) = fed_from(marked(1, &dropped));

        assert_eq!(mark_read(&mut fed), 1);
        assert!(handing.hand(marked(2, &dropped)).is_ok());

        assert_eq!(mark_read(&mut fed), 2);
        assert_eq!(mark_read(&mut fed), 2);
    }

    #[test]
    fn the_source_replaced_is_dropped_by_the_side_that_handed_not_the_one_that_fills() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let (mut fed, mut handing) = fed_from(marked(1, &dropped));

        assert!(handing.hand(marked(2, &dropped)).is_ok());
        mark_read(&mut fed);
        assert_eq!(dropped.load(Ordering::Acquire), 0);

        handing.let_go_of_the_spent();
        assert_eq!(dropped.load(Ordering::Acquire), 1);
    }

    #[test]
    fn a_second_source_is_refused_until_the_first_is_taken() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let (mut fed, mut handing) = fed_from(marked(1, &dropped));

        assert!(handing.has_room());
        assert!(handing.hand(marked(2, &dropped)).is_ok());
        assert!(!handing.has_room());
        assert!(handing.hand(marked(3, &dropped)).is_err());

        mark_read(&mut fed);
        assert!(handing.has_room());
    }

    #[test]
    fn nothing_is_taken_while_the_way_back_is_full() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let (mut fed, mut handing) = fed_from(marked(1, &dropped));

        for mark in 2..=3 {
            assert!(handing.hand(marked(mark, &dropped)).is_ok());
            assert_eq!(mark_read(&mut fed), mark);
        }
        assert!(handing.hand(marked(4, &dropped)).is_ok());

        assert_eq!(mark_read(&mut fed), 3);

        handing.let_go_of_the_spent();
        assert_eq!(mark_read(&mut fed), 4);
    }
}
