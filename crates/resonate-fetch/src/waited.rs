use std::{
    ops::RangeInclusive,
    time::{Duration, SystemTime},
};

use resonate_core::{CivilDate, SECONDS_PER_DAY, seconds_since_the_epoch};
use ureq::{
    Body,
    http::{HeaderMap, Response, header::RETRY_AFTER},
};

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const DAYS_IN_A_MONTH: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
const FEBRUARY: i64 = 2;
const GMT: &str = "GMT";
const SECONDS_PER_HOUR: i64 = 3_600;
const SECONDS_PER_MINUTE: i64 = 60;
const HOURS_PER_DAY: i64 = 24;
const MINUTES_PER_HOUR: i64 = 60;
const LEAP_SECOND: i64 = 60;
const YEARS_PER_CENTURY: i64 = 100;
const FULL_YEAR_DIGITS: usize = 4;
const SHORT_YEAR_DIGITS: usize = 2;
const CLOCK_FIELD_DIGITS: usize = 2;
const DAY_DIGITS_AT_MOST: usize = 2;
const FUTURE_A_SHORT_YEAR_MAY_NAME: i64 = 50;

#[derive(Clone, Copy)]
enum Year {
    Full(i64),
    Short(i64),
}

pub fn retry_after(headers: &HeaderMap, now: SystemTime) -> Option<Duration> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();

    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }

    let until = second_named(value, now)?.saturating_sub(seconds_since_the_epoch(now));
    Some(Duration::from_secs(u64::try_from(until).unwrap_or(0)))
}

pub fn retry_after_of(response: &Response<Body>) -> Option<Duration> {
    retry_after(response.headers(), SystemTime::now())
}

fn month_named(name: &str) -> Option<i64> {
    let month = MONTHS.iter().position(|month| *month == name)?;
    i64::try_from(month).ok().map(|month| month + 1)
}

fn digits_in(text: &str, lengths: RangeInclusive<usize>) -> Option<i64> {
    let all_digits = text.bytes().all(|byte| byte.is_ascii_digit());
    (all_digits && lengths.contains(&text.len()))
        .then(|| text.parse().ok())
        .flatten()
}

fn full_year(text: &str) -> Option<Year> {
    digits_in(text, FULL_YEAR_DIGITS..=FULL_YEAR_DIGITS).map(Year::Full)
}

fn short_year(text: &str) -> Option<Year> {
    digits_in(text, SHORT_YEAR_DIGITS..=SHORT_YEAR_DIGITS).map(Year::Short)
}

fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % YEARS_PER_CENTURY != 0 || year % (YEARS_PER_CENTURY * 4) == 0)
}

fn days_in(year: i64, month: i64) -> Option<i64> {
    let index = usize::try_from(month - 1).ok()?;
    let days = *DAYS_IN_A_MONTH.get(index)?;
    Some(days + i64::from(month == FEBRUARY && is_leap(year)))
}

fn clock_read(clock: &str) -> Option<i64> {
    let mut parts = clock
        .split(':')
        .map(|part| digits_in(part, CLOCK_FIELD_DIGITS..=CLOCK_FIELD_DIGITS));

    let (Some(Some(hour)), Some(Some(minute)), Some(Some(second)), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };

    let read = (0..HOURS_PER_DAY).contains(&hour)
        && (0..MINUTES_PER_HOUR).contains(&minute)
        && (0..=LEAP_SECOND).contains(&second);
    read.then_some(hour * SECONDS_PER_HOUR + minute * SECONDS_PER_MINUTE + second)
}

fn year_against(year: Year, month: i64, day: i64, clock: i64, now: SystemTime) -> i64 {
    let short = match year {
        Year::Full(year) => return year,
        Year::Short(short) => short,
    };

    let today = CivilDate::at(now);
    let into_the_day = seconds_since_the_epoch(now).rem_euclid(SECONDS_PER_DAY);
    let next_century =
        today.year - today.year.rem_euclid(YEARS_PER_CENTURY) + YEARS_PER_CENTURY + short;

    let furthest_named = (
        today.year + FUTURE_A_SHORT_YEAR_MAY_NAME,
        today.month,
        today.day,
        into_the_day,
    );

    let mut year = next_century;

    while (year, month, day, clock) > furthest_named {
        year -= YEARS_PER_CENTURY;
    }
    year
}

fn second_named(date: &str, now: SystemTime) -> Option<i64> {
    let words: Vec<&str> = date
        .split([' ', ','])
        .filter(|word| !word.is_empty())
        .collect();
    let (day, month, year, clock) = match words.as_slice() {
        [_, day, month, year, clock, GMT] => (*day, *month, full_year(year)?, *clock),
        [_, dated, clock, GMT] => {
            let mut parts = dated.split('-');

            let (Some(day), Some(month), Some(year), None) =
                (parts.next(), parts.next(), parts.next(), parts.next())
            else {
                return None;
            };
            (day, month, short_year(year)?, *clock)
        }
        [_, month, day, clock, year] => (*day, *month, full_year(year)?, *clock),
        _ => return None,
    };

    let month = month_named(month)?;
    let day = digits_in(day, 1..=DAY_DIGITS_AT_MOST)?;
    let clock = clock_read(clock)?;

    let year = year_against(year, month, day, clock, now);

    if !(1..=days_in(year, month)?).contains(&day) {
        return None;
    }

    let day = CivilDate { year, month, day }.day_since_the_epoch()?;
    Some(day.saturating_mul(SECONDS_PER_DAY) + clock)
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use ureq::http::HeaderValue;

    use super::*;

    const NOVEMBER_THE_SIXTH_1994: u64 = 784_111_777;

    fn asked(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();

        headers.insert(
            RETRY_AFTER,
            HeaderValue::from_str(value).expect("a header value"),
        );
        headers
    }

    #[test]
    fn a_wait_is_read_in_seconds() {
        assert_eq!(
            retry_after(&asked(" 120 "), SystemTime::now()),
            Some(Duration::from_secs(120))
        );
        assert_eq!(retry_after(&HeaderMap::new(), SystemTime::now()), None);
        assert_eq!(retry_after(&asked("soon"), SystemTime::now()), None);
    }

    #[test]
    fn a_wait_is_read_off_a_date_in_each_of_the_three_forms_http_allows() {
        let now = UNIX_EPOCH + Duration::from_secs(NOVEMBER_THE_SIXTH_1994 - 30);

        for date in [
            "Sun, 06 Nov 1994 08:49:37 GMT",
            "Sunday, 06-Nov-94 08:49:37 GMT",
            "Sun Nov  6 08:49:37 1994",
        ] {
            assert_eq!(
                retry_after(&asked(date), now),
                Some(Duration::from_secs(30)),
                "{date}"
            );
        }
    }

    #[test]
    fn a_date_in_asctimes_form_naming_a_year_past_any_calendar_is_none() {
        let now = UNIX_EPOCH + Duration::from_secs(NOVEMBER_THE_SIXTH_1994);

        for year in ["9223372036854775807", "-9223372036854775808", "10000", "-1"] {
            let date = format!("Sun Nov  6 08:49:37 {year}");

            assert_eq!(retry_after(&asked(&date), now), None, "{date}");
        }
    }

    fn second_of(date: CivilDate, clock: i64) -> u64 {
        u64::try_from(date.day_since_the_epoch().expect("a day") * SECONDS_PER_DAY + clock)
            .expect("after 1970")
    }

    fn the_sixth_of_november(year: i64) -> CivilDate {
        CivilDate {
            year,
            month: 11,
            day: 6,
        }
    }

    const EARLY_MORNING: i64 = 8 * SECONDS_PER_HOUR + 49 * SECONDS_PER_MINUTE + 37;

    fn at(date: CivilDate, clock: i64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(second_of(date, clock))
    }

    #[test]
    fn a_short_year_not_more_than_fifty_years_ahead_is_in_this_century() {
        let now = at(the_sixth_of_november(2026), EARLY_MORNING);
        let ahead = Duration::from_secs(
            second_of(the_sixth_of_november(2076), EARLY_MORNING)
                - second_of(the_sixth_of_november(2026), EARLY_MORNING),
        );

        for date in [
            "Sunday, 06-Nov-76 08:49:37 GMT",
            "Sunday, 06-Nov-70 08:49:37 GMT",
        ] {
            assert!(
                retry_after(&asked(date), now) > Some(Duration::ZERO),
                "{date}"
            );
        }

        assert_eq!(
            retry_after(&asked("Sunday, 06-Nov-76 08:49:37 GMT"), now),
            Some(ahead)
        );
    }

    #[test]
    fn a_short_year_more_than_fifty_years_ahead_is_in_the_century_before() {
        let now = at(the_sixth_of_november(2026), EARLY_MORNING);

        assert_eq!(
            retry_after(&asked("Sunday, 06-Nov-77 08:49:37 GMT"), now),
            Some(Duration::ZERO)
        );
        assert_eq!(
            retry_after(&asked("Sunday, 06-Nov-94 08:49:37 GMT"), now),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn a_short_year_is_judged_to_the_second_at_fifty_years_ahead() {
        let now = at(the_sixth_of_november(2026), EARLY_MORNING);
        let exactly = "Sunday, 06-Nov-76 08:49:37 GMT";
        let a_second_over = "Sunday, 06-Nov-76 08:49:38 GMT";

        assert!(retry_after(&asked(exactly), now) > Some(Duration::ZERO));
        assert_eq!(
            retry_after(&asked(a_second_over), now),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn a_short_year_follows_the_clock_across_a_century() {
        let now = at(the_sixth_of_november(1999), EARLY_MORNING);

        assert!(retry_after(&asked("Sunday, 06-Nov-49 08:49:37 GMT"), now) > Some(Duration::ZERO));
        assert_eq!(
            retry_after(&asked("Sunday, 06-Nov-50 08:49:37 GMT"), now),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn a_leap_day_is_read_only_in_a_leap_year() {
        let now = UNIX_EPOCH;

        for date in [
            "Thu, 29 Feb 2024 08:49:37 GMT",
            "Tue, 29 Feb 2000 08:49:37 GMT",
            "Thu Feb 29 08:49:37 2024",
        ] {
            assert!(
                retry_after(&asked(date), now) > Some(Duration::ZERO),
                "{date}"
            );
        }
        for date in [
            "Wed, 29 Feb 2023 08:49:37 GMT",
            "Mon, 29 Feb 2100 08:49:37 GMT",
            "Wed, 31 Apr 2024 08:49:37 GMT",
            "Wed, 00 Apr 2024 08:49:37 GMT",
        ] {
            assert_eq!(retry_after(&asked(date), now), None, "{date}");
        }
    }

    #[test]
    fn a_year_short_of_four_digits_is_no_date_outside_the_two_digit_form() {
        let now = at(the_sixth_of_november(1994), EARLY_MORNING);

        for date in [
            "Sun Nov  6 08:49:37 94",
            "Sun Nov  6 08:49:37 994",
            "Sun, 06 Nov 94 08:49:37 GMT",
            "Sunday, 06-Nov-1994 08:49:37 GMT",
            "Sunday, 06-Nov-9 08:49:37 GMT",
            "Sun, 06 Nov +994 08:49:37 GMT",
        ] {
            assert_eq!(retry_after(&asked(date), now), None, "{date}");
        }
    }

    #[test]
    fn a_date_already_past_is_no_wait_and_a_misspelt_one_is_none() {
        let now = UNIX_EPOCH + Duration::from_secs(NOVEMBER_THE_SIXTH_1994 + 30);

        assert_eq!(
            retry_after(&asked("Sun, 06 Nov 1994 08:49:37 GMT"), now),
            Some(Duration::ZERO)
        );
        assert_eq!(
            retry_after(&asked("Sun, 06 Nob 1994 08:49:37 GMT"), now),
            None
        );
        assert_eq!(
            retry_after(&asked("Sun, 06 Nov 1994 25:49:37 GMT"), now),
            None
        );
        assert_eq!(
            retry_after(&asked("Sun, 32 Nov 1994 08:49:37 GMT"), now),
            None
        );
    }
}
