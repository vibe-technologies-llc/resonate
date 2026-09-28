use std::time::{SystemTime, UNIX_EPOCH};

pub const SECONDS_PER_DAY: i64 = 86_400;
const DAYS_FROM_0000_03_01_TO_THE_EPOCH: i64 = 719_468;
const DAYS_PER_ERA: i64 = 146_097;
const LAST_DAY_OF_AN_ERA: i64 = DAYS_PER_ERA - 1;
const YEARS_PER_ERA: i64 = 400;
const DAYS_PER_COMMON_YEAR: i64 = 365;
const DAYS_PER_FOUR_YEARS: i64 = 1_460;
const DAYS_PER_CENTURY: i64 = 36_524;
const DAYS_PER_FIVE_MONTHS: i64 = 153;
const MARCH: i64 = 3;
const MONTHS_PER_YEAR: i64 = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CivilDate {
    pub year: i64,
    pub month: i64,
    pub day: i64,
}

impl CivilDate {
    pub fn of_day(days_since_the_epoch: i64) -> Self {
        let shifted = days_since_the_epoch.saturating_add(DAYS_FROM_0000_03_01_TO_THE_EPOCH);
        let era = shifted.div_euclid(DAYS_PER_ERA);
        let day_of_era = shifted.rem_euclid(DAYS_PER_ERA);
        let year_of_era = (day_of_era - day_of_era / DAYS_PER_FOUR_YEARS
            + day_of_era / DAYS_PER_CENTURY
            - day_of_era / LAST_DAY_OF_AN_ERA)
            / DAYS_PER_COMMON_YEAR;
        let day_of_year =
            day_of_era - (DAYS_PER_COMMON_YEAR * year_of_era + year_of_era / 4 - year_of_era / 100);
        let months_from_march = (5 * day_of_year + 2) / DAYS_PER_FIVE_MONTHS;
        let day = day_of_year - (DAYS_PER_FIVE_MONTHS * months_from_march + 2) / 5 + 1;
        let month = months_from_march + MARCH;
        let month = if month > MONTHS_PER_YEAR {
            month - MONTHS_PER_YEAR
        } else {
            month
        };
        let year = year_of_era + era * YEARS_PER_ERA + i64::from(month <= 2);

        Self { year, month, day }
    }

    pub fn at(moment: SystemTime) -> Self {
        Self::of_day(seconds_since_the_epoch(moment).div_euclid(SECONDS_PER_DAY))
    }

    pub const fn month_named(self) -> &'static str {
        match self.month {
            1 => "Jan",
            2 => "Feb",
            3 => "Mar",
            4 => "Apr",
            5 => "May",
            6 => "Jun",
            7 => "Jul",
            8 => "Aug",
            9 => "Sep",
            10 => "Oct",
            11 => "Nov",
            _ => "Dec",
        }
    }
}

pub fn seconds_since_the_epoch(at: SystemTime) -> i64 {
    match at.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
        Err(before) => {
            let before = before.duration();
            let whole = i64::try_from(before.as_secs()).unwrap_or(i64::MAX);
            let part_of_a_second = i64::from(before.subsec_nanos() > 0);
            whole.saturating_add(part_of_a_second).saturating_neg()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_day_is_read_as_the_date_the_gregorian_calendar_gives_it() {
        assert_eq!(
            CivilDate::of_day(0),
            CivilDate {
                year: 1970,
                month: 1,
                day: 1
            }
        );
        assert_eq!(
            CivilDate::of_day(19_782),
            CivilDate {
                year: 2024,
                month: 2,
                day: 29
            }
        );
        assert_eq!(
            CivilDate::of_day(-1),
            CivilDate {
                year: 1969,
                month: 12,
                day: 31
            }
        );
        assert_eq!(
            CivilDate::at(UNIX_EPOCH - Duration::from_nanos(1)),
            CivilDate {
                year: 1969,
                month: 12,
                day: 31
            }
        );
        assert_eq!(CivilDate::of_day(19_782).month_named(), "Feb");
    }
}
