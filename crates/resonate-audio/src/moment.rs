use std::{
    sync::LazyLock,
    time::{Duration, Instant},
};

use resonate_core::{Frames, SampleRate};

static EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

const NANOS_A_SECOND: f64 = 1_000_000_000.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Moment(u64);

impl Moment {
    pub fn start_the_clock() {
        LazyLock::force(&EPOCH);
    }

    pub fn now() -> Self {
        Self(u64::try_from(EPOCH.elapsed().as_nanos()).unwrap_or(u64::MAX))
    }

    pub const fn from_nanos(nanos: u64) -> Self {
        Self(nanos)
    }

    pub const fn nanos(self) -> u64 {
        self.0
    }

    pub fn after(self, by: Duration) -> Self {
        Self(
            self.0
                .saturating_add(u64::try_from(by.as_nanos()).unwrap_or(u64::MAX)),
        )
    }

    pub fn after_frames(self, frames: f64, rate: SampleRate) -> Self {
        let nanos = frames.max(0.0) * NANOS_A_SECOND / f64::from(rate.hz());
        Self(self.0.saturating_add(nanos as u64))
    }

    pub fn frames_since(self, earlier: Self, rate: SampleRate) -> f64 {
        let nanos = self.0 as f64 - earlier.0 as f64;
        nanos * f64::from(rate.hz()) / NANOS_A_SECOND
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Stamp {
    pub at: Moment,
    pub ahead: Frames,
}

impl Stamp {
    pub fn first_heard(self, rate: SampleRate) -> Moment {
        self.at.after_frames(self.ahead.get() as f64, rate)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DeviceTime {
    pub clock_now: u64,
    pub read_at: u64,
    pub ahead: Frames,
}

impl DeviceTime {
    pub fn stamp(self) -> Stamp {
        Stamp {
            at: self.moment_of_the_reading(Moment::now()),
            ahead: self.ahead,
        }
    }

    const fn moment_of_the_reading(self, now: Moment) -> Moment {
        if self.read_at >= self.clock_now {
            Moment(now.0.saturating_add(self.read_at - self.clock_now))
        } else {
            Moment(now.0.saturating_sub(self.clock_now - self.read_at))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moment_a_second_of_frames_on_is_a_second_later() {
        let start = Moment::from_nanos(5);
        let later = start.after_frames(48_000.0, SampleRate::HZ_48000);

        assert_eq!(later, start.after(Duration::from_secs(1)));
        assert!((later.frames_since(start, SampleRate::HZ_48000) - 48_000.0).abs() < 1e-6);
        assert!((start.frames_since(later, SampleRate::HZ_48000) + 48_000.0).abs() < 1e-6);
    }

    #[test]
    fn the_first_frame_of_a_fill_is_heard_once_what_is_ahead_of_it_has_been() {
        let stamp = Stamp {
            at: Moment::from_nanos(1_000),
            ahead: Frames(441),
        };

        assert_eq!(
            stamp.first_heard(SampleRate::HZ_44100),
            Moment::from_nanos(1_000).after(Duration::from_millis(10))
        );
    }

    #[test]
    fn a_device_reading_lands_as_far_from_now_as_it_was_on_the_devices_own_clock() {
        let now = Moment::from_nanos(10_000_000);
        let past = DeviceTime {
            clock_now: 7_000_000_000,
            read_at: 6_997_000_000,
            ahead: Frames(64),
        };
        let future = DeviceTime {
            read_at: 7_000_500_000,
            ..past
        };
        let before_the_epoch = DeviceTime { read_at: 0, ..past };

        assert_eq!(
            past.moment_of_the_reading(now),
            Moment::from_nanos(7_000_000)
        );
        assert_eq!(
            future.moment_of_the_reading(now),
            Moment::from_nanos(10_500_000)
        );
        assert_eq!(
            before_the_epoch.moment_of_the_reading(now),
            Moment::from_nanos(0)
        );
    }

    #[test]
    fn a_stamp_from_a_device_reading_keeps_what_lies_ahead_of_the_fill() {
        let reading = DeviceTime {
            clock_now: 5_000,
            read_at: 5_000,
            ahead: Frames(441),
        };
        let before = Moment::now();

        let stamp = reading.stamp();

        assert_eq!(stamp.ahead, Frames(441));
        assert!(stamp.at >= before && stamp.at <= Moment::now());
    }

    #[test]
    fn moments_go_forward() {
        let first = Moment::now();
        let second = Moment::now();

        assert!(second >= first);
    }
}
