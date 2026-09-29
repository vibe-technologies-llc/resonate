use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tz::TimeZone;

use crate::date::{CivilDate, SECONDS_PER_DAY, seconds_since_the_epoch};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Calendar {
    zone: TimeZone,
}

impl Calendar {
    pub fn local() -> Self {
        TimeZone::local().map_or_else(|_| Self::utc(), |zone| Self { zone })
    }

    pub fn utc() -> Self {
        Self {
            zone: TimeZone::utc(),
        }
    }

    pub fn fixed(seconds_east_of_utc: i32) -> Option<Self> {
        TimeZone::fixed(seconds_east_of_utc)
            .ok()
            .map(|zone| Self { zone })
    }

    pub fn day_of(&self, at: SystemTime) -> i64 {
        self.day_at(seconds_since_the_epoch(at))
    }

    pub fn date_at(&self, at: SystemTime) -> CivilDate {
        CivilDate::of_day(self.day_of(at))
    }

    pub fn midnight_of(&self, day: i64) -> SystemTime {
        let reading = day.saturating_mul(SECONDS_PER_DAY);
        let before = reading.saturating_sub(self.offset_at(reading));
        let after = reading.saturating_sub(self.offset_at(before));

        let began = [before.min(after), before.max(after)]
            .into_iter()
            .find(|candidate| self.day_at(*candidate) == day)
            .unwrap_or(before);
        moment(began)
    }

    fn day_at(&self, seconds: i64) -> i64 {
        seconds
            .saturating_add(self.offset_at(seconds))
            .div_euclid(SECONDS_PER_DAY)
    }

    fn offset_at(&self, seconds: i64) -> i64 {
        self.zone
            .find_local_time_type(seconds)
            .map_or(0, |kind| i64::from(kind.ut_offset()))
    }
}

fn moment(seconds: i64) -> SystemTime {
    let apart = Duration::from_secs(seconds.unsigned_abs());
    if seconds < 0 {
        UNIX_EPOCH.checked_sub(apart).unwrap_or(UNIX_EPOCH)
    } else {
        UNIX_EPOCH.checked_add(apart).unwrap_or(UNIX_EPOCH)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AN_HOUR: i64 = 3_600;
    const TWO_HOURS_EAST: i32 = 7_200;
    const FIVE_HOURS_WEST: i32 = -18_000;
    const CENTRAL_EUROPE: &str = "CET-1CEST,M3.5.0,M10.5.0/3";
    const THE_SEVENTH_OF_JULY_2024: i64 = 19_911;
    const THE_LAST_SUNDAY_OF_MARCH_2024: i64 = 19_813;
    const THE_LAST_SUNDAY_OF_OCTOBER_2024: i64 = 20_023;
    const SKIPPING_MIDNIGHT_INTO_SUMMER: &str = "<-04>4<-03>,M8.2.0/0,M5.2.0/0";
    const THE_SECOND_SUNDAY_OF_AUGUST_2018: i64 = 17_755;

    fn at(seconds: i64) -> SystemTime {
        moment(seconds)
    }

    fn zoned(rule: &str) -> Calendar {
        Calendar {
            zone: TimeZone::from_posix_tz(rule).expect("the fixture rule reads"),
        }
    }

    #[test]
    fn a_moment_late_in_a_utc_day_is_the_next_day_east_of_greenwich_and_the_same_day_west() {
        let late = THE_SEVENTH_OF_JULY_2024 * SECONDS_PER_DAY + 23 * AN_HOUR;
        let east = Calendar::fixed(TWO_HOURS_EAST).expect("a fixed offset is a zone");
        let west = Calendar::fixed(FIVE_HOURS_WEST).expect("a fixed offset is a zone");

        assert_eq!(Calendar::utc().day_of(at(late)), THE_SEVENTH_OF_JULY_2024);
        assert_eq!(east.day_of(at(late)), THE_SEVENTH_OF_JULY_2024 + 1);
        assert_eq!(west.day_of(at(late)), THE_SEVENTH_OF_JULY_2024);
        assert_eq!(
            east.date_at(at(late)),
            CivilDate {
                year: 2024,
                month: 7,
                day: 8
            }
        );
    }

    #[test]
    fn a_day_begins_at_the_zones_own_midnight() {
        let east = Calendar::fixed(TWO_HOURS_EAST).expect("a fixed offset is a zone");
        let midnight = THE_SEVENTH_OF_JULY_2024 * SECONDS_PER_DAY;

        assert_eq!(
            Calendar::utc().midnight_of(THE_SEVENTH_OF_JULY_2024),
            at(midnight)
        );
        assert_eq!(
            east.midnight_of(THE_SEVENTH_OF_JULY_2024),
            at(midnight - 2 * AN_HOUR)
        );
        assert_eq!(
            east.day_of(east.midnight_of(THE_SEVENTH_OF_JULY_2024)),
            THE_SEVENTH_OF_JULY_2024
        );
    }

    #[test]
    fn a_day_the_clocks_change_on_is_as_long_as_the_clocks_make_it() {
        let europe = zoned(CENTRAL_EUROPE);
        let length = |day: i64| {
            europe
                .midnight_of(day + 1)
                .duration_since(europe.midnight_of(day))
                .expect("a day ends after it begins")
                .as_secs()
        };

        assert_eq!(length(THE_SEVENTH_OF_JULY_2024), 24 * AN_HOUR as u64);
        assert_eq!(length(THE_LAST_SUNDAY_OF_MARCH_2024), 23 * AN_HOUR as u64);
        assert_eq!(length(THE_LAST_SUNDAY_OF_OCTOBER_2024), 25 * AN_HOUR as u64);
    }

    #[test]
    fn a_day_whose_midnight_the_clocks_skip_begins_when_its_first_hour_does() {
        let skipping = zoned(SKIPPING_MIDNIGHT_INTO_SUMMER);
        let began = skipping.midnight_of(THE_SECOND_SUNDAY_OF_AUGUST_2018);

        assert_eq!(skipping.day_of(began), THE_SECOND_SUNDAY_OF_AUGUST_2018);
        assert_eq!(
            skipping.day_of(began - Duration::from_secs(1)),
            THE_SECOND_SUNDAY_OF_AUGUST_2018 - 1
        );
    }
}
