use std::{
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Select, Sender, TryRecvError};
use resonate_codec::{
    BoxLayout, DecodeStatus, Decoder, MediaInfo, Packing, ProfileBuilder, ReplayGain, Sources,
    probe_boxes,
};
use resonate_core::{
    AppliedGain, AudioBuffer, FrameSpan, Frames, Gain, MeasuredGain, MediaLocation, RtFault, Span,
    StreamSpec, TrackHints, TrackId,
};
use resonate_dsp::Chain;
use resonate_pipewire::{
    LatencyRequest, MediaRole, NodeName, SinkChange, SinkId, SinkInfo, SinkStream, StreamEvent,
    StreamRequest, StreamState,
};

use crate::{
    Backend, Command, CommandKind, EngineConfig, Error, Event, OutputMode, OutputPlan,
    OutputSettings, OutputStatus, PlaybackState, PlayerState, Published, RepeatMode,
    ReplayGainMode, Reply, Request, Result, Seeks, SkipUnderRepeat, Sleeping, StreamDigest, Tapped,
    Tapping, TrackState, TransportState,
    pipeline::{Decoded, packs_again, plan_for, plan_output, resolve_replay_gain},
    queue::{Queue, Queued, Removal},
    ring::{RingConsumer, RingMonitor, RingProducer, ring},
};

const SINK_TIMEOUT: Duration = Duration::from_secs(2);
const SINK_REFRESH_BUDGET: Duration = Duration::from_millis(50);
const SHORTEST_TICK: Duration = Duration::from_millis(4);
const PUBLISH_TICK: Duration = Duration::from_millis(16);
const IDLE_TICK: Duration = Duration::from_millis(100);
const CHAIN_BLOCK: usize = 1024;
const MIN_RING_FRAMES: u64 = 8_192;
const BLOCKS_A_RING_HOLDS: u64 = 2;
const LARGEST_RING: u64 = 64 * 1024 * 1024;
const DISCARD_TIMEOUT: Duration = Duration::from_millis(500);
const MAX_RENEGOTIATIONS: u8 = 3;
const GRAPH_BACK_WITHIN: Duration = Duration::from_secs(10);

struct Track {
    id: TrackId,
    location: MediaLocation,
    span: Option<FrameSpan>,
    decoder: Decoder,
    info: MediaInfo,
    layout: Option<BoxLayout>,
    hints: TrackHints,
    replay_gain: AppliedGain,
    decoded: AudioBuffer,
    decoded_at: usize,
    profile: ProfileBuilder,
    profiled_from: Frames,
    sampled: Option<Frames>,
    published: Option<usize>,
}

impl Track {
    fn open(
        id: TrackId,
        location: &MediaLocation,
        span: Option<FrameSpan>,
        sources: &Sources,
        config: &EngineConfig,
    ) -> resonate_codec::Result<Self> {
        let (decoder, info) = match span {
            Some(span) => Decoder::open_span(sources, location, span)?,
            None => Decoder::open(sources, location)?,
        };
        let hints = sources.hints(location, span);
        let replay_gain = levelled(config, &info, hints);
        Ok(Self {
            id,
            location: location.clone(),
            span,
            hints,
            decoded: AudioBuffer::empty(info.spec),
            layout: inspected(sources, location),
            profile: ProfileBuilder::new(info.spec.rate),
            decoder,
            info,
            replay_gain,
            decoded_at: 0,
            profiled_from: Frames::ZERO,
            sampled: None,
            published: None,
        })
    }

    const fn source(&self) -> StreamSpec {
        self.info.spec
    }

    fn decoded(&self) -> Decoded {
        Decoded::of(&self.info, self.hints.lowpass)
    }

    fn undecoded(&self) -> usize {
        self.decoded.frames().saturating_sub(self.decoded_at)
    }

    fn rewind_carry(&mut self) {
        self.decoded.set_frames(0);
        self.decoded_at = 0;
    }

    fn sample_packet(&mut self) {
        let Some(packet) = self.decoder.last_packet() else {
            return;
        };
        if self.sampled == Some(packet.at) {
            return;
        }
        self.sampled = Some(packet.at);
        self.profile.push(packet);
    }

    fn restart_profile(&mut self) {
        self.profile = ProfileBuilder::new(self.info.spec.rate);
        self.profiled_from = self.decoder.position();
        self.sampled = None;
        self.published = None;
    }

    fn digest(&self, mode: ReplayGainMode) -> StreamDigest {
        StreamDigest {
            track: self.id,
            location: self.location.clone(),
            span: self.span,
            info: self.info.clone(),
            layout: self.layout.clone(),
            replay_gain_mode: mode,
            replay_gain: self.replay_gain,
            profiled_from: self.profiled_from,
            decoded: self.decoder.position(),
            packet: self.decoder.last_packet(),
            profile: self.profile.finish(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Unanswered {
    NothingPulling,
    GraphStalled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tail {
    Unasked,
    Asked,
    Played,
}

fn carried_samples(chain: &Chain, stream: StreamSpec) -> usize {
    chain.max_output_frames() * stream.channel_count().get() as usize
}

struct Output {
    sink: SinkId,
    bound: NodeName,
    plan: OutputPlan,
    chain: Chain,
    widened: Vec<f64>,
    carrier: Vec<f64>,
    staged: AudioBuffer,
    producer: RingProducer,
    monitor: RingMonitor,
    consumer: Option<RingConsumer>,
    stream: Option<SinkStream>,
    tapping: Option<Tapping>,
    capacity: usize,
    status: OutputStatus,
    draining: Option<usize>,
    ended: bool,
    deaf: bool,
    tail: Tail,
    silent_until: Option<Instant>,
    discarding_since: Option<Instant>,
    settles_into: Option<OutputPlan>,
}

impl Output {
    fn open(
        track: &Track,
        sink: &SinkInfo,
        config: &EngineConfig,
        target: Option<StreamSpec>,
        listening: &Arc<AtomicBool>,
    ) -> Result<Self> {
        let source = track.source();
        let decoded = track.decoded();
        let plan = match target {
            Some(target) => plan_for(decoded, &sink.name, target, config, track.replay_gain),
            None => plan_output(decoded, sink, config, track.replay_gain),
        };

        let chain = plan
            .build_chain(source, config, CHAIN_BLOCK)
            .map_err(|error| Error::Convert {
                track: track.id,
                source_spec: source,
                sink_spec: plan.stream,
                source: error,
            })?;

        let capacity = ring_capacity(config.buffer, plan.stream, chain.max_output_frames());
        let (producer, consumer, monitor) = ring(plan.stream, capacity, plan.silence());

        let widened = vec![0.0; CHAIN_BLOCK * source.channel_count().get() as usize];
        let carrier = vec![0.0; carried_samples(&chain, plan.stream)];
        let staged = AudioBuffer::silence(plan.stream, chain.max_output_frames());
        let tapping = match plan.packing {
            Packing::Samples => Some(Tapping::new(
                plan.stream.rate,
                capacity.get() as usize,
                listening,
            )),
            Packing::DopMarked(_) => None,
        };

        Ok(Self {
            sink: sink.id,
            bound: sink.name.clone(),
            status: OutputStatus {
                sink: sink.id,
                negotiated: plan.stream,
                mode: plan.mode,
                latency: Frames::ZERO,
                underruns: 0,
                went_without: Frames::ZERO,
            },
            plan,
            chain,
            widened,
            carrier,
            staged,
            producer,
            monitor,
            consumer: Some(consumer),
            stream: None,
            tapping,
            capacity: capacity.get() as usize,
            draining: None,
            ended: false,
            deaf: false,
            tail: Tail::Unasked,
            silent_until: None,
            discarding_since: None,
            settles_into: None,
        })
    }

    fn take_chain(&mut self, chain: Chain, plan: OutputPlan) -> OutputStatus {
        if self.draining.is_none() {
            self.carrier
                .resize(carried_samples(&chain, plan.stream), 0.0);
            self.staged.set_frames(chain.max_output_frames());
        }
        self.chain = chain;
        self.status.mode = plan.mode;
        self.plan = plan;
        self.settles_into = None;
        self.status
    }

    fn hand_on_what_the_chain_holds(&mut self) -> bool {
        let held = self.chain.max_flush_frames();
        if held == 0 {
            return true;
        }
        if self.producer.free_frames() < held {
            return false;
        }
        let frames = match self.chain.flush(&mut self.carrier) {
            Ok(frames) => frames,
            Err(error) => {
                tracing::error!(%error, "the chain's held frames did not fit the carrier; they were dropped");
                return true;
            }
        };
        self.stage(frames);
        let written = self.producer.write(&self.staged);
        if let Some(tapping) = self.tapping.as_mut() {
            tapping.record(&self.staged, 0, written);
        }
        true
    }

    fn stage(&mut self, frames: usize) {
        let channels = self.plan.stream.channel_count().get() as usize;
        self.staged.set_frames(frames);
        let carried = self
            .carrier
            .get(..frames.saturating_mul(channels))
            .unwrap_or_default();
        self.staged.data_mut().write_f64(carried);
    }

    fn buffered(&self) -> usize {
        self.capacity.saturating_sub(self.producer.free_frames())
    }

    fn tapped(&self) -> Tapped {
        self.tapping
            .as_ref()
            .map_or(Tapped::Markers, Tapping::tapped)
    }

    fn hear_the_tap(&self, moving: bool) {
        let Some(tapping) = self.tapping.as_ref() else {
            return;
        };
        let downstream = (self.buffered() as u64).saturating_add(self.latency().get());
        tapping.hear(downstream, moving && self.stream.is_some());
    }

    fn primed(&self) -> bool {
        self.ended || self.buffered().saturating_mul(2) >= self.capacity
    }

    fn wants_more(&self) -> bool {
        if self.draining.is_some() {
            return true;
        }
        if self.ended || self.producer.is_discarding() {
            return false;
        }
        self.producer.free_frames() >= self.chain.max_output_frames()
    }

    fn holds_a_live_stream(&self) -> bool {
        self.stream.is_some()
            && !self.ended
            && self.draining.is_none()
            && !self.producer.is_discarding()
    }

    fn latency(&self) -> Frames {
        self.stream
            .as_ref()
            .map(SinkStream::latency)
            .unwrap_or(Frames::ZERO)
    }

    fn end(&mut self) {
        self.ended = true;
        self.producer.finish();
    }

    fn ask_to_drain(&mut self) {
        if self.tail != Tail::Unasked {
            return;
        }
        let Some(stream) = self.stream.as_ref() else {
            return;
        };
        if let Err(error) = stream.drain() {
            tracing::debug!(%error, "the graph would not take a drain request");
        }
        self.tail = Tail::Asked;
    }

    fn tail_is_out(&mut self) -> bool {
        if self.tail == Tail::Played {
            return true;
        }
        let played_by = self.latency().to_duration(self.plan.stream.rate);
        let until = *self.silent_until.get_or_insert(Instant::now() + played_by);
        Instant::now() >= until
    }

    fn close(&mut self) {
        let Some(stream) = self.stream.take() else {
            return;
        };
        let _ = stream.set_active(false);
        if let Err(error) = stream.close() {
            tracing::warn!(%error, "the sink stream did not close cleanly");
        }
    }
}

pub struct Engine {
    config: EngineConfig,
    sources: Arc<Sources>,
    backend: Box<dyn Backend>,
    commands: Receiver<Request>,
    events: Sender<Event>,
    published: Published,
    changes: Option<Receiver<SinkChange>>,
    queue: Queue,
    announced: Option<u64>,
    stale_sinks: bool,
    transport: TransportState,
    track: Option<Track>,
    output: Option<Output>,
    unbound: Option<Frames>,
    graph_lost: Option<Instant>,
    graph_last_lost: Option<Instant>,
    heard_at_least: Option<Frames>,
    playing: bool,
    seeks: Seeks,
    sleep: Option<Sleeping>,
    failures: usize,
    renegotiations: u8,
    answers: Vec<Answer>,
}

struct Answer {
    kind: CommandKind,
    reply: Answering,
}

enum Answering {
    Outcome(Sender<Result<()>>, Result<()>),
    Landed(Sender<()>),
}

struct Heard {
    changes: Option<Receiver<SinkChange>>,
    events: Option<Receiver<StreamEvent>>,
}

impl Heard {
    fn registered<'a>(&'a self, commands: &'a Receiver<Request>) -> Select<'a> {
        let mut select = Select::new();
        select.recv(commands);
        if let Some(changes) = self.changes.as_ref() {
            select.recv(changes);
        }
        if let Some(events) = self.events.as_ref() {
            select.recv(events);
        }

        select
    }

    fn is_what(&self, engine: &Engine) -> bool {
        is_one_channel(self.changes.as_ref(), engine.changes.as_ref())
            && is_one_channel(
                self.events.as_ref(),
                engine.listening().map(SinkStream::events),
            )
    }
}

fn is_one_channel<T>(held: Option<&Receiver<T>>, now: Option<&Receiver<T>>) -> bool {
    match (held, now) {
        (Some(held), Some(now)) => held.same_channel(now),
        (None, None) => true,
        _ => false,
    }
}

impl Engine {
    pub fn new(
        config: EngineConfig,
        sources: Arc<Sources>,
        backend: Box<dyn Backend>,
        commands: Receiver<Request>,
        events: Sender<Event>,
        published: Published,
    ) -> Self {
        let changes = Some(backend.subscribe_sinks());
        *published.sinks.write() = backend
            .enumerate_sinks(SINK_TIMEOUT)
            .unwrap_or_default()
            .into();
        *published.settings.write() = Arc::new(OutputSettings::of(&config));

        Self {
            config,
            sources,
            backend,
            commands,
            events,
            published,
            changes,
            queue: Queue::new(),
            announced: None,
            stale_sinks: false,
            transport: TransportState::Idle,
            track: None,
            output: None,
            unbound: None,
            graph_lost: None,
            graph_last_lost: None,
            heard_at_least: None,
            playing: false,
            seeks: Seeks::default(),
            sleep: None,
            failures: 0,
            renegotiations: 0,
            answers: Vec::new(),
        }
    }

    pub fn run(mut self) {
        let commands = self.commands.clone();

        'running: loop {
            let heard = self.heard();
            let mut parked = heard.registered(&commands);

            loop {
                let _ = parked.ready_timeout(self.budget());
                if !self.take_commands() {
                    break 'running;
                }

                self.rediscover();
                self.watch_discard();
                self.pump();
                self.finish_reshaping();
                self.promote();
                self.collect_faults();
                self.poll_stream();
                self.watch_graph();
                self.settle();
                self.doze();
                self.publish();
                self.answer();

                if !heard.is_what(&self) {
                    break;
                }
            }
        }

        if let Some(output) = self.output.as_mut() {
            output.close();
        }
        if let Err(error) = self.backend.shutdown() {
            tracing::warn!(%error, "the sink backend did not shut down cleanly");
        }
    }

    fn heard(&self) -> Heard {
        Heard {
            changes: self.changes.clone(),
            events: self.listening().map(|stream| stream.events().clone()),
        }
    }

    fn listening(&self) -> Option<&SinkStream> {
        let output = self.output.as_ref()?;
        if output.deaf {
            return None;
        }
        output.stream.as_ref()
    }

    fn budget(&self) -> Duration {
        let budget = match self.output.as_ref() {
            None => IDLE_TICK,
            Some(output) => wait_for(
                Frames(output.buffered() as u64).to_duration(output.plan.stream.rate),
                self.playing || output.wants_more(),
            ),
        };

        match self.sleep.and_then(Sleeping::left) {
            Some(left) => budget.min(left),
            None => budget,
        }
    }

    fn take_commands(&mut self) -> bool {
        loop {
            match self.commands.try_recv() {
                Ok(request) => self.dispatch(request),
                Err(TryRecvError::Empty) => return true,
                Err(TryRecvError::Disconnected) => return false,
            }
        }
    }

    fn emit(&self, event: Event) {
        if self.events.try_send(event).is_err() {
            tracing::warn!("an engine event was dropped; nothing is draining the event channel");
        }
    }

    fn dispatch(&mut self, request: Request) {
        let Request { command, reply } = request;
        let kind = command.kind();
        let outcome = self.apply(command);

        if let Err(error) = outcome.as_ref() {
            tracing::warn!(%error, ?kind, "command rejected");
        }
        let reply = match reply {
            Reply::Answered(reply) => Answering::Outcome(reply, outcome),
            Reply::Landed(reply) => {
                self.announce_a_refusal(kind, outcome);
                Answering::Landed(reply)
            }
            Reply::Unwaited => {
                self.announce_a_refusal(kind, outcome);
                return;
            }
        };
        self.answers.push(Answer { kind, reply });
    }

    fn announce_a_refusal(&self, command: CommandKind, outcome: Result<()>) {
        if let Err(error) = outcome {
            self.emit(Event::CommandFailed { command, error });
        }
    }

    fn answer(&mut self) {
        for answer in self.answers.drain(..) {
            let delivered = match answer.reply {
                Answering::Outcome(reply, outcome) => reply.try_send(outcome).is_ok(),
                Answering::Landed(reply) => reply.try_send(()).is_ok(),
            };
            if !delivered {
                tracing::debug!(kind = ?answer.kind, "nothing was waiting on the command's outcome");
            }
        }
    }

    fn apply(&mut self, command: Command) -> Result<()> {
        match command {
            Command::Load {
                items,
                start_at,
                autoplay,
            } => {
                self.stop();
                let loaded = self.queue.load(items, start_at).is_some();
                self.playing = autoplay;
                if loaded && autoplay {
                    let started = self.start(Frames::ZERO);
                    return self.past_what_will_not_open(started);
                }
                Ok(())
            }
            Command::Resume(resumption) => {
                self.stop();
                let at = resumption.at;
                let resumed = self.queue.restore(resumption).is_some();
                self.playing = false;
                if resumed {
                    self.start(at)?;
                }
                Ok(())
            }
            Command::Insert { items, at, play } => match self.queue.insert(items, at) {
                Some(row) if play => self.hear(row),
                _ => Ok(()),
            },
            Command::Remove(rows) => self.remove(rows),
            Command::Move { rows, to } => self.queue.move_rows(rows, to).ok_or(Error::NoSuchRow {
                rows: Span::between(rows.last(), to),
                len: self.queue.len(),
            }),
            Command::Order(rows) => {
                let named = rows.len();
                self.queue.reorder(&rows).ok_or(Error::NotAnOrder {
                    named,
                    len: self.queue.len(),
                })
            }
            Command::Play => self.play(),
            Command::Pause => self.pause(),
            Command::TogglePlayPause => {
                if self.playing {
                    self.pause()
                } else {
                    self.play()
                }
            }
            Command::Stop => {
                self.stop();
                Ok(())
            }
            Command::Seek(to) => self.seek(to),
            Command::SeekBy(delta) => {
                let now = self.position().get();
                let to = if delta < 0 {
                    now.saturating_sub(delta.unsigned_abs())
                } else {
                    now.saturating_add(delta.unsigned_abs())
                };
                self.seek(Frames(to))
            }
            Command::Next => {
                self.failures = 0;
                self.skipped_by_hand();
                let skipped = self.skip(false);
                self.past_what_will_not_open(skipped)
            }
            Command::Previous => {
                self.failures = 0;
                self.skipped_by_hand();
                self.queue.retreat().ok_or(Error::QueueEmpty)?;
                let started = self.start(Frames::ZERO);
                self.past_what_will_not_open(started)
            }
            Command::JumpTo(item) => self.hear(item),
            Command::SetVolume(volume) => {
                self.config.volume = volume;
                self.retune()
            }
            Command::SetRepeat(repeat) => {
                self.queue.set_repeat(repeat);
                Ok(())
            }
            Command::SetSkipUnderRepeat(skip) => {
                self.config.skip_under_repeat = skip;
                Ok(())
            }
            Command::SetShuffle(shuffle) => {
                self.queue.set_shuffle(shuffle);
                Ok(())
            }
            Command::SetSink(sink) => {
                self.config.sink = sink;
                let at = self.position();
                self.rebind(Some(at), None)
            }
            Command::SetQuality(quality) => {
                self.config.quality = quality;
                let at = self.position();
                self.rebind(Some(at), None)
            }
            Command::SetFilterPhase(phase) => {
                self.config.filter_phase = phase;
                let at = self.position();
                self.rebind(Some(at), None)
            }
            Command::SetDither(dither) => {
                self.config.dither = dither;
                let at = self.position();
                self.rebind(Some(at), None)
            }
            Command::SetRestoration(restoration) => {
                self.config.restoration = restoration;
                let at = self.position();
                self.rebind(Some(at), None)
            }
            Command::SetTruePeak(on) => {
                self.config.true_peak = on;
                if let Some(track) = self.track.as_mut() {
                    track.replay_gain = levelled(&self.config, &track.info, track.hints);
                    track.published = None;
                }
                let at = self.position();
                self.rebind(Some(at), None)
            }
            Command::SetNoiseShaping(shaping) => {
                self.config.noise_shaping = shaping;
                let at = self.position();
                self.rebind(Some(at), None)
            }
            Command::SetReplayGain(mode) => {
                self.config.replay_gain = mode;
                self.re_level()
            }
            Command::SetLevelling(levelling) => {
                self.config.levelling = levelling;
                self.re_level()
            }
            Command::SetEqualisation(equalisation) => {
                self.config.equaliser = equalisation;
                self.retune()
            }
            Command::SetBitPerfect(prefer) => {
                self.config.prefer_bit_perfect = prefer;
                let at = self.position();
                self.rebind(Some(at), None)
            }
            Command::SetDop(marked) => {
                self.config.dop = marked;
                let at = self.position();
                self.rebind(Some(at), None)
            }
            Command::SetForceGraphRate(force) => {
                self.config.force_graph_rate = force;
                let at = self.position();
                self.rebind(Some(at), None)
            }
            Command::SetBuffer(buffer) => {
                self.config.buffer = buffer;
                let at = self.position();
                self.rebind(Some(at), None)
            }
            Command::SleepUntil(until) => {
                self.sleep = until.map(Sleeping::set);
                Ok(())
            }
        }
    }

    fn doze(&mut self) {
        if !self.sleep.is_some_and(Sleeping::is_out) {
            return;
        }
        self.sleep = None;
        if self.track.is_none() {
            return;
        }
        if let Err(error) = self.pause() {
            tracing::warn!(%error, "the sleep timer could not pause the transport");
        }
    }

    fn nods_off(&mut self) {
        self.sleep = None;
        self.playing = false;
    }

    fn re_level(&mut self) -> Result<()> {
        if let Some(track) = self.track.as_mut() {
            track.replay_gain = levelled(&self.config, &track.info, track.hints);
            track.published = None;
        }
        self.retune()
    }

    fn hear(&mut self, row: usize) -> Result<()> {
        self.queue.jump_to(row).ok_or(Error::QueueEmpty)?;
        self.failures = 0;
        self.playing = true;
        let started = self.start(Frames::ZERO);
        self.past_what_will_not_open(started)
    }

    fn past_what_will_not_open(&mut self, started: Result<()>) -> Result<()> {
        match started {
            Err(error @ Error::Decode { .. }) if self.playing => {
                self.fail(error);
                Ok(())
            }
            started => started,
        }
    }

    fn play(&mut self) -> Result<()> {
        self.playing = true;
        if self.track.is_none() {
            let started = self.start(Frames::ZERO);
            return self.past_what_will_not_open(started);
        }
        if let Some(at) = self.unbound {
            self.rebind(Some(at), None)?;
            self.unbound = None;
            return Ok(());
        }
        if self.transport == TransportState::Paused {
            self.transport = TransportState::Playing;
        }
        self.set_active(true)
    }

    fn pause(&mut self) -> Result<()> {
        if self.track.is_none() {
            return Err(Error::InvalidTransition {
                state: self.transport,
                attempted: CommandKind::Pause,
            });
        }
        self.playing = false;
        if self.transport == TransportState::Playing {
            self.transport = TransportState::Paused;
        }
        self.set_active(false)
    }

    fn set_active(&mut self, active: bool) -> Result<()> {
        let Some(stream) = self
            .output
            .as_ref()
            .and_then(|output| output.stream.as_ref())
        else {
            return Ok(());
        };
        stream.set_active(active)?;
        Ok(())
    }

    fn stop(&mut self) {
        self.playing = false;
        self.failures = 0;
        if let Some(output) = self.output.as_mut() {
            output.close();
        }
        self.output = None;
        self.track = None;
        self.unbound = None;
        self.heard_at_least = None;
        self.transport = TransportState::Stopped;
    }

    fn remove(&mut self, rows: Span) -> Result<()> {
        let removal = self.queue.remove_rows(rows).ok_or(Error::NoSuchRow {
            rows,
            len: self.queue.len(),
        })?;
        if removal == Removal::Queued || self.track.is_none() {
            return Ok(());
        }
        if self.queue.current().is_none() {
            self.stop();
            self.emit(Event::QueueFinished);
            return Ok(());
        }
        let started = self.start(Frames::ZERO);
        self.past_what_will_not_open(started)
    }

    fn skipped_by_hand(&mut self) {
        if self.queue.repeat() == RepeatMode::Track
            && self.config.skip_under_repeat == SkipUnderRepeat::RepeatsTheQueue
        {
            self.queue.set_repeat(RepeatMode::Queue);
        }
    }

    fn skip(&mut self, natural: bool) -> Result<()> {
        if self.queue.current().is_none() {
            return Err(Error::QueueEmpty);
        }
        if natural && let Some(id) = self.track.as_ref().map(|track| track.id) {
            self.emit(Event::TrackFinished(id));
        }
        let wraps = self.queue.wraps_next();

        if natural && self.sleep.is_some_and(Sleeping::ends_the_track) {
            self.nods_off();
        }
        if self.queue.advance(natural).is_none() {
            if self.sleep.is_some_and(Sleeping::ends_the_queue) {
                self.sleep = None;
            }
            self.stop();
            self.emit(Event::QueueFinished);
            return Ok(());
        }
        if wraps && self.sleep.is_some_and(Sleeping::ends_the_queue) {
            self.nods_off();
        }
        self.start(Frames::ZERO)
    }

    fn seek(&mut self, to: Frames) -> Result<()> {
        let Some(track) = self.track.as_ref() else {
            return Err(Error::InvalidTransition {
                state: self.transport,
                attempted: CommandKind::Seek,
            });
        };
        if let Some(duration) = track.info.duration
            && to > duration
        {
            return Err(Error::SeekOutOfRange {
                track: track.id,
                requested: to,
                duration,
            });
        }
        let landed = if self.reuses_stream() {
            self.seek_in_place(to)
        } else {
            self.rebind(Some(to), None)
        };
        if landed.is_ok() {
            self.seeks = self.seeks.stepped();
            self.heard_at_least = None;
        }
        landed
    }

    fn reuses_stream(&self) -> bool {
        let (Some(track), Some(output)) = (self.track.as_ref(), self.output.as_ref()) else {
            return false;
        };
        self.playing && track.info.is_seekable && output.holds_a_live_stream()
    }

    fn seek_in_place(&mut self, to: Frames) -> Result<()> {
        if let Some(track) = self.track.as_mut() {
            if let Err(source) = track.decoder.seek(to) {
                let track = track.id;
                return Err(Error::Decode { track, source });
            }
            track.rewind_carry();
            track.restart_profile();
        }

        if let Some(output) = self.output.as_mut() {
            output.chain.reset();
            output.producer.discard_buffered();
            output.discarding_since = Some(Instant::now());
            if let Some(tapping) = output.tapping.as_ref() {
                tapping.forget();
            }
        }
        Ok(())
    }

    fn watch_discard(&mut self) {
        let Some(unanswered) = self.unanswered_discard() else {
            return;
        };
        match unanswered {
            Unanswered::NothingPulling => tracing::debug!(
                "the transport stopped before the graph took the seek; rebuilding the ring around it"
            ),
            Unanswered::GraphStalled => tracing::warn!(
                "the graph did not drain the ring after a seek; rebuilding the stream around it"
            ),
        }
        let at = self.position();
        if let Err(error) = self.rebind(Some(at), None) {
            self.fail(error);
        }
    }

    fn unanswered_discard(&mut self) -> Option<Unanswered> {
        let playing = self.playing;
        let output = self.output.as_mut()?;
        let since = output.discarding_since?;
        if !output.producer.is_discarding() {
            output.discarding_since = None;
            return None;
        }
        if !playing {
            return Some(Unanswered::NothingPulling);
        }
        (since.elapsed() >= DISCARD_TIMEOUT).then_some(Unanswered::GraphStalled)
    }

    fn start(&mut self, at: Frames) -> Result<()> {
        let Some(item) = self.queue.current().cloned() else {
            return Err(Error::QueueEmpty);
        };

        if let Some(output) = self.output.as_mut() {
            output.close();
        }
        self.output = None;
        self.track = None;
        self.heard_at_least = None;

        let track = Track::open(
            item.id,
            &item.location,
            item.span,
            &self.sources,
            &self.config,
        )
        .map_err(|source| Error::Decode {
            track: item.id,
            source,
        })?;
        let at = track
            .info
            .duration
            .filter(|duration| at > *duration)
            .map_or(at, |_| Frames::ZERO);
        self.track = Some(track);
        self.unbound = (!self.playing).then_some(at);

        self.rebind(Some(at), None)?;
        self.renegotiations = 0;
        self.emit(Event::TrackStarted(item.id));
        Ok(())
    }

    fn rebind(&mut self, at: Option<Frames>, target: Option<StreamSpec>) -> Result<()> {
        if self.track.is_none() {
            return Ok(());
        }
        let resumes_at = at.unwrap_or_else(|| self.position());
        if let Some(output) = self.output.as_mut() {
            output.close();
        }
        self.output = None;

        let bound = self.bind(at, target);
        if bound.is_err() && self.track.is_some() {
            self.unbound = Some(resumes_at);
        }
        bound
    }

    fn bind(&mut self, at: Option<Frames>, target: Option<StreamSpec>) -> Result<()> {
        if let Some(track) = self.track.as_mut()
            && let Some(at) = at
            && track.decoder.position() != at
            && track.info.is_seekable
            && let Err(source) = track.decoder.seek(at)
        {
            let id = track.id;
            return Err(Error::Decode { track: id, source });
        }

        if !self.playing && self.unbound.is_some() {
            let at = at.unwrap_or_else(|| self.position());
            self.unbound = Some(at);
            self.transport = TransportState::Paused;
            return Ok(());
        }

        let sink = self.select_sink()?;
        let Some(track) = self.track.as_ref() else {
            return Ok(());
        };
        let output = Output::open(
            track,
            &sink,
            &self.config,
            target,
            &self.published.listening,
        )?;

        let delivery = output.plan.delivery();
        if let Some(track) = self.track.as_mut() {
            track.decoder.deliver(delivery);
            track.rewind_carry();
            track.restart_profile();
        }

        let status = output.status;
        self.output = Some(output);
        self.transport = TransportState::Loading;
        self.emit(Event::OutputChanged(status));
        Ok(())
    }

    fn retune(&mut self) -> Result<()> {
        let (Some(track), Some(output)) = (self.track.as_ref(), self.output.as_mut()) else {
            return Ok(());
        };
        output.settles_into = None;
        let replay_gain = track.replay_gain;
        let packs = self
            .published
            .sinks
            .read()
            .iter()
            .find(|sink| sink.name == output.bound)
            .is_some_and(|sink| {
                packs_again(
                    track.decoded(),
                    &output.plan,
                    sink,
                    &self.config,
                    replay_gain,
                )
            });
        if packs {
            let at = self.position();
            return self.rebind(Some(at), None);
        }
        let wanted = plan_for(
            track.decoded(),
            &output.bound,
            output.plan.stream,
            &self.config,
            replay_gain,
        );

        if wanted.same_shape_as(&output.plan) {
            output.chain.set_gain(self.config.volume, replay_gain);
            if let Some(profile) = wanted.equalisation.as_ref() {
                output.chain.set_equalisation(profile);
            }
            output.plan = wanted;
            return Ok(());
        }
        if output.plan.becomes_on_the_same_stream(&wanted) {
            return self.reshape(wanted);
        }
        let at = self.position();
        self.rebind(Some(at), None)
    }

    fn reshape(&mut self, wanted: OutputPlan) -> Result<()> {
        let (Some(track), Some(output)) = (self.track.as_ref(), self.output.as_mut()) else {
            return Ok(());
        };
        let drops_a_gain_stage = wanted.gain.is_none() && output.chain.gain_amplitude().is_some();
        if drops_a_gain_stage {
            output.chain.set_gain(self.config.volume, track.replay_gain);
            if output.chain.is_ramping() {
                output.settles_into = Some(wanted);
                return Ok(());
            }
        }
        self.swap_chain(wanted)
    }

    fn finish_reshaping(&mut self) {
        let settled = self
            .output
            .as_mut()
            .filter(|output| output.settles_into.is_some() && !output.chain.is_ramping())
            .and_then(|output| output.settles_into.take());
        let Some(wanted) = settled else {
            return;
        };
        if let Err(error) = self.swap_chain(wanted) {
            tracing::warn!(%error, "the chain could not be reshaped once its gain had settled");
        }
    }

    fn swap_chain(&mut self, wanted: OutputPlan) -> Result<()> {
        let (Some(track), Some(output)) = (self.track.as_mut(), self.output.as_mut()) else {
            return Ok(());
        };
        if !output.hand_on_what_the_chain_holds() {
            output.settles_into = Some(wanted);
            return Ok(());
        }
        let source = track.source();
        let mut chain = wanted
            .build_chain(source, &self.config, CHAIN_BLOCK)
            .map_err(|error| Error::Convert {
                track: track.id,
                source_spec: source,
                sink_spec: wanted.stream,
                source: error,
            })?;
        chain.ramp_gain_from(output.chain.gain_amplitude().unwrap_or(Gain::UNITY.get()));

        let delivery = wanted.delivery();
        track.decoder.deliver(delivery);
        track.decoded.retype(delivery.format);

        let status = output.take_chain(chain, wanted);
        self.emit(Event::OutputChanged(status));
        Ok(())
    }

    fn note_sink_changes(&mut self) {
        loop {
            match self.changes.as_ref().map(Receiver::try_recv) {
                Some(Ok(_)) => self.stale_sinks = true,
                Some(Err(TryRecvError::Disconnected)) => {
                    tracing::warn!("the backend stopped announcing sinks; the list is now frozen");
                    self.changes = None;
                    return;
                }
                Some(Err(TryRecvError::Empty)) | None => return,
            }
        }
    }

    fn rediscover(&mut self) {
        self.note_sink_changes();
        if !self.stale_sinks {
            return;
        }
        let Some(budget) = self.enumeration_budget() else {
            return;
        };

        self.stale_sinks = false;
        match self.backend.enumerate_sinks(budget) {
            Ok(found) => *self.published.sinks.write() = found.into(),
            Err(error) => tracing::warn!(%error, "the sink list could not be refreshed"),
        }
    }

    fn enumeration_budget(&self) -> Option<Duration> {
        let Some(output) = self.output.as_ref() else {
            return Some(SINK_TIMEOUT);
        };
        let held = Frames(output.buffered() as u64).to_duration(output.plan.stream.rate);
        (held >= SINK_REFRESH_BUDGET.saturating_mul(2)).then_some(SINK_REFRESH_BUDGET)
    }

    fn select_sink(&mut self) -> Result<SinkInfo> {
        self.note_sink_changes();
        if self.sinks_are_stale() {
            self.stale_sinks = false;
            match self.backend.enumerate_sinks(SINK_TIMEOUT) {
                Ok(found) => *self.published.sinks.write() = found.into(),
                Err(error) if self.published.sinks.read().is_empty() => return Err(error.into()),
                Err(error) => {
                    tracing::warn!(%error, "the sink list could not be refreshed; binding to what was last published");
                }
            }
        }

        let sinks = self.published.sinks.read();
        sinks
            .iter()
            .find(|sink| self.config.sink.as_ref() == Some(&sink.name))
            .or_else(|| sinks.iter().find(|sink| sink.is_default))
            .or_else(|| sinks.first())
            .cloned()
            .ok_or(Error::Sink(resonate_pipewire::Error::NoSink))
    }

    fn sinks_are_stale(&self) -> bool {
        self.stale_sinks || self.changes.is_none() || self.published.sinks.read().is_empty()
    }

    fn pump(&mut self) {
        let outcome = {
            let (Some(track), Some(output)) = (self.track.as_mut(), self.output.as_mut()) else {
                return;
            };
            if output.ended {
                return;
            }
            Self::fill(track, output).map_err(|source| Error::Decode {
                track: track.id,
                source,
            })
        };
        if let Err(error) = outcome {
            self.fail(error);
        }
    }

    fn fill(track: &mut Track, output: &mut Output) -> resonate_codec::Result<()> {
        if output.producer.is_discarding() {
            return Ok(());
        }
        if output.draining.is_some() {
            Self::drain(output);
            return Ok(());
        }

        loop {
            if track.undecoded() == 0 {
                track.decoded_at = 0;
                if track.decoder.next_block(&mut track.decoded)? == DecodeStatus::EndOfStream {
                    Self::flush(output);
                    return Ok(());
                }
                track.sample_packet();
                if track.decoded.frames() == 0 {
                    continue;
                }
            }

            if output.plan.is_transparent() {
                let written = output.producer.write_from(&track.decoded, track.decoded_at);
                if written == 0 {
                    return Ok(());
                }
                if let Some(tapping) = output.tapping.as_mut() {
                    tapping.record(&track.decoded, track.decoded_at, written);
                }
                track.decoded_at = track.decoded_at.saturating_add(written);
            } else {
                if output.producer.free_frames() < output.chain.max_output_frames() {
                    return Ok(());
                }
                if !Self::convert(track, output) {
                    return Ok(());
                }
            }
        }
    }

    fn convert(track: &mut Track, output: &mut Output) -> bool {
        let channels = track.source().channel_count().get() as usize;
        let frames_in = track.undecoded().min(CHAIN_BLOCK);
        let start = track.decoded_at.saturating_mul(channels);
        let samples = frames_in.saturating_mul(channels);

        let Some(room) = output.widened.get_mut(..samples) else {
            return false;
        };
        let widened = track.decoded.data().widen_into(start, room);
        let Some(input) = output.widened.get(..widened) else {
            return false;
        };

        let count = output.chain.process(input, &mut output.carrier);

        track.decoded_at = track
            .decoded_at
            .saturating_add(count.frames_in.min(frames_in));
        output.stage(count.frames_out);
        let written = output.producer.write(&output.staged);
        if let Some(tapping) = output.tapping.as_mut() {
            tapping.record(&output.staged, 0, written);
        }

        count.frames_in > 0 || count.frames_out > 0
    }

    fn flush(output: &mut Output) {
        if output.plan.is_transparent() {
            output.end();
            return;
        }
        let frames = match output.chain.flush(&mut output.carrier) {
            Ok(frames) => frames,
            Err(error) => {
                tracing::error!(%error, "the chain's tail did not fit the carrier; it was dropped");
                output.end();
                return;
            }
        };

        output.stage(frames);
        output.draining = Some(0);
        Self::drain(output);
    }

    fn drain(output: &mut Output) {
        let Some(written) = output.draining else {
            return;
        };
        let laid = output.producer.write_from(&output.staged, written);
        if let Some(tapping) = output.tapping.as_mut() {
            tapping.record(&output.staged, written, laid);
        }
        let written = written.saturating_add(laid);

        if written >= output.staged.frames() {
            output.draining = None;
            output.end();
        } else {
            output.draining = Some(written);
        }
    }

    fn promote(&mut self) {
        let Some(output) = self.output.as_ref() else {
            return;
        };
        if output.stream.is_some() || !output.primed() {
            return;
        }

        let request = StreamRequest {
            target: Some(output.sink),
            spec: output.plan.stream,
            latency: LatencyRequest::Auto,
            role: MediaRole::Music,
            media_name: self
                .track
                .as_ref()
                .and_then(|track| track.info.tags.title.clone())
                .unwrap_or_else(|| self.config.app_name.clone()),
            force_graph_rate: self.config.force_graph_rate,
            no_convert: output.plan.mode != OutputMode::Converted,
            exclusive: false,
            realtime: true,
        };

        let Some(consumer) = self
            .output
            .as_mut()
            .and_then(|output| output.consumer.take())
        else {
            return;
        };

        match self.backend.open(&request, Box::new(consumer)) {
            Ok(stream) => {
                if let Err(error) = stream.set_active(self.playing) {
                    tracing::warn!(%error, "the stream would not take its initial active state");
                }
                if let Some(output) = self.output.as_mut() {
                    output.stream = Some(stream);
                }
                self.transport = if self.playing {
                    TransportState::Playing
                } else {
                    TransportState::Paused
                };
            }
            Err(error) => {
                self.output = None;
                self.fail(error.into());
            }
        }
    }

    fn watch_graph(&mut self) {
        if let Some(since) = self.graph_lost {
            self.wait_for_the_graph(since);
            return;
        }
        let Some(output) = self.output.as_ref() else {
            return;
        };
        if output.stream.is_none() || !output.producer.is_abandoned() {
            return;
        }
        let again = self
            .graph_last_lost
            .is_some_and(|last| last.elapsed() < GRAPH_BACK_WITHIN);
        self.graph_last_lost = Some(Instant::now());
        if again {
            self.fail(Error::Sink(resonate_pipewire::Error::LoopStopped));
            return;
        }

        tracing::warn!("the graph let go of the stream; the row waits for it to come back");
        let at = self.heard_position();
        if let Some(output) = self.output.as_mut() {
            output.close();
        }
        self.output = None;
        self.unbound = Some(at);
        self.transport = TransportState::Loading;
        self.graph_lost = Some(Instant::now());
    }

    fn wait_for_the_graph(&mut self, since: Instant) {
        let at = self
            .unbound
            .filter(|_| self.playing && self.track.is_some() && self.output.is_none());
        let Some(at) = at else {
            self.graph_lost = None;
            return;
        };

        let back = self
            .backend
            .enumerate_sinks(SINK_TIMEOUT)
            .map_err(Error::Sink)
            .and_then(|found| {
                *self.published.sinks.write() = found.into();
                self.stale_sinks = false;
                self.rebind(Some(at), None)
            });
        match back {
            Ok(()) => {
                tracing::info!("the graph is back; the row plays on from where it was heard");
                self.unbound = None;
                self.graph_lost = None;
            }
            Err(error) if since.elapsed() < GRAPH_BACK_WITHIN => {
                tracing::debug!(%error, "the graph is not back yet");
                self.transport = TransportState::Loading;
            }
            Err(error) => {
                self.graph_lost = None;
                self.fail(error);
            }
        }
    }

    fn collect_faults(&mut self) {
        let playing = self.transport == TransportState::Playing;
        let Some(output) = self.output.as_mut() else {
            return;
        };

        let mut seen = 0;
        while let Some(fault) = output.monitor.next_fault() {
            match fault {
                RtFault::Underrun => seen += 1,
                RtFault::Dropped { count } => seen += u64::from(count),
            }
        }
        let missing = output.monitor.went_without();

        if !playing || output.ended || seen == 0 {
            return;
        }
        output.status.underruns = output.status.underruns.saturating_add(seen);
        output.status.went_without = output.status.went_without.saturating_add(missing);
        self.emit(Event::Underrun { missing });
    }

    fn poll_stream(&mut self) {
        let Some(output) = self.output.as_mut() else {
            return;
        };
        let node = output.sink;

        let Some(latency) = output.stream.as_ref().map(SinkStream::latency) else {
            return;
        };
        output.status.latency = latency;
        let events = Self::drain_events(output);

        let mut renegotiated = None;
        let mut failed = None;

        for event in events {
            match event {
                StreamEvent::FormatChanged(spec) if spec != output.plan.stream => {
                    renegotiated = Some(spec);
                }
                StreamEvent::Drained if output.tail == Tail::Asked => output.tail = Tail::Played,
                StreamEvent::FormatChanged(_) | StreamEvent::Drained => {}
                StreamEvent::StateChanged {
                    from,
                    to: StreamState::Failed,
                } => failed = Some(from),
                StreamEvent::StateChanged { .. } => {}
            }
        }

        if let Some(previous) = failed {
            self.fail(Error::Sink(resonate_pipewire::Error::StreamFailed {
                node,
                previous,
            }));
            return;
        }
        if let Some(spec) = renegotiated {
            self.downgrade(spec);
        }
    }

    fn drain_events(output: &mut Output) -> Vec<StreamEvent> {
        let mut events = Vec::new();
        loop {
            let Some(stream) = output.stream.as_ref() else {
                return events;
            };
            match stream.events().try_recv() {
                Ok(event) => events.push(event),
                Err(TryRecvError::Empty) => return events,
                Err(TryRecvError::Disconnected) => {
                    if !output.deaf {
                        tracing::warn!("the sink stream stopped reporting; nothing is on its loop");
                    }
                    output.deaf = true;
                    return events;
                }
            }
        }
    }

    fn downgrade(&mut self, negotiated: StreamSpec) {
        let Some(requested) = self.output.as_ref().map(|output| output.plan.stream) else {
            return;
        };

        self.renegotiations = self.renegotiations.saturating_add(1);
        if self.renegotiations > MAX_RENEGOTIATIONS {
            let Some(track) = self.track.as_ref().map(|track| track.id) else {
                return;
            };
            self.fail(Error::Renegotiation {
                track,
                requested,
                negotiated,
                attempts: self.renegotiations,
            });
            return;
        }

        tracing::info!(
            %requested,
            %negotiated,
            "the graph negotiated a different format; rebuilding around it"
        );
        let at = self.position();
        if let Err(error) = self.rebind(Some(at), Some(negotiated)) {
            self.fail(error);
        }
    }

    fn settle(&mut self) {
        let finished = {
            let Some(output) = self.output.as_mut() else {
                return;
            };
            if !output.ended {
                return;
            }

            if output.buffered() > 0 {
                output.silent_until = None;
                false
            } else {
                output.ask_to_drain();
                output.tail_is_out()
            }
        };

        if self.transport == TransportState::Playing {
            self.transport = TransportState::Draining;
        }
        if !finished {
            return;
        }
        self.failures = 0;
        if let Err(error) = self.skip(true) {
            tracing::warn!(%error, "could not advance past the finished track");
            self.fail(error);
        }
    }

    fn fail(&mut self, error: Error) {
        let mut error = error;
        loop {
            tracing::error!(%error, "playback failed");
            match self.track.as_ref().map(|track| track.id).or(error.track()) {
                Some(track) => self.emit(Event::Failed { track, error }),
                None => drop(error),
            }

            self.failures = self.failures.saturating_add(1);
            if self.failures > self.queue.len() {
                self.stop();
                self.emit(Event::QueueFinished);
                return;
            }
            match self.skip(false) {
                Ok(()) => return,
                Err(next) => error = next,
            }
        }
    }

    fn heard_position(&mut self) -> Frames {
        let position = self
            .heard_at_least
            .map_or(self.position(), |floor| self.position().max(floor));
        self.heard_at_least = self.track.is_some().then_some(position);
        position
    }

    fn position(&self) -> Frames {
        let Some(track) = self.track.as_ref() else {
            return Frames::ZERO;
        };
        let decoded = track
            .decoder
            .position()
            .saturating_sub(Frames(track.undecoded() as u64));

        let Some(output) = self.output.as_ref() else {
            return decoded;
        };
        if output.producer.is_discarding() {
            return decoded;
        }
        let downstream = (output.buffered() as u64)
            .saturating_add(output.latency().get())
            .saturating_add(output.chain.latency_frames().round() as u64);
        let at_source = u128::from(downstream) * u128::from(track.source().rate.hz())
            / u128::from(output.plan.stream.rate.hz());

        decoded.saturating_sub(Frames(at_source as u64))
    }

    fn publish(&mut self) {
        let playback = match self.transport {
            TransportState::Idle => PlaybackState::Idle,
            TransportState::Loading => PlaybackState::Buffering,
            TransportState::Playing | TransportState::Draining => PlaybackState::Playing,
            TransportState::Paused => PlaybackState::Paused,
            TransportState::Stopped => PlaybackState::Stopped,
        };
        let position = self.heard_position();
        let current = self.track.as_ref().map(|track| TrackState {
            id: track.id,
            source: track.source(),
            position,
            duration: track.info.duration,
        });

        self.publish_queue();
        self.publish_digest();
        self.publish_settings();
        self.publish_tap();

        *self.published.state.write() = PlayerState {
            playback,
            current,
            volume: self.config.volume,
            repeat: self.queue.repeat(),
            skip_under_repeat: self.config.skip_under_repeat,
            shuffle: self.queue.shuffle(),
            queue_position: self.queue.cursor(),
            loaded_position: self.queue.position(),
            queue_len: self.queue.len(),
            queue_stamp: self.queue.stamp(),
            seeks: self.seeks,
            sleeping: self.sleep.map(Sleeping::published),
            output: self.output.as_ref().map(|output| output.status),
        };
    }

    fn publish_tap(&self) {
        let tapped = self.output.as_ref().map_or(Tapped::Nothing, Output::tapped);
        if !self.published.tap.read().is(&tapped) {
            *self.published.tap.write() = tapped;
        }

        let moving = matches!(
            self.transport,
            TransportState::Playing | TransportState::Draining
        );
        if let Some(output) = self.output.as_ref() {
            output.hear_the_tap(moving);
        }
    }

    fn publish_settings(&self) {
        let unchanged = self.published.settings.read().already_says(&self.config);
        if unchanged {
            return;
        }
        *self.published.settings.write() = Arc::new(OutputSettings::of(&self.config));
    }

    fn publish_queue(&mut self) {
        let revision = self.queue.revision();
        if self.announced == Some(revision) {
            return;
        }
        self.announced = Some(revision);
        *self.published.queue.write() = Queued {
            revision,
            rows: Arc::new(self.queue.in_play_order()),
            loaded_at: self.queue.loaded_at(),
        };
    }

    fn publish_digest(&mut self) {
        let mode = self.config.replay_gain;
        let Some(track) = self.track.as_mut() else {
            if self.published.digest.read().is_some() {
                *self.published.digest.write() = None;
            }
            return;
        };

        let points = track.profile.points();
        if track.published == Some(points) {
            return;
        }
        track.published = Some(points);
        *self.published.digest.write() = Some(Arc::new(track.digest(mode)));
    }
}

fn levelled(config: &EngineConfig, info: &MediaInfo, hints: TrackHints) -> AppliedGain {
    let resolved = resolve_replay_gain(
        config.replay_gain,
        config.levelling,
        &tagged_or_measured(info.tags.replay_gain, hints.measured),
    );
    if config.true_peak {
        resolved.heeding(hints.true_peak)
    } else {
        resolved
    }
}

fn tagged_or_measured(tags: ReplayGain, measured: MeasuredGain) -> ReplayGain {
    if tags.track_gain.is_some() || tags.album_gain.is_some() {
        return tags;
    }
    ReplayGain {
        track_gain: measured.track,
        album_gain: measured.album,
        ..tags
    }
}

fn inspected(sources: &Sources, location: &MediaLocation) -> Option<BoxLayout> {
    match probe_boxes(sources, location) {
        Ok(layout) => layout,
        Err(error) => {
            tracing::debug!(%error, "the box layout the inspector draws could not be read");
            None
        }
    }
}

fn wait_for(held: Duration, moving: bool) -> Duration {
    if !moving {
        return IDLE_TICK;
    }
    (held / 2).clamp(SHORTEST_TICK, PUBLISH_TICK)
}

fn ring_capacity(asked: Duration, spec: StreamSpec, block: usize) -> Frames {
    let wanted = Frames::from_duration(asked, spec.rate).get();
    let least = (block as u64)
        .saturating_mul(BLOCKS_A_RING_HOLDS)
        .max(MIN_RING_FRAMES);
    let most = spec.bytes_to_frames(LARGEST_RING).get().max(least);

    Frames(wanted.clamp(least, most))
}

#[cfg(test)]
mod tests {
    use crossbeam_channel::unbounded;
    use resonate_core::{ChannelLayout, SampleFormat, SampleRate};

    use super::*;

    fn spec() -> StreamSpec {
        StreamSpec::new(
            SampleRate::HZ_192000,
            ChannelLayout::Stereo,
            SampleFormat::S32,
        )
    }

    #[test]
    fn the_buffer_a_setting_asks_for_is_what_the_ring_is_sized_to() {
        assert_eq!(
            ring_capacity(Duration::from_millis(500), spec(), CHAIN_BLOCK),
            Frames(96_000)
        );
    }

    #[test]
    fn a_buffer_deeper_than_the_ring_may_allocate_is_cut_back_to_what_it_may() {
        let held = ring_capacity(Duration::from_millis(500_000_000), spec(), CHAIN_BLOCK);

        assert_eq!(held, spec().bytes_to_frames(LARGEST_RING));
        assert!(spec().frames_to_bytes(held) <= LARGEST_RING);
    }

    #[test]
    fn a_buffer_shallower_than_the_ring_needs_is_lifted_to_what_it_needs() {
        assert_eq!(
            ring_capacity(Duration::ZERO, spec(), CHAIN_BLOCK),
            Frames(MIN_RING_FRAMES)
        );
    }

    #[test]
    fn a_ring_always_holds_two_of_the_largest_blocks_the_chain_writes() {
        const UPSAMPLED_BLOCK: usize = 24_577;

        let held = ring_capacity(Duration::from_millis(100), spec(), UPSAMPLED_BLOCK);
        assert_eq!(held, Frames(UPSAMPLED_BLOCK as u64 * BLOCKS_A_RING_HOLDS));
    }

    #[test]
    fn the_wait_is_half_of_what_the_ring_holds_kept_inside_the_publishing_rate() {
        assert_eq!(wait_for(Duration::ZERO, true), SHORTEST_TICK);
        assert_eq!(wait_for(Duration::from_millis(4), true), SHORTEST_TICK);
        assert_eq!(
            wait_for(Duration::from_millis(20), true),
            Duration::from_millis(10)
        );
        assert_eq!(wait_for(Duration::from_millis(500), true), PUBLISH_TICK);
    }

    #[test]
    fn a_ring_nothing_is_pulling_and_nothing_can_fill_waits_the_idle_tick() {
        assert_eq!(wait_for(Duration::ZERO, false), IDLE_TICK);
        assert_eq!(wait_for(Duration::from_millis(500), false), IDLE_TICK);
    }

    #[test]
    fn a_receiver_is_the_channel_its_clone_is_and_no_other() {
        let (_announced, one) = unbounded::<SinkChange>();
        let (_reported, another) = unbounded::<SinkChange>();

        assert!(is_one_channel(Some(&one), Some(&one.clone())));
        assert!(!is_one_channel(Some(&one), Some(&another)));
        assert!(is_one_channel::<SinkChange>(None, None));
        assert!(!is_one_channel(Some(&one), None));
        assert!(!is_one_channel(None, Some(&one)));
    }

    #[test]
    fn the_set_parks_on_the_commands_the_sink_changes_and_the_stream_in_that_order() {
        let (_orders, commands) = unbounded::<Request>();
        let (announce, changes) = unbounded::<SinkChange>();
        let (report, events) = unbounded::<StreamEvent>();
        let heard = Heard {
            changes: Some(changes.clone()),
            events: Some(events.clone()),
        };
        let mut parked = heard.registered(&commands);

        assert!(
            parked.ready_timeout(SHORTEST_TICK).is_err(),
            "a set nothing has spoken on woke of its own accord"
        );

        announce
            .send(SinkChange::DefaultChanged)
            .expect("the announcement lands");
        assert_eq!(parked.ready_timeout(SHORTEST_TICK), Ok(1));
        changes.try_recv().expect("the announcement is read back");

        report.send(StreamEvent::Drained).expect("the event lands");
        assert_eq!(parked.ready_timeout(SHORTEST_TICK), Ok(2));
        events.try_recv().expect("the event is read back");

        assert!(parked.ready_timeout(SHORTEST_TICK).is_err());
    }

    #[test]
    fn a_set_with_no_stream_and_no_announcements_parks_on_the_commands_alone() {
        let (orders, commands) = unbounded::<Request>();
        let heard = Heard {
            changes: None,
            events: None,
        };
        let mut parked = heard.registered(&commands);

        assert!(parked.ready_timeout(SHORTEST_TICK).is_err());

        drop(orders);
        assert_eq!(
            parked.ready_timeout(SHORTEST_TICK),
            Ok(0),
            "a commands channel nothing holds the other end of is what ends the run"
        );
    }
}
