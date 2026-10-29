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
use resonate_audio::{DeviceTime, StreamClock};
use resonate_core::SampleRate;
use rustix::time::{ClockId, clock_gettime};

use super::{
    handover::Fed,
    oboe_frames::{Frame, Presented, heard_by_the_device, rendered},
};

const NANOS_A_SECOND: u64 = 1_000_000_000;

pub(crate) struct Rendering<S> {
    pub(crate) fed: Fed,
    pub(crate) empty: Arc<AtomicBool>,
    pub(crate) failed: Arc<AtomicBool>,
    pub(crate) bytes: Box<[u8]>,
    pub(crate) clock: Arc<StreamClock>,
    pub(crate) rate: SampleRate,
    pub(crate) shape: PhantomData<S>,
}

fn monotonic_now() -> u64 {
    let now = clock_gettime(ClockId::Monotonic);
    u64::try_from(now.tv_sec)
        .unwrap_or(0)
        .saturating_mul(NANOS_A_SECOND)
        .saturating_add(u64::try_from(now.tv_nsec).unwrap_or(0))
}

fn device_time_of(stream: &mut dyn AudioOutputStreamSafe, rate: SampleRate) -> Option<DeviceTime> {
    let written = stream.get_frames_written();
    let presented = stream.get_timestamp(ClockId::Monotonic as i32).ok()?;
    heard_by_the_device(
        written,
        Presented {
            frame: presented.position,
            at: presented.timestamp,
        },
        monotonic_now(),
        rate,
    )
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
        stream: &mut dyn AudioOutputStreamSafe,
        frames: &mut [<(T, C) as IsFrameType>::Type],
    ) -> DataCallbackResult {
        let stamp = self.clock.stamp_by(device_time_of(stream, self.rate));
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
