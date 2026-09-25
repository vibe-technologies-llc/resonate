use std::{
    collections::BTreeMap,
    env, fs,
    io::Cursor,
    ops::Range,
    path::{Path, PathBuf},
    process,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Sender, unbounded};
use parking_lot::Mutex;
use resonate_codec::{Error as CodecError, Result as CodecResult};
use resonate_core::{
    ChannelLayout, Decibels, FrameSpan, Frames, Gain, MeasuredGain, MediaLocation, SampleFormat,
    SampleRate, Span, StreamSpec, TrackHints, TrackId, Volume,
};
use resonate_engine::{
    AudioSource, Backend, Band, BandGain, BandKind, Caught, Command, DitherKind, EngineConfig,
    Equalisation, Error as EngineError, Event, Frequency, Hinting, Media, MediaProvider, NodeName,
    OutputMode, Placement, PlaybackState, Player, Preamp, Profile, Q, QueueItem, Reading,
    RepeatMode, ReplayGainMode, Result, Resumable, Resumption, SinkChange, SinkFormats, SinkId,
    SinkInfo, SinkResult, SinkStream, SkipUnderRepeat, SourceId, Sources, StreamCommand,
    StreamEvent, StreamRequest, Surveyor, Tapped, Until, stamp_of,
};

const RATE: u32 = 44_100;
const CHANNELS: u16 = 2;
const FRAMES: usize = 20_000;
const BLOCK_FRAMES: usize = 1_024;
const SHORT_PULL_FRAMES: usize = 64;
const PATIENCE: Duration = Duration::from_secs(20);
const SECOND_ROW: Frames = Frames(RATE as u64 * 15 / 75);
const A_SHORT_DOZE: Duration = Duration::from_millis(150);
const A_LONG_DOZE: Duration = Duration::from_secs(30);
const RING_DEPTH: Duration = Duration::from_millis(200);
const A_RAMP_AT_MOST: Duration = Duration::from_millis(50);
const LOUD_ENOUGH_TO_READ_A_LEVEL: f64 = 8_192.0;
const A_LEVEL_STEP: f64 = 0.05;
const HALF_SCALE: i16 = 16_384;

struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = env::temp_dir().join(format!(
            "resonate-transport-{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("a writable temporary directory");
        Self { root }
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.root.join(name);
        fs::write(&path, bytes).expect("a writable temporary file");
        path
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct Pcm {
    file: Vec<u8>,
    stream: Vec<u8>,
}

fn pcm(bits: u16, frames: usize) -> Pcm {
    pcm_of(bits, CHANNELS, frames)
}

fn pcm_of(bits: u16, channels: u16, frames: usize) -> Pcm {
    let stride = usize::from(bits / 8);
    let span = 1_i64 << (bits - 1);
    let samples = frames * usize::from(channels);
    let mut data = Vec::with_capacity(samples * stride);
    let mut stream = Vec::with_capacity(samples * stride);
    for n in 0..samples {
        let sample = ((n as i64 % (2 * span - 1)) - span + 1) as i32;
        let encoded = sample.to_le_bytes();
        data.extend_from_slice(&encoded[..stride]);
        stream.extend_from_slice(&encoded[..carried(bits).bytes_per_sample().get() as usize]);
    }

    Pcm {
        file: wave(bits, channels, &data),
        stream,
    }
}

fn steady(level: i16, frames: usize) -> Vec<u8> {
    let data: Vec<u8> = (0..frames * usize::from(CHANNELS))
        .flat_map(|_| level.to_le_bytes())
        .collect();
    wave(16, CHANNELS, &data)
}

fn wave(bits: u16, channels: u16, data: &[u8]) -> Vec<u8> {
    let block_align = channels * bits / 8;
    let mut fmt = Vec::new();
    fmt.extend_from_slice(&1_u16.to_le_bytes());
    fmt.extend_from_slice(&channels.to_le_bytes());
    fmt.extend_from_slice(&RATE.to_le_bytes());
    fmt.extend_from_slice(&(RATE * u32::from(block_align)).to_le_bytes());
    fmt.extend_from_slice(&block_align.to_le_bytes());
    fmt.extend_from_slice(&bits.to_le_bytes());

    let mut body = b"WAVE".to_vec();
    chunk(&mut body, b"fmt ", &fmt);
    chunk(&mut body, b"data", data);

    let mut file = b"RIFF".to_vec();
    file.extend_from_slice(&(body.len() as u32).to_le_bytes());
    file.extend_from_slice(&body);
    file
}

const fn carried(bits: u16) -> SampleFormat {
    match bits {
        16 => SampleFormat::S16,
        _ => SampleFormat::S24,
    }
}

fn chunk(into: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
    into.extend_from_slice(id);
    into.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    into.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        into.push(0);
    }
}

#[derive(Default)]
struct Graph {
    source: Option<Box<dyn AudioSource>>,
    events: Option<Sender<StreamEvent>>,
    requests: Vec<StreamRequest>,
    opens: usize,
    closes: usize,
    active: bool,
    played: Vec<u8>,
    sinks: Vec<SinkInfo>,
    enumerations: usize,
    announce: Option<Sender<SinkChange>>,
}

impl Graph {
    fn pull(&mut self, bytes: usize) -> usize {
        if !self.active {
            return 0;
        }
        let Some(source) = self.source.as_mut() else {
            return 0;
        };
        let mut block = vec![0_u8; bytes];
        let filled = source.fill(&mut block);
        block.truncate(filled);
        self.played.extend_from_slice(&block);
        filled
    }
}

#[derive(Clone)]
struct FakeSink {
    graph: Arc<Mutex<Graph>>,
    changes: Receiver<SinkChange>,
    latency: u64,
    answers_drain: bool,
    lets_go: bool,
}

impl FakeSink {
    fn new(sinks: Vec<SinkInfo>) -> (Self, Arc<Mutex<Graph>>) {
        Self::with_tail(sinks, 0, false)
    }

    fn with_tail(
        sinks: Vec<SinkInfo>,
        latency: u64,
        answers_drain: bool,
    ) -> (Self, Arc<Mutex<Graph>>) {
        let (announce, changes) = unbounded();
        let graph = Arc::new(Mutex::new(Graph {
            sinks,
            announce: Some(announce),
            ..Graph::default()
        }));
        (
            Self {
                graph: Arc::clone(&graph),
                changes,
                latency,
                answers_drain,
                lets_go: false,
            },
            graph,
        )
    }

    fn letting_go_of_every_ring(self) -> Self {
        Self {
            lets_go: true,
            ..self
        }
    }
}

struct Surveyed(Arc<Mutex<Graph>>);

impl Surveyor for Surveyed {
    fn enumerate_sinks(&self, _timeout: Duration) -> SinkResult<Vec<SinkInfo>> {
        let mut graph = self.0.lock();
        graph.enumerations += 1;
        Ok(graph.sinks.clone())
    }
}

impl Backend for FakeSink {
    fn subscribe_sinks(&self) -> Receiver<SinkChange> {
        self.changes.clone()
    }

    fn surveyor(&self) -> Arc<dyn Surveyor> {
        Arc::new(Surveyed(Arc::clone(&self.graph)))
    }

    fn open(
        &self,
        request: &StreamRequest,
        source: Box<dyn AudioSource>,
    ) -> SinkResult<SinkStream> {
        let (announce, incoming) = unbounded();

        {
            let mut graph = self.graph.lock();
            graph.opens += 1;
            graph.requests.push(request.clone());
            graph.source = (!self.lets_go).then_some(source);
            graph.events = Some(announce);
            graph.active = false;
        }

        let control = Arc::clone(&self.graph);
        let answers_drain = self.answers_drain;
        Ok(SinkStream::new(
            incoming,
            Arc::new(AtomicU64::new(self.latency)),
            Box::new(move |command| {
                let mut graph = control.lock();
                match command {
                    StreamCommand::SetActive(active) => graph.active = active,
                    StreamCommand::Drain => {
                        if answers_drain && let Some(events) = graph.events.as_ref() {
                            let _ = events.try_send(StreamEvent::Drained);
                        }
                    }
                    StreamCommand::Close => {
                        graph.closes += 1;
                        graph.active = false;
                        graph.source = None;
                        graph.events = None;
                    }
                }
                Ok(())
            }),
        ))
    }

    fn shutdown(self: Box<Self>) -> SinkResult<()> {
        Ok(())
    }
}

fn sink(rates: &[SampleRate], formats: &[SampleFormat]) -> SinkInfo {
    SinkInfo {
        id: SinkId::new(1),
        name: NodeName::new("alsa_output.fake"),
        description: "Fake DAC".to_owned(),
        is_default: true,
        is_hardware: true,
        port: None,
        profile: None,
        formats: formats
            .iter()
            .map(|format| SinkFormats {
                format: *format,
                rates: rates.to_vec(),
                channels: vec![ChannelLayout::Stereo],
            })
            .collect(),
        allowed_rates: rates.to_vec(),
        current_rate: rates.first().copied(),
    }
}

fn arriving() -> SinkInfo {
    SinkInfo {
        id: SinkId::new(2),
        name: NodeName::new("alsa_output.usb"),
        description: "USB DAC".to_owned(),
        is_default: false,
        ..sink(&[SampleRate::HZ_48000], &[SampleFormat::S32])
    }
}

const DSF_BLOCK: usize = 4_096;
const DSD64_HZ: u32 = 2_822_400;

fn dsf_tone() -> Vec<u8> {
    let lanes = usize::from(CHANNELS);
    let blocks = 8;
    let per_channel = blocks * DSF_BLOCK;

    let mut planes = vec![Vec::with_capacity(per_channel); lanes];
    for (lane, plane) in planes.iter_mut().enumerate() {
        let mut error = 0.0_f64;
        let mut byte = 0_u8;
        let hertz = 1_000.0 * (lane as f64 + 1.0);

        for n in 0..per_channel * 8 {
            let wanted =
                0.5 * (std::f64::consts::TAU * hertz * n as f64 / f64::from(DSD64_HZ)).sin();
            error += wanted;
            let high = error > 0.0;
            error -= if high { 1.0 } else { -1.0 };

            let within = n % 8;
            byte |= u8::from(high) << within;
            if within == 7 {
                plane.push(byte);
                byte = 0;
            }
        }
    }

    let mut data = Vec::with_capacity(per_channel * lanes);
    for block in 0..blocks {
        for plane in &planes {
            data.extend_from_slice(&plane[block * DSF_BLOCK..(block + 1) * DSF_BLOCK]);
        }
    }

    let mut fmt = Vec::new();
    fmt.extend_from_slice(b"fmt ");
    fmt.extend_from_slice(&52_u64.to_le_bytes());
    fmt.extend_from_slice(&1_u32.to_le_bytes());
    fmt.extend_from_slice(&0_u32.to_le_bytes());
    fmt.extend_from_slice(&u32::from(CHANNELS).to_le_bytes());
    fmt.extend_from_slice(&u32::from(CHANNELS).to_le_bytes());
    fmt.extend_from_slice(&DSD64_HZ.to_le_bytes());
    fmt.extend_from_slice(&1_u32.to_le_bytes());
    fmt.extend_from_slice(&((per_channel * 8) as u64).to_le_bytes());
    fmt.extend_from_slice(&(DSF_BLOCK as u32).to_le_bytes());
    fmt.extend_from_slice(&0_u32.to_le_bytes());

    let mut chunk = Vec::new();
    chunk.extend_from_slice(b"data");
    chunk.extend_from_slice(&((data.len() + 12) as u64).to_le_bytes());
    chunk.extend_from_slice(&data);

    let total = (28 + fmt.len() + chunk.len()) as u64;
    let mut file = Vec::new();
    file.extend_from_slice(b"DSD ");
    file.extend_from_slice(&28_u64.to_le_bytes());
    file.extend_from_slice(&total.to_le_bytes());
    file.extend_from_slice(&0_u64.to_le_bytes());
    file.extend_from_slice(&fmt);
    file.extend_from_slice(&chunk);
    file
}

fn config() -> EngineConfig {
    EngineConfig {
        buffer: Duration::from_millis(100),
        ..EngineConfig::default()
    }
}

fn player(sinks: Vec<SinkInfo>) -> Result<(Player, Arc<Mutex<Graph>>)> {
    let (backend, graph) = FakeSink::new(sinks);
    let player = Player::with_backend(config(), move |_| Ok(Box::new(backend)))?;
    Ok((player, graph))
}

struct InMemory {
    source: SourceId,
    key: String,
    bytes: Vec<u8>,
}

impl MediaProvider for InMemory {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn open(&self, location: &MediaLocation) -> CodecResult<Media> {
        if location.locator().as_key() != Some(self.key.as_str()) {
            return Err(CodecError::LocatorNotUsable {
                location: location.clone(),
            });
        }

        Ok(Media {
            stream: Box::new(Reading::new(Cursor::new(self.bytes.clone()))),
            hint: None,
        })
    }
}

struct ServesOnce {
    source: SourceId,
    key: String,
    bytes: Vec<u8>,
    served: AtomicU64,
}

impl MediaProvider for ServesOnce {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn open(&self, location: &MediaLocation) -> CodecResult<Media> {
        if location.locator().as_key() != Some(self.key.as_str())
            || self.served.fetch_add(1, Ordering::Relaxed) > 0
        {
            return Err(CodecError::LocatorNotUsable {
                location: location.clone(),
            });
        }

        Ok(Media {
            stream: Box::new(Reading::new(Cursor::new(self.bytes.clone()))),
            hint: None,
        })
    }
}

fn player_over(sinks: Vec<SinkInfo>, sources: Arc<Sources>) -> Result<(Player, Arc<Mutex<Graph>>)> {
    settled_over(sinks, sources, config())
}

fn settled_over(
    sinks: Vec<SinkInfo>,
    sources: Arc<Sources>,
    config: EngineConfig,
) -> Result<(Player, Arc<Mutex<Graph>>)> {
    let (backend, graph) = FakeSink::new(sinks);
    let player = Player::with_sources_and_backend(config, sources, move |_| Ok(Box::new(backend)))?;
    Ok((player, graph))
}

fn player_with_tail(
    sinks: Vec<SinkInfo>,
    latency: u64,
    answers_drain: bool,
) -> Result<(Player, Arc<Mutex<Graph>>)> {
    let (backend, graph) = FakeSink::with_tail(sinks, latency, answers_drain);
    let player = Player::with_backend(config(), move |_| Ok(Box::new(backend)))?;
    Ok((player, graph))
}

fn track(path: &Path, id: u64) -> QueueItem {
    QueueItem {
        id: TrackId::new(id).expect("a non-zero track id"),
        location: MediaLocation::local(path),
        span: None,
    }
}

fn kept(path: &Path) -> Resumable {
    Resumable {
        location: MediaLocation::local(path),
        span: None,
    }
}

fn playing_row(player: &Player) -> Option<MediaLocation> {
    let playing = player.state().current?.id;
    player
        .queue()
        .iter()
        .find(|item| item.id == playing)
        .map(|item| item.location.clone())
}

fn drawn(player: &Player) -> Vec<MediaLocation> {
    player
        .queue()
        .iter()
        .map(|item| item.location.clone())
        .collect()
}

fn frame_bytes(format: SampleFormat) -> usize {
    usize::from(CHANNELS) * usize::from(format.bytes_per_sample().get())
}

fn wait_for(player: &Player, mut ready: impl FnMut(&Player) -> bool, what: &str) {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        if ready(player) {
            return;
        }
        thread::sleep(Duration::from_millis(1));
    }
    panic!("timed out waiting for {what}; {}", transport(player));
}

fn transport(player: &Player) -> String {
    let state = player.state();
    format!(
        "playback {:?}, track {:?} at row {:?} of {}, output {:?} after {} underruns",
        state.playback,
        state.current.map(|track| track.id.get()),
        state.queue_position,
        state.queue_len,
        state.output.map(|output| output.sink),
        state.output.map_or(0, |output| output.underruns),
    )
}

fn pulls(graph: &Graph) -> String {
    format!(
        "graph opened {} streams and closed {}, {} now, {} bytes pulled",
        graph.opens,
        graph.closes,
        if graph.active { "pulling" } else { "idle" },
        graph.played.len(),
    )
}

fn play_until(
    player: &Player,
    graph: &Arc<Mutex<Graph>>,
    block: usize,
    mut done: impl FnMut(&Player, &Graph) -> bool,
    what: &str,
) {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        let settled = {
            let mut held = graph.lock();
            held.pull(block);
            done(player, &held)
        };
        if settled {
            return;
        }
        thread::sleep(Duration::from_millis(1));
    }
    panic!(
        "timed out waiting for {what}; {}, {}",
        transport(player),
        pulls(&graph.lock())
    );
}

fn announce(graph: &Arc<Mutex<Graph>>, device: SinkInfo) {
    let mut held = graph.lock();
    let node = device.id;
    held.sinks.push(device);
    let announced = held
        .announce
        .as_ref()
        .map(|announce| announce.send(SinkChange::Added(node)));
    assert!(
        announced.is_some_and(|sent| sent.is_ok()),
        "the fake daemon could not announce its new device"
    );
}

fn playing(player: &Player) -> bool {
    player.state().playback == PlaybackState::Playing
}

fn paused(player: &Player) -> bool {
    player.state().playback == PlaybackState::Paused
}

fn plays(player: &Player, id: u64) -> bool {
    player
        .state()
        .current
        .is_some_and(|track| track.id.get() == id)
}

fn stand_still(player: &Player) {
    player
        .request(Command::Pause)
        .and_then(|outcome| outcome.wait_for(PATIENCE))
        .expect("the transport to take a pause");
    wait_for(player, paused, "the transport to stand still");
}

#[test]
fn a_track_whose_sink_takes_its_format_reaches_the_graph_byte_for_byte() -> Result<()> {
    for bits in [16_u16, 24] {
        let format = carried(bits);
        let tree = Tree::new();
        let source = pcm(bits, FRAMES);
        let path = tree.write("track.wav", &source.file);

        let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[format])])?;
        player.send(Command::Load {
            items: vec![track(&path, 1)],
            start_at: 0,
            autoplay: true,
        })?;

        let wanted = source.stream.len();
        play_until(
            &player,
            &graph,
            BLOCK_FRAMES * frame_bytes(format),
            |_, graph| graph.played.len() >= wanted,
            "the whole track to reach the graph",
        );

        let graph = graph.lock();
        assert_eq!(
            graph.played, source.stream,
            "{bits}-bit audio did not survive the stream"
        );
        assert_eq!(graph.opens, 1, "{bits}-bit audio reopened the stream");
        assert_eq!(
            graph.requests.first().map(|request| request.spec),
            Some(StreamSpec::new(
                SampleRate::HZ_44100,
                ChannelLayout::Stereo,
                format
            ))
        );
        assert_eq!(
            graph.requests.first().map(|request| request.no_convert),
            Some(true)
        );
    }
    Ok(())
}

fn surround(rates: &[SampleRate], formats: &[SampleFormat]) -> SinkInfo {
    SinkInfo {
        formats: formats
            .iter()
            .map(|format| SinkFormats {
                format: *format,
                rates: rates.to_vec(),
                channels: vec![ChannelLayout::Stereo, ChannelLayout::Surround51],
            })
            .collect(),
        ..sink(rates, formats)
    }
}

#[test]
fn a_stereo_sink_is_handed_a_stereo_stream_of_a_surround_track() -> Result<()> {
    const SURROUND: u16 = 6;
    let tree = Tree::new();
    let source = pcm_of(16, SURROUND, FRAMES);
    let path = tree.write("surround.wav", &source.file);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;

    wait_for(&player, playing, "the stream to open");

    let status = player.state().output.expect("an output status");
    assert_eq!(status.negotiated.channels, ChannelLayout::Stereo);
    assert_eq!(status.mode, OutputMode::Converted);

    let wanted = FRAMES * frame_bytes(SampleFormat::S16);
    play_until(
        &player,
        &graph,
        BLOCK_FRAMES * frame_bytes(SampleFormat::S16),
        |_, graph| graph.played.len() >= wanted,
        "the downmixed track to reach the graph",
    );

    let graph = graph.lock();
    assert_eq!(
        graph.requests.first().map(|request| request.spec),
        Some(status.negotiated),
        "the graph was asked for a layout the sink never advertised"
    );
    assert_eq!(graph.opens, 1, "the downmix reopened the stream");
    Ok(())
}

#[test]
fn a_surround_sink_is_handed_every_channel_the_file_carries() -> Result<()> {
    const SURROUND: u16 = 6;
    let tree = Tree::new();
    let source = pcm_of(16, SURROUND, FRAMES);
    let path = tree.write("surround.wav", &source.file);

    let (player, graph) = player(vec![surround(
        &[SampleRate::HZ_44100],
        &[SampleFormat::S16],
    )])?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;

    let wanted = source.stream.len();
    play_until(
        &player,
        &graph,
        BLOCK_FRAMES * usize::from(SURROUND) * 2,
        |_, graph| graph.played.len() >= wanted,
        "the whole surround track to reach the graph",
    );

    let status = player.state().output.expect("an output status");
    assert_eq!(status.negotiated.channels, ChannelLayout::Surround51);
    assert_eq!(status.mode, OutputMode::BitPerfect);

    let graph = graph.lock();
    assert_eq!(
        graph.played, source.stream,
        "a sink that takes 5.1 was handed something else"
    );
    Ok(())
}

#[test]
fn a_sink_that_cannot_take_the_source_rate_is_asked_for_a_converted_stream() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let path = tree.write("track.wav", &source.file);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_48000], &[SampleFormat::S32])])?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;

    wait_for(&player, playing, "the stream to open");

    let status = player.state().output.expect("an output status");
    assert_eq!(status.negotiated.rate, SampleRate::HZ_48000);
    assert_eq!(status.negotiated.format, SampleFormat::S32);
    assert_eq!(status.mode, OutputMode::Converted);
    assert_eq!(
        graph.lock().requests.first().map(|request| request.spec),
        Some(status.negotiated)
    );
    Ok(())
}

#[test]
fn the_published_stamp_holds_while_the_queue_holds_the_rows_it_was_loaded_with() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let first = tree.write("first.wav", &source.file);
    let second = tree.write("second.wav", &source.file);

    let (player, _graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    let items = vec![track(&first, 1), track(&second, 2)];
    let as_loaded = stamp_of(&items);
    player.send(Command::Load {
        items,
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the stream to open");
    wait_for(
        &player,
        |player| player.state().queue_stamp == as_loaded,
        "the loaded queue to publish the stamp it was loaded with",
    );

    player
        .request(Command::Move {
            rows: Span::one(0),
            to: 1,
        })?
        .wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| {
            player
                .queue()
                .first()
                .is_some_and(|item| item.id.get() == 2)
        },
        "the moved row to publish",
    );
    assert_eq!(
        player.state().queue_stamp,
        as_loaded,
        "a move took rows out of the queue"
    );

    player
        .request(Command::Insert {
            items: vec![track(&first, 3)],
            at: Placement::Last,
            play: false,
        })?
        .wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| player.state().queue_len == 3,
        "the arriving row to publish",
    );
    assert_ne!(
        player.state().queue_stamp,
        as_loaded,
        "a row arriving over the bus left the queue stamped as it was loaded"
    );

    player
        .request(Command::Remove(Span::one(2)))?
        .wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| player.state().queue_len == 2,
        "the dropped row to publish",
    );
    assert_eq!(
        player.state().queue_stamp,
        as_loaded,
        "a queue holding the rows it was loaded with stamped otherwise"
    );
    Ok(())
}

#[test]
fn a_command_answered_is_one_the_published_state_already_says() -> Result<()> {
    let (player, _graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    let half = Volume::new(0.5).expect("half is a volume");

    player
        .request(Command::SetShuffle(true))?
        .wait_for(PATIENCE)?;
    assert!(
        player.state().shuffle,
        "a shuffle was answered before it was published"
    );

    player
        .request(Command::SetRepeat(RepeatMode::Track))?
        .wait_for(PATIENCE)?;
    assert_eq!(
        player.state().repeat,
        RepeatMode::Track,
        "a repeat was answered before it was published"
    );

    player
        .request(Command::SetVolume(half))?
        .wait_for(PATIENCE)?;
    assert_eq!(
        player.state().volume,
        half,
        "a volume was answered before it was published"
    );
    Ok(())
}

#[test]
fn pausing_stops_the_graph_and_playing_starts_it_again() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let path = tree.write("track.wav", &source.file);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the stream to open");
    assert!(graph.lock().active);

    player.request(Command::Pause)?.wait_for(PATIENCE)?;
    assert!(!graph.lock().active);
    wait_for(
        &player,
        |player| player.state().playback == PlaybackState::Paused,
        "the transport to report itself paused",
    );

    player.request(Command::Play)?.wait_for(PATIENCE)?;
    assert!(graph.lock().active);
    wait_for(&player, playing, "the transport to report itself playing");
    assert_eq!(graph.lock().opens, 1, "a pause reopened the stream");
    Ok(())
}

#[test]
fn what_a_starved_graph_went_without_is_published_in_frames() -> Result<()> {
    const A_SECOND: usize = RATE as usize;

    let tree = Tree::new();
    let path = tree.write("long.wav", &steady(1_000, 10 * A_SECOND));

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the stream to open");
    assert_eq!(
        player.state().output.map(|output| output.went_without),
        Some(Frames::ZERO)
    );

    let stride = frame_bytes(SampleFormat::S16);
    let taken = graph.lock().pull(A_SECOND * stride) / stride;
    assert!(taken < A_SECOND, "a ring as deep as a second was asked for");

    wait_for(
        &player,
        |player| {
            player
                .state()
                .output
                .is_some_and(|output| output.underruns > 0)
        },
        "the starve to be counted",
    );
    let output = player.state().output.expect("an open output");
    assert_eq!(
        output.went_without,
        Frames((A_SECOND - taken) as u64),
        "what the graph went without is not what it asked for less what it was handed"
    );
    Ok(())
}

#[test]
fn a_seek_discards_what_the_ring_held_without_reopening_the_stream() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let path = tree.write("track.wav", &source.file);
    let stride = frame_bytes(SampleFormat::S16);
    let target = Frames(FRAMES as u64 / 2);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the stream to open");

    play_until(
        &player,
        &graph,
        BLOCK_FRAMES * stride,
        |_, graph| graph.played.len() >= BLOCK_FRAMES * stride,
        "the first block to play",
    );

    player.request(Command::Seek(target))?.wait_for(PATIENCE)?;

    let tail = source.stream.split_at(target.get() as usize * stride).1;
    play_until(
        &player,
        &graph,
        BLOCK_FRAMES * stride,
        |_, graph| graph.played.ends_with(tail),
        "the audio after the seek to play",
    );

    let graph = graph.lock();
    assert_eq!(graph.opens, 1, "a seek reopened the stream");
    assert_eq!(graph.closes, 0, "a seek closed the stream");
    assert!(
        graph.played.len() < source.stream.len(),
        "the seek played no less audio than the whole track"
    );
    Ok(())
}

#[test]
fn only_a_seek_that_landed_moves_the_count_the_engine_publishes() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let first = tree.write("first.wav", &source.file);
    let second = tree.write("second.wav", &source.file);

    let (player, _graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: vec![track(&first, 1), track(&second, 2)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the stream to open");
    wait_for(
        &player,
        |player| {
            player
                .state()
                .current
                .is_some_and(|track| track.duration.is_some())
        },
        "the length of the track to be known",
    );

    let unseeked = player.state().seeks;
    player
        .request(Command::Seek(Frames(FRAMES as u64 / 2)))?
        .wait_for(PATIENCE)?;
    let seeked = player.state().seeks;
    assert_ne!(seeked, unseeked, "a seek that landed was not counted");

    player
        .request(Command::Seek(Frames(FRAMES as u64 * 4)))?
        .wait_for(PATIENCE)
        .expect_err("a seek past the end of the track was taken");
    assert_eq!(
        player.state().seeks,
        seeked,
        "a seek the engine refused was counted"
    );

    player.request(Command::Next)?.wait_for(PATIENCE)?;
    wait_for(&player, |player| plays(player, 2), "the next track to open");
    assert_eq!(
        player.state().seeks,
        seeked,
        "a track change was counted as a seek within the track"
    );
    Ok(())
}

#[test]
fn a_seek_while_paused_leaves_the_ring_ready_rather_than_waiting_on_the_graph() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let path = tree.write("track.wav", &source.file);
    let stride = frame_bytes(SampleFormat::S16);
    let target = Frames(FRAMES as u64 / 2);
    let block = BLOCK_FRAMES * stride;

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the stream to open");
    play_until(
        &player,
        &graph,
        block,
        |_, graph| graph.played.len() >= block,
        "the first block to play",
    );

    player.request(Command::Pause)?.wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| player.state().playback == PlaybackState::Paused,
        "the transport to report itself paused",
    );

    player.request(Command::Seek(target))?.wait_for(PATIENCE)?;
    wait_for(
        &player,
        |_| graph.lock().opens == 2,
        "the paused seek to fill a ring the graph is not draining",
    );
    assert_eq!(
        player.state().playback,
        PlaybackState::Paused,
        "a seek started playback that was paused"
    );

    player.request(Command::Play)?.wait_for(PATIENCE)?;
    let played = {
        let mut held = graph.lock();
        held.played.clear();
        assert_eq!(
            held.pull(block),
            block,
            "the first pull after resuming came back empty"
        );
        held.played.clone()
    };

    let tail = source.stream.split_at(target.get() as usize * stride).1;
    assert!(
        tail.starts_with(&played),
        "the audio waiting for the graph was not the audio at the seek target"
    );
    Ok(())
}

#[test]
fn a_pause_caught_between_a_seek_and_the_graph_rebuilds_the_ring_rather_than_parking_it()
-> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let path = tree.write("track.wav", &source.file);
    let stride = frame_bytes(SampleFormat::S16);
    let target = Frames(FRAMES as u64 / 2);
    let block = BLOCK_FRAMES * stride;

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the stream to open");
    play_until(
        &player,
        &graph,
        block,
        |_, graph| graph.played.len() >= block,
        "the first block to play",
    );

    player.request(Command::Seek(target))?.wait_for(PATIENCE)?;
    player.request(Command::Pause)?.wait_for(PATIENCE)?;
    wait_for(&player, paused, "the transport to report itself paused");
    wait_for(
        &player,
        |_| graph.lock().opens == 2,
        "the discard no one is pulling to be rebuilt rather than parked",
    );

    player.request(Command::Play)?.wait_for(PATIENCE)?;
    let played = {
        let mut held = graph.lock();
        held.played.clear();
        assert_eq!(
            held.pull(block),
            block,
            "the first pull after resuming came back empty"
        );
        held.played.clone()
    };

    let tail = source.stream.split_at(target.get() as usize * stride).1;
    assert!(
        tail.starts_with(&played),
        "the ring a pause caught mid-seek did not hold the audio at the seek target"
    );
    Ok(())
}

#[test]
fn a_graph_that_lets_go_of_the_ring_is_waited_for_and_the_row_plays_on_from_where_it_was_heard()
-> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let path = tree.write("track.wav", &source.file);
    let block = BLOCK_FRAMES * frame_bytes(SampleFormat::S16);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the stream to open");
    play_until(
        &player,
        &graph,
        block,
        |_, graph| graph.played.len() >= block,
        "the first block to play",
    );

    graph.lock().source = None;

    wait_for(
        &player,
        |_| graph.lock().opens == 2,
        "the row to be bound to the graph again",
    );
    wait_for(&player, playing, "the row to play again");
    let heard = graph.lock().played.len();
    play_until(
        &player,
        &graph,
        block,
        |_, graph| graph.played.len() >= heard + block,
        "the row to play on",
    );

    assert_eq!(player.state().current.map(|track| track.id.get()), Some(1));
    let played = graph.lock().played.clone();
    assert!(
        source.stream.starts_with(&played),
        "the row did not play on from the frame the graph had last been handed"
    );
    assert!(
        !player
            .events()
            .try_iter()
            .any(|event| matches!(event, Event::Failed { .. })),
        "a graph that came back was reported as a failed track"
    );
    Ok(())
}

#[test]
fn a_skip_while_one_track_repeats_goes_on_repeating_the_queue_unless_told_to_keep_the_track()
-> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let rows: Vec<QueueItem> = (1..=3)
        .map(|id| track(&tree.write(&format!("{id}.wav"), &source.file), id))
        .collect();

    let (player, _graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: rows,
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the first row to play");
    assert_eq!(
        player.state().skip_under_repeat,
        SkipUnderRepeat::RepeatsTheQueue
    );

    player
        .request(Command::SetRepeat(RepeatMode::Track))?
        .wait_for(PATIENCE)?;
    player.request(Command::Next)?.wait_for(PATIENCE)?;
    let skipped = player.state();
    assert_eq!(skipped.repeat, RepeatMode::Queue);
    assert_eq!(skipped.loaded_position, Some(1));

    player
        .request(Command::SetSkipUnderRepeat(
            SkipUnderRepeat::KeepsRepeatingTheTrack,
        ))?
        .wait_for(PATIENCE)?;
    player
        .request(Command::SetRepeat(RepeatMode::Track))?
        .wait_for(PATIENCE)?;
    player.request(Command::Previous)?.wait_for(PATIENCE)?;
    let kept = player.state();
    assert_eq!(
        kept.skip_under_repeat,
        SkipUnderRepeat::KeepsRepeatingTheTrack
    );
    assert_eq!(kept.repeat, RepeatMode::Track);
    assert_eq!(kept.loaded_position, Some(0));

    player
        .request(Command::SetRepeat(RepeatMode::Off))?
        .wait_for(PATIENCE)?;
    player.request(Command::Next)?.wait_for(PATIENCE)?;
    assert_eq!(player.state().repeat, RepeatMode::Off);
    Ok(())
}

#[test]
fn a_track_change_binds_from_the_published_list_rather_than_asking_the_graph_again() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let first = tree.write("first.wav", &source.file);
    let second = tree.write("second.wav", &source.file);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: vec![track(&first, 1), track(&second, 2)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the first row to play");
    let taken = graph.lock().enumerations;

    player.request(Command::Next)?.wait_for(PATIENCE)?;
    wait_for(&player, |player| plays(player, 2), "the second row to play");
    player
        .request(Command::Seek(Frames(FRAMES as u64 / 2)))?
        .wait_for(PATIENCE)?;
    player
        .request(Command::SetBuffer(Duration::from_millis(200)))?
        .wait_for(PATIENCE)?;

    assert_eq!(
        graph.lock().enumerations,
        taken,
        "a rebind asked the graph for its devices although none had announced a change"
    );

    announce(&graph, arriving());
    player
        .request(Command::SetSink(Some(NodeName::new("alsa_output.usb"))))?
        .wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| {
            player
                .state()
                .output
                .is_some_and(|output| output.sink == SinkId::new(2))
        },
        "the device that announced itself to take over",
    );
    assert!(
        graph.lock().enumerations > taken,
        "an announced device was bound without the list being refreshed"
    );
    Ok(())
}

#[test]
fn a_device_that_appears_mid_track_reaches_the_picker_without_reopening_anything() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let path = tree.write("track.wav", &source.file);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the stream to open");
    wait_for(
        &player,
        |player| player.sinks().len() == 1,
        "the first sink list to publish",
    );

    announce(&graph, arriving());

    wait_for(
        &player,
        |player| player.sinks().len() == 2,
        "the device that appeared to reach the picker while a track was playing",
    );

    let graph = graph.lock();
    assert_eq!(graph.opens, 1, "a sink list refresh reopened the stream");
    assert!(graph.active, "a sink list refresh stopped the graph");
    Ok(())
}

#[test]
fn switching_sink_rebuilds_the_stream_around_the_new_device() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let path = tree.write("track.wav", &source.file);

    let (player, graph) = player(vec![
        sink(&[SampleRate::HZ_44100], &[SampleFormat::S16]),
        arriving(),
    ])?;

    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the stream to open");

    player
        .request(Command::SetSink(Some(NodeName::new("alsa_output.usb"))))?
        .wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| {
            player
                .state()
                .output
                .is_some_and(|output| output.sink == SinkId::new(2))
        },
        "the second sink to take over",
    );

    let graph = graph.lock();
    assert_eq!(graph.opens, 2);
    assert_eq!(graph.closes, 1);
    Ok(())
}

fn bound_by(sink: Option<NodeName>) -> EngineConfig {
    EngineConfig {
        sink,
        ..EngineConfig::default()
    }
}

fn change_the_graph(
    graph: &Arc<Mutex<Graph>>,
    change: SinkChange,
    edit: impl FnOnce(&mut Vec<SinkInfo>),
) {
    let mut held = graph.lock();
    edit(&mut held.sinks);
    let announced = held.announce.as_ref().map(|announce| announce.send(change));
    assert!(
        announced.is_some_and(|sent| sent.is_ok()),
        "the fake daemon could not announce the change"
    );
}

fn make_the_default(sinks: &mut [SinkInfo], id: SinkId) {
    for sink in sinks {
        sink.is_default = sink.id == id;
    }
}

fn bound_to(player: &Player) -> Option<SinkId> {
    player.state().output.map(|output| output.sink)
}

fn playing_one_track_over(config: EngineConfig) -> Result<(Player, Arc<Mutex<Graph>>)> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let path = tree.write("track.wav", &source.file);

    let (backend, graph) = FakeSink::new(vec![
        sink(&[SampleRate::HZ_44100], &[SampleFormat::S16]),
        arriving(),
    ]);
    let player = Player::with_backend(config, move |_| Ok(Box::new(backend)))?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the stream to open");
    assert_eq!(bound_to(&player), Some(SinkId::new(1)));
    Ok((player, graph))
}

#[test]
fn a_stream_following_the_default_moves_when_the_desktop_chooses_another() -> Result<()> {
    let (player, graph) = playing_one_track_over(bound_by(None))?;

    change_the_graph(&graph, SinkChange::DefaultChanged, |sinks| {
        make_the_default(sinks, SinkId::new(2));
    });
    wait_for(
        &player,
        |player| playing(player) && bound_to(player) == Some(SinkId::new(2)),
        "the stream to follow the desktop's new default",
    );

    let graph = graph.lock();
    assert_eq!(graph.opens, 2);
    assert_eq!(
        graph.requests.last().map(|request| request.target),
        Some(Some(SinkId::new(2))),
        "the stream was reopened somewhere other than the new default"
    );
    Ok(())
}

#[test]
fn a_stream_whose_ring_holds_under_a_tenth_of_a_second_still_follows_the_default() -> Result<()> {
    const RATE_AT: Range<usize> = 24..28;
    const BYTES_A_SECOND_AT: Range<usize> = 28..32;
    const HIGH_RATE: u32 = 192_000;

    let tree = Tree::new();
    let mut file = pcm(16, FRAMES).file;
    file[RATE_AT].copy_from_slice(&HIGH_RATE.to_le_bytes());
    file[BYTES_A_SECOND_AT].copy_from_slice(&(HIGH_RATE * 4).to_le_bytes());
    let path = tree.write("high.wav", &file);

    let (backend, graph) = FakeSink::new(vec![
        sink(&[SampleRate::HZ_192000], &[SampleFormat::S16]),
        arriving(),
    ]);
    let player = Player::with_backend(
        EngineConfig {
            buffer: Duration::from_millis(40),
            ..EngineConfig::default()
        },
        move |_| Ok(Box::new(backend)),
    )?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the stream to open");
    assert_eq!(bound_to(&player), Some(SinkId::new(1)));

    change_the_graph(&graph, SinkChange::DefaultChanged, |sinks| {
        make_the_default(sinks, SinkId::new(2));
    });
    wait_for(
        &player,
        |player| playing(player) && bound_to(player) == Some(SinkId::new(2)),
        "a stream whose ring never holds 100 ms to follow the new default all the same",
    );
    Ok(())
}

#[test]
fn a_device_chosen_by_name_stays_bound_when_the_desktops_default_moves() -> Result<()> {
    let (player, graph) =
        playing_one_track_over(bound_by(Some(NodeName::new("alsa_output.fake"))))?;
    let enumerated = graph.lock().enumerations;

    change_the_graph(&graph, SinkChange::DefaultChanged, |sinks| {
        make_the_default(sinks, SinkId::new(2));
    });
    wait_for(
        &player,
        |_| graph.lock().enumerations > enumerated,
        "the engine to read the changed graph",
    );

    assert_eq!(bound_to(&player), Some(SinkId::new(1)));
    assert_eq!(
        graph.lock().opens,
        1,
        "a chosen device was left for the default"
    );
    Ok(())
}

#[test]
fn a_stream_whose_device_goes_away_moves_to_the_one_the_desktop_falls_back_to() -> Result<()> {
    let (player, graph) =
        playing_one_track_over(bound_by(Some(NodeName::new("alsa_output.fake"))))?;

    change_the_graph(&graph, SinkChange::Removed(SinkId::new(1)), |sinks| {
        sinks.retain(|sink| sink.id != SinkId::new(1));
        make_the_default(sinks, SinkId::new(2));
    });
    wait_for(
        &player,
        |player| playing(player) && bound_to(player) == Some(SinkId::new(2)),
        "the stream to move off the device that went",
    );
    Ok(())
}

#[test]
fn a_pause_survives_the_track_changing_under_it() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let first = tree.write("first.wav", &source.file);
    let second = tree.write("second.wav", &source.file);
    let third = tree.write("third.wav", &source.file);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: vec![track(&first, 1), track(&second, 2), track(&third, 3)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the first row to play");

    player.request(Command::Pause)?.wait_for(PATIENCE)?;
    wait_for(&player, paused, "the transport to pause");

    player
        .request(Command::Remove(Span::one(0)))?
        .wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| paused(player) && plays(player, 2),
        "the row that slid into place to be loaded, still paused",
    );
    assert!(
        !graph.lock().active,
        "removing the playing row started the graph"
    );

    player.request(Command::Next)?.wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| paused(player) && plays(player, 3),
        "the next row to be loaded, still paused",
    );
    assert!(!graph.lock().active, "skipping a row started the graph");

    player.request(Command::Play)?.wait_for(PATIENCE)?;
    wait_for(
        &player,
        playing,
        "the transport to resume where it was asked to",
    );
    Ok(())
}

#[test]
fn a_run_of_paused_skips_binds_nothing_until_the_transport_is_asked_to_play() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let first = tree.write("first.wav", &source.file);
    let second = tree.write("second.wav", &source.file);
    let third = tree.write("third.wav", &source.file);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: vec![track(&first, 1), track(&second, 2), track(&third, 3)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the first row to play");
    assert_eq!(graph.lock().opens, 1);

    player.request(Command::Pause)?.wait_for(PATIENCE)?;
    wait_for(&player, paused, "the transport to pause");

    for row in [2, 3] {
        player.request(Command::Next)?.wait_for(PATIENCE)?;
        wait_for(
            &player,
            |player| paused(player) && plays(player, row),
            "the skipped row to be loaded, still paused",
        );
        assert!(
            player.state().output.is_none(),
            "a paused skip bound the row to a sink"
        );
    }

    assert_eq!(
        graph.lock().opens,
        1,
        "a paused skip opened a stream nothing would be heard through"
    );
    assert_eq!(
        graph.lock().closes,
        1,
        "the stream the pause was holding was not given back on the first skip"
    );

    player.request(Command::Play)?.wait_for(PATIENCE)?;
    wait_for(&player, playing, "the row the skips landed on to play");
    assert!(
        plays(&player, 3),
        "playing after a run of paused skips played a different row"
    );
    assert_eq!(
        graph.lock().opens,
        2,
        "playing did not bind the row the skips landed on"
    );
    Ok(())
}

#[test]
fn following_the_graph_rate_resamples_where_matching_the_file_would_not() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let path = tree.write("track.wav", &source.file);

    let (player, graph) = player(vec![sink(
        &[SampleRate::HZ_48000, SampleRate::HZ_44100],
        &[SampleFormat::S16],
    )])?;

    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(
        &player,
        playing,
        "the stream to open at the file's own rate",
    );

    let matched = player.state().output.expect("a stream is open");
    assert_eq!(matched.negotiated.rate, SampleRate::HZ_44100);
    assert_eq!(matched.mode, OutputMode::BitPerfect);

    player
        .request(Command::SetBitPerfect(false))?
        .wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| {
            player
                .state()
                .output
                .is_some_and(|output| output.negotiated.rate == SampleRate::HZ_48000)
        },
        "the stream to reopen at the rate the graph is already running",
    );

    assert_eq!(
        player.state().output.map(|output| output.mode),
        Some(OutputMode::Converted),
        "following the graph left the stream unconverted"
    );
    assert!(!player.output_settings().prefer_bit_perfect);
    assert_eq!(graph.lock().opens, 2);
    Ok(())
}

#[test]
fn leaving_the_graph_rate_to_the_daemon_stops_the_stream_asking_for_it() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let path = tree.write("track.wav", &source.file);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the stream to open");

    assert_eq!(
        graph
            .lock()
            .requests
            .last()
            .map(|request| request.force_graph_rate),
        Some(true),
        "the first stream did not ask the graph to switch"
    );

    player
        .request(Command::SetForceGraphRate(false))?
        .wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| !player.output_settings().force_graph_rate && graph.lock().opens == 2,
        "the stream to reopen without asking for the graph's rate",
    );

    assert_eq!(
        graph
            .lock()
            .requests
            .last()
            .map(|request| request.force_graph_rate),
        Some(false)
    );
    Ok(())
}

#[test]
fn setting_the_buffer_reopens_the_stream_around_the_new_depth() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let path = tree.write("track.wav", &source.file);
    let deeper = Duration::from_millis(250);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the stream to open");
    assert_eq!(player.output_settings().buffer, config().buffer);

    player
        .request(Command::SetBuffer(deeper))?
        .wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| player.output_settings().buffer == deeper && graph.lock().opens == 2,
        "the stream to reopen around the deeper buffer",
    );

    assert!(
        playing(&player),
        "the deeper buffer left the transport idle"
    );
    Ok(())
}

#[test]
fn a_queue_that_runs_out_finishes_every_track_then_stops() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, 4_000);
    let first = tree.write("first.wav", &source.file);
    let second = tree.write("second.wav", &source.file);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: vec![track(&first, 1), track(&second, 2)],
        start_at: 0,
        autoplay: true,
    })?;

    play_until(
        &player,
        &graph,
        BLOCK_FRAMES * frame_bytes(SampleFormat::S16),
        |player, _| player.state().playback == PlaybackState::Stopped,
        "the queue to run out",
    );

    let mut started = Vec::new();
    let mut finished = Vec::new();
    let mut ended = false;
    for event in player.events().try_iter() {
        match event {
            Event::TrackStarted(id) => started.push(id.get()),
            Event::TrackFinished(id) => finished.push(id.get()),
            Event::QueueFinished => ended = true,
            Event::Failed { error, .. } => panic!("playback failed: {error}"),
            _ => {}
        }
    }

    assert_eq!(started, [1, 2]);
    assert_eq!(finished, [1, 2]);
    assert!(ended, "the queue never reported itself finished");
    assert_eq!(
        graph.lock().played.len(),
        source.stream.len() * 2,
        "both tracks did not reach the graph in full"
    );
    Ok(())
}

#[test]
fn a_graph_that_lets_go_of_every_stream_stops_a_repeating_queue_rather_than_looping() -> Result<()>
{
    let tree = Tree::new();
    let source = pcm(16, 4_000);
    let first = tree.write("first.wav", &source.file);
    let second = tree.write("second.wav", &source.file);

    let (backend, graph) = FakeSink::new(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])]);
    let backend = backend.letting_go_of_every_ring();
    let player = Player::with_backend(config(), move |_| Ok(Box::new(backend)))?;
    player
        .request(Command::SetRepeat(RepeatMode::Queue))?
        .wait_for(PATIENCE)?;
    player.send(Command::Load {
        items: vec![track(&first, 1), track(&second, 2)],
        start_at: 0,
        autoplay: true,
    })?;

    wait_for(
        &player,
        |player| player.state().playback == PlaybackState::Stopped,
        "a queue whose every stream is let go of to stop",
    );

    let failures = player
        .events()
        .try_iter()
        .filter(|event| matches!(event, Event::Failed { .. }))
        .count();
    assert!(
        failures <= 3,
        "a queue of two rows failed {failures} times before it stopped"
    );
    assert!(
        graph.lock().opens <= 4,
        "the graph was asked for {} streams, one to wait out the first loss and one a failure",
        graph.lock().opens
    );
    Ok(())
}

#[test]
fn next_drains_the_current_track_and_opens_the_following_one() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let first = tree.write("first.wav", &source.file);
    let second = tree.write("second.wav", &source.file);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: vec![track(&first, 1), track(&second, 2)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the first track to open");

    player.request(Command::Next)?.wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| {
            player
                .state()
                .current
                .is_some_and(|track| track.id.get() == 2)
        },
        "the second track to take over",
    );

    assert_eq!(player.state().queue_position, Some(1));
    assert_eq!(graph.lock().opens, 2);
    Ok(())
}

#[test]
fn a_track_boundary_ends_when_the_graph_says_it_is_done_not_on_its_own_clock() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, 4_000);
    let first = tree.write("first.wav", &source.file);
    let second = tree.write("second.wav", &source.file);

    let unplayable_tail = u64::from(RATE) * 30;
    let (player, graph) = player_with_tail(
        vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])],
        unplayable_tail,
        true,
    )?;
    player.send(Command::Load {
        items: vec![track(&first, 1), track(&second, 2)],
        start_at: 0,
        autoplay: true,
    })?;

    play_until(
        &player,
        &graph,
        BLOCK_FRAMES * frame_bytes(SampleFormat::S16),
        |player, graph| {
            graph.opens == 2
                && player
                    .state()
                    .current
                    .is_some_and(|track| track.id.get() == 2)
        },
        "the second track to open without waiting out a thirty-second tail",
    );
    Ok(())
}

#[test]
fn a_rebind_mid_track_never_publishes_a_position_behind_one_already_published() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, RATE as usize * 4);
    let path = tree.write("long.wav", &source.file);

    let (player, graph) = player_with_tail(
        vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])],
        u64::from(RATE) / 10,
        true,
    )?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;

    let block = BLOCK_FRAMES * frame_bytes(SampleFormat::S16);
    let position = |player: &Player| {
        player
            .state()
            .current
            .map_or(Frames::ZERO, |track| track.position)
    };
    play_until(
        &player,
        &graph,
        block,
        |player, _| position(player) >= Frames(u64::from(RATE)),
        "a second of the track to play",
    );

    let mut furthest = position(&player);
    player
        .request(Command::SetDither(DitherKind::None))?
        .wait_for(PATIENCE)?;

    let mut behind = Vec::new();
    play_until(
        &player,
        &graph,
        block,
        |player, _| {
            let now = position(player);
            if now < furthest {
                behind.push((furthest, now));
            }
            furthest = furthest.max(now);
            now >= Frames(u64::from(RATE) * 2)
        },
        "the rebound track to play on",
    );

    assert!(
        behind.is_empty(),
        "the published position stepped back after a rebind: {behind:?}"
    );
    Ok(())
}

#[test]
fn a_graph_that_never_reports_a_drain_still_advances_on_the_delay_it_reports() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, 4_000);
    let first = tree.write("first.wav", &source.file);
    let second = tree.write("second.wav", &source.file);

    let (player, graph) = player_with_tail(
        vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])],
        u64::from(RATE) / 10,
        false,
    )?;
    player.send(Command::Load {
        items: vec![track(&first, 1), track(&second, 2)],
        start_at: 0,
        autoplay: true,
    })?;

    play_until(
        &player,
        &graph,
        BLOCK_FRAMES * frame_bytes(SampleFormat::S16),
        |player, graph| {
            graph.opens == 2
                && player
                    .state()
                    .current
                    .is_some_and(|track| track.id.get() == 2)
        },
        "the second track to open once the reported tail had played out",
    );
    Ok(())
}

#[test]
fn the_queue_the_engine_publishes_is_the_order_it_will_play() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let paths: Vec<PathBuf> = (0..4)
        .map(|n| tree.write(&format!("{n}.wav"), &source.file))
        .collect();

    let (player, _graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    assert!(player.queue().is_empty());

    let items: Vec<QueueItem> = paths
        .iter()
        .enumerate()
        .map(|(index, path)| track(path, index as u64 + 1))
        .collect();
    player
        .request(Command::Load {
            items: items.clone(),
            start_at: 0,
            autoplay: true,
        })?
        .wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| player.queue().len() == 4,
        "the queue to publish",
    );
    stand_still(&player);

    assert_eq!(player.queue().as_slice(), items.as_slice());
    assert_eq!(player.state().queue_position, Some(0));

    player
        .request(Command::SetShuffle(true))?
        .wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| player.state().shuffle,
        "the shuffle to land",
    );

    let shuffled = player.queue();
    let mut seen: Vec<u64> = shuffled.iter().map(|item| item.id.get()).collect();
    seen.sort_unstable();
    assert_eq!(seen, [1, 2, 3, 4], "shuffling lost or duplicated an item");
    assert_eq!(
        player.state().queue_position,
        Some(0),
        "the playing track did not stay at the head of the shuffle"
    );
    assert_eq!(
        shuffled.first().map(|item| item.id),
        player.state().current.map(|track| track.id),
        "the head of the published order is not the track playing"
    );

    player.request(Command::JumpTo(2))?.wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| player.state().queue_position == Some(2),
        "the jump to land on the row it was given",
    );
    stand_still(&player);
    let jumped = player.state();
    assert_eq!(
        jumped.current.map(|track| track.id),
        shuffled.get(2).map(|item| item.id),
        "jumping by row played a different track"
    );
    assert_eq!(
        jumped.loaded_position,
        items
            .iter()
            .position(|item| Some(item.id) == shuffled.get(2).map(|shuffled| shuffled.id)),
        "the row of the order the queue was loaded in did not follow the shuffled row"
    );

    player
        .request(Command::SetShuffle(false))?
        .wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| !player.state().shuffle,
        "the shuffle to be turned off",
    );
    let unshuffled = player.state();
    assert_eq!(
        player.queue().as_slice(),
        items.as_slice(),
        "the load order did not come back"
    );
    assert_eq!(
        unshuffled.loaded_position, unshuffled.queue_position,
        "the two rows parted although the play order is the load order"
    );
    Ok(())
}

#[test]
fn editing_the_queue_leaves_the_playing_track_alone_until_its_own_row_goes() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let paths: Vec<PathBuf> = (0..3)
        .map(|n| tree.write(&format!("{n}.wav"), &source.file))
        .collect();
    let added = tree.write("added.wav", &source.file);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: (0..3).map(|n| track(&paths[n], n as u64 + 1)).collect(),
        start_at: 1,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the second track to open");
    stand_still(&player);

    player
        .request(Command::Insert {
            items: vec![track(&added, 90)],
            at: Placement::At(0),
            play: false,
        })?
        .wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| player.queue().len() == 4,
        "the inserted row to publish",
    );

    assert_eq!(
        player.queue().first().map(|item| item.id.get()),
        Some(90),
        "the row did not land where it was asked for"
    );
    assert_eq!(player.state().queue_position, Some(2));
    assert_eq!(player.state().current.map(|track| track.id.get()), Some(2));
    assert_eq!(graph.lock().opens, 1, "an insert reopened the stream");

    player
        .request(Command::Remove(Span::one(0)))?
        .wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| player.queue().len() == 3,
        "the removed row to publish",
    );
    assert_eq!(player.state().queue_position, Some(1));
    assert_eq!(player.state().current.map(|track| track.id.get()), Some(2));
    assert_eq!(graph.lock().opens, 1, "a removal reopened the stream");

    player
        .request(Command::Remove(Span::one(1)))?
        .wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| {
            player
                .state()
                .current
                .is_some_and(|track| track.id.get() == 3)
        },
        "the row that slid in to take over",
    );
    assert_eq!(player.state().queue_position, Some(1));
    assert_eq!(player.queue().len(), 2);
    assert!(
        player.state().output.is_none(),
        "removing the playing row bound its replacement although nothing was playing"
    );
    assert_eq!(graph.lock().opens, 1);

    player.request(Command::Play)?.wait_for(PATIENCE)?;
    wait_for(&player, playing, "the row that took over to play");
    assert_eq!(graph.lock().opens, 2);

    let outcome = player
        .request(Command::Remove(Span::one(9)))?
        .wait_for(PATIENCE);
    assert!(outcome.is_err(), "a row past the end was removed");
    Ok(())
}

#[test]
fn queueing_a_row_next_leaves_the_track_playing_and_does_not_start_one() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let paths: Vec<PathBuf> = (0..3)
        .map(|n| tree.write(&format!("{n}.wav"), &source.file))
        .collect();
    let queued = tree.write("queued.wav", &source.file);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: (0..3).map(|n| track(&paths[n], n as u64 + 1)).collect(),
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the first track to open");

    player
        .request(Command::Insert {
            items: vec![track(&queued, 90)],
            at: Placement::Next,
            play: false,
        })?
        .wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| player.queue().len() == 4,
        "the queued row to publish",
    );

    assert_eq!(
        player.queue().get(1).map(|item| item.id.get()),
        Some(90),
        "the row did not land after the one playing"
    );
    assert_eq!(player.state().queue_position, Some(0));
    assert_eq!(player.state().current.map(|track| track.id.get()), Some(1));
    assert_eq!(graph.lock().opens, 1, "queueing reopened the stream");

    player.request(Command::Next)?.wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| {
            player
                .state()
                .current
                .is_some_and(|track| track.id.get() == 90)
        },
        "the queued row to take over",
    );
    Ok(())
}

#[test]
fn queueing_into_an_empty_queue_waits_for_play() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let queued = tree.write("queued.wav", &source.file);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player
        .request(Command::Insert {
            items: vec![track(&queued, 90)],
            at: Placement::Last,
            play: false,
        })?
        .wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| player.queue().len() == 1,
        "the queued row to publish",
    );

    assert_eq!(player.state().current, None, "queueing started a track");
    assert_eq!(graph.lock().opens, 0, "queueing opened the graph");

    player.request(Command::Play)?.wait_for(PATIENCE)?;
    wait_for(&player, playing, "the queued row to open on play");
    assert_eq!(player.state().current.map(|track| track.id.get()), Some(90));
    Ok(())
}

#[test]
fn queueing_into_an_empty_queue_to_hear_it_plays_it_without_a_play() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let queued = tree.write("queued.wav", &source.file);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player
        .request(Command::Insert {
            items: vec![track(&queued, 90)],
            at: Placement::Last,
            play: true,
        })?
        .wait_for(PATIENCE)?;
    wait_for(&player, playing, "the queued row to open on its own");

    assert_eq!(player.state().current.map(|track| track.id.get()), Some(90));
    assert_eq!(player.state().queue_position, Some(0));
    assert_eq!(
        graph.lock().opens,
        1,
        "queueing to hear it did not open the graph exactly once"
    );
    Ok(())
}

#[test]
fn queueing_a_row_next_to_hear_it_jumps_to_the_row_that_landed() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let paths: Vec<PathBuf> = (0..3)
        .map(|n| tree.write(&format!("{n}.wav"), &source.file))
        .collect();
    let queued = tree.write("queued.wav", &source.file);

    let (player, _graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: (0..3).map(|n| track(&paths[n], n as u64 + 1)).collect(),
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the first track to open");

    player
        .request(Command::Insert {
            items: vec![track(&queued, 90)],
            at: Placement::Next,
            play: true,
        })?
        .wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| plays(player, 90),
        "the queued row to take over",
    );
    stand_still(&player);

    assert_eq!(player.state().queue_position, Some(1));
    assert_eq!(
        player.queue().get(1).map(|item| item.id.get()),
        Some(90),
        "the row did not land after the one playing"
    );
    assert_eq!(player.queue().len(), 4);
    Ok(())
}

#[test]
fn moving_a_row_reorders_the_queue_without_disturbing_the_stream() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let paths: Vec<PathBuf> = (0..3)
        .map(|n| tree.write(&format!("{n}.wav"), &source.file))
        .collect();

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: (0..3).map(|n| track(&paths[n], n as u64 + 1)).collect(),
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the first track to open");

    player
        .request(Command::Move {
            rows: Span::one(2),
            to: 0,
        })?
        .wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| {
            player
                .queue()
                .first()
                .is_some_and(|item| item.id.get() == 3)
        },
        "the moved row to publish",
    );

    assert_eq!(
        player.state().queue_position,
        Some(1),
        "the playing row did not slide down"
    );
    assert_eq!(player.state().current.map(|track| track.id.get()), Some(1));
    assert_eq!(graph.lock().opens, 1, "a move reopened the stream");
    assert!(playing(&player), "a move stopped the transport");

    let order: Vec<u64> = player.queue().iter().map(|item| item.id.get()).collect();
    assert_eq!(order, vec![3, 1, 2]);

    player
        .request(Command::Move {
            rows: Span::one(1),
            to: 2,
        })?
        .wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| player.state().queue_position == Some(2),
        "the playing row to follow the move",
    );
    assert_eq!(player.state().current.map(|track| track.id.get()), Some(1));

    let outcome = player
        .request(Command::Move {
            rows: Span::one(0),
            to: 9,
        })?
        .wait_for(PATIENCE);
    assert!(outcome.is_err(), "a row past the end was moved");
    Ok(())
}

#[test]
fn a_named_sink_binds_when_the_device_turns_up_rather_than_at_startup() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let first = tree.write("first.wav", &source.file);
    let second = tree.write("second.wav", &source.file);

    let (backend, graph) = FakeSink::new(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])]);
    let wanted = NodeName::new("alsa_output.usb");
    let player = Player::with_backend(
        EngineConfig {
            sink: Some(wanted.clone()),
            ..config()
        },
        move |_| Ok(Box::new(backend)),
    )?;

    player.send(Command::Load {
        items: vec![track(&first, 1), track(&second, 2)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(
        &player,
        playing,
        "the stream to open on the only device there",
    );
    assert_eq!(
        player.state().output.map(|output| output.sink),
        Some(SinkId::new(1)),
        "a device that was not in the graph was bound anyway"
    );

    announce(&graph, arriving());

    player.request(Command::Next)?.wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| {
            player
                .state()
                .output
                .is_some_and(|output| output.sink == SinkId::new(2))
        },
        "the named device to take over once it turned up",
    );
    assert_eq!(
        graph.lock().requests.last().map(|request| request.target),
        Some(Some(SinkId::new(2)))
    );
    Ok(())
}

#[test]
fn a_sink_list_with_no_devices_fails_the_load_rather_than_playing_silence() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, 4_000);
    let path = tree.write("track.wav", &source.file);

    let (player, graph) = player(Vec::new())?;
    let outcome = player
        .request(Command::Load {
            items: vec![track(&path, 1)],
            start_at: 0,
            autoplay: true,
        })?
        .wait_for(PATIENCE);

    assert!(outcome.is_err(), "a queue with no sink started playing");
    assert_eq!(graph.lock().opens, 0);
    Ok(())
}

#[test]
fn a_source_that_is_not_the_local_files_reaches_the_graph_the_same_way() -> Result<()> {
    let source = pcm(16, FRAMES);
    let named = SourceId::new("held").expect("a lowercase name");
    let sources = Sources::local().and(Arc::new(InMemory {
        source: named.clone(),
        key: "tracks/1.wav".to_owned(),
        bytes: source.file.clone(),
    }));
    let location = MediaLocation::new(named, "tracks/1.wav");

    let (player, graph) = player_over(
        vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])],
        Arc::new(sources),
    )?;
    player.send(Command::Load {
        items: vec![QueueItem {
            id: TrackId::new(1).expect("a non-zero track id"),
            location: location.clone(),
            span: None,
        }],
        start_at: 0,
        autoplay: true,
    })?;

    let wanted = source.stream.len();
    play_until(
        &player,
        &graph,
        BLOCK_FRAMES * frame_bytes(SampleFormat::S16),
        |_, graph| graph.played.len() >= wanted,
        "a track held in memory to reach the graph",
    );

    assert_eq!(
        graph.lock().played,
        source.stream,
        "a source that is not the local files did not survive the stream"
    );
    assert_eq!(
        player.digest().map(|digest| digest.location.clone()),
        Some(location),
        "the inspector was told a different location than the one that played"
    );
    Ok(())
}

#[test]
fn a_source_that_serves_one_handle_plays_without_the_layout_the_inspector_draws() -> Result<()> {
    let source = pcm(16, FRAMES);
    let named = SourceId::new("once").expect("a lowercase name");
    let sources = Sources::local().and(Arc::new(ServesOnce {
        source: named.clone(),
        key: "tracks/1.wav".to_owned(),
        bytes: source.file.clone(),
        served: AtomicU64::new(0),
    }));
    let location = MediaLocation::new(named, "tracks/1.wav");

    let (player, graph) = player_over(
        vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])],
        Arc::new(sources),
    )?;
    player.send(Command::Load {
        items: vec![QueueItem {
            id: TrackId::new(1).expect("a non-zero track id"),
            location,
            span: None,
        }],
        start_at: 0,
        autoplay: true,
    })?;

    let wanted = source.stream.len();
    play_until(
        &player,
        &graph,
        BLOCK_FRAMES * frame_bytes(SampleFormat::S16),
        |_, graph| graph.played.len() >= wanted,
        "a track whose second open is refused to reach the graph",
    );

    assert_eq!(
        graph.lock().played,
        source.stream,
        "a source that serves one handle did not survive the stream"
    );
    assert!(
        player
            .digest()
            .is_some_and(|digest| digest.layout.is_none()),
        "a box layout was drawn from a handle the source never served"
    );
    Ok(())
}

#[test]
fn a_queue_row_cut_to_a_span_hands_the_graph_that_span_and_stops() -> Result<()> {
    let source = pcm(16, FRAMES);
    let named = SourceId::new("held").expect("a lowercase name");
    let sources = Sources::local().and(Arc::new(InMemory {
        source: named.clone(),
        key: "album.wav".to_owned(),
        bytes: source.file.clone(),
    }));
    let location = MediaLocation::new(named, "album.wav");

    let start = Frames(1_000);
    let end = Frames(4_000);
    let stride = frame_bytes(SampleFormat::S16);
    let wanted = source.stream[start.get() as usize * stride..end.get() as usize * stride].to_vec();

    let (player, graph) = player_over(
        vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])],
        Arc::new(sources),
    )?;
    player.send(Command::Load {
        items: vec![QueueItem {
            id: TrackId::new(1).expect("a non-zero track id"),
            location: location.clone(),
            span: Some(FrameSpan::between(start, end)),
        }],
        start_at: 0,
        autoplay: true,
    })?;

    play_until(
        &player,
        &graph,
        BLOCK_FRAMES * stride,
        |_, graph| graph.played.len() >= wanted.len(),
        "a span to reach the graph",
    );

    assert_eq!(
        graph.lock().played,
        wanted,
        "the graph was handed something other than the span the row names"
    );
    assert_eq!(
        player.state().current.and_then(|track| track.duration),
        Some(end.saturating_sub(start)),
        "a row cut to a span reported a length that is not the span's"
    );
    Ok(())
}

const SHEET: &str = r#"PERFORMER "Pink Floyd"
TITLE "Meddle"
FILE "Meddle.wav" WAVE
  TRACK 01 AUDIO
    TITLE "One of These Days"
    REM REPLAYGAIN_TRACK_GAIN -3.00 dB
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "Echoes"
    REM REPLAYGAIN_TRACK_GAIN -7.06 dB
    INDEX 01 00:00:15
"#;

#[test]
fn a_row_a_sheet_cut_it_out_of_plays_at_the_gain_that_sheet_declares_for_it() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write("Meddle.wav", &pcm(16, FRAMES).file);
    tree.write("Meddle.cue", SHEET.as_bytes());

    let (player, graph) = settled_over(
        vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])],
        Arc::new(Sources::local()),
        EngineConfig {
            replay_gain: ReplayGainMode::Track,
            ..config()
        },
    )?;
    player.send(Command::Load {
        items: vec![QueueItem {
            id: TrackId::new(1).expect("a non-zero track id"),
            location: MediaLocation::local(&file),
            span: Some(FrameSpan::between(SECOND_ROW, Frames(FRAMES as u64))),
        }],
        start_at: 0,
        autoplay: true,
    })?;

    play_until(
        &player,
        &graph,
        BLOCK_FRAMES * frame_bytes(SampleFormat::S16),
        |player, _| player.digest().is_some(),
        "the row to be opened",
    );

    let digest = player.digest().expect("a digest for the row being played");
    assert_eq!(
        digest.info.tags.title.as_deref(),
        Some("Echoes"),
        "the row was billed as the file it was cut out of"
    );
    assert_eq!(
        digest.replay_gain.gain,
        Decibels::new(-7.06).ok(),
        "the row plays at the gain the file declares rather than its own"
    );
    Ok(())
}

const PEAK_NORMALISED: &str = r#"FILE "Normalised.wav" WAVE
  TRACK 01 AUDIO
    TITLE "Full Scale"
    REM REPLAYGAIN_TRACK_GAIN +6.00 dB
    REM REPLAYGAIN_TRACK_PEAK 1.000000
    INDEX 01 00:00:00
"#;

#[test]
fn a_boost_its_full_scale_peak_caps_at_unity_reaches_the_graph_byte_for_byte() -> Result<()> {
    for dither in [DitherKind::None, DitherKind::Triangular] {
        let tree = Tree::new();
        let source = pcm(16, FRAMES);
        let file = tree.write("Normalised.wav", &source.file);
        tree.write("Normalised.cue", PEAK_NORMALISED.as_bytes());

        let (player, graph) = settled_over(
            vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])],
            Arc::new(Sources::local()),
            EngineConfig {
                replay_gain: ReplayGainMode::Track,
                dither,
                ..config()
            },
        )?;
        player.send(Command::Load {
            items: vec![QueueItem {
                id: TrackId::new(1).expect("a non-zero track id"),
                location: MediaLocation::local(&file),
                span: Some(FrameSpan::between(Frames::ZERO, Frames(FRAMES as u64))),
            }],
            start_at: 0,
            autoplay: true,
        })?;

        let wanted = source.stream.len();
        let mut mode = None;
        let mut applied = None;
        play_until(
            &player,
            &graph,
            BLOCK_FRAMES * frame_bytes(SampleFormat::S16),
            |player, graph| {
                if let Some(output) = player.state().output {
                    mode = Some(output.mode);
                }
                if let Some(digest) = player.digest() {
                    applied = Some(digest.replay_gain);
                }
                mode.is_some() && applied.is_some() && graph.played.len() >= wanted
            },
            "the whole track to reach the graph",
        );

        assert_eq!(
            applied.and_then(|applied| applied.gain),
            Decibels::new(6.0).ok(),
            "the sheet's boost never reached the engine, so nothing was proved"
        );
        assert_eq!(
            applied.and_then(|applied| applied.peak),
            Gain::new(1.0).ok()
        );

        let graph = graph.lock();
        assert_eq!(
            graph.played.get(..wanted),
            Some(source.stream.as_slice()),
            "a boost capped at unity changed the samples with {dither:?} dither"
        );
        assert_eq!(
            mode,
            Some(OutputMode::BitPerfect),
            "a boost capped at unity left the bit-perfect path with {dither:?} dither"
        );
        assert_eq!(
            graph.requests.first().map(|request| request.no_convert),
            Some(true)
        );
    }
    Ok(())
}

#[test]
fn a_dsd_track_reaches_a_dop_capable_sink_with_its_markers_intact() -> Result<()> {
    let named = SourceId::new("held").expect("a lowercase name");
    let sources = Sources::local().and(Arc::new(InMemory {
        source: named.clone(),
        key: "tone.dsf".to_owned(),
        bytes: dsf_tone(),
    }));
    let location = MediaLocation::new(named, "tone.dsf");

    let (player, graph) = settled_over(
        vec![sink(&[SampleRate::HZ_176400], &[SampleFormat::S24])],
        Arc::new(sources),
        EngineConfig {
            dop: true,
            ..config()
        },
    )?;
    player.send(Command::Load {
        items: vec![QueueItem::whole(
            TrackId::new(1).expect("a non-zero track id"),
            location,
        )],
        start_at: 0,
        autoplay: true,
    })?;

    let stride = frame_bytes(SampleFormat::S24);
    play_until(
        &player,
        &graph,
        BLOCK_FRAMES * stride,
        |_, graph| graph.played.len() >= 512 * stride,
        "a DoP stream to reach the graph",
    );

    let played = graph.lock().played.clone();
    let words = played.as_chunks::<4>().0.to_vec();

    assert!(
        !words.is_empty(),
        "the graph was handed no DoP words at all"
    );
    for (index, word) in words.iter().enumerate().take(256) {
        let frame = index / usize::from(CHANNELS);
        let wanted = if frame % 2 == 0 { 0x05 } else { 0xFA };
        assert_eq!(
            word[2], wanted,
            "word {index} reached the graph carrying {:#04x} where a DAC reads {wanted:#04x}",
            word[2]
        );
    }
    Ok(())
}

#[test]
fn a_seek_on_a_dop_stream_marks_the_prime_window_rather_than_dropping_the_carrier() -> Result<()> {
    let named = SourceId::new("held").expect("a lowercase name");
    let sources = Sources::local().and(Arc::new(InMemory {
        source: named.clone(),
        key: "tone.dsf".to_owned(),
        bytes: dsf_tone(),
    }));
    let location = MediaLocation::new(named, "tone.dsf");

    let (player, graph) = settled_over(
        vec![sink(&[SampleRate::HZ_176400], &[SampleFormat::S24])],
        Arc::new(sources),
        EngineConfig {
            dop: true,
            buffer: Duration::from_millis(20),
            ..config()
        },
    )?;
    player.send(Command::Load {
        items: vec![QueueItem::whole(
            TrackId::new(1).expect("a non-zero track id"),
            location,
        )],
        start_at: 0,
        autoplay: true,
    })?;

    let stride = frame_bytes(SampleFormat::S24);
    let block = BLOCK_FRAMES * stride;
    play_until(
        &player,
        &graph,
        block,
        |_, graph| graph.played.len() >= block,
        "the first block to play",
    );

    player
        .request(Command::Seek(Frames(4_096)))?
        .wait_for(PATIENCE)?;

    let primed = {
        let mut held = graph.lock();
        held.played.clear();
        assert_eq!(
            held.pull(block),
            block,
            "the prime window handed the graph a gap rather than its carrier"
        );
        held.played.clone()
    };

    for (index, word) in primed.as_chunks::<4>().0.iter().enumerate() {
        let frame = index / usize::from(CHANNELS);
        let wanted = if frame % 2 == 0 { 0x05 } else { 0xFA };
        assert_eq!(
            word[2], wanted,
            "word {index} of the prime window left the marker a DAC locks onto"
        );
        assert_eq!(
            [word[0], word[1]],
            [0x69, 0x69],
            "word {index} of the prime window was not a silent DSD pair"
        );
    }
    Ok(())
}

#[test]
fn a_dop_stream_turned_down_and_back_up_is_marked_again_within_the_track() -> Result<()> {
    let named = SourceId::new("held").expect("a lowercase name");
    let sources = Sources::local().and(Arc::new(InMemory {
        source: named.clone(),
        key: "tone.dsf".to_owned(),
        bytes: dsf_tone(),
    }));
    let location = MediaLocation::new(named, "tone.dsf");

    let (player, graph) = settled_over(
        vec![sink(&[SampleRate::HZ_176400], &[SampleFormat::S24])],
        Arc::new(sources),
        EngineConfig {
            dop: true,
            ..config()
        },
    )?;
    player.send(Command::Load {
        items: vec![QueueItem::whole(
            TrackId::new(1).expect("a non-zero track id"),
            location,
        )],
        start_at: 0,
        autoplay: true,
    })?;

    let stride = frame_bytes(SampleFormat::S24);
    play_until(
        &player,
        &graph,
        BLOCK_FRAMES * stride,
        |player, graph| {
            player.state().output.map(|output| output.mode) == Some(OutputMode::BitPerfect)
                && graph.played.len() >= 64 * stride
        },
        "a DoP stream to reach the graph",
    );

    player
        .request(Command::SetVolume(Volume::new(0.5).expect("half volume")))?
        .wait_for(PATIENCE)?;
    assert_eq!(
        player.state().output.map(|output| output.mode),
        Some(OutputMode::Converted),
        "a DoP stream turned down was not decimated"
    );

    player
        .request(Command::SetVolume(Volume::MAX))?
        .wait_for(PATIENCE)?;
    assert_eq!(
        player.state().output.map(|output| output.mode),
        Some(OutputMode::BitPerfect),
        "a decimated stream brought back to full volume stayed decimated until the next track"
    );

    let marked_from = graph.lock().played.len();
    play_until(
        &player,
        &graph,
        BLOCK_FRAMES * stride,
        |_, graph| graph.played.len() >= marked_from + 512 * stride,
        "the stream to play on after it was marked again",
    );
    let played = graph.lock().played.clone();
    let tail = played
        .get(played.len() - 256 * stride..)
        .unwrap_or_default();
    assert!(
        tail.as_chunks::<4>()
            .0
            .windows(2)
            .all(|pair| matches!((pair[0][2], pair[1][2]), (0x05 | 0xFA, 0x05 | 0xFA))),
        "the stream marked again carried no DoP markers"
    );
    Ok(())
}

#[test]
fn a_dsd_track_a_sink_cannot_take_is_decimated_rather_than_marked() -> Result<()> {
    let named = SourceId::new("held").expect("a lowercase name");
    let sources = Sources::local().and(Arc::new(InMemory {
        source: named.clone(),
        key: "tone.dsf".to_owned(),
        bytes: dsf_tone(),
    }));
    let location = MediaLocation::new(named, "tone.dsf");

    let (player, graph) = settled_over(
        vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])],
        Arc::new(sources),
        EngineConfig {
            dop: true,
            ..config()
        },
    )?;
    player.send(Command::Load {
        items: vec![QueueItem::whole(
            TrackId::new(1).expect("a non-zero track id"),
            location,
        )],
        start_at: 0,
        autoplay: true,
    })?;

    let stride = frame_bytes(SampleFormat::S16);
    let mut negotiated = None;
    play_until(
        &player,
        &graph,
        SHORT_PULL_FRAMES * stride,
        |player, graph| {
            if let Some(output) = player.state().output {
                negotiated = Some(output.negotiated.rate);
            }
            negotiated.is_some() && graph.played.len() >= 256 * stride
        },
        "a decimated DSD stream to reach the graph",
    );

    assert_eq!(
        negotiated,
        Some(SampleRate::HZ_44100),
        "a sink that cannot take the carrier was not resampled to"
    );
    assert!(
        graph.lock().played.iter().any(|byte| *byte != 0),
        "a decimated DSD track reached the graph as silence"
    );
    Ok(())
}

fn ring_deep() -> EngineConfig {
    EngineConfig {
        buffer: RING_DEPTH,
        ..config()
    }
}

fn frames_in(duration: Duration, rate: SampleRate) -> usize {
    Frames::from_duration(duration, rate).get() as usize
}

fn words(bytes: &[u8]) -> Vec<i16> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|word| i16::from_le_bytes(*word))
        .collect()
}

fn loudness(bytes: &[u8]) -> f64 {
    let words = bytes.as_chunks::<4>().0;
    let summed: f64 = words
        .iter()
        .map(|word| f64::from(i32::from_le_bytes(*word)).abs())
        .sum();
    summed / words.len().max(1) as f64
}

fn level_at(heard: &[i16], meant: &[i16], at: usize) -> Option<f64> {
    let meant = f64::from(*meant.get(at)?);
    let heard = f64::from(*heard.get(at)?);
    (meant.abs() >= LOUD_ENOUGH_TO_READ_A_LEVEL).then(|| heard / meant)
}

fn mean_level(heard: &[i16], meant: &[i16], over: Range<usize>) -> f64 {
    let levels: Vec<f64> = over.filter_map(|at| level_at(heard, meant, at)).collect();
    levels.iter().sum::<f64>() / levels.len().max(1) as f64
}

fn steepest_level_change(heard: &[i16], meant: &[i16]) -> f64 {
    let channels = usize::from(CHANNELS);
    (channels..heard.len())
        .filter_map(|at| {
            let now = level_at(heard, meant, at)?;
            let before = level_at(heard, meant, at - channels)?;
            Some((now - before).abs())
        })
        .fold(0.0, f64::max)
}

#[test]
fn turning_a_bit_perfect_track_down_and_back_up_keeps_its_stream_and_every_frame() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, 4 * FRAMES);
    let path = tree.write("track.wav", &source.file);
    let rate = SampleRate::HZ_44100;
    let stride = frame_bytes(SampleFormat::S16);
    let sample = usize::from(SampleFormat::S16.bytes_per_sample().get());
    let block = BLOCK_FRAMES * stride;
    let ring = frames_in(RING_DEPTH, rate) * stride;
    let ramp = frames_in(A_RAMP_AT_MOST, rate) * stride;
    let window = 16 * block;
    let half = Volume::new(0.5).expect("in range");

    let (player, graph) = settled_over(
        vec![sink(&[rate], &[SampleFormat::S16])],
        Arc::new(Sources::local()),
        ring_deep(),
    )?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;

    let mut mode = None;
    play_until(
        &player,
        &graph,
        block,
        |player, graph| {
            mode = player.state().output.map(|output| output.mode);
            graph.played.len() >= 4 * block
        },
        "the first blocks to play",
    );
    assert_eq!(mode, Some(OutputMode::BitPerfect));

    player
        .request(Command::SetVolume(half))?
        .wait_for(PATIENCE)?;
    assert_eq!(
        player.state().output.map(|output| output.mode),
        Some(OutputMode::Converted),
        "turning a bit-perfect track down left it bit-perfect"
    );
    let turned_down = graph.lock().played.len();

    let attenuated = turned_down + ring + ramp;
    play_until(
        &player,
        &graph,
        block,
        |_, graph| graph.played.len() >= attenuated + window,
        "the turned-down audio to reach the graph",
    );

    player
        .request(Command::SetVolume(Volume::MAX))?
        .wait_for(PATIENCE)?;
    let mut restored = None;
    play_until(
        &player,
        &graph,
        block,
        |player, graph| {
            if player.state().output.map(|output| output.mode) == Some(OutputMode::BitPerfect) {
                restored = Some((graph.played.len(), graph.opens, graph.closes));
            }
            restored.is_some()
        },
        "the ramp back to unity to settle into a bit-perfect chain",
    );
    let Some((restored, opens, closes)) = restored else {
        panic!("the chain never came back bit-perfect");
    };
    assert_eq!(
        (opens, closes),
        (1, 0),
        "turning the track down and back up reopened the stream"
    );

    let wanted = source.stream.len();
    play_until(
        &player,
        &graph,
        block,
        |_, graph| graph.played.len() >= wanted,
        "the whole track to reach the graph",
    );

    let graph = graph.lock();
    assert_eq!(
        graph.opens, 1,
        "the track reopened the stream on its way out"
    );
    assert_eq!(
        graph.played.len(),
        wanted,
        "frames were dropped, doubled or padded across the two reshapes"
    );

    let exact = restored + ring;
    assert!(
        exact + block <= wanted,
        "the ramp back settled too late to leave anything to compare"
    );
    assert!(
        graph.played.get(exact..) == source.stream.get(exact..),
        "the audio after the chain came back bit-perfect is not the file's own at the same offsets"
    );

    let heard = words(&graph.played);
    let meant = words(&source.stream);
    let level = mean_level(
        &heard,
        &meant,
        attenuated / sample..(attenuated + window) / sample,
    );
    let asked = f64::from(half.to_gain().get());
    assert!(
        (level - asked).abs() < 0.01,
        "the turned-down audio reached the graph at {level} where the slider asks for {asked}"
    );

    let steepest = steepest_level_change(&heard, &meant);
    assert!(
        steepest < A_LEVEL_STEP,
        "the level stepped by {steepest} between one frame and the next"
    );
    Ok(())
}

#[test]
fn turning_a_resampled_track_down_retunes_the_stream_it_is_already_playing() -> Result<()> {
    let tree = Tree::new();
    let path = tree.write("steady.wav", &steady(HALF_SCALE, 3 * FRAMES));
    let rate = SampleRate::HZ_48000;
    let stride = frame_bytes(SampleFormat::S32);
    let block = BLOCK_FRAMES * stride;
    let ring = frames_in(RING_DEPTH, rate) * stride;
    let ramp = frames_in(A_RAMP_AT_MOST, rate) * stride;
    let half = Volume::new(0.5).expect("in range");

    let (player, graph) = settled_over(
        vec![sink(&[rate], &[SampleFormat::S32])],
        Arc::new(Sources::local()),
        ring_deep(),
    )?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;

    let mut mode = None;
    play_until(
        &player,
        &graph,
        block,
        |player, graph| {
            mode = player.state().output.map(|output| output.mode);
            graph.played.len() >= 8 * block
        },
        "the resampled track to settle",
    );
    assert_eq!(mode, Some(OutputMode::Converted));
    let (before, turned_down) = {
        let held = graph.lock();
        let played = held.played.len();
        (loudness(&held.played[played - block..]), played)
    };

    player
        .request(Command::SetVolume(half))?
        .wait_for(PATIENCE)?;
    let settled = turned_down + ring + ramp + block;
    play_until(
        &player,
        &graph,
        block,
        |_, graph| graph.played.len() >= settled,
        "the turned-down audio to reach the graph",
    );

    let graph = graph.lock();
    assert_eq!(
        graph.opens, 1,
        "turning a resampled track down reopened the stream"
    );
    assert_eq!(
        graph.closes, 0,
        "turning a resampled track down closed the stream"
    );

    let after = loudness(&graph.played[settled - block..settled]);
    let asked = f64::from(half.to_gain().get());
    assert!(
        (after / before - asked).abs() < 0.01,
        "the level went from {before} to {after} where the slider asks for {asked} of it"
    );
    Ok(())
}

fn equalised(bands: Vec<(f64, f64, f64)>) -> Arc<Equalisation> {
    let bands = bands
        .into_iter()
        .map(|(at, gain, q)| {
            Band::new(
                BandKind::Peaking,
                Frequency::from_hertz(at).expect("a frequency in range"),
                BandGain::from_decibels(gain).expect("a gain in range"),
                Q::from_units(q).expect("a Q in range"),
            )
        })
        .collect();

    Arc::new(Equalisation {
        enabled: true,
        bound: BTreeMap::new(),
        fallback: Some(Arc::new(
            Profile::new(Preamp::NONE, bands).expect("a profile in range"),
        )),
    })
}

#[test]
fn a_default_configuration_carries_no_equaliser() {
    let config = config();
    assert!(!config.equaliser.enabled);
    assert!(!config.equaliser.binds_anything());
}

#[test]
fn the_first_band_reshapes_the_chain_in_place_and_switching_off_puts_it_back() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let path = tree.write("track.wav", &source.file);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the stream to open");
    assert_eq!(
        player.state().output.map(|output| output.mode),
        Some(OutputMode::BitPerfect)
    );

    player
        .request(Command::SetEqualisation(equalised(vec![(
            1_000.0, 6.0, 1.0,
        )])))?
        .wait_for(PATIENCE)?;
    assert_eq!(
        player.state().output.map(|output| output.mode),
        Some(OutputMode::Converted),
        "the first band left the stream bit-perfect"
    );
    assert!(playing(&player), "the equaliser left the transport idle");
    {
        let graph = graph.lock();
        assert_eq!(graph.opens, 1, "the first band reopened the stream");
        assert_eq!(graph.closes, 0, "the first band closed the stream");
    }

    player
        .request(Command::SetEqualisation(Arc::new(Equalisation::default())))?
        .wait_for(PATIENCE)?;
    let mut restored = None;
    play_until(
        &player,
        &graph,
        SHORT_PULL_FRAMES * frame_bytes(SampleFormat::S16),
        |player, graph| {
            if player.state().output.map(|output| output.mode) == Some(OutputMode::BitPerfect) {
                restored = Some((graph.opens, graph.closes));
            }
            restored.is_some()
        },
        "the chain to come back bit-perfect without the equaliser",
    );

    assert_eq!(
        restored,
        Some((1, 0)),
        "switching the equaliser off reopened the stream"
    );
    Ok(())
}

#[test]
fn switching_the_equaliser_on_and_off_mid_track_glides_rather_than_steps() -> Result<()> {
    let tree = Tree::new();
    let level = 16_000_i16;
    let path = tree.write("steady.wav", &steady(level, RATE as usize * 2));
    let halved = Arc::new(Equalisation {
        enabled: true,
        bound: BTreeMap::new(),
        fallback: Some(Arc::new(
            Profile::new(
                Preamp::from_decibels(-6.0).expect("in range"),
                vec![Band::new(
                    BandKind::Peaking,
                    Frequency::from_hertz(1_000.0).expect("in range"),
                    BandGain::from_decibels(3.0).expect("in range"),
                    Q::from_units(1.0).expect("in range"),
                )],
            )
            .expect("one band"),
        )),
    });
    let frame = frame_bytes(SampleFormat::S16);
    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the stream to open");
    let pulled_past =
        |frames: usize| move |_: &Player, graph: &Graph| graph.played.len() >= frames * frame;
    play_until(
        &player,
        &graph,
        BLOCK_FRAMES,
        pulled_past(2_048),
        "the steady level to play",
    );

    player
        .request(Command::SetEqualisation(halved))?
        .wait_for(PATIENCE)?;
    play_until(
        &player,
        &graph,
        BLOCK_FRAMES,
        pulled_past(24_000),
        "the equaliser to take hold",
    );
    player
        .request(Command::SetEqualisation(Arc::new(Equalisation::default())))?
        .wait_for(PATIENCE)?;
    play_until(
        &player,
        &graph,
        BLOCK_FRAMES,
        pulled_past(64_000),
        "the equaliser to let go",
    );

    let played = graph.lock().played.clone();
    let left: Vec<i32> = played
        .as_chunks::<4>()
        .0
        .iter()
        .map(|frame| i32::from(i16::from_le_bytes([frame[0], frame[1]])))
        .collect();
    let largest_step = left
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .max()
        .unwrap_or_default();
    let halfway = i32::from(level) / 2;
    assert!(
        left.iter().any(|sample| (sample - halfway).abs() < 64),
        "the equaliser never took the level down to half"
    );
    assert_eq!(left.first().copied(), Some(i32::from(level)));
    assert_eq!(left.last().copied(), Some(i32::from(level)));
    assert!(
        largest_step < 64,
        "the level stepped by {largest_step} where the equaliser came and went"
    );
    Ok(())
}

#[test]
fn the_first_band_and_every_edit_after_it_leave_the_stream_open() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let path = tree.write("track.wav", &source.file);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the stream to open");

    player
        .request(Command::SetEqualisation(equalised(vec![(
            1_000.0, 3.0, 1.0,
        )])))?
        .wait_for(PATIENCE)?;
    assert_eq!(
        player.state().output.map(|output| output.mode),
        Some(OutputMode::Converted),
        "the first band left the stream bit-perfect"
    );

    for gain in [4.0, 5.0, 6.0, 7.0] {
        let wanted = equalised(vec![(1_000.0, gain, 1.0)]);
        player
            .request(Command::SetEqualisation(Arc::clone(&wanted)))?
            .wait_for(PATIENCE)?;
        wait_for(
            &player,
            |player| player.output_settings().equaliser == wanted,
            "the engine to publish the edited profile",
        );
    }

    let graph = graph.lock();
    assert_eq!(
        graph.opens,
        1,
        "adding or editing a band reopened the stream: {}",
        pulls(&graph)
    );
    assert_eq!(
        graph.closes, 0,
        "adding or editing a band closed the stream"
    );
    Ok(())
}

#[test]
fn switching_the_equaliser_on_and_off_under_a_resampler_keeps_the_stream_and_its_level()
-> Result<()> {
    const SETTLED_AFTER: usize = 4_096;
    const STEPS_AT_MOST: f64 = 64.0 / 32_768.0;

    let tree = Tree::new();
    let level = 16_000_i16;
    let path = tree.write("steady.wav", &steady(level, RATE as usize * 2));
    let frame = frame_bytes(SampleFormat::S32);
    let (player, graph) = player(vec![sink(&[SampleRate::HZ_48000], &[SampleFormat::S32])])?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the stream to open");
    let pulled_past =
        |frames: usize| move |_: &Player, graph: &Graph| graph.played.len() >= frames * frame;
    play_until(
        &player,
        &graph,
        BLOCK_FRAMES,
        pulled_past(SETTLED_AFTER * 2),
        "the resampled level to play",
    );

    player
        .request(Command::SetEqualisation(equalised(vec![(
            1_000.0, 3.0, 1.0,
        )])))?
        .wait_for(PATIENCE)?;
    play_until(
        &player,
        &graph,
        BLOCK_FRAMES,
        pulled_past(32_000),
        "the equaliser to take hold",
    );
    player
        .request(Command::SetEqualisation(Arc::new(Equalisation::default())))?
        .wait_for(PATIENCE)?;
    play_until(
        &player,
        &graph,
        BLOCK_FRAMES,
        pulled_past(72_000),
        "the equaliser to let go",
    );

    let graph = graph.lock();
    assert_eq!(
        (graph.opens, graph.closes),
        (1, 0),
        "the equaliser reopened a stream whose resampler it could carry: {}",
        pulls(&graph)
    );
    let left: Vec<f64> = graph
        .played
        .as_chunks::<8>()
        .0
        .iter()
        .map(|frame| {
            f64::from(i32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]))
                / f64::from(i32::MAX)
        })
        .collect();
    let largest_step = left
        .windows(2)
        .skip(SETTLED_AFTER)
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0, f64::max);
    assert!(
        largest_step < STEPS_AT_MOST,
        "the level stepped by {largest_step} where the equaliser came and went"
    );
    Ok(())
}

#[test]
fn a_profile_bound_to_another_device_does_not_reach_this_one() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let path = tree.write("track.wav", &source.file);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the stream to open");

    let elsewhere = Arc::new(Equalisation {
        enabled: true,
        bound: [(
            NodeName::new("alsa_output.somewhere-else"),
            Arc::new(
                Profile::new(
                    Preamp::NONE,
                    vec![Band::new(
                        BandKind::Peaking,
                        Frequency::from_hertz(1_000.0).expect("in range"),
                        BandGain::from_decibels(9.0).expect("in range"),
                        Q::from_units(1.0).expect("in range"),
                    )],
                )
                .expect("one band"),
            ),
        )]
        .into_iter()
        .collect(),
        fallback: None,
    });

    player
        .request(Command::SetEqualisation(Arc::clone(&elsewhere)))?
        .wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| player.output_settings().equaliser == elsewhere,
        "the engine to publish the binding",
    );

    assert_eq!(
        player.state().output.map(|output| output.mode),
        Some(OutputMode::BitPerfect),
        "a profile bound to another device coloured this one"
    );
    assert_eq!(
        graph.lock().opens,
        1,
        "a profile bound to another device reopened this stream"
    );
    Ok(())
}

#[test]
fn the_bytes_the_graph_is_handed_carry_the_correction() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let path = tree.write("track.wav", &source.file);

    let loudness = |equaliser: Option<Arc<Equalisation>>| -> Result<f64> {
        let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
        if let Some(equaliser) = equaliser {
            player
                .request(Command::SetEqualisation(equaliser))?
                .wait_for(PATIENCE)?;
        }
        player.send(Command::Load {
            items: vec![track(&path, 1)],
            start_at: 0,
            autoplay: true,
        })?;
        wait_for(&player, playing, "the stream to open");
        play_until(
            &player,
            &graph,
            BLOCK_FRAMES,
            |_, graph| graph.played.len() >= BLOCK_FRAMES * frame_bytes(SampleFormat::S16) * 8,
            "a run of the tone to reach the graph",
        );

        let played = graph.lock().played.clone();
        player.shutdown()?;

        let words = played.as_chunks::<2>().0;
        let energy: f64 = words
            .iter()
            .map(|word| {
                let sample = f64::from(i16::from_le_bytes(*word));
                sample * sample
            })
            .sum();
        Ok((energy / words.len().max(1) as f64).sqrt())
    };

    let shelved = Arc::new(Equalisation {
        enabled: true,
        bound: BTreeMap::new(),
        fallback: Some(Arc::new(
            Profile::new(
                Preamp::NONE,
                vec![Band::new(
                    BandKind::LowShelf,
                    Frequency::from_hertz(200.0).expect("in range"),
                    BandGain::from_decibels(-12.0).expect("in range"),
                    Q::from_units(0.7).expect("in range"),
                )],
            )
            .expect("one band"),
        )),
    });

    let flat = loudness(None)?;
    let cut = loudness(Some(shelved))?;

    assert!(
        flat > 0.0,
        "nothing reached the graph without the equaliser"
    );
    assert!(
        cut < flat / 2.0,
        "a -12 dB shelf under the whole waveform took {flat:.0} only down to {cut:.0}"
    );
    Ok(())
}

#[test]
fn the_equaliser_survives_a_track_change() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, 4_000);
    let first = tree.write("first.wav", &source.file);
    let second = tree.write("second.wav", &source.file);

    let (player, _graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    let wanted = equalised(vec![(1_000.0, 6.0, 1.0)]);
    player
        .request(Command::SetEqualisation(Arc::clone(&wanted)))?
        .wait_for(PATIENCE)?;

    player.send(Command::Load {
        items: vec![track(&first, 1), track(&second, 2)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the first track to open");

    player.request(Command::Next)?.wait_for(PATIENCE)?;
    wait_for(&player, |player| plays(player, 2), "the second track");

    assert_eq!(player.output_settings().equaliser, wanted);
    assert_eq!(
        player.state().output.map(|output| output.mode),
        Some(OutputMode::Converted),
        "the equaliser did not follow the track change"
    );
    Ok(())
}

#[test]
fn a_queue_resumed_opens_paused_on_the_row_it_was_left_on_at_that_frame() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let paths: Vec<PathBuf> = (0..3)
        .map(|n| tree.write(&format!("{n}.wav"), &source.file))
        .collect();
    let resumed = Frames(FRAMES as u64 / 2);
    let left_on = MediaLocation::local(&paths[1]);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player
        .request(Command::Resume(Resumption {
            rows: (0..3).map(|n| kept(&paths[n])).collect(),
            order: (0..3).collect(),
            row: 1,
            at: resumed,
            shuffle: false,
        }))?
        .wait_for(PATIENCE)?;

    wait_for(
        &player,
        |player| playing_row(player).as_ref() == Some(&left_on),
        "the resumed row to be the one open",
    );
    let state = player.state();
    assert_eq!(
        state.playback,
        PlaybackState::Paused,
        "a queue resumed into started playing on its own"
    );
    assert_eq!(
        state.current.map(|track| track.position),
        Some(resumed),
        "the resumed row did not open where it was left"
    );
    let graph = graph.lock();
    assert!(
        graph.played.is_empty(),
        "a queue resumed into handed the graph audio before anything asked it to"
    );
    assert_eq!(
        graph.opens, 0,
        "a queue resumed into opened a stream before anything asked it to"
    );
    Ok(())
}

#[test]
fn playing_a_resumed_queue_carries_on_from_where_it_was_left() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let path = tree.write("track.wav", &source.file);
    let resumed = Frames(FRAMES as u64 / 2);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player
        .request(Command::Resume(Resumption {
            rows: vec![kept(&path)],
            order: vec![0],
            row: 0,
            at: resumed,
            shuffle: false,
        }))?
        .wait_for(PATIENCE)?;
    player.request(Command::Play)?.wait_for(PATIENCE)?;

    let tail = source.stream.len() - resumed.get() as usize * frame_bytes(SampleFormat::S16);
    play_until(
        &player,
        &graph,
        BLOCK_FRAMES * frame_bytes(SampleFormat::S16),
        |_, graph| graph.played.len() >= tail,
        "the rest of the resumed track to reach the graph",
    );

    let graph = graph.lock();
    assert_eq!(
        &graph.played[..tail],
        &source.stream[source.stream.len() - tail..],
        "a resumed track handed the graph bytes from somewhere other than where it was left"
    );
    Ok(())
}

#[test]
fn a_queue_loaded_from_the_top_with_nothing_to_play_opens_no_track() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let path = tree.write("track.wav", &source.file);

    let (player, _graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player
        .request(Command::Load {
            items: vec![track(&path, 1)],
            start_at: 0,
            autoplay: false,
        })?
        .wait_for(PATIENCE)?;

    let state = player.state();
    assert_eq!(state.queue_len, 1, "the queue did not take the row");
    assert!(
        state.current.is_none(),
        "a load that was to play nothing and start nowhere opened a track anyway"
    );
    Ok(())
}

#[test]
fn a_shuffled_queue_resumed_plays_as_it_played_and_unshuffles_into_what_it_was_loaded_as()
-> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let paths: Vec<PathBuf> = (0..4)
        .map(|n| tree.write(&format!("{n}.wav"), &source.file))
        .collect();
    let played = [2_usize, 0, 3, 1];

    let (player, _graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player
        .request(Command::Resume(Resumption {
            rows: paths.iter().map(|path| kept(path)).collect(),
            order: played.to_vec(),
            row: 0,
            at: Frames::ZERO,
            shuffle: true,
        }))?
        .wait_for(PATIENCE)?;

    assert!(
        player.state().shuffle,
        "a queue resumed under shuffle came back without it"
    );
    assert_eq!(
        drawn(&player),
        played
            .iter()
            .map(|n| MediaLocation::local(&paths[*n]))
            .collect::<Vec<_>>(),
        "a resumed queue plays in an order the run that kept it never played"
    );
    assert_eq!(
        playing_row(&player),
        Some(MediaLocation::local(&paths[2])),
        "the resumed row is not the one the queue was left on"
    );

    player
        .request(Command::SetShuffle(false))?
        .wait_for(PATIENCE)?;

    assert_eq!(
        drawn(&player),
        paths.iter().map(MediaLocation::local).collect::<Vec<_>>(),
        "the order the queue was loaded in did not survive the run"
    );
    assert_eq!(
        playing_row(&player),
        Some(MediaLocation::local(&paths[2])),
        "unshuffling a resumed queue took the row it was playing away"
    );
    Ok(())
}

#[test]
fn what_the_engine_publishes_beside_the_queue_says_where_each_row_was_loaded() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let paths: Vec<PathBuf> = (0..8)
        .map(|n| tree.write(&format!("{n}.wav"), &source.file))
        .collect();

    let (player, _graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player
        .request(Command::Load {
            items: (0..8).map(|n| track(&paths[n], n as u64 + 1)).collect(),
            start_at: 0,
            autoplay: false,
        })?
        .wait_for(PATIENCE)?;

    let loaded = player.queued();
    assert_eq!(
        loaded.loaded_at.to_vec(),
        (0..8).collect::<Vec<_>>(),
        "a queue nothing has reordered draws itself out of order"
    );

    player
        .request(Command::SetShuffle(true))?
        .wait_for(PATIENCE)?;

    let shuffled = player.queued();
    assert_ne!(
        shuffled.revision, loaded.revision,
        "shuffling published the queue that was drawn before it"
    );
    assert_eq!(shuffled.rows.len(), 8);
    for (row, loaded_at) in shuffled.loaded_at.iter().copied().enumerate() {
        assert_eq!(
            shuffled.rows[row].location,
            MediaLocation::local(&paths[loaded_at]),
            "row {row} says it was loaded somewhere it was not"
        );
    }
    Ok(())
}

#[test]
fn putting_the_queue_in_order_leaves_the_playing_row_playing() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let paths: Vec<PathBuf> = (0..4)
        .map(|n| tree.write(&format!("{n}.wav"), &source.file))
        .collect();

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: (0..4).map(|n| track(&paths[n], n as u64 + 1)).collect(),
        start_at: 1,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the second track to open");
    stand_still(&player);

    player
        .request(Command::Order(vec![3, 2, 1, 0]))?
        .wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| {
            player
                .queue()
                .first()
                .is_some_and(|item| item.id.get() == 4)
        },
        "the reordered queue to publish",
    );

    assert_eq!(
        player
            .queue()
            .iter()
            .map(|item| item.id.get())
            .collect::<Vec<u64>>(),
        vec![4, 3, 2, 1],
        "the queue was not put in the order it was given"
    );
    assert_eq!(player.state().queue_position, Some(2));
    assert_eq!(player.state().current.map(|track| track.id.get()), Some(2));
    assert_eq!(graph.lock().opens, 1, "a reordering reopened the stream");

    let refused = player
        .request(Command::Order(vec![0, 0, 1, 2]))?
        .wait_for(PATIENCE);
    assert!(
        matches!(refused, Err(EngineError::NotAnOrder { named: 4, len: 4 })),
        "a run naming one row twice was taken as an order: {refused:?}"
    );

    let short = player
        .request(Command::Order(vec![0, 1]))?
        .wait_for(PATIENCE);
    assert!(
        matches!(short, Err(EngineError::NotAnOrder { named: 2, len: 4 })),
        "a run naming half the queue was taken as an order: {short:?}"
    );

    Ok(())
}

fn dozing(player: &Player) -> Option<Until> {
    player.state().sleeping.map(|asleep| asleep.until)
}

fn faulted(player: &Player) -> Vec<String> {
    player
        .events()
        .try_iter()
        .filter_map(|event| match event {
            Event::CommandFailed { command, error } => Some(format!("{command}: {error}")),
            Event::Failed { track, error } => Some(format!("track {track}: {error}")),
            _ => None,
        })
        .collect()
}

#[test]
fn a_settled_command_lands_with_its_state_published_and_a_refusal_is_still_announced() -> Result<()>
{
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let first = tree.write("one.wav", &source.file);
    let second = tree.write("two.wav", &source.file);

    let (player, _graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player
        .settle(Command::Load {
            items: vec![track(&first, 1), track(&second, 2)],
            start_at: 0,
            autoplay: true,
        })?
        .wait_for(PATIENCE)?;
    assert!(
        plays(&player, 1),
        "a settled load had not published its track"
    );

    player.settle(Command::Next)?.wait_for(PATIENCE)?;
    assert!(
        plays(&player, 2),
        "a settled skip had not published its track"
    );

    player.settle(Command::Pause)?.wait_for(PATIENCE)?;
    assert!(paused(&player), "a settled pause had not published itself");

    player
        .settle(Command::Remove(Span::one(9)))?
        .wait_for(PATIENCE)?;
    let refused = faulted(&player);
    assert_eq!(
        refused.len(),
        1,
        "a settled refusal was not announced: {refused:?}"
    );

    player
        .request(Command::Remove(Span::one(9)))?
        .wait_for(PATIENCE)
        .expect_err("a refused removal to answer its refusal");
    assert_eq!(
        faulted(&player),
        Vec::<String>::new(),
        "a refusal handed back to its asker was announced as well"
    );

    Ok(())
}

#[test]
fn a_sleep_timer_pauses_the_transport_when_its_time_is_out() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let path = tree.write("track.wav", &source.file);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the stream to open");

    player
        .request(Command::SleepUntil(Some(Until::After(A_SHORT_DOZE))))?
        .wait_for(PATIENCE)?;
    wait_for(&player, paused, "the sleep timer to pause the transport");

    assert!(
        !graph.lock().active,
        "the graph kept pulling past the timer"
    );
    assert_eq!(
        dozing(&player),
        None,
        "a timer that fired is still published"
    );
    assert!(
        plays(&player, 1),
        "the sleep timer dropped the track rather than pausing it"
    );
    assert_eq!(graph.lock().opens, 1, "the sleep timer reopened the stream");
    Ok(())
}

#[test]
fn a_sleep_timer_set_to_the_end_of_a_track_leaves_the_queue_on_the_next_row() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, 4_000);
    let first = tree.write("first.wav", &source.file);
    let second = tree.write("second.wav", &source.file);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player
        .request(Command::SleepUntil(Some(Until::EndOfTrack)))?
        .wait_for(PATIENCE)?;
    player.send(Command::Load {
        items: vec![track(&first, 1), track(&second, 2)],
        start_at: 0,
        autoplay: true,
    })?;

    play_until(
        &player,
        &graph,
        BLOCK_FRAMES * frame_bytes(SampleFormat::S16),
        |player, _| paused(player) && plays(player, 2),
        "the timer to leave the second row cued",
    );

    assert_eq!(player.state().queue_position, Some(1));
    assert_eq!(
        dozing(&player),
        None,
        "a timer that fired is still published"
    );
    let graph = graph.lock();
    assert_eq!(
        graph.played.len(),
        source.stream.len(),
        "the second track played past the sleep timer"
    );
    assert_eq!(graph.opens, 1, "the second row opened a stream anyway");
    Ok(())
}

#[test]
fn a_sleep_timer_set_to_the_end_of_a_queue_pauses_where_the_queue_wraps() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, 4_000);
    let first = tree.write("first.wav", &source.file);
    let second = tree.write("second.wav", &source.file);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player
        .request(Command::SetRepeat(RepeatMode::Queue))?
        .wait_for(PATIENCE)?;
    player
        .request(Command::SleepUntil(Some(Until::EndOfQueue)))?
        .wait_for(PATIENCE)?;
    player.send(Command::Load {
        items: vec![track(&first, 1), track(&second, 2)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the first track to open");

    play_until(
        &player,
        &graph,
        BLOCK_FRAMES * frame_bytes(SampleFormat::S16),
        |player, _| paused(player) && plays(player, 1),
        "the repeating queue to wrap onto the timer",
    );

    assert_eq!(player.state().queue_position, Some(0));
    assert_eq!(
        dozing(&player),
        None,
        "a timer that fired is still published"
    );
    assert_eq!(
        graph.lock().played.len(),
        source.stream.len() * 2,
        "the wrapped queue played a round and a half"
    );
    Ok(())
}

#[test]
fn cancelling_a_sleep_timer_leaves_the_transport_playing() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let path = tree.write("track.wav", &source.file);

    let (player, _graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;
    wait_for(&player, playing, "the stream to open");

    player
        .request(Command::SleepUntil(Some(Until::After(A_SHORT_DOZE))))?
        .wait_for(PATIENCE)?;
    player
        .request(Command::SleepUntil(None))?
        .wait_for(PATIENCE)?;
    thread::sleep(A_SHORT_DOZE * 4);

    assert!(playing(&player), "a cancelled timer paused the transport");
    assert_eq!(
        dozing(&player),
        None,
        "a cancelled timer is still published"
    );
    Ok(())
}

#[test]
fn a_sleep_timer_says_how_long_is_left_on_it() -> Result<()> {
    let (player, _graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player
        .request(Command::SleepUntil(Some(Until::After(A_LONG_DOZE))))?
        .wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| player.state().sleeping.is_some(),
        "the sleep timer to publish itself",
    );

    let asleep = player.state().sleeping.expect("a timer that is set");
    assert_eq!(asleep.until, Until::After(A_LONG_DOZE));
    let left = asleep.left.expect("a timer counting down says how long");
    assert!(left <= A_LONG_DOZE && left > Duration::ZERO, "{left:?}");

    thread::sleep(A_SHORT_DOZE * 4);
    let later = player
        .state()
        .sleeping
        .and_then(|asleep| asleep.left)
        .expect("a timer that is still set");
    assert!(later < left, "the countdown stood still at {later:?}");
    Ok(())
}

#[test]
fn a_sleep_timer_firing_against_a_stopped_transport_says_nothing() -> Result<()> {
    let (player, _graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player
        .request(Command::SleepUntil(Some(Until::After(A_SHORT_DOZE))))?
        .wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| player.state().sleeping.is_none(),
        "the sleep timer to clear itself",
    );

    assert_eq!(player.state().playback, PlaybackState::Idle);
    assert_eq!(faulted(&player), Vec::<String>::new());
    Ok(())
}

const LOOKED_AT: usize = 64;

fn heard_now(player: &Player) -> (Vec<(f32, f32)>, Caught) {
    let Tapped::Samples(tap) = player.tap() else {
        panic!("a PCM output published no tap; {}", transport(player));
    };
    let mut left = vec![f32::NAN; LOOKED_AT];
    let mut right = vec![f32::NAN; LOOKED_AT];
    let caught = tap.around(Instant::now(), &mut left, &mut right);
    (left.into_iter().zip(right).collect(), caught)
}

fn levels_of(bytes: &[u8]) -> Vec<(f32, f32)> {
    words(bytes)
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[left, right]| (f32::from(*left) / 32_768.0, f32::from(*right) / 32_768.0))
        .collect()
}

fn around_the_played(played: usize) -> Range<usize> {
    let stride = frame_bytes(SampleFormat::S16);
    (played - LOOKED_AT / 2) * stride..(played + LOOKED_AT / 2) * stride
}

#[test]
fn what_the_tap_says_is_heard_is_what_the_graph_is_playing_at_that_moment() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let path = tree.write("track.wav", &source.file);
    let stride = frame_bytes(SampleFormat::S16);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.listen_in(true);
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;
    play_until(
        &player,
        &graph,
        BLOCK_FRAMES * stride,
        |_, graph| graph.played.len() >= 8 * BLOCK_FRAMES * stride,
        "the first blocks to play",
    );
    stand_still(&player);

    let played = graph.lock().played.len() / stride;
    let (heard, caught) = heard_now(&player);

    assert_eq!(caught, Caught { tapped: LOOKED_AT });
    assert_eq!(heard, levels_of(&source.stream[around_the_played(played)]));
    Ok(())
}

#[test]
fn the_tap_reads_what_the_chain_hands_the_graph_rather_than_what_the_file_holds() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let path = tree.write("track.wav", &source.file);
    let stride = frame_bytes(SampleFormat::S16);
    let block = BLOCK_FRAMES * stride;

    let (player, graph) = settled_over(
        vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])],
        Arc::new(Sources::local()),
        EngineConfig {
            volume: Volume::new(0.5).expect("half is a volume"),
            ..config()
        },
    )?;
    player.listen_in(true);
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;
    play_until(
        &player,
        &graph,
        block,
        |_, graph| graph.played.len() >= 8 * block,
        "the first blocks to play",
    );
    stand_still(&player);
    assert_eq!(
        player.state().output.map(|output| output.mode),
        Some(OutputMode::Converted)
    );

    let played = graph.lock().played.len() / stride;
    let (heard, _) = heard_now(&player);

    player.request(Command::Play)?.wait_for(PATIENCE)?;
    let reached = around_the_played(played).end;
    play_until(
        &player,
        &graph,
        block,
        |_, graph| graph.played.len() >= reached,
        "the frames the tap was read around to reach the graph",
    );
    let handed = levels_of(&graph.lock().played[around_the_played(played)]);
    let written = levels_of(&source.stream[around_the_played(played)]);

    assert_eq!(
        heard, handed,
        "the tap and the graph disagree about the chain's output"
    );
    assert_ne!(
        heard, written,
        "the tap read the file rather than the chain"
    );
    Ok(())
}

#[test]
fn nothing_is_tapped_while_nobody_listens_in() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let path = tree.write("track.wav", &source.file);
    let stride = frame_bytes(SampleFormat::S16);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;
    play_until(
        &player,
        &graph,
        BLOCK_FRAMES * stride,
        |_, graph| graph.played.len() >= 4 * BLOCK_FRAMES * stride,
        "the first blocks to play",
    );
    stand_still(&player);

    let (heard, caught) = heard_now(&player);

    assert_eq!(caught.tapped, 0);
    assert!(heard.iter().all(|levels| *levels == (0.0, 0.0)));
    Ok(())
}

#[test]
fn a_dop_stream_publishes_markers_rather_than_a_tap() -> Result<()> {
    let named = SourceId::new("held").expect("a lowercase name");
    let sources = Sources::local().and(Arc::new(InMemory {
        source: named.clone(),
        key: "tone.dsf".to_owned(),
        bytes: dsf_tone(),
    }));
    let location = MediaLocation::new(named, "tone.dsf");

    let (player, _graph) = settled_over(
        vec![sink(&[SampleRate::HZ_176400], &[SampleFormat::S24])],
        Arc::new(sources),
        EngineConfig {
            dop: true,
            ..config()
        },
    )?;
    assert!(matches!(player.tap(), Tapped::Nothing));
    player.listen_in(true);
    player.send(Command::Load {
        items: vec![QueueItem::whole(
            TrackId::new(1).expect("a non-zero track id"),
            location,
        )],
        start_at: 0,
        autoplay: true,
    })?;

    wait_for(
        &player,
        |player| matches!(player.tap(), Tapped::Markers),
        "the DoP output to say it carries markers",
    );
    Ok(())
}

fn events_of(player: &Player) -> (Vec<u64>, Vec<u64>) {
    let mut started = Vec::new();
    let mut failed = Vec::new();
    for event in player.events().try_iter() {
        match event {
            Event::TrackStarted(id) => started.push(id.get()),
            Event::Failed { track, .. } => failed.push(track.get()),
            _ => {}
        }
    }
    (started, failed)
}

#[test]
fn a_row_whose_file_has_gone_is_passed_over_for_the_next_one_that_opens() -> Result<()> {
    let tree = Tree::new();
    let short = pcm(16, 2_000);
    let good = pcm(16, 2_000);
    let first = tree.write("short.wav", &short.file);
    let missing = tree.root.join("missing.wav");
    let last = tree.write("good.wav", &good.file);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player.send(Command::Load {
        items: vec![track(&first, 1), track(&missing, 2), track(&last, 3)],
        start_at: 0,
        autoplay: true,
    })?;

    play_until(
        &player,
        &graph,
        BLOCK_FRAMES * frame_bytes(SampleFormat::S16),
        |player, _| player.state().playback == PlaybackState::Stopped,
        "the queue to play past the row that will not open",
    );

    let (started, failed) = events_of(&player);
    assert_eq!(
        started,
        [1, 3],
        "the row after the missing one never opened"
    );
    assert_eq!(
        failed,
        [2],
        "the missing row was not reported by its own id"
    );
    Ok(())
}

#[test]
fn next_and_play_on_a_row_that_will_not_open_move_on_to_one_that_does() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let missing = tree.root.join("missing.wav");
    let second = tree.write("second.wav", &source.file);
    let third = tree.write("third.wav", &source.file);

    let (player, _graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player
        .request(Command::Load {
            items: vec![track(&missing, 1), track(&second, 2), track(&third, 3)],
            start_at: 0,
            autoplay: false,
        })?
        .wait_for(PATIENCE)?;
    player.request(Command::Play)?.wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| playing(player) && plays(player, 2),
        "play on a row that will not open to move on to the next",
    );

    player.request(Command::JumpTo(0))?.wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| playing(player) && plays(player, 2),
        "a jump onto a row that will not open to move on to the next",
    );

    player
        .request(Command::Load {
            items: vec![track(&missing, 1), track(&second, 2), track(&third, 3)],
            start_at: 0,
            autoplay: false,
        })?
        .wait_for(PATIENCE)?;
    player.request(Command::Next)?.wait_for(PATIENCE)?;
    wait_for(
        &player,
        |player| plays(player, 2),
        "next on a row that never opened to reach the following one",
    );
    Ok(())
}

#[test]
fn a_play_after_a_sink_appears_binds_the_track_an_autoplay_found_no_sink_for() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let path = tree.write("track.wav", &source.file);

    let (player, graph) = player(Vec::new())?;
    let loaded = player
        .request(Command::Load {
            items: vec![track(&path, 1)],
            start_at: 0,
            autoplay: true,
        })?
        .wait_for(PATIENCE);
    assert!(
        loaded.is_err(),
        "a load with no sink to play through answered Ok"
    );

    announce(&graph, sink(&[SampleRate::HZ_44100], &[SampleFormat::S16]));
    wait_for(
        &player,
        |player| player.sinks().len() == 1,
        "the device that appeared to reach the picker",
    );

    player.request(Command::Pause)?.wait_for(PATIENCE)?;
    player.request(Command::Play)?.wait_for(PATIENCE)?;
    wait_for(
        &player,
        playing,
        "play to bind the track once a sink was there",
    );
    assert_eq!(graph.lock().opens, 1);
    Ok(())
}

#[test]
fn a_resumption_past_the_end_of_a_file_since_cut_shorter_opens_at_its_start() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, FRAMES);
    let path = tree.write("track.wav", &source.file);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_44100], &[SampleFormat::S16])])?;
    player
        .request(Command::Resume(Resumption {
            rows: vec![kept(&path)],
            order: vec![0],
            row: 0,
            at: Frames(FRAMES as u64 * 4),
            shuffle: false,
        }))?
        .wait_for(PATIENCE)?;
    wait_for(&player, paused, "the resumed row to open paused");
    assert_eq!(
        player.state().current.map(|track| track.position),
        Some(Frames::ZERO)
    );

    player.request(Command::Play)?.wait_for(PATIENCE)?;
    play_until(
        &player,
        &graph,
        BLOCK_FRAMES * frame_bytes(SampleFormat::S16),
        |_, graph| graph.played.len() >= source.stream.len(),
        "the whole of the shortened file to reach the graph",
    );
    Ok(())
}

#[test]
fn a_block_the_resampler_writes_wider_than_the_buffer_still_primes_the_stream() -> Result<()> {
    const RATE_AT: Range<usize> = 24..28;
    const BYTES_A_SECOND_AT: Range<usize> = 28..32;
    const LOW_RATE: u32 = 8_000;

    let tree = Tree::new();
    let mut file = pcm(16, 8_000).file;
    file[RATE_AT].copy_from_slice(&LOW_RATE.to_le_bytes());
    file[BYTES_A_SECOND_AT].copy_from_slice(&(LOW_RATE * 4).to_le_bytes());
    let path = tree.write("telephone.wav", &file);

    let (player, graph) = player(vec![sink(&[SampleRate::HZ_192000], &[SampleFormat::S32])])?;
    player.send(Command::Load {
        items: vec![track(&path, 1)],
        start_at: 0,
        autoplay: true,
    })?;
    play_until(
        &player,
        &graph,
        BLOCK_FRAMES * frame_bytes(SampleFormat::S32),
        |player, _| playing(player),
        "an 8 kHz file upsampled onto a 192 kHz sink to prime and play",
    );
    Ok(())
}

struct Measured(f32);

impl Hinting for Measured {
    fn hints(&self, _: &MediaLocation, _: Option<FrameSpan>) -> TrackHints {
        TrackHints {
            true_peak: Gain::new(self.0).ok(),
            ..TrackHints::default()
        }
    }
}

struct Loud(f32);

impl Hinting for Loud {
    fn hints(&self, _: &MediaLocation, _: Option<FrameSpan>) -> TrackHints {
        TrackHints {
            measured: MeasuredGain {
                track: Decibels::new(self.0).ok(),
                album: None,
            },
            ..TrackHints::default()
        }
    }
}

#[test]
fn a_track_with_no_replay_gain_tags_is_levelled_by_what_the_catalog_measured() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, 4 * FRAMES);
    let path = tree.write("untagged.wav", &source.file);
    let rate = SampleRate::HZ_44100;
    let block = BLOCK_FRAMES * frame_bytes(SampleFormat::S16);
    let sample = usize::from(SampleFormat::S16.bytes_per_sample().get());
    let measured = Arc::new(Sources::local().hinted_by(Arc::new(Loud(-6.0206))));

    for (replay_gain, wanted_level) in [(ReplayGainMode::Track, 0.5), (ReplayGainMode::Off, 1.0)] {
        let (player, graph) = settled_over(
            vec![sink(&[rate], &[SampleFormat::S16])],
            Arc::clone(&measured),
            EngineConfig {
                replay_gain,
                ..ring_deep()
            },
        )?;
        player.send(Command::Load {
            items: vec![track(&path, 1)],
            start_at: 0,
            autoplay: true,
        })?;
        play_until(
            &player,
            &graph,
            block,
            |_, graph| graph.played.len() >= 16 * block,
            "the first blocks to play",
        );

        let graph = graph.lock();
        let level = mean_level(
            &words(&graph.played),
            &words(&source.stream),
            4 * block / sample..12 * block / sample,
        );
        assert!(
            (level - wanted_level).abs() < 0.01,
            "under {replay_gain:?} the untagged track played at {level}"
        );
    }
    Ok(())
}

#[test]
fn a_track_the_catalog_measured_past_full_scale_is_turned_down_under_it() -> Result<()> {
    let tree = Tree::new();
    let source = pcm(16, 4 * FRAMES);
    let path = tree.write("track.wav", &source.file);
    let rate = SampleRate::HZ_44100;
    let block = BLOCK_FRAMES * frame_bytes(SampleFormat::S16);
    let sample = usize::from(SampleFormat::S16.bytes_per_sample().get());
    let measured = Arc::new(Sources::local().hinted_by(Arc::new(Measured(2.0))));

    for (heeded, wanted_mode, wanted_level) in [
        (true, OutputMode::Converted, 0.5),
        (false, OutputMode::BitPerfect, 1.0),
    ] {
        let (player, graph) = settled_over(
            vec![sink(&[rate], &[SampleFormat::S16])],
            Arc::clone(&measured),
            EngineConfig {
                true_peak: heeded,
                ..ring_deep()
            },
        )?;
        player.send(Command::Load {
            items: vec![track(&path, 1)],
            start_at: 0,
            autoplay: true,
        })?;

        let mut mode = None;
        play_until(
            &player,
            &graph,
            block,
            |player, graph| {
                mode = player.state().output.map(|output| output.mode);
                graph.played.len() >= 16 * block
            },
            "the first blocks to play",
        );
        assert_eq!(mode, Some(wanted_mode), "heeding the true peak: {heeded}");

        let graph = graph.lock();
        let level = mean_level(
            &words(&graph.played),
            &words(&source.stream),
            4 * block / sample..12 * block / sample,
        );
        assert!(
            (level - wanted_level).abs() < 0.01,
            "heeding the true peak: {heeded}, the track played at {level}"
        );
    }
    Ok(())
}
