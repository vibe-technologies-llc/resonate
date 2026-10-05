use std::{
    sync::{
        Arc,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
    time::Duration,
};

use crossbeam_channel::Receiver;
use resonate_core::{Frames, Ratio, SampleRate, StreamSpec};

use crate::{Result, SinkId, Words};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GraphTime {
    pub delay: i64,
    pub buffered: u64,
    pub queued: u64,
    pub tick: Ratio,
}

impl GraphTime {
    pub const fn downstream(self, stream: SampleRate) -> Frames {
        Frames(
            self.ahead_of_the_device(stream)
                .saturating_add(self.buffered)
                .saturating_add(self.queued),
        )
    }

    pub fn graph_rate(self) -> Option<SampleRate> {
        if self.tick.numer.get() != 1 {
            return None;
        }
        SampleRate::new(self.tick.denom.get()).ok()
    }

    const fn ahead_of_the_device(self, stream: SampleRate) -> u64 {
        if self.delay <= 0 {
            return 0;
        }
        let seconds = (self.delay as u64).saturating_mul(self.tick.numer.get() as u64);
        seconds.saturating_mul(stream.hz() as u64) / self.tick.denom.get() as u64
    }
}

const NO_GRAPH_RATE: u32 = 0;

#[derive(Debug, Default)]
pub struct StreamClock {
    latency: AtomicU64,
    graph_hz: AtomicU32,
}

impl StreamClock {
    pub const fn behind_by(latency: Frames) -> Self {
        Self {
            latency: AtomicU64::new(latency.get()),
            graph_hz: AtomicU32::new(NO_GRAPH_RATE),
        }
    }

    pub fn note(&self, time: GraphTime, stream: SampleRate) {
        self.latency
            .store(time.downstream(stream).get(), Ordering::Relaxed);
        self.graph_hz.store(
            time.graph_rate().map_or(NO_GRAPH_RATE, SampleRate::hz),
            Ordering::Relaxed,
        );
    }

    pub fn latency(&self) -> Frames {
        Frames(self.latency.load(Ordering::Relaxed))
    }

    pub fn graph_rate(&self) -> Option<SampleRate> {
        SampleRate::new(self.graph_hz.load(Ordering::Relaxed)).ok()
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
    FormatChanged { spec: StreamSpec, words: Words },
    Drained,
}

pub struct SinkStream {
    events: Receiver<StreamEvent>,
    clock: Arc<StreamClock>,
    control: Box<dyn Fn(StreamCommand) -> Result<()> + Send + Sync>,
}

impl SinkStream {
    pub fn new(
        events: Receiver<StreamEvent>,
        clock: Arc<StreamClock>,
        control: Box<dyn Fn(StreamCommand) -> Result<()> + Send + Sync>,
    ) -> Self {
        Self {
            events,
            clock,
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
        self.clock.latency()
    }

    pub fn graph_rate(&self) -> Option<SampleRate> {
        self.clock.graph_rate()
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
            queued: 0,
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
    fn the_buffers_queued_for_the_graph_are_counted_beside_what_the_stream_holds() {
        let queued = GraphTime {
            buffered: 128,
            queued: 1_024,
            ..ticking(48_000, 480)
        };

        assert_eq!(
            queued.downstream(SampleRate::HZ_48000),
            Frames(1_632),
            "the buffers filled and not yet taken by the graph were left out of the reading"
        );
    }

    #[test]
    fn the_clock_says_the_rate_the_graph_ticks_at_and_only_where_a_tick_is_one_frame() {
        let clock = StreamClock::default();
        assert_eq!(clock.graph_rate(), None);

        clock.note(ticking(48_000, 480), SampleRate::HZ_44100);
        assert_eq!(clock.graph_rate(), Some(SampleRate::HZ_48000));
        assert_eq!(clock.latency(), Frames(441));

        let coarse = GraphTime {
            tick: Ratio {
                numer: NonZeroU32::new(2).expect("a tick numerator"),
                denom: NonZeroU32::new(96_000).expect("a tick denominator"),
            },
            ..ticking(48_000, 0)
        };
        clock.note(coarse, SampleRate::HZ_48000);
        assert_eq!(clock.graph_rate(), None);
    }

    #[test]
    fn a_delay_no_graph_could_report_saturates_rather_than_wrapping() {
        let absurd = ticking(1, i64::MAX);
        assert_eq!(absurd.downstream(SampleRate::HZ_384000), Frames(u64::MAX));
    }
}
