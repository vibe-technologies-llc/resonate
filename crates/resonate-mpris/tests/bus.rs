use std::{
    collections::HashMap,
    env, fs,
    io::Cursor,
    path::{Path, PathBuf},
    process,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant, UNIX_EPOCH},
};

use crossbeam_channel::{Receiver, Sender, unbounded};
use parking_lot::Mutex;
use resonate_codec::{Error as CodecError, Result as CodecResult};
use resonate_core::{
    ChannelLayout, FrameSpan, Frames, MediaLocation, PlaylistId, SampleFormat, SampleRate,
    SourceId, TrackId, Volume,
};
use resonate_engine::{
    AudioSource, Backend, Command, EngineConfig, Media, MediaProvider, NodeName, Placement, Player,
    QueueItem, Reading, RepeatMode, SinkChange, SinkFormats, SinkId, SinkInfo, SinkResult,
    SinkStream, Sources, StreamCommand, StreamRequest, Until,
};
use resonate_mpris::{
    Heard, Host, Mpris, Opened, PlaybackStatus, PlayerName, PlaylistInfo, PlaylistOrder, Playlists,
    Queueing, Running, Seeking,
};
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::{OwnedObjectPath, OwnedValue},
};

const RATE: u32 = 44_100;
const CHANNELS: u16 = 2;
const SECONDS: usize = 30;
const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT: &str = "org.mpris.MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const TRACK_LIST: &str = "org.mpris.MediaPlayer2.TrackList";
const PLAYLISTS: &str = "org.mpris.MediaPlayer2.Playlists";
const RESONATE: &str = "org.resonate.Player1";
const SET_SLEEP: &str = "SetSleep";
const SLEEP: &str = "Sleep";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const NO_PLAYLIST: &str = "/org/resonate/playlist/none";
const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";
const BUS_NAME_PREFIX: &str = "org.mpris.MediaPlayer2.resonate.";
const UTF8: u8 = 3;
const FRONT_COVER: u8 = 3;
const PATIENCE: Duration = Duration::from_secs(15);
const SETTLE: Duration = Duration::from_millis(500);

struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = env::temp_dir().join(format!(
            "resonate-mpris-{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("a writable temporary directory");
        Self { root }
    }

    fn wav(&self, name: &str) -> PathBuf {
        let path = self.root.join(name);
        fs::write(&path, wav()).expect("a writable temporary file");
        path
    }

    fn untitled(&self, name: &str) -> PathBuf {
        let path = self.root.join(name);
        fs::write(&path, wav_naming(&[])).expect("a writable temporary file");
        path
    }

    fn pictured(&self, name: &str) -> PathBuf {
        let path = self.root.join(name);
        fs::write(&path, [id3(&png()), wav()].concat()).expect("a writable temporary file");
        path
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn wav() -> Vec<u8> {
    wav_naming(&[
        (b"INAM", "Echoes"),
        (b"IART", "Pink Floyd"),
        (b"IPRD", "Meddle"),
    ])
}

fn wav_naming(names: &[(&[u8; 4], &str)]) -> Vec<u8> {
    let frames = RATE as usize * SECONDS;
    let mut data = Vec::with_capacity(frames * usize::from(CHANNELS) * 2);
    for n in 0..frames * usize::from(CHANNELS) {
        data.extend_from_slice(&((n % 30_000) as i16).to_le_bytes());
    }

    let align = CHANNELS * 2;
    let mut fmt = Vec::new();
    fmt.extend_from_slice(&1_u16.to_le_bytes());
    fmt.extend_from_slice(&CHANNELS.to_le_bytes());
    fmt.extend_from_slice(&RATE.to_le_bytes());
    fmt.extend_from_slice(&(RATE * u32::from(align)).to_le_bytes());
    fmt.extend_from_slice(&align.to_le_bytes());
    fmt.extend_from_slice(&16_u16.to_le_bytes());

    let mut info = b"INFO".to_vec();
    for (id, value) in names {
        chunk(&mut info, id, &[value.as_bytes(), b"\0"].concat());
    }

    let mut body = b"WAVE".to_vec();
    chunk(&mut body, b"fmt ", &fmt);
    if !names.is_empty() {
        chunk(&mut body, b"LIST", &info);
    }
    chunk(&mut body, b"data", &data);

    let mut file = b"RIFF".to_vec();
    file.extend_from_slice(&(body.len() as u32).to_le_bytes());
    file.extend_from_slice(&body);
    file
}

fn png() -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend((0..512).map(|n| n as u8));
    bytes
}

fn id3(picture: &[u8]) -> Vec<u8> {
    let mut body = vec![UTF8];
    body.extend_from_slice(b"image/png\0");
    body.push(FRONT_COVER);
    body.push(0);
    body.extend_from_slice(picture);

    let mut frames = b"APIC".to_vec();
    frames.extend_from_slice(&synchsafe(body.len() as u32));
    frames.extend_from_slice(&[0, 0]);
    frames.extend_from_slice(&body);

    let mut tag = b"ID3\x04\x00\x00".to_vec();
    tag.extend_from_slice(&synchsafe(frames.len() as u32));
    tag.extend_from_slice(&frames);
    tag
}

fn synchsafe(value: u32) -> [u8; 4] {
    [
        ((value >> 21) & 0x7f) as u8,
        ((value >> 14) & 0x7f) as u8,
        ((value >> 7) & 0x7f) as u8,
        (value & 0x7f) as u8,
    ]
}

fn chunk(into: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
    into.extend_from_slice(id);
    into.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    into.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        into.push(0);
    }
}

struct Pulling {
    active: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
}

struct RealtimeSink {
    sinks: Vec<SinkInfo>,
    changes: Receiver<SinkChange>,
    open: Arc<Mutex<Vec<Pulling>>>,
}

impl RealtimeSink {
    fn new() -> Self {
        let (announce, changes) = unbounded();
        drop(announce);
        Self {
            sinks: vec![SinkInfo {
                id: SinkId::new(1),
                name: NodeName::new("alsa_output.fake"),
                description: "Fake DAC".to_owned(),
                is_default: true,
                is_hardware: true,
                port: None,
                profile: None,
                formats: vec![SinkFormats {
                    format: SampleFormat::S16,
                    rates: vec![SampleRate::HZ_44100],
                    channels: vec![ChannelLayout::Stereo],
                }],
                allowed_rates: vec![SampleRate::HZ_44100],
                current_rate: Some(SampleRate::HZ_44100),
            }],
            changes,
            open: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl Backend for RealtimeSink {
    fn subscribe_sinks(&self) -> Receiver<SinkChange> {
        self.changes.clone()
    }

    fn enumerate_sinks(&self, _timeout: Duration) -> SinkResult<Vec<SinkInfo>> {
        Ok(self.sinks.clone())
    }

    fn open(
        &self,
        request: &StreamRequest,
        mut source: Box<dyn AudioSource>,
    ) -> SinkResult<SinkStream> {
        let quantum = 1_024;
        let stride = request.spec.bytes_per_frame().get() as usize;
        let period =
            Duration::from_secs_f64(f64::from(quantum) / f64::from(request.spec.rate.hz()));

        let active = Arc::new(AtomicBool::new(false));
        let closed = Arc::new(AtomicBool::new(false));
        self.open.lock().push(Pulling {
            active: Arc::clone(&active),
            closed: Arc::clone(&closed),
        });

        {
            let active = Arc::clone(&active);
            let closed = Arc::clone(&closed);
            thread::Builder::new()
                .name("fake-graph".to_owned())
                .spawn(move || {
                    let mut block = vec![0_u8; quantum as usize * stride];
                    while !closed.load(Ordering::Acquire) {
                        thread::sleep(period);
                        if active.load(Ordering::Acquire) {
                            source.fill(&mut block);
                        }
                    }
                })
                .map(drop)
                .unwrap_or_default();
        }

        let setter = Arc::clone(&active);
        let closer = Arc::clone(&closed);
        Ok(SinkStream::new(
            unbounded().1,
            Arc::new(AtomicU64::new(0)),
            Box::new(move |command| {
                match command {
                    StreamCommand::SetActive(playing) => setter.store(playing, Ordering::Release),
                    StreamCommand::Drain => {}
                    StreamCommand::Close => closer.store(true, Ordering::Release),
                }
                Ok(())
            }),
        ))
    }

    fn shutdown(self: Box<Self>) -> SinkResult<()> {
        for stream in self.open.lock().iter() {
            stream.active.store(false, Ordering::Release);
            stream.closed.store(true, Ordering::Release);
        }
        Ok(())
    }
}

type Catalog = Arc<Mutex<Vec<(MediaLocation, Option<FrameSpan>, Heard)>>>;

struct Desktop {
    quit: Arc<AtomicBool>,
    raised: Arc<AtomicBool>,
    opened: Arc<Mutex<Vec<MediaLocation>>>,
    catalog: Catalog,
}

impl Host for Desktop {
    fn identity(&self) -> String {
        "Resonate".to_owned()
    }

    fn desktop_entry(&self) -> Option<String> {
        Some("resonate".to_owned())
    }

    fn can_quit(&self) -> bool {
        true
    }

    fn quit(&self) {
        self.quit.store(true, Ordering::Release);
    }

    fn can_raise(&self) -> bool {
        true
    }

    fn raise(&self) {
        self.raised.store(true, Ordering::Release);
    }

    fn mime_types(&self) -> Vec<String> {
        vec!["audio/flac".to_owned()]
    }

    fn open(&self, location: &MediaLocation, _span: Option<FrameSpan>) -> Opened {
        self.opened.lock().push(location.clone());
        Opened::Accepted
    }

    fn heard(&self, location: &MediaLocation, span: Option<FrameSpan>) -> Option<Heard> {
        self.catalog
            .lock()
            .iter()
            .find(|(held, cut, _)| held == location && *cut == span)
            .map(|(_, _, heard)| *heard)
    }
}

struct Shelf {
    rows: Mutex<Vec<PlaylistInfo>>,
    playing: Mutex<Option<PlaylistInfo>>,
    activated: Mutex<Vec<PlaylistId>>,
    revision: AtomicU64,
}

impl Shelf {
    fn new() -> Self {
        Self {
            rows: Mutex::new(
                [(1, "Evening"), (2, "Anything Loud")]
                    .into_iter()
                    .map(|(id, name)| PlaylistInfo {
                        id: PlaylistId::new(id).expect("playlist ids start at one"),
                        name: name.to_owned(),
                    })
                    .collect(),
            ),
            playing: Mutex::new(None),
            activated: Mutex::new(Vec::new()),
            revision: AtomicU64::new(0),
        }
    }
}

impl Playlists for Shelf {
    fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    fn count(&self) -> usize {
        self.rows.lock().len()
    }

    fn listing(
        &self,
        order: PlaylistOrder,
        reverse: bool,
        from: usize,
        most: Option<usize>,
    ) -> Vec<PlaylistInfo> {
        let mut rows = self.rows.lock().clone();
        if order == PlaylistOrder::Alphabetical {
            rows.sort_by(|left, right| left.name.cmp(&right.name));
        }
        if reverse {
            rows.reverse();
        }

        rows.into_iter()
            .skip(from)
            .take(most.unwrap_or(usize::MAX))
            .collect()
    }

    fn playing(&self) -> Option<PlaylistInfo> {
        self.playing.lock().clone()
    }

    fn activate(&self, playlist: PlaylistId) -> Opened {
        let Some(found) = self
            .rows
            .lock()
            .iter()
            .find(|row| row.id == playlist)
            .cloned()
        else {
            return Opened::Refused;
        };
        self.activated.lock().push(playlist);
        *self.playing.lock() = Some(found);
        Opened::Accepted
    }
}

struct Harness {
    player: Arc<Player>,
    connection: Connection,
    destination: String,
    quit: Arc<AtomicBool>,
    raised: Arc<AtomicBool>,
    opened: Arc<Mutex<Vec<MediaLocation>>>,
    catalog: Catalog,
    shelf: Arc<Shelf>,
    mpris: Option<Mpris>,
}

impl Harness {
    fn start() -> Option<Self> {
        Self::started(Arc::new(Sources::local()))
    }

    fn started(sources: Arc<Sources>) -> Option<Self> {
        let connection = match Connection::session() {
            Ok(connection) => connection,
            Err(error) => {
                eprintln!("skipped: no session bus to talk to ({error})");
                return None;
            }
        };

        let config = EngineConfig {
            buffer: Duration::from_millis(200),
            ..EngineConfig::default()
        };
        let player = Arc::new(
            Player::with_sources_and_backend(config, sources, |_| {
                Ok(Box::new(RealtimeSink::new()))
            })
            .expect("the engine starts behind a fake sink"),
        );

        let quit = Arc::new(AtomicBool::new(false));
        let raised = Arc::new(AtomicBool::new(false));
        let opened = Arc::new(Mutex::new(Vec::new()));
        let catalog: Catalog = Arc::new(Mutex::new(Vec::new()));
        let host = Desktop {
            quit: Arc::clone(&quit),
            raised: Arc::clone(&raised),
            opened: Arc::clone(&opened),
            catalog: Arc::clone(&catalog),
        };
        let shelf = Arc::new(Shelf::new());
        let mpris = Mpris::start(
            Arc::clone(&player),
            Arc::new(host),
            Some(Arc::clone(&shelf) as Arc<dyn Playlists>),
        )
        .expect("the MPRIS service reaches the session bus");

        Some(Self {
            destination: mpris.name().to_string(),
            player,
            connection,
            quit,
            raised,
            opened,
            catalog,
            shelf,
            mpris: Some(mpris),
        })
    }

    fn remember(&self, path: &Path, span: Option<FrameSpan>, heard: Heard) {
        self.catalog
            .lock()
            .push((MediaLocation::local(path), span, heard));
    }

    fn load_cut(&self, path: &Path, spans: &[FrameSpan]) {
        let items = spans
            .iter()
            .enumerate()
            .map(|(index, span)| QueueItem {
                id: TrackId::new(index as u64 + 1).expect("a non-zero track id"),
                location: MediaLocation::local(path),
                span: Some(*span),
            })
            .collect();
        self.player
            .send(Command::Load {
                items,
                start_at: 0,
                autoplay: false,
            })
            .expect("the engine takes a queue");
    }

    fn proxy(&self, interface: &'static str) -> Proxy<'_> {
        Proxy::new(
            &self.connection,
            self.destination.clone(),
            OBJECT_PATH,
            interface,
        )
        .expect("the served object answers")
    }

    fn load(&self, path: &Path) {
        self.player
            .send(Command::Load {
                items: vec![QueueItem {
                    id: TrackId::new(1).expect("a non-zero track id"),
                    location: MediaLocation::local(path),
                    span: None,
                }],
                start_at: 0,
                autoplay: true,
            })
            .expect("the engine takes a queue");
    }

    fn load_all(&self, paths: &[PathBuf]) {
        let items = paths
            .iter()
            .enumerate()
            .map(|(index, path)| QueueItem {
                id: TrackId::new(index as u64 + 1).expect("a non-zero track id"),
                location: MediaLocation::local(path),
                span: None,
            })
            .collect();
        self.player
            .send(Command::Load {
                items,
                start_at: 0,
                autoplay: true,
            })
            .expect("the engine takes a queue");
    }

    fn load_alike(&self, path: &Path, rows: usize) {
        let id = TrackId::new(1).expect("a non-zero track id");
        let items = (0..rows)
            .map(|_| QueueItem {
                id,
                location: MediaLocation::local(path),
                span: None,
            })
            .collect();
        self.player
            .send(Command::Load {
                items,
                start_at: 0,
                autoplay: true,
            })
            .expect("the engine takes a queue");
    }

    fn player_signals(&self) -> Receiver<String> {
        self.signals(PLAYER)
    }

    fn playlist_signals(&self) -> Receiver<String> {
        self.signals(PLAYLISTS)
    }

    fn tracklist_signals(&self) -> Receiver<String> {
        self.signals(TRACK_LIST)
    }

    fn changed(&self, property: &'static str) -> Receiver<OwnedValue> {
        let signals = self
            .proxy(PROPERTIES)
            .receive_signal("PropertiesChanged")
            .expect("the served object announces its property changes");

        let (announced, heard) = unbounded();
        thread::spawn(move || {
            for message in signals {
                let Ok((_, mut changed, _)) =
                    message
                        .body()
                        .deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
                else {
                    continue;
                };
                let Some(value) = changed.remove(property) else {
                    continue;
                };
                if announced.send(value).is_err() {
                    return;
                }
            }
        });
        heard
    }

    fn invalidated(&self, property: &'static str) -> Receiver<()> {
        let signals = self
            .proxy(PROPERTIES)
            .receive_signal("PropertiesChanged")
            .expect("the served object announces its property changes");

        let (announced, heard) = unbounded();
        thread::spawn(move || {
            for message in signals {
                let Ok((_, _, invalidated)) =
                    message
                        .body()
                        .deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>()
                else {
                    continue;
                };
                if invalidated.iter().any(|named| named == property) && announced.send(()).is_err()
                {
                    return;
                }
            }
        });
        heard
    }

    fn signals(&self, interface: &'static str) -> Receiver<String> {
        let signals = self
            .proxy(interface)
            .receive_all_signals()
            .expect("the served object announces its changes");

        let (announced, heard) = unbounded();
        thread::spawn(move || {
            for message in signals {
                let name = message
                    .header()
                    .member()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                if announced.send(name).is_err() {
                    return;
                }
            }
        });
        heard
    }

    fn tracks(&self) -> Vec<OwnedObjectPath> {
        self.proxy(TRACK_LIST)
            .get_property::<Vec<OwnedObjectPath>>("Tracks")
            .expect("Tracks is readable")
    }

    fn wait_for(&self, mut ready: impl FnMut(&Self) -> bool, what: &str) {
        let deadline = Instant::now() + PATIENCE;
        while Instant::now() < deadline {
            if ready(self) {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        panic!("timed out waiting for {what}; {}", self.transport());
    }

    fn transport(&self) -> String {
        let state = self.player.state();
        format!(
            "playback {:?}, track {:?} at row {:?} of {}, {} rows published",
            state.playback,
            state.current.map(|track| track.id.get()),
            state.queue_position,
            state.queue_len,
            self.player.queue().len(),
        )
    }

    fn status(&self) -> String {
        self.proxy(PLAYER)
            .get_property::<String>("PlaybackStatus")
            .expect("PlaybackStatus is readable")
    }

    fn metadata(&self) -> HashMap<String, OwnedValue> {
        self.proxy(PLAYER)
            .get_property::<HashMap<String, OwnedValue>>("Metadata")
            .expect("Metadata is readable")
    }

    fn position(&self) -> i64 {
        self.proxy(PLAYER)
            .get_property::<i64>("Position")
            .expect("Position is readable")
    }

    fn playing_track(&self) -> OwnedObjectPath {
        path_of(&self.metadata()).expect("a track id")
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if let Some(mpris) = self.mpris.take() {
            mpris.shutdown();
        }
    }
}

fn announced(signals: &Receiver<String>, wanted: &str) {
    let deadline = Instant::now() + PATIENCE;
    while let Ok(name) = signals.recv_deadline(deadline) {
        if name == wanted {
            return;
        }
    }
    panic!("no {wanted} reached the bus");
}

fn unannounced(signals: &Receiver<String>, unwanted: &str, within: Duration) {
    let deadline = Instant::now() + within;
    while let Ok(name) = signals.recv_deadline(deadline) {
        assert_ne!(name, unwanted, "{unwanted} reached the bus");
    }
}

fn path_of(fields: &HashMap<String, OwnedValue>) -> Option<OwnedObjectPath> {
    OwnedObjectPath::try_from(fields.get("mpris:trackid")?.try_clone().ok()?).ok()
}

fn text(fields: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    String::try_from(fields.get(key)?.try_clone().ok()?).ok()
}

fn strings(fields: &HashMap<String, OwnedValue>, key: &str) -> Option<Vec<String>> {
    Vec::<String>::try_from(fields.get(key)?.try_clone().ok()?).ok()
}

fn count(fields: &HashMap<String, OwnedValue>, key: &str) -> Option<i32> {
    i32::try_from(fields.get(key)?.try_clone().ok()?).ok()
}

#[test]
fn the_root_interface_names_the_player_the_way_a_desktop_expects() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let root = harness.proxy(ROOT);

    assert_eq!(
        root.get_property::<String>("Identity").expect("Identity"),
        "Resonate"
    );
    assert_eq!(
        root.get_property::<String>("DesktopEntry")
            .expect("DesktopEntry"),
        "resonate"
    );
    assert!(root.get_property::<bool>("CanQuit").expect("CanQuit"));
    assert!(root.get_property::<bool>("CanRaise").expect("CanRaise"));
    assert!(
        root.get_property::<bool>("HasTrackList")
            .expect("HasTrackList")
    );
    assert_eq!(
        root.get_property::<Vec<String>>("SupportedUriSchemes")
            .expect("SupportedUriSchemes"),
        ["file"]
    );

    root.call::<_, _, ()>("Quit", &()).expect("Quit is served");
    assert!(harness.quit.load(Ordering::Acquire));
}

#[test]
fn a_playing_track_is_described_by_its_metadata_and_its_position() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let tree = Tree::new();
    harness.load(&tree.wav("echoes.wav"));

    harness.wait_for(|harness| harness.status() == "Playing", "playback to start");
    harness.wait_for(
        |harness| text(&harness.metadata(), "xesam:title").is_some(),
        "the tags to reach the bus",
    );

    let fields = harness.metadata();
    assert_eq!(text(&fields, "xesam:title").as_deref(), Some("Echoes"));
    assert_eq!(
        strings(&fields, "xesam:artist").as_deref(),
        Some(["Pink Floyd".to_owned()].as_slice())
    );
    assert_eq!(text(&fields, "xesam:album").as_deref(), Some("Meddle"));
    assert!(
        text(&fields, "xesam:url").is_some_and(|url| url.starts_with("file:///")),
        "no file URI reached the bus"
    );
    assert!(
        OwnedObjectPath::try_from(
            fields
                .get("mpris:trackid")
                .expect("a track id")
                .try_clone()
                .expect("a cloneable value")
        )
        .is_ok(),
        "mpris:trackid was not an object path"
    );

    let player = harness.proxy(PLAYER);
    assert!(player.get_property::<bool>("CanSeek").expect("CanSeek"));
    assert!(player.get_property::<bool>("CanPause").expect("CanPause"));
    assert!(player.get_property::<bool>("CanPlay").expect("CanPlay"));
    assert!(
        !player.get_property::<bool>("CanGoNext").expect("CanGoNext"),
        "a one-track queue offered a next track"
    );

    let first = harness.position();
    harness.wait_for(|harness| harness.position() > first, "the position to move");
}

#[test]
fn a_playing_track_whose_file_names_no_title_is_titled_by_its_file() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let tree = Tree::new();
    harness.load(&tree.untitled("Shine On.wav"));

    harness.wait_for(|harness| harness.status() == "Playing", "playback to start");
    harness.wait_for(
        |harness| text(&harness.metadata(), "xesam:url").is_some(),
        "the stream to reach the bus",
    );

    assert_eq!(
        text(&harness.metadata(), "xesam:title").as_deref(),
        Some("Shine On")
    );
}

#[test]
fn the_transport_methods_a_media_key_sends_reach_the_engine() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let tree = Tree::new();
    harness.load(&tree.wav("echoes.wav"));
    harness.wait_for(|harness| harness.status() == "Playing", "playback to start");

    let player = harness.proxy(PLAYER);
    player
        .call::<_, _, ()>("PlayPause", &())
        .expect("PlayPause is served");
    harness.wait_for(|harness| harness.status() == "Paused", "the pause to land");

    player
        .call::<_, _, ()>("Play", &())
        .expect("Play is served");
    harness.wait_for(
        |harness| harness.status() == "Playing",
        "the resume to land",
    );

    player
        .call::<_, _, ()>("Stop", &())
        .expect("Stop is served");
    harness.wait_for(|harness| harness.status() == "Stopped", "the stop to land");
}

#[test]
fn the_writable_properties_are_pushed_back_into_the_engine() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let tree = Tree::new();
    harness.load(&tree.wav("echoes.wav"));
    harness.wait_for(|harness| harness.status() == "Playing", "playback to start");

    let player = harness.proxy(PLAYER);
    player
        .set_property("LoopStatus", "Track")
        .expect("LoopStatus is writable");
    harness.wait_for(
        |harness| harness.player.state().repeat == RepeatMode::Track,
        "the loop status to reach the engine",
    );
    assert_eq!(
        player
            .get_property::<String>("LoopStatus")
            .expect("LoopStatus"),
        "Track"
    );

    player
        .set_property("Shuffle", true)
        .expect("Shuffle is writable");
    harness.wait_for(
        |harness| harness.player.state().shuffle,
        "the shuffle flag to reach the engine",
    );

    player
        .set_property("Volume", 0.5_f64)
        .expect("Volume is writable");
    harness.wait_for(
        |harness| (harness.player.state().volume.get() - 0.5).abs() < 1e-3,
        "the volume to reach the engine",
    );

    assert!(
        player.set_property("Rate", 2.0_f64).is_err(),
        "a rate other than 1.0 was accepted"
    );

    player
        .set_property("Rate", 0.0_f64)
        .expect("a rate of nothing is a pause, not a refusal");
    harness.wait_for(
        |harness| harness.status() == "Paused",
        "a rate of nothing to pause the transport",
    );
    assert_eq!(player.get_property::<f64>("Rate").expect("Rate"), 1.0);
}

#[test]
fn a_property_that_was_set_announces_the_value_it_was_set_to() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let announced = harness.changed("Shuffle");
    let player = harness.proxy(PLAYER);

    player
        .set_property("Shuffle", true)
        .expect("Shuffle is writable");

    let told = announced
        .recv_timeout(PATIENCE)
        .expect("a Shuffle change reaches the bus");
    assert!(
        bool::try_from(told).expect("Shuffle is announced as a boolean"),
        "the change announced the shuffle the player was set away from"
    );
}

#[test]
fn set_position_seeks_the_track_it_names_and_ignores_any_other() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let tree = Tree::new();
    harness.load(&tree.wav("echoes.wav"));
    harness.wait_for(|harness| harness.status() == "Playing", "playback to start");

    let player = harness.proxy(PLAYER);
    let track = harness.playing_track();
    let stale = OwnedObjectPath::try_from("/org/resonate/track/99").expect("a valid path");
    player
        .call::<_, _, ()>("SetPosition", &(&stale, 20_000_000_i64))
        .expect("SetPosition is served");
    thread::sleep(Duration::from_millis(300));
    assert!(
        harness.position() < 10_000_000,
        "a stale track id moved the position"
    );

    player
        .call::<_, _, ()>("SetPosition", &(&track, 20_000_000_i64))
        .expect("SetPosition is served");
    harness.wait_for(
        |harness| harness.position() >= 19_000_000,
        "the seek to land",
    );
}

#[test]
fn the_picture_a_track_carries_is_named_by_a_uri_the_desktop_can_open() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let tree = Tree::new();
    harness.load(&tree.pictured("echoes.wav"));
    harness.wait_for(|harness| harness.status() == "Playing", "playback to start");
    harness.wait_for(
        |harness| text(&harness.metadata(), "mpris:artUrl").is_some(),
        "the cover to reach the bus",
    );

    let named = text(&harness.metadata(), "mpris:artUrl").expect("a cover URI");
    let laid = MediaLocation::from_uri(&named).expect("a local location");
    let path = laid.as_path().expect("a path the desktop can open");

    assert_eq!(fs::read(path).expect("the cover reads back"), png());
}

#[test]
fn a_track_carrying_no_picture_names_none_rather_than_an_empty_one() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let tree = Tree::new();
    harness.load(&tree.wav("bare.wav"));
    harness.wait_for(|harness| harness.status() == "Playing", "playback to start");
    harness.wait_for(
        |harness| text(&harness.metadata(), "xesam:title").is_some(),
        "the tags to reach the bus",
    );

    thread::sleep(SETTLE);
    assert!(text(&harness.metadata(), "mpris:artUrl").is_none());
}

#[test]
fn set_position_before_the_start_of_a_track_is_ignored_rather_than_seeking_to_it() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let tree = Tree::new();
    harness.load(&tree.wav("echoes.wav"));
    harness.wait_for(|harness| harness.status() == "Playing", "playback to start");

    let player = harness.proxy(PLAYER);
    let track = harness.playing_track();
    player
        .call::<_, _, ()>("SetPosition", &(&track, 20_000_000_i64))
        .expect("SetPosition is served");
    harness.wait_for(
        |harness| harness.position() >= 19_000_000,
        "the seek to land",
    );

    player
        .call::<_, _, ()>("SetPosition", &(&track, -5_000_000_i64))
        .expect("SetPosition is served");
    thread::sleep(SETTLE);
    assert!(
        harness.position() >= 19_000_000,
        "a position before the start of the track was seeked to"
    );
}

#[test]
fn a_seek_further_back_than_an_offset_can_name_lands_on_the_start() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let tree = Tree::new();
    harness.load(&tree.wav("echoes.wav"));
    harness.wait_for(|harness| harness.status() == "Playing", "playback to start");

    let player = harness.proxy(PLAYER);
    let track = harness.playing_track();
    player
        .call::<_, _, ()>("SetPosition", &(&track, 20_000_000_i64))
        .expect("SetPosition is served");
    harness.wait_for(
        |harness| harness.position() >= 19_000_000,
        "the seek to land",
    );

    player
        .call::<_, _, ()>("Seek", &(i64::MIN,))
        .expect("Seek is served");
    harness.wait_for(
        |harness| harness.position() < 10_000_000,
        "the seek back to the start to land",
    );
}

#[test]
fn a_seek_past_the_end_of_a_track_acts_like_next_the_way_the_spec_says() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let tree = Tree::new();
    harness.load_all(&[tree.wav("first.wav"), tree.wav("second.wav")]);
    harness.wait_for(|harness| harness.status() == "Playing", "playback to start");
    harness.wait_for(
        |harness| {
            harness
                .player
                .state()
                .current
                .is_some_and(|track| track.duration.is_some())
        },
        "the length of the track to be known",
    );

    harness
        .proxy(PLAYER)
        .call::<_, _, ()>("Seek", &(60_000_000_i64,))
        .expect("Seek is served");

    harness.wait_for(
        |harness| {
            harness
                .player
                .state()
                .current
                .is_some_and(|track| track.id.get() == 2)
        },
        "the seek past the end to move to the next track",
    );
}

#[test]
fn a_cut_added_by_its_frames_is_queued_as_that_cut_and_named_by_them_again() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let tree = Tree::new();
    harness.load(&tree.wav("echoes.wav"));
    harness.wait_for(
        |harness| harness.tracks().len() == 1,
        "the track list to publish",
    );

    let file = tree.wav("side.wav");
    let span = FrameSpan::between(Frames(441), Frames(4_410));
    let uri = MediaLocation::local(&file).to_uri_within(Some(span));
    let list = harness.proxy(TRACK_LIST);
    let tail = harness.tracks().last().cloned().expect("the loaded row");
    list.call::<_, _, ()>("AddTrack", &(&uri, &tail, false))
        .expect("AddTrack is served");
    harness.wait_for(
        |harness| harness.tracks().len() == 2,
        "the cut to reach the track list",
    );

    let added = harness
        .player
        .queue()
        .get(1)
        .cloned()
        .expect("the added row");
    assert_eq!(added.location, MediaLocation::local(&file));
    assert_eq!(
        added.span,
        Some(span),
        "the cut was queued as the whole file"
    );

    let rows = harness.tracks();
    let described = list
        .call::<_, _, Vec<HashMap<String, OwnedValue>>>("GetTracksMetadata", &rows)
        .expect("GetTracksMetadata is served");
    assert_eq!(
        described
            .get(1)
            .and_then(|fields| text(fields, "xesam:url")),
        Some(uri),
        "the bus named the cut by its file alone"
    );
}

#[test]
fn the_track_list_carries_the_queue_and_takes_edits_back() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let tree = Tree::new();
    let queued = [tree.wav("first.wav"), tree.wav("second.wav")];
    harness.load_all(&queued);

    harness.wait_for(|harness| harness.status() == "Playing", "playback to start");
    harness.wait_for(
        |harness| harness.tracks().len() == 2,
        "the track list to publish",
    );
    harness.wait_for(
        |harness| text(&harness.metadata(), "xesam:title").is_some(),
        "the tags to reach the bus",
    );

    let list = harness.proxy(TRACK_LIST);
    assert!(
        list.get_property::<bool>("CanEditTracks")
            .expect("CanEditTracks")
    );

    let rows = harness.tracks();
    let described = list
        .call::<_, _, Vec<HashMap<String, OwnedValue>>>("GetTracksMetadata", &rows)
        .expect("GetTracksMetadata is served");
    assert_eq!(described.len(), 2);
    assert_eq!(
        text(described.first().expect("the playing row"), "xesam:title").as_deref(),
        Some("Echoes"),
        "the playing row did not carry the tags the engine decoded"
    );
    let queued = described.get(1).expect("the queued row");
    assert_eq!(
        text(queued, "xesam:title").as_deref(),
        Some("Echoes"),
        "a queued row the engine never played carries no tags"
    );
    assert_eq!(
        strings(queued, "xesam:artist"),
        Some(vec!["Pink Floyd".to_owned()]),
        "a queued row carried a title but no artist"
    );
    assert!(
        queued.contains_key("mpris:length"),
        "a queued row that was read carries no length"
    );

    let signals = harness.tracklist_signals();
    let invalidated = harness.invalidated("Tracks");
    thread::sleep(SETTLE);

    let added = tree.wav("added.wav");
    let uri = format!("file://{}", added.display());
    let head = OwnedObjectPath::try_from(NO_TRACK).expect("a valid path");
    list.call::<_, _, ()>("AddTrack", &(&uri, &head, false))
        .expect("AddTrack is served");
    announced(&signals, "TrackAdded");
    invalidated
        .recv_timeout(PATIENCE)
        .expect("a row added left Tracks announced as unchanged");
    harness.wait_for(
        |harness| harness.tracks().len() == 3,
        "the added row to reach the track list",
    );
    assert_eq!(
        harness
            .player
            .queue()
            .first()
            .map(|item| item.location.clone()),
        Some(MediaLocation::local(&added)),
        "the row did not land at the head of the list"
    );
    assert_eq!(
        harness.player.state().current.map(|track| track.id.get()),
        Some(1),
        "adding a row moved the track that was playing"
    );

    let last = harness.tracks().last().cloned().expect("a third row");
    list.call::<_, _, ()>("GoTo", &&last)
        .expect("GoTo is served");
    harness.wait_for(
        |harness| harness.player.state().queue_position == Some(2),
        "the jump to the row GoTo named",
    );

    let head = harness.tracks().first().cloned().expect("the added row");
    list.call::<_, _, ()>("RemoveTrack", &&head)
        .expect("RemoveTrack is served");
    announced(&signals, "TrackRemoved");
    harness.wait_for(
        |harness| harness.tracks().len() == 2,
        "the removed row to leave the track list",
    );
    assert_eq!(
        harness.player.state().queue_position,
        Some(1),
        "removing a row above the playing one lost the position"
    );
    assert_eq!(harness.status(), "Playing");
}

#[test]
fn one_file_queued_twice_is_two_rows_the_track_list_can_tell_apart() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let tree = Tree::new();
    let twice = tree.wav("twice.wav");
    harness.load_alike(&twice, 2);

    harness.wait_for(
        |harness| harness.tracks().len() == 2,
        "the track list to publish",
    );
    let rows = harness.tracks();
    let (first, second) = (rows[0].clone(), rows[1].clone());
    assert_ne!(
        first, second,
        "one file queued twice published one path for both rows"
    );

    let list = harness.proxy(TRACK_LIST);
    list.call::<_, _, ()>("RemoveTrack", &&second)
        .expect("RemoveTrack is served");
    harness.wait_for(
        |harness| harness.tracks().len() == 1,
        "the removed row to leave the track list",
    );
    assert_eq!(
        harness.tracks().first(),
        Some(&first),
        "removing the second of two alike rows took the first"
    );
    assert_eq!(harness.status(), "Playing");
}

fn queued_rows(harness: &Harness) -> Vec<MediaLocation> {
    harness
        .player
        .queue()
        .iter()
        .map(|item| item.location.clone())
        .collect()
}

#[test]
fn files_queued_onto_a_running_player_land_at_the_end_in_the_order_they_were_named() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let tree = Tree::new();
    let playing = tree.wav("playing.wav");
    harness.load(&playing);
    harness.wait_for(
        |harness| harness.tracks().len() == 1,
        "the queue to reach the track list",
    );

    let running = listed_as(&harness.destination);
    let first = tree.wav("first.wav");
    let second = tree.wav("second.wav");
    let rows = [
        (MediaLocation::local(&first), None),
        (MediaLocation::local(&second), None),
    ];

    assert_eq!(
        running
            .queue(
                &[],
                Queueing {
                    at: Placement::Last,
                    play: false
                }
            )
            .expect("queueing nothing is nothing to queue"),
        0
    );
    assert_eq!(
        running
            .queue(
                &rows,
                Queueing {
                    at: Placement::Last,
                    play: false
                }
            )
            .expect("the running player takes the rows"),
        2
    );

    harness.wait_for(
        |harness| harness.tracks().len() == 3,
        "the queued rows to reach the track list",
    );
    assert_eq!(
        queued_rows(&harness),
        vec![
            MediaLocation::local(&playing),
            MediaLocation::local(&first),
            MediaLocation::local(&second),
        ],
        "the rows landed somewhere other than the end, or out of the order they were named in"
    );
    assert_eq!(
        harness.player.state().queue_position,
        Some(0),
        "queueing moved the row being played"
    );
    assert_eq!(harness.status(), "Playing");
}

#[test]
fn a_file_queued_next_lands_after_the_row_being_played_and_is_heard_when_it_is_told_to_be() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let tree = Tree::new();
    let paths: Vec<PathBuf> = (0..3).map(|n| tree.wav(&format!("{n}.wav"))).collect();
    harness.load_all(&paths);
    harness.wait_for(
        |harness| harness.tracks().len() == 3,
        "the queue to reach the track list",
    );

    let running = listed_as(&harness.destination);
    let next = tree.wav("next.wav");
    running
        .queue(
            &[(MediaLocation::local(&next), None)],
            Queueing {
                at: Placement::Next,
                play: false,
            },
        )
        .expect("the running player takes the row");

    harness.wait_for(
        |harness| harness.tracks().len() == 4,
        "the queued row to reach the track list",
    );
    assert_eq!(
        queued_rows(&harness).get(1),
        Some(&MediaLocation::local(&next)),
        "a row queued next landed somewhere other than after the row being played"
    );
    assert_eq!(
        harness.player.state().queue_position,
        Some(0),
        "queueing a row next started playing it"
    );

    let heard = tree.wav("heard.wav");
    running
        .queue(
            &[(MediaLocation::local(&heard), None)],
            Queueing {
                at: Placement::Next,
                play: true,
            },
        )
        .expect("the running player takes the row");

    harness.wait_for(
        |harness| harness.player.state().queue_position == Some(1),
        "the row queued to be heard now to be the one playing",
    );
    assert_eq!(
        queued_rows(&harness).get(1),
        Some(&MediaLocation::local(&heard))
    );
}

#[test]
fn a_run_of_files_queued_at_a_row_lands_on_it_in_the_order_they_were_named() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let tree = Tree::new();
    let paths: Vec<PathBuf> = (0..3).map(|n| tree.wav(&format!("{n}.wav"))).collect();
    harness.load_all(&paths);
    harness.wait_for(
        |harness| harness.tracks().len() == 3,
        "the queue to reach the track list",
    );

    let running = listed_as(&harness.destination);
    let first = tree.wav("at-first.wav");
    let second = tree.wav("at-second.wav");
    running
        .queue(
            &[
                (MediaLocation::local(&first), None),
                (MediaLocation::local(&second), None),
            ],
            Queueing {
                at: Placement::At(2),
                play: false,
            },
        )
        .expect("the running player takes the rows");

    harness.wait_for(
        |harness| harness.tracks().len() == 5,
        "the queued rows to reach the track list",
    );
    let rows = queued_rows(&harness);
    assert_eq!(rows.get(2), Some(&MediaLocation::local(&first)));
    assert_eq!(rows.get(3), Some(&MediaLocation::local(&second)));
    assert_eq!(
        rows.get(4),
        Some(&MediaLocation::local(&paths[2])),
        "the rows that were there did not move down past the ones that landed"
    );
}

fn listed_as(name: &str) -> Running {
    Running::listed()
        .expect("the bus lists the names it holds")
        .into_iter()
        .find(|player| player.name().as_str() == name)
        .unwrap_or_else(|| panic!("{name} was not listed among the players of this build"))
}

#[test]
fn a_window_on_the_bus_says_it_can_be_raised_and_answers_a_raise() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let running = listed_as(&harness.destination);

    assert!(running.can_raise().expect("CanRaise is served"));
    running.raise().expect("Raise is served");
    harness.wait_for(
        |harness| harness.raised.load(Ordering::Acquire),
        "the raise to reach the window",
    );
}

#[test]
fn a_player_of_this_build_is_listed_with_what_it_is_playing() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let tree = Tree::new();
    harness.load(&tree.wav("listed.wav"));
    harness.wait_for(
        |harness| harness.tracks().len() == 1,
        "the queue to reach the track list",
    );

    let running = listed_as(&harness.destination);
    harness.wait_for(
        |_| {
            running
                .standing()
                .is_ok_and(|standing| standing.title.is_some())
        },
        "the tags of the track being played to reach the metadata",
    );

    let standing = running.standing().expect("the player answers about itself");
    assert_eq!(standing.name.as_str(), harness.destination);
    assert_eq!(standing.playback, Some(PlaybackStatus::Playing));
    assert_eq!(standing.queued, 1);
    assert_eq!(standing.title.as_deref(), Some("Echoes"));
    assert_eq!(standing.artist.as_deref(), Some("Pink Floyd"));
}

#[test]
fn a_player_is_named_by_its_whole_bus_name_or_by_the_instance_under_it() {
    let Some(harness) = Harness::start() else {
        return;
    };

    let whole = Running::named(&PlayerName::new(harness.destination.as_str()))
        .expect("the bus lists the names it holds")
        .expect("the player answers to the name it claimed");
    assert_eq!(whole.name().as_str(), harness.destination);

    if let Some(instance) = harness.destination.strip_prefix(BUS_NAME_PREFIX) {
        let under = Running::named(&PlayerName::new(instance))
            .expect("the bus lists the names it holds")
            .expect("the player answers to the instance alone");
        assert_eq!(under.name().as_str(), harness.destination);
    }

    assert!(
        Running::named(&PlayerName::new("instance-nothing-holds"))
            .expect("the bus lists the names it holds")
            .is_none(),
        "a name no player holds was answered by one anyway"
    );
}

type Listed = (OwnedObjectPath, String, String);

#[test]
fn the_playlists_interface_lists_what_the_collection_holds() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let playlists = harness.proxy(PLAYLISTS);

    assert_eq!(
        playlists
            .get_property::<u32>("PlaylistCount")
            .expect("PlaylistCount"),
        2
    );
    assert_eq!(
        playlists
            .get_property::<Vec<String>>("Orderings")
            .expect("Orderings"),
        [
            "Alphabetical",
            "CreationDate",
            "ModifiedDate",
            "LastPlayDate"
        ]
    );

    let alphabetical = playlists
        .call::<_, _, Vec<Listed>>("GetPlaylists", &(0_u32, 10_u32, "Alphabetical", false))
        .expect("GetPlaylists is served");
    assert_eq!(
        names(&alphabetical),
        vec!["Anything Loud".to_owned(), "Evening".to_owned()]
    );

    let reversed = playlists
        .call::<_, _, Vec<Listed>>("GetPlaylists", &(0_u32, 1_u32, "Alphabetical", true))
        .expect("GetPlaylists is served");
    assert_eq!(names(&reversed), vec!["Evening".to_owned()]);

    let paged = playlists
        .call::<_, _, Vec<Listed>>("GetPlaylists", &(1_u32, 10_u32, "Alphabetical", false))
        .expect("GetPlaylists is served");
    assert_eq!(names(&paged), vec!["Evening".to_owned()]);

    let by_last_play = playlists
        .call::<_, _, Vec<Listed>>("GetPlaylists", &(0_u32, 10_u32, "LastPlayDate", false))
        .expect("an advertised ordering is served");
    assert_eq!(by_last_play.len(), 2);

    assert!(
        playlists
            .call::<_, _, Vec<Listed>>("GetPlaylists", &(0_u32, 10_u32, "UserDefined", false))
            .is_err(),
        "an ordering the player never advertised was served"
    );

    let (valid, _) = playlists
        .get_property::<(bool, Listed)>("ActivePlaylist")
        .expect("ActivePlaylist");
    assert!(!valid, "a playlist was active before one was activated");
}

#[test]
fn activating_a_playlist_reaches_the_collection_and_is_announced() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let playlists = harness.proxy(PLAYLISTS);
    let signals = harness.playlist_signals();
    thread::sleep(SETTLE);

    let evening = OwnedObjectPath::try_from("/org/resonate/playlist/1").expect("a valid path");
    playlists
        .call::<_, _, ()>("ActivatePlaylist", &&evening)
        .expect("ActivatePlaylist is served");
    assert_eq!(
        harness.shelf.activated.lock().first().map(|id| id.get()),
        Some(1)
    );

    let (valid, active) = playlists
        .get_property::<(bool, Listed)>("ActivePlaylist")
        .expect("ActivePlaylist");
    assert!(valid, "the activated playlist is not reported as active");
    assert_eq!(active.0, evening);
    assert_eq!(active.1, "Evening");

    harness.shelf.rows.lock()[0].name = "Late Evening".to_owned();
    harness.shelf.revision.fetch_add(1, Ordering::AcqRel);
    announced(&signals, "PlaylistChanged");

    let gone = OwnedObjectPath::try_from(NO_PLAYLIST).expect("a valid path");
    assert!(
        playlists
            .call::<_, _, ()>("ActivatePlaylist", &&gone)
            .is_err(),
        "a path naming no playlist was activated"
    );
}

fn names(listed: &[Listed]) -> Vec<String> {
    listed.iter().map(|row| row.1.clone()).collect()
}

#[test]
fn open_uri_hands_a_local_file_to_the_front_end_and_refuses_the_rest() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let player = harness.proxy(PLAYER);

    player
        .call::<_, _, ()>("OpenUri", &"file:///music/Pink%20Floyd/Echoes.flac")
        .expect("OpenUri is served");
    assert_eq!(
        harness.opened.lock().first().cloned(),
        Some(MediaLocation::local("/music/Pink Floyd/Echoes.flac"))
    );

    assert!(
        player
            .call::<_, _, ()>("OpenUri", &"http://example.com/a.mp3")
            .is_err(),
        "a remote URI was accepted"
    );
}

#[test]
fn a_seek_is_announced_from_the_count_the_engine_publishes_and_nothing_else_is() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let tree = Tree::new();
    harness.load_all(&[tree.wav("first.wav"), tree.wav("second.wav")]);
    harness.wait_for(|harness| harness.status() == "Playing", "playback to start");
    harness.wait_for(
        |harness| {
            harness
                .player
                .state()
                .current
                .is_some_and(|track| track.duration.is_some())
        },
        "the length of the track to be known",
    );

    let signals = harness.player_signals();
    thread::sleep(SETTLE);

    let track = harness.playing_track();
    let a_quarter_of_a_second_on = harness.position() + 250_000;
    harness
        .proxy(PLAYER)
        .call::<_, _, ()>("SetPosition", &(&track, a_quarter_of_a_second_on))
        .expect("SetPosition is served");
    announced(&signals, "Seeked");

    harness
        .player
        .send(Command::Seek(Frames(u64::from(RATE) * 120)))
        .expect("the engine takes a seek");
    unannounced(&signals, "Seeked", SETTLE * 4);

    harness
        .proxy(PLAYER)
        .call::<_, _, ()>("Next", &())
        .expect("Next is served");
    harness.wait_for(
        |harness| {
            harness
                .player
                .state()
                .current
                .is_some_and(|track| track.id.get() == 2)
        },
        "the second track to take over",
    );
    unannounced(&signals, "Seeked", SETTLE * 4);
}

#[test]
fn every_id_a_client_asks_about_is_answered_in_the_order_it_asked() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let tree = Tree::new();
    harness.load_all(&[tree.wav("first.wav"), tree.wav("second.wav")]);
    harness.wait_for(|harness| harness.status() == "Playing", "playback to start");
    harness.wait_for(
        |harness| harness.tracks().len() == 2,
        "the track list to publish",
    );
    harness.wait_for(
        |harness| text(&harness.metadata(), "xesam:title").is_some(),
        "the tags to reach the bus",
    );

    let rows = harness.tracks();
    let unheld = OwnedObjectPath::try_from("/org/resonate/track/99").expect("a valid path");
    let nothing = OwnedObjectPath::try_from(NO_TRACK).expect("a valid path");
    let asked = vec![unheld, rows[1].clone(), nothing, rows[0].clone()];

    let described = harness
        .proxy(TRACK_LIST)
        .call::<_, _, Vec<HashMap<String, OwnedValue>>>("GetTracksMetadata", &asked)
        .expect("GetTracksMetadata is served");

    assert_eq!(
        described.len(),
        asked.len(),
        "the reply did not line up with the ids it was asked about"
    );
    for (entry, id) in described.iter().zip(&asked) {
        assert_eq!(
            path_of(entry).as_ref(),
            Some(id),
            "an entry named a row other than the one asked about"
        );
    }
    assert_eq!(
        described[0].len(),
        1,
        "a row the queue does not hold carried more than the id it was asked about"
    );
    assert_eq!(
        described[2].len(),
        1,
        "the no-track path carried more than the id it was asked about"
    );
    assert!(
        text(&described[1], "xesam:title").is_some(),
        "a queued row the player does hold carried no tags"
    );
}

#[test]
fn what_the_catalog_has_counted_reaches_the_bus_as_a_use_count_and_a_date() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let tree = Tree::new();
    let counted = tree.wav("counted.wav");
    let fresh = tree.wav("fresh.wav");
    let unknown = tree.wav("unknown.wav");

    harness.remember(
        &counted,
        None,
        Heard {
            plays: 12,
            played: Some(UNIX_EPOCH + Duration::from_secs(1_000_000_000)),
        },
    );
    harness.remember(
        &fresh,
        None,
        Heard {
            plays: 0,
            played: None,
        },
    );

    harness.load_all(&[counted, fresh, unknown]);
    harness.wait_for(|harness| harness.status() == "Playing", "playback to start");
    harness.wait_for(
        |harness| harness.tracks().len() == 3,
        "the track list to publish",
    );
    harness.wait_for(
        |harness| text(&harness.metadata(), "xesam:title").is_some(),
        "the tags to reach the bus",
    );

    let playing = harness.metadata();
    assert_eq!(
        count(&playing, "xesam:useCount"),
        Some(12),
        "the playing track carried no play count"
    );
    assert_eq!(
        text(&playing, "xesam:lastUsed").as_deref(),
        Some("2001-09-09T01:46:40Z"),
        "the playing track carried no date it was last played"
    );

    let rows = harness.tracks();
    let described = harness
        .proxy(TRACK_LIST)
        .call::<_, _, Vec<HashMap<String, OwnedValue>>>("GetTracksMetadata", &rows)
        .expect("GetTracksMetadata is served");
    assert_eq!(described.len(), 3);

    assert_eq!(count(&described[0], "xesam:useCount"), Some(12));
    assert_eq!(
        text(&described[0], "xesam:lastUsed").as_deref(),
        Some("2001-09-09T01:46:40Z")
    );

    assert_eq!(
        count(&described[1], "xesam:useCount"),
        Some(0),
        "a row the catalog holds and nothing has played carried no count"
    );
    assert!(
        !described[1].contains_key("xesam:lastUsed"),
        "a row nothing has played carried a date it was last played"
    );

    assert!(
        !described[2].contains_key("xesam:useCount"),
        "a row the catalog does not hold carried a play count"
    );
    assert!(
        !described[2].contains_key("xesam:lastUsed"),
        "a row the catalog does not hold carried a date it was last played"
    );
}

#[test]
fn two_rows_cut_out_of_one_file_are_counted_apart() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let tree = Tree::new();
    let cut = tree.wav("cut.wav");
    let first = FrameSpan::between(Frames::ZERO, Frames(RATE as u64 * 10));
    let second = FrameSpan::between(Frames(RATE as u64 * 10), Frames(RATE as u64 * 20));

    harness.remember(
        &cut,
        Some(first),
        Heard {
            plays: 7,
            played: None,
        },
    );
    harness.remember(
        &cut,
        Some(second),
        Heard {
            plays: 3,
            played: None,
        },
    );

    harness.load_cut(&cut, &[first, second]);
    harness.wait_for(
        |harness| harness.tracks().len() == 2,
        "the track list to publish",
    );

    let rows = harness.tracks();
    let described = harness
        .proxy(TRACK_LIST)
        .call::<_, _, Vec<HashMap<String, OwnedValue>>>("GetTracksMetadata", &rows)
        .expect("GetTracksMetadata is served");

    assert_eq!(count(&described[0], "xesam:useCount"), Some(7));
    assert_eq!(
        count(&described[1], "xesam:useCount"),
        Some(3),
        "both rows of one file answered with the same count"
    );
}

struct Held {
    source: SourceId,
    key: String,
    bytes: Vec<u8>,
    opened: Sender<()>,
    may_serve: Mutex<Receiver<()>>,
}

impl MediaProvider for Held {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn open(&self, location: &MediaLocation) -> CodecResult<Media> {
        if location.locator().as_key() != Some(self.key.as_str()) {
            return Err(CodecError::LocatorNotUsable {
                location: location.clone(),
            });
        }
        let _ = self.opened.send(());
        let _ = self.may_serve.lock().recv();

        Ok(Media {
            stream: Box::new(Reading::new(Cursor::new(self.bytes.clone()))),
            hint: None,
        })
    }
}

struct Refuses {
    source: SourceId,
    opened: Sender<()>,
}

impl MediaProvider for Refuses {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn open(&self, location: &MediaLocation) -> CodecResult<Media> {
        let _ = self.opened.send(());
        Err(CodecError::LocatorNotUsable {
            location: location.clone(),
        })
    }
}

#[test]
fn a_row_whose_read_answered_nothing_is_told_so_rather_than_owed_for_ever() {
    let named = SourceId::new("refuses").expect("a lowercase name");
    let (opened, has_opened) = unbounded();
    let sources = Sources::local().and(Arc::new(Refuses {
        source: named.clone(),
        opened,
    }));
    let Some(harness) = Harness::started(Arc::new(sources)) else {
        return;
    };

    harness
        .player
        .send(Command::Load {
            items: vec![QueueItem {
                id: TrackId::new(1).expect("a non-zero track id"),
                location: MediaLocation::new(named, "refuses/1.wav"),
                span: None,
            }],
            start_at: 0,
            autoplay: false,
        })
        .expect("the engine takes a queue");
    harness.wait_for(
        |harness| harness.tracks().len() == 1,
        "the track list to publish",
    );

    let signals = harness.tracklist_signals();
    let rows = harness.tracks();
    let described = harness
        .proxy(TRACK_LIST)
        .call::<_, _, Vec<HashMap<String, OwnedValue>>>("GetTracksMetadata", &rows)
        .expect("GetTracksMetadata is served");

    assert_eq!(described.len(), 1);
    assert!(
        !described[0].contains_key("mpris:length"),
        "a row the source refused answered with a length"
    );
    has_opened
        .recv_timeout(PATIENCE)
        .expect("the tag reader opens the row");

    announced(&signals, "TrackMetadataChanged");
}

#[test]
fn a_row_the_call_could_not_read_in_time_is_announced_when_its_tags_land() {
    let named = SourceId::new("held").expect("a lowercase name");
    let (opened, has_opened) = unbounded();
    let (may_serve, serve) = unbounded();
    let sources = Sources::local().and(Arc::new(Held {
        source: named.clone(),
        key: "held/1.wav".to_owned(),
        bytes: wav(),
        opened,
        may_serve: Mutex::new(serve),
    }));
    let Some(harness) = Harness::started(Arc::new(sources)) else {
        return;
    };

    harness
        .player
        .send(Command::Load {
            items: vec![QueueItem {
                id: TrackId::new(1).expect("a non-zero track id"),
                location: MediaLocation::new(named, "held/1.wav"),
                span: None,
            }],
            start_at: 0,
            autoplay: false,
        })
        .expect("the engine takes a queue");
    harness.wait_for(
        |harness| harness.tracks().len() == 1,
        "the track list to publish",
    );

    let signals = harness.tracklist_signals();
    let rows = harness.tracks();
    let described = harness
        .proxy(TRACK_LIST)
        .call::<_, _, Vec<HashMap<String, OwnedValue>>>("GetTracksMetadata", &rows)
        .expect("GetTracksMetadata is served");

    assert_eq!(described.len(), 1);
    assert!(
        !described[0].contains_key("mpris:length"),
        "a row whose tags had not been read answered with a length"
    );
    has_opened
        .recv_timeout(PATIENCE)
        .expect("the tag reader opens the row");
    may_serve.send(()).expect("the reader is still waiting");
    announced(&signals, "TrackMetadataChanged");

    let described = harness
        .proxy(TRACK_LIST)
        .call::<_, _, Vec<HashMap<String, OwnedValue>>>("GetTracksMetadata", &rows)
        .expect("GetTracksMetadata is served");
    assert!(
        described[0].contains_key("mpris:length"),
        "the row that was announced still answered without its tags"
    );
}

fn sleep_reads(harness: &Harness) -> (String, u64) {
    harness
        .proxy(RESONATE)
        .get_property::<(String, u64)>(SLEEP)
        .expect("Sleep is readable")
}

fn set_sleep(harness: &Harness, mode: &str, seconds: u64) {
    harness
        .proxy(RESONATE)
        .call::<_, _, ()>(SET_SLEEP, &(mode, seconds))
        .expect("SetSleep is served");
}

#[test]
fn the_sleep_timer_is_read_back_as_it_was_set_over_the_bus() {
    let Some(harness) = Harness::start() else {
        return;
    };

    let announced = harness.changed(SLEEP);
    set_sleep(&harness, "after", 900);
    announced
        .recv_deadline(Instant::now() + PATIENCE)
        .expect("the poll announced no change to the sleep timer");

    harness.wait_for(
        |harness| sleep_reads(harness).0 == "after",
        "the sleep timer to reach the bus",
    );
    let (_, seconds) = sleep_reads(&harness);
    assert!(
        (880..=900).contains(&seconds),
        "a timer set for 900 seconds read back with {seconds} left"
    );

    for mode in ["end-of-track", "end-of-queue"] {
        set_sleep(&harness, mode, 0);
        harness.wait_for(
            |harness| sleep_reads(harness).0 == mode,
            "the sleep timer to reach the bus",
        );
        assert_eq!(sleep_reads(&harness), (mode.to_owned(), 0));
    }
}

#[test]
fn a_sleep_timer_set_to_nothing_reads_as_off() {
    let Some(harness) = Harness::start() else {
        return;
    };

    assert_eq!(sleep_reads(&harness), ("off".to_owned(), 0));

    set_sleep(&harness, "after", 600);
    harness.wait_for(
        |harness| sleep_reads(harness).0 == "after",
        "the sleep timer to reach the bus",
    );

    set_sleep(&harness, "off", 0);
    harness.wait_for(
        |harness| sleep_reads(harness).0 == "off",
        "the sleep timer to be taken off",
    );
    assert_eq!(sleep_reads(&harness), ("off".to_owned(), 0));
}

#[test]
fn an_unknown_sleep_mode_is_refused_by_name() {
    let Some(harness) = Harness::start() else {
        return;
    };

    let refused = harness
        .proxy(RESONATE)
        .call::<_, _, ()>(SET_SLEEP, &("tomorrow", 0_u64))
        .expect_err("a mode this build does not know is refused");

    let zbus::Error::MethodError(named, said, _) = &refused else {
        panic!("an unknown sleep mode was refused as {refused}");
    };
    assert_eq!(named.as_str(), "org.freedesktop.DBus.Error.InvalidArgs");
    assert!(
        said.as_deref()
            .is_some_and(|said| said.contains("tomorrow")),
        "the refusal did not name the mode it was given: {said:?}"
    );
    assert_eq!(sleep_reads(&harness), ("off".to_owned(), 0));
}

#[test]
fn a_running_player_answers_the_transport_calls_a_client_makes() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let tree = Tree::new();
    let paths: Vec<PathBuf> = (0..2)
        .map(|n| tree.wav(&format!("heard-{n}.wav")))
        .collect();
    harness.load_all(&paths);
    harness.wait_for(|harness| harness.status() == "Playing", "playback to start");

    let running = listed_as(&harness.destination);

    running.pause().expect("Pause is served");
    harness.wait_for(
        |harness| harness.status() == "Paused",
        "the pause to reach the transport",
    );
    running.play_pause().expect("PlayPause is served");
    harness.wait_for(
        |harness| harness.status() == "Playing",
        "the toggle to reach the transport",
    );

    running.next().expect("Next is served");
    harness.wait_for(
        |harness| harness.player.state().queue_position == Some(1),
        "the next row to be the one playing",
    );
    running.previous().expect("Previous is served");
    harness.wait_for(
        |harness| harness.player.state().queue_position == Some(0),
        "the row before to be the one playing",
    );

    let half = Volume::new(0.5).expect("a volume between zero and one");
    running.set_volume(half).expect("Volume is writable");
    harness.wait_for(
        |_| running.volume().is_ok_and(|volume| volume == half),
        "the volume to settle",
    );

    harness.wait_for(
        |_| {
            running
                .metadata()
                .is_ok_and(|playing| playing.is_some_and(|playing| playing.location.is_some()))
        },
        "the tags of the track being played to reach the metadata",
    );
    let playing = running
        .metadata()
        .expect("Metadata is readable")
        .expect("a track is playing");
    assert_eq!(playing.location, Some(MediaLocation::local(&paths[0])));
    assert_eq!(playing.title.as_deref(), Some("Echoes"));
    assert_eq!(playing.artist.as_deref(), Some("Pink Floyd"));
    assert_eq!(playing.album.as_deref(), Some("Meddle"));

    running
        .seek(Seeking::Forward(Duration::from_secs(10)))
        .expect("Seek is served");
    harness.wait_for(
        |_| {
            running
                .position()
                .is_ok_and(|at| at >= Duration::from_secs(9))
        },
        "the seek to move the position",
    );
    running
        .set_position(playing.track, Duration::from_secs(1))
        .expect("SetPosition is served");
    harness.wait_for(
        |_| {
            running
                .position()
                .is_ok_and(|at| at < Duration::from_secs(9))
        },
        "the position to be set where it was asked for",
    );

    running
        .set_sleep(Some(Until::EndOfQueue))
        .expect("SetSleep is served");
    harness.wait_for(
        |_| {
            running
                .sleep()
                .is_ok_and(|asleep| asleep.is_some_and(|asleep| asleep.until == Until::EndOfQueue))
        },
        "the sleep timer to reach the bus",
    );
    running.set_sleep(None).expect("SetSleep is served");
    harness.wait_for(
        |_| running.sleep().is_ok_and(|asleep| asleep.is_none()),
        "the sleep timer to be taken off",
    );

    running.stop().expect("Stop is served");
    harness.wait_for(
        |harness| harness.status() == "Stopped",
        "the stop to reach the transport",
    );
    running.play().expect("Play is served");
    harness.wait_for(
        |harness| harness.status() == "Playing",
        "the play to reach the transport",
    );
}

#[test]
fn a_running_player_reads_the_queue_back_through_its_client() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let tree = Tree::new();
    let paths: Vec<PathBuf> = (0..3).map(|n| tree.wav(&format!("row-{n}.wav"))).collect();
    harness.load_all(&paths);
    harness.wait_for(
        |harness| harness.tracks().len() == 3,
        "the queue to reach the track list",
    );

    let running = listed_as(&harness.destination);
    assert!(
        running
            .playlists(PlaylistOrder::Alphabetical, false, 0, None)
            .expect("the playlists are listed")
            .iter()
            .any(|playlist| playlist.name == "Evening"),
        "the collection the service was started with was not listed"
    );

    harness.wait_for(
        |_| {
            running.rows().is_ok_and(|rows| {
                rows.iter()
                    .all(|row| row.title.as_deref() == Some("Echoes"))
            })
        },
        "the tags of every queued row to reach the track list",
    );
    let rows = running.rows().expect("the track list answers");
    assert_eq!(
        rows.iter()
            .map(|row| row.location.clone())
            .collect::<Vec<_>>(),
        paths
            .iter()
            .map(|path| Some(MediaLocation::local(path)))
            .collect::<Vec<_>>()
    );

    running
        .remove_track(rows[1].track)
        .expect("RemoveTrack is served");
    harness.wait_for(
        |harness| harness.tracks().len() == 2,
        "the row to leave the track list",
    );
    assert_eq!(
        running
            .rows()
            .expect("the track list answers")
            .iter()
            .map(|row| row.location.clone())
            .collect::<Vec<_>>(),
        vec![
            Some(MediaLocation::local(&paths[0])),
            Some(MediaLocation::local(&paths[2])),
        ]
    );
}

#[test]
fn a_transport_call_answers_once_the_engine_has_applied_it() {
    let Some(harness) = Harness::start() else {
        return;
    };
    let tree = Tree::new();
    let paths: Vec<PathBuf> = (0..3).map(|n| tree.wav(&format!("row-{n}.wav"))).collect();
    harness.load_all(&paths);
    harness.wait_for(
        |harness| harness.player.state().queue_position == Some(0),
        "the first row to be the one playing",
    );
    let running = listed_as(&harness.destination);

    running.next().expect("Next is served");
    assert_eq!(harness.player.state().queue_position, Some(1));
    assert_eq!(
        running
            .metadata()
            .expect("Metadata is readable")
            .map(|playing| playing.track),
        TrackId::new(2).ok()
    );

    running.pause().expect("Pause is served");
    assert_eq!(
        running.playback().expect("PlaybackStatus is readable"),
        Some(PlaybackStatus::Paused)
    );

    running
        .seek(Seeking::Forward(Duration::from_secs(10)))
        .expect("Seek is served");
    assert!(
        running
            .position()
            .is_ok_and(|at| at >= Duration::from_secs(9)),
        "the seek had not landed when the call answered"
    );

    let queued = running.queued().expect("the track list answers");
    assert_eq!(queued.len(), 3);
    running
        .remove_track(queued[2])
        .expect("RemoveTrack is served");
    assert_eq!(
        running.queued().expect("the track list answers"),
        queued[..2]
    );
    assert_eq!(
        running
            .described(&queued[..1])
            .expect("the track list answers")
            .iter()
            .map(|row| row.track)
            .collect::<Vec<_>>(),
        queued[..1]
    );
}
