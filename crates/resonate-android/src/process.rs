#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use oboe::{AudioOutputCallback, AudioOutputStreamSafe, DataCallbackResult, Stereo};
use resonate_audio::AudioSource;

const FRAME_CAPACITY: usize = 4096;
pub(crate) const RENDER_BYTES: usize = FRAME_CAPACITY * 8;

pub(crate) struct Rendering {
    pub(crate) source: Box<dyn AudioSource>,
    pub(crate) empty: Arc<AtomicBool>,
    pub(crate) failed: Arc<AtomicBool>,
    pub(crate) bytes: Box<[u8]>,
}

impl AudioOutputCallback for Rendering {
    type FrameType = (f32, Stereo);

    fn on_audio_ready(
        &mut self,
        _: &mut dyn AudioOutputStreamSafe,
        frames: &mut [(f32, f32)],
    ) -> DataCallbackResult {
        for chunk in frames.chunks_mut(FRAME_CAPACITY) {
            let Some(bytes) = self.bytes.get_mut(..chunk.len().saturating_mul(8)) else {
                return DataCallbackResult::Stop;
            };
            let filled = self.source.fill(bytes);
            self.empty.store(filled == 0, Ordering::Release);
            if let Some(unfilled) = bytes.get_mut(filled..) {
                unfilled.fill(0);
            }
            for (frame, encoded) in chunk.iter_mut().zip(bytes.as_chunks::<8>().0) {
                let Some((left, right)) = encoded.split_at_checked(4) else {
                    return DataCallbackResult::Stop;
                };
                let (Ok(left), Ok(right)) = (left.try_into(), right.try_into()) else {
                    return DataCallbackResult::Stop;
                };
                *frame = (f32::from_le_bytes(left), f32::from_le_bytes(right));
            }
        }
        DataCallbackResult::Continue
    }

    fn on_error_after_close(&mut self, _: &mut dyn AudioOutputStreamSafe, _: oboe::Error) {
        self.failed.store(true, Ordering::Release);
    }
}
