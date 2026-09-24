use std::time::Duration;

use resonate_core::{Frames, SampleRate};
use symphonia::core::units::{Duration as Ticks, Time, TimeBase, Timestamp};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timeline {
    time_base: Option<TimeBase>,
    music_at: Timestamp,
    rate: SampleRate,
}

impl Timeline {
    pub const fn new(time_base: Option<TimeBase>, music_at: Timestamp, rate: SampleRate) -> Self {
        Self {
            time_base,
            music_at,
            rate,
        }
    }

    pub fn at(rate: SampleRate) -> Self {
        Self {
            time_base: TimeBase::try_from_recip(rate.hz()),
            music_at: Timestamp::ZERO,
            rate,
        }
    }

    pub fn is_sample_accurate(&self) -> bool {
        self.time_base == TimeBase::try_from_recip(self.rate.hz())
    }

    pub fn timestamp(&self, frames: Frames) -> Option<Timestamp> {
        if !self.is_sample_accurate() {
            return None;
        }
        self.music_at.checked_add(Ticks::new(frames.get()))
    }

    pub fn frames(&self, ts: Timestamp) -> Frames {
        let elapsed = u64::try_from(ts.saturating_delta(self.music_at).get()).unwrap_or(0);
        self.span(Ticks::new(elapsed))
    }

    pub fn short_of_the_music(&self, ts: Timestamp) -> Frames {
        let short = u64::try_from(self.music_at.saturating_delta(ts).get()).unwrap_or(0);
        self.span(Ticks::new(short))
    }

    pub fn span(&self, ticks: Ticks) -> Frames {
        if self.is_sample_accurate() {
            return Frames(ticks.get());
        }
        let Some(time) = self
            .time_base
            .and_then(|time_base| time_base.calc_duration(ticks))
        else {
            return Frames::ZERO;
        };
        let nanos = u64::try_from(time.as_nanos()).unwrap_or(0);
        Frames::from_duration(Duration::from_nanos(nanos), self.rate)
    }

    pub fn elapsed(&self, frames: Frames) -> Time {
        Time::try_from_nanos_u128(frames.to_duration(self.rate).as_nanos()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native(rate: u32) -> Timeline {
        Timeline::new(
            TimeBase::try_from_recip(rate),
            Timestamp::ZERO,
            SampleRate::new(rate).expect("a supported rate"),
        )
    }

    fn matroska(rate: u32) -> Timeline {
        Timeline::new(
            TimeBase::try_new(1, 1_000),
            Timestamp::ZERO,
            SampleRate::new(rate).expect("a supported rate"),
        )
    }

    #[test]
    fn a_native_timebase_makes_ticks_and_frames_the_same_thing() {
        let timeline = native(44_100);

        assert!(timeline.is_sample_accurate());
        assert_eq!(timeline.span(Ticks::new(44_100)), Frames(44_100));
        assert_eq!(timeline.frames(Timestamp::new(1_234)), Frames(1_234));
    }

    #[test]
    fn millisecond_ticks_convert_through_the_sample_rate() {
        let timeline = matroska(48_000);

        assert!(!timeline.is_sample_accurate());
        assert_eq!(timeline.span(Ticks::new(1_000)), Frames(48_000));
        assert_eq!(timeline.span(Ticks::new(500)), Frames(24_000));
    }

    #[test]
    fn a_timestamp_is_only_offered_where_it_would_be_exact() {
        assert_eq!(
            native(44_100).timestamp(Frames(100)),
            Some(Timestamp::new(100))
        );
        assert_eq!(matroska(44_100).timestamp(Frames(100)), None);
    }

    #[test]
    fn the_music_the_priming_pushes_along_is_where_a_frame_is_counted_from() {
        let timeline = Timeline::new(
            TimeBase::try_from_recip(44_100),
            Timestamp::new(576),
            SampleRate::HZ_44100,
        );

        assert_eq!(timeline.timestamp(Frames(0)), Some(Timestamp::new(576)));
        assert_eq!(timeline.frames(Timestamp::new(576)), Frames(0));
        assert_eq!(timeline.frames(Timestamp::new(0)), Frames(0));
        assert_eq!(timeline.short_of_the_music(Timestamp::new(0)), Frames(576));
        assert_eq!(
            timeline.short_of_the_music(Timestamp::new(576)),
            Frames::ZERO
        );
        assert_eq!(
            timeline.short_of_the_music(Timestamp::new(1_000)),
            Frames::ZERO
        );
    }

    #[test]
    fn a_reader_that_primes_before_its_own_zero_counts_the_music_from_there() {
        let timeline = Timeline::new(
            TimeBase::try_from_recip(44_100),
            Timestamp::ZERO,
            SampleRate::HZ_44100,
        );

        assert_eq!(timeline.timestamp(Frames(0)), Some(Timestamp::ZERO));
        assert_eq!(timeline.frames(Timestamp::new(-1_105)), Frames(0));
        assert_eq!(timeline.frames(Timestamp::new(1_105)), Frames(1_105));
    }
}
