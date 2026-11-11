use std::{fmt::Display, io, sync::Arc, thread, time::Duration};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TryRecvError, bounded, unbounded};
use parking_lot::Mutex;
use resonate_audio::{
    AudioSource, Error, Operation, Reshaped, Reshaper, SinkResult, SinkStream, StreamClock,
    StreamCommand, StreamEvent, StreamRequest, StreamState, Unreshapable, Words,
    handover::{Fed, Handing, fed_from},
    refusal_of,
};
use resonate_core::{Frames, SampleFormat};

const IDLE_POLL: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cycled {
    pub heard: usize,
    pub played: u64,
    pub heard_until: u64,
}

impl Cycled {
    pub const fn has_played_out(self) -> bool {
        self.heard == 0 && self.played >= self.heard_until
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Woken {
    Asked,
    TimedOut,
}

pub trait Hardware {
    type Fault: Display;

    fn wait_for_room(&mut self) -> Woken;
    fn cycle(&mut self) -> Result<Cycled, Self::Fault>;
    fn start(&mut self) -> Result<(), Self::Fault>;
    fn pause(&mut self) -> Result<(), Self::Fault>;
    fn stop(&mut self);
    fn prioritise(&mut self, realtime: bool);
}

pub struct Feeding {
    pub fed: Fed,
    pub clock: Arc<StreamClock>,
}

enum Order {
    Command(StreamCommand),
    Reshaped { realtime: bool },
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

pub const fn words_of(format: SampleFormat) -> Words {
    match format {
        SampleFormat::S24 => Words::Padded,
        SampleFormat::S16 | SampleFormat::S32 | SampleFormat::F32 => Words::Whole,
    }
}

pub fn launch<H, M>(
    request: &StreamRequest,
    source: Box<dyn AudioSource>,
    within: Duration,
    make: M,
) -> SinkResult<SinkStream>
where
    H: Hardware,
    M: FnOnce(Feeding) -> SinkResult<H> + Send + 'static,
{
    let spec = request.spec;
    let realtime = request.realtime;
    let (sending, orders) = unbounded();
    let (announced, events) = unbounded();
    let (ready, started) = bounded(1);
    let (disposed, closed) = bounded(1);
    let (fed, handing) = fed_from(source);
    let clock = Arc::new(StreamClock::behind_by(Frames::ZERO));
    let feeding = Feeding {
        fed,
        clock: Arc::clone(&clock),
    };

    thread::Builder::new()
        .name("resonate-wasapi-stream".into())
        .spawn(move || {
            let mut hardware = match make(feeding) {
                Ok(hardware) => hardware,
                Err(refused) => {
                    let _ = ready.send(Err(refused));
                    return;
                }
            };
            hardware.prioritise(realtime);
            let _ = announced.send(StreamEvent::FormatChanged {
                spec,
                words: words_of(spec.format),
            });
            let _ = ready.send(Ok(()));
            serve(
                &mut hardware,
                &Serving {
                    orders: &orders,
                    announced: &announced,
                },
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
                (Ok(()), false) | (Err(_), true) => Ok(()),
                (Err(_), false) => Err(Error::LoopStopped),
                (Ok(()), true) => let_go_within(&closed, within),
            }
        }),
    )
    .reshaped_by(reshaper(reshaping, sending)))
}

fn let_go_within(closed: &Receiver<()>, within: Duration) -> SinkResult<()> {
    match closed.recv_timeout(within) {
        Ok(()) | Err(RecvTimeoutError::Disconnected) => Ok(()),
        Err(RecvTimeoutError::Timeout) => Err(Error::Backend {
            op: Operation::Close,
            source: io::ErrorKind::TimedOut.into(),
        }),
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

enum Next {
    Order(Order),
    Quiet,
    Unheld,
}

fn next_order(orders: &Receiver<Order>, state: StreamState) -> Next {
    if state == StreamState::Streaming {
        return match orders.try_recv() {
            Ok(order) => Next::Order(order),
            Err(TryRecvError::Empty) => Next::Quiet,
            Err(TryRecvError::Disconnected) => Next::Unheld,
        };
    }
    match orders.recv_timeout(IDLE_POLL) {
        Ok(order) => Next::Order(order),
        Err(RecvTimeoutError::Timeout) => Next::Quiet,
        Err(RecvTimeoutError::Disconnected) => Next::Unheld,
    }
}

fn serve<H: Hardware>(hardware: &mut H, serving: &Serving<'_>) {
    let mut standing = Standing {
        state: StreamState::Paused,
        draining: false,
    };
    loop {
        match next_order(serving.orders, standing.state) {
            Next::Order(order) => match handled(hardware, order, &mut standing, serving) {
                Handled::Went => return,
                Handled::Stays => continue,
            },
            Next::Unheld => {
                hardware.stop();
                return;
            }
            Next::Quiet => {}
        }
        if standing.state != StreamState::Streaming || hardware.wait_for_room() == Woken::TimedOut {
            continue;
        }
        match hardware.cycle() {
            Ok(cycled) => {
                if standing.draining && cycled.has_played_out() {
                    standing.draining = false;
                    let _ = serving.announced.send(StreamEvent::Drained);
                }
            }
            Err(error) => {
                tracing::warn!(%error, "a WASAPI stream failed");
                hardware.stop();
                let _ = serving.announced.send(StreamEvent::StateChanged {
                    from: standing.state,
                    to: StreamState::Failed,
                });
                return;
            }
        }
    }
}

enum Handled {
    Stays,
    Went,
}

fn handled<H: Hardware>(
    hardware: &mut H,
    order: Order,
    standing: &mut Standing,
    serving: &Serving<'_>,
) -> Handled {
    match order {
        Order::Reshaped { realtime } => {
            standing.draining = false;
            hardware.prioritise(realtime);
            Handled::Stays
        }
        Order::Command(StreamCommand::Close) => {
            hardware.stop();
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
                hardware.cycle().and_then(|_| hardware.start())
            } else {
                hardware.pause()
            };
            let to = match (moved, active) {
                (Err(error), _) => {
                    tracing::warn!(%error, "a WASAPI stream would not change state");
                    hardware.stop();
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

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicU64, AtomicUsize, Ordering},
        time::Instant,
    };

    use resonate_audio::{LatencyRequest, MediaRole, SinkId};
    use resonate_core::{ChannelLayout, SampleRate, StreamSpec};

    use super::*;

    const WAIT: Duration = Duration::from_secs(5);
    const SHORT: Duration = Duration::from_millis(300);
    const A_DEVICE_PERIOD: Duration = Duration::from_millis(1);
    const LETTING_GO_TAKES: Duration = Duration::from_millis(100);
    const FRAME_BYTES: usize = 8;
    const FRAMES_A_CYCLE: usize = 4;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Call {
        Cycle,
        Start,
        Pause,
        Stop,
        Prioritise(bool),
    }

    #[derive(Debug)]
    struct Refused(Call);

    impl Display for Refused {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(formatter, "the fake refused {:?}", self.0)
        }
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

    struct Lasting {
        bytes_left: usize,
    }

    impl AudioSource for Lasting {
        fn fill(&mut self, dst: &mut [u8]) -> usize {
            let filled = dst.len().min(self.bytes_left);
            self.bytes_left -= filled;
            filled
        }
    }

    #[derive(Clone, Default)]
    struct Seen {
        calls: Arc<Mutex<Vec<Call>>>,
        refusing: Arc<Mutex<Option<Call>>>,
        played: Arc<AtomicU64>,
        heard_last: Arc<Mutex<Vec<u8>>>,
        disposed: Arc<AtomicUsize>,
    }

    impl Seen {
        fn calls_but_cycles(&self) -> Vec<Call> {
            self.calls
                .lock()
                .iter()
                .copied()
                .filter(|call| *call != Call::Cycle)
                .collect()
        }

        fn let_go(&self) -> bool {
            self.disposed.load(Ordering::Acquire) == 1
        }
    }

    struct Fake {
        fed: Fed,
        seen: Seen,
        written: u64,
        heard_until: u64,
    }

    impl Fake {
        fn answer(&self, call: Call) -> Result<(), Refused> {
            self.seen.calls.lock().push(call);
            match *self.seen.refusing.lock() {
                Some(refused) if refused == call => Err(Refused(call)),
                _ => Ok(()),
            }
        }
    }

    impl Hardware for Fake {
        type Fault = Refused;

        fn wait_for_room(&mut self) -> Woken {
            thread::sleep(A_DEVICE_PERIOD);
            Woken::Asked
        }

        fn cycle(&mut self) -> Result<Cycled, Refused> {
            self.answer(Call::Cycle)?;
            let mut bytes = [0_u8; FRAMES_A_CYCLE * FRAME_BYTES];
            let heard = self.fed.source().fill(&mut bytes) / FRAME_BYTES;
            if heard > 0 {
                self.heard_until = self.written + heard as u64;
                *self.seen.heard_last.lock() = bytes.to_vec();
            }
            self.written += FRAMES_A_CYCLE as u64;
            Ok(Cycled {
                heard,
                played: self.seen.played.load(Ordering::Acquire),
                heard_until: self.heard_until,
            })
        }

        fn start(&mut self) -> Result<(), Refused> {
            self.answer(Call::Start)
        }

        fn pause(&mut self) -> Result<(), Refused> {
            self.answer(Call::Pause)?;
            self.written = 0;
            self.heard_until = 0;
            Ok(())
        }

        fn stop(&mut self) {
            let _ = self.answer(Call::Stop);
        }

        fn prioritise(&mut self, realtime: bool) {
            let _ = self.answer(Call::Prioritise(realtime));
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            thread::sleep(LETTING_GO_TAKES);
            self.seen.disposed.fetch_add(1, Ordering::AcqRel);
        }
    }

    struct Opened {
        stream: SinkStream,
        seen: Seen,
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

    fn fake(feeding: Feeding, seen: Seen) -> Fake {
        Fake {
            fed: feeding.fed,
            seen,
            written: 0,
            heard_until: 0,
        }
    }

    fn opened_with(source: Box<dyn AudioSource>, format: SampleFormat) -> Opened {
        let seen = Seen::default();

        let stream = launch(&request(format), source, WAIT, {
            let seen = seen.clone();
            move |feeding| Ok(fake(feeding, seen))
        })
        .expect("a stream");

        Opened { stream, seen }
    }

    fn opened() -> Opened {
        opened_with(Box::new(Silent), SampleFormat::F32)
    }

    fn streaming(source: Box<dyn AudioSource>) -> Opened {
        let opened = opened_with(source, SampleFormat::F32);
        let _ = event(&opened);

        opened.stream.set_active(true).expect("started");

        assert_eq!(
            event(&opened),
            StreamEvent::StateChanged {
                from: StreamState::Paused,
                to: StreamState::Streaming
            }
        );
        opened
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
        let opened = opened_with(Box::new(Silent), SampleFormat::S24);

        assert_eq!(
            event(&opened),
            StreamEvent::FormatChanged {
                spec: request(SampleFormat::S24).spec,
                words: Words::Padded,
            }
        );
        quiet(&opened);
        assert_eq!(opened.seen.calls_but_cycles(), [Call::Prioritise(true)]);
    }

    #[test]
    fn a_stream_fills_the_buffer_before_it_starts_and_starts_once_however_often_it_is_told_to() {
        let opened = streaming(Box::new(Silent));

        opened.stream.set_active(true).expect("started again");
        opened.stream.set_active(false).expect("paused");

        assert_eq!(
            event(&opened),
            StreamEvent::StateChanged {
                from: StreamState::Streaming,
                to: StreamState::Paused
            }
        );
        assert_eq!(
            opened.seen.calls.lock().get(..3),
            Some([Call::Prioritise(true), Call::Cycle, Call::Start].as_slice())
        );
        assert_eq!(
            opened.seen.calls_but_cycles(),
            [Call::Prioritise(true), Call::Start, Call::Pause]
        );
    }

    #[test]
    fn a_paused_stream_cycles_no_more() {
        let opened = streaming(Box::new(Silent));
        opened.stream.set_active(false).expect("paused");
        let _ = event(&opened);

        let cycles = opened.seen.calls.lock().len();
        thread::sleep(SHORT);

        assert_eq!(opened.seen.calls.lock().len(), cycles);
    }

    #[test]
    fn a_drain_is_told_once_the_device_has_played_past_the_last_frame_music_reached() {
        let twelve_frames = Lasting {
            bytes_left: 12 * FRAME_BYTES,
        };
        let opened = streaming(Box::new(twelve_frames));

        opened.stream.drain().expect("asked to drain");
        quiet(&opened);
        opened.seen.played.store(11, Ordering::Release);
        quiet(&opened);
        opened.seen.played.store(12, Ordering::Release);

        assert_eq!(event(&opened), StreamEvent::Drained);
        quiet(&opened);
    }

    #[test]
    fn a_pause_calls_off_a_drain() {
        let opened = streaming(Box::new(Silent));

        opened.stream.drain().expect("asked to drain");
        opened.stream.set_active(false).expect("paused");
        let _ = event(&opened);
        opened.seen.played.store(u64::MAX, Ordering::Release);
        opened.stream.set_active(true).expect("started");
        let _ = event(&opened);

        quiet(&opened);
    }

    #[test]
    fn a_stream_asked_for_the_shape_it_holds_takes_the_new_source_its_priority_and_calls_off_a_drain()
     {
        let opened = streaming(Box::new(Silent));
        opened.stream.drain().expect("asked to drain");
        let lowered = StreamRequest {
            realtime: false,
            ..request(SampleFormat::F32)
        };

        let reshaped = opened.stream.reshape(&lowered, Box::new(Marked(0x5A)));

        assert!(matches!(reshaped, Reshaped::Kept));
        wait_until(|| *opened.seen.heard_last.lock() == [0x5A; FRAMES_A_CYCLE * FRAME_BYTES]);
        opened.seen.played.store(u64::MAX, Ordering::Release);
        quiet(&opened);
        assert_eq!(
            opened.seen.calls_but_cycles(),
            [Call::Prioritise(true), Call::Start, Call::Prioritise(false)]
        );
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
    fn a_refused_start_fails_the_stream_and_lets_the_hardware_go() {
        let opened = opened();
        let _ = event(&opened);
        *opened.seen.refusing.lock() = Some(Call::Start);

        opened.stream.set_active(true).expect("asked");

        assert_eq!(
            event(&opened),
            StreamEvent::StateChanged {
                from: StreamState::Paused,
                to: StreamState::Failed
            }
        );
        wait_until(|| opened.seen.let_go());
        assert!(matches!(
            opened.stream.set_active(true),
            Err(Error::LoopStopped)
        ));
        assert!(matches!(
            opened
                .stream
                .reshape(&request(SampleFormat::F32), Box::new(Silent)),
            Reshaped::Refused {
                because: Unreshapable::NoStream,
                ..
            }
        ));
    }

    #[test]
    fn a_cycle_that_fails_while_streaming_fails_the_stream_once_and_lets_the_hardware_go() {
        let opened = streaming(Box::new(Silent));

        *opened.seen.refusing.lock() = Some(Call::Cycle);

        assert_eq!(
            event(&opened),
            StreamEvent::StateChanged {
                from: StreamState::Streaming,
                to: StreamState::Failed
            }
        );
        quiet(&opened);
        assert!(opened.seen.let_go());
        assert_eq!(opened.seen.calls_but_cycles().last(), Some(&Call::Stop));
    }

    #[test]
    fn closing_returns_only_once_the_hardware_is_let_go() {
        let opened = opened();
        let seen = opened.seen.clone();

        opened.stream.close().expect("closed");

        assert!(seen.let_go());
        assert_eq!(seen.calls_but_cycles().last(), Some(&Call::Stop));
    }

    #[test]
    fn closing_a_streaming_stream_returns_only_once_the_hardware_is_let_go() {
        let opened = streaming(Box::new(Marked(1)));
        let seen = opened.seen.clone();

        opened.stream.close().expect("closed");

        assert!(seen.let_go());
        assert_eq!(seen.calls_but_cycles().last(), Some(&Call::Stop));
    }

    #[test]
    fn closing_a_stream_that_already_failed_returns_at_once() {
        let opened = opened();
        let _ = event(&opened);
        *opened.seen.refusing.lock() = Some(Call::Start);
        opened.stream.set_active(true).expect("asked");
        let _ = event(&opened);
        wait_until(|| opened.seen.let_go());

        let seen = opened.seen.clone();
        opened.stream.close().expect("closed");

        assert!(seen.let_go());
    }

    #[test]
    fn a_streaming_stream_dropped_unclosed_stops_and_lets_the_hardware_go() {
        let opened = streaming(Box::new(Marked(1)));
        let seen = opened.seen.clone();

        drop(opened);

        wait_until(|| seen.let_go());
        assert_eq!(seen.calls_but_cycles().last(), Some(&Call::Stop));
    }

    #[test]
    fn a_paused_stream_dropped_unclosed_stops_and_lets_the_hardware_go() {
        let opened = opened();
        let seen = opened.seen.clone();

        drop(opened);

        wait_until(|| seen.let_go());
        assert_eq!(seen.calls_but_cycles().last(), Some(&Call::Stop));
    }

    #[test]
    fn an_open_that_never_answers_times_out_and_the_late_hardware_is_let_go_by_its_own_thread() {
        let seen = Seen::default();
        let (release, released) = bounded::<()>(1);

        let made = launch(
            &request(SampleFormat::F32),
            Box::new(Silent),
            Duration::from_millis(50),
            {
                let seen = seen.clone();
                move |feeding| {
                    let _ = released.recv_timeout(WAIT);
                    Ok(fake(feeding, seen))
                }
            },
        );

        assert!(matches!(made, Err(Error::OpenUnanswered)));
        release.send(()).expect("the opening thread is waiting");
        wait_until(|| seen.let_go());
    }

    #[test]
    fn hardware_that_would_not_open_is_the_opens_own_error() {
        let made = launch(
            &request(SampleFormat::F32),
            Box::new(Silent),
            WAIT,
            |_| -> SinkResult<Fake> { Err(Error::NoSink) },
        );

        assert!(matches!(made, Err(Error::NoSink)));
    }
}
