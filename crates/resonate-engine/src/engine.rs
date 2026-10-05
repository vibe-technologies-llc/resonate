use std::{
    collections::VecDeque,
    iter, mem,
    sync::{Arc, atomic::AtomicBool},
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Select, Sender, TryRecvError, TrySendError};
use resonate_codec::{
    BoxLayout, Decoder, MediaInfo, Packing, ProfileBuilder, ReplayGain, Sources, probe_boxes,
};
use resonate_core::{
    AppliedGain, AudioBuffer, FrameSpan, Frames, Gain, MeasuredGain, MediaLocation, RtFault, Span,
    StreamSpec, TrackHints, TrackId, Volume,
};
use resonate_dsp::{Chain, Easing};
use resonate_pipewire::{
    LatencyRequest, MediaRole, NodeName, SinkChange, SinkId, SinkInfo, SinkStream, StreamEvent,
    StreamRequest, StreamState,
};
use smallvec::SmallVec;

use crate::{
    Backend, Command, CommandKind, EngineConfig, Error, Event, OutputMode, OutputPlan,
    OutputSettings, OutputStatus, PlaybackState, PlayerState, Published, RepeatMode,
    ReplayGainMode, Reply, Request, Result, Seeks, SkipUnderRepeat, Sleeping, StreamDigest, Tapped,
    Tapping, TrackState, TransportState,
    backend::Surveyor,
    lending::{Block, Decoding, Lost, Returned},
    measure::{Measured, Measuring},
    pipeline::{Attenuator, Decoded, packs_again, plan_for, plan_output, resolve_replay_gain},
    queue::{self, Queue, QueueItem, Queued, Removal},
    ring::{Entering, FADED_OVER, RingConsumer, RingMonitor, RingProducer, ring},
    surveying::{Surveyed, Surveying},
};

const SINK_TIMEOUT: Duration = Duration::from_secs(2);
const SHORTEST_TICK: Duration = Duration::from_millis(4);
const PUBLISH_TICK: Duration = Duration::from_millis(16);
const IDLE_TICK: Duration = Duration::from_millis(100);
const AT_REST_TICK: Duration = Duration::from_secs(1);
const EVENTS_A_PASS_HELD_INLINE: usize = 4;

type Drained = SmallVec<[StreamEvent; EVENTS_A_PASS_HELD_INLINE]>;
const CHAIN_BLOCK: usize = 1024;
const TURNS_REMEMBERED: usize = 8;
const ONE_LEVEL_WITHIN: f32 = 1e-5;
const MIN_RING_FRAMES: u64 = 8_192;
const BLOCKS_A_RING_HOLDS: u64 = 2;
const LARGEST_RING: u64 = 64 * 1024 * 1024;
const DISCARD_TIMEOUT: Duration = Duration::from_millis(500);
const MAX_RENEGOTIATIONS: u8 = 3;
const GRAPH_BACK_WITHIN: Duration = Duration::from_secs(10);
const FILLED_IN_ONE_GO: Duration = Duration::from_millis(20);
const EVENTS_OWED_AT_MOST: usize = 1_024;
const UNDERRUNS_TOLD_EVERY: Duration = Duration::from_secs(1);
const LINK_NAPS_AFTER: Duration = Duration::from_secs(3);
const QUIET_WITHIN_AT_LEAST: Duration = Duration::from_millis(40);
const QUIET_WITHIN_AT_MOST: Duration = Duration::from_millis(400);
const SLEEP_FADES_OVER: Duration = Duration::from_secs(10);
const PULLED_WITHIN: Duration = Duration::from_millis(250);
const OPENING_ANSWERED_WITHIN: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Filled {
    AsFarAsItGoes,
    ForNow,
    WhenTheSourceAnswers,
}

struct Track {
    id: TrackId,
    location: MediaLocation,
    span: Option<FrameSpan>,
    decoding: Decoding,
    info: Arc<MediaInfo>,
    layout: Option<Arc<BoxLayout>>,
    hints: TrackHints,
    replay_gain: AppliedGain,
    peak: Peak,
    decoded: AudioBuffer,
    decoded_at: usize,
    profile: ProfileBuilder,
    profiled_from: Frames,
    sampled: Option<Frames>,
    published: Option<usize>,
}

enum Peak {
    Unasked,
    Measuring(Measuring),
    Unmeasurable,
}

struct Unwrapped {
    decoder: Decoder,
    info: MediaInfo,
    hints: TrackHints,
    layout: Option<BoxLayout>,
}

impl Unwrapped {
    fn open(item: &QueueItem, sources: &Sources) -> resonate_codec::Result<Self> {
        let (decoder, info) = match item.span {
            Some(span) => Decoder::open_span(sources, &item.location, span)?,
            None => Decoder::open(sources, &item.location)?,
        };
        Ok(Self {
            decoder,
            info,
            hints: sources.hints(&item.location, item.span),
            layout: inspected(sources, &item.location),
        })
    }
}

struct Opening {
    item: QueueItem,
    at: Frames,
    landed: Receiver<resonate_codec::Result<Unwrapped>>,
}

impl Opening {
    fn begin(item: QueueItem, at: Frames, sources: &Arc<Sources>) -> Self {
        let (landing, landed) = crossbeam_channel::bounded(1);
        let sources = Arc::clone(sources);
        let opened = item.clone();
        let spawned = thread::Builder::new()
            .name("resonate-track-open".into())
            .spawn(move || {
                let _ = landing.send(Unwrapped::open(&opened, &sources));
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "no thread could be started to open a track");
        }
        Self { item, at, landed }
    }

    fn landed(&self) -> Option<Result<Unwrapped>> {
        let track = self.item.id;
        match self.landed.try_recv() {
            Ok(landed) => Some(landed.map_err(|source| Error::Decode { track, source })),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(Error::OpenerStopped { track })),
        }
    }
}

impl Track {
    fn of(
        item: &QueueItem,
        unwrapped: Unwrapped,
        sources: &Arc<Sources>,
        config: &EngineConfig,
    ) -> Self {
        let Unwrapped {
            decoder,
            info,
            hints,
            layout,
        } = unwrapped;
        let replay_gain = levelled(config, &info, hints);
        let mut track = Self {
            id: item.id,
            location: item.location.clone(),
            span: item.span,
            hints,
            peak: Peak::Unasked,
            decoded: AudioBuffer::empty(info.spec),
            layout: layout.map(Arc::new),
            profile: ProfileBuilder::new(info.spec.rate),
            decoding: Decoding::new(decoder),
            info: Arc::new(info),
            replay_gain,
            decoded_at: 0,
            profiled_from: Frames::ZERO,
            sampled: None,
            published: None,
        };
        track.measure_where_wanted(sources, config);
        track
    }

    fn measure_where_wanted(&mut self, sources: &Arc<Sources>, config: &EngineConfig) {
        if !matches!(self.peak, Peak::Unasked)
            || !Measuring::wanted(config, &self.info, self.replay_gain)
        {
            return;
        }
        if let Some(measuring) =
            Measuring::start(Arc::clone(sources), self.location.clone(), self.span)
        {
            self.peak = Peak::Measuring(measuring);
        }
    }

    fn heed_what_was_measured(&mut self, config: &EngineConfig) -> bool {
        let Peak::Measuring(measuring) = &self.peak else {
            return false;
        };
        let Some(measured) = measuring.landed() else {
            return false;
        };
        let Measured::Peaking(peak) = measured else {
            self.peak = Peak::Unmeasurable;
            return false;
        };
        self.peak = Peak::Unasked;
        self.hints.true_peak = Some(peak);
        self.replay_gain = levelled(config, &self.info, self.hints);
        self.published = None;
        true
    }

    fn source(&self) -> StreamSpec {
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

    fn seek(&mut self, to: Frames) -> Result<Frames> {
        let track = self.id;
        let Some(decoder) = self.decoding.home() else {
            let location = self.location.clone();
            return Err(Error::Decode {
                track,
                source: resonate_codec::Error::NotSeekable { location },
            });
        };
        decoder
            .seek(to)
            .map_err(|source| Error::Decode { track, source })
    }

    fn next_block(&mut self) -> Result<Block> {
        let may_stall = !self.info.is_seekable;
        self.decoding
            .next_block(&mut self.decoded, may_stall)
            .map_err(|lost| match lost {
                Lost::Decode(source) => Error::Decode {
                    track: self.id,
                    source,
                },
                Lost::WorkerStopped => Error::DecoderStopped { track: self.id },
            })
    }

    fn sample_packet(&mut self) {
        let Some(packet) = self.decoding.last_packet() else {
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
        self.profiled_from = self.decoding.position();
        self.sampled = None;
        self.published = None;
    }

    fn digest(&self, mode: ReplayGainMode) -> StreamDigest {
        StreamDigest {
            track: self.id,
            location: self.location.clone(),
            span: self.span,
            info: Arc::clone(&self.info),
            layout: self.layout.clone(),
            replay_gain_mode: mode,
            replay_gain: self.replay_gain,
            profiled_from: self.profiled_from,
            decoded: self.decoding.position(),
            packet: self.decoding.last_packet(),
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
    attenuator: Attenuator,
    over_bluetooth: bool,
    awake_since: Option<Instant>,
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
    active: bool,
    quietening_since: Option<Instant>,
    sleep_fading: bool,
    sleep_lifted: Option<Instant>,
    last_pulled: Option<(u64, Instant)>,
}

struct Retiring {
    output: Output,
    since: Instant,
}

fn chosen_sink<'s>(sinks: &'s [SinkInfo], named: Option<&NodeName>) -> Option<&'s SinkInfo> {
    sinks
        .iter()
        .find(|sink| named == Some(&sink.name))
        .or_else(|| sinks.iter().find(|sink| sink.is_default))
        .or_else(|| sinks.first())
}

impl Output {
    fn open(
        track: &Track,
        sink: &SinkInfo,
        config: &EngineConfig,
        target: Option<StreamSpec>,
        listening: &Arc<AtomicBool>,
        entering: Entering,
    ) -> Result<Self> {
        let source = track.source();
        let decoded = track.decoded();
        let attenuator = Attenuator::of(config, sink);
        let plan = match target {
            Some(target) => plan_for(
                decoded,
                &sink.name,
                target,
                config,
                track.replay_gain,
                attenuator,
            ),
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

        let capacity = ring_capacity(config.buffer, plan.stream, chain.max_process_frames());
        let fade = Frames::from_duration(FADED_OVER, plan.stream.rate);
        let (producer, consumer, monitor) =
            ring(plan.stream, capacity, plan.silence(), fade, entering);

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
            attenuator,
            over_bluetooth: sink.is_bluetooth(),
            awake_since: None,
            status: OutputStatus {
                sink: sink.id,
                negotiated: plan.stream,
                words: None,
                mode: plan.mode,
                latency: Frames::ZERO,
                underruns: 0,
                went_without: Frames::ZERO,
                device_turned: attenuator == Attenuator::Device,
                device_muted: attenuator.hears_the_mute_of(sink),
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
            active: false,
            quietening_since: None,
            sleep_fading: false,
            sleep_lifted: None,
            last_pulled: None,
        })
    }

    fn fade(&self) -> Frames {
        Frames::from_duration(FADED_OVER, self.plan.stream.rate)
    }

    fn quiet_within(&self) -> Duration {
        let behind = self.latency().to_duration(self.plan.stream.rate);
        (FADED_OVER + behind * 2).clamp(QUIET_WITHIN_AT_LEAST, QUIET_WITHIN_AT_MOST)
    }

    fn has_gone_quiet(&self, since: Instant) -> bool {
        self.producer.is_quiet() || since.elapsed() >= self.quiet_within()
    }

    fn note_the_pulls(&mut self) {
        let pulls = self.producer.pulls();
        if self.last_pulled.is_none_or(|(seen, _)| seen != pulls) {
            self.last_pulled = Some((pulls, Instant::now()));
        }
    }

    fn is_sounding(&self) -> bool {
        self.active
            && self.stream.is_some()
            && !self.producer.is_quiet()
            && self
                .last_pulled
                .is_some_and(|(pulls, at)| pulls > 0 && at.elapsed() < PULLED_WITHIN)
    }

    fn activate(&mut self, active: bool) -> Result<()> {
        let Some(stream) = self.stream.as_ref() else {
            return Ok(());
        };
        stream.set_active(active)?;
        self.active = active;
        Ok(())
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

    fn has_room_for_what_the_chain_holds(&self, carrying_the_front: bool) -> bool {
        let held = if carrying_the_front {
            self.chain.max_flush_frames_behind_the_front()
        } else {
            self.chain.max_flush_frames()
        };
        self.producer.free_frames() >= held
    }

    fn waits_for_room_to_reshape(&self) -> bool {
        self.settles_into.is_some() && !self.chain.is_ramping()
    }

    fn hand_on_what_the_chain_holds(&mut self) {
        if self.chain.max_flush_frames() == 0 {
            return;
        }
        let frames = match self.chain.flush(&mut self.carrier) {
            Ok(frames) => frames,
            Err(error) => {
                tracing::error!(%error, "the chain's held frames did not fit the carrier; they were dropped");
                return;
            }
        };
        self.stage(frames);
        let written = self.producer.write(&self.staged);
        if let Some(tapping) = self.tapping.as_mut() {
            tapping.record(&self.staged, 0, written);
        }
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
        if self.ended || self.producer.is_discarding() || self.waits_for_room_to_reshape() {
            return false;
        }
        self.producer.free_frames() >= self.chain.max_process_frames()
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
    surveyor: Arc<dyn Surveyor>,
    surveying: Option<Surveying>,
    commands: Receiver<Request>,
    events: Sender<Event>,
    published: Published,
    changes: Option<Receiver<SinkChange>>,
    queue: Queue,
    opening: Option<Opening>,
    announced: Option<u64>,
    stale_sinks: bool,
    transport: TransportState,
    track: Option<Track>,
    output: Option<Output>,
    retiring: Option<Retiring>,
    unbound: Option<Frames>,
    rebind_owed: bool,
    graph_lost: Option<Instant>,
    graph_last_lost: Option<Instant>,
    graph_still_away: Option<Error>,
    fill_owed: bool,
    events_owed: VecDeque<Event>,
    underruns_told_at: Option<Instant>,
    missing_untold: Frames,
    waiting_for_a_device: bool,
    device_last_lost: Option<Instant>,
    heard_at_least: Option<Frames>,
    playing: bool,
    seeks: Seeks,
    sleep: Option<Sleeping>,
    failures: usize,
    renegotiations: u8,
    sounded: Option<(SinkId, Instant)>,
    turned: VecDeque<Gain>,
    answers: Vec<Answer>,
    openings_begun: u64,
    deferred: Vec<Deferred>,
}

struct Answer {
    kind: CommandKind,
    reply: Answering,
}

struct Deferred {
    kind: CommandKind,
    reply: Reply,
    since: Instant,
}

enum Answering {
    Outcome(Sender<Result<()>>, Result<()>),
    Landed(Sender<()>),
}

struct Heard {
    changes: Option<Receiver<SinkChange>>,
    surveyed: Option<Receiver<Surveyed>>,
    events: Option<Receiver<StreamEvent>>,
    opened: Option<Receiver<resonate_codec::Result<Unwrapped>>>,
    decoded: Option<Receiver<Returned>>,
}

impl Heard {
    fn registered<'a>(&'a self, commands: &'a Receiver<Request>) -> Select<'a> {
        let mut select = Select::new();
        select.recv(commands);
        if let Some(changes) = self.changes.as_ref() {
            select.recv(changes);
        }
        if let Some(surveyed) = self.surveyed.as_ref() {
            select.recv(surveyed);
        }
        if let Some(events) = self.events.as_ref() {
            select.recv(events);
        }
        if let Some(opened) = self.opened.as_ref() {
            select.recv(opened);
        }
        if let Some(decoded) = self.decoded.as_ref() {
            select.recv(decoded);
        }

        select
    }

    fn is_what(&self, engine: &Engine) -> bool {
        is_one_channel(self.changes.as_ref(), engine.changes.as_ref())
            && is_one_channel(
                self.events.as_ref(),
                engine.listening().map(SinkStream::events),
            )
            && is_one_channel(
                self.opened.as_ref(),
                engine.opening.as_ref().map(|opening| &opening.landed),
            )
            && is_one_channel(
                self.decoded.as_ref(),
                engine
                    .track
                    .as_ref()
                    .and_then(|track| track.decoding.awaited()),
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
        let surveyor = backend.surveyor();
        *published.sinks.write() = surveyor
            .enumerate_sinks(SINK_TIMEOUT)
            .unwrap_or_default()
            .into();
        let surveying = Surveying::start(Arc::clone(&surveyor), SINK_TIMEOUT);
        *published.settings.write() = Arc::new(OutputSettings::of(&config));

        Self {
            config,
            sources,
            backend,
            surveyor,
            surveying,
            commands,
            events,
            published,
            changes,
            queue: Queue::new(),
            opening: None,
            announced: None,
            stale_sinks: false,
            transport: TransportState::Idle,
            track: None,
            output: None,
            retiring: None,
            unbound: None,
            rebind_owed: false,
            graph_lost: None,
            graph_last_lost: None,
            graph_still_away: None,
            fill_owed: false,
            events_owed: VecDeque::new(),
            underruns_told_at: None,
            missing_untold: Frames::ZERO,
            waiting_for_a_device: false,
            device_last_lost: None,
            heard_at_least: None,
            playing: false,
            seeks: Seeks::default(),
            sleep: None,
            failures: 0,
            renegotiations: 0,
            sounded: None,
            turned: VecDeque::with_capacity(TURNS_REMEMBERED),
            answers: Vec::new(),
            openings_begun: 0,
            deferred: Vec::new(),
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
                self.land_the_opening();
                self.watch_discard();
                self.take_what_was_decoded();
                self.pump();
                self.finish_reshaping();
                self.finish_fading();
                self.promote();
                self.collect_faults();
                self.poll_stream();
                self.watch_graph();
                self.settle();
                self.let_the_link_rest();
                self.heed_the_measured_peak();
                self.settle_what_was_spooled();
                self.doze();
                self.hand_over_what_is_owed();
                self.answer_what_waited_on_the_opening_too_long();
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
        self.close_what_was_retiring();
        if let Some(surveying) = self.surveying.as_mut() {
            surveying.stop();
        }
        if let Err(error) = self.backend.shutdown() {
            tracing::warn!(%error, "the sink backend did not shut down cleanly");
        }
    }

    fn heard(&self) -> Heard {
        Heard {
            changes: self.changes.clone(),
            surveyed: self
                .surveying
                .as_ref()
                .map(|surveying| surveying.answers().clone()),
            events: self.listening().map(|stream| stream.events().clone()),
            opened: self.opening.as_ref().map(|opening| opening.landed.clone()),
            decoded: self
                .track
                .as_ref()
                .and_then(|track| track.decoding.awaited())
                .cloned(),
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
        if self.fill_owed {
            return Duration::ZERO;
        }
        let budget = if self.at_rest() {
            AT_REST_TICK
        } else {
            match self.output.as_ref() {
                None => IDLE_TICK,
                Some(output) => wait_for(
                    Frames(output.buffered() as u64).to_duration(output.plan.stream.rate),
                    self.playing || output.wants_more(),
                ),
            }
        };

        let budget = if self.is_fading() {
            budget.min(FADED_OVER)
        } else {
            budget
        };
        let budget = match self.sleep.and_then(Sleeping::left) {
            Some(left) if left > SLEEP_FADES_OVER => budget.min(left - SLEEP_FADES_OVER),
            Some(left) => budget.min(left),
            None => budget,
        };
        let budget = match self.deferred_answer_due_in() {
            Some(left) => budget.min(left),
            None => budget,
        };
        match self.kept_awake_for() {
            Some(left) => budget.min(left),
            None => budget,
        }
    }

    fn is_fading(&self) -> bool {
        self.retiring.is_some()
            || self
                .output
                .as_ref()
                .is_some_and(|output| output.quietening_since.is_some())
    }

    fn kept_awake_for(&self) -> Option<Duration> {
        let since = self.output.as_ref()?.awake_since?;
        Some(
            self.config
                .bluetooth
                .awake_for
                .saturating_sub(since.elapsed()),
        )
    }

    fn keeps_the_link_awake(&self) -> bool {
        self.config.bluetooth.on
            && self
                .output
                .as_ref()
                .is_some_and(|output| output.over_bluetooth && output.stream.is_some())
    }

    fn link_is_asleep(&self, sink: SinkId) -> bool {
        self.sounded
            .is_none_or(|(held, at)| held != sink || at.elapsed() >= LINK_NAPS_AFTER)
    }

    fn note_what_is_sounding(&mut self) {
        let Some(output) = self.output.as_ref() else {
            return;
        };
        let sounding = output.stream.is_some() && (self.playing || output.awake_since.is_some());
        if sounding {
            self.sounded = Some((output.sink, Instant::now()));
        }
    }

    fn wake_the_link(&self) {
        let Some(output) = self.output.as_ref() else {
            return;
        };
        if !self.config.bluetooth.on || !output.over_bluetooth || !self.link_is_asleep(output.sink)
        {
            return;
        }
        let lead = Frames::from_duration(self.config.bluetooth.lead, output.plan.stream.rate);
        output.producer.lead_in(lead);
    }

    fn let_the_link_rest(&mut self) {
        self.note_what_is_sounding();
        let Some(output) = self.output.as_mut() else {
            return;
        };
        let Some(since) = output.awake_since else {
            return;
        };
        let wake = self.config.bluetooth;
        if wake.on && since.elapsed() < wake.awake_for {
            return;
        }
        output.awake_since = None;
        if let Err(error) = output.activate(false) {
            tracing::warn!(%error, "the stream kept awake would not stand down");
        }
    }

    fn at_rest(&self) -> bool {
        if self.playing
            || self.is_fading()
            || self.sleep.is_some()
            || self.graph_lost.is_some()
            || self.stale_sinks
            || self.opening.is_some()
        {
            return false;
        }
        self.output.as_ref().is_none_or(|output| {
            !output.wants_more()
                && output.discarding_since.is_none()
                && output.settles_into.is_none()
                && !output.chain.is_ramping()
        })
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

    fn emit(&mut self, event: Event) {
        self.hand_over_what_is_owed();
        if !self.events_owed.is_empty() {
            self.owe(event);
            return;
        }
        match self.events.try_send(event) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => {}
            Err(TrySendError::Full(event)) => self.owe(event),
        }
    }

    fn owe(&mut self, event: Event) {
        if matches!(event, Event::Underrun { .. }) {
            return;
        }
        if self.events_owed.len() == EVENTS_OWED_AT_MOST {
            tracing::warn!("an engine event was dropped; nothing is draining the event channel");
            self.events_owed.pop_front();
        }
        self.events_owed.push_back(event);
    }

    fn hand_over_what_is_owed(&mut self) {
        while let Some(event) = self.events_owed.pop_front() {
            match self.events.try_send(event) {
                Ok(()) => {}
                Err(TrySendError::Full(event)) => {
                    self.events_owed.push_front(event);
                    return;
                }
                Err(TrySendError::Disconnected(_)) => {
                    self.events_owed.clear();
                    return;
                }
            }
        }
    }

    fn dispatch(&mut self, request: Request) {
        let Request {
            command,
            reply,
            queue_seen,
        } = request;
        let kind = command.kind();
        let begun = self.openings_begun;
        let now = self.queue.revision();
        let outcome = match queue_seen {
            Some(seen) if seen != now => Err(Error::QueueChanged { seen, now }),
            _ => self.apply(command),
        };
        let outcome = self.carried_on_past_a_stranded_row(outcome);

        if outcome.is_ok() && self.opening.is_some() && self.openings_begun != begun {
            self.deferred.push(Deferred {
                kind,
                reply,
                since: Instant::now(),
            });
            return;
        }
        self.reply(kind, reply, outcome);
    }

    fn answer_the_deferred(&mut self, outcome: Result<()>) {
        let mut outcome = Some(outcome);
        for Deferred { kind, reply, .. } in mem::take(&mut self.deferred) {
            let outcome = outcome.take().unwrap_or(Ok(()));
            self.reply(kind, reply, outcome);
        }
    }

    fn answer_what_waited_on_the_opening_too_long(&mut self) {
        if self
            .deferred
            .first()
            .is_some_and(|deferred| deferred.since.elapsed() >= OPENING_ANSWERED_WITHIN)
        {
            self.answer_the_deferred(Ok(()));
        }
    }

    fn deferred_answer_due_in(&self) -> Option<Duration> {
        self.deferred
            .first()
            .map(|deferred| OPENING_ANSWERED_WITHIN.saturating_sub(deferred.since.elapsed()))
    }

    fn reply(&mut self, kind: CommandKind, reply: Reply, outcome: Result<()>) {
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

    fn carried_on_past_a_stranded_row(&mut self, outcome: Result<()>) -> Result<()> {
        match outcome {
            Err(error) if error.track().is_some() && self.is_stranded() => {
                tracing::warn!(%error, "the row could not play on after the change and is passed over");
                self.fail(error);
                Ok(())
            }
            outcome => outcome,
        }
    }

    fn is_stranded(&self) -> bool {
        self.playing
            && self.track.is_some()
            && self.output.is_none()
            && self.opening.is_none()
            && !self.waiting_for_a_device
            && self.graph_lost.is_none()
    }

    fn announce_a_refusal(&mut self, command: CommandKind, outcome: Result<()>) {
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
                self.playing = autoplay && loaded;
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
                if self.playing && !self.is_stranded() {
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
                let to = Frames(if delta < 0 {
                    now.saturating_sub(delta.unsigned_abs())
                } else {
                    now.saturating_add(delta.unsigned_abs())
                });
                if self.lands_past_the_end(to) {
                    return self.next();
                }
                self.seek(to)
            }
            Command::Next => self.next(),
            Command::Previous => {
                self.failures = 0;
                if self.restarts_the_track() {
                    self.seek(Frames::ZERO)
                } else {
                    self.skipped_by_hand();
                    self.queue.retreat().ok_or(Error::QueueEmpty)?;
                    let started = self.start(Frames::ZERO);
                    self.past_what_will_not_open(started)
                }
            }
            Command::JumpTo(item) => self.hear(item),
            Command::SetVolume(volume) => {
                self.config.volume = volume;
                self.turn_the_device();
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
            Command::SetPreviousRestarts(previous) => {
                self.config.previous_restarts = previous;
                Ok(())
            }
            Command::SetShuffle(shuffle) => {
                self.queue.set_shuffle(shuffle);
                Ok(())
            }
            Command::SetSink(sink) => {
                self.config.sink = sink;
                self.rebind_where_it_stands()
            }
            Command::SetQuality(quality) => {
                self.config.quality = quality;
                self.rebind_where_it_stands()
            }
            Command::SetFilterPhase(phase) => {
                self.config.filter_phase = phase;
                self.rebind_where_it_stands()
            }
            Command::SetDither(dither) => {
                self.config.dither = dither;
                self.rebind_where_it_stands()
            }
            Command::SetRestoration(restoration) => {
                self.config.restoration = restoration;
                self.rebind_where_it_stands()
            }
            Command::SetTruePeak(on) => {
                self.config.true_peak = on;
                if let Some(track) = self.track.as_mut() {
                    track.replay_gain = levelled(&self.config, &track.info, track.hints);
                    track.published = None;
                    track.measure_where_wanted(&self.sources, &self.config);
                }
                self.rebind_where_it_stands()
            }
            Command::SetNoiseShaping(shaping) => {
                self.config.noise_shaping = shaping;
                self.rebind_where_it_stands()
            }
            Command::SetReplayGain(mode) => {
                self.config.replay_gain = mode;
                self.re_level()
            }
            Command::SetLevelling(levelling) => {
                self.config.levelling = levelling;
                self.re_level()
            }
            Command::SetConvolution(impulse) => {
                self.config.convolution = impulse;
                self.rebind_where_it_stands()
            }
            Command::SetEqualisation(equalisation) => {
                self.config.equaliser = equalisation;
                self.retune()
            }
            Command::SetBitPerfect(prefer) => {
                self.config.prefer_bit_perfect = prefer;
                self.reopen_where_the_stream_moves()
            }
            Command::SetDop(marked) => {
                self.config.dop = marked;
                self.reopen_where_the_stream_moves()
            }
            Command::SetDsdLikePcm(raised) => {
                self.config.dsd_like_pcm = raised;
                self.retune()
            }
            Command::SetDeviceMute(muted) => self.mute_the_device(muted),
            Command::SetDeviceVolume(handed) => {
                self.config.device_volume = handed;
                self.retune()
            }
            Command::SetBluetoothWake(wake) => {
                self.config.bluetooth = wake;
                self.let_the_link_rest();
                Ok(())
            }
            Command::SetForceGraphRate(force) => {
                self.config.force_graph_rate = force;
                self.rebind_where_it_stands()
            }
            Command::SetBuffer(buffer) => {
                self.config.buffer = buffer;
                self.reopen_where_the_stream_moves()
            }
            Command::SleepUntil(until) => {
                self.sleep = until.map(Sleeping::set);
                self.wake_from_the_sleep_fade();
                Ok(())
            }
            Command::Relocate(moved) => {
                self.queue.relocate(&moved);
                if let Some(track) = self.track.as_mut()
                    && let Some(to) = queue::landed_at(&track.location, &moved)
                {
                    track.location = to;
                }
                Ok(())
            }
        }
    }

    fn doze(&mut self) {
        self.fade_toward_sleep();
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

    fn sleep_is_due_in(&self) -> Option<Duration> {
        let sleeping = self.sleep?;
        if let Some(left) = sleeping.left() {
            return Some(left);
        }
        let ends_here = sleeping.ends_the_track()
            || (sleeping.ends_the_queue() && self.queue.ends_with_this_row());
        if !ends_here {
            return None;
        }
        let track = self.track.as_ref()?;
        let left = track.info.duration?.saturating_sub(self.position());
        Some(left.to_duration(track.info.spec.rate))
    }

    fn fade_toward_sleep(&mut self) {
        let Some(left) = self
            .sleep_is_due_in()
            .filter(|left| *left <= SLEEP_FADES_OVER)
        else {
            return;
        };
        let playing = self.playing;
        let Some(output) = self.output.as_mut() else {
            return;
        };
        output.note_the_pulls();
        let lifting = output
            .sleep_lifted
            .is_some_and(|lifted| lifted.elapsed() < output.quiet_within());
        if !playing || output.sleep_fading || lifting || !output.is_sounding() {
            return;
        }
        output.sleep_fading = true;
        output
            .producer
            .fade_out(Frames::from_duration(left, output.plan.stream.rate));
    }

    fn wake_from_the_sleep_fade(&mut self) {
        if self
            .sleep_is_due_in()
            .is_some_and(|left| left <= SLEEP_FADES_OVER)
        {
            return;
        }
        self.lift_the_sleep_fade();
    }

    fn lift_the_sleep_fade(&mut self) {
        let playing = self.playing;
        let Some(output) = self.output.as_mut().filter(|output| output.sleep_fading) else {
            return;
        };
        output.sleep_fading = false;
        output.sleep_lifted = Some(Instant::now());
        if playing {
            output.producer.fade_in(output.fade());
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
            track.measure_where_wanted(&self.sources, &self.config);
        }
        self.retune()
    }

    fn settle_what_was_spooled(&mut self) {
        let Some(track) = self.track.as_mut() else {
            return;
        };
        if track.info.is_seekable {
            return;
        }
        let Some(settled) = track.decoding.home().and_then(Decoder::settle_the_spool) else {
            return;
        };
        let seekable = settled.is_seekable;
        let info = Arc::make_mut(&mut track.info);
        info.is_seekable = seekable;
        info.duration = settled.duration;
        info.playable = settled.playable;
        track.published = None;
        if !seekable || !mem::take(&mut self.rebind_owed) {
            return;
        }
        if let Err(error) = self.rebind_where_it_stands() {
            self.fail(error);
        }
    }

    fn heed_the_measured_peak(&mut self) {
        let heeded = self
            .track
            .as_mut()
            .is_some_and(|track| track.heed_what_was_measured(&self.config));
        if !heeded {
            return;
        }
        if let Err(error) = self.retune() {
            tracing::warn!(%error, "the peak measured for the playing track could not be heeded");
        }
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
        let nothing_to_start =
            self.track.is_none() && self.opening.is_none() && self.queue.current().is_none();
        if nothing_to_start {
            return Err(Error::QueueEmpty);
        }
        self.playing = true;
        if self.opening.is_some() {
            return Ok(());
        }
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
        if let Some(output) = self.output.as_mut() {
            output.producer.fade_in(output.fade());
            output.quietening_since = None;
            if output.sleep_fading {
                output.sleep_lifted = Some(Instant::now());
            }
            output.sleep_fading = false;
            if output.awake_since.take().is_some() {
                return Ok(());
            }
        }
        self.wake_the_link();
        self.set_active(true)
    }

    fn pause(&mut self) -> Result<()> {
        if self.opening.is_some() {
            self.playing = false;
            return Ok(());
        }
        if self.track.is_none() {
            return Err(Error::InvalidTransition {
                state: self.transport,
                attempted: CommandKind::Pause,
            });
        }
        self.playing = false;
        if self.transport == TransportState::Playing
            || (self.transport == TransportState::Loading
                && self.output.is_none()
                && self.unbound.is_some())
        {
            self.transport = TransportState::Paused;
        }
        let keeps_the_link_awake = self.keeps_the_link_awake();
        let Some(output) = self.output.as_mut() else {
            return Ok(());
        };
        output.note_the_pulls();
        output.producer.fade_out(output.fade());
        if keeps_the_link_awake {
            output.awake_since = Some(Instant::now());
            return Ok(());
        }
        if output.is_sounding() {
            output.quietening_since = Some(Instant::now());
            return Ok(());
        }
        output.activate(false)
    }

    fn set_active(&mut self, active: bool) -> Result<()> {
        let Some(output) = self.output.as_mut() else {
            return Ok(());
        };
        output.activate(active)
    }

    fn retire_the_output(&mut self) {
        let Some(mut output) = self.output.take() else {
            return;
        };
        output.note_the_pulls();
        self.close_what_was_retiring();
        if !output.is_sounding() {
            output.close();
            return;
        }
        output.producer.fade_out(output.fade());
        self.retiring = Some(Retiring {
            output,
            since: Instant::now(),
        });
    }

    fn close_what_was_retiring(&mut self) {
        if let Some(mut retiring) = self.retiring.take() {
            retiring.output.close();
        }
    }

    fn finish_fading(&mut self) {
        let retired = self
            .retiring
            .as_ref()
            .is_some_and(|retiring| retiring.output.has_gone_quiet(retiring.since));
        if retired {
            self.close_what_was_retiring();
        }
        let Some(output) = self.output.as_mut() else {
            return;
        };
        output.note_the_pulls();
        let quiet = output
            .quietening_since
            .is_some_and(|since| output.has_gone_quiet(since));
        if !quiet {
            return;
        }
        output.quietening_since = None;
        if let Err(error) = output.activate(false) {
            tracing::warn!(%error, "the stream would not stand down once it had faded out");
        }
    }

    fn stop(&mut self) {
        self.playing = false;
        self.failures = 0;
        self.retire_the_output();
        self.track = None;
        self.opening = None;
        self.answer_the_deferred(Ok(()));
        self.unbound = None;
        self.heard_at_least = None;
        self.transport = TransportState::Stopped;
    }

    fn remove(&mut self, rows: Span) -> Result<()> {
        let removal = self.queue.remove_rows(rows).ok_or(Error::NoSuchRow {
            rows,
            len: self.queue.len(),
        })?;
        if removal == Removal::Queued || (self.track.is_none() && self.opening.is_none()) {
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

    fn restarts_the_track(&mut self) -> bool {
        let Some((rate, duration)) = self
            .track
            .as_ref()
            .map(|track| (track.source().rate, track.info.duration))
        else {
            return false;
        };
        let heard = self.heard_position();
        self.config
            .previous_restarts
            .starts_the_track_over(heard, duration, rate)
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
        let leaving = self.queue.current().map(|item| item.id);

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
        if self.queue.current().map(|item| item.id) == leaving {
            self.seeks = self.seeks.stepped();
        }
        self.start(Frames::ZERO)
    }

    fn next(&mut self) -> Result<()> {
        self.failures = 0;
        self.skipped_by_hand();
        let skipped = self.skip(false);
        self.past_what_will_not_open(skipped)
    }

    fn lands_past_the_end(&self, to: Frames) -> bool {
        self.opening.is_none()
            && self
                .track
                .as_ref()
                .and_then(|track| track.info.duration)
                .is_some_and(|duration| to >= duration)
    }

    fn seek(&mut self, to: Frames) -> Result<()> {
        if let Some(opening) = self.opening.as_mut() {
            opening.at = to;
            return Ok(());
        }
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
        if !track.info.is_seekable {
            return self.seek_where_nothing_seeks(to);
        }
        let landed = if self.reuses_stream() {
            self.seek_in_place(to)
        } else {
            self.rebind(Some(to), None)
        };
        if landed.is_ok() {
            self.seeks = self.seeks.stepped();
            self.heard_at_least = None;
            self.lift_the_sleep_fade();
        }
        landed
    }

    fn seek_where_nothing_seeks(&mut self, to: Frames) -> Result<()> {
        let Some(track) = self.track.as_ref() else {
            return Ok(());
        };
        if self.position() == to {
            return Ok(());
        }
        if to == Frames::ZERO {
            self.seeks = self.seeks.stepped();
            return self.start(Frames::ZERO);
        }
        Err(Error::Decode {
            track: track.id,
            source: resonate_codec::Error::NotSeekable {
                location: track.location.clone(),
            },
        })
    }

    fn rebind_where_it_stands(&mut self) -> Result<()> {
        let streaming_what_cannot_seek = self.output.is_some()
            && self
                .track
                .as_ref()
                .is_some_and(|track| !track.info.is_seekable);
        if streaming_what_cannot_seek {
            tracing::debug!(
                "a track that cannot seek keeps its stream until it can or the next begins"
            );
            self.rebind_owed = true;
            return Ok(());
        }
        let at = self.position();
        self.rebind(Some(at), None)
    }

    fn reuses_stream(&self) -> bool {
        let (Some(track), Some(output)) = (self.track.as_ref(), self.output.as_ref()) else {
            return false;
        };
        self.playing && track.info.is_seekable && output.holds_a_live_stream()
    }

    fn seek_in_place(&mut self, to: Frames) -> Result<()> {
        if let Some(track) = self.track.as_mut() {
            track.seek(to)?;
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

        self.retire_the_output();
        self.track = None;
        if self.opening.take().is_some() {
            self.answer_the_deferred(Ok(()));
        }
        self.heard_at_least = None;
        self.rebind_owed = false;

        self.opening = Some(Opening::begin(item, at, &self.sources));
        self.openings_begun = self.openings_begun.wrapping_add(1);
        self.transport = TransportState::Loading;
        Ok(())
    }

    fn land_the_opening(&mut self) {
        let Some(landed) = self.opening.as_ref().and_then(Opening::landed) else {
            return;
        };
        let Some(Opening { item, at, .. }) = self.opening.take() else {
            return;
        };
        let started = landed.and_then(|unwrapped| self.started(&item, unwrapped, at));
        if !self.deferred.is_empty() {
            self.answer_for_the_opening(started);
            return;
        }
        let Err(error) = started else {
            return;
        };
        if self.playing {
            self.fail(error);
            return;
        }
        self.transport = TransportState::Stopped;
        self.emit(Event::Failed {
            track: item.id,
            error,
        });
    }

    fn answer_for_the_opening(&mut self, started: Result<()>) {
        let outcome = self.past_what_will_not_open(started);
        let outcome = self.carried_on_past_a_stranded_row(outcome);
        if outcome.is_ok() && self.opening.is_some() {
            return;
        }
        if outcome.is_err() && self.track.is_none() && self.transport == TransportState::Loading {
            self.transport = TransportState::Stopped;
        }
        self.answer_the_deferred(outcome);
    }

    fn started(&mut self, item: &QueueItem, unwrapped: Unwrapped, at: Frames) -> Result<()> {
        let track = Track::of(item, unwrapped, &self.sources, &self.config);
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

    fn reopen_where_the_stream_moves(&mut self) -> Result<()> {
        if self.keeps_its_stream() {
            return Ok(());
        }
        self.rebind_where_it_stands()
    }

    fn keeps_its_stream(&self) -> bool {
        let (Some(track), Some(output)) = (self.track.as_ref(), self.output.as_ref()) else {
            return false;
        };
        if output.plan.stream != output.status.negotiated {
            return false;
        }
        let Some(sink) = self.bound_sink() else {
            return false;
        };
        let wanted = plan_output(track.decoded(), &sink, &self.config, track.replay_gain);
        let capacity = ring_capacity(
            self.config.buffer,
            wanted.stream,
            output.chain.max_process_frames(),
        );
        wanted.stream == output.plan.stream
            && wanted.packing == output.plan.packing
            && capacity.get() as usize == output.capacity
    }

    fn rebind(&mut self, at: Option<Frames>, target: Option<StreamSpec>) -> Result<()> {
        if self.track.is_none() {
            return Ok(());
        }
        let resumes_at = at.unwrap_or_else(|| self.position());
        self.retire_the_output();

        let bound = self.bind(at, target);
        if bound.is_err() && self.track.is_some() {
            self.unbound = Some(resumes_at);
        }
        bound
    }

    fn bind(&mut self, at: Option<Frames>, target: Option<StreamSpec>) -> Result<()> {
        if let Some(track) = self.track.as_mut()
            && let Some(at) = at
            && track.decoding.position() != at
            && track.info.is_seekable
        {
            track.seek(at)?;
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
        let entering = if at.unwrap_or_else(|| track.decoding.position()) > Frames::ZERO {
            Entering::FadedIn
        } else {
            Entering::Whole
        };
        let output = Output::open(
            track,
            &sink,
            &self.config,
            target,
            &self.published.listening,
            entering,
        )?;

        let delivery = output.plan.delivery();
        if let Some(track) = self.track.as_mut() {
            track.decoding.deliver(delivery);
            track.rewind_carry();
            track.restart_profile();
        }

        if output.attenuator == Attenuator::Device {
            self.take_the_devices_volume(&sink);
        }
        let status = output.status;
        self.output = Some(output);
        self.transport = TransportState::Loading;
        self.emit(Event::OutputChanged(status));
        Ok(())
    }

    fn retune(&mut self) -> Result<()> {
        self.weigh_whose_volume_it_is();
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
            return self.rebind_where_it_stands();
        }
        let wanted = plan_for(
            track.decoded(),
            &output.bound,
            output.plan.stream,
            &self.config,
            replay_gain,
            output.attenuator,
        );

        if wanted.same_shape_as(&output.plan) {
            output
                .chain
                .set_gain(output.attenuator.leaves(self.config.volume), replay_gain);
            if let Some(profile) = wanted.equalisation.as_ref() {
                output.chain.set_equalisation(profile);
                output.chain.ease_equalisation(Easing::Returning);
            }
            output.plan = wanted;
            return Ok(());
        }
        if output.plan.becomes_on_the_same_stream(&wanted) {
            return self.reshape(wanted);
        }
        self.rebind_where_it_stands()
    }

    fn reshape(&mut self, wanted: OutputPlan) -> Result<()> {
        let (Some(track), Some(output)) = (self.track.as_ref(), self.output.as_mut()) else {
            return Ok(());
        };
        let drops_a_gain_stage = wanted.gain.is_none() && output.chain.gain_amplitude().is_some();
        if drops_a_gain_stage {
            output.chain.set_gain(
                output.attenuator.leaves(self.config.volume),
                track.replay_gain,
            );
        }
        let drops_the_equaliser =
            wanted.equalisation.is_none() && output.plan.equalisation.is_some();
        if drops_the_equaliser && self.playing {
            output.chain.ease_equalisation(Easing::Leaving);
        }
        if (drops_a_gain_stage || drops_the_equaliser) && output.chain.is_ramping() {
            output.settles_into = Some(wanted);
            return Ok(());
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
        let still_filling = output.draining.is_none() && !output.ended;
        let carries = output.plan.carries_the_front_into(&wanted);
        if still_filling && !output.has_room_for_what_the_chain_holds(carries) {
            output.settles_into = Some(wanted);
            return Ok(());
        }
        let front = carries.then(|| output.chain.take_the_front()).flatten();
        if still_filling {
            output.hand_on_what_the_chain_holds();
        }
        let source = track.source();
        let built = match front {
            Some(front) => wanted.build_chain_after(front, &self.config, CHAIN_BLOCK),
            None => wanted.build_chain(source, &self.config, CHAIN_BLOCK),
        };
        let mut chain = built.map_err(|error| Error::Convert {
            track: track.id,
            source_spec: source,
            sink_spec: wanted.stream,
            source: error,
        })?;
        chain.ramp_gain_from(output.chain.gain_amplitude().unwrap_or(Gain::UNITY.get()));
        let brings_the_equaliser =
            wanted.equalisation.is_some() && output.plan.equalisation.is_none();
        if brings_the_equaliser && self.playing {
            chain.ease_equalisation(Easing::Entering);
        }

        let delivery = wanted.delivery();
        track.decoding.deliver(delivery);
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
        if let Some(found) = self.surveying.as_mut().and_then(Surveying::answered) {
            self.take_the_survey(found);
        }
        if !self.stale_sinks {
            return;
        }

        match self.surveying.as_mut() {
            Some(surveying) => {
                if surveying.ask() {
                    self.stale_sinks = false;
                }
            }
            None if self.output.is_none() => {
                self.stale_sinks = false;
                let found = self.surveyor.enumerate_sinks(SINK_TIMEOUT);
                self.take_the_survey(found);
            }
            None => {}
        }
    }

    fn take_the_survey(&mut self, found: Surveyed) {
        match found {
            Ok(found) => {
                *self.published.sinks.write() = found.into();
                self.follow_the_devices_volume();
                self.follow_the_sink_it_would_choose();
                self.bind_the_row_waiting_for_a_device();
                self.bind_the_row_the_graph_let_go();
            }
            Err(error) if self.graph_lost.is_some() => {
                tracing::debug!(%error, "the graph is not back yet");
                self.graph_still_away = Some(Error::Sink(error));
            }
            Err(error) => tracing::warn!(%error, "the sink list could not be refreshed"),
        }
    }

    fn bound_sink(&self) -> Option<SinkInfo> {
        let bound = self.output.as_ref()?.sink;
        self.published
            .sinks
            .read()
            .iter()
            .find(|sink| sink.id == bound)
            .cloned()
    }

    fn weigh_whose_volume_it_is(&mut self) {
        let Some(sink) = self.bound_sink() else {
            return;
        };
        let attenuator = Attenuator::of(&self.config, &sink);
        let Some(output) = self.output.as_mut() else {
            return;
        };
        output.status.device_turned = attenuator == Attenuator::Device;
        output.status.device_muted = attenuator.hears_the_mute_of(&sink);
        if output.attenuator == attenuator {
            return;
        }
        output.attenuator = attenuator;
        if attenuator == Attenuator::Device {
            self.take_the_devices_volume(&sink);
        }
    }

    fn take_the_devices_volume(&mut self, sink: &SinkInfo) {
        let Some(heard) = sink.port.as_ref().and_then(|port| port.volume) else {
            return;
        };
        self.config.volume = Volume::heard_at(heard);
    }

    fn follow_the_devices_volume(&mut self) {
        let turned_here = self
            .output
            .as_ref()
            .is_some_and(|output| output.attenuator == Attenuator::Device);
        if !turned_here {
            return;
        }
        let Some(sink) = self.bound_sink() else {
            return;
        };
        if let Some(output) = self.output.as_mut() {
            output.status.device_muted = output.attenuator.hears_the_mute_of(&sink);
        }
        let Some(heard) = sink.port.as_ref().and_then(|port| port.volume) else {
            return;
        };
        let ours = self.config.volume.to_gain();
        let echoed = iter::once(&ours)
            .chain(&self.turned)
            .any(|turned| (turned.get() - heard.get()).abs() < ONE_LEVEL_WITHIN);
        if echoed {
            return;
        }
        tracing::debug!(%heard, "the device's volume was turned from elsewhere; the slider follows it");
        self.config.volume = Volume::heard_at(heard);
    }

    fn turn_the_device(&mut self) {
        let Some(output) = self
            .output
            .as_ref()
            .filter(|output| output.attenuator == Attenuator::Device)
        else {
            return;
        };
        let gain = self.config.volume.to_gain();
        if let Err(error) = self.backend.set_device_volume(output.sink, gain) {
            tracing::warn!(%error, "the device's volume could not be turned");
            return;
        }
        if self.turned.len() == TURNS_REMEMBERED {
            self.turned.pop_front();
        }
        self.turned.push_back(gain);
    }

    fn mute_the_device(&self, muted: bool) -> Result<()> {
        let Some(output) = self
            .output
            .as_ref()
            .filter(|output| output.attenuator == Attenuator::Device)
        else {
            tracing::debug!("no device turns the volume here, so it has no mute to set");
            return Ok(());
        };
        Ok(self.backend.set_device_mute(output.sink, muted)?)
    }

    fn follow_the_sink_it_would_choose(&mut self) {
        let Some(bound) = self.output.as_ref().map(|output| output.sink) else {
            return;
        };
        let chosen = chosen_sink(&self.published.sinks.read(), self.config.sink.as_ref())
            .map(|sink| sink.id);
        if chosen.is_none_or(|chosen| chosen == bound) {
            return;
        }

        tracing::info!(
            from = %bound,
            "the device this build would play on moved; the stream follows it"
        );
        let at = self.position();
        if let Err(error) = self.rebind(Some(at), None) {
            self.fail(error);
        }
    }

    fn select_sink(&mut self) -> Result<SinkInfo> {
        self.note_sink_changes();
        if self.sinks_are_stale() {
            self.stale_sinks = false;
            match self.surveyor.enumerate_sinks(SINK_TIMEOUT) {
                Ok(found) => *self.published.sinks.write() = found.into(),
                Err(error) if self.published.sinks.read().is_empty() => return Err(error.into()),
                Err(error) => {
                    tracing::warn!(%error, "the sink list could not be refreshed; binding to what was last published");
                }
            }
        }

        chosen_sink(&self.published.sinks.read(), self.config.sink.as_ref())
            .cloned()
            .ok_or(Error::Sink(resonate_pipewire::Error::NoSink))
    }

    fn sinks_are_stale(&self) -> bool {
        self.stale_sinks || self.changes.is_none() || self.published.sinks.read().is_empty()
    }

    fn take_what_was_decoded(&mut self) {
        if let Some(track) = self.track.as_mut() {
            track.decoding.take_what_came_back();
        }
    }

    fn pump(&mut self) {
        self.fill_owed = false;
        let outcome = {
            let (Some(track), Some(output)) = (self.track.as_mut(), self.output.as_mut()) else {
                return;
            };
            if output.ended {
                return;
            }
            Self::fill(track, output)
        };
        match outcome {
            Ok(filled) => self.fill_owed = filled == Filled::ForNow,
            Err(error) => self.fail(error),
        }
    }

    fn fill(track: &mut Track, output: &mut Output) -> Result<Filled> {
        if output.producer.is_discarding() {
            return Ok(Filled::AsFarAsItGoes);
        }
        if output.draining.is_some() {
            Self::drain(output);
            return Ok(Filled::AsFarAsItGoes);
        }

        let began = Instant::now();
        loop {
            if began.elapsed() >= FILLED_IN_ONE_GO {
                return Ok(Filled::ForNow);
            }
            if track.undecoded() == 0 {
                track.decoded_at = 0;
                match track.next_block()? {
                    Block::Decoded => {}
                    Block::Ended => {
                        Self::flush(output);
                        return Ok(Filled::AsFarAsItGoes);
                    }
                    Block::Awaited => return Ok(Filled::WhenTheSourceAnswers),
                }
                track.sample_packet();
                if track.decoded.frames() == 0 {
                    continue;
                }
            }

            if output.plan.is_transparent() {
                let written = output.producer.write_from(&track.decoded, track.decoded_at);
                if written == 0 {
                    return Ok(Filled::AsFarAsItGoes);
                }
                if let Some(tapping) = output.tapping.as_mut() {
                    tapping.record(&track.decoded, track.decoded_at, written);
                }
                track.decoded_at = track.decoded_at.saturating_add(written);
            } else {
                if output.producer.free_frames() < output.chain.max_process_frames()
                    || output.waits_for_room_to_reshape()
                {
                    return Ok(Filled::AsFarAsItGoes);
                }
                if !Self::convert(track, output) {
                    return Ok(Filled::AsFarAsItGoes);
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
        if output.stream.is_some() || !output.primed() || self.retiring.is_some() {
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
            realtime: true,
        };

        let Some(consumer) = self
            .output
            .as_mut()
            .and_then(|output| output.consumer.take())
        else {
            return;
        };
        if self.playing {
            self.wake_the_link();
        }

        match self.backend.open(&request, Box::new(consumer)) {
            Ok(stream) => {
                let active = match stream.set_active(self.playing) {
                    Ok(()) => self.playing,
                    Err(error) => {
                        tracing::warn!(%error, "the stream would not take its initial active state");
                        false
                    }
                };
                if let Some(output) = self.output.as_mut() {
                    output.stream = Some(stream);
                    output.active = active;
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

    fn row_the_graph_let_go(&self) -> Option<Frames> {
        self.unbound
            .filter(|_| self.playing && self.track.is_some() && self.output.is_none())
    }

    fn wait_for_the_graph(&mut self, since: Instant) {
        if self.opening.is_some() {
            return;
        }
        let Some(at) = self.row_the_graph_let_go() else {
            self.graph_lost = None;
            self.graph_still_away = None;
            return;
        };
        if self.surveying.is_none() {
            self.wait_for_the_graph_in_line(since, at);
            return;
        }
        if since.elapsed() >= GRAPH_BACK_WITHIN {
            self.graph_lost = None;
            let away = self
                .graph_still_away
                .take()
                .unwrap_or(Error::Sink(resonate_pipewire::Error::Disconnected));
            self.fail(away);
            return;
        }
        self.stale_sinks = true;
        self.transport = TransportState::Loading;
    }

    fn bind_the_row_the_graph_let_go(&mut self) {
        if self.graph_lost.is_none() {
            return;
        }
        let Some(at) = self.row_the_graph_let_go() else {
            return;
        };
        match self.rebind(Some(at), None) {
            Ok(()) => {
                tracing::info!("the graph is back; the row plays on from where it was heard");
                self.unbound = None;
                self.graph_lost = None;
                self.graph_still_away = None;
            }
            Err(error) => {
                tracing::debug!(%error, "the graph is not back yet");
                self.graph_still_away = Some(error);
                self.transport = TransportState::Loading;
            }
        }
    }

    fn wait_for_the_graph_in_line(&mut self, since: Instant, at: Frames) {
        let back = self
            .surveyor
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

        if !playing || output.ended {
            return;
        }
        output.status.went_without = output.status.went_without.saturating_add(missing);
        if seen == 0 {
            return;
        }
        output.status.underruns = output.status.underruns.saturating_add(seen);
        self.missing_untold = self.missing_untold.saturating_add(missing);
        if self
            .underruns_told_at
            .is_some_and(|told| told.elapsed() < UNDERRUNS_TOLD_EVERY)
        {
            return;
        }
        self.underruns_told_at = Some(Instant::now());
        let missing = mem::replace(&mut self.missing_untold, Frames::ZERO);
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
                StreamEvent::FormatChanged { spec, words } => {
                    output.status.words = Some(words);
                    if spec != output.plan.stream {
                        renegotiated = Some(spec);
                    }
                }
                StreamEvent::Drained if output.tail == Tail::Asked => output.tail = Tail::Played,
                StreamEvent::Drained => {}
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

    fn drain_events(output: &mut Output) -> Drained {
        let mut events = Drained::new();
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

    fn parked_for_a_device(&mut self, error: Error) -> Result<()> {
        use resonate_pipewire::Error as Sink;

        let the_graph_is_away = self.graph_lost.is_some()
            && matches!(
                error,
                Error::Sink(Sink::Disconnected | Sink::LoopStopped | Sink::Daemon { .. })
            );
        if the_graph_is_away && self.track.is_some() {
            tracing::warn!(%error, "the graph is away; the row waits for it to come back");
            let at = self.unbound.unwrap_or_else(|| self.heard_position());
            if let Some(output) = self.output.as_mut() {
                output.close();
            }
            self.output = None;
            self.unbound = Some(at);
            self.transport = if self.playing {
                TransportState::Loading
            } else {
                TransportState::Paused
            };
            return Ok(());
        }

        let had_a_stream = match &error {
            Error::Sink(Sink::NoSink) => false,
            Error::Sink(Sink::SinkGone { .. } | Sink::StreamFailed { .. }) => true,
            _ => return Err(error),
        };
        let Some(track) = self.track.as_ref().map(|track| track.id) else {
            return Err(error);
        };
        if had_a_stream {
            let again = self
                .device_last_lost
                .is_some_and(|last| last.elapsed() < GRAPH_BACK_WITHIN);
            if again {
                return Err(error);
            }
            self.device_last_lost = Some(Instant::now());
        }

        tracing::warn!(%error, "the device went; the row waits for one to play on");
        let at = self.unbound.unwrap_or_else(|| self.heard_position());
        if let Some(output) = self.output.as_mut() {
            output.close();
        }
        self.output = None;
        self.unbound = Some(at);
        self.transport = if self.playing {
            TransportState::Loading
        } else {
            TransportState::Paused
        };
        self.waiting_for_a_device = true;
        self.stale_sinks = true;
        self.emit(Event::Waiting { track, error });
        Ok(())
    }

    fn bind_the_row_waiting_for_a_device(&mut self) {
        if !self.waiting_for_a_device {
            return;
        }
        let at = self
            .unbound
            .filter(|_| self.track.is_some() && self.output.is_none());
        let Some(at) = at else {
            self.waiting_for_a_device = false;
            return;
        };
        if !self.playing {
            return;
        }

        match self.rebind(Some(at), None) {
            Ok(()) => {
                tracing::info!("a device is there again; the row plays on from where it was heard");
                self.unbound = None;
                self.waiting_for_a_device = false;
            }
            Err(Error::Sink(resonate_pipewire::Error::NoSink)) => {
                tracing::debug!("still no device to play through");
                self.transport = TransportState::Loading;
            }
            Err(error) => {
                self.waiting_for_a_device = false;
                self.fail(error);
            }
        }
    }

    fn fail(&mut self, error: Error) {
        let mut error = error;
        loop {
            error = match self.parked_for_a_device(error) {
                Ok(()) => return,
                Err(error) => error,
            };
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
            .decoding
            .position()
            .saturating_sub(Frames(track.undecoded() as u64));

        let Some(output) = self.output.as_ref() else {
            return self.unbound.unwrap_or(decoded);
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
            previous_restarts: self.config.previous_restarts,
            shuffle: self.queue.shuffle(),
            queue_position: self.queue.cursor(),
            loaded_position: self.queue.position(),
            queue_len: self.queue.len(),
            queue_stamp: self.queue.playing_from(),
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
            next: self.queue.playing_next(),
            stamp: self.queue.stamp(),
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
    fn the_set_parks_on_the_commands_the_sink_changes_the_survey_and_the_stream_in_that_order() {
        let (_orders, commands) = unbounded::<Request>();
        let (announce, changes) = unbounded::<SinkChange>();
        let (answer, surveyed) = unbounded::<Surveyed>();
        let (report, events) = unbounded::<StreamEvent>();
        let heard = Heard {
            changes: Some(changes.clone()),
            surveyed: Some(surveyed.clone()),
            events: Some(events.clone()),
            opened: None,
            decoded: None,
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

        answer
            .send(Ok(Vec::new()))
            .expect("the survey's answer lands");
        assert_eq!(parked.ready_timeout(SHORTEST_TICK), Ok(2));
        let _ = surveyed.try_recv().expect("the answer is read back");

        report.send(StreamEvent::Drained).expect("the event lands");
        assert_eq!(parked.ready_timeout(SHORTEST_TICK), Ok(3));
        events.try_recv().expect("the event is read back");

        assert!(parked.ready_timeout(SHORTEST_TICK).is_err());
    }

    #[test]
    fn a_set_with_no_stream_and_no_announcements_parks_on_the_commands_alone() {
        let (orders, commands) = unbounded::<Request>();
        let heard = Heard {
            changes: None,
            surveyed: None,
            events: None,
            opened: None,
            decoded: None,
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
