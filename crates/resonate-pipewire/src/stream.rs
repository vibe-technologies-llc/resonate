use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use crossbeam_channel::Receiver;
use resonate_core::{Frames, Ratio, SampleRate, StreamSpec};

use crate::{Result, SinkId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GraphTime {
    pub delay: i64,
    pub buffered: u64,
    pub tick: Ratio,
}

impl GraphTime {
    pub const fn downstream(self, stream: SampleRate) -> Frames {
        Frames(
            self.ahead_of_the_device(stream)
                .saturating_add(self.buffered),
        )
    }

    const fn ahead_of_the_device(self, stream: SampleRate) -> u64 {
        if self.delay <= 0 {
            return 0;
        }
        let seconds = (self.delay as u64).saturating_mul(self.tick.numer.get() as u64);
        seconds.saturating_mul(stream.hz() as u64) / self.tick.denom.get() as u64
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StreamState {
    Unconnected,
    Connecting,
    Paused,
    Streaming,
    Failed,
}

impl From<&pipewire::stream::StreamState> for StreamState {
    fn from(state: &pipewire::stream::StreamState) -> Self {
        match state {
            pipewire::stream::StreamState::Error(_) => Self::Failed,
            pipewire::stream::StreamState::Unconnected => Self::Unconnected,
            pipewire::stream::StreamState::Connecting => Self::Connecting,
            pipewire::stream::StreamState::Paused => Self::Paused,
            pipewire::stream::StreamState::Streaming => Self::Streaming,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LatencyRequest {
    Auto,
    Frames(u32),
    Duration(Duration),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MediaRole {
    Music,
    Notification,
}

#[derive(Clone, Debug)]
pub struct StreamRequest {
    pub target: Option<SinkId>,
    pub spec: StreamSpec,
    pub latency: LatencyRequest,
    pub role: MediaRole,
    pub media_name: String,
    pub force_graph_rate: bool,
    pub no_convert: bool,
    pub exclusive: bool,
    pub realtime: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StreamCommand {
    SetActive(bool),
    Drain,
    Close,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StreamEvent {
    StateChanged { from: StreamState, to: StreamState },
    FormatChanged(StreamSpec),
    Drained,
}

pub struct SinkStream {
    events: Receiver<StreamEvent>,
    latency: Arc<AtomicU64>,
    control: Box<dyn Fn(StreamCommand) -> Result<()> + Send + Sync>,
}

impl SinkStream {
    pub fn new(
        events: Receiver<StreamEvent>,
        latency: Arc<AtomicU64>,
        control: Box<dyn Fn(StreamCommand) -> Result<()> + Send + Sync>,
    ) -> Self {
        Self {
            events,
            latency,
            control,
        }
    }

    pub fn set_active(&self, active: bool) -> Result<()> {
        (self.control)(StreamCommand::SetActive(active))
    }

    pub fn drain(&self) -> Result<()> {
        (self.control)(StreamCommand::Drain)
    }

    pub fn latency(&self) -> Frames {
        Frames(self.latency.load(Ordering::Relaxed))
    }

    pub const fn events(&self) -> &Receiver<StreamEvent> {
        &self.events
    }

    pub fn close(self) -> Result<()> {
        (self.control)(StreamCommand::Close)
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;

    fn ticking(hz: u32, delay: i64) -> GraphTime {
        GraphTime {
            delay,
            buffered: 0,
            tick: Ratio {
                numer: NonZeroU32::new(1).expect("a tick numerator"),
                denom: NonZeroU32::new(hz).expect("a graph rate"),
            },
        }
    }

    #[test]
    fn a_graph_on_the_streams_own_rate_reports_the_ticks_it_counted() {
        let time = ticking(44_100, 4_410);
        assert_eq!(time.downstream(SampleRate::HZ_44100), Frames(4_410));
    }

    #[test]
    fn a_graph_on_another_rate_reports_the_delay_in_the_streams_frames() {
        let ten_milliseconds = ticking(48_000, 480);
        assert_eq!(
            ten_milliseconds.downstream(SampleRate::HZ_44100),
            Frames(441),
            "a 48 kHz graph was read as though it ticked at the stream's rate"
        );
        assert_eq!(
            ticking(44_100, 441).downstream(SampleRate::HZ_48000),
            Frames(480)
        );
    }

    #[test]
    fn a_delay_that_is_not_ahead_of_the_stream_is_no_delay_at_all() {
        assert_eq!(
            ticking(48_000, 0).downstream(SampleRate::HZ_48000),
            Frames::ZERO
        );
        assert_eq!(
            ticking(48_000, -960).downstream(SampleRate::HZ_48000),
            Frames::ZERO
        );
    }

    #[test]
    fn what_the_stream_still_holds_is_counted_beside_the_delay_to_the_device() {
        let held = GraphTime {
            buffered: 128,
            ..ticking(48_000, 480)
        };
        assert_eq!(
            held.downstream(SampleRate::HZ_48000),
            Frames(608),
            "the frames the stream had not handed over were left out of the reading"
        );
        assert_eq!(
            GraphTime {
                buffered: 64,
                ..ticking(48_000, -960)
            }
            .downstream(SampleRate::HZ_48000),
            Frames(64),
            "a stream ahead of the device still holds what it holds"
        );
    }

    #[test]
    fn a_delay_no_graph_could_report_saturates_rather_than_wrapping() {
        let absurd = ticking(1, i64::MAX);
        assert_eq!(absurd.downstream(SampleRate::HZ_384000), Frames(u64::MAX));
    }
}
