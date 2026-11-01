use std::mem;

use audio_thread_priority::{
    RtPriorityHandle, demote_current_thread_from_real_time, promote_current_thread_to_real_time,
};
use resonate_core::SampleRate;
use thread_priority::{ThreadPriority, WinAPIThreadPriority, set_current_thread_priority};

const FRAMES_A_CYCLE_UNSAID: u32 = 0;

enum Held {
    Ordinary,
    Scheduled(RtPriorityHandle),
    Raised,
}

pub(crate) struct Priority {
    held: Held,
}

impl Priority {
    pub(crate) const fn ordinary() -> Self {
        Self {
            held: Held::Ordinary,
        }
    }

    pub(crate) fn set(&mut self, realtime: bool, rate: SampleRate) {
        match (realtime, &self.held) {
            (true, Held::Ordinary) => self.held = raised(rate),
            (false, Held::Scheduled(_) | Held::Raised) => self.lower(),
            (true, Held::Scheduled(_) | Held::Raised) | (false, Held::Ordinary) => {}
        }
    }

    fn lower(&mut self) {
        match mem::replace(&mut self.held, Held::Ordinary) {
            Held::Scheduled(handle) => {
                if let Err(error) = demote_current_thread_from_real_time(handle) {
                    tracing::warn!(%error, "the stream thread would not leave its MMCSS class");
                }
            }
            Held::Raised => {
                let normal = ThreadPriority::Os(WinAPIThreadPriority::Normal.into());
                if let Err(error) = set_current_thread_priority(normal) {
                    tracing::warn!(
                        ?error,
                        "the stream thread would not return to normal priority"
                    );
                }
            }
            Held::Ordinary => {}
        }
    }
}

fn raised(rate: SampleRate) -> Held {
    match promote_current_thread_to_real_time(FRAMES_A_CYCLE_UNSAID, rate.hz()) {
        Ok(handle) => Held::Scheduled(handle),
        Err(error) => {
            tracing::warn!(%error, "MMCSS refused the stream thread; raising its priority instead");
            match set_current_thread_priority(ThreadPriority::Max) {
                Ok(()) => Held::Raised,
                Err(error) => {
                    tracing::warn!(?error, "the stream thread's priority could not be raised");
                    Held::Ordinary
                }
            }
        }
    }
}

impl Drop for Priority {
    fn drop(&mut self) {
        self.lower();
    }
}
