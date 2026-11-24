use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use tokio::sync::Notify;

use super::{ExportDeclined, ExportHeld};

type ExportJudge = dyn Fn(&ExportHeld, u64) -> Result<(), ExportDeclined> + Send + Sync;

#[derive(Default)]
pub struct ExportReceiving {
    received: AtomicU64,
    of: AtomicU64,
    called_off: AtomicBool,
    calling_off: Notify,
    judge: Option<Box<ExportJudge>>,
}

impl ExportReceiving {
    pub fn judged_by(
        judge: impl Fn(&ExportHeld, u64) -> Result<(), ExportDeclined> + Send + Sync + 'static,
    ) -> Self {
        Self {
            judge: Some(Box::new(judge)),
            ..Self::default()
        }
    }

    pub fn so_far(&self) -> (u64, u64) {
        (
            self.received.load(Ordering::Relaxed),
            self.of.load(Ordering::Relaxed),
        )
    }

    pub fn call_off(&self) {
        self.called_off.store(true, Ordering::Release);
        self.calling_off.notify_waiters();
    }

    pub(crate) fn called_off(&self) -> bool {
        self.called_off.load(Ordering::Acquire)
    }

    pub(crate) async fn until_called_off(&self) {
        loop {
            let calling_off = self.calling_off.notified();

            if self.called_off() {
                return;
            }
            calling_off.await;
        }
    }

    pub(crate) fn judged(
        &self,
        offered: &ExportHeld,
        remaining: u64,
    ) -> Result<(), ExportDeclined> {
        self.judge
            .as_ref()
            .map_or(Ok(()), |judge| judge(offered, remaining))
    }

    pub(crate) fn heard(&self, received: u64, of: u64) {
        self.received.store(received, Ordering::Relaxed);
        self.of.store(of, Ordering::Relaxed);
    }
}
