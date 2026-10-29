use std::{
    env, fs,
    io::{Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket},
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
use resonate_audio::{
    AudioSource, Backend, Error as SinkError, HardwareVolume, LatencyRequest, MediaRole, Opener,
    SinkChange, SinkId, SinkInfo, SinkResult, SinkStream, StreamEvent, StreamRequest, Surveyor,
};
use resonate_core::{
    ChannelLayout, Frames, Gain, MediaLocation, SampleFormat, SampleRate, StreamSpec, TrackId,
    Volume,
};
use resonate_engine::{Command, EngineConfig, Event, NodeName, Player, QueueItem};
use resonate_upnp::{Beside, FirstSearch, Network, Searching};

const UDN: &str = "uuid:fake-renderer";

const NAME: &str = "Fake renderer";

const WAVE_HEADER_BYTES: usize = 44;

const BYTES_A_SECOND_AT: usize = 28;

const WAITED_AT_MOST: Duration = Duration::from_secs(20);

const LOOKS_EVERY: Duration = Duration::from_millis(20);

const RATE: u32 = 44_100;

const STRIDE: usize = 4;

const SOONER_THAN: Duration = Duration::from_millis(400);

const PAST_THE_FADE_IN: Duration = Duration::from_millis(50);

#[derive(Default)]
struct Heard {
    calls: Vec<(String, String)>,
    offered: Option<String>,
    offers: Vec<String>,
    fetched: Vec<u8>,
    fetching: Option<String>,
    fetch_ended: bool,
    searches: usize,
}

impl Heard {
    fn actions(&self) -> Vec<&str> {
        self.calls
            .iter()
            .map(|(action, _)| action.as_str())
            .filter(|action| *action != "GetPositionInfo")
            .collect()
    }

    fn body_of(&self, action: &str) -> Option<&str> {
        self.calls
            .iter()
            .find(|(called, _)| called == action)
            .map(|(_, body)| body.as_str())
    }

    fn heard_for(&self) -> Duration {
        let Some(per_second) = self
            .fetched
            .get(BYTES_A_SECOND_AT..BYTES_A_SECOND_AT + 4)
            .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            .filter(|per_second| *per_second > 0)
        else {
            return Duration::ZERO;
        };
        let pcm = self.fetched.len().saturating_sub(WAVE_HEADER_BYTES);
        Duration::from_secs_f64(pcm as f64 / f64::from(per_second))
    }
}

struct Fake {
    search_at: SocketAddr,
    heard: Arc<Mutex<Heard>>,
}

impl Fake {
    fn start() -> Self {
        let ssdp = UdpSocket::bind("127.0.0.1:0").expect("a loopback socket for the search");
        let http = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let search_at = ssdp.local_addr().expect("the search address");
        let described_at = http.local_addr().expect("the description address");
        let heard = Arc::new(Mutex::new(Heard::default()));

        let searched = Arc::clone(&heard);
        thread::spawn(move || answer_searches(&ssdp, described_at, &searched));
        let asked = Arc::clone(&heard);
        thread::spawn(move || {
            for stream in http.incoming().flatten() {
                let asked = Arc::clone(&asked);
                thread::spawn(move || answer(stream, &asked));
            }
        });

        Self { search_at, heard }
    }

    fn searching(&self) -> Searching {
        Searching {
            to: self.search_at,
            within: Duration::from_millis(300),
            every: Duration::from_secs(3_600),
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

fn answer_searches(ssdp: &UdpSocket, described_at: SocketAddr, heard: &Mutex<Heard>) {
    let mut buffer = [0; 2048];
    while let Ok((read, from)) = ssdp.recv_from(&mut buffer) {
        let asked = String::from_utf8_lossy(&buffer[..read]).into_owned();
        if !asked.starts_with("M-SEARCH") || !asked.contains("device:MediaRenderer:1") {
            continue;
        }
        heard.lock().searches += 1;
        let answer = format!(
            "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\nLOCATION: \
             http://{described_at}/device/description.xml\r\nST: \
             urn:schemas-upnp-org:device:MediaRenderer:1\r\nUSN: \
             {UDN}::urn:schemas-upnp-org:device:MediaRenderer:1\r\n\r\n"
        );
        let _ = ssdp.send_to(answer.as_bytes(), from);
    }
}

fn description() -> String {
    format!(
        r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
  <device>
    <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
    <friendlyName>{NAME}</friendlyName>
    <UDN>{UDN}</UDN>
    <serviceList>
      <service><serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType><controlURL>control/av</controlURL></service>
      <service><serviceType>urn:schemas-upnp-org:service:RenderingControl:1</serviceType><controlURL>/rc</controlURL></service>
      <service><serviceType>urn:schemas-upnp-org:service:ConnectionManager:1</serviceType><controlURL>/cm</controlURL></service>
    </serviceList>
  </device>
</root>"#
    )
}

fn answer(mut stream: TcpStream, heard: &Arc<Mutex<Heard>>) {
    let mut held = Vec::new();
    let mut buffer = [0; 4096];
    let head_ends = loop {
        if let Some(at) = held.windows(4).position(|window| window == b"\r\n\r\n") {
            break at + 4;
        }
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => held.extend_from_slice(&buffer[..read]),
        }
    };
    let head = String::from_utf8_lossy(&held[..head_ends]).into_owned();
    let mut lines = head.lines();
    let mut first = lines.next().unwrap_or_default().split(' ');
    let method = first.next().unwrap_or_default().to_owned();
    let path = first.next().unwrap_or_default().to_owned();
    let header = |name: &str| {
        head.lines().find_map(|line| {
            let (named, value) = line.split_once(':')?;
            named
                .trim()
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().to_owned())
        })
    };
    let length: usize = header("Content-Length")
        .and_then(|length| length.parse().ok())
        .unwrap_or(0);
    let mut body = held[head_ends..].to_vec();
    while body.len() < length {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => body.extend_from_slice(&buffer[..read]),
        }
    }
    let body = String::from_utf8_lossy(&body).into_owned();

    if method == "GET" && path == "/device/description.xml" {
        let description = description();
        let (first, second) = description.split_at(description.len() / 2);
        let chunked = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{first}\r\n{:x}\r\n{second}\r\n0\r\n\r\n",
            first.len(),
            second.len()
        );
        let _ = stream.write_all(chunked.as_bytes());
        return;
    }

    let action = header("SOAPAction")
        .and_then(|named| {
            named
                .trim_matches('"')
                .split_once('#')
                .map(|(_, action)| action.to_owned())
        })
        .unwrap_or_default();
    heard.lock().calls.push((action.clone(), body.clone()));

    let arguments = match action.as_str() {
        "GetProtocolInfo" => {
            "<Source></Source><Sink>http-get:*:audio/mpeg:*,http-get:*:audio/wav:*</Sink>"
                .to_owned()
        }
        "GetVolume" => "<CurrentVolume>40</CurrentVolume>".to_owned(),
        "SetAVTransportURI" => {
            let mut heard = heard.lock();
            heard.offered = between(&body, "<CurrentURI>", "</CurrentURI>");
            if let Some(offered) = heard.offered.clone() {
                heard.offers.push(offered);
            }
            String::new()
        }
        "Stop" => {
            heard.lock().fetching = None;
            String::new()
        }
        "Play" => {
            fetch(heard);
            String::new()
        }
        "GetPositionInfo" => {
            let heard_for = heard.lock().heard_for();
            format!("<Track>1</Track><RelTime>{}</RelTime>", clock(heard_for))
        }
        _ => String::new(),
    };
    let envelope = format!(
        "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body><u:{action}Response xmlns:u=\"urn:schemas-upnp-org:service:AVTransport:1\">{arguments}</u:{action}Response></s:Body></s:Envelope>"
    );
    let answer = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{envelope}",
        envelope.len()
    );
    let _ = stream.write_all(answer.as_bytes());
}

fn between(text: &str, open: &str, close: &str) -> Option<String> {
    let from = text.find(open)? + open.len();
    let to = text[from..].find(close)? + from;
    Some(text[from..to].replace("&amp;", "&"))
}

fn clock(heard: Duration) -> String {
    let seconds = heard.as_secs();
    format!(
        "{}:{:02}:{:02}.{:03}",
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60,
        heard.subsec_millis()
    )
}

fn fetch(heard: &Arc<Mutex<Heard>>) {
    let offered = {
        let mut heard = heard.lock();
        let Some(offered) = heard.offered.clone() else {
            return;
        };
        if heard.fetching.as_ref() == Some(&offered) {
            return;
        }
        heard.fetching = Some(offered.clone());
        heard.fetched.clear();
        heard.fetch_ended = false;
        offered
    };
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
        let mut head_ends = None;
        loop {
            let read = match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            };
            let mut heard = heard.lock();
            if heard.fetching.as_ref() != Some(&offered) {
                return;
            }
            if head_ends.is_some() {
                heard.fetched.extend_from_slice(&buffer[..read]);
            } else {
                held.extend_from_slice(&buffer[..read]);
                if let Some(at) = held.windows(4).position(|window| window == b"\r\n\r\n") {
                    assert!(held.starts_with(b"HTTP/1.1 200 OK\r\n"));
                    head_ends = Some(at + 4);
                    heard.fetched.extend_from_slice(&held[at + 4..]);
                }
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

struct Counting {
    next: u8,
    left: usize,
}

impl AudioSource for Counting {
    fn fill(&mut self, dst: &mut [u8]) -> usize {
        let filled = dst.len().min(self.left);
        for byte in &mut dst[..filled] {
            *byte = self.next;
            self.next = self.next.wrapping_add(1);
        }
        self.left -= filled;
        filled
    }
}

fn counted(bytes: usize) -> Vec<u8> {
    (0..bytes).map(|at| at as u8).collect()
}

fn stereo_sixteen() -> StreamSpec {
    StreamSpec::new(
        SampleRate::HZ_44100,
        ChannelLayout::Stereo,
        SampleFormat::S16,
    )
}

fn the_renderer(network: &Network) -> SinkInfo {
    let sinks = network.sinks();
    assert_eq!(
        sinks.len(),
        1,
        "the one renderer on the loopback was not found"
    );
    sinks.into_iter().next().expect("a renderer")
}

#[test]
fn a_renderer_answering_the_search_is_described_and_listed_as_a_sink() {
    let fake = Fake::start();

    let network = Network::start(fake.searching(), FirstSearch::Awaited).expect("searches");
    let sink = the_renderer(&network);

    assert_eq!(sink.name, NodeName::new(format!("upnp:{UDN}")));
    assert_eq!(sink.description, NAME);
    assert!(Network::owns(sink.id));
    assert!(sink.supports(stereo_sixteen()));
    assert!(sink.supports(StreamSpec::new(
        SampleRate::HZ_192000,
        ChannelLayout::Stereo,
        SampleFormat::S24
    )));
    assert!(!sink.supports(StreamSpec::new(
        SampleRate::HZ_44100,
        ChannelLayout::Stereo,
        SampleFormat::F32
    )));
    let port = sink.port.expect("a renderer names where it is");
    assert_eq!(port.hardware_volume, HardwareVolume::Always);
    let volume = Volume::heard_at(port.volume.expect("the renderer's volume was read"));
    assert!((volume.get() - 0.4).abs() < 1e-4);
    assert_eq!(
        fake.heard.lock().actions(),
        ["GetProtocolInfo", "GetVolume"]
    );
}

#[test]
fn a_stream_on_a_renderer_is_offered_by_address_played_paused_drained_and_stopped_and_fetched_as_written()
 {
    let fake = Fake::start();
    let network =
        Arc::new(Network::start(fake.searching(), FirstSearch::Awaited).expect("searches"));
    let sink = the_renderer(&network);
    let backend = Beside::over(Box::new(NoGraph::new()), Arc::clone(&network));
    let half_a_second = RATE as usize / 2 * STRIDE;

    let stream = backend
        .opener()
        .open(
            &StreamRequest {
                target: Some(sink.id),
                spec: stereo_sixteen(),
                latency: LatencyRequest::Auto,
                role: MediaRole::Music,
                media_name: "Songs & <Sounds>".to_owned(),
                force_graph_rate: false,
                no_convert: true,
                realtime: true,
            },
            Box::new(Counting {
                next: 0,
                left: half_a_second,
            }),
        )
        .expect("the renderer takes the stream");
    stream.set_active(true).expect("plays");
    fake.until("the whole stream to be fetched", |heard| {
        heard.fetched.len() >= WAVE_HEADER_BYTES + half_a_second
    });
    stream.drain().expect("drains");
    let drained = drained_within(stream.events());
    stream.set_active(false).expect("pauses");
    fake.until("the renderer to be paused", |heard| {
        heard.body_of("Pause").is_some()
    });
    backend
        .set_device_volume(sink.id, Volume::new(0.5).expect("a volume").to_gain())
        .expect("turns");
    fake.until("the renderer to be turned", |heard| {
        heard.body_of("SetVolume").is_some()
    });
    stream.close().expect("closes");
    fake.until("the renderer to be stopped", |heard| {
        heard.body_of("Stop").is_some()
    });

    let heard = fake.heard.lock();
    assert_eq!(drained, Some(StreamEvent::Drained));
    assert_eq!(
        heard.actions(),
        [
            "GetProtocolInfo",
            "GetVolume",
            "SetAVTransportURI",
            "Play",
            "Pause",
            "SetVolume",
            "Stop"
        ]
    );
    let offered = heard.offered.as_deref().expect("an address was offered");
    assert!(
        offered.starts_with("http://127.0.0.1:"),
        "offered {offered}"
    );
    let set = heard.body_of("SetAVTransportURI").expect("set");
    assert!(set.contains("protocolInfo=&quot;http-get:*:audio/wav:*&quot;"));
    assert!(set.contains("Songs &amp;amp; &amp;lt;Sounds&amp;gt;"));
    assert!(
        heard
            .body_of("SetVolume")
            .is_some_and(|body| body.contains("<DesiredVolume>50</DesiredVolume>"))
    );
    assert!(heard.fetch_ended, "the fetch was not ended once drained");
    assert_eq!(&heard.fetched[..4], b"RIFF");
    assert_eq!(
        u32::from_le_bytes(heard.fetched[24..28].try_into().expect("four bytes")),
        RATE
    );
    assert_eq!(
        &heard.fetched[WAVE_HEADER_BYTES..WAVE_HEADER_BYTES + half_a_second],
        counted(half_a_second).as_slice()
    );
}

struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = env::temp_dir().join(format!(
            "resonate-upnp-{}-{}",
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

fn playing_on(network: &Arc<Network>) -> Player {
    let played = Arc::clone(network);
    Player::with_backend(
        EngineConfig {
            sink: Some(NodeName::new(format!("upnp:{UDN}"))),
            buffer: Duration::from_millis(200),
            ..EngineConfig::default()
        },
        move |_| Ok(Box::new(Beside::over(Box::new(NoGraph::new()), played))),
    )
    .expect("a player")
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

fn pcm_bytes(heard: Duration) -> usize {
    (heard.as_secs_f64() * f64::from(RATE)) as usize * STRIDE
}

#[test]
fn a_song_played_through_the_engine_to_a_renderer_reaches_it_bit_for_bit() {
    let fake = Fake::start();
    let network =
        Arc::new(Network::start(fake.searching(), FirstSearch::Awaited).expect("searches"));
    let tree = Tree::new();
    let pcm = music(RATE as usize * 3 / 4);
    let path = tree.write("song.wav", &wave_file(&pcm));
    let player = playing_on(&network);

    loaded(&player, &path);
    finished(&player);
    fake.until("the fetch to take the whole song", |heard| {
        contains(&heard.fetched, &pcm)
    });

    let heard = fake.heard.lock();
    assert!(heard.actions().starts_with(&[
        "GetProtocolInfo",
        "GetVolume",
        "SetAVTransportURI",
        "Play"
    ]));
}

#[test]
fn a_pause_and_a_volume_change_reach_a_renderer_at_once_and_the_song_resumes_bit_for_bit() {
    let fake = Fake::start();
    let network =
        Arc::new(Network::start(fake.searching(), FirstSearch::Awaited).expect("searches"));
    let tree = Tree::new();
    let pcm = music(RATE as usize * 3 / 2);
    let path = tree.write("song.wav", &wave_file(&pcm));
    let player = playing_on(&network);

    loaded(&player, &path);
    fake.until("half a second to be fetched", |heard| {
        heard.fetched.len() >= WAVE_HEADER_BYTES + pcm_bytes(Duration::from_millis(500))
    });
    let paused_at = Instant::now();
    player.send(Command::Pause).expect("pauses");
    fake.until("the renderer to be paused", |heard| {
        heard.body_of("Pause").is_some()
    });
    let pausing_took = paused_at.elapsed();
    let turned_at = Instant::now();
    player
        .send(Command::SetVolume(Volume::new(0.5).expect("a volume")))
        .expect("turns");
    fake.until("the renderer to be turned", |heard| {
        heard
            .body_of("SetVolume")
            .is_some_and(|body| body.contains("<DesiredVolume>50</DesiredVolume>"))
    });
    let turning_took = turned_at.elapsed();
    player.send(Command::Play).expect("plays on");
    finished(&player);
    fake.until("the fetch to take the whole song", |heard| {
        contains(&heard.fetched, &pcm)
    });

    let heard = fake.heard.lock();
    assert!(
        pausing_took < SOONER_THAN,
        "the pause took {pausing_took:?} to reach the renderer"
    );
    assert!(
        turning_took < SOONER_THAN,
        "the volume took {turning_took:?} to reach the renderer"
    );
    assert_eq!(heard.offers.len(), 1, "a pause asked for a new address");
    let actions = heard.actions();
    let paused = actions.iter().position(|action| *action == "Pause");
    let played_again = actions.iter().rposition(|action| *action == "Play");
    assert!(paused < played_again, "{actions:?}");
}

#[test]
fn a_seek_on_a_renderer_stops_it_and_offers_a_fresh_address_starting_where_it_landed() {
    let fake = Fake::start();
    let network =
        Arc::new(Network::start(fake.searching(), FirstSearch::Awaited).expect("searches"));
    let tree = Tree::new();
    let pcm = music(RATE as usize * 3);
    let path = tree.write("song.wav", &wave_file(&pcm));
    let player = playing_on(&network);
    let lands_at = Duration::from_secs(2);

    loaded(&player, &path);
    fake.until("a quarter second to be fetched", |heard| {
        heard.fetched.len() >= WAVE_HEADER_BYTES + pcm_bytes(Duration::from_millis(250))
    });
    let sought_at = Instant::now();
    player
        .send(Command::Seek(Frames::from_duration(
            lands_at,
            SampleRate::HZ_44100,
        )))
        .expect("seeks");
    fake.until("a second address to be fetched", |heard| {
        heard.offers.len() >= 2 && heard.fetching == heard.offers.last().cloned()
    });
    let seeking_took = sought_at.elapsed();
    finished(&player);
    let rest = &pcm[pcm_bytes(lands_at + PAST_THE_FADE_IN)..];
    fake.until("the fetch to take the song from where it landed", |heard| {
        contains(&heard.fetched, rest)
    });

    let heard = fake.heard.lock();
    assert!(
        seeking_took < SOONER_THAN,
        "the seek took {seeking_took:?} to reach the renderer"
    );
    assert_ne!(heard.offers[0], heard.offers[1]);
    let actions = heard.actions();
    let stopped = actions
        .iter()
        .position(|action| *action == "Stop")
        .expect("the renderer was stopped");
    assert_eq!(
        actions[stopped..stopped + 3],
        ["Stop", "SetAVTransportURI", "Play"],
        "{actions:?}"
    );
    assert!(
        heard.fetched.len() - WAVE_HEADER_BYTES <= pcm.len() - pcm_bytes(lands_at),
        "the fresh address carried more than lies past where the seek landed"
    );
}

fn queued(path: &Path) -> QueueItem {
    QueueItem {
        id: TrackId::new(1).expect("a non-zero id"),
        location: MediaLocation::local(path),
        span: None,
    }
}

fn contains(fetched: &[u8], pcm: &[u8]) -> bool {
    let Some(fetched) = fetched.get(WAVE_HEADER_BYTES..) else {
        return false;
    };
    let opening = &pcm[..STRIDE * 4];
    fetched
        .windows(opening.len())
        .enumerate()
        .filter(|(_, window)| *window == opening)
        .any(|(at, _)| fetched.get(at..at + pcm.len()) == Some(pcm))
}

fn drained_within(events: &Receiver<StreamEvent>) -> Option<StreamEvent> {
    let since = Instant::now();
    while since.elapsed() < WAITED_AT_MOST {
        if let Ok(event) = events.recv_timeout(LOOKS_EVERY)
            && event == StreamEvent::Drained
        {
            return Some(event);
        }
    }
    None
}
