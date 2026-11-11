use std::time::{Duration, SystemTime};

use crate::words;

const SECONDS_A_MINUTE: u64 = 60;
const SECONDS_AN_HOUR: u64 = 60 * SECONDS_A_MINUTE;
const SECONDS_A_DAY: u64 = 24 * SECONDS_AN_HOUR;
const SECONDS_A_WEEK: u64 = 7 * SECONDS_A_DAY;
const SECONDS_A_MONTH: u64 = 30 * SECONDS_A_DAY;
const SECONDS_A_YEAR: u64 = 365 * SECONDS_A_DAY;

struct Reach {
    seconds: u64,
    ago: fn(u64) -> String,
    briefly: fn(u64) -> String,
}

const REACHES: [Reach; 6] = [
    Reach {
        seconds: SECONDS_A_YEAR,
        ago: words::elapsed::years_ago,
        briefly: words::elapsed::years_briefly,
    },
    Reach {
        seconds: SECONDS_A_MONTH,
        ago: words::elapsed::months_ago,
        briefly: words::elapsed::months_briefly,
    },
    Reach {
        seconds: SECONDS_A_WEEK,
        ago: words::elapsed::weeks_ago,
        briefly: words::elapsed::weeks_briefly,
    },
    Reach {
        seconds: SECONDS_A_DAY,
        ago: words::elapsed::days_ago,
        briefly: words::elapsed::days_briefly,
    },
    Reach {
        seconds: SECONDS_AN_HOUR,
        ago: words::elapsed::hours_ago,
        briefly: words::elapsed::hours_briefly,
    },
    Reach {
        seconds: SECONDS_A_MINUTE,
        ago: words::elapsed::minutes_ago,
        briefly: words::elapsed::minutes_briefly,
    },
];

pub fn clock(span: Duration) -> String {
    let seconds = span.as_secs();
    let minutes = seconds / SECONDS_A_MINUTE;
    let hours = seconds / SECONDS_AN_HOUR;

    match hours {
        0 => format!("{minutes}:{:02}", seconds % SECONDS_A_MINUTE),
        hours => format!(
            "{hours}:{:02}:{:02}",
            minutes % (SECONDS_AN_HOUR / SECONDS_A_MINUTE),
            seconds % SECONDS_A_MINUTE
        ),
    }
}

fn seconds_since(when: SystemTime, now: SystemTime) -> u64 {
    now.duration_since(when).unwrap_or_default().as_secs()
}

pub fn since(when: SystemTime, now: SystemTime) -> String {
    let ago = seconds_since(when, now);

    REACHES
        .iter()
        .find(|reach| ago >= reach.seconds)
        .map_or_else(
            || words::elapsed::just_now().to_owned(),
            |reach| (reach.ago)(ago / reach.seconds),
        )
}

pub fn age(when: SystemTime, now: SystemTime) -> String {
    let ago = seconds_since(when, now);

    REACHES
        .iter()
        .find(|reach| ago >= reach.seconds)
        .map_or_else(
            || words::elapsed::now().to_owned(),
            |reach| (reach.briefly)(ago / reach.seconds),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ago(seconds: u64) -> String {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(SECONDS_A_YEAR * 40);
        since(now - Duration::from_secs(seconds), now)
    }

    fn briefly(seconds: u64) -> String {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(SECONDS_A_YEAR * 40);
        age(now - Duration::from_secs(seconds), now)
    }

    #[test]
    fn a_reading_is_taken_in_the_largest_span_that_fits_it() {
        assert_eq!(ago(0), "just now");
        assert_eq!(ago(59), "just now");
        assert_eq!(ago(SECONDS_A_MINUTE), "1 minute ago");
        assert_eq!(ago(SECONDS_A_MINUTE * 5), "5 minutes ago");
        assert_eq!(ago(SECONDS_AN_HOUR * 3), "3 hours ago");
        assert_eq!(ago(SECONDS_A_DAY), "1 day ago");
        assert_eq!(ago(SECONDS_A_DAY * 6), "6 days ago");
        assert_eq!(ago(SECONDS_A_WEEK * 2), "2 weeks ago");
        assert_eq!(ago(SECONDS_A_MONTH * 5), "5 months ago");
        assert_eq!(ago(SECONDS_A_YEAR * 3), "3 years ago");
        assert_eq!(ago(400 * SECONDS_A_DAY), "1 year ago");
    }

    #[test]
    fn a_reading_short_enough_for_a_row_names_the_same_span() {
        assert_eq!(briefly(0), "now");
        assert_eq!(briefly(59), "now");
        assert_eq!(briefly(SECONDS_A_MINUTE * 5), "5m");
        assert_eq!(briefly(SECONDS_AN_HOUR * 3), "3h");
        assert_eq!(briefly(SECONDS_A_DAY * 6), "6d");
        assert_eq!(briefly(SECONDS_A_WEEK * 2), "2w");
        assert_eq!(briefly(SECONDS_A_MONTH * 5), "5mo");
        assert_eq!(briefly(SECONDS_A_YEAR * 3), "3y");
    }

    #[test]
    fn every_span_is_named_both_ways_and_no_two_read_alike() {
        let mut brief: Vec<String> = REACHES.iter().map(|reach| (reach.briefly)(1)).collect();
        brief.sort_unstable();
        let held = brief.len();
        brief.dedup();

        assert_eq!(brief.len(), held, "two spans answer to one short name");
        assert!(REACHES.iter().all(|reach| (reach.ago)(1) != (reach.ago)(2)));
    }

    #[test]
    fn a_reading_from_the_future_is_taken_as_now() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(SECONDS_A_YEAR * 40);

        assert_eq!(
            since(now + Duration::from_secs(SECONDS_A_DAY), now),
            "just now"
        );
        assert_eq!(age(now + Duration::from_secs(SECONDS_A_DAY), now), "now");
    }

    #[test]
    fn a_clock_shows_hours_once_there_are_any_and_never_more_than_sixty_minutes() {
        assert_eq!(clock(Duration::ZERO), "0:00");
        assert_eq!(clock(Duration::from_secs(59)), "0:59");
        assert_eq!(clock(Duration::from_secs(60)), "1:00");
        assert_eq!(clock(Duration::from_secs(3_599)), "59:59");
        assert_eq!(clock(Duration::from_secs(3_600)), "1:00:00");
        assert_eq!(clock(Duration::from_secs(4_503)), "1:15:03");
        assert_eq!(clock(Duration::from_millis(61_999)), "1:01");
    }
}
