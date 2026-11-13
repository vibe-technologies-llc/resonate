use std::time::Duration;

use resonate_core::{
    AppliedGain, Frames, MediaLocation, SampleFormat, SampleRate, StreamSpec, elapsed,
};
use resonate_engine::OutputMode;
use smallvec::SmallVec;

use crate::{theme, words};

const PARTS_HELD_INLINE: usize = 6;

pub type Parts<T> = SmallVec<[T; PARTS_HELD_INLINE]>;

pub use resonate_core::elapsed::{age, since};

pub fn clock(frames: Frames, rate: SampleRate) -> String {
    elapsed::clock(frames.to_duration(rate))
}

pub fn left(position: Frames, duration: Frames, rate: SampleRate) -> String {
    let length = duration.to_duration(rate).as_secs();
    let played = position.to_duration(rate).as_secs();
    elapsed::clock(Duration::from_secs(length.saturating_sub(played)))
}

pub fn depth(format: SampleFormat) -> &'static str {
    match format {
        SampleFormat::S16 => words::format::depth_16(),
        SampleFormat::S24 => words::format::depth_24(),
        SampleFormat::S32 => words::format::depth_32(),
        SampleFormat::F32 => words::format::depth_32_float(),
    }
}

pub fn stem(location: &MediaLocation) -> String {
    location
        .stem()
        .map_or_else(|| location.to_string(), |stem| stem.into_owned())
}

pub fn quality(spec: StreamSpec) -> String {
    words::format::quality(depth(spec.format), spec.rate.to_string())
}

pub fn spanned(played: Duration) -> String {
    elapsed::clock(played)
}

pub fn counting_down(left: Duration) -> String {
    let seconds = left.as_secs() + u64::from(left.subsec_nanos() > 0);

    format!("{}:{:02}", seconds / A_MINUTE, seconds % A_MINUTE)
}

const A_MINUTE: u64 = 60;
const AN_HOUR: u64 = 60 * A_MINUTE;
const MINUTES_AN_HOUR: u64 = AN_HOUR / A_MINUTE;

pub fn heard_for(span: Duration) -> String {
    let seconds = span.as_secs();
    let minutes = seconds / A_MINUTE;

    match seconds / AN_HOUR {
        0 if minutes == 0 => words::format::heard_seconds(seconds),
        0 => words::format::heard_minutes(minutes),
        hours => words::format::heard_hours(hours, minutes % MINUTES_AN_HOUR),
    }
}

pub fn applied(gain: AppliedGain) -> String {
    let landed = rounded(gain.applied().to_decibels().get());
    let landed = format!("{landed:+.2}");
    match (gain.gain, gain.is_capped()) {
        (None, false) => words::format::gain_none().to_owned(),
        (None, true) => words::format::gain_kept_under_full_scale(landed),
        (Some(_), false) => words::format::gain(landed),
        (Some(requested), true) => {
            words::format::gain_capped(landed, format!("{:+.2}", rounded(requested.get())))
        }
    }
}

pub fn headroom(gain: AppliedGain) -> String {
    gain.headroom().map_or_else(
        || words::format::headroom_unknown().to_owned(),
        |margin| words::format::headroom(format!("{:+.2}", rounded(margin.get()))),
    )
}

pub fn mode(mode: OutputMode) -> (&'static str, u32) {
    match mode {
        OutputMode::BitPerfect => (words::format::bit_perfect(), theme::bit_perfect()),
        OutputMode::Repacked => (words::format::repacked(), theme::repacked()),
        OutputMode::Dithered => (words::format::dithered(), theme::dithered()),
        OutputMode::Converted => (words::format::converted(), theme::converted()),
    }
}

pub fn bytes(count: u64) -> String {
    const UNITS: [fn(String) -> String; 3] = [
        words::format::kibibytes,
        words::format::mebibytes,
        words::format::gibibytes,
    ];

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
        Some(unit) => unit(resonate_words::decimal(scaled, 1)),
        None => words::format::bytes(count),
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
    fn the_time_elapsed_and_the_time_left_add_up_to_the_length_through_the_last_second() {
        let rate = SampleRate::HZ_44100;
        let length = Frames(44_100 * 210 + 30_000);

        for position in [
            Frames::ZERO,
            Frames(44_100 * 60 + 8_000),
            Frames(44_100 * 209 + 40_000),
            Frames(44_100 * 210),
            length,
        ] {
            let elapsed = position.to_duration(rate).as_secs();
            let seconds_left = length.to_duration(rate).as_secs() - elapsed;

            assert_eq!(elapsed + seconds_left, 210);
            assert_eq!(
                clock(Frames(44_100 * seconds_left), rate),
                left(position, length, rate)
            );
        }
        assert_eq!(left(Frames(44_100 * 209 + 40_000), length, rate), "0:01");
        assert_eq!(left(length, length, rate), "0:00");
    }
}
