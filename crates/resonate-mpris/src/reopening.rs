use std::time::{Duration, Instant};

pub(crate) const REOPENED_AT_MOST_EVERY: Duration = Duration::from_secs(10);

#[derive(Default)]
pub(crate) struct Reopening {
    last: Option<Instant>,
}

impl Reopening {
    pub(crate) fn due(&mut self, now: Instant) -> bool {
        if self
            .last
            .is_some_and(|last| now.saturating_duration_since(last) < REOPENED_AT_MOST_EVERY)
        {
            return false;
        }
        self.last = Some(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_connection_is_opened_again_at_once_and_then_no_more_often_than_its_pace() {
        let start = Instant::now();
        let mut reopening = Reopening::default();

        assert!(reopening.due(start));
        assert!(!reopening.due(start + REOPENED_AT_MOST_EVERY / 2));
        assert!(reopening.due(start + REOPENED_AT_MOST_EVERY));
        assert!(!reopening.due(start + REOPENED_AT_MOST_EVERY));
    }
}
