use std::time::{Duration, SystemTime};

use resonate_core::{CivilDate, SECONDS_PER_DAY, seconds_since_the_epoch};
use ureq::{
    Body,
    http::{HeaderMap, Response, header::RETRY_AFTER},
};

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const GMT: &str = "GMT";
const SECONDS_PER_HOUR: i64 = 3_600;
const SECONDS_PER_MINUTE: i64 = 60;
const HOURS_PER_DAY: i64 = 24;
const MINUTES_PER_HOUR: i64 = 60;
const LEAP_SECOND: i64 = 60;
const LAST_DAY_OF_A_MONTH: i64 = 31;
const TWO_DIGIT_YEARS_FROM: i64 = 70;
const NINETEEN_HUNDRED: i64 = 1_900;
const TWO_THOUSAND: i64 = 2_000;

pub fn retry_after(headers: &HeaderMap, now: SystemTime) -> Option<Duration> {
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let until = second_named(value)?.saturating_sub(seconds_since_the_epoch(now));
    Some(Duration::from_secs(u64::try_from(until).unwrap_or(0)))
}

pub fn retry_after_of(response: &Response<Body>) -> Option<Duration> {
    retry_after(response.headers(), SystemTime::now())
}

fn month_named(name: &str) -> Option<i64> {
    let month = MONTHS.iter().position(|month| *month == name)?;
    i64::try_from(month).ok().map(|month| month + 1)
}

fn of_two_digits(year: i64) -> i64 {
    if year < TWO_DIGIT_YEARS_FROM {
        TWO_THOUSAND + year
    } else {
        NINETEEN_HUNDRED + year
    }
}

fn clock_read(clock: &str) -> Option<i64> {
    let mut parts = clock.split(':').map(|part| part.parse::<i64>().ok());
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

fn second_named(date: &str) -> Option<i64> {
    let words: Vec<&str> = date
        .split([' ', ','])
        .filter(|word| !word.is_empty())
        .collect();
    let (day, month, year, clock) = match words.as_slice() {
        [_, day, month, year, clock, GMT] if year.len() == 4 => {
            (day.parse().ok()?, *month, year.parse().ok()?, *clock)
        }
        [_, dated, clock, GMT] => {
            let mut parts = dated.split('-');
            let (Some(day), Some(month), Some(year), None) =
                (parts.next(), parts.next(), parts.next(), parts.next())
            else {
                return None;
            };
            (
                day.parse().ok()?,
                month,
                of_two_digits(year.parse().ok()?),
                *clock,
            )
        }
        [_, month, day, clock, year] => (day.parse().ok()?, *month, year.parse().ok()?, *clock),
        _ => return None,
    };
    if !(1..=LAST_DAY_OF_A_MONTH).contains(&day) {
        return None;
    }
    let month = month_named(month)?;
    let day = CivilDate { year, month, day }.day_since_the_epoch();
    Some(day.saturating_mul(SECONDS_PER_DAY) + clock_read(clock)?)
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
