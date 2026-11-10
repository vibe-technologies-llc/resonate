use std::time::{Duration, Instant};

const SHORTEST_DRAIN: Duration = Duration::from_millis(20);

pub struct Drain {
    requested: bool,
    ready_at: Option<Instant>,
}

impl Default for Drain {
    fn default() -> Self {
        Self::new()
    }
}

impl Drain {
    pub const fn new() -> Self {
        Self {
            requested: false,
            ready_at: None,
        }
    }

    pub fn request(&mut self) {
        self.requested = true;
    }

    pub const fn is_requested(&self) -> bool {
        self.requested
    }

    pub fn settled(&mut self, empty: bool, latency: Duration, now: Instant) -> bool {
        if !(self.requested && empty) {
            self.ready_at = None;
            return false;
        }
        let ready_at = *self
            .ready_at
            .get_or_insert_with(|| now + latency.max(SHORTEST_DRAIN));
        if now < ready_at {
            return false;
        }
        self.requested = false;
        self.ready_at = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drain_waits_the_latency_after_the_ring_empties_and_then_settles_once() {
        let mut drain = Drain::new();
        let start = Instant::now();
        let latency = Duration::from_millis(100);

        drain.request();

        assert!(!drain.settled(true, latency, start));
        assert!(!drain.settled(true, latency, start + Duration::from_millis(99)));
        assert!(drain.settled(true, latency, start + latency));
        assert!(!drain.is_requested());
        assert!(!drain.settled(true, latency, start + latency * 2));
    }

    #[test]
    fn a_drain_never_waits_less_than_the_shortest() {
        let mut drain = Drain::new();
        let start = Instant::now();

        drain.request();

        assert!(!drain.settled(true, Duration::ZERO, start));
        assert!(!drain.settled(true, Duration::ZERO, start + Duration::from_millis(19)));
        assert!(drain.settled(true, Duration::ZERO, start + SHORTEST_DRAIN));
    }

    #[test]
    fn a_source_that_fills_again_restarts_the_wait() {
        let mut drain = Drain::new();
        let start = Instant::now();
        let latency = Duration::from_millis(100);

        drain.request();
        assert!(!drain.settled(true, latency, start));
        assert!(!drain.settled(false, latency, start + Duration::from_millis(90)));

        assert!(!drain.settled(true, latency, start + Duration::from_millis(120)));
        assert!(!drain.settled(true, latency, start + Duration::from_millis(200)));
        assert!(drain.settled(true, latency, start + Duration::from_millis(220)));
    }

    #[test]
    fn nothing_settles_that_was_never_asked_to_drain() {
        let mut drain = Drain::new();
        let start = Instant::now();

        assert!(!drain.settled(true, Duration::ZERO, start));
        assert!(!drain.settled(true, Duration::ZERO, start + Duration::from_secs(60)));
        assert!(!drain.is_requested());
    }
}
