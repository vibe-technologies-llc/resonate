use std::time::{Duration, SystemTime};

use resonate_core::{AppliedGain, Frames, MediaLocation, SampleFormat, SampleRate, StreamSpec};
use resonate_engine::OutputMode;
use smallvec::SmallVec;

use crate::theme;

const PARTS_HELD_INLINE: usize = 6;

pub type Parts<T> = SmallVec<[T; PARTS_HELD_INLINE]>;

pub fn clock(frames: Frames, rate: SampleRate) -> String {
    let seconds = frames.to_duration(rate).as_secs();
    let minutes = seconds / 60;
    let hours = minutes / 60;

    if hours > 0 {
        format!("{hours}:{:02}:{:02}", minutes % 60, seconds % 60)
    } else {
        format!("{minutes}:{:02}", seconds % 60)
    }
}

pub fn depth(format: SampleFormat) -> &'static str {
    match format {
        SampleFormat::S16 => "16-bit",
        SampleFormat::S24 => "24-bit",
        SampleFormat::S32 => "32-bit",
        SampleFormat::F32 => "32-bit float",
    }
}

pub fn stem(location: &MediaLocation) -> String {
    location
        .stem()
        .map_or_else(|| location.to_string(), |stem| stem.into_owned())
}

pub fn quality(spec: StreamSpec) -> String {
    format!("{} / {}", depth(spec.format), spec.rate)
}

pub fn kilohertz(rate: SampleRate) -> String {
    let hz = rate.hz();
    if hz.is_multiple_of(1_000) {
        format!("{}", hz / 1_000)
    } else {
        format!("{}.{}", hz / 1_000, (hz % 1_000) / 100)
    }
}

pub fn spanned(played: Duration) -> String {
    let seconds = played.as_secs();
    let minutes = seconds / 60;

    match minutes / 60 {
        0 => format!("{minutes}:{:02}", seconds % 60),
        hours => format!("{hours}:{:02}:{:02}", minutes % 60, seconds % 60),
    }
}

pub fn counting_down(left: Duration) -> String {
    let seconds = left.as_secs() + u64::from(left.subsec_nanos() > 0);

    format!("{}:{:02}", seconds / A_MINUTE, seconds % A_MINUTE)
}

pub fn counted(count: usize, one: &str, many: &str) -> String {
    match count {
        1 => format!("1 {one}"),
        count => format!("{count} {many}"),
    }
}

const A_MINUTE: u64 = 60;
const AN_HOUR: u64 = 60 * A_MINUTE;
const MINUTES_AN_HOUR: u64 = AN_HOUR / A_MINUTE;
const A_DAY: u64 = 24 * AN_HOUR;
const A_WEEK: u64 = 7 * A_DAY;
const A_MONTH: u64 = 30 * A_DAY;
const A_YEAR: u64 = 365 * A_DAY;

struct Span {
    seconds: u64,
    one: &'static str,
    many: &'static str,
    brief: &'static str,
}

const SPANS: [Span; 6] = [
    Span {
        seconds: A_YEAR,
        one: "year",
        many: "years",
        brief: "y",
    },
    Span {
        seconds: A_MONTH,
        one: "month",
        many: "months",
        brief: "mo",
    },
    Span {
        seconds: A_WEEK,
        one: "week",
        many: "weeks",
        brief: "w",
    },
    Span {
        seconds: A_DAY,
        one: "day",
        many: "days",
        brief: "d",
    },
    Span {
        seconds: AN_HOUR,
        one: "hour",
        many: "hours",
        brief: "h",
    },
    Span {
        seconds: A_MINUTE,
        one: "minute",
        many: "minutes",
        brief: "m",
    },
];

pub fn heard_for(span: Duration) -> String {
    let seconds = span.as_secs();
    let minutes = seconds / A_MINUTE;

    match seconds / AN_HOUR {
        0 if minutes == 0 => format!("{seconds}s"),
        0 => format!("{minutes}m"),
        hours => format!("{hours}h {}m", minutes % MINUTES_AN_HOUR),
    }
}

fn seconds_since(when: SystemTime, now: SystemTime) -> u64 {
    now.duration_since(when).unwrap_or_default().as_secs()
}

pub fn since(when: SystemTime, now: SystemTime) -> String {
    let ago = seconds_since(when, now);

    for span in SPANS {
        if ago >= span.seconds {
            return format!(
                "{} ago",
                counted((ago / span.seconds) as usize, span.one, span.many)
            );
        }
    }

    "just now".to_owned()
}

pub fn age(when: SystemTime, now: SystemTime) -> String {
    let ago = seconds_since(when, now);

    for span in SPANS {
        if ago >= span.seconds {
            return format!("{}{}", ago / span.seconds, span.brief);
        }
    }

    "now".to_owned()
}

pub fn applied(gain: AppliedGain) -> String {
    let landed = rounded(gain.applied().to_decibels().get());
    match (gain.gain, gain.is_capped()) {
        (None, false) => "none".to_owned(),
        (None, true) => format!("{landed:+.2} dB, to keep the peak under full scale"),
        (Some(_), false) => format!("{landed:+.2} dB"),
        (Some(requested), true) => {
            format!(
                "{landed:+.2} dB, capped from {:+.2} dB",
                rounded(requested.get())
            )
        }
    }
}

pub fn headroom(gain: AppliedGain) -> String {
    gain.headroom().map_or_else(
        || "unknown".to_owned(),
        |margin| format!("{:+.2} dB", rounded(margin.get())),
    )
}

pub fn mode(mode: OutputMode) -> (&'static str, u32) {
    match mode {
        OutputMode::BitPerfect => ("bit-perfect", theme::bit_perfect()),
        OutputMode::Repacked => ("repacked", theme::repacked()),
        OutputMode::Dithered => ("dithered", theme::dithered()),
        OutputMode::Converted => ("converted", theme::converted()),
    }
}

pub fn bytes(count: u64) -> String {
    const UNITS: [&str; 3] = ["KiB", "MiB", "GiB"];

    let mut scaled = count as f64;
    let mut chosen = None;
    for unit in UNITS {
        if scaled < 1024.0 {
            break;
        }
        scaled /= 1024.0;
        chosen = Some(unit);
    }

    match chosen {
        Some(unit) => format!("{scaled:.1} {unit}"),
        None => format!("{count} B"),
    }
}

pub fn progress(position: Frames, duration: Option<Frames>) -> f32 {
    let Some(duration) = duration.filter(|duration| duration.get() > 0) else {
        return 0.0;
    };
    (position.get() as f32 / duration.get() as f32).clamp(0.0, 1.0)
}

fn rounded(db: f32) -> f32 {
    (db * 100.0).round() / 100.0 + 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ago(seconds: u64) -> String {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(A_YEAR * 40);
        since(now - Duration::from_secs(seconds), now)
    }

    #[test]
    fn a_reading_is_taken_in_the_largest_span_that_fits_it() {
        assert_eq!(ago(0), "just now");
        assert_eq!(ago(59), "just now");
        assert_eq!(ago(A_MINUTE), "1 minute ago");
        assert_eq!(ago(A_MINUTE * 5), "5 minutes ago");
        assert_eq!(ago(AN_HOUR * 3), "3 hours ago");
        assert_eq!(ago(A_DAY), "1 day ago");
        assert_eq!(ago(A_DAY * 6), "6 days ago");
        assert_eq!(ago(A_WEEK * 2), "2 weeks ago");
        assert_eq!(ago(A_MONTH * 5), "5 months ago");
        assert_eq!(ago(A_YEAR * 3), "3 years ago");
    }

    fn briefly(seconds: u64) -> String {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(A_YEAR * 40);
        age(now - Duration::from_secs(seconds), now)
    }

    #[test]
    fn a_reading_short_enough_for_a_row_names_the_same_span() {
        assert_eq!(briefly(0), "now");
        assert_eq!(briefly(59), "now");
        assert_eq!(briefly(A_MINUTE * 5), "5m");
        assert_eq!(briefly(AN_HOUR * 3), "3h");
        assert_eq!(briefly(A_DAY * 6), "6d");
        assert_eq!(briefly(A_WEEK * 2), "2w");
        assert_eq!(briefly(A_MONTH * 5), "5mo");
        assert_eq!(briefly(A_YEAR * 3), "3y");
    }

    #[test]
    fn every_span_is_named_both_ways_and_no_two_read_alike() {
        let mut brief: Vec<&str> = SPANS.iter().map(|span| span.brief).collect();
        brief.sort_unstable();
        let held = brief.len();
        brief.dedup();

        assert_eq!(brief.len(), held, "two spans answer to one short name");
        assert!(SPANS.iter().all(|span| !span.one.is_empty()));
    }

    #[test]
    fn a_long_listening_time_is_drawn_in_hours_rather_than_minutes() {
        let many_hours = Duration::from_secs(413 * AN_HOUR + 27 * A_MINUTE + 9);

        assert_eq!(heard_for(many_hours), "413h 27m");
        assert_eq!(heard_for(Duration::from_secs(AN_HOUR)), "1h 0m");
    }

    #[test]
    fn a_listening_time_short_of_an_hour_is_drawn_in_minutes_and_one_short_of_a_minute_in_seconds()
    {
        assert_eq!(heard_for(Duration::from_secs(3_599)), "59m");
        assert_eq!(heard_for(Duration::from_secs(60)), "1m");
        assert_eq!(heard_for(Duration::from_secs(59)), "59s");
        assert_eq!(heard_for(Duration::ZERO), "0s");
    }

    #[test]
    fn a_countdown_is_read_in_minutes_and_seconds_however_many_minutes_are_left() {
        assert_eq!(counting_down(Duration::from_secs(90 * A_MINUTE)), "90:00");
        assert_eq!(counting_down(Duration::from_secs(15 * A_MINUTE)), "15:00");
        assert_eq!(counting_down(Duration::from_secs(61)), "1:01");
        assert_eq!(counting_down(Duration::ZERO), "0:00");
    }

    #[test]
    fn a_countdown_rounds_a_part_second_up_so_a_timer_just_set_reads_its_whole_length() {
        let just_set = Duration::from_secs(15 * A_MINUTE) - Duration::from_millis(1);

        assert_eq!(counting_down(just_set), "15:00");
        assert_eq!(counting_down(Duration::from_millis(1)), "0:01");
    }

    #[test]
    fn a_reading_from_the_future_is_taken_as_now() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(A_YEAR * 40);
        assert_eq!(since(now + Duration::from_secs(A_DAY), now), "just now");
        assert_eq!(age(now + Duration::from_secs(A_DAY), now), "now");
    }
}
