use std::{
    env, fs,
    io::{self, Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream},
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
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use parking_lot::Mutex;
use resonate_audio::{
    AudioSource, Backend, Error as SinkError, HardwareVolume, LatencyRequest, Learnt, MediaRole,
    Opener, Plays, SinkChange, SinkId, SinkInfo, SinkResult, SinkStream, StreamEvent,
    StreamRequest, Surveyor, Trial, TrialVerdict, Unplayed,
};
use resonate_cast::{CAST_SERVICE, CastId, Casting, Finding, Found, Given, Searching};
use resonate_codec::{DecodeStatus, Decoder, Sources};
use resonate_core::{
    AudioBuffer, ChannelLayout, Frames, Gain, MediaLocation, SampleFormat, SampleRate, StreamSpec,
    TrackId, Volume,
};
use resonate_engine::{Command, EngineConfig, Event, NodeName, Player, QueueItem};
use resonate_upnp::{Beside, FirstSearch, Neighbours};
use rustls::{
    ServerConfig, ServerConnection, StreamOwned,
    crypto::ring,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
};
use serde_json::{Value, json};

const ID: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f0";

const NAME: &str = "Fake speaker";

const TRANSPORT: &str = "web-7";

const MEDIA_RECEIVER: &str = "CC1AD845";

const CONNECTION: &str = "urn:x-cast:com.google.cast.tp.connection";

const HEARTBEAT: &str = "urn:x-cast:com.google.cast.tp.heartbeat";

const RECEIVER: &str = "urn:x-cast:com.google.cast.receiver";

const MEDIA: &str = "urn:x-cast:com.google.cast.media";

const WAITED_AT_MOST: Duration = Duration::from_secs(20);

const LOOKS_EVERY: Duration = Duration::from_millis(20);

const READS_WAKE_EVERY: Duration = Duration::from_millis(20);

const RATE: u32 = 44_100;

const STRIDE: usize = 4;

const SOONER_THAN: Duration = Duration::from_millis(400);

const PAST_THE_FADE_IN: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Default)]
struct Profile {
    refuses_loads: bool,
}

#[derive(Clone, Debug)]
struct Asked {
    namespace: &'static str,
    kind: String,
    said: Value,
}

struct Playing {
    session: u64,
    state: &'static str,
    idle: Option<&'static str>,
    since: Option<Instant>,
    banked: Duration,
}

impl Playing {
    fn heard(&self) -> Duration {
        self.banked + self.since.map(|since| since.elapsed()).unwrap_or_default()
    }

    fn entry(&self) -> Value {
        let mut entry = json!({
            "mediaSessionId": self.session,
            "playerState": self.state,
            "currentTime": self.heard().as_secs_f64(),
        });
        if let Some(idle) = self.idle {
            entry["idleReason"] = json!(idle);
        }
        entry
    }
}

#[derive(Default)]
struct Heard {
    profile: Profile,
    asked: Vec<Asked>,
    connections: usize,
    pongs: usize,
    launched: bool,
    level: f64,
    muted: bool,
    sessions: u64,
    playing: Option<Playing>,
    offers: Vec<String>,
    fetching: Option<String>,
    fetched: Vec<u8>,
    fetch_ended: bool,
    decodes_at_most: Option<(u32, u16)>,
}

impl Heard {
    fn cannot_decode(&self) -> bool {
        let Some((hz, channels)) = self.decodes_at_most else {
            return false;
        };
        let Some(info) = self.fetched.get(18..21) else {
            return false;
        };
        let rate = u32::from(info[0]) << 12 | u32::from(info[1]) << 4 | u32::from(info[2]) >> 4;
        let count = u16::from((info[2] >> 1) & 0b111) + 1;
        rate > hz || count > channels
    }

    fn kinds(&self) -> Vec<&str> {
        self.asked
            .iter()
            .filter(|asked| asked.namespace != CONNECTION)
            .filter(|asked| !(asked.namespace == MEDIA && asked.kind == "GET_STATUS"))
            .map(|asked| asked.kind.as_str())
            .collect()
    }

    fn said(&self, kind: &str) -> Option<&Value> {
        self.asked
            .iter()
            .find(|asked| asked.kind == kind)
            .map(|asked| &asked.said)
    }

    fn count(&self, kind: &str) -> usize {
        self.asked.iter().filter(|asked| asked.kind == kind).count()
    }

    fn receiver_status(&self, request: &Value) -> Value {
        let applications = if self.launched {
            json!([{"appId": MEDIA_RECEIVER, "sessionId": "session-7", "transportId": TRANSPORT}])
        } else {
            json!([])
        };
        json!({
            "type": "RECEIVER_STATUS",
            "requestId": request,
            "status": {
                "volume": {"level": self.level, "muted": self.muted},
                "applications": applications,
            },
        })
    }

    fn media_status(&mut self, request: &Value) -> Value {
        if let Some(playing) = self.playing.as_mut()
            && self.fetch_ended
            && playing.state == "PLAYING"
        {
            playing.banked = playing.heard();
            playing.since = None;
            playing.state = "IDLE";
            playing.idle = Some("FINISHED");
        }
        let status: Vec<Value> = self.playing.iter().map(Playing::entry).collect();
        json!({"type": "MEDIA_STATUS", "requestId": request, "status": status})
    }
}

struct Fake {
    at: SocketAddr,
    heard: Arc<Mutex<Heard>>,
}

impl Fake {
    fn start() -> Self {
        Self::with(Profile::default())
    }

    fn with(profile: Profile) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let at = listener.local_addr().expect("the receiver's address");
        let heard = Arc::new(Mutex::new(Heard {
            profile,
            level: 0.4,
            ..Heard::default()
        }));
        let config = tls_config();

        let asked = Arc::clone(&heard);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let asked = Arc::clone(&asked);
                let config = Arc::clone(&config);
                thread::spawn(move || answer(stream, config, &asked));
            }
        });

        Self { at, heard }
    }

    fn decoding_at_most(self, hz: u32, channels: u16) -> Self {
        self.heard.lock().decodes_at_most = Some((hz, channels));
        self
    }

    fn found(&self) -> Found {
        Found {
            id: CastId::new(ID),
            name: NAME.to_owned(),
            at: self.at,
        }
    }

    fn searching(&self) -> Searching {
        Searching {
            finding: Finding::Listed(vec![self.found()]),
            within: Duration::from_millis(300),
            serving_on: IpAddr::V4(Ipv4Addr::LOCALHOST),
        }
    }

    fn until(&self, what: &str, done: impl Fn(&Heard) -> bool) {
        let since = Instant::now();
        while !done(&self.heard.lock()) {
            assert!(
                since.elapsed() < WAITED_AT_MOST,
                "waited too long for {what}"
            );
            thread::sleep(LOOKS_EVERY);
        }
    }
}

fn tls_config() -> Arc<ServerConfig> {
    let certified =
        rcgen::generate_simple_self_signed(vec!["fake-cast.local".to_owned()]).expect("a cert");
    let chain = vec![CertificateDer::from(certified.cert.der().to_vec())];
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        certified.signing_key.serialize_der(),
    ));
    Arc::new(
        ServerConfig::builder_with_provider(Arc::new(ring::default_provider()))
            .with_safe_default_protocol_versions()
            .expect("TLS versions")
            .with_no_client_auth()
            .with_single_cert(chain, key)
            .expect("a server config"),
    )
}

fn framed(source: &str, destination: &str, namespace: &str, payload: &str) -> Vec<u8> {
    let mut body = vec![0x08, 0x00];
    for (key, text) in [
        (0x12, source),
        (0x1A, destination),
        (0x22, namespace),
        (0x28, ""),
        (0x32, payload),
    ] {
        body.push(key);
        if key == 0x28 {
            body.push(0);
            continue;
        }
        let mut length = text.len();
        while length > 0x7F {
            body.push((length & 0x7F) as u8 | 0x80);
            length >>= 7;
        }
        body.push(length as u8);
        body.extend_from_slice(text.as_bytes());
    }
    let mut framed = (body.len() as u32).to_be_bytes().to_vec();
    framed.extend_from_slice(&body);
    framed
}

fn varint(body: &[u8], at: &mut usize) -> u64 {
    let mut value = 0;
    let mut shift = 0;
    loop {
        let byte = body[*at];
        *at += 1;
        value |= u64::from(byte & 0x7F) << shift;
        if byte & 0x80 == 0 {
            return value;
        }
        shift += 7;
    }
}

struct Received {
    source: String,
    destination: String,
    namespace: String,
    payload: String,
}

fn unframed(body: &[u8]) -> Received {
    let mut received = Received {
        source: String::new(),
        destination: String::new(),
        namespace: String::new(),
        payload: String::new(),
    };
    let mut at = 0;
    while at < body.len() {
        let key = varint(body, &mut at);
        if key & 7 == 0 {
            varint(body, &mut at);
            continue;
        }
        let length = varint(body, &mut at) as usize;
        let text = String::from_utf8(body[at..at + length].to_vec()).expect("UTF-8");
        at += length;
        match key >> 3 {
            2 => received.source = text,
            3 => received.destination = text,
            4 => received.namespace = text,
            6 => received.payload = text,
            _ => {}
        }
    }
    received
}

type Tls = StreamOwned<ServerConnection, TcpStream>;

fn answer(stream: TcpStream, config: Arc<ServerConfig>, heard: &Arc<Mutex<Heard>>) {
    stream
        .set_read_timeout(Some(READS_WAKE_EVERY))
        .expect("a read timeout");
    let connection = ServerConnection::new(config).expect("a TLS connection");
    let mut tls: Tls = StreamOwned::new(connection, stream);
    heard.lock().connections += 1;

    let mut pinged = false;
    let mut held = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        if !pinged && !tls.conn.is_handshaking() {
            pinged = true;
            let ping = framed("receiver-0", "sender-0", HEARTBEAT, r#"{"type":"PING"}"#);
            if tls.write_all(&ping).and_then(|()| tls.flush()).is_err() {
                return;
            }
        }
        match tls.read(&mut buffer) {
            Ok(0) => return,
            Ok(read) => held.extend_from_slice(&buffer[..read]),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(_) => return,
        }
        while held.len() >= 4 {
            let length = u32::from_be_bytes([held[0], held[1], held[2], held[3]]) as usize;
            if held.len() < 4 + length {
                break;
            }
            let received = unframed(&held[4..4 + length]);
            held.drain(..4 + length);
            let replies = replied(&received, heard);
            for (namespace, reply) in replies {
                let reply = framed(
                    &received.destination,
                    &received.source,
                    namespace,
                    &reply.to_string(),
                );
                if tls.write_all(&reply).and_then(|()| tls.flush()).is_err() {
                    return;
                }
            }
        }
    }
}

fn replied(received: &Received, heard: &Arc<Mutex<Heard>>) -> Vec<(&'static str, Value)> {
    let said: Value = serde_json::from_str(&received.payload).expect("JSON from the sender");
    let kind = said["type"].as_str().unwrap_or_default().to_owned();
    let request = said["requestId"].clone();
    let namespace = match received.namespace.as_str() {
        CONNECTION => CONNECTION,
        HEARTBEAT => {
            if kind == "PING" {
                return vec![(HEARTBEAT, json!({"type": "PONG"}))];
            }
            heard.lock().pongs += 1;
            return Vec::new();
        }
        RECEIVER => RECEIVER,
        MEDIA => MEDIA,
        other => panic!("a message on {other}"),
    };

    let mut heard_now = heard.lock();
    heard_now.asked.push(Asked {
        namespace,
        kind: kind.clone(),
        said: said.clone(),
    });
    match (namespace, kind.as_str()) {
        (CONNECTION, _) => Vec::new(),
        (RECEIVER, "GET_STATUS") => vec![(RECEIVER, heard_now.receiver_status(&request))],
        (RECEIVER, "LAUNCH") => {
            assert_eq!(said["appId"], MEDIA_RECEIVER);
            heard_now.launched = true;
            vec![(RECEIVER, heard_now.receiver_status(&request))]
        }
        (RECEIVER, "SET_VOLUME") => {
            if let Some(level) = said["volume"]["level"].as_f64() {
                heard_now.level = level;
            }
            if let Some(muted) = said["volume"]["muted"].as_bool() {
                heard_now.muted = muted;
            }
            vec![(RECEIVER, heard_now.receiver_status(&request))]
        }
        (MEDIA, "LOAD") => {
            assert_eq!(received.destination, TRANSPORT);
            if heard_now.profile.refuses_loads {
                return vec![(MEDIA, json!({"type": "LOAD_FAILED", "requestId": request}))];
            }
            let offered = said["media"]["contentId"]
                .as_str()
                .expect("an address")
                .to_owned();
            heard_now.sessions += 1;
            heard_now.playing = Some(Playing {
                session: heard_now.sessions,
                state: "PLAYING",
                idle: None,
                since: Some(Instant::now()),
                banked: Duration::ZERO,
            });
            heard_now.offers.push(offered.clone());
            heard_now.fetching = Some(offered.clone());
            heard_now.fetched.clear();
            heard_now.fetch_ended = false;
            drop(heard_now);
            fetch(heard, offered);
            let status = heard.lock().media_status(&request);
            vec![(MEDIA, status)]
        }
        (MEDIA, "PLAY" | "PAUSE" | "STOP" | "GET_STATUS") => {
            let session = said["mediaSessionId"].as_u64();
            let ours = heard_now
                .playing
                .as_ref()
                .is_some_and(|playing| Some(playing.session) == session);
            if !ours {
                return vec![(
                    MEDIA,
                    json!({"type": "INVALID_REQUEST", "requestId": request, "reason": "INVALID_MEDIA_SESSION_ID"}),
                )];
            }
            if let Some(playing) = heard_now.playing.as_mut() {
                match kind.as_str() {
                    "PLAY" if playing.since.is_none() && playing.idle.is_none() => {
                        playing.since = Some(Instant::now());
                        playing.state = "PLAYING";
                    }
                    "PAUSE" if playing.since.is_some() => {
                        playing.banked = playing.heard();
                        playing.since = None;
                        playing.state = "PAUSED";
                    }
                    "STOP" => {
                        playing.banked = playing.heard();
                        playing.since = None;
                        playing.state = "IDLE";
                        playing.idle = Some("CANCELLED");
                    }
                    _ => {}
                }
            }
            vec![(MEDIA, heard_now.media_status(&request))]
        }
        (_, other) => panic!("asked {other} on {namespace}"),
    }
}

fn fetch(heard: &Arc<Mutex<Heard>>, offered: String) {
    let heard = Arc::clone(heard);
    thread::spawn(move || {
        let rest = offered.strip_prefix("http://").expect("an http address");
        let (authority, path) = rest.split_at(rest.find('/').expect("a path"));
        let mut stream = TcpStream::connect(authority).expect("the stream server answers");
        stream
            .write_all(format!("GET {path} HTTP/1.1\r\nHost: {authority}\r\n\r\n").as_bytes())
            .expect("the fetch is asked");
        let mut held = Vec::new();
        let mut buffer = [0; 4096];
        let mut head_ended = false;
        loop {
            let read = match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            };
            let mut heard = heard.lock();
            if heard.fetching.as_ref() != Some(&offered) {
                return;
            }
            if heard.cannot_decode() {
                heard.fetching = None;
                if let Some(playing) = heard.playing.as_mut() {
                    playing.since = None;
                    playing.state = "IDLE";
                    playing.idle = Some("ERROR");
                }
                return;
            }
            if head_ended {
                heard.fetched.extend_from_slice(&buffer[..read]);
                continue;
            }
            held.extend_from_slice(&buffer[..read]);
            if let Some(at) = held.windows(4).position(|window| window == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&held[..at]).into_owned();
                assert!(head.starts_with("HTTP/1.1 200 OK\r\n"), "{head}");
                assert!(head.contains("Content-Type: audio/flac"), "{head}");
                head_ended = true;
                heard.fetched.extend_from_slice(&held[at + 4..]);
            }
        }
        let mut heard = heard.lock();
        if heard.fetching.as_ref() == Some(&offered) {
            heard.fetch_ended = true;
        }
    });
}

struct NoGraph {
    changes: (Sender<SinkChange>, Receiver<SinkChange>),
}

impl NoGraph {
    fn new() -> Self {
        Self {
            changes: unbounded(),
        }
    }
}

struct Nothing;

impl Surveyor for Nothing {
    fn enumerate_sinks(&self, _timeout: Duration) -> SinkResult<Vec<SinkInfo>> {
        Ok(Vec::new())
    }
}

impl Opener for Nothing {
    fn open(
        &self,
        _request: &StreamRequest,
        _source: Box<dyn AudioSource>,
    ) -> SinkResult<SinkStream> {
        Err(SinkError::NoSink)
    }
}

impl Backend for NoGraph {
    fn subscribe_sinks(&self) -> Receiver<SinkChange> {
        self.changes.1.clone()
    }

    fn surveyor(&self) -> Arc<dyn Surveyor> {
        Arc::new(Nothing)
    }

    fn opener(&self) -> Arc<dyn Opener> {
        Arc::new(Nothing)
    }

    fn set_device_volume(&self, _sink: SinkId, _gain: Gain) -> SinkResult<()> {
        Ok(())
    }

    fn set_device_mute(&self, _sink: SinkId, _muted: bool) -> SinkResult<()> {
        Ok(())
    }

    fn shutdown(self: Box<Self>) -> SinkResult<()> {
        Ok(())
    }
}

struct Poured {
    pcm: Vec<u8>,
    at: usize,
}

impl AudioSource for Poured {
    fn fill(&mut self, dst: &mut [u8]) -> usize {
        let left = self.pcm.get(self.at..).unwrap_or_default();
        let filled = dst.len().min(left.len());
        dst[..filled].copy_from_slice(&left[..filled]);
        self.at += filled;
        filled
    }
}

fn stereo_sixteen() -> StreamSpec {
    StreamSpec::new(
        SampleRate::HZ_44100,
        ChannelLayout::Stereo,
        SampleFormat::S16,
    )
}

fn the_receiver(casting: &Casting) -> SinkInfo {
    let sinks = casting.sinks();
    assert_eq!(
        sinks.len(),
        1,
        "the one receiver on the loopback was not reached"
    );
    sinks.into_iter().next().expect("a receiver")
}

struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = env::temp_dir().join(format!(
            "resonate-cast-{}-{}",
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

fn wave_file(pcm: &[u8]) -> Vec<u8> {
    let mut file = b"RIFF".to_vec();
    file.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    file.extend_from_slice(b"WAVEfmt ");
    file.extend_from_slice(&16_u32.to_le_bytes());
    file.extend_from_slice(&1_u16.to_le_bytes());
    file.extend_from_slice(&2_u16.to_le_bytes());
    file.extend_from_slice(&RATE.to_le_bytes());
    file.extend_from_slice(&(RATE * STRIDE as u32).to_le_bytes());
    file.extend_from_slice(&(STRIDE as u16).to_le_bytes());
    file.extend_from_slice(&16_u16.to_le_bytes());
    file.extend_from_slice(b"data");
    file.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    file.extend_from_slice(pcm);
    file
}

fn music(frames: usize) -> Vec<u8> {
    let mut pcm = Vec::with_capacity(frames * STRIDE);
    for frame in 0..frames {
        let left = 1_000 + (frame * 37 % 20_000) as i16;
        let right = -1_000 - (frame * 53 % 20_000) as i16;
        pcm.extend_from_slice(&left.to_le_bytes());
        pcm.extend_from_slice(&right.to_le_bytes());
    }
    pcm
}

fn decoded(tree: &Tree, flac: &[u8]) -> Vec<u8> {
    let path = tree.write("fetched.flac", flac);
    let (mut decoder, info) =
        Decoder::open(&Sources::local(), &MediaLocation::local(&path)).expect("a FLAC stream");
    decoder.set_output_format(SampleFormat::S16);
    let mut block = AudioBuffer::empty(StreamSpec::new(
        info.spec.rate,
        info.spec.channels,
        SampleFormat::S16,
    ));
    let mut samples = Vec::new();
    while decoder.next_block(&mut block).expect("a decoded block") != DecodeStatus::EndOfStream {
        samples.extend_from_slice(block.as_bytes());
    }
    samples
}

fn within(fetched: &[u8], pcm: &[u8]) -> bool {
    let opening = &pcm[..STRIDE * 4];
    fetched
        .windows(opening.len())
        .enumerate()
        .filter(|(_, window)| *window == opening)
        .any(|(at, _)| fetched.get(at..at + pcm.len()) == Some(pcm))
}

fn pcm_bytes(heard: Duration) -> usize {
    (heard.as_secs_f64() * f64::from(RATE)) as usize * STRIDE
}

fn playing_on(casting: &Arc<Casting>) -> Player {
    let played = Arc::clone(casting);
    Player::with_backend(
        EngineConfig {
            sink: Some(NodeName::new(format!("cast:{ID}"))),
            buffer: Duration::from_millis(200),
            ..EngineConfig::default()
        },
        move |_| Ok(Box::new(Beside::over(Box::new(NoGraph::new()), played))),
    )
    .expect("a player")
}

fn queued(path: &Path) -> QueueItem {
    QueueItem {
        id: TrackId::new(1).expect("a non-zero id"),
        location: MediaLocation::local(path),
        span: None,
    }
}

fn loaded(player: &Player, path: &Path) {
    player
        .send(Command::Load {
            items: vec![queued(path)],
            start_at: 0,
            autoplay: true,
        })
        .expect("loads");
}

fn finished(player: &Player) {
    let since = Instant::now();
    loop {
        match player.events().recv_timeout(LOOKS_EVERY) {
            Ok(Event::QueueFinished) => return,
            Ok(Event::Failed { error, .. } | Event::Waiting { error, .. }) => {
                panic!("the song did not play: {error}")
            }
            Ok(_) | Err(_) => {}
        }
        assert!(since.elapsed() < WAITED_AT_MOST, "the song never finished");
    }
}

fn waited_for(events: &Receiver<StreamEvent>, wanted: impl Fn(&StreamEvent) -> bool) -> bool {
    let since = Instant::now();
    while since.elapsed() < WAITED_AT_MOST {
        if let Ok(event) = events.recv_timeout(LOOKS_EVERY)
            && wanted(&event)
        {
            return true;
        }
    }
    false
}

fn request(sink: &SinkInfo, spec: StreamSpec) -> StreamRequest {
    StreamRequest {
        target: Some(sink.id),
        spec,
        latency: LatencyRequest::Auto,
        role: MediaRole::Music,
        media_name: "Songs \"&\" <Sounds>".to_owned(),
        force_graph_rate: false,
        no_convert: true,
        realtime: true,
    }
}

#[test]
fn a_receiver_listed_is_reached_and_offered_as_a_sink() {
    let fake = Fake::start();

    let casting = Casting::start(fake.searching(), FirstSearch::Awaited).expect("reaches");
    let sink = the_receiver(&casting);

    assert_eq!(sink.name, NodeName::new(format!("cast:{ID}")));
    assert_eq!(sink.description, NAME);
    assert!(Casting::owns(sink.id));
    assert!(!resonate_upnp::Network::owns(sink.id));
    assert!(sink.supports(stereo_sixteen()));
    assert!(sink.supports(StreamSpec::new(
        SampleRate::HZ_96000,
        ChannelLayout::Stereo,
        SampleFormat::S24
    )));
    assert!(!sink.supports(StreamSpec::new(
        SampleRate::HZ_192000,
        ChannelLayout::Stereo,
        SampleFormat::S24
    )));
    assert!(!sink.supports(StreamSpec::new(
        SampleRate::HZ_48000,
        ChannelLayout::Surround51,
        SampleFormat::S16
    )));
    let port = sink.port.expect("a receiver names where it is");
    assert_eq!(port.hardware_volume, HardwareVolume::Always);
    let volume = Volume::heard_at(port.volume.expect("the receiver's volume was read"));
    assert!((volume.get() - 0.4).abs() < 1e-4);

    let heard = fake.heard.lock();
    assert_eq!(heard.kinds(), ["GET_STATUS"]);
    assert_eq!(
        heard.said("CONNECT").expect("a connect")["userAgent"],
        json!(format!("resonate/{}", env!("CARGO_PKG_VERSION")))
    );
}

#[test]
fn a_stream_to_a_receiver_launches_it_loads_flac_by_address_and_is_paused_drained_and_stopped_its_pings_answered()
 {
    let fake = Fake::start();
    let casting =
        Arc::new(Casting::start(fake.searching(), FirstSearch::Awaited).expect("reaches"));
    let sink = the_receiver(&casting);
    let backend = Beside::over(Box::new(NoGraph::new()), Arc::<Casting>::clone(&casting));
    let pcm = music(RATE as usize / 2);
    let tree = Tree::new();

    let stream = backend
        .opener()
        .open(
            &request(&sink, stereo_sixteen()),
            Box::new(Poured {
                pcm: pcm.clone(),
                at: 0,
            }),
        )
        .expect("the receiver takes the stream");
    stream.set_active(true).expect("plays");
    fake.until("the stream to be loaded", |heard| heard.count("LOAD") == 1);
    stream.drain().expect("drains");
    let drained = waited_for(stream.events(), |event| *event == StreamEvent::Drained);
    fake.until("the fetch to end once drained", |heard| heard.fetch_ended);
    stream.set_active(false).expect("pauses");
    fake.until("the receiver to be paused", |heard| {
        heard.count("PAUSE") == 1
    });
    backend
        .set_device_volume(sink.id, Volume::new(0.5).expect("a volume").to_gain())
        .expect("turns");
    backend.set_device_mute(sink.id, true).expect("mutes");
    fake.until("the receiver to be turned and muted", |heard| {
        (heard.level - 0.5).abs() < 1e-6 && heard.muted
    });
    fake.until("the receiver's ping to be answered", |heard| {
        heard.pongs > 0
    });
    stream.close().expect("closes");
    fake.until("the receiver to be stopped", |heard| {
        heard.count("STOP") == 1
    });

    let heard = fake.heard.lock();
    assert!(drained, "the drain was never told");
    assert_eq!(
        heard.kinds(),
        [
            "GET_STATUS",
            "GET_STATUS",
            "LAUNCH",
            "LOAD",
            "PAUSE",
            "SET_VOLUME",
            "SET_VOLUME",
            "STOP"
        ]
    );
    assert_eq!(
        heard
            .asked
            .iter()
            .filter(|asked| asked.namespace == CONNECTION && asked.kind == "CONNECT")
            .count(),
        4,
        "a connection each to reach, to stream, to the media receiver and to turn"
    );
    let load = heard.said("LOAD").expect("a load");
    let offered = load["media"]["contentId"].as_str().expect("an address");
    assert!(
        offered.starts_with("http://127.0.0.1:") && offered.ends_with(".flac"),
        "offered {offered}"
    );
    assert_eq!(load["media"]["contentType"], "audio/flac");
    assert_eq!(load["media"]["streamType"], "LIVE");
    assert_eq!(load["media"]["metadata"]["title"], "Songs \"&\" <Sounds>");
    assert_eq!(load["autoplay"], true);
    assert_eq!(&heard.fetched[..4], b"fLaC");
    assert!(heard.fetched.len() < pcm.len());
    assert_eq!(decoded(&tree, &heard.fetched), pcm);
}

#[test]
fn a_load_the_receiver_refuses_is_told_as_refused() {
    let fake = Fake::with(Profile {
        refuses_loads: true,
    });
    let casting =
        Arc::new(Casting::start(fake.searching(), FirstSearch::Awaited).expect("reaches"));
    let sink = the_receiver(&casting);
    let backend = Beside::over(Box::new(NoGraph::new()), Arc::<Casting>::clone(&casting));

    let stream = backend
        .opener()
        .open(
            &request(&sink, stereo_sixteen()),
            Box::new(Poured {
                pcm: music(RATE as usize),
                at: 0,
            }),
        )
        .expect("the receiver is reached");
    stream.set_active(true).expect("plays");

    assert!(waited_for(stream.events(), |event| matches!(
        event,
        StreamEvent::Refused { .. }
    )));
}

#[test]
fn a_stream_the_receiver_cannot_take_is_refused_before_it_is_reached() {
    let fake = Fake::start();
    let casting =
        Arc::new(Casting::start(fake.searching(), FirstSearch::Awaited).expect("reaches"));
    let sink = the_receiver(&casting);
    let backend = Beside::over(Box::new(NoGraph::new()), Arc::<Casting>::clone(&casting));

    let refused = backend.opener().open(
        &request(
            &sink,
            StreamSpec::new(
                SampleRate::HZ_192000,
                ChannelLayout::Stereo,
                SampleFormat::S24,
            ),
        ),
        Box::new(Poured {
            pcm: Vec::new(),
            at: 0,
        }),
    );

    assert!(matches!(refused, Err(SinkError::FormatRefused)));
    assert_eq!(fake.heard.lock().connections, 1);
}

#[test]
fn a_song_played_through_the_engine_to_a_receiver_reaches_it_bit_for_bit() {
    let fake = Fake::start();
    let casting =
        Arc::new(Casting::start(fake.searching(), FirstSearch::Awaited).expect("reaches"));
    let tree = Tree::new();
    let pcm = music(RATE as usize * 3 / 4);
    let path = tree.write("song.wav", &wave_file(&pcm));
    let player = playing_on(&casting);

    loaded(&player, &path);
    finished(&player);
    fake.until("the fetch to end once drained", |heard| heard.fetch_ended);

    let heard = fake.heard.lock();
    let samples = decoded(&tree, &heard.fetched);
    assert!(within(&samples, &pcm), "the decoded stream lacks the song");
    assert_eq!(heard.count("LAUNCH"), 1);
    assert_eq!(heard.count("LOAD"), 1);
}

#[test]
fn a_pause_and_a_volume_change_reach_a_receiver_at_once_and_the_song_resumes_bit_for_bit() {
    let fake = Fake::start();
    let casting =
        Arc::new(Casting::start(fake.searching(), FirstSearch::Awaited).expect("reaches"));
    let tree = Tree::new();
    let pcm = music(RATE as usize * 3 / 2);
    let path = tree.write("song.wav", &wave_file(&pcm));
    let player = playing_on(&casting);

    loaded(&player, &path);
    fake.until("half a second to be fetched", |heard| {
        heard
            .playing
            .as_ref()
            .is_some_and(|playing| playing.heard() >= Duration::from_millis(500))
    });
    let paused_at = Instant::now();
    player.send(Command::Pause).expect("pauses");
    fake.until("the receiver to be paused", |heard| {
        heard.count("PAUSE") == 1
    });
    let pausing_took = paused_at.elapsed();
    let turned_at = Instant::now();
    player
        .send(Command::SetVolume(Volume::new(0.5).expect("a volume")))
        .expect("turns");
    fake.until("the receiver to be turned", |heard| {
        (heard.level - 0.5).abs() < 1e-6
    });
    let turning_took = turned_at.elapsed();
    player.send(Command::Play).expect("plays on");
    finished(&player);
    fake.until("the fetch to end once drained", |heard| heard.fetch_ended);

    let heard = fake.heard.lock();
    assert!(
        pausing_took < SOONER_THAN,
        "the pause took {pausing_took:?} to reach the receiver"
    );
    assert!(
        turning_took < SOONER_THAN,
        "the volume took {turning_took:?} to reach the receiver"
    );
    assert_eq!(heard.offers.len(), 1, "a pause asked for a new address");
    let kinds = heard.kinds();
    let paused = kinds.iter().position(|kind| *kind == "PAUSE");
    let played_again = kinds.iter().rposition(|kind| *kind == "PLAY");
    assert!(paused < played_again, "{kinds:?}");
    assert!(
        within(&decoded(&tree, &heard.fetched), &pcm),
        "the song did not resume where it paused"
    );
}

#[test]
fn a_seek_on_a_receiver_loads_a_fresh_address_starting_where_it_landed() {
    let fake = Fake::start();
    let casting =
        Arc::new(Casting::start(fake.searching(), FirstSearch::Awaited).expect("reaches"));
    let tree = Tree::new();
    let pcm = music(RATE as usize * 3);
    let path = tree.write("song.wav", &wave_file(&pcm));
    let player = playing_on(&casting);
    let lands_at = Duration::from_secs(2);

    loaded(&player, &path);
    fake.until("a quarter second to be played", |heard| {
        heard
            .playing
            .as_ref()
            .is_some_and(|playing| playing.heard() >= Duration::from_millis(250))
    });
    let sought_at = Instant::now();
    player
        .send(Command::Seek(Frames::from_duration(
            lands_at,
            SampleRate::HZ_44100,
        )))
        .expect("seeks");
    fake.until("a second address to be loaded", |heard| {
        heard.offers.len() >= 2
    });
    let seeking_took = sought_at.elapsed();
    finished(&player);
    fake.until("the fresh fetch to end once drained", |heard| {
        heard.fetch_ended && heard.fetching == heard.offers.last().cloned()
    });

    let heard = fake.heard.lock();
    assert!(
        seeking_took < SOONER_THAN,
        "the seek took {seeking_took:?} to reach the receiver"
    );
    assert_ne!(heard.offers[0], heard.offers[1]);
    assert_eq!(heard.count("LAUNCH"), 1, "{:?}", heard.kinds());
    let samples = decoded(&tree, &heard.fetched);
    let rest = &pcm[pcm_bytes(lands_at + PAST_THE_FADE_IN)..];
    assert!(
        within(&samples, rest),
        "the fresh address lacks where it landed"
    );
    assert!(
        samples.len() <= pcm.len() - pcm_bytes(lands_at),
        "the fresh address carried more than lies past where the seek landed"
    );
}

const ADVERTISED_AS: &str = "Fake-speaker";

fn multicast_answers(daemon: &ServiceDaemon) -> bool {
    let Ok(browsing) = daemon.browse(CAST_SERVICE) else {
        return false;
    };
    let since = Instant::now();
    while let Some(left) = WAITED_AT_MOST.checked_sub(since.elapsed()) {
        match browsing.recv_timeout(left) {
            Ok(ServiceEvent::ServiceResolved(resolved))
                if resolved.get_fullname().starts_with(ADVERTISED_AS) =>
            {
                let _ = daemon.stop_browse(CAST_SERVICE);
                return true;
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    false
}

#[test]
fn a_receiver_advertised_over_mdns_is_found_and_gone_once_withdrawn() {
    let fake = Fake::start();
    let Ok(advertiser) = ServiceDaemon::new() else {
        eprintln!("skipped: mDNS cannot start here");
        return;
    };
    let record = ServiceInfo::new(
        CAST_SERVICE,
        ADVERTISED_AS,
        "fake-speaker.local.",
        "127.0.0.1",
        fake.at.port(),
        &[
            ("id", ID),
            ("fn", NAME),
            ("md", "Fake model"),
            ("ca", "2052"),
        ][..],
    )
    .expect("a record");
    let fullname = record.get_fullname().to_owned();
    advertiser.register(record).expect("advertises");
    if !multicast_answers(&advertiser) {
        eprintln!("skipped: no multicast answers here");
        let _ = advertiser.shutdown();
        return;
    }

    let casting = Casting::start(
        Searching {
            finding: Finding::Bonjour,
            within: Duration::from_secs(3),
            serving_on: IpAddr::V4(Ipv4Addr::LOCALHOST),
        },
        FirstSearch::Awaited,
    )
    .expect("browses");
    let changes = casting.subscribe();
    let since = Instant::now();
    while casting.sinks().is_empty() && since.elapsed() < WAITED_AT_MOST {
        thread::sleep(LOOKS_EVERY);
    }
    let sink = the_receiver(&casting);
    advertiser.unregister(&fullname).expect("withdraws");
    let removed = waited_for_change(&changes, |change| *change == SinkChange::Removed(sink.id));
    let _ = advertiser.shutdown();
    casting.rest();

    assert_eq!(sink.name, NodeName::new(format!("cast:{ID}")));
    assert_eq!(sink.description, NAME);
    assert!(removed, "the receiver withdrawn was not removed");
    assert!(casting.sinks().is_empty());
}

fn waited_for_change(changes: &Receiver<SinkChange>, wanted: impl Fn(&SinkChange) -> bool) -> bool {
    let since = Instant::now();
    while since.elapsed() < WAITED_AT_MOST {
        if let Ok(change) = changes.recv_timeout(LOOKS_EVERY)
            && wanted(&change)
        {
            return true;
        }
    }
    false
}

fn nothing_listed() -> Searching {
    Searching {
        finding: Finding::Listed(Vec::new()),
        within: Duration::from_millis(300),
        serving_on: IpAddr::V4(Ipv4Addr::LOCALHOST),
    }
}

#[test]
fn a_receiver_given_by_address_is_reached_without_a_browse_and_named_by_where_it_is() {
    let fake = Fake::start();
    let casting = Casting::resting(nothing_listed()).expect("rests");
    casting.locate(fake.at).expect("holds the address");
    assert!(casting.sinks().is_empty());

    casting
        .look_at_addresses(FirstSearch::Awaited)
        .expect("reaches");
    let sink = the_receiver(&casting);

    assert_eq!(
        sink.name,
        NodeName::new(format!("cast:{}", CastId::at(fake.at)))
    );
    assert_eq!(sink.description, "127.0.0.1");
    assert!(casting.is_reaching());
    assert!(!casting.is_looking());
    assert_eq!(casting.located(), [Given::from(fake.at)]);
}

#[test]
fn an_address_given_to_a_receiver_already_listed_makes_no_second_sink() {
    let fake = Fake::start();
    let casting = Casting::start(fake.searching(), FirstSearch::Awaited).expect("reaches");

    casting.locate(fake.at).expect("already held");

    let sink = the_receiver(&casting);
    assert_eq!(sink.name, NodeName::new(format!("cast:{ID}")));
    assert_eq!(
        sink.aliases,
        [NodeName::new(format!("cast:{}", CastId::at(fake.at)))]
    );
}

#[test]
fn an_address_given_to_a_reaching_casting_lists_the_receiver_and_forgetting_it_removes_it() {
    let fake = Fake::start();
    let casting = Casting::resting(nothing_listed()).expect("rests");
    casting
        .look_at_addresses(FirstSearch::Awaited)
        .expect("reaches");
    let changes = Neighbours::subscribe(&casting);

    casting.locate(fake.at).expect("the receiver is reached");
    let sink = the_receiver(&casting);
    casting.unlocate(fake.at);

    assert_eq!(changes.try_recv(), Ok(SinkChange::Added(sink.id)));
    assert_eq!(changes.try_recv(), Ok(SinkChange::Removed(sink.id)));
    assert!(casting.sinks().is_empty());
    assert!(casting.located().is_empty());
}

#[test]
fn a_casting_that_stops_browsing_keeps_the_receivers_given_by_address_and_drops_the_rest() {
    let browsed = Fake::start();
    let given = Fake::start();
    let casting = Casting::start(browsed.searching(), FirstSearch::Awaited).expect("reaches");
    casting.locate(given.at).expect("the receiver is reached");
    assert_eq!(casting.sinks().len(), 2);

    casting.stop_browsing();

    assert!(!casting.is_looking());
    assert!(casting.is_reaching());
    let sink = the_receiver(&casting);
    assert_eq!(
        sink.name,
        NodeName::new(format!("cast:{}", CastId::at(given.at)))
    );
}

#[test]
fn a_casting_with_no_address_that_stops_browsing_rests() {
    let fake = Fake::start();
    let casting = Casting::start(fake.searching(), FirstSearch::Awaited).expect("reaches");

    casting.stop_browsing();

    assert!(!casting.is_reaching());
    assert!(casting.sinks().is_empty());
}

#[test]
fn a_casting_reaching_addresses_alone_that_is_asked_to_look_browses_too() {
    let fake = Fake::start();
    let other = Fake::start();
    let searching = Searching {
        finding: Finding::Listed(vec![other.found()]),
        ..nothing_listed()
    };
    let casting = Casting::resting(searching).expect("rests");
    casting.locate(fake.at).expect("holds the address");
    casting
        .look_at_addresses(FirstSearch::Awaited)
        .expect("reaches");
    assert_eq!(casting.sinks().len(), 1);

    casting.look(FirstSearch::Awaited).expect("looks");

    assert!(casting.is_looking());
    assert_eq!(casting.sinks().len(), 2);
}

fn named_given(fake: &Fake) -> Given {
    Given::parse(&format!("localhost:{}", fake.at.port())).expect("a host name with a port")
}

fn waited_for_receivers(casting: &Casting, count: usize) {
    let since = Instant::now();
    while casting.sinks().len() != count {
        assert!(
            since.elapsed() < WAITED_AT_MOST,
            "waited too long for {count} receivers"
        );
        thread::sleep(LOOKS_EVERY);
    }
}

#[test]
fn a_receiver_given_by_host_name_is_resolved_reached_and_named_by_where_it_is() {
    let fake = Fake::start();
    let casting = Casting::resting(nothing_listed()).expect("rests");
    casting
        .locate(named_given(&fake))
        .expect("holds the host name");
    assert!(casting.sinks().is_empty());

    casting
        .look_at_addresses(FirstSearch::Awaited)
        .expect("reaches");
    let sink = the_receiver(&casting);

    assert_eq!(
        sink.name,
        NodeName::new(format!("cast:{}", CastId::at(fake.at)))
    );
    assert!(sink.aliases.is_empty());
    assert_eq!(casting.located(), [named_given(&fake)]);
}

#[test]
fn a_host_name_given_to_a_reaching_casting_is_resolved_off_the_callers_thread_and_forgetting_it_removes_the_receiver()
 {
    let fake = Fake::start();
    let casting = Casting::resting(nothing_listed()).expect("rests");
    casting
        .look_at_addresses(FirstSearch::Awaited)
        .expect("reaches");
    let changes = Neighbours::subscribe(&casting);

    casting
        .locate(named_given(&fake))
        .expect("the name is handed to a thread");
    waited_for_receivers(&casting, 1);
    let sink = the_receiver(&casting);
    casting.unlocate(named_given(&fake));

    assert_eq!(changes.try_recv(), Ok(SinkChange::Added(sink.id)));
    assert_eq!(changes.try_recv(), Ok(SinkChange::Removed(sink.id)));
    assert!(casting.sinks().is_empty());
    assert!(casting.located().is_empty());
}

#[test]
fn a_host_name_nothing_resolves_lists_no_receiver_and_is_kept_to_try_again() {
    let casting = Casting::resting(nothing_listed()).expect("rests");
    let unresolved = Given::parse("no-such-receiver.invalid").expect("a host name");
    casting.locate(unresolved.clone()).expect("held");

    casting
        .look_at_addresses(FirstSearch::Awaited)
        .expect("reaches");

    assert!(casting.sinks().is_empty());
    assert_eq!(casting.located(), [unresolved]);
}

#[test]
fn a_receiver_given_by_address_and_then_heard_by_the_browse_is_named_by_its_mdns_id_under_the_same_sink()
 {
    let fake = Fake::start();
    let searching = fake.searching();
    let casting = Casting::resting(searching).expect("rests");
    casting.locate(fake.at).expect("holds the address");
    casting
        .look_at_addresses(FirstSearch::Awaited)
        .expect("reaches");
    let before = the_receiver(&casting);
    let by_address = NodeName::new(format!("cast:{}", CastId::at(fake.at)));
    assert_eq!(before.name, by_address);
    let changes = Neighbours::subscribe(&casting);

    casting.look(FirstSearch::Awaited).expect("browses");
    let after = the_receiver(&casting);

    assert_eq!(after.id, before.id);
    assert_eq!(after.name, NodeName::new(format!("cast:{ID}")));
    assert_eq!(after.description, NAME);
    assert_eq!(after.aliases, std::slice::from_ref(&by_address));
    assert!(after.is_named(&by_address));
    assert!(after.is_named(&NodeName::new(format!("cast:{ID}"))));
    assert!(!after.is_named(&NodeName::new("cast:other")));
    assert_eq!(changes.try_recv(), Ok(SinkChange::Reformatted(before.id)));
    assert_eq!(
        changes.try_recv(),
        Err(crossbeam_channel::TryRecvError::Empty)
    );
}

#[test]
fn a_receiver_named_by_its_mdns_id_while_its_address_is_held_stays_listed_when_the_address_is_forgotten()
 {
    let fake = Fake::start();
    let casting = Casting::resting(fake.searching()).expect("rests");
    casting.locate(fake.at).expect("holds the address");
    casting
        .look_at_addresses(FirstSearch::Awaited)
        .expect("reaches");
    casting.look(FirstSearch::Awaited).expect("browses");
    let named = the_receiver(&casting);

    casting.unlocate(fake.at);

    let kept = the_receiver(&casting);
    assert_eq!(kept.id, named.id);
    assert_eq!(kept.name, NodeName::new(format!("cast:{ID}")));
    assert!(kept.aliases.is_empty());
}

#[test]
fn a_receiver_named_by_its_mdns_id_keeps_that_name_when_browsing_stops_with_its_address_held() {
    let fake = Fake::start();
    let casting = Casting::start(fake.searching(), FirstSearch::Awaited).expect("reaches");
    casting.locate(fake.at).expect("already held");

    casting.stop_browsing();

    let sink = the_receiver(&casting);
    assert_eq!(sink.name, NodeName::new(format!("cast:{ID}")));
    assert_eq!(
        sink.aliases,
        [NodeName::new(format!("cast:{}", CastId::at(fake.at)))]
    );
}

fn high_flac() -> StreamSpec {
    StreamSpec::new(
        SampleRate::HZ_192000,
        ChannelLayout::Stereo,
        SampleFormat::S24,
    )
}

fn surround(layout: ChannelLayout) -> StreamSpec {
    StreamSpec::new(SampleRate::HZ_48000, layout, SampleFormat::S24)
}

fn twenty_four_bit_pattern(channels: u32, frames: u32) -> Vec<u8> {
    let mut words = Vec::new();
    for frame in 0..frames {
        for channel in 0..channels {
            let word = ((frame * 131 + channel * 100_003) % 8_000_000) as i32 - 4_000_000;
            words.extend_from_slice(&word.to_le_bytes());
        }
    }
    words
}

fn decoded_at_twenty_four_bits(tree: &Tree, flac: &[u8]) -> Vec<u8> {
    let path = tree.write("fetched-24.flac", flac);
    let (mut decoder, info) =
        Decoder::open(&Sources::local(), &MediaLocation::local(&path)).expect("a FLAC stream");
    decoder.set_output_format(SampleFormat::S24);
    let mut block = AudioBuffer::empty(StreamSpec::new(
        info.spec.rate,
        info.spec.channels,
        SampleFormat::S24,
    ));
    let mut samples = Vec::new();
    while decoder.next_block(&mut block).expect("a decoded block") != DecodeStatus::EndOfStream {
        samples.extend_from_slice(block.as_bytes());
    }
    samples
}

fn poured_whole(fake: &Fake, backend: &Beside, sink: &SinkInfo, spec: StreamSpec, pcm: &[u8]) {
    let loads = fake.heard.lock().count("LOAD");
    let stream = backend
        .opener()
        .open(
            &request(sink, spec),
            Box::new(Poured {
                pcm: pcm.to_vec(),
                at: 0,
            }),
        )
        .expect("the receiver takes the stream");
    stream.set_active(true).expect("plays");
    fake.until("the stream to be loaded", |heard| {
        heard.count("LOAD") == loads + 1
    });
    stream.drain().expect("drains");
    fake.until("the fetch to end once drained", |heard| heard.fetch_ended);
    stream.close().expect("closes");
}

#[test]
fn learning_what_a_receiver_plays_offers_exactly_what_played_and_it_is_then_sent_high_rate_flac_and_six_channels_bit_for_bit()
 {
    let fake = Fake::start();
    let casting =
        Arc::new(Casting::start(fake.searching(), FirstSearch::Awaited).expect("reaches"));
    let changes = casting.subscribe();
    let before = the_receiver(&casting);
    assert!(!before.supports(high_flac()));
    assert!(!before.supports(surround(ChannelLayout::Surround51)));

    let learning = casting.learn(before.id).expect("the receiver is tried");

    assert_eq!(
        learning.verdicts(),
        Trial::ALL.map(|trial| (trial, TrialVerdict::Played))
    );
    assert_eq!(changes.try_recv(), Ok(SinkChange::Reformatted(before.id)));
    let learnt = the_receiver(&casting);
    for spec in [
        high_flac(),
        StreamSpec::new(
            SampleRate::HZ_176400,
            ChannelLayout::Stereo,
            SampleFormat::S24,
        ),
        surround(ChannelLayout::Surround51),
        surround(ChannelLayout::Surround71),
    ] {
        assert!(
            learnt.supports(spec),
            "{spec} was learnt and is not offered"
        );
    }
    {
        let heard = fake.heard.lock();
        assert_eq!(heard.count("LOAD"), 4);
        assert_eq!(heard.count("STOP"), 4, "each trial is stopped");
        assert_eq!(heard.count("LAUNCH"), 1);
    }

    let backend = Beside::over(Box::new(NoGraph::new()), Arc::<Casting>::clone(&casting));
    let tree = Tree::new();
    let pcm = twenty_four_bit_pattern(2, 192_000 / 4);
    poured_whole(&fake, &backend, &learnt, high_flac(), &pcm);
    assert_eq!(
        decoded_at_twenty_four_bits(&tree, &fake.heard.lock().fetched),
        pcm,
        "192 kHz did not come back from the FLAC as it went in"
    );

    let pcm = twenty_four_bit_pattern(6, 48_000 / 2);
    poured_whole(
        &fake,
        &backend,
        &learnt,
        surround(ChannelLayout::Surround51),
        &pcm,
    );
    assert_eq!(
        decoded_at_twenty_four_bits(&tree, &fake.heard.lock().fetched),
        pcm,
        "the six channels did not come back from the FLAC as they went in"
    );
}

#[test]
fn a_receiver_that_fails_what_it_cannot_decode_is_learnt_to_play_none_of_it() {
    let fake = Fake::start().decoding_at_most(96_000, 2);
    let casting = Casting::start(fake.searching(), FirstSearch::Awaited).expect("reaches");
    let changes = casting.subscribe();
    let before = the_receiver(&casting);

    let learning = casting.learn(before.id).expect("the receiver is tried");

    assert_eq!(
        learning.verdicts(),
        [
            (
                Trial::FlacAt192kHz,
                TrialVerdict::Unplayed(Unplayed::Errored)
            ),
            (
                Trial::FlacAt176kHz,
                TrialVerdict::Unplayed(Unplayed::Errored)
            ),
            (
                Trial::SixChannels,
                TrialVerdict::Unplayed(Unplayed::Errored)
            ),
            (Trial::EightChannels, TrialVerdict::Untried),
        ]
    );
    assert!(changes.try_recv().is_err(), "nothing learnt moves nothing");
    assert_eq!(the_receiver(&casting), before);
}

#[test]
fn a_receiver_that_refuses_the_load_is_learnt_to_play_none_of_it() {
    let fake = Fake::with(Profile {
        refuses_loads: true,
    });
    let casting = Casting::start(fake.searching(), FirstSearch::Awaited).expect("reaches");
    let sink = the_receiver(&casting);

    let learning = casting.learn(sink.id).expect("the receiver is tried");

    assert_eq!(
        learning.verdict_on(Trial::FlacAt192kHz),
        Some(TrialVerdict::Unplayed(Unplayed::Refused))
    );
    assert!(learning.plays().is_empty());
}

#[test]
fn a_receiver_a_stream_holds_is_not_tried_and_teaching_what_was_learnt_offers_it_at_once() {
    let fake = Fake::start();
    let casting =
        Arc::new(Casting::start(fake.searching(), FirstSearch::Awaited).expect("reaches"));
    let sink = the_receiver(&casting);
    let backend = Beside::over(Box::new(NoGraph::new()), Arc::<Casting>::clone(&casting));
    let stream = backend
        .opener()
        .open(
            &request(&sink, stereo_sixteen()),
            Box::new(Poured {
                pcm: music(RATE as usize),
                at: 0,
            }),
        )
        .expect("the receiver takes the stream");

    assert!(matches!(
        casting.learn(sink.id),
        Err(resonate_cast::Error::InUse(held)) if held == sink.id
    ));

    stream.close().expect("closes");
    let changes = casting.subscribe();
    let learnt: Learnt = [(sink.name.clone(), Plays::from_iter([Trial::SixChannels]))]
        .into_iter()
        .collect();

    casting.teach(learnt.clone());

    assert_eq!(changes.try_recv(), Ok(SinkChange::Reformatted(sink.id)));
    assert!(the_receiver(&casting).supports(surround(ChannelLayout::Surround51)));
    assert!(!the_receiver(&casting).supports(high_flac()));

    casting.teach(learnt);

    assert!(changes.try_recv().is_err());

    casting.teach(Learnt::default());

    assert_eq!(changes.try_recv(), Ok(SinkChange::Reformatted(sink.id)));
    assert!(!the_receiver(&casting).supports(surround(ChannelLayout::Surround51)));
}
