use std::time::Duration;

const SECONDS_A_MINUTE: u64 = 60;
const SECONDS_AN_HOUR: u64 = 60 * SECONDS_A_MINUTE;
const MOST_CLOCK_FIELDS: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bare {
    Seconds,
    Minutes,
}

impl Bare {
    const fn seconds(self) -> u64 {
        match self {
            Self::Seconds => 1,
            Self::Minutes => SECONDS_A_MINUTE,
        }
    }
}

pub fn lasting(written: &str, bare: Bare) -> Option<Duration> {
    let written = written.trim();
    let seconds = if written.bytes().all(|byte| byte.is_ascii_digit()) {
        digits(written)?.checked_mul(bare.seconds())
    } else if written.contains(':') {
        on_a_clock(written)
    } else {
        in_units(written)
    }?;
    Some(Duration::from_secs(seconds))
}

fn digits(written: &str) -> Option<u64> {
    if written.is_empty() || !written.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    written.parse().ok()
}

fn on_a_clock(written: &str) -> Option<u64> {
    let fields: Vec<&str> = written.split(':').collect();
    if fields.len() > MOST_CLOCK_FIELDS {
        return None;
    }

    let (leading, within) = fields.split_first()?;
    let mut seconds = digits(leading)?;
    for field in within {
        let field = digits(field)?;
        if field >= SECONDS_A_MINUTE {
            return None;
        }
        seconds = seconds.checked_mul(SECONDS_A_MINUTE)?.checked_add(field)?;
    }
    Some(seconds)
}

fn in_units(written: &str) -> Option<u64> {
    let mut seconds: u64 = 0;
    let mut rest = written;
    let mut coarsest_left = SECONDS_AN_HOUR.checked_mul(SECONDS_A_MINUTE)?;

    while !rest.is_empty() {
        let count_ends = rest.find(|letter: char| !letter.is_ascii_digit())?;
        let (count, after) = rest.split_at(count_ends);
        let unit_ends = after
            .find(|letter: char| letter.is_ascii_digit())
            .unwrap_or(after.len());
        let (unit, after) = after.split_at(unit_ends);

        let each = unit_seconds(unit)?;
        if each >= coarsest_left {
            return None;
        }
        coarsest_left = each;

        seconds = seconds.checked_add(digits(count)?.checked_mul(each)?)?;
        rest = after;
    }
    Some(seconds)
}

fn unit_seconds(unit: &str) -> Option<u64> {
    match unit.to_ascii_lowercase().as_str() {
        "h" | "hr" | "hrs" | "hour" | "hours" => Some(SECONDS_AN_HOUR),
        "m" | "min" | "mins" | "minute" | "minutes" => Some(SECONDS_A_MINUTE),
        "s" | "sec" | "secs" | "second" | "seconds" => Some(1),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seconds(count: u64) -> Option<Duration> {
        Some(Duration::from_secs(count))
    }

    #[test]
    fn a_bare_number_is_counted_in_the_unit_the_caller_names() {
        assert_eq!(lasting("90", Bare::Seconds), seconds(90));
        assert_eq!(lasting(" 30 ", Bare::Minutes), seconds(1_800));
        assert_eq!(lasting("0", Bare::Seconds), seconds(0));
    }

    #[test]
    fn a_clock_reads_as_minutes_and_seconds_or_hours_minutes_and_seconds() {
        assert_eq!(lasting("1:30", Bare::Seconds), seconds(90));
        assert_eq!(lasting("1:30", Bare::Minutes), seconds(90));
        assert_eq!(lasting("1:02:03", Bare::Seconds), seconds(3_723));
        assert_eq!(lasting("90:00", Bare::Seconds), seconds(5_400));
    }

    #[test]
    fn units_are_read_coarsest_first_whatever_they_are_spelled() {
        assert_eq!(lasting("30m", Bare::Seconds), seconds(1_800));
        assert_eq!(lasting("1h30m", Bare::Seconds), seconds(5_400));
        assert_eq!(lasting("2H", Bare::Seconds), seconds(7_200));
        assert_eq!(lasting("1m30s", Bare::Minutes), seconds(90));
        assert_eq!(lasting("45sec", Bare::Minutes), seconds(45));
        assert_eq!(lasting("10mins", Bare::Seconds), seconds(600));
    }

    #[test]
    fn anything_else_is_not_a_length() {
        for refused in [
            "",
            "-5",
            "+5",
            "1.5",
            "soon",
            "m",
            "30x",
            "1:60",
            "1:2:3:4",
            ":30",
            "1:",
            "30s1m",
            "1m1m",
            "1 m",
            "18446744073709551615m",
            "99999999999999999999",
        ] {
            assert_eq!(
                lasting(refused, Bare::Seconds),
                None,
                "{refused:?} was read"
            );
        }
    }
}
