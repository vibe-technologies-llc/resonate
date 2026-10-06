use std::{
    thread,
    time::{Duration, Instant},
};

use parking_lot::Mutex;

#[derive(Debug)]
pub struct Pacing {
    apart: Duration,
    turns: Mutex<Turns>,
}

#[derive(Debug)]
struct Turns {
    next: Instant,
    cooling: Option<Instant>,
}

impl Pacing {
    #[must_use]
    pub fn new(apart: Duration) -> Self {
        Self {
            apart,
            turns: Mutex::new(Turns {
                next: Instant::now(),
                cooling: None,
            }),
        }
    }

    pub fn paced(&self) {
        let slot = {
            let mut turns = self.turns.lock();
            let now = Instant::now();
            let slot = turns.next.max(now).max(turns.cooling.unwrap_or(now));
            turns.next = slot + self.apart;
            slot
        };
        let wait = slot.saturating_duration_since(Instant::now());
        if !wait.is_zero() {
            thread::sleep(wait);
        }
    }

    pub fn cool_for(&self, wait: Duration) {
        let until = Instant::now() + wait;
        let mut turns = self.turns.lock();
        turns.cooling = Some(turns.cooling.map_or(until, |cooling| cooling.max(until)));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    const APART: Duration = Duration::from_millis(60);

    #[test]
    fn turns_are_reserved_apart_and_nobody_waits_on_the_lock_while_another_sleeps() {
        let pacing = Arc::new(Pacing::new(APART));
        pacing.paced();
        let began = Instant::now();

        let sleeping = {
            let pacing = Arc::clone(&pacing);
            thread::spawn(move || pacing.paced())
        };
        thread::sleep(APART / 6);
        let touched = Instant::now();
        pacing.cool_for(Duration::ZERO);
        let touching_took = touched.elapsed();
        pacing.paced();
        sleeping.join().expect("the other caller");

        assert!(
            touching_took < APART / 3,
            "the turns were locked while a caller slept: {touching_took:?}"
        );
        assert!(began.elapsed() >= APART * 2, "{:?}", began.elapsed());
    }

    #[test]
    fn a_server_asking_to_be_asked_later_holds_back_every_caller() {
        const ASKED_FOR: Duration = Duration::from_millis(150);

        let pacing = Pacing::new(APART);
        pacing.cool_for(ASKED_FOR);
        let began = Instant::now();
        pacing.paced();

        assert!(began.elapsed() >= ASKED_FOR, "{:?}", began.elapsed());
    }
}
