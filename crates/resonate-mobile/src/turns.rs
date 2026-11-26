use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};

use crate::{Error, Result};

pub(crate) struct ReaderShare {
    taken: Mutex<usize>,
    handed_back: Condvar,
    takes_at_most: usize,
    waits_at_most: Duration,
}

pub(crate) struct Turn<'a> {
    share: &'a ReaderShare,
}

impl ReaderShare {
    pub(crate) const fn of(takes_at_most: usize, waits_at_most: Duration) -> Self {
        Self {
            taken: Mutex::new(0),
            handed_back: Condvar::new(),
            takes_at_most,
            waits_at_most,
        }
    }

    pub(crate) fn turn(&self) -> Result<Turn<'_>> {
        let since = Instant::now();
        let deadline = since + self.waits_at_most;
        let mut taken = self.taken.lock();

        while *taken >= self.takes_at_most {
            if self
                .handed_back
                .wait_until(&mut taken, deadline)
                .timed_out()
                && *taken >= self.takes_at_most
            {
                return Err(Error::NoTurnToRead {
                    waited: since.elapsed(),
                });
            }
        }
        *taken += 1;
        Ok(Turn { share: self })
    }
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        *self.share.taken.lock() -= 1;
        self.share.handed_back.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;

    const TURNS: usize = 3;

    const A_SHORT_WAIT: Duration = Duration::from_millis(50);

    const A_LONG_WAIT: Duration = Duration::from_secs(30);

    #[test]
    fn a_share_hands_out_its_turns_and_wakes_a_waiter_as_one_comes_back() {
        let share = ReaderShare::of(TURNS, A_SHORT_WAIT);
        let patient = ReaderShare::of(TURNS, A_LONG_WAIT);

        let mut taken: Vec<_> = (0..TURNS)
            .map(|_| share.turn().expect("a turn within the share"))
            .collect();
        let refused = share.turn().err();

        drop(taken.pop());

        let handed_back = share.turn();
        let held: Vec<_> = (0..TURNS)
            .map(|_| patient.turn().expect("a turn within the share"))
            .collect();

        let woken = thread::scope(|scope| {
            let waiter = scope.spawn(|| {
                let since = Instant::now();
                patient.turn().map(|_| since.elapsed())
            });

            thread::sleep(A_SHORT_WAIT);
            drop(held);
            waiter.join().expect("the waiter")
        });

        assert!(
            matches!(refused, Some(Error::NoTurnToRead { waited }) if waited >= A_SHORT_WAIT),
            "{refused:?}"
        );
        assert!(handed_back.is_ok());
        assert!(
            woken.as_ref().is_ok_and(|waited| *waited < A_LONG_WAIT),
            "{woken:?}"
        );
    }
}
