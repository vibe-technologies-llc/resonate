use std::{sync::Arc, thread, time::Duration};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded, unbounded};
use parking_lot::Mutex;
use resonate_audio::{
    AudioSource, Error, NodeName, Opener, Operation, SinkId, SinkResult, SinkStream, StreamClock,
    StreamCommand, StreamEvent, StreamRequest, StreamState, Words,
    handover::{Fed, fed_from},
};
use resonate_core::{Frames, SampleFormat};
use thread_priority::{ThreadPriority, set_current_thread_priority};
use wasapi::{ShareMode, StreamMode, WasapiError};

use crate::{
    device::{backend_error, default_device, device_for, enter_the_apartment, enumerate, wave_of},
    formats::wire_of,
    render::Playing,
    ways::{Way, endpoint_of},
};

const OPEN_WITHIN: Duration = Duration::from_secs(5);
const EVENT_WAIT_MS: u32 = 100;
const IDLE_POLL: Duration = Duration::from_millis(50);
const HNS_A_NANOSECOND_DIVISOR: u128 = 100;
const ALIGN_TO_BYTES: u32 = 128;

pub(crate) type Known = Arc<Mutex<Vec<(SinkId, NodeName)>>>;

pub(crate) struct Opens {
    pub(crate) known: Known,
}

enum Target {
    Default,
    Node(NodeName),
}

struct Opening {
    target: Target,
    request: StreamRequest,
    fed: Fed,
    ready: Sender<SinkResult<()>>,
    commands: Receiver<StreamCommand>,
    announced: Sender<StreamEvent>,
    clock: Arc<StreamClock>,
}

impl Opener for Opens {
    fn open(
        &self,
        request: &StreamRequest,
        source: Box<dyn AudioSource>,
    ) -> SinkResult<SinkStream> {
        let target = self.target_of(request.target)?;
        let (sending, commands) = unbounded();
        let (announced, events) = unbounded();
        let (ready, started) = bounded(1);
        let (fed, _handing) = fed_from(source);
        let clock = Arc::new(StreamClock::behind_by(Frames::ZERO));
        let opening = Opening {
            target,
            request: request.clone(),
            fed,
            ready,
            commands,
            announced,
            clock: Arc::clone(&clock),
        };
        thread::Builder::new()
            .name("resonate-wasapi-stream".into())
            .spawn(move || play(opening))
            .map_err(|source| Error::Backend {
                op: Operation::Open,
                source,
            })?;
        match started.recv_timeout(OPEN_WITHIN) {
            Ok(opened) => opened?,
            Err(RecvTimeoutError::Timeout) => return Err(Error::OpenUnanswered),
            Err(RecvTimeoutError::Disconnected) => return Err(Error::LoopStopped),
        }
        Ok(SinkStream::new(
            events,
            clock,
            Box::new(move |command| sending.send(command).map_err(|_| Error::LoopStopped)),
        ))
    }
}

impl Opens {
    fn target_of(&self, chosen: Option<SinkId>) -> SinkResult<Target> {
        let Some(id) = chosen else {
            return Ok(Target::Default);
        };
        let named = |known: &Known| {
            known
                .lock()
                .iter()
                .find(|(held, _)| *held == id)
                .map(|(_, node)| node.clone())
        };
        if let Some(node) = named(&self.known) {
            return Ok(Target::Node(node));
        }
        let sinks = enumerate()?;
        *self.known.lock() = sinks
            .iter()
            .map(|sink| (sink.id, sink.name.clone()))
            .collect();
        named(&self.known)
            .map(Target::Node)
            .ok_or(Error::SinkGone { node: id })
    }
}

fn play(opening: Opening) {
    enter_the_apartment();
    let Opening {
        target,
        request,
        fed,
        ready,
        commands,
        announced,
        clock,
    } = opening;
    if request.realtime {
        let _ = set_current_thread_priority(ThreadPriority::Max);
    }
    match start(&target, &request, fed, clock) {
        Ok(playing) => {
            let _ = announced.send(StreamEvent::FormatChanged {
                spec: request.spec,
                words: words_of(request.spec.format),
            });
            let _ = ready.send(Ok(()));
            serve(playing, &commands, &announced);
        }
        Err(error) => {
            let _ = ready.send(Err(error));
        }
    }
}

const fn words_of(format: SampleFormat) -> Words {
    match format {
        SampleFormat::S24 => Words::Padded,
        SampleFormat::S16 | SampleFormat::S32 | SampleFormat::F32 => Words::Whole,
    }
}

fn start(
    target: &Target,
    request: &StreamRequest,
    fed: Fed,
    clock: Arc<StreamClock>,
) -> SinkResult<Playing> {
    let (device, way) = match target {
        Target::Default => (default_device()?, Way::Shared),
        Target::Node(node) => {
            let (way, endpoint) = endpoint_of(node).ok_or(Error::NoSink)?;
            (device_for(endpoint)?, way)
        }
    };
    let spec = request.spec;
    let wave = wave_of(spec.format, spec.rate, spec.channels);
    let mut client = device
        .get_iaudioclient()
        .map_err(|error| backend_error(Operation::Open, error))?;
    let (mode, share) = match way {
        Way::Shared => (
            StreamMode::EventsShared {
                autoconvert: true,
                buffer_duration_hns: buffer_hns(request),
            },
            ShareMode::Shared,
        ),
        Way::Exclusive => {
            let (default_period, _) = client
                .get_device_period()
                .map_err(|error| backend_error(Operation::Open, error))?;
            let desired = match request.latency {
                resonate_audio::LatencyRequest::Auto => default_period,
                resonate_audio::LatencyRequest::Duration(_) => buffer_hns(request),
            };
            let period = client
                .calculate_aligned_period_near(desired, Some(ALIGN_TO_BYTES), &wave)
                .map_err(|error| backend_error(Operation::Open, error))?;
            (
                StreamMode::EventsExclusive { period_hns: period },
                ShareMode::Exclusive,
            )
        }
    };
    client
        .initialize_client(&wave, &wasapi::Direction::Render, &mode)
        .map_err(initialize_error)?;
    let event = client
        .set_get_eventhandle()
        .map_err(|error| backend_error(Operation::Open, error))?;
    let render = client
        .get_audiorenderclient()
        .map_err(|error| backend_error(Operation::Open, error))?;
    let buffer_frames = client
        .get_buffer_size()
        .map_err(|error| backend_error(Operation::Open, error))?;
    let frame_bytes =
        wire_of(spec.format).store_bits / 8 * usize::from(spec.channels.count().get());
    let frame_bytes = std::num::NonZeroUsize::new(frame_bytes).ok_or(Error::FormatRefused)?;
    let scratch =
        vec![0_u8; frame_bytes.get() * usize::try_from(buffer_frames).unwrap_or_default()];

    Ok(Playing {
        client,
        render,
        event,
        fed,
        scratch,
        buffer_frames,
        way: share,
        format: spec.format,
        rate: spec.rate,
        frame_bytes,
        clock,
        written: 0,
        real: 0,
    })
}

fn buffer_hns(request: &StreamRequest) -> i64 {
    match request.latency {
        resonate_audio::LatencyRequest::Auto => 0,
        resonate_audio::LatencyRequest::Duration(wanted) => {
            i64::try_from(wanted.as_nanos() / HNS_A_NANOSECOND_DIVISOR).unwrap_or(0)
        }
    }
}

fn initialize_error(error: WasapiError) -> Error {
    match error {
        WasapiError::UnsupportedFormat => Error::FormatRefused,
        other => backend_error(Operation::Format, other),
    }
}

fn serve(
    mut playing: Playing,
    commands: &Receiver<StreamCommand>,
    announced: &Sender<StreamEvent>,
) {
    let mut state = StreamState::Paused;
    let mut draining = false;
    loop {
        if state == StreamState::Streaming {
            while let Ok(command) = commands.try_recv() {
                match handled(&mut playing, command, &mut state, &mut draining, announced) {
                    Handled::Went => return,
                    Handled::Stays => {}
                }
            }
            if playing.event.wait_for_event(EVENT_WAIT_MS).is_err() {
                continue;
            }
            match playing.cycle() {
                Ok(cycled) => {
                    if draining && cycled.has_played_out(playing.real) {
                        draining = false;
                        let _ = announced.send(StreamEvent::Drained);
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "a WASAPI stream failed");
                    let _ = announced.send(StreamEvent::StateChanged {
                        from: state,
                        to: StreamState::Failed,
                    });
                    return;
                }
            }
        } else {
            match commands.recv_timeout(IDLE_POLL) {
                Ok(command) => {
                    match handled(&mut playing, command, &mut state, &mut draining, announced) {
                        Handled::Went => return,
                        Handled::Stays => {}
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    let _ = playing.client.stop_stream();
                    return;
                }
            }
        }
    }
}

enum Handled {
    Stays,
    Went,
}

fn handled(
    playing: &mut Playing,
    command: StreamCommand,
    state: &mut StreamState,
    draining: &mut bool,
    announced: &Sender<StreamEvent>,
) -> Handled {
    match command {
        StreamCommand::Close => {
            let _ = playing.client.stop_stream();
            Handled::Went
        }
        StreamCommand::Drain => {
            *draining = true;
            Handled::Stays
        }
        StreamCommand::SetActive(active) => {
            let from = *state;
            let moved = if active {
                playing
                    .cycle()
                    .map(|_| ())
                    .and_then(|()| playing.client.start_stream())
            } else {
                playing
                    .client
                    .stop_stream()
                    .and_then(|()| playing.client.reset_stream())
                    .map(|()| {
                        playing.written = 0;
                        playing.real = 0;
                    })
            };
            let to = match (moved, active) {
                (Err(error), _) => {
                    tracing::warn!(%error, "a WASAPI stream would not change state");
                    StreamState::Failed
                }
                (Ok(()), true) => StreamState::Streaming,
                (Ok(()), false) => StreamState::Paused,
            };
            *state = to;
            if !active {
                *draining = false;
            }
            let _ = announced.send(StreamEvent::StateChanged { from, to });
            match to {
                StreamState::Failed => Handled::Went,
                _ => Handled::Stays,
            }
        }
    }
}
