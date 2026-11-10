use std::num::NonZeroU32;

use resonate_audio::GraphTime;
use resonate_core::{Ratio, SampleRate};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Latencies {
    pub device: u32,
    pub safety_offset: u32,
    pub stream: u32,
    pub buffer: u32,
}

impl Latencies {
    pub const fn behind_the_write(self) -> u64 {
        self.device as u64 + self.safety_offset as u64 + self.stream as u64 + self.buffer as u64
    }

    pub const fn past_the_host_time(self) -> u64 {
        self.device as u64 + self.stream as u64
    }

    pub fn heard_past_the_host_time(self, device_hz: u32, stream: SampleRate) -> u64 {
        let device = NonZeroU32::new(device_hz).unwrap_or(NonZeroU32::MIN);
        self.past_the_host_time()
            .saturating_mul(u64::from(stream.hz()))
            / u64::from(device.get())
    }

    pub fn graph_time(self, device_hz: u32) -> GraphTime {
        GraphTime {
            delay: i64::try_from(self.behind_the_write()).unwrap_or(i64::MAX),
            buffered: 0,
            queued: 0,
            tick: Ratio {
                numer: NonZeroU32::MIN,
                denom: NonZeroU32::new(device_hz).unwrap_or(NonZeroU32::MIN),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use resonate_audio::StreamClock;
    use resonate_core::Frames;

    use super::*;

    const HEARD: Latencies = Latencies {
        device: 120,
        safety_offset: 64,
        stream: 30,
        buffer: 512,
    };

    #[test]
    fn a_write_is_heard_behind_everything_between_it_and_the_ear() {
        assert_eq!(HEARD.behind_the_write(), 726);
    }

    #[test]
    fn the_host_time_already_counts_the_buffer_and_the_safety_offset() {
        assert_eq!(HEARD.past_the_host_time(), 150);
    }

    #[test]
    fn what_lies_past_the_host_time_is_counted_in_the_streams_own_frames() {
        assert_eq!(
            HEARD.heard_past_the_host_time(96_000, SampleRate::HZ_48000),
            75
        );
        assert_eq!(
            HEARD.heard_past_the_host_time(0, SampleRate::HZ_48000),
            7_200_000
        );
    }

    #[test]
    fn the_clock_reads_the_latency_and_the_rate_the_device_runs_at() {
        let clock = StreamClock::behind_by(Frames::ZERO);

        clock.note(HEARD.graph_time(96_000), SampleRate::HZ_48000);

        assert_eq!(clock.latency(), Frames(363));
        assert_eq!(clock.graph_rate(), Some(SampleRate::HZ_96000));
    }
}
