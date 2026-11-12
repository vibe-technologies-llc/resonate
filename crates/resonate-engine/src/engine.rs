use std::{
    collections::VecDeque,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Select, Sender, TryRecvError};
use resonate_audio::{SinkChange, SinkId, SinkStream, StreamEvent};
use resonate_codec::{BoxLayout, Decoder, MediaInfo, ProfileBuilder, ReplayGain, Sources};
use resonate_core::{
    AppliedGain, AudioBuffer, FrameSpan, Frames, MeasuredGain, MediaLocation, QueueStamp,
    SampleRate, StreamSpec, TrackHints, TrackId,
};
use resonate_dsp::Chain;
use smallvec::SmallVec;

use self::output::{Kept, Output, Retiring};
use crate::{
    Backend, CommandKind, EngineConfig, Error, Event, HeardClock, Opener, OutputPlan, Published,
    ReplayGainMode, Reply, Request, Result, Seeks, Sleeping, Stepping, StreamDigest, Surveyor,
    TransportState,
    audition::{Audition, Contested},
    lending::{Block, Decoding, Lost, Returned},
    measure::{Measured, Measuring},
    pipeline::{Decoded, resolve_replay_gain},
    queue::{Queue, QueueItem},
    ring::{FADED_OVER, Turned},
    shuffling::{KinAsking, KinLanded},
    streaming::{Streamed, Streaming},
    surveying::{Surveyed, Surveying},
    turns::Turns,
};

mod arms;
mod auditioning;
mod binding;
mod boundary;
mod commands;
mod dozing;
mod filling;
mod graph;
mod in_step;
mod levels;
mod moving;
mod opening;
mod output;
mod publishing;
mod reshaping;
mod resting;
mod seeking;
mod sinks;
mod splicing;
mod transport;

const SINK_TIMEOUT: Duration = Duration::from_secs(2);
const SHORTEST_TICK: Duration = Duration::from_millis(4);
const DOWNSTREAM_RESTARTED_AFTER_QUIET_FOR: Duration = Duration::from_millis(300);
const PUBLISH_TICK: Duration = Duration::from_millis(16);
const IDLE_TICK: Duration = Duration::from_millis(100);
const AT_REST_TICK: Duration = Duration::from_secs(1);
const EVENTS_A_PASS_HELD_INLINE: usize = 4;

type Drained = SmallVec<[StreamEvent; EVENTS_A_PASS_HELD_INLINE]>;
const CHAIN_BLOCK: usize = 1024;
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
const RESHAPED_FORMAT_SETTLED_WITHIN: Duration = Duration::from_secs(1);
const OPENED_AHEAD_BY: Duration = Duration::from_secs(10);
const HANDED_ON_WHILE_THE_RING_HOLDS: Duration = Duration::from_millis(40);
const IN_STEP_WITHIN: Duration = Duration::from_secs(3);
const RELEASED_AHEAD_BY: Duration = Duration::from_millis(30);
const PULLS_RELEASED_AHEAD_BY: u64 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReadBy {
    PlanAlone,
    Resampler,
    Dither,
}

impl ReadBy {
    const fn reaches_into(self, plan: &OutputPlan) -> bool {
        match self {
            Self::PlanAlone => false,
            Self::Resampler => plan.resamples(),
            Self::Dither => plan.dithers(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Filled {
    AsFarAsItGoes,
    ForNow,
    WhenTheSourceAnswers,
    AtAChange,
    AtTheEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Handing {
    Handed,
    Waits,
    Refused,
}

struct Track {
    id: TrackId,
    heard_as: QueueStamp,
    location: MediaLocation,
    span: Option<FrameSpan>,
    decoding: Decoding,
    info: Arc<MediaInfo>,
    sounding: StreamSpec,
    at_a_change: bool,
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
    ended: bool,
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
    fn decoded(&self) -> Decoded {
        Decoded::of(&self.info, self.hints.lowpass)
    }

    fn open(item: &QueueItem, sources: &Sources) -> resonate_codec::Result<Self> {
        let (mut decoder, info) = match item.span {
            Some(span) => Decoder::open_span(sources, &item.location, span)?,
            None => Decoder::open(sources, &item.location)?,
        };
        decoder.follow_format_changes();
        let layout = decoder.layout().cloned();
        Ok(Self {
            decoder,
            info,
            hints: sources.hints(&item.location, item.span),
            layout,
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

    fn already(item: QueueItem, at: Frames, came: resonate_codec::Result<Unwrapped>) -> Self {
        let (landing, landed) = crossbeam_channel::bounded(1);
        let _ = landing.send(came);
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

struct Ahead {
    opening: Opening,
    came: Option<resonate_codec::Result<Unwrapped>>,
    lost: bool,
}

impl Ahead {
    fn begin(item: QueueItem, sources: &Arc<Sources>) -> Self {
        Self {
            opening: Opening::begin(item, Frames::ZERO, sources),
            came: None,
            lost: false,
        }
    }

    fn look(&mut self) -> bool {
        if self.came.is_some() {
            return true;
        }
        match self.opening.landed.try_recv() {
            Ok(came) => {
                self.came = Some(came);
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                self.lost = true;
                false
            }
        }
    }

    fn awaited(&self) -> Option<&Receiver<resonate_codec::Result<Unwrapped>>> {
        (self.came.is_none() && !self.lost).then_some(&self.opening.landed)
    }

    fn opening_at(self, at: Frames) -> Opening {
        match self.came {
            Some(came) => Opening::already(self.opening.item, at, came),
            None => Opening { at, ..self.opening },
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Waiting {
    ForTheBoundary,
    NoLonger,
}

struct Boundary {
    at: u64,
    made: u64,
    turned: Turned,
    leaving: Track,
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
            heard_as: QueueStamp::of([(&item.location, item.span)]),
            location: item.location.clone(),
            span: item.span,
            hints,
            peak: Peak::Unasked,
            decoded: AudioBuffer::empty(info.spec),
            layout: layout.map(Arc::new),
            profile: ProfileBuilder::new(info.spec.rate),
            decoding: Decoding::new(decoder),
            sounding: info.spec,
            at_a_change: false,
            info: Arc::new(info),
            replay_gain,
            decoded_at: 0,
            profiled_from: Frames::ZERO,
            sampled: None,
            published: None,
            ended: false,
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
        self.sounding
    }

    fn clock(&self) -> SampleRate {
        self.info.spec.rate
    }

    fn decoded(&self) -> Decoded {
        Decoded {
            spec: self.sounding,
            ..Decoded::of(&self.info, self.hints.lowpass)
        }
    }

    fn undecoded(&self) -> usize {
        self.decoded.frames().saturating_sub(self.decoded_at)
    }

    fn undecoded_on_the_clock(&self) -> Frames {
        Frames(self.undecoded() as u64).rescaled(self.sounding.rate, self.clock())
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
        self.ended = false;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Retuning {
    Level,
    Equaliser,
}

fn carried_samples(chain: &Chain, stream: StreamSpec) -> usize {
    chain.max_output_frames() * stream.channel_count().get() as usize
}

pub struct Engine {
    config: EngineConfig,
    sources: Arc<Sources>,
    backend: Box<dyn Backend>,
    surveyor: Arc<dyn Surveyor>,
    surveying: Option<Surveying>,
    opener: Arc<dyn Opener>,
    streaming: Option<Streaming>,
    outputs_made: u64,
    commands: Receiver<Request>,
    events: Sender<Event>,
    published: Published,
    changes: Option<Receiver<SinkChange>>,
    queue: Queue,
    opening: Option<Opening>,
    ahead: Option<Ahead>,
    boundaries: VecDeque<Boundary>,
    failing_past_the_boundary: Option<Error>,
    announced: Option<u64>,
    stale_sinks: bool,
    formats_owed: bool,
    transport: TransportState,
    track: Option<Track>,
    output: Option<Output>,
    retiring: Option<Retiring>,
    kept: Option<Kept>,
    unbound: Option<Frames>,
    rebind_owed: bool,
    downstream_restart_due: Option<Instant>,
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
    turned: Turns,
    muted: bool,
    answers: Vec<Answer>,
    openings_begun: u64,
    deferred: Vec<Deferred>,
    lockstep: Option<Following>,
    nudging: Option<Following>,
    faded_for_a_nudge: bool,
    stepping: Stepping,
    auditioning: Option<Auditioning>,
    kin: Option<KinAsking>,
}

struct Auditioning {
    audition: Audition,
    before: Contested,
}

struct Following {
    with: HeardClock,
    since: Instant,
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
    ahead: Option<Receiver<resonate_codec::Result<Unwrapped>>>,
    decoded: Option<Receiver<Returned>>,
    streamed: Option<Receiver<Streamed>>,
    kin: Option<Receiver<KinLanded>>,
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
        if let Some(ahead) = self.ahead.as_ref() {
            select.recv(ahead);
        }
        if let Some(decoded) = self.decoded.as_ref() {
            select.recv(decoded);
        }
        if let Some(streamed) = self.streamed.as_ref() {
            select.recv(streamed);
        }
        if let Some(kin) = self.kin.as_ref() {
            select.recv(kin);
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
                self.ahead.as_ref(),
                engine.ahead.as_ref().and_then(Ahead::awaited),
            )
            && is_one_channel(
                self.decoded.as_ref(),
                engine
                    .track
                    .as_ref()
                    .and_then(|track| track.decoding.awaited()),
            )
            && is_one_channel(
                self.streamed.as_ref(),
                engine.streaming.as_ref().map(Streaming::answers),
            )
            && is_one_channel(
                self.kin.as_ref(),
                engine.kin.as_ref().map(|asking| &asking.landed),
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
        let found = surveyor.enumerate_sinks(SINK_TIMEOUT);
        let stale_sinks = found.is_err();
        *published.sinks.write() = found.unwrap_or_default().into();
        published.first_survey.answered();
        let surveying = Surveying::start(Arc::clone(&surveyor), SINK_TIMEOUT);
        let opener = backend.opener();
        let streaming = Streaming::start(Arc::clone(&opener));

        Self {
            config,
            sources,
            backend,
            surveyor,
            surveying,
            opener,
            streaming,
            outputs_made: 0,
            commands,
            events,
            published,
            changes,
            queue: Queue::new(),
            opening: None,
            ahead: None,
            boundaries: VecDeque::new(),
            failing_past_the_boundary: None,
            announced: None,
            stale_sinks,
            formats_owed: false,
            transport: TransportState::Idle,
            track: None,
            output: None,
            retiring: None,
            kept: None,
            unbound: None,
            rebind_owed: false,
            downstream_restart_due: None,
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
            turned: Turns::new(),
            muted: false,
            answers: Vec::new(),
            openings_begun: 0,
            deferred: Vec::new(),
            lockstep: None,
            nudging: None,
            faded_for_a_nudge: false,
            stepping: Stepping::Alone,
            auditioning: None,
            kin: None,
        }
        .with_the_queue_shuffled_as_set()
    }

    fn with_the_queue_shuffled_as_set(mut self) -> Self {
        self.queue.set_smart_shuffle(self.config.smart_shuffle);
        self
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

                self.land_the_kin();
                self.ask_for_the_kin();

                self.rediscover();
                self.land_the_opening();
                self.watch_discard();
                self.take_what_was_decoded();
                self.reach_the_boundary();
                self.fail_what_waited_for_the_boundary();
                self.pump();
                self.open_ahead();
                self.finish_reshaping();
                self.splice_what_is_owed();
                self.switch_what_is_owed();
                self.restart_what_a_renderer_holds();
                self.finish_fading();
                self.promote();
                self.begin_what_was_nudged();
                self.keep_in_step();
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
        self.close_what_was_kept();
        if let Some(streaming) = self.streaming.as_mut() {
            streaming.stop();
        }
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
                .filter(|surveying| surveying.is_running())
                .map(|surveying| surveying.answers().clone()),
            events: self.listening().map(|stream| stream.events().clone()),
            opened: self.opening.as_ref().map(|opening| opening.landed.clone()),
            ahead: self.ahead.as_ref().and_then(Ahead::awaited).cloned(),
            decoded: self
                .track
                .as_ref()
                .and_then(|track| track.decoding.awaited())
                .cloned(),
            streamed: self
                .streaming
                .as_ref()
                .map(|streaming| streaming.answers().clone()),
            kin: self.kin.as_ref().map(|asking| asking.landed.clone()),
        }
    }

    fn ask_for_the_kin(&mut self) {
        if self.kin.is_some() {
            return;
        }
        let Some(kinship) = self.config.kinship.kinship() else {
            return;
        };
        if let Some(ask) = self.queue.kin_wanted() {
            self.kin = ask.sent_to(kinship);
        }
    }

    fn land_the_kin(&mut self) {
        let Some(asking) = self.kin.as_ref() else {
            return;
        };
        let landed = match asking.landed.try_recv() {
            Ok(landed) => landed,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                self.kin = None;
                return;
            }
        };
        let respread_at = asking.respread_at;
        self.kin = None;

        if !self.queue.know_kin(landed.epoch, landed.kin) {
            self.queue.owe_the_kin_again();
            return;
        }
        if let Some(revision) = respread_at {
            self.queue.respread_unless_moved_since(revision);
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
        let budget = if self.playing
            && self
                .output
                .as_ref()
                .is_some_and(|output| output.splice_owed || output.switch_waits_for_room)
        {
            budget.min(SHORTEST_TICK)
        } else {
            budget
        };
        let budget = match self.downstream_restart_in() {
            Some(left) => budget.min(left.max(SHORTEST_TICK)),
            None => budget,
        };
        let budget = match self.sleep.and_then(Sleeping::left) {
            Some(left) if left > SLEEP_FADES_OVER => budget.min(left - SLEEP_FADES_OVER),
            Some(left) => budget.min(left),
            None => budget,
        };
        let budget = match self.until_the_boundary().filter(|_| self.playing) {
            Some(left) => budget.min(left.max(SHORTEST_TICK)),
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
}

const DIGEST_GROWTH_DIVISOR: usize = 16;

const DIGEST_STEP_AT_MOST: usize = 30;

fn digest_is_due(published: Option<usize>, points: usize) -> bool {
    match published {
        None => true,
        Some(published) => {
            let step = (published / DIGEST_GROWTH_DIVISOR).clamp(1, DIGEST_STEP_AT_MOST);
            points >= published + step
        }
    }
}

fn levelled(config: &EngineConfig, info: &MediaInfo, hints: TrackHints) -> AppliedGain {
    let resolved = resolve_replay_gain(
        config.replay_gain,
        config.levelling,
        &tagged_or_measured(info.tags.heard_gain(), hints.measured),
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
    fn a_digest_is_published_at_once_when_none_was_and_after_growth_a_share_of_what_it_held() {
        assert!(digest_is_due(None, 0));
        assert!(digest_is_due(Some(0), 1));
        assert!(!digest_is_due(Some(5), 5));
        assert!(digest_is_due(Some(5), 6));

        assert!(!digest_is_due(Some(160), 160));
        assert!(!digest_is_due(Some(160), 169));
        assert!(digest_is_due(Some(160), 170));
        assert!(!digest_is_due(Some(3_200), 3_229));
        assert!(digest_is_due(Some(3_200), 3_230));

        let republished = (1..36_000_usize).fold((0, 0), |(published, count), points| {
            if digest_is_due(Some(published), points) {
                (points, count + 1)
            } else {
                (published, count)
            }
        });
        assert!(republished.1 < 1_500, "{} publications", republished.1);
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
            ahead: None,
            decoded: None,
            streamed: None,
            kin: None,
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
            ahead: None,
            decoded: None,
            streamed: None,
            kin: None,
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
