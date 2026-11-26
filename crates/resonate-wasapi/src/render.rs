#![deny(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unreachable,
    clippy::unwrap_used
)]

use std::{
    num::{NonZeroU32, NonZeroUsize},
    sync::Arc,
};

use resonate_audio::{GraphTime, Moment, Stamp, StreamClock, handover::Fed};
use resonate_core::{Frames, Ratio, SampleFormat, SampleRate};

use crate::{
    clock::{ClockReading, CounterNow},
    fill::fill_buffer,
    serving::Cycled,
    ways::Way,
};

pub trait Buffer {
    type Fault;

    fn room(&mut self) -> Result<u32, Self::Fault>;
    fn position(&self) -> Option<ClockReading>;
    fn counter(&self) -> Option<CounterNow>;
    fn write(&mut self, frames: &[u8]) -> Result<(), Self::Fault>;
}

pub struct Cycle {
    pub fed: Fed,
    pub scratch: Vec<u8>,
    pub buffer_frames: u32,
    pub way: Way,
    pub format: SampleFormat,
    pub rate: SampleRate,
    pub frame_bytes: NonZeroUsize,
    pub clock: Arc<StreamClock>,
    pub written: u64,
    pub real: u64,
}

impl Cycle {
    pub fn turn<B: Buffer>(&mut self, buffer: &mut B) -> Result<Cycled, B::Fault> {
        let available = buffer.room()?;
        let reading = buffer.position();
        let counter = buffer.counter();
        let ahead = match reading {
            Some(reading) => reading.frames_ahead(self.written, self.rate),
            None => u64::from(self.padding(available)),
        };
        let played = self.written.saturating_sub(ahead);
        let wanted = usize::try_from(available).unwrap_or_default();

        if wanted == 0 {
            self.note(ahead);
            return Ok(self.cycled(0, played));
        }

        let device = reading
            .zip(counter)
            .and_then(|(reading, counter)| reading.device_time(counter, self.written, self.rate));
        let stamp = device.map_or_else(
            || Stamp {
                at: Moment::now(),
                ahead: Frames(ahead),
            },
            |device| device.stamp(),
        );
        let filled = fill_buffer(
            self.fed.source(),
            &mut self.scratch,
            self.frame_bytes,
            wanted,
            self.format,
            stamp,
        );
        let bytes = filled.frames.saturating_mul(self.frame_bytes.get());
        let Some(window) = self.scratch.get(..bytes) else {
            self.note(ahead);
            return Ok(self.cycled(0, played));
        };

        buffer.write(window)?;

        let wrote = filled.frames as u64;

        if filled.heard > 0 {
            self.real = self.written.saturating_add(filled.heard as u64);
        }
        self.written = self.written.saturating_add(wrote);
        self.note(ahead.saturating_add(wrote));
        Ok(self.cycled(filled.heard, played))
    }

    pub const fn restart(&mut self) {
        self.written = 0;
        self.real = 0;
    }

    fn note(&self, queued: u64) {
        self.clock.note(
            GraphTime {
                delay: i64::try_from(queued).unwrap_or(i64::MAX),
                buffered: 0,
                queued: 0,
                tick: Ratio {
                    numer: NonZeroU32::MIN,
                    denom: NonZeroU32::new(self.rate.hz()).unwrap_or(NonZeroU32::MIN),
                },
            },
            self.rate,
        );
    }

    const fn cycled(&self, heard: usize, played: u64) -> Cycled {
        Cycled {
            heard,
            played,
            heard_until: self.real,
        }
    }

    const fn padding(&self, available: u32) -> u32 {
        match self.way {
            Way::Shared => self.buffer_frames.saturating_sub(available),
            Way::Exclusive => self.buffer_frames,
        }
    }
}

#[cfg(windows)]
pub(crate) use self::real::{Client, Playing};

#[cfg(windows)]
mod real {
    use std::num::NonZeroU64;

    use resonate_win32::{PerformanceCounter, performance_counter};
    use wasapi::{AudioClient, AudioClock, AudioRenderClient, Handle, WasapiError};

    use super::{Buffer, Cycle};
    use crate::{
        clock::{ClockReading, CounterNow},
        priority::Priority,
        serving::{Cycled, Hardware, Woken},
    };

    const EVENT_WAIT_MS: u32 = 100;

    pub(crate) struct Client {
        pub(crate) client: AudioClient,
        pub(crate) render: AudioRenderClient,
        pub(crate) audio_clock: Option<(AudioClock, NonZeroU64)>,
        pub(crate) counter: Option<PerformanceCounter>,
    }

    pub(crate) struct Playing {
        pub(crate) client: Client,
        pub(crate) cycle: Cycle,
        pub(crate) event: Handle,
        pub(crate) priority: Priority,
    }

    impl Buffer for Client {
        type Fault = WasapiError;

        fn room(&mut self) -> Result<u32, WasapiError> {
            self.client.get_available_space_in_frames()
        }

        fn position(&self) -> Option<ClockReading> {
            let (audio_clock, frequency) = self.audio_clock.as_ref()?;
            let (position, counter_in_hundred_nanoseconds) = audio_clock.get_position().ok()?;
            Some(ClockReading {
                position,
                frequency: *frequency,
                counter_in_hundred_nanoseconds,
            })
        }

        fn counter(&self) -> Option<CounterNow> {
            Some(CounterNow {
                per_second: self.counter?.per_second,
                ticks: performance_counter().ok()?,
            })
        }

        fn write(&mut self, frames: &[u8]) -> Result<(), WasapiError> {
            self.render.write_to_device(frames, None)
        }
    }

    impl Hardware for Playing {
        type Fault = WasapiError;

        fn wait_for_room(&mut self) -> Woken {
            match self.event.wait_for_event(EVENT_WAIT_MS) {
                Ok(()) => Woken::Asked,
                Err(_) => Woken::TimedOut,
            }
        }

        fn cycle(&mut self) -> Result<Cycled, WasapiError> {
            self.cycle.turn(&mut self.client)
        }

        fn probe(&mut self) -> Result<(), WasapiError> {
            self.client.client.get_current_padding().map(|_| ())
        }

        fn start(&mut self) -> Result<(), WasapiError> {
            self.client.client.start_stream()
        }

        fn pause(&mut self) -> Result<(), WasapiError> {
            self.client.client.stop_stream()?;
            self.client.client.reset_stream()?;
            self.cycle.restart();
            Ok(())
        }

        fn stop(&mut self) {
            if let Err(error) = self.client.client.stop_stream() {
                tracing::debug!(%error, "a WASAPI stream would not stop as it was let go");
            }
        }

        fn prioritise(&mut self, realtime: bool) {
            self.priority.set(realtime);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, num::NonZeroU64};

    use resonate_audio::{AudioSource, handover::fed_from};

    use super::*;

    const FRAME_BYTES: NonZeroUsize = NonZeroUsize::new(8).expect("a stereo float frame");
    const BUFFER_FRAMES: u32 = 480;
    const A_PERIOD: u64 = 160;
    const CLOCKED_IN_FRAMES: NonZeroU64 = NonZeroU64::new(48_000).expect("a frequency");

    struct Device {
        queued: u64,
        played: u64,
        with_a_clock: bool,
    }

    impl Device {
        fn holding(queued: u64, with_a_clock: bool) -> Self {
            Self {
                queued,
                played: 0,
                with_a_clock,
            }
        }

        fn plays(&mut self, frames: u64) {
            let played = frames.min(self.queued);

            self.queued -= played;
            self.played += played;
        }
    }

    impl Buffer for Device {
        type Fault = Infallible;

        fn room(&mut self) -> Result<u32, Infallible> {
            Ok(BUFFER_FRAMES.saturating_sub(u32::try_from(self.queued).unwrap_or(BUFFER_FRAMES)))
        }

        fn position(&self) -> Option<ClockReading> {
            self.with_a_clock.then_some(ClockReading {
                position: self.played,
                frequency: CLOCKED_IN_FRAMES,
                counter_in_hundred_nanoseconds: 0,
            })
        }

        fn counter(&self) -> Option<CounterNow> {
            None
        }

        fn write(&mut self, frames: &[u8]) -> Result<(), Infallible> {
            self.queued += (frames.len() / FRAME_BYTES) as u64;
            Ok(())
        }
    }

    struct Stamped(Arc<parking_lot::Mutex<Vec<Frames>>>);

    impl AudioSource for Stamped {
        fn fill(&mut self, dst: &mut [u8]) -> usize {
            dst.len()
        }

        fn fill_stamped(&mut self, dst: &mut [u8], stamp: Stamp) -> usize {
            self.0.lock().push(stamp.ahead);
            dst.len()
        }
    }

    fn cycle(stamps: &Arc<parking_lot::Mutex<Vec<Frames>>>, already_written: u64) -> Cycle {
        let (fed, _handing) = fed_from(Box::new(Stamped(Arc::clone(stamps))));
        Cycle {
            fed,
            scratch: vec![0; BUFFER_FRAMES as usize * FRAME_BYTES.get()],
            buffer_frames: BUFFER_FRAMES,
            way: Way::Shared,
            format: SampleFormat::F32,
            rate: SampleRate::HZ_48000,
            frame_bytes: FRAME_BYTES,
            clock: Arc::new(StreamClock::behind_by(Frames::ZERO)),
            written: already_written,
            real: 0,
        }
    }

    fn the_latency_noted_is_the_queue_after_each_write(with_a_clock: bool) {
        let stamps = Arc::default();
        let mut device = Device::holding(0, with_a_clock);
        let mut cycle = cycle(&stamps, 0);

        let mut queues = Vec::new();
        let mut latencies = Vec::new();

        for _ in 0..4 {
            let _ = cycle.turn(&mut device);

            queues.push(device.queued);
            latencies.push(cycle.clock.latency().get());

            device.plays(A_PERIOD);
        }

        assert_eq!(latencies, queues);
        assert!(
            queues
                .iter()
                .all(|queued| *queued == u64::from(BUFFER_FRAMES))
        );
        assert_eq!(
            *stamps.lock(),
            [
                Frames::ZERO,
                Frames(u64::from(BUFFER_FRAMES) - A_PERIOD),
                Frames(u64::from(BUFFER_FRAMES) - A_PERIOD),
                Frames(u64::from(BUFFER_FRAMES) - A_PERIOD),
            ]
        );
    }

    #[test]
    fn the_latency_a_cycle_notes_counts_the_frames_it_wrote_by_the_devices_clock() {
        the_latency_noted_is_the_queue_after_each_write(true);
    }

    #[test]
    fn the_latency_a_cycle_notes_counts_the_frames_it_wrote_by_the_padding() {
        the_latency_noted_is_the_queue_after_each_write(false);
    }

    #[test]
    fn a_cycle_with_no_room_notes_the_queue_as_it_stands() {
        let stamps = Arc::default();
        let mut device = Device::holding(u64::from(BUFFER_FRAMES), true);
        let mut cycle = cycle(&stamps, u64::from(BUFFER_FRAMES));

        let cycled = cycle.turn(&mut device);

        assert!(matches!(cycled, Ok(Cycled { heard: 0, .. })));
        assert_eq!(cycle.clock.latency().get(), u64::from(BUFFER_FRAMES));
        assert!(stamps.lock().is_empty());
    }
}
