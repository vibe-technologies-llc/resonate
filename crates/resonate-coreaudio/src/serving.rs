use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded, unbounded};
use parking_lot::Mutex;
use resonate_audio::{
    AudioSource, Error, Operation, Reshaped, Reshaper, SinkResult, SinkStream, StreamClock,
    StreamCommand, StreamEvent, StreamRequest, StreamState, Unreshapable, handover::Handing,
    refusal_of,
};
use resonate_core::{Frames, StreamSpec};

use crate::{
    clock::Latencies,
    drain::Drain,
    fill::{Presented, Rendering, Shaped},
    formats::{ClientWord, frame_of, laid_of, words_of},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Looks {
    pub looked_at_every: Duration,
    pub looked_at_while_draining_every: Duration,
}

impl Looks {
    pub const DEVICES: Self = Self {
        looked_at_every: Duration::from_millis(500),
        looked_at_while_draining_every: Duration::from_millis(10),
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    Gone,
    Here {
        device_hz: u32,
        latencies: Latencies,
    },
}

pub trait Hardware {
    fn start(&self) -> i32;
    fn stop(&self) -> i32;
    fn standing(&self) -> Standing;
}

enum Order {
    Command(StreamCommand),
    Reshaped,
}

struct Reshaping {
    open: StreamRequest,
    handing: Handing,
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
        if orders.send(Order::Reshaped).is_err() {
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

struct Served {
    spec: StreamSpec,
    clock: Arc<StreamClock>,
    past_the_host_time: Arc<AtomicU64>,
    empty: Arc<AtomicBool>,
    orders: Receiver<Order>,
    announced: Sender<StreamEvent>,
    looks: Looks,
}

pub fn launch<H, M>(
    request: &StreamRequest,
    source: Box<dyn AudioSource>,
    within: Duration,
    looks: Looks,
    make: M,
) -> SinkResult<SinkStream>
where
    H: Hardware,
    M: FnOnce(Box<Rendering>) -> SinkResult<H> + Send + 'static,
{
    let spec = request.spec;
    if ClientWord::of(spec.format).is_none() {
        return Err(Error::FormatRefused);
    }
    let (sending, orders) = unbounded();
    let (announced, events) = unbounded();
    let (ready, started) = bounded(1);
    let (disposed, closed) = bounded(1);
    let clock = Arc::new(StreamClock::behind_by(Frames::ZERO));
    let empty = Arc::new(AtomicBool::new(false));
    let past_the_host_time = Arc::new(AtomicU64::new(0));
    let shaped = Shaped {
        frame: frame_of(spec),
        laid: laid_of(spec.format),
        presented: Presented::PastTheHostTime(Arc::clone(&past_the_host_time)),
    };
    let (rendering, handing) =
        Rendering::fed_by(source, Arc::clone(&empty), Arc::clone(&clock), shaped);
    let served = Served {
        spec,
        clock: Arc::clone(&clock),
        past_the_host_time,
        empty,
        orders,
        announced,
        looks,
    };
    thread::Builder::new()
        .name("resonate-coreaudio-stream".into())
        .spawn(move || {
            let hardware = match make(Box::new(rendering)) {
                Ok(hardware) => hardware,
                Err(refused) => {
                    let _ = ready.send(Err(refused));
                    return;
                }
            };
            let _ = served.announced.send(StreamEvent::FormatChanged {
                spec: served.spec,
                words: words_of(served.spec.format),
            });
            let _ = ready.send(Ok(()));
            serve(&hardware, &served);
            drop(hardware);
            let _ = disposed.send(());
        })
        .map_err(|source| Error::Backend {
            op: Operation::Open,
            source,
        })?;
    match started.recv_timeout(within) {
        Ok(opened) => opened?,
        Err(RecvTimeoutError::Timeout) => return Err(Error::OpenUnanswered),
        Err(RecvTimeoutError::Disconnected) => return Err(Error::LoopStopped),
    }
    let reshaping = Reshaping {
        open: request.clone(),
        handing,
    };
    let control = sending.clone();
    Ok(SinkStream::new(
        events,
        clock,
        Box::new(move |command| {
            let closing = matches!(command, StreamCommand::Close);
            match (control.send(Order::Command(command)), closing) {
                (Ok(()), false) => Ok(()),
                (Err(_), false) => Err(Error::LoopStopped),
                (Err(_), true) => Ok(()),
                (Ok(()), true) => closed.recv_timeout(within).map_err(|_| Error::Backend {
                    op: Operation::Close,
                    source: io::ErrorKind::TimedOut.into(),
                }),
            }
        }),
    )
    .reshaped_by(reshaper(reshaping, sending)))
}

fn serve<H: Hardware>(hardware: &H, served: &Served) {
    let mut state = StreamState::Paused;
    let mut drain = Drain::new();
    loop {
        let latency = match hardware.standing() {
            Standing::Gone => {
                if state != StreamState::Failed {
                    let _ = served.announced.send(StreamEvent::StateChanged {
                        from: state,
                        to: StreamState::Failed,
                    });
                    state = StreamState::Failed;
                }
                Duration::ZERO
            }
            Standing::Here {
                device_hz,
                latencies,
            } => {
                let rate = served.spec.rate;
                served.clock.note(latencies.graph_time(device_hz), rate);
                served.past_the_host_time.store(
                    latencies.heard_past_the_host_time(device_hz, rate),
                    Ordering::Relaxed,
                );
                served.clock.latency().to_duration(rate)
            }
        };
        if drain.settled(
            served.empty.load(Ordering::Acquire),
            latency,
            Instant::now(),
        ) {
            let _ = served.announced.send(StreamEvent::Drained);
        }
        let wait = if drain.is_requested() {
            served.looks.looked_at_while_draining_every
        } else {
            served.looks.looked_at_every
        };
        match served.orders.recv_timeout(wait) {
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) | Ok(Order::Command(StreamCommand::Close)) => {
                return;
            }
            Ok(Order::Reshaped) => drain = Drain::new(),
            Ok(Order::Command(StreamCommand::Drain)) => drain.request(),
            Ok(Order::Command(StreamCommand::SetActive(active))) => {
                let settled = active && state == StreamState::Streaming;
                if settled || state == StreamState::Failed {
                    continue;
                }
                let status = if active {
                    hardware.start()
                } else {
                    drain = Drain::new();
                    hardware.stop()
                };
                if status != 0 {
                    let op = if active {
                        Operation::Start
                    } else {
                        Operation::Pause
                    };
                    let _ = served.announced.send(StreamEvent::Refused {
                        from: state,
                        op,
                        status,
                    });
                    state = StreamState::Failed;
                    continue;
                }
                let next = if active {
                    StreamState::Streaming
                } else {
                    StreamState::Paused
                };
                let _ = served.announced.send(StreamEvent::StateChanged {
                    from: state,
                    to: next,
                });
                state = next;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use resonate_audio::{LatencyRequest, MediaRole, SinkId, Words};
    use resonate_core::{ChannelLayout, SampleFormat, SampleRate};

    use super::*;

    const WAIT: Duration = Duration::from_secs(5);
    const SHORT: Duration = Duration::from_millis(300);
    const QUICK: Looks = Looks {
        looked_at_every: Duration::from_millis(5),
        looked_at_while_draining_every: Duration::from_millis(2),
    };
    const HEARD: Latencies = Latencies {
        device: 96,
        safety_offset: 0,
        stream: 0,
        buffer: 384,
    };

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Call {
        Start,
        Stop,
    }

    struct Marked(u8);

    impl AudioSource for Marked {
        fn fill(&mut self, dst: &mut [u8]) -> usize {
            dst.fill(self.0);
            dst.len()
        }
    }

    struct Silent;

    impl AudioSource for Silent {
        fn fill(&mut self, _: &mut [u8]) -> usize {
            0
        }
    }

    struct Fake {
        calls: Arc<Mutex<Vec<Call>>>,
        refusing: Option<(Call, i32)>,
        standing: Arc<Mutex<Standing>>,
        disposed: Arc<AtomicUsize>,
        _rendering: Arc<Mutex<Box<Rendering>>>,
    }

    impl Fake {
        fn answer(&self, call: Call) -> i32 {
            self.calls.lock().push(call);
            self.refusing
                .filter(|(refused, _)| *refused == call)
                .map_or(0, |(_, status)| status)
        }
    }

    impl Hardware for Fake {
        fn start(&self) -> i32 {
            self.answer(Call::Start)
        }

        fn stop(&self) -> i32 {
            self.answer(Call::Stop)
        }

        fn standing(&self) -> Standing {
            *self.standing.lock()
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            self.disposed.fetch_add(1, Ordering::AcqRel);
        }
    }

    struct Opened {
        stream: SinkStream,
        calls: Arc<Mutex<Vec<Call>>>,
        standing: Arc<Mutex<Standing>>,
        disposed: Arc<AtomicUsize>,
        rendering: Arc<Mutex<Box<Rendering>>>,
    }

    fn request(format: SampleFormat) -> StreamRequest {
        StreamRequest {
            target: Some(SinkId::new(7)),
            spec: StreamSpec::new(SampleRate::HZ_48000, ChannelLayout::Stereo, format),
            latency: LatencyRequest::Auto,
            role: MediaRole::Music,
            media_name: String::new(),
            force_graph_rate: false,
            no_convert: true,
            realtime: true,
        }
    }

    fn opened_with(
        source: Box<dyn AudioSource>,
        format: SampleFormat,
        refusing: Option<(Call, i32)>,
    ) -> Opened {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let disposed = Arc::new(AtomicUsize::new(0));
        let standing = Arc::new(Mutex::new(Standing::Here {
            device_hz: 48_000,
            latencies: HEARD,
        }));
        let (flag, flagged) = bounded(1);
        let stream = launch(&request(format), source, WAIT, QUICK, {
            let calls = Arc::clone(&calls);
            let disposed = Arc::clone(&disposed);
            let standing = Arc::clone(&standing);
            move |rendering| {
                let rendering = Arc::new(Mutex::new(rendering));
                let _ = flag.send(Arc::clone(&rendering));
                Ok(Fake {
                    calls,
                    refusing,
                    standing,
                    disposed,
                    _rendering: rendering,
                })
            }
        })
        .expect("a stream");
        let rendering = flagged.recv_timeout(WAIT).expect("the rendering");
        Opened {
            stream,
            calls,
            standing,
            disposed,
            rendering,
        }
    }

    fn opened() -> Opened {
        opened_with(Box::new(Silent), SampleFormat::F32, None)
    }

    fn event(opened: &Opened) -> StreamEvent {
        opened.stream.events().recv_timeout(WAIT).expect("an event")
    }

    fn quiet(opened: &Opened) {
        assert!(opened.stream.events().recv_timeout(SHORT).is_err());
    }

    fn wait_until(holds: impl Fn() -> bool) {
        let started = Instant::now();
        while !holds() {
            assert!(started.elapsed() < WAIT, "the stream never got there");
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn a_stream_opens_announcing_the_format_it_was_made_in_and_how_its_words_travel() {
        let opened = opened_with(Box::new(Silent), SampleFormat::S24, None);

        assert_eq!(
            event(&opened),
            StreamEvent::FormatChanged {
                spec: request(SampleFormat::S24).spec,
                words: Words::Padded,
            }
        );
        quiet(&opened);
    }

    #[test]
    fn a_word_the_unit_cannot_carry_exactly_is_refused_before_anything_is_made() {
        let made = launch(
            &request(SampleFormat::S32),
            Box::new(Silent),
            WAIT,
            QUICK,
            |_| -> SinkResult<Fake> { panic!("hardware was made for a refused word") },
        );

        assert!(matches!(made, Err(Error::FormatRefused)));
    }

    #[test]
    fn a_stream_starts_once_however_often_it_is_told_to_and_stops_when_paused() {
        let opened = opened();
        let _ = event(&opened);

        opened.stream.set_active(true).expect("started");
        opened.stream.set_active(true).expect("started again");
        opened.stream.set_active(false).expect("paused");

        assert_eq!(
            event(&opened),
            StreamEvent::StateChanged {
                from: StreamState::Paused,
                to: StreamState::Streaming
            }
        );
        assert_eq!(
            event(&opened),
            StreamEvent::StateChanged {
                from: StreamState::Streaming,
                to: StreamState::Paused
            }
        );
        assert_eq!(*opened.calls.lock(), [Call::Start, Call::Stop]);
    }

    #[test]
    fn a_refused_start_is_told_with_its_status_and_leaves_the_stream_failed() {
        let opened = opened_with(
            Box::new(Silent),
            SampleFormat::F32,
            Some((Call::Start, 560_227_702)),
        );
        let _ = event(&opened);

        opened.stream.set_active(true).expect("asked");
        opened.stream.set_active(true).expect("asked again");

        assert_eq!(
            event(&opened),
            StreamEvent::Refused {
                from: StreamState::Paused,
                op: Operation::Start,
                status: 560_227_702
            }
        );
        quiet(&opened);
        assert_eq!(*opened.calls.lock(), [Call::Start]);
    }

    #[test]
    fn a_device_that_went_is_told_once_as_a_failed_stream() {
        let opened = opened();
        let _ = event(&opened);
        opened.stream.set_active(true).expect("started");
        let _ = event(&opened);

        *opened.standing.lock() = Standing::Gone;

        assert_eq!(
            event(&opened),
            StreamEvent::StateChanged {
                from: StreamState::Streaming,
                to: StreamState::Failed
            }
        );
        quiet(&opened);
    }

    #[test]
    fn the_clock_reads_the_devices_latency_and_the_rate_another_program_moved_it_to() {
        let opened = opened();

        wait_until(|| opened.stream.latency() == Frames(480));
        assert_eq!(opened.stream.graph_rate(), Some(SampleRate::HZ_48000));

        *opened.standing.lock() = Standing::Here {
            device_hz: 96_000,
            latencies: HEARD,
        };

        wait_until(|| opened.stream.graph_rate() == Some(SampleRate::HZ_96000));
        assert_eq!(opened.stream.latency(), Frames(240));
    }

    #[test]
    fn a_drain_is_told_once_the_source_has_nothing_left_and_the_latency_has_passed() {
        let opened = opened();
        let _ = event(&opened);
        opened.stream.set_active(true).expect("started");
        let _ = event(&opened);

        opened.stream.drain().expect("asked to drain");
        quiet(&opened);
        opened.rendering.lock().fill(&mut [0_u8; 16], None);

        assert_eq!(event(&opened), StreamEvent::Drained);
        quiet(&opened);
        assert_eq!(*opened.calls.lock(), [Call::Start]);
    }

    #[test]
    fn a_stream_asked_for_the_shape_it_holds_takes_the_new_source_and_calls_off_a_drain() {
        let opened = opened();
        let _ = event(&opened);
        opened.stream.drain().expect("asked to drain");

        let reshaped = opened
            .stream
            .reshape(&request(SampleFormat::F32), Box::new(Marked(0x5A)));

        assert!(matches!(reshaped, Reshaped::Kept));
        let mut bytes = [0_u8; 16];
        opened.rendering.lock().fill(&mut bytes, None);
        assert_eq!(bytes, [0x5A; 16]);
        quiet(&opened);
    }

    #[test]
    fn another_format_or_device_is_refused_and_hands_the_source_back() {
        let opened = opened();
        let elsewhere = StreamRequest {
            target: None,
            ..request(SampleFormat::F32)
        };

        let reformatted = opened
            .stream
            .reshape(&request(SampleFormat::S16), Box::new(Silent));
        let moved = opened.stream.reshape(&elsewhere, Box::new(Silent));

        assert!(matches!(
            reformatted,
            Reshaped::Refused {
                because: Unreshapable::AnotherFormat,
                ..
            }
        ));
        assert!(matches!(
            moved,
            Reshaped::Refused {
                because: Unreshapable::AnotherTarget,
                ..
            }
        ));
    }

    #[test]
    fn closing_returns_only_once_the_hardware_is_let_go() {
        let opened = opened();
        let disposed = Arc::clone(&opened.disposed);

        opened.stream.close().expect("closed");

        assert_eq!(disposed.load(Ordering::Acquire), 1);
    }

    #[test]
    fn an_open_that_never_answers_times_out_and_the_late_hardware_is_let_go_by_its_own_thread() {
        let disposed = Arc::new(AtomicUsize::new(0));
        let (release, released) = bounded::<()>(1);
        let made = launch(
            &request(SampleFormat::F32),
            Box::new(Silent),
            Duration::from_millis(50),
            QUICK,
            {
                let disposed = Arc::clone(&disposed);
                move |rendering| {
                    let _ = released.recv_timeout(WAIT);
                    Ok(Fake {
                        calls: Arc::default(),
                        refusing: None,
                        standing: Arc::new(Mutex::new(Standing::Gone)),
                        disposed,
                        _rendering: Arc::new(Mutex::new(rendering)),
                    })
                }
            },
        );

        assert!(matches!(made, Err(Error::OpenUnanswered)));
        release.send(()).expect("the opening thread is waiting");
        wait_until(|| disposed.load(Ordering::Acquire) == 1);
    }

    #[test]
    fn hardware_that_would_not_open_is_the_opens_own_error() {
        let made = launch(
            &request(SampleFormat::F32),
            Box::new(Silent),
            WAIT,
            QUICK,
            |_| -> SinkResult<Fake> { Err(Error::NoSink) },
        );

        assert!(matches!(made, Err(Error::NoSink)));
    }
}
