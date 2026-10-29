#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use std::{
    marker::PhantomData,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use oboe::{
    AudioOutputCallback, AudioOutputStreamSafe, DataCallbackResult, IsChannelCount, IsFormat,
    IsFrameType,
};
use resonate_audio::StreamClock;

use super::{
    handover::Fed,
    oboe_frames::{Frame, rendered},
};

pub(crate) struct Rendering<S> {
    pub(crate) fed: Fed,
    pub(crate) empty: Arc<AtomicBool>,
    pub(crate) failed: Arc<AtomicBool>,
    pub(crate) bytes: Box<[u8]>,
    pub(crate) clock: Arc<StreamClock>,
    pub(crate) shape: PhantomData<S>,
}

impl<T, C> AudioOutputCallback for Rendering<(T, C)>
where
    T: IsFormat,
    C: IsChannelCount,
    (T, C): IsFrameType,
    <(T, C) as IsFrameType>::Type: Frame,
{
    type FrameType = (T, C);

    fn on_audio_ready(
        &mut self,
        _: &mut dyn AudioOutputStreamSafe,
        frames: &mut [<(T, C) as IsFrameType>::Type],
    ) -> DataCallbackResult {
        let stamp = self.clock.stamp();
        if rendered(
            self.fed.source(),
            &mut self.bytes,
            &self.empty,
            frames,
            stamp,
        ) {
            DataCallbackResult::Continue
        } else {
            DataCallbackResult::Stop
        }
    }

    fn on_error_after_close(&mut self, _: &mut dyn AudioOutputStreamSafe, _: oboe::Error) {
        self.failed.store(true, Ordering::Release);
    }
}
