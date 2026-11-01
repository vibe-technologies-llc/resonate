use std::{sync::Arc, thread, time::Duration};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded, unbounded};
use parking_lot::Mutex;
use resonate_audio::{
    AudioSource, Error, NodeName, Opener, Operation, Reshaped, Reshaper, SinkId, SinkResult,
    SinkStream, StreamClock, StreamCommand, StreamEvent, StreamRequest, StreamState, Unreshapable,
    Words,
    handover::{Fed, Handing, fed_from},
};
use resonate_core::{Frames, SampleFormat};
use resonate_win32::PerformanceCounter;
use wasapi::{ShareMode, StreamMode, WasapiError};

use crate::{
    device::{backend_error, default_device, device_for, enter_the_apartment, enumerate, wave_of},
    formats::wire_of,
    priority::Priority,
    render::Playing,
    reshaping::refusal_of,
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

enum Order {
    Command(StreamCommand),
    Reshaped { realtime: bool },
}

struct Opening {
    target: Target,
    request: StreamRequest,
    fed: Fed,
    ready: Sender<SinkResult<()>>,
    orders: Receiver<Order>,
    announced: Sender<StreamEvent>,
    clock: Arc<StreamClock>,
}

struct Reshaping {
    open: StreamRequest,
    handing: Handing,
}

impl Opener for Opens {
    fn open(
        &self,
        request: &StreamRequest,
        source: Box<dyn AudioSource>,
    ) -> SinkResult<SinkStream> {
        let target = self.target_of(request.target)?;
        let (sending, orders) = unbounded();
        let (announced, events) = unbounded();
        let (ready, started) = bounded(1);
        let (fed, handing) = fed_from(source);
        let clock = Arc::new(StreamClock::behind_by(Frames::ZERO));
        let opening = Opening {
            target,
            request: request.clone(),
            fed,
            ready,
            orders,
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
        let reshaping = sending.clone();
        Ok(SinkStream::new(
            events,
            clock,
            Box::new(move |command| {
                sending
                    .send(Order::Command(command))
                    .map_err(|_| Error::LoopStopped)
            }),
        )
        .reshaped_by(reshaper(
            Reshaping {
                open: request.clone(),
                handing,
            },
            reshaping,
        )))
    }
}

fn reshaper(reshaping: Reshaping, orders: Sender<Order>) -> Reshaper {
    let reshaping = Mutex::new(reshaping);
    Box::new(move |asked, source| {
        let mut reshaping = reshaping.lock();
        reshaping.handing.let_go_of_the_spent();
        if let Some(because) = refusal_of(&reshaping.open, asked) {
            return Reshaped::Refused { source, because };
        }
        if !reshaping.handing.has_room() {
            return Reshaped::Refused {
                source,
                because: Unreshapable::StillHandingOver,
            };
        }
        let order = Order::Reshaped {
            realtime: asked.realtime,
        };
        if orders.send(order).is_err() {
            return Reshaped::Refused {
                source,
                because: Unreshapable::NoStream,
            };
        }
        match reshaping.handing.hand(source) {
            Ok(()) => {
                reshaping.open = asked.clone();
                Reshaped::Kept
            }
            Err(source) => Reshaped::Refused {
                source,
                because: Unreshapable::StillHandingOver,
            },
        }
    })
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
        orders,
        announced,
        clock,
    } = opening;
    let mut priority = Priority::ordinary();
    priority.set(request.realtime);
    match start(&target, &request, fed, clock) {
        Ok(playing) => {
            let _ = announced.send(StreamEvent::FormatChanged {
                spec: request.spec,
                words: words_of(request.spec.format),
            });
            let _ = ready.send(Ok(()));
            serve(
                playing,
                &Serving {
                    orders: &orders,
                    announced: &announced,
                },
                &mut priority,
            );
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
    let audio_clock = client.get_audioclock().ok().and_then(|audio_clock| {
        let frequency = std::num::NonZeroU64::new(audio_clock.get_frequency().ok()?)?;
        Some((audio_clock, frequency))
    });
    let counter = PerformanceCounter::of_this_machine().ok();

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
        audio_clock,
        counter,
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

struct Serving<'a> {
    orders: &'a Receiver<Order>,
    announced: &'a Sender<StreamEvent>,
}

struct Standing {
    state: StreamState,
    draining: bool,
}

fn serve(mut playing: Playing, serving: &Serving<'_>, priority: &mut Priority) {
    let mut standing = Standing {
        state: StreamState::Paused,
        draining: false,
    };
    loop {
        if standing.state == StreamState::Streaming {
            while let Ok(order) = serving.orders.try_recv() {
                match handled(&mut playing, order, &mut standing, serving, priority) {
                    Handled::Went => return,
                    Handled::Stays => {}
                }
            }
            if playing.event.wait_for_event(EVENT_WAIT_MS).is_err() {
                continue;
            }
            match playing.cycle() {
                Ok(cycled) => {
                    if standing.draining && cycled.has_played_out(playing.real) {
                        standing.draining = false;
                        let _ = serving.announced.send(StreamEvent::Drained);
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "a WASAPI stream failed");
                    let _ = serving.announced.send(StreamEvent::StateChanged {
                        from: standing.state,
                        to: StreamState::Failed,
                    });
                    return;
                }
            }
        } else {
            match serving.orders.recv_timeout(IDLE_POLL) {
                Ok(order) => match handled(&mut playing, order, &mut standing, serving, priority) {
                    Handled::Went => return,
                    Handled::Stays => {}
                },
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
    order: Order,
    standing: &mut Standing,
    serving: &Serving<'_>,
    priority: &mut Priority,
) -> Handled {
    match order {
        Order::Reshaped { realtime } => {
            standing.draining = false;
            priority.set(realtime);
            Handled::Stays
        }
        Order::Command(StreamCommand::Close) => {
            let _ = playing.client.stop_stream();
            Handled::Went
        }
        Order::Command(StreamCommand::Drain) => {
            standing.draining = true;
            Handled::Stays
        }
        Order::Command(StreamCommand::SetActive(true))
            if standing.state == StreamState::Streaming =>
        {
            Handled::Stays
        }
        Order::Command(StreamCommand::SetActive(active)) => {
            let from = standing.state;
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
            standing.state = to;
            if !active {
                standing.draining = false;
            }
            let _ = serving
                .announced
                .send(StreamEvent::StateChanged { from, to });
            match to {
                StreamState::Failed => Handled::Went,
                _ => Handled::Stays,
            }
        }
    }
}
