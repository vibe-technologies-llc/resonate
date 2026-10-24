use std::time::Instant;

pub struct Bucket {
    rate: u64,
    tokens: f64,
    at: Instant,
}

impl Bucket {
    pub fn new(rate: u64) -> Self {
        Self::starting(rate, Instant::now())
    }

    fn starting(rate: u64, at: Instant) -> Self {
        Self {
            rate,
            tokens: rate as f64,
            at,
        }
    }

    pub fn spend(&mut self, bytes: usize) -> bool {
        self.spend_at(bytes, Instant::now())
    }

    fn spend_at(&mut self, bytes: usize, now: Instant) -> bool {
        let earned = now.saturating_duration_since(self.at).as_secs_f64() * self.rate as f64;
        self.tokens = (self.tokens + earned).min(self.rate as f64);
        self.at = now;
        let needed = bytes as f64;
        if self.tokens < needed {
            return false;
        }
        self.tokens -= needed;
        true
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_second_of_bytes_is_spent_at_once_and_more_waits_for_the_rate() {
        let start = Instant::now();
        let mut bucket = Bucket::starting(1000, start);

        let first = bucket.spend_at(600, start);
        let second = bucket.spend_at(600, start);
        let later = bucket.spend_at(600, start + Duration::from_millis(500));

        assert!(first);
        assert!(!second);
        assert!(later);
    }

    #[test]
    fn what_is_earned_never_builds_past_a_second_of_bytes() {
        let start = Instant::now();
        let mut bucket = Bucket::starting(1000, start);

        let after_an_hour = start + Duration::from_secs(3600);

        assert!(bucket.spend_at(1000, after_an_hour));
        assert!(!bucket.spend_at(1, after_an_hour));
    }

    #[test]
    fn no_rate_spends_nothing() {
        let mut bucket = Bucket::new(0);

        assert!(!bucket.spend(1));
    }
}
