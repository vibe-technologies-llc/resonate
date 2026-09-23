use std::{fmt, time::Duration};

use crate::SampleRate;

const NANOS_PER_SECOND: u128 = 1_000_000_000;
const HALF_A_SECOND_IN_NANOS: u128 = NANOS_PER_SECOND / 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Frames(pub u64);

impl Frames {
    pub const ZERO: Self = Self(0);

    pub const fn get(self) -> u64 {
        self.0
    }

    pub const fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    pub const fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }

    pub const fn checked_sub(self, other: Self) -> Option<Self> {
        match self.0.checked_sub(other.0) {
            Some(frames) => Some(Self(frames)),
            None => None,
        }
    }

    pub const fn to_duration(self, rate: SampleRate) -> Duration {
        let hz = rate.hz() as u64;
        let seconds = self.0 / hz;
        let remainder = self.0 % hz;
        Duration::new(seconds, (remainder * 1_000_000_000 / hz) as u32)
    }

    pub fn from_duration(duration: Duration, rate: SampleRate) -> Self {
        let hz = u128::from(rate.hz());
        let nanos = duration.as_nanos();
        let rounded = (nanos * hz + HALF_A_SECOND_IN_NANOS) / NANOS_PER_SECOND;
        Self(u64::try_from(rounded).unwrap_or(u64::MAX))
    }
}

impl fmt::Display for Frames {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrameSpan {
    start: Frames,
    frames: Option<Frames>,
}

impl FrameSpan {
    pub const fn starting(start: Frames) -> Self {
        Self {
            start,
            frames: None,
        }
    }

    pub const fn between(one: Frames, other: Frames) -> Self {
        let (start, end) = if one.0 <= other.0 {
            (one, other)
        } else {
            (other, one)
        };
        Self {
            start,
            frames: Some(Frames(end.0 - start.0)),
        }
    }

    pub const fn start(self) -> Frames {
        self.start
    }

    pub const fn frames(self) -> Option<Frames> {
        self.frames
    }

    pub const fn end(self) -> Option<Frames> {
        match self.frames {
            Some(frames) => Some(Frames(self.start.0 + frames.0)),
            None => None,
        }
    }

    pub const fn within(self, whole: Frames) -> Self {
        let start = if self.start.0 > whole.0 {
            whole
        } else {
            self.start
        };
        let rest = Frames(whole.0 - start.0);
        let frames = match self.frames {
            Some(frames) if frames.0 <= rest.0 => frames,
            _ => rest,
        };
        Self {
            start,
            frames: Some(frames),
        }
    }
}

impl fmt::Display for FrameSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.end() {
            Some(end) => write!(f, "{}..{}", self.start, end),
            None => write!(f, "{}..", self.start),
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn a_sample_rate() -> impl Strategy<Value = SampleRate> {
        (SampleRate::MIN_HZ..=SampleRate::MAX_HZ)
            .prop_map(|hz| SampleRate::new(hz).expect("a rate between the bounds is a rate"))
    }

    fn a_count_of_frames() -> impl Strategy<Value = Frames> {
        prop_oneof![
            Just(Frames::ZERO),
            Just(Frames(u64::MAX)),
            (0u64..1 << 32).prop_map(Frames),
            any::<u64>().prop_map(Frames),
        ]
    }

    fn a_duration() -> impl Strategy<Value = Duration> {
        prop_oneof![
            Just(Duration::ZERO),
            Just(Duration::MAX),
            any::<u64>().prop_map(Duration::from_nanos),
            (any::<u64>(), 0u32..1_000_000_000)
                .prop_map(|(seconds, nanos)| Duration::new(seconds, nanos)),
        ]
    }

    #[test]
    fn frames_round_trip_through_duration_at_cd_rate() {
        let frames = Frames(44_100 * 3);
        let duration = frames.to_duration(SampleRate::HZ_44100);
        assert_eq!(duration, Duration::from_secs(3));
        assert_eq!(
            Frames::from_duration(duration, SampleRate::HZ_44100),
            frames
        );
    }

    #[test]
    fn a_duration_longer_than_any_count_of_frames_saturates_rather_than_wrapping() {
        assert_eq!(
            Frames::from_duration(Duration::MAX, SampleRate::HZ_192000),
            Frames(u64::MAX)
        );
    }

    #[test]
    fn a_span_between_two_frames_reads_the_same_either_way_round() {
        let forwards = FrameSpan::between(Frames(100), Frames(400));
        let backwards = FrameSpan::between(Frames(400), Frames(100));

        assert_eq!(forwards, backwards);
        assert_eq!(forwards.start(), Frames(100));
        assert_eq!(forwards.frames(), Some(Frames(300)));
        assert_eq!(forwards.end(), Some(Frames(400)));
    }

    #[test]
    fn a_span_with_no_end_runs_to_whatever_the_media_holds() {
        let open = FrameSpan::starting(Frames(50));

        assert_eq!(open.frames(), None);
        assert_eq!(open.end(), None);
        assert_eq!(open.within(Frames(500)).frames(), Some(Frames(450)));
    }

    #[test]
    fn a_span_reaching_past_the_media_is_cut_back_to_it() {
        let over = FrameSpan::between(Frames(100), Frames(900));

        assert_eq!(over.within(Frames(500)).frames(), Some(Frames(400)));
        assert_eq!(over.within(Frames(500)).end(), Some(Frames(500)));
    }

    #[test]
    fn a_span_starting_past_the_media_holds_nothing() {
        let past = FrameSpan::between(Frames(800), Frames(900)).within(Frames(500));

        assert_eq!(past.start(), Frames(500));
        assert_eq!(past.frames(), Some(Frames::ZERO));
    }

    #[test]
    fn sub_second_positions_keep_nanosecond_precision() {
        let one = Frames(1).to_duration(SampleRate::HZ_44100);
        assert_eq!(one.as_nanos(), 22_675);
    }

    proptest! {
        #[test]
        fn no_frames_are_no_time_and_no_time_is_no_frames(rate in a_sample_rate()) {
            prop_assert_eq!(Frames::ZERO.to_duration(rate), Duration::ZERO);
            prop_assert_eq!(Frames::from_duration(Duration::ZERO, rate), Frames::ZERO);
        }

        #[test]
        fn more_frames_is_never_less_time(
            rate in a_sample_rate(),
            one in a_count_of_frames(),
            other in a_count_of_frames(),
            step in 1u64..4_096,
        ) {
            let beside = one.saturating_add(Frames(step));

            prop_assert_eq!(
                one.to_duration(rate).cmp(&other.to_duration(rate)),
                one.cmp(&other)
            );
            prop_assert_eq!(
                one.to_duration(rate).cmp(&beside.to_duration(rate)),
                one.cmp(&beside)
            );
        }

        #[test]
        fn a_longer_duration_is_never_fewer_frames(
            rate in a_sample_rate(),
            one in a_duration(),
            other in a_duration(),
        ) {
            let (shorter, longer) = if one <= other { (one, other) } else { (other, one) };

            prop_assert!(
                Frames::from_duration(shorter, rate) <= Frames::from_duration(longer, rate)
            );
        }

        #[test]
        fn no_duration_asks_for_more_frames_than_one_count_can_hold(rate in a_sample_rate()) {
            prop_assert_eq!(Frames::from_duration(Duration::MAX, rate), Frames(u64::MAX));
            prop_assert!(Frames(u64::MAX).to_duration(rate) < Duration::MAX);
        }

        #[test]
        fn a_count_of_frames_comes_back_from_its_duration_exactly(
            rate in a_sample_rate(),
            frames in a_count_of_frames(),
        ) {
            let back = Frames::from_duration(frames.to_duration(rate), rate);

            prop_assert_eq!(back, frames);
        }

        #[test]
        fn a_whole_second_of_frames_comes_back_from_its_duration_exactly(
            rate in a_sample_rate(),
            seconds in 0u64..1_000_000,
        ) {
            let frames = Frames(seconds * u64::from(rate.hz()));

            prop_assert_eq!(frames.to_duration(rate), Duration::from_secs(seconds));
            prop_assert_eq!(Frames::from_duration(frames.to_duration(rate), rate), frames);
        }
    }
}
