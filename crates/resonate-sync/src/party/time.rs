use std::{collections::VecDeque, time::Duration};

pub const TIME_ASKED_EVERY: Duration = Duration::from_secs(64);

const SAMPLES_KEPT: usize = 8;

pub const TIGHT_ROUND_TRIP: Duration = Duration::from_millis(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Sample {
    round_trip: u64,
    utc_ahead_by: i128,
}

#[derive(Clone, Debug, Default)]
pub struct UtcClock {
    samples: VecDeque<Sample>,
}

impl UtcClock {
    pub fn answered(&mut self, sent_at: u64, received: u128, replied: u128, heard_at: u64) {
        let Some(here_took) = heard_at.checked_sub(sent_at) else {
            return;
        };
        let Some(there_took) = replied.checked_sub(received) else {
            return;
        };

        let Some(round_trip) = u128::from(here_took).checked_sub(there_took) else {
            return;
        };
        let Ok(round_trip) = u64::try_from(round_trip) else {
            return;
        };
        let (Ok(received), Ok(replied)) = (i128::try_from(received), i128::try_from(replied))
        else {
            return;
        };

        let utc_ahead_by =
            ((received - i128::from(sent_at)) + (replied - i128::from(heard_at))) / 2;

        if self.samples.len() == SAMPLES_KEPT {
            self.samples.pop_front();
        }
        self.samples.push_back(Sample {
            round_trip,
            utc_ahead_by,
        });
    }

    fn best(&self) -> Option<&Sample> {
        self.samples.iter().min_by_key(|sample| sample.round_trip)
    }

    pub fn round_trip(&self) -> Option<Duration> {
        self.best()
            .map(|sample| Duration::from_nanos(sample.round_trip))
    }

    pub fn utc_ahead_by(&self) -> Option<i128> {
        self.best().map(|sample| sample.utc_ahead_by)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartyTimed {
    pub server_ahead_of_utc: i64,
    pub server_round_trip: u64,
    pub utc_round_trip: u64,
}

impl PartyTimed {
    pub fn of(server_ahead_by: i128, server_round_trip: Duration, utc: &UtcClock) -> Option<Self> {
        let utc_ahead_by = utc.utc_ahead_by()?;
        let utc_round_trip = utc.round_trip()?;

        Some(Self {
            server_ahead_of_utc: i64::try_from(server_ahead_by - utc_ahead_by).ok()?,
            server_round_trip: u64::try_from(server_round_trip.as_nanos()).ok()?,
            utc_round_trip: u64::try_from(utc_round_trip.as_nanos()).ok()?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartyApart {
    pub nanos: i64,
    pub doubt: u64,
    pub corrected: bool,
}

impl PartyApart {
    pub fn weighed(mine: PartyTimed, hosts: PartyTimed) -> Self {
        let nanos = mine
            .server_ahead_of_utc
            .saturating_sub(hosts.server_ahead_of_utc);
        let tight = u64::try_from(TIGHT_ROUND_TRIP.as_nanos()).unwrap_or(u64::MAX);
        let utc_doubt = (mine.utc_round_trip.saturating_add(hosts.utc_round_trip)) / 2;
        let path_doubt = (mine
            .server_round_trip
            .saturating_add(hosts.server_round_trip))
            / 2;

        let doubt = path_doubt.saturating_add(utc_doubt);
        let both_tight = mine.utc_round_trip <= tight && hosts.utc_round_trip <= tight;
        let plausible = nanos.unsigned_abs() <= doubt;

        Self {
            nanos,
            doubt,
            corrected: both_tight && utc_doubt < path_doubt && plausible,
        }
    }

    pub fn correction(self) -> i128 {
        if self.corrected {
            i128::from(self.nanos)
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    fn timed(apart_ms: i64, server_round_trip_ms: u64, utc_round_trip_ms: u64) -> PartyTimed {
        PartyTimed {
            server_ahead_of_utc: apart_ms * 1_000_000,
            server_round_trip: server_round_trip_ms * MS,
            utc_round_trip: utc_round_trip_ms * MS,
        }
    }

    #[test]
    fn the_quickest_answer_sets_what_utc_reads_here_and_the_servers_own_wait_is_not_counted() {
        let mut utc = UtcClock::default();

        utc.answered(
            0,
            u128::from(1_000 * MS + 50 * MS),
            u128::from(1_000 * MS + 60 * MS),
            120 * MS,
        );
        utc.answered(
            200 * MS,
            u128::from(1_210 * MS),
            u128::from(1_211 * MS),
            221 * MS,
        );

        assert_eq!(utc.round_trip(), Some(Duration::from_millis(20)));
        assert_eq!(utc.utc_ahead_by(), Some(i128::from(1_000 * MS)));
    }

    #[test]
    fn an_answer_that_came_back_before_it_left_or_took_longer_there_than_here_is_not_believed() {
        let mut utc = UtcClock::default();

        utc.answered(100, 5_000, 5_001, 50);
        utc.answered(0, 5_000, 6_000, 10);

        assert_eq!(utc.utc_ahead_by(), None);
    }

    #[test]
    fn only_the_latest_answers_are_weighed() {
        let mut utc = UtcClock::default();

        utc.answered(0, 0, 0, MS);
        for at in 1..=u64::try_from(SAMPLES_KEPT).unwrap() {
            let sent = at * 1_000 * MS;

            utc.answered(sent, u128::from(sent), u128::from(sent), sent + 10 * MS);
        }

        assert_eq!(utc.round_trip(), Some(Duration::from_millis(10)));
    }

    #[test]
    fn a_guest_far_from_the_server_and_near_its_time_server_is_corrected_by_what_the_host_says() {
        let apart = PartyApart::weighed(timed(-30, 120, 8), timed(0, 20, 6));

        assert_eq!(apart.nanos, -30_000_000);
        assert!(apart.corrected);
        assert_eq!(apart.correction(), -30_000_000);
    }

    #[test]
    fn a_time_server_slower_to_reach_than_the_party_corrects_nothing() {
        let apart = PartyApart::weighed(timed(3, 4, 20), timed(0, 4, 20));

        assert_eq!(apart.nanos, 3_000_000);
        assert!(!apart.corrected);
        assert_eq!(apart.correction(), 0);
    }

    #[test]
    fn a_loose_time_server_on_either_side_corrects_nothing() {
        assert!(!PartyApart::weighed(timed(10, 200, 50), timed(0, 200, 5)).corrected);
        assert!(!PartyApart::weighed(timed(10, 200, 5), timed(0, 200, 50)).corrected);
    }

    #[test]
    fn a_gap_wider_than_both_paths_could_make_is_shown_and_never_corrected() {
        let apart = PartyApart::weighed(timed(500, 60, 4), timed(0, 40, 4));

        assert_eq!(apart.nanos, 500_000_000);
        assert_eq!(apart.doubt, 54_000_000);
        assert!(!apart.corrected);
    }
}
