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
};

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
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Cycled {
    pub(crate) heard: usize,
    pub(crate) played: u64,
}

impl Cycled {
    pub(crate) const fn has_played_out(self, real: u64) -> bool {
        self.heard == 0 && self.played >= real
    }
}

impl Playing {
    pub(crate) fn cycle(&mut self) -> Result<Cycled, WasapiError> {
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
            return Ok(Cycled { heard: 0, played });
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
            return Ok(Cycled { heard: 0, played });
        };
        self.render.write_to_device(filled.frames, window, None)?;

        if filled.heard > 0 {
            self.real = self.written.saturating_add(filled.heard as u64);
        }
        self.written = self.written.saturating_add(filled.frames as u64);
        Ok(Cycled {
            heard: filled.heard,
            played,
        })
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
