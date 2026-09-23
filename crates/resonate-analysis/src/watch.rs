use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use resonate_core::Frames;

pub trait Watching: Sync {
    fn stopped(&self) -> bool;

    fn reached(&self, _done: Frames, _of: Option<Frames>) {}
}

const LENGTH_UNKNOWN: u64 = 0;

#[derive(Debug, Default)]
pub struct Watch {
    done: AtomicU64,
    of: AtomicU64,
    stopped: AtomicBool,
}

impl Watch {
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }

    pub fn done(&self) -> Frames {
        Frames(self.done.load(Ordering::Relaxed))
    }

    pub fn share(&self) -> Option<f32> {
        let of = self.of.load(Ordering::Relaxed);
        (of != LENGTH_UNKNOWN).then(|| {
            let share = self.done.load(Ordering::Relaxed) as f64 / of as f64;
            share.clamp(0.0, 1.0) as f32
        })
    }
}

impl Watching for Watch {
    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }

    fn reached(&self, done: Frames, of: Option<Frames>) {
        self.done.store(done.get(), Ordering::Relaxed);
        self.of
            .store(of.map_or(LENGTH_UNKNOWN, Frames::get), Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_watch_reads_back_how_far_the_pass_has_got_and_whether_it_was_stopped() {
        let watch = Watch::default();
        assert_eq!(watch.share(), None);
        assert!(!watch.stopped());

        watch.reached(Frames(250), Some(Frames(1_000)));
        assert_eq!(watch.share(), Some(0.25));
        assert_eq!(watch.done(), Frames(250));

        watch.reached(Frames(250), None);
        assert_eq!(watch.share(), None);

        watch.stop();
        assert!(watch.stopped());
    }
}
