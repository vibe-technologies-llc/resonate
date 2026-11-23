use std::sync::Arc;

use resonate_sync::{Heard, Holds, Incoming as Reader, wire::ToServer};
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

#[derive(Clone)]
pub struct FrameBytes {
    bytes: Arc<Semaphore>,
    total: usize,
}

pub struct Held {
    _read_ahead: Option<OwnedSemaphorePermit>,
    _across_sessions: Option<OwnedSemaphorePermit>,
}

pub struct First {
    pub payload: Vec<u8>,
    _across_sessions: Option<OwnedSemaphorePermit>,
}

pub struct ReadingAhead(JoinHandle<()>);

impl Drop for ReadingAhead {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub enum Read {
    Frame {
        frame: Result<ToServer>,
        held: Option<Held>,
    },
    PassedOver,
}

impl FrameBytes {
    pub fn of(total: usize) -> Self {
        let total = total.clamp(1, Semaphore::MAX_PERMITS);
        Self {
            bytes: Arc::new(Semaphore::new(total)),
            total,
        }
    }
}

impl Holds for FrameBytes {
    fn hold(&self, length: usize) -> Option<OwnedSemaphorePermit> {
        let weight = length.clamp(A_FRAME_WEIGHS_AT_LEAST.min(self.total), self.total);
        let weight = u32::try_from(weight).unwrap_or(u32::MAX);

        Arc::clone(&self.bytes).try_acquire_many_owned(weight).ok()
    }

    fn draw(&self, bytes: usize) -> Option<OwnedSemaphorePermit> {
        let weight = u32::try_from(bytes).ok()?;

        Arc::clone(&self.bytes).try_acquire_many_owned(weight).ok()
    }
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

pub async fn read_the_first(
    recv: &mut Reader,
    most: usize,
    across_sessions: FrameBytes,
) -> Result<Option<First>> {
    let across_sessions: Arc<dyn Holds> = Arc::new(across_sessions);
    loop {
        match recv.read_heard(most, &across_sessions).await? {
            None => return Ok(None),
            Some(Heard::Frame { payload, held }) => {
                return Ok(Some(First {
                    payload,
                    _across_sessions: held,
                }));
            }
            Some(Heard::PassedOver) => {}
        }
    }
}

pub fn read_ahead(
    recv: Reader,
    most: usize,
    across_sessions: FrameBytes,
) -> (Receiver<Read>, ReadingAhead) {
    let (frames_in, frames) = channel(READ_AHEAD_BYTES_AT_MOST / A_FRAME_WEIGHS_AT_LEAST);
    let budget = Budget::of(READ_AHEAD_BYTES_AT_MOST);
    let across_sessions: Arc<dyn Holds> = Arc::new(across_sessions);

    let reading = tokio::spawn(read_frames(recv, frames_in, most, budget, across_sessions));

    (frames, ReadingAhead(reading))
}

async fn read_frames(
    mut recv: Reader,
    frames: Sender<Read>,
    most: usize,
    budget: Budget,
    across_sessions: Arc<dyn Holds>,
) {
    loop {
        budget.has_room().await;

        let read = match recv.read_heard(most, &across_sessions).await {
            Ok(Some(Heard::Frame { payload, held })) => Read::Frame {
                held: Some(Held {
                    _read_ahead: budget.weigh(payload.len()).await,
                    _across_sessions: held,
                }),
                frame: ToServer::decoded(&payload).map_err(Error::from),
            },
            Ok(Some(Heard::PassedOver)) => Read::PassedOver,
            Ok(None) => return,
            Err(error) => Read::Frame {
                frame: Err(Error::from(error)),
                held: None,
            },
        };
        let failed = matches!(read, Read::Frame { frame: Err(_), .. });
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

    #[test]
    fn frames_held_across_sessions_are_refused_once_their_bytes_are_spent_and_room_comes_back() {
        let shared = FrameBytes::of(1000);

        let first = shared.hold(600).expect("held");
        let refused = shared.hold(600);
        let small = shared.hold(1).expect("held");

        drop(first);

        let after = shared.hold(900);
        let huge = shared.hold(32 * 1024 * 1024);

        assert!(refused.is_none());
        assert!(after.is_some());
        assert!(huge.is_none());

        drop((small, after));

        assert!(shared.hold(32 * 1024 * 1024).is_some());
    }

    #[tokio::test]
    async fn a_reader_let_go_stops_and_gives_back_the_bytes_its_frame_held() {
        let shared = FrameBytes::of(1000);

        let held = shared.hold(600).expect("held");
        let reading = ReadingAhead(tokio::spawn(async move {
            let _held = held;

            std::future::pending::<()>().await;
        }));
        let while_reading = shared.bytes.available_permits();

        drop(reading);

        let given_back = timeout(PATIENCE * 10, async {
            while shared.bytes.available_permits() < 1000 {
                tokio::task::yield_now().await;
            }
        })
        .await;

        assert_eq!(while_reading, 400);
        assert!(given_back.is_ok(), "the reader kept its frame's bytes");
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
