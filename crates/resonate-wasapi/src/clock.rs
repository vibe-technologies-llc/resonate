#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use std::num::NonZeroU64;

use resonate_audio::DeviceTime;
use resonate_core::{Frames, SampleRate};

const NANOS_A_SECOND: u128 = 1_000_000_000;
const NANOS_A_HUNDRED_NANOSECOND_TICK: u64 = 100;
const READING_TRUSTED_WITHIN_NANOS: u64 = 100_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockReading {
    pub position: u64,
    pub frequency: NonZeroU64,
    pub counter_in_hundred_nanoseconds: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CounterNow {
    pub ticks: u64,
    pub per_second: NonZeroU64,
}

impl CounterNow {
    fn nanos(self) -> u64 {
        let nanos = u128::from(self.ticks) * NANOS_A_SECOND / u128::from(self.per_second.get());
        u64::try_from(nanos).unwrap_or(u64::MAX)
    }
}

impl ClockReading {
    pub fn frames_played(self, rate: SampleRate) -> u64 {
        let frames =
            u128::from(self.position) * u128::from(rate.hz()) / u128::from(self.frequency.get());
        u64::try_from(frames).unwrap_or(u64::MAX)
    }

    pub fn frames_ahead(self, written: u64, rate: SampleRate) -> u64 {
        written.saturating_sub(self.frames_played(rate))
    }

    pub fn device_time(
        self,
        now: CounterNow,
        written: u64,
        rate: SampleRate,
    ) -> Option<DeviceTime> {
        if self.counter_in_hundred_nanoseconds == 0 {
            return None;
        }
        let clock_now = now.nanos();
        let read_at = self
            .counter_in_hundred_nanoseconds
            .saturating_mul(NANOS_A_HUNDRED_NANOSECOND_TICK);
        if clock_now.abs_diff(read_at) > READING_TRUSTED_WITHIN_NANOS {
            return None;
        }
        Some(DeviceTime {
            clock_now,
            read_at,
            ahead: Frames(self.frames_ahead(written, rate)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEN_MEGAHERTZ: NonZeroU64 = NonZeroU64::new(10_000_000).expect("a counter frequency");
    const BYTES_A_SECOND_OF_CD: NonZeroU64 = NonZeroU64::new(176_400).expect("a clock frequency");
    const FRAMES_A_SECOND_AT_48_KHZ: NonZeroU64 =
        NonZeroU64::new(48_000).expect("a clock frequency");

    fn counter_at_seconds(seconds: u64) -> CounterNow {
        CounterNow {
            ticks: seconds * TEN_MEGAHERTZ.get(),
            per_second: TEN_MEGAHERTZ,
        }
    }

    #[test]
    fn a_position_in_the_clocks_own_units_is_counted_in_stream_frames() {
        let half_a_second_of_bytes = ClockReading {
            position: 88_200,
            frequency: BYTES_A_SECOND_OF_CD,
            counter_in_hundred_nanoseconds: 1,
        };

        assert_eq!(
            half_a_second_of_bytes.frames_played(SampleRate::HZ_44100),
            22_050
        );
        assert_eq!(
            half_a_second_of_bytes.frames_ahead(22_050 + 441, SampleRate::HZ_44100),
            441
        );
        assert_eq!(
            half_a_second_of_bytes.frames_ahead(100, SampleRate::HZ_44100),
            0
        );
    }

    #[test]
    fn a_reading_lands_on_the_counter_in_nanoseconds_with_what_lies_ahead_of_the_fill() {
        let now = counter_at_seconds(7);
        let three_milliseconds_ago = ClockReading {
            position: 48_000,
            frequency: FRAMES_A_SECOND_AT_48_KHZ,
            counter_in_hundred_nanoseconds: 69_970_000,
        };

        let device = three_milliseconds_ago.device_time(now, 48_480, SampleRate::HZ_48000);

        assert_eq!(
            device,
            Some(DeviceTime {
                clock_now: 7_000_000_000,
                read_at: 6_997_000_000,
                ahead: Frames(480),
            })
        );
    }

    #[test]
    fn a_reading_with_no_counter_or_far_from_now_is_not_trusted() {
        let now = counter_at_seconds(7);
        let unstamped = ClockReading {
            position: 0,
            frequency: TEN_MEGAHERTZ,
            counter_in_hundred_nanoseconds: 0,
        };
        let a_second_old = ClockReading {
            counter_in_hundred_nanoseconds: 60_000_000,
            ..unstamped
        };

        assert_eq!(unstamped.device_time(now, 0, SampleRate::HZ_48000), None);
        assert_eq!(a_second_old.device_time(now, 0, SampleRate::HZ_48000), None);
    }
}
