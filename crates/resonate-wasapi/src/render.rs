#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use std::{
    num::{NonZeroU32, NonZeroUsize},
    sync::Arc,
};

use resonate_audio::{GraphTime, StreamClock, handover::Fed};
use resonate_core::{Ratio, SampleFormat, SampleRate};
use wasapi::{AudioClient, AudioRenderClient, Handle, ShareMode, WasapiError};

use crate::fill::fill_buffer;

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
        let ahead = match self.way {
            ShareMode::Shared => self.buffer_frames.saturating_sub(available),
            ShareMode::Exclusive => self.buffer_frames,
        };
        self.clock.note(
            GraphTime {
                delay: i64::from(ahead),
                buffered: 0,
                queued: 0,
                tick: Ratio {
                    numer: NonZeroU32::MIN,
                    denom: NonZeroU32::new(self.rate.hz()).unwrap_or(NonZeroU32::MIN),
                },
            },
            self.rate,
        );
        let played = self.written.saturating_sub(u64::from(ahead));
        let wanted = usize::try_from(available).unwrap_or_default();
        if wanted == 0 {
            return Ok(Cycled { heard: 0, played });
        }

        let filled = fill_buffer(
            self.fed.source(),
            &mut self.scratch,
            self.frame_bytes,
            wanted,
            self.format,
            self.clock.stamp(),
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
}
