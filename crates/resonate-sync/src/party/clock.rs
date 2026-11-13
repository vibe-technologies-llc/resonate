use std::{collections::VecDeque, time::Duration};

pub const CLOCK_WINDOW: Duration = Duration::from_secs(60);

const A_GAP_FORGETS_THE_WINDOW_PAST: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug)]
struct Sample {
    heard_at: u64,
    round_trip: u64,
    server_ahead_by: i128,
}

#[derive(Clone, Debug, Default)]
pub struct ServerClock {
    samples: VecDeque<Sample>,
    corrected_by: i128,
}

impl ServerClock {
    pub fn answered(&mut self, sent_at: u64, server_at: u64, heard_at: u64) {
        let Some(round_trip) = heard_at.checked_sub(sent_at) else {
            return;
        };
        let gap = u64::try_from(A_GAP_FORGETS_THE_WINDOW_PAST.as_nanos()).unwrap_or(u64::MAX);
        if self
            .samples
            .back()
            .is_some_and(|last| sent_at.saturating_sub(last.heard_at) > gap)
        {
            self.samples.clear();
        }

        let midway = i128::from(sent_at) + i128::from(round_trip / 2);
        self.samples.push_back(Sample {
            heard_at,
            round_trip,
            server_ahead_by: i128::from(server_at) - midway,
        });

        let window = u64::try_from(CLOCK_WINDOW.as_nanos()).unwrap_or(u64::MAX);
        while self
            .samples
            .front()
            .is_some_and(|oldest| heard_at.saturating_sub(oldest.heard_at) > window)
        {
            self.samples.pop_front();
        }
    }

    fn best(&self) -> Option<&Sample> {
        self.samples.iter().min_by_key(|sample| sample.round_trip)
    }

    pub fn round_trip(&self) -> Option<Duration> {
        self.best()
            .map(|sample| Duration::from_nanos(sample.round_trip))
    }

    pub fn server_ahead_by(&self) -> Option<i128> {
        self.best().map(|sample| sample.server_ahead_by)
    }

    pub fn correct_by(&mut self, nanos: i128) {
        self.corrected_by = nanos;
    }

    fn ahead_as_corrected(&self) -> Option<i128> {
        Some(self.server_ahead_by()? - self.corrected_by)
    }

    pub fn to_server(&self, here: u64) -> Option<u64> {
        u64::try_from(i128::from(here) + self.ahead_as_corrected()?).ok()
    }

    pub fn from_server(&self, there: u64) -> Option<u64> {
        u64::try_from(i128::from(there) - self.ahead_as_corrected()?).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    #[test]
    fn the_quickest_answer_of_the_window_sets_the_clock_whatever_the_others_said() {
        let mut clock = ServerClock::default();

        clock.answered(0, 1_000 * MS + 50 * MS, 100 * MS);
        clock.answered(1_000 * MS, 2_000 * MS + 5 * MS, 1_010 * MS);
        clock.answered(2_000 * MS, 3_000 * MS + 90 * MS, 2_200 * MS);

        assert_eq!(clock.round_trip(), Some(Duration::from_millis(10)));
        assert_eq!(clock.to_server(5 * MS), Some(1_005 * MS));
        assert_eq!(clock.from_server(1_005 * MS), Some(5 * MS));
    }

    #[test]
    fn an_answer_that_left_after_it_came_back_is_not_believed() {
        let mut clock = ServerClock::default();

        clock.answered(100, 5_000, 50);

        assert_eq!(clock.to_server(0), None);
    }

    #[test]
    fn a_suspended_machine_forgets_what_it_measured_before_it_slept() {
        let mut clock = ServerClock::default();

        clock.answered(0, 1_000 * MS + MS, 2 * MS);
        clock.answered(60_000 * MS, 90_000 * MS + 50 * MS, 60_100 * MS);

        assert_eq!(clock.round_trip(), Some(Duration::from_millis(100)));
        assert_eq!(clock.to_server(60_050 * MS), Some(90_050 * MS));
    }

    #[test]
    fn answers_older_than_the_window_stop_counting() {
        let mut clock = ServerClock::default();

        clock.answered(0, 2 * MS, 2 * MS);
        let mut at = 5_000 * MS;
        while at < 70_000 * MS {
            clock.answered(at, at + 50 * MS, at + 100 * MS);
            at += 5_000 * MS;
        }

        assert_eq!(clock.round_trip(), Some(Duration::from_millis(100)));
    }

    #[test]
    fn a_correction_moves_both_ways_of_reading_the_clock_and_never_what_was_measured() {
        let mut clock = ServerClock::default();
        clock.answered(0, 1_000 * MS + 5 * MS, 10 * MS);

        clock.correct_by(i128::from(30 * MS));

        assert_eq!(clock.to_server(5 * MS), Some(975 * MS));
        assert_eq!(clock.from_server(975 * MS), Some(5 * MS));
        assert_eq!(clock.server_ahead_by(), Some(i128::from(1_000 * MS)));
    }

    #[test]
    fn no_answer_yet_knows_no_time() {
        let clock = ServerClock::default();

        assert_eq!(clock.to_server(1), None);
        assert_eq!(clock.from_server(1), None);
        assert_eq!(clock.round_trip(), None);
    }
}
