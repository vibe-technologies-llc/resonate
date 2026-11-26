use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};

use super::READER_POOL;
use crate::{Error, Result};

const _: () = assert!(ReaderShare::TAKES_AT_MOST < READER_POOL);

pub struct ReaderShare {
    taken: Mutex<usize>,
    handed_back: Condvar,
    waits_at_most: Duration,
}

pub struct Turn<'a> {
    share: &'a ReaderShare,
}

impl ReaderShare {
    pub const TAKES_AT_MOST: usize = READER_POOL / 2;

    pub const fn waiting_at_most(waits_at_most: Duration) -> Self {
        Self {
            taken: Mutex::new(0),
            handed_back: Condvar::new(),
            waits_at_most,
        }
    }

    pub fn turn(&self) -> Result<Turn<'_>> {
        let since = Instant::now();
        let deadline = since + self.waits_at_most;
        let mut taken = self.taken.lock();

        while *taken >= Self::TAKES_AT_MOST {
            if self
                .handed_back
                .wait_until(&mut taken, deadline)
                .timed_out()
                && *taken >= Self::TAKES_AT_MOST
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
