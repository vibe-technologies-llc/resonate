use std::sync::Arc;

use resonate_sync::{Incoming as Reader, wire::ToServer};
use tokio::{
    sync::{
        OwnedSemaphorePermit, Semaphore,
        mpsc::{Receiver, Sender, channel},
    },
    task::JoinHandle,
};

use crate::{Error, Result};

pub const READ_AHEAD_BYTES_AT_MOST: usize = 1024 * 1024;
const A_FRAME_WEIGHS_AT_LEAST: usize = 64;

#[derive(Clone)]
pub struct Budget {
    bytes: Arc<Semaphore>,
    total: usize,
}

pub struct Read {
    pub frame: Result<ToServer>,
    pub held: Option<OwnedSemaphorePermit>,
}

impl Budget {
    pub fn of(total: usize) -> Self {
        Self {
            bytes: Arc::new(Semaphore::new(total)),
            total,
        }
    }

    pub async fn has_room(&self) {
        drop(self.bytes.acquire().await);
    }

    pub async fn weigh(&self, length: usize) -> Option<OwnedSemaphorePermit> {
        let weight = length.clamp(A_FRAME_WEIGHS_AT_LEAST, self.total);
        let weight = u32::try_from(weight).unwrap_or(u32::MAX);

        Arc::clone(&self.bytes)
            .acquire_many_owned(weight)
            .await
            .ok()
    }
}

pub fn read_ahead(recv: Reader, most: usize) -> (Receiver<Read>, JoinHandle<()>) {
    let (frames_in, frames) = channel(READ_AHEAD_BYTES_AT_MOST / A_FRAME_WEIGHS_AT_LEAST);
    let reading = tokio::spawn(read_frames(
        recv,
        frames_in,
        most,
        Budget::of(READ_AHEAD_BYTES_AT_MOST),
    ));

    (frames, reading)
}

async fn read_frames(mut recv: Reader, frames: Sender<Read>, most: usize, budget: Budget) {
    loop {
        budget.has_room().await;
        let read = match recv.read_at_most(most).await {
            Ok(Some(bytes)) => Read {
                held: budget.weigh(bytes.len()).await,
                frame: ToServer::decoded(&bytes).map_err(Error::from),
            },
            Ok(None) => return,
            Err(error) => Read {
                frame: Err(Error::from(error)),
                held: None,
            },
        };
        let failed = read.frame.is_err();
        if frames.send(read).await.is_err() || failed {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::timeout;

    use super::*;

    const PATIENCE: Duration = Duration::from_millis(50);

    #[tokio::test]
    async fn frames_are_read_ahead_only_while_the_bytes_held_leave_room() {
        let budget = Budget::of(1000);

        let first = budget.weigh(400).await.expect("weighed");
        let second = budget.weigh(400).await.expect("weighed");
        let room = timeout(PATIENCE, budget.has_room()).await;
        drop(first);
        let freed = timeout(PATIENCE, budget.has_room()).await;

        assert!(room.is_ok(), "200 bytes are still free");
        assert!(freed.is_ok());
        drop(second);
    }

    #[tokio::test]
    async fn a_frame_larger_than_the_budget_takes_all_of_it_and_nothing_is_read_until_it_is_handled()
     {
        let budget = Budget::of(1000);

        let huge = budget.weigh(32 * 1024 * 1024).await.expect("weighed");
        let blocked = timeout(PATIENCE, budget.has_room()).await;
        drop(huge);
        let freed = timeout(PATIENCE, budget.has_room()).await;

        assert!(blocked.is_err(), "no room while the frame is held");
        assert!(freed.is_ok());
    }

    #[tokio::test]
    async fn a_run_of_tiny_frames_fills_the_budget_by_their_least_weight() {
        let budget = Budget::of(A_FRAME_WEIGHS_AT_LEAST * 4);

        let held: Vec<_> = {
            let mut held = Vec::new();
            for _ in 0..4 {
                held.push(budget.weigh(1).await.expect("weighed"));
            }
            held
        };
        let blocked = timeout(PATIENCE, budget.has_room()).await;

        assert!(blocked.is_err());
        drop(held);
    }
}
