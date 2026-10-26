use std::{
    io,
    num::NonZeroU32,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded, select, unbounded};
use parking_lot::Mutex;
use resonate_audio::{
    AudioSource, Error, GraphTime, Operation, Reshaped, Reshaper, Result, SinkStream, StreamClock,
    StreamCommand, StreamEvent, StreamRequest, StreamState, Unreshapable, Words,
};
use resonate_core::{ChannelLayout, Frames, Ratio, SampleFormat, SampleRate, StreamSpec};

use super::{
    handover::Handing,
    remote_io_fill::Rendering,
    remote_io_state::{Drain, Routing},
};

const COMMAND_POLL: Duration = Duration::from_millis(10);
const QUIET_WAIT: Duration = Duration::from_secs(24 * 60 * 60);

pub(crate) trait Hardware {
    fn start(&self) -> i32;
    fn stop(&self) -> i32;
    fn reformat(&self, rate: SampleRate) -> i32;
}

enum Order {
    Command(StreamCommand),
    Reshape {
        rate: SampleRate,
        answer: Sender<i32>,
    },
}

fn shape_of(rate: SampleRate) -> StreamSpec {
    StreamSpec::new(rate, ChannelLayout::Stereo, SampleFormat::F32)
}

fn reshaper(
    routing: Arc<Routing>,
    orders: Sender<Order>,
    handing: Handing,
    within: Duration,
) -> Reshaper {
    let handing = Mutex::new(handing);
    Box::new(move |request, source| {
        let mut handing = handing.lock();
        handing.let_go_of_the_spent();
        let rate = routing.rate();
        if request.spec != shape_of(rate) {
            return Reshaped::Refused {
                source,
                because: Unreshapable::AnotherFormat,
            };
        }
        if !handing.has_room() {
            return Reshaped::Refused {
                source,
                because: Unreshapable::StillHandingOver,
            };
        }
        let (answer, answered) = bounded(1);
        if orders.send(Order::Reshape { rate, answer }).is_err() {
            return Reshaped::Refused {
                source,
                because: Unreshapable::NoStream,
            };
        }
        match answered.recv_timeout(within) {
            Ok(0) => {}
            Ok(_) => {
                return Reshaped::Refused {
                    source,
                    because: Unreshapable::FormatUnwritten,
                };
            }
            Err(_) => {
                return Reshaped::Refused {
                    source,
                    because: Unreshapable::NoStream,
                };
            }
        }
        match handing.hand(source) {
            Ok(()) => Reshaped::Kept,
            Err(source) => Reshaped::Refused {
                source,
                because: Unreshapable::StillHandingOver,
            },
        }
    })
}

pub(crate) fn launch<H, M>(
    routing: &Arc<Routing>,
    request: &StreamRequest,
    source: Box<dyn AudioSource>,
    within: Duration,
    make: M,
) -> Result<SinkStream>
where
    H: Hardware,
    M: FnOnce(SampleRate, Box<Rendering>) -> Result<H> + Send + 'static,
{
    let rate = routing.rate();
    let spec = shape_of(rate);
    if request.spec != spec {
        return Err(Error::FormatRefused);
    }
    let (commands, asked) = unbounded();
    let (announced, events) = unbounded();
    let (ready, started) = bounded(1);
    let (disposed, closed) = bounded(1);
    let clock = Arc::new(StreamClock::behind_by(Frames::ZERO));
    let empty = Arc::new(AtomicBool::new(false));
    let (rendering, handing) = Rendering::fed_by(source, Arc::clone(&empty));
    let thread_clock = Arc::clone(&clock);
    let routing = Arc::clone(routing);
    let reshape = reshaper(Arc::clone(&routing), commands.clone(), handing, within);
    thread::Builder::new()
        .name("resonate-mobile-output".into())
        .spawn(move || {
            let rendering = Box::new(rendering);
            let hardware = match make(rate, rendering) {
                Ok(hardware) => hardware,
                Err(refused) => {
                    let _ = ready.send(Err(refused));
                    return;
                }
            };
            let _ = announced.send(StreamEvent::FormatChanged {
                spec,
                words: Words::Whole,
            });
            let _ = ready.send(Ok(()));
            serve(
                &hardware,
                rate,
                &routing,
                &thread_clock,
                &empty,
                &asked,
                &announced,
            );
            drop(hardware);
            let _ = disposed.send(());
        })
        .map_err(|source| Error::Backend {
            op: Operation::Open,
            source,
        })?;
    match started.recv_timeout(within) {
        Ok(opened) => opened?,
        Err(RecvTimeoutError::Timeout) => {
            return Err(Error::Backend {
                op: Operation::Open,
                source: io::ErrorKind::TimedOut.into(),
            });
        }
        Err(RecvTimeoutError::Disconnected) => return Err(Error::LoopStopped),
    }
    Ok(SinkStream::new(
        events,
        clock,
        Box::new(move |command| {
            let closing = matches!(command, StreamCommand::Close);
            commands
                .send(Order::Command(command))
                .map_err(|_| Error::Disconnected)?;
            if closing {
                closed.recv_timeout(within).map_err(|_| Error::Backend {
                    op: Operation::Close,
                    source: io::ErrorKind::TimedOut.into(),
                })?;
            }
            Ok(())
        }),
    )
    .reshaped_by(reshape))
}

fn serve<H: Hardware>(
    hardware: &H,
    rate: SampleRate,
    routing: &Routing,
    clock: &StreamClock,
    empty: &AtomicBool,
    asked: &Receiver<Order>,
    announced: &Sender<StreamEvent>,
) {
    let mut rate = rate;
    let mut tick = tick_of(rate);
    let mut state = StreamState::Paused;
    let mut drain = Drain::new();
    loop {
        clock.note(
            GraphTime {
                delay: routing.latency().get().min(i64::MAX as u64) as i64,
                buffered: 0,
                queued: 0,
                tick,
            },
            rate,
        );
        let latency = clock.latency().to_duration(rate);
        if drain.settled(empty.load(Ordering::Acquire), latency, Instant::now()) {
            hardware.stop();
            let _ = announced.send(StreamEvent::Drained);
        }
        let wait = if drain.is_requested() {
            COMMAND_POLL
        } else {
            QUIET_WAIT
        };
        let command = select! {
            recv(asked) -> command => Some(command),
            recv(routing.latency_moves) -> _ => None,
            default(wait) => None,
        };
        match command {
            Some(Ok(Order::Command(StreamCommand::Close)) | Err(_)) => break,
            Some(Ok(Order::Command(StreamCommand::Drain))) => drain.request(),
            Some(Ok(Order::Reshape {
                rate: wanted,
                answer,
            })) => {
                drain = Drain::new();
                let status = if wanted == rate {
                    0
                } else {
                    hardware.reformat(wanted)
                };
                if status == 0 && wanted != rate {
                    rate = wanted;
                    tick = tick_of(rate);
                    let _ = announced.send(StreamEvent::FormatChanged {
                        spec: shape_of(rate),
                        words: Words::Whole,
                    });
                    if state != StreamState::Paused {
                        let _ = announced.send(StreamEvent::StateChanged {
                            from: state,
                            to: StreamState::Paused,
                        });
                        state = StreamState::Paused;
                    }
                }
                let _ = answer.send(status);
            }
            Some(Ok(Order::Command(StreamCommand::SetActive(active)))) => {
                let status = if active {
                    hardware.start()
                } else {
                    hardware.stop()
                };
                if status != 0 {
                    let op = if active {
                        Operation::Start
                    } else {
                        Operation::Pause
                    };
                    let _ = announced.send(StreamEvent::Refused {
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
                let _ = announced.send(StreamEvent::StateChanged {
                    from: state,
                    to: next,
                });
                state = next;
            }
            None => {}
        }
    }
}

fn tick_of(rate: SampleRate) -> Ratio {
    Ratio {
        numer: NonZeroU32::MIN,
        denom: NonZeroU32::new(rate.hz()).unwrap_or(NonZeroU32::MIN),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use parking_lot::Mutex;
    use resonate_audio::{LatencyRequest, MediaRole};

    use super::*;

    const WAIT: Duration = Duration::from_secs(5);
    const SHORT: Duration = Duration::from_millis(300);

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Call {
        Start,
        Stop,
        Reformat(SampleRate),
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
        disposed: Arc<AtomicUsize>,
        _rendering: Arc<Mutex<Box<Rendering>>>,
    }

    impl Hardware for Fake {
        fn start(&self) -> i32 {
            self.answer(Call::Start)
        }

        fn stop(&self) -> i32 {
            self.answer(Call::Stop)
        }

        fn reformat(&self, rate: SampleRate) -> i32 {
            self.answer(Call::Reformat(rate))
        }
    }

    impl Fake {
        fn answer(&self, call: Call) -> i32 {
            self.calls.lock().push(call);
            self.refusing
                .filter(|(refused, _)| *refused == call)
                .map_or(0, |(_, status)| status)
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            self.disposed.fetch_add(1, Ordering::AcqRel);
        }
    }

    struct Opened {
        stream: SinkStream,
        routing: Arc<Routing>,
        calls: Arc<Mutex<Vec<Call>>>,
        disposed: Arc<AtomicUsize>,
        empty: Arc<AtomicBool>,
        rendering: Arc<Mutex<Box<Rendering>>>,
    }

    fn request(routing: &Routing) -> StreamRequest {
        StreamRequest {
            target: None,
            spec: StreamSpec::new(routing.rate(), ChannelLayout::Stereo, SampleFormat::F32),
            latency: LatencyRequest::Auto,
            role: MediaRole::Music,
            media_name: String::new(),
            force_graph_rate: false,
            no_convert: false,
            realtime: false,
        }
    }

    fn opened_refusing(refusing: Option<(Call, i32)>) -> Opened {
        let routing = Arc::new(Routing::new());
        let calls = Arc::new(Mutex::new(Vec::new()));
        let disposed = Arc::new(AtomicUsize::new(0));
        let (flag, flagged) = bounded(1);
        let stream = launch(&routing, &request(&routing), Box::new(Silent), WAIT, {
            let calls = Arc::clone(&calls);
            let disposed = Arc::clone(&disposed);
            move |_, rendering| {
                let rendering = Arc::new(Mutex::new(rendering));
                let _ = flag.send(Arc::clone(&rendering));
                Ok(Fake {
                    calls,
                    refusing,
                    disposed,
                    _rendering: rendering,
                })
            }
        })
        .expect("a stream");
        let rendering = flagged.recv_timeout(WAIT).expect("the rendering");
        let empty = Arc::clone(&rendering.lock().empty);
        Opened {
            stream,
            routing,
            calls,
            disposed,
            empty,
            rendering,
        }
    }

    fn opened() -> Opened {
        opened_refusing(None)
    }

    fn event(opened: &Opened) -> StreamEvent {
        opened.stream.events().recv_timeout(WAIT).expect("an event")
    }

    fn quiet(opened: &Opened) {
        assert!(opened.stream.events().recv_timeout(SHORT).is_err());
    }

    #[test]
    fn a_stream_opens_announcing_the_format_it_was_made_in() {
        let opened = opened();

        assert_eq!(
            event(&opened),
            StreamEvent::FormatChanged {
                spec: StreamSpec::new(
                    opened.routing.rate(),
                    ChannelLayout::Stereo,
                    SampleFormat::F32
                ),
                words: Words::Whole,
            }
        );
        quiet(&opened);
    }

    #[test]
    fn a_request_for_another_format_is_refused_before_anything_is_made() {
        let routing = Arc::new(Routing::new());
        let mut asked = request(&routing);
        asked.spec = StreamSpec::new(
            SampleRate::HZ_44100,
            ChannelLayout::Stereo,
            SampleFormat::F32,
        );

        let made = launch(
            &routing,
            &asked,
            Box::new(Silent),
            WAIT,
            |_, _| -> Result<Fake> {
                panic!("a unit was made for a format the route does not run")
            },
        );

        assert!(matches!(made, Err(Error::FormatRefused)));
    }

    #[test]
    fn a_unit_that_would_not_open_is_the_opens_error_and_no_stream() {
        let routing = Arc::new(Routing::new());

        let made = launch(
            &routing,
            &request(&routing),
            Box::new(Silent),
            WAIT,
            |_, _| -> Result<Fake> {
                Err(Error::Backend {
                    op: Operation::Open,
                    source: io::ErrorKind::NotFound.into(),
                })
            },
        );

        assert!(matches!(
            made,
            Err(Error::Backend {
                op: Operation::Open,
                ref source
            }) if source.kind() == io::ErrorKind::NotFound
        ));
    }

    #[test]
    fn an_open_that_never_answers_times_out_and_the_late_unit_is_disposed_by_its_own_thread() {
        let routing = Arc::new(Routing::new());
        let disposed = Arc::new(AtomicUsize::new(0));
        let late = Arc::clone(&disposed);

        let made = launch(
            &routing,
            &request(&routing),
            Box::new(Silent),
            Duration::from_millis(50),
            move |_, rendering| {
                thread::sleep(Duration::from_millis(400));
                Ok(Fake {
                    calls: Arc::default(),
                    refusing: None,
                    disposed: late,
                    _rendering: Arc::new(Mutex::new(rendering)),
                })
            },
        );

        assert!(matches!(
            made,
            Err(Error::Backend {
                op: Operation::Open,
                ref source
            }) if source.kind() == io::ErrorKind::TimedOut
        ));
        let until = Instant::now() + WAIT;
        while disposed.load(Ordering::Acquire) == 0 && Instant::now() < until {
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(disposed.load(Ordering::Acquire), 1);
    }

    #[test]
    fn a_thread_that_stopped_before_answering_is_a_stopped_loop() {
        let routing = Arc::new(Routing::new());

        let made = launch(
            &routing,
            &request(&routing),
            Box::new(Silent),
            WAIT,
            |_, _| -> Result<Fake> {
                std::panic::resume_unwind(Box::new("the unit's thread died"))
            },
        );

        assert!(matches!(made, Err(Error::LoopStopped)));
    }

    #[test]
    fn activating_starts_the_unit_and_deactivating_stops_it_each_announced_once() {
        let opened = opened();
        event(&opened);

        opened.stream.set_active(true).expect("a command");
        assert_eq!(
            event(&opened),
            StreamEvent::StateChanged {
                from: StreamState::Paused,
                to: StreamState::Streaming
            }
        );
        opened.stream.set_active(false).expect("a command");
        assert_eq!(
            event(&opened),
            StreamEvent::StateChanged {
                from: StreamState::Streaming,
                to: StreamState::Paused
            }
        );

        assert_eq!(*opened.calls.lock(), [Call::Start, Call::Stop]);
        quiet(&opened);
    }

    #[test]
    fn a_refused_start_is_told_with_its_status_and_leaves_the_stream_failed() {
        let opened = opened_refusing(Some((Call::Start, 560_557_684)));
        event(&opened);

        opened.stream.set_active(true).expect("a command");
        assert_eq!(
            event(&opened),
            StreamEvent::Refused {
                from: StreamState::Paused,
                op: Operation::Start,
                status: 560_557_684
            }
        );
        opened.stream.set_active(false).expect("a command");

        assert_eq!(
            event(&opened),
            StreamEvent::StateChanged {
                from: StreamState::Failed,
                to: StreamState::Paused
            }
        );
    }

    #[test]
    fn a_refused_stop_is_told_as_a_refused_pause() {
        let opened = opened_refusing(Some((Call::Stop, -66_635)));
        event(&opened);
        opened.stream.set_active(true).expect("a command");
        event(&opened);

        opened.stream.set_active(false).expect("a command");

        assert_eq!(
            event(&opened),
            StreamEvent::Refused {
                from: StreamState::Streaming,
                op: Operation::Pause,
                status: -66_635
            }
        );
    }

    #[test]
    fn a_drain_over_a_source_with_something_left_is_not_settled() {
        let opened = opened();
        event(&opened);

        opened.stream.drain().expect("a command");

        quiet(&opened);
        assert_eq!(*opened.calls.lock(), []);
    }

    #[test]
    fn a_drain_settles_once_the_source_is_empty_stopping_the_unit_and_saying_so_once() {
        let opened = opened();
        event(&opened);
        opened.empty.store(true, Ordering::Release);

        opened.stream.drain().expect("a command");

        assert_eq!(event(&opened), StreamEvent::Drained);
        assert_eq!(*opened.calls.lock(), [Call::Stop]);
        quiet(&opened);
    }

    #[test]
    fn a_drain_waits_the_routes_latency_after_the_source_empties() {
        let opened = opened();
        event(&opened);
        opened.routing.route_changed(48_000, 400.0);
        opened.empty.store(true, Ordering::Release);
        let asked_at = Instant::now();

        opened.stream.drain().expect("a command");

        assert_eq!(event(&opened), StreamEvent::Drained);
        assert!(
            asked_at.elapsed() >= Duration::from_millis(350),
            "drained after {:?}",
            asked_at.elapsed()
        );
    }

    #[test]
    fn a_moved_latency_reaches_the_streams_clock_without_a_command() {
        let opened = opened();
        event(&opened);
        assert_eq!(opened.stream.latency(), Frames(0));

        opened.routing.route_changed(48_000, 10.0);

        let until = Instant::now() + WAIT;
        while opened.stream.latency() == Frames(0) && Instant::now() < until {
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(opened.stream.latency(), Frames(480));
    }

    #[test]
    fn closing_returns_only_once_the_unit_is_disposed() {
        let opened = opened();
        event(&opened);
        let disposed = Arc::clone(&opened.disposed);

        opened.stream.close().expect("a closed stream");

        assert_eq!(disposed.load(Ordering::Acquire), 1);
    }

    #[test]
    fn a_stream_dropped_without_closing_still_has_its_unit_disposed() {
        let opened = opened();
        event(&opened);
        let disposed = Arc::clone(&opened.disposed);

        drop(opened.stream);

        let until = Instant::now() + WAIT;
        while disposed.load(Ordering::Acquire) == 0 && Instant::now() < until {
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(disposed.load(Ordering::Acquire), 1);
    }

    fn mark_heard(opened: &Opened) -> u8 {
        let mut bytes = [0_u8; 4];
        opened.rendering.lock().fill(&mut bytes);
        bytes[0]
    }

    fn refused_for(reshaped: Reshaped) -> Option<Unreshapable> {
        match reshaped {
            Reshaped::Refused { because, .. } => Some(because),
            Reshaped::Kept | Reshaped::Lost(_) => None,
        }
    }

    #[test]
    fn a_stream_asked_for_the_shape_it_holds_takes_the_new_source_and_the_unit_is_left_alone() {
        let opened = opened();
        event(&opened);

        let reshaped = opened
            .stream
            .reshape(&request(&opened.routing), Box::new(Marked(7)));

        assert!(matches!(reshaped, Reshaped::Kept));
        assert_eq!(mark_heard(&opened), 7);
        assert_eq!(*opened.calls.lock(), []);
        quiet(&opened);
    }

    #[test]
    fn a_drain_asked_before_a_reshape_is_called_off_so_the_new_source_is_not_drained() {
        let opened = opened();
        event(&opened);
        opened.routing.route_changed(48_000, 100.0);
        opened.empty.store(true, Ordering::Release);

        opened.stream.drain().expect("a command");
        let reshaped = opened
            .stream
            .reshape(&request(&opened.routing), Box::new(Marked(7)));

        assert!(matches!(reshaped, Reshaped::Kept));
        quiet(&opened);
        assert_eq!(*opened.calls.lock(), []);
    }

    #[test]
    fn a_stream_that_drained_is_reshaped_and_started_again_rather_than_reopened() {
        let opened = opened();
        event(&opened);
        opened.empty.store(true, Ordering::Release);
        opened.stream.drain().expect("a command");
        assert_eq!(event(&opened), StreamEvent::Drained);

        let reshaped = opened
            .stream
            .reshape(&request(&opened.routing), Box::new(Marked(7)));
        opened.stream.set_active(true).expect("a command");

        assert!(matches!(reshaped, Reshaped::Kept));
        assert_eq!(
            event(&opened),
            StreamEvent::StateChanged {
                from: StreamState::Paused,
                to: StreamState::Streaming
            }
        );
        assert_eq!(*opened.calls.lock(), [Call::Stop, Call::Start]);
        assert_eq!(mark_heard(&opened), 7);
    }

    #[test]
    fn a_rate_the_route_took_since_the_unit_opened_reformats_the_unit_and_says_so() {
        let opened = opened();
        event(&opened);
        opened.stream.set_active(true).expect("a command");
        event(&opened);
        opened.routing.route_changed(44_100, 0.0);

        let reshaped = opened
            .stream
            .reshape(&request(&opened.routing), Box::new(Marked(7)));

        assert!(matches!(reshaped, Reshaped::Kept));
        assert_eq!(
            event(&opened),
            StreamEvent::FormatChanged {
                spec: StreamSpec::new(
                    SampleRate::HZ_44100,
                    ChannelLayout::Stereo,
                    SampleFormat::F32
                ),
                words: Words::Whole,
            }
        );
        assert_eq!(
            event(&opened),
            StreamEvent::StateChanged {
                from: StreamState::Streaming,
                to: StreamState::Paused
            }
        );
        assert_eq!(
            *opened.calls.lock(),
            [Call::Start, Call::Reformat(SampleRate::HZ_44100)]
        );
        assert_eq!(mark_heard(&opened), 7);

        opened.stream.set_active(true).expect("a command");
        assert_eq!(
            event(&opened),
            StreamEvent::StateChanged {
                from: StreamState::Paused,
                to: StreamState::Streaming
            }
        );
    }

    #[test]
    fn a_rate_already_held_is_not_reformatted_again() {
        let opened = opened();
        event(&opened);
        opened.routing.route_changed(44_100, 0.0);
        let again = request(&opened.routing);

        assert!(matches!(
            opened.stream.reshape(&again, Box::new(Marked(7))),
            Reshaped::Kept
        ));
        event(&opened);
        assert_eq!(mark_heard(&opened), 7);
        assert!(matches!(
            opened.stream.reshape(&again, Box::new(Marked(8))),
            Reshaped::Kept
        ));

        quiet(&opened);
        assert_eq!(*opened.calls.lock(), [Call::Reformat(SampleRate::HZ_44100)]);
        assert_eq!(mark_heard(&opened), 8);
    }

    #[test]
    fn a_reformat_the_system_refuses_hands_the_source_back_and_announces_nothing() {
        let opened = opened_refusing(Some((Call::Reformat(SampleRate::HZ_44100), -50)));
        event(&opened);
        opened.routing.route_changed(44_100, 0.0);

        let reshaped = opened
            .stream
            .reshape(&request(&opened.routing), Box::new(Marked(7)));

        let Reshaped::Refused {
            mut source,
            because,
        } = reshaped
        else {
            panic!("a unit the system would not reformat was kept");
        };
        let mut bytes = [0_u8; 2];
        source.fill(&mut bytes);
        assert_eq!(because, Unreshapable::FormatUnwritten);
        assert_eq!(bytes, [7, 7]);
        quiet(&opened);
    }

    #[test]
    fn a_shape_other_than_the_routes_is_refused_before_the_unit_is_touched() {
        let opened = opened();
        event(&opened);
        let asked = request(&opened.routing);

        for spec in [
            StreamSpec::new(
                SampleRate::HZ_44100,
                ChannelLayout::Stereo,
                SampleFormat::F32,
            ),
            StreamSpec::new(
                opened.routing.rate(),
                ChannelLayout::Mono,
                SampleFormat::F32,
            ),
            StreamSpec::new(
                opened.routing.rate(),
                ChannelLayout::Stereo,
                SampleFormat::S16,
            ),
        ] {
            let reshaped = opened.stream.reshape(
                &StreamRequest {
                    spec,
                    ..asked.clone()
                },
                Box::new(Marked(7)),
            );
            assert_eq!(refused_for(reshaped), Some(Unreshapable::AnotherFormat));
        }

        assert_eq!(*opened.calls.lock(), []);
        quiet(&opened);
    }
}
