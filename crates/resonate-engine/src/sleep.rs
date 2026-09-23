use std::time::{Duration, Instant};

pub(crate) const LONGEST_SLEEP: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Until {
    After(Duration),
    EndOfTrack,
    EndOfQueue,
}

impl Until {
    pub(crate) fn bounded(self) -> Self {
        match self {
            Self::After(delay) => Self::After(delay.min(LONGEST_SLEEP)),
            Self::EndOfTrack | Self::EndOfQueue => self,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Asleep {
    pub until: Until,
    pub left: Option<Duration>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Sleeping {
    until: Until,
    due: Option<Instant>,
}

impl Sleeping {
    pub(crate) fn set(until: Until) -> Self {
        let until = until.bounded();
        let due = match until {
            Until::After(delay) => Some(Instant::now() + delay),
            Until::EndOfTrack | Until::EndOfQueue => None,
        };

        Self { until, due }
    }

    pub(crate) const fn ends_the_track(self) -> bool {
        matches!(self.until, Until::EndOfTrack)
    }

    pub(crate) const fn ends_the_queue(self) -> bool {
        matches!(self.until, Until::EndOfQueue)
    }

    pub(crate) fn left(self) -> Option<Duration> {
        self.due
            .map(|due| due.saturating_duration_since(Instant::now()))
    }

    pub(crate) fn is_out(self) -> bool {
        self.due.is_some_and(|due| Instant::now() >= due)
    }

    pub(crate) fn published(self) -> Asleep {
        Asleep {
            until: self.until,
            left: self.left(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_delay_longer_than_an_instant_holds_is_held_to_the_longest_sleep() {
        let sleeping = Sleeping::set(Until::After(Duration::MAX));

        assert_eq!(sleeping.published().until, Until::After(LONGEST_SLEEP));
        assert!(sleeping.left().is_some_and(|left| left <= LONGEST_SLEEP));
        assert!(!sleeping.is_out());
    }

    #[test]
    fn a_delay_inside_the_bound_is_kept_as_it_was_asked_for() {
        let asked = Until::After(Duration::from_secs(45 * 60));

        assert_eq!(asked.bounded(), asked);
        assert_eq!(Until::EndOfQueue.bounded(), Until::EndOfQueue);
    }
}
