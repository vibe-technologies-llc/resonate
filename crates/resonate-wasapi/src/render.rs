#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use std::{
    num::{NonZeroU32, NonZeroU64, NonZeroUsize},
    sync::Arc,
};

use resonate_audio::{GraphTime, StreamClock, handover::Fed};
use resonate_core::{Ratio, SampleFormat, SampleRate};
use resonate_win32::{PerformanceCounter, performance_counter};
use wasapi::{AudioClient, AudioClock, AudioRenderClient, Handle, ShareMode, WasapiError};

use crate::{
    clock::{ClockReading, CounterNow},
    fill::fill_buffer,
    priority::Priority,
    serving::{Cycled, Hardware, Woken},
};

const EVENT_WAIT_MS: u32 = 100;

pub(crate) struct Playing {
    pub(crate) client: AudioClient,
    pub(crate) render: AudioRenderClient,
    pub(crate) event: Handle,
    pub(crate) fed: Fed,
    pub(crate) scratch: Vec<u8>,
    pub(crate) buffer_frames: u32,
    pub(crate) way: ShareMode,
    pub(crate) format: SampleFormat,
    pub(crate) rate: SampleRate,
    pub(crate) frame_bytes: NonZeroUsize,
    pub(crate) clock: Arc<StreamClock>,
    pub(crate) audio_clock: Option<(AudioClock, NonZeroU64)>,
    pub(crate) counter: Option<PerformanceCounter>,
    pub(crate) written: u64,
    pub(crate) real: u64,
    pub(crate) priority: Priority,
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
        let available = self.client.get_available_space_in_frames()?;
        let reading = self.read_the_clock();
        let counter = self.read_the_counter();
        let ahead = match reading {
            Some(reading) => reading.frames_ahead(self.written, self.rate),
            None => u64::from(self.padding(available)),
        };
        self.clock.note(
            GraphTime {
                delay: i64::try_from(ahead).unwrap_or(i64::MAX),
                buffered: 0,
                queued: 0,
                tick: Ratio {
                    numer: NonZeroU32::MIN,
                    denom: NonZeroU32::new(self.rate.hz()).unwrap_or(NonZeroU32::MIN),
                },
            },
            self.rate,
        );
        let played = self.written.saturating_sub(ahead);
        let wanted = usize::try_from(available).unwrap_or_default();
        if wanted == 0 {
            return Ok(self.cycled(0, played));
        }

        let device = reading
            .zip(counter)
            .and_then(|(reading, counter)| reading.device_time(counter, self.written, self.rate));
        let filled = fill_buffer(
            self.fed.source(),
            &mut self.scratch,
            self.frame_bytes,
            wanted,
            self.format,
            self.clock.stamp_by(device),
        );
        let bytes = filled.frames.saturating_mul(self.frame_bytes.get());
        let Some(window) = self.scratch.get(..bytes) else {
            return Ok(self.cycled(0, played));
        };
        self.render.write_to_device(window, None)?;

        if filled.heard > 0 {
            self.real = self.written.saturating_add(filled.heard as u64);
        }
        self.written = self.written.saturating_add(filled.frames as u64);
        Ok(self.cycled(filled.heard, played))
    }

    fn start(&mut self) -> Result<(), WasapiError> {
        self.client.start_stream()
    }

    fn pause(&mut self) -> Result<(), WasapiError> {
        self.client.stop_stream()?;
        self.client.reset_stream()?;
        self.written = 0;
        self.real = 0;
        Ok(())
    }

    fn stop(&mut self) {
        if let Err(error) = self.client.stop_stream() {
            tracing::debug!(%error, "a WASAPI stream would not stop as it was let go");
        }
    }

    fn prioritise(&mut self, realtime: bool) {
        self.priority.set(realtime);
    }
}

impl Playing {
    const fn cycled(&self, heard: usize, played: u64) -> Cycled {
        Cycled {
            heard,
            played,
            heard_until: self.real,
        }
    }

    const fn padding(&self, available: u32) -> u32 {
        match self.way {
            ShareMode::Shared => self.buffer_frames.saturating_sub(available),
            ShareMode::Exclusive => self.buffer_frames,
        }
    }

    fn read_the_clock(&self) -> Option<ClockReading> {
        let (audio_clock, frequency) = self.audio_clock.as_ref()?;
        let (position, counter_in_hundred_nanoseconds) = audio_clock.get_position().ok()?;
        Some(ClockReading {
            position,
            frequency: *frequency,
            counter_in_hundred_nanoseconds,
        })
    }

    fn read_the_counter(&self) -> Option<CounterNow> {
        Some(CounterNow {
            per_second: self.counter?.per_second,
            ticks: performance_counter().ok()?,
        })
    }
}
