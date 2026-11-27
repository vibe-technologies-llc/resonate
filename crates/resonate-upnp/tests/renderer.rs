use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, Read, Write},
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
    AudioSource, Backend, Error as SinkError, HardwareVolume, LatencyRequest, Learnt, MediaRole,
    Opener, Operation, Plays, SinkChange, SinkId, SinkInfo, SinkResult, SinkStream, StreamEvent,
    StreamRequest, StreamState, Surveyor, Trial, TrialVerdict, Unplayed,
};
use resonate_codec::{DecodeStatus, Decoder, Sources};
use resonate_core::{
    AudioBuffer, ChannelLayout, Frames, Gain, MediaLocation, SampleFormat, SampleRate, StreamSpec,
    TrackId, Trim, Volume,
};
use resonate_engine::{
    Command, EngineConfig, Equalisation, Error as EngineError, Event, Levelling, NodeName, Player,
    Preamp, Profile as EqProfile, QueueItem, ReplayGainMode,
};
use resonate_upnp::{
    Action, Asked, Beside, Error as UpnpError, FirstSearch, Located, Neighbours, Network,
    Searching, Udn, Vouched,
};

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

const DRAGGED_EVERY: Duration = Duration::from_millis(40);

const RESTARTED_WITHIN: Duration = Duration::from_millis(1_500);

const QUIET_FOR: Duration = Duration::from_millis(900);

const STALLED_FOR: Duration = Duration::from_secs(10);

const ENDLESS_STOPS_AFTER_CHUNKS: usize = 8 << 10;

const WAVE_ONLY: &str = "http-get:*:audio/mpeg:*,http-get:*:audio/wav:*";

#[derive(Clone, Copy, PartialEq, Eq)]
enum On {
    Description,
    Action(&'static str),
}

#[derive(Clone, Copy)]
enum Misbehaviour {
    Fault(u32),
    Status(u16),
    Stall,
    Endless,
    Body(usize),
    Head(usize),
}

#[derive(Clone, Copy)]
struct Fault {
    on: On,
    how: Misbehaviour,
}

#[derive(Clone, Copy)]
struct Profile {
    sink: &'static str,
    openhome: bool,
    fault: Option<Fault>,
    name_letters: Option<usize>,
}

impl Profile {
    const fn wave() -> Self {
        Self {
            sink: WAVE_ONLY,
            openhome: false,
            fault: None,
            name_letters: None,
        }
    }

    const fn faulting(on: On, how: Misbehaviour) -> Self {
        Self {
            fault: Some(Fault { on, how }),
            ..Self::wave()
        }
    }

    const fn named_by(letters: usize) -> Self {
        Self {
            name_letters: Some(letters),
            ..Self::wave()
        }
    }
}

impl Default for Profile {
    fn default() -> Self {
        Self::wave()
    }
}

#[derive(Default)]
struct Heard {
    profile: Profile,
    openhome: Vec<(String, String)>,
    fetch_began: Option<Instant>,
    calls: Vec<(String, String)>,
    offered: Option<String>,
    offers: Vec<String>,
    fetched: Vec<u8>,
    fetching: Option<String>,
    fetch_ended: bool,
    searches: usize,
    udn: Option<&'static str>,
    answers_from: Option<SocketAddr>,
    decodes_at_most: Option<(u32, u16)>,
    undecodable: bool,
}

impl Heard {
    fn decodes(&self) -> Option<bool> {
        let (hz, channels) = self.decodes_at_most?;
        let (rate, count) = shape_of(&self.fetched)?;
        Some(rate <= hz && count <= channels)
    }

    fn times(&self, action: &str) -> usize {
        self.calls
            .iter()
            .filter(|(called, _)| called == action)
            .count()
    }

    fn actions(&self) -> Vec<&str> {
        self.calls
            .iter()
            .map(|(action, _)| action.as_str())
            .filter(|action| *action != "GetPositionInfo" && *action != "GetTransportInfo")
            .collect()
    }

    fn body_of(&self, action: &str) -> Option<&str> {
        self.calls
            .iter()
            .find(|(called, _)| called == action)
            .map(|(_, body)| body.as_str())
    }

    fn heard_for(&self) -> Duration {
        if !self.fetched.starts_with(b"RIFF") {
            return self
                .fetch_began
                .map(|began| began.elapsed())
                .unwrap_or_default();
        }

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
    described_at: SocketAddr,
    heard: Arc<Mutex<Heard>>,
}

impl Fake {
    fn start() -> Self {
        Self::with(Profile::wave())
    }

    fn with(profile: Profile) -> Self {
        let ssdp = UdpSocket::bind("127.0.0.1:0").expect("a loopback socket for the search");
        let http = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let search_at = ssdp.local_addr().expect("the search address");
        let described_at = http.local_addr().expect("the description address");
        let heard = Arc::new(Mutex::new(Heard {
            profile,
            ..Heard::default()
        }));

        let searched = Arc::clone(&heard);

        thread::spawn(move || answer_searches(&ssdp, described_at, &searched));

        let asked = Arc::clone(&heard);

        thread::spawn(move || {
            for stream in http.incoming().flatten() {
                let asked = Arc::clone(&asked);

                thread::spawn(move || answer(stream, &asked));
            }
        });

        Self {
            search_at,
            described_at,
            heard,
        }
    }

    fn searching(&self) -> Searching {
        Searching {
            to: self.search_at,
            within: Duration::from_millis(300),
            every: Duration::from_secs(3_600),
            serving_on: IpAddr::V4(Ipv4Addr::LOCALHOST),
        }
    }

    fn decoding_at_most(self, hz: u32, channels: u16) -> Self {
        self.heard.lock().decodes_at_most = Some((hz, channels));
        self
    }

    fn heal(&self) {
        self.heard.lock().profile.fault = None;
    }

    fn description_address(&self) -> Located {
        Located::parse(&format!(
            "http://{}/device/description.xml",
            self.described_at
        ))
        .expect("the fake's description address reads")
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

        let (udn, described_at) = {
            let mut held = heard.lock();

            held.searches += 1;
            (
                held.udn.unwrap_or(UDN),
                held.answers_from.unwrap_or(described_at),
            )
        };
        let answer = format!(
            "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\nLOCATION: \
             http://{described_at}/device/description.xml\r\nST: \
             urn:schemas-upnp-org:device:MediaRenderer:1\r\nUSN: \
             {udn}::urn:schemas-upnp-org:device:MediaRenderer:1\r\n\r\n"
        );
        let _ = ssdp.send_to(answer.as_bytes(), from);
    }
}

fn description(openhome: bool, udn: &str, name: &str) -> String {
    let extra = if openhome {
        r#"<service><serviceType>urn:av-openhome-org:service:Product:1</serviceType><controlURL>/oh/product</controlURL></service>
      <service><serviceType>urn:av-openhome-org:service:Playlist:1</serviceType><controlURL>/oh/playlist</controlURL></service>
      <service><serviceType>urn:av-openhome-org:service:Volume:1</serviceType><controlURL>/oh/volume</controlURL></service>"#
    } else {
        ""
    };
    format!(
        r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
  <device>
    <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
    <friendlyName>{name}</friendlyName>
    <UDN>{udn}</UDN>
    <serviceList>
      <service><serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType><controlURL>control/av</controlURL></service>
      <service><serviceType>urn:schemas-upnp-org:service:RenderingControl:1</serviceType><controlURL>/rc</controlURL></service>
      <service><serviceType>urn:schemas-upnp-org:service:ConnectionManager:1</serviceType><controlURL>/cm</controlURL></service>
      {extra}
    </serviceList>
  </device>
</root>"#
    )
}

fn padded(mut text: String, bytes: usize) -> String {
    text.push_str(&" ".repeat(bytes.saturating_sub(text.len())));
    text
}

fn chunked_in_two(body: &str) -> Vec<u8> {
    let (first, second) = body.as_bytes().split_at(body.len() / 2);

    let mut chunked = format!("{:x}\r\n", first.len()).into_bytes();

    chunked.extend_from_slice(first);
    chunked.extend_from_slice(format!("\r\n{:x}\r\n", second.len()).as_bytes());
    chunked.extend_from_slice(second);
    chunked.extend_from_slice(b"\r\n0\r\n\r\n");
    chunked
}

fn on_the_wire(body: &str, chunked: bool) -> Vec<u8> {
    if chunked {
        chunked_in_two(body)
    } else {
        body.as_bytes().to_vec()
    }
}

fn padded_on_the_wire_to(body: &str, bytes: usize, chunked: bool) -> String {
    let trial = padded(body.to_owned(), bytes);
    let framing = on_the_wire(&trial, chunked).len() - trial.len();

    padded(body.to_owned(), bytes.saturating_sub(framing))
}

fn answered_whole(body: &str, how: Option<Misbehaviour>, chunked: bool) -> Vec<u8> {
    let (body, padding) = match how {
        Some(Misbehaviour::Body(bytes)) => {
            (padded_on_the_wire_to(body, bytes, chunked), String::new())
        }
        Some(Misbehaviour::Head(bytes)) => (
            body.to_owned(),
            format!("X-Padding: {}\r\n", "a".repeat(bytes)),
        ),
        _ => (body.to_owned(), String::new()),
    };

    let mut answer = if chunked {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\n{padding}Transfer-Encoding: chunked\r\n\r\n"
        )
        .into_bytes()
    } else {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\n{padding}Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes()
    };

    answer.extend_from_slice(&on_the_wire(&body, chunked));
    answer
}

fn fault_envelope(code: u32) -> String {
    format!(
        "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body><s:Fault><faultcode>s:Client</faultcode><faultstring>UPnPError</faultstring><detail><UPnPError xmlns=\"urn:schemas-upnp-org:control-1-0\"><errorCode>{code}</errorCode><errorDescription>Action Failed</errorDescription></UPnPError></detail></s:Fault></s:Body></s:Envelope>"
    )
}

fn stalled(stream: &mut TcpStream) {
    let mut buffer = [0; 64];

    let _ = stream.set_read_timeout(Some(STALLED_FOR));

    while matches!(stream.read(&mut buffer), Ok(read) if read > 0) {}
}

fn endless(stream: &mut TcpStream) {
    let chunk = [b'a'; 8 << 10];

    if stream
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\n\r\n")
        .is_err()
    {
        return;
    }
    for _ in 0..ENDLESS_STOPS_AFTER_CHUNKS {
        if stream.write_all(&chunk).is_err() {
            return;
        }
    }
}

fn misbehaved_early(stream: &mut TcpStream, how: Option<Misbehaviour>) -> bool {
    match how {
        Some(Misbehaviour::Fault(code)) => {
            let envelope = fault_envelope(code);
            let answer = format!(
                "HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{envelope}",
                envelope.len()
            );
            let _ = stream.write_all(answer.as_bytes());
            true
        }
        Some(Misbehaviour::Status(status)) => {
            let answer = format!(
                "HTTP/1.1 {status} Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = stream.write_all(answer.as_bytes());
            true
        }
        Some(Misbehaviour::Stall) => {
            stalled(stream);
            true
        }
        Some(Misbehaviour::Endless) => {
            endless(stream);
            true
        }
        Some(Misbehaviour::Body(_) | Misbehaviour::Head(_)) | None => false,
    }
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
    let action = header("SOAPAction")
        .and_then(|named| {
            named
                .trim_matches('"')
                .split_once('#')
                .map(|(_, action)| action.to_owned())
        })
        .unwrap_or_default();

    let describing = method == "GET" && path == "/device/description.xml";
    let misbehaving = {
        let held = heard.lock();

        held.profile
            .fault
            .filter(|fault| match fault.on {
                On::Description => describing,
                On::Action(named) => !describing && named == action,
            })
            .map(|fault| fault.how)
    };

    if describing {
        if misbehaved_early(&mut stream, misbehaving) {
            return;
        }

        let (openhome, udn, name) = {
            let held = heard.lock();
            (
                held.profile.openhome,
                held.udn.unwrap_or(UDN),
                held.profile
                    .name_letters
                    .map_or_else(|| NAME.to_owned(), |letters| "é".repeat(letters)),
            )
        };

        let _ = stream.write_all(&answered_whole(
            &description(openhome, udn, &name),
            misbehaving,
            true,
        ));

        return;
    }

    if path.starts_with("/oh/") {
        heard.lock().openhome.push((action.clone(), body.clone()));

        if misbehaved_early(&mut stream, misbehaving) {
            return;
        }

        let arguments = match action.as_str() {
            "Characteristics" => "<VolumeMax>50</VolumeMax><VolumeUnity>25</VolumeUnity>",
            "Volume" => "<Value>20</Value>",
            _ => "",
        };
        let envelope = format!(
            "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body><u:{action}Response xmlns:u=\"urn:av-openhome-org:service:Volume:1\">{arguments}</u:{action}Response></s:Body></s:Envelope>"
        );
        let _ = stream.write_all(&answered_whole(&envelope, misbehaving, false));

        return;
    }
    heard.lock().calls.push((action.clone(), body.clone()));

    if misbehaved_early(&mut stream, misbehaving) {
        return;
    }

    let arguments = match action.as_str() {
        "GetProtocolInfo" => format!(
            "<Source></Source><Sink>{}</Sink>",
            heard.lock().profile.sink
        ),
        "GetVolume" => "<CurrentVolume>40</CurrentVolume>".to_owned(),
        "SetAVTransportURI" => {
            let mut heard = heard.lock();

            heard.offered = between(&body, "<CurrentURI>", "</CurrentURI>");
            heard.undecodable = false;
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
        "GetTransportInfo" => {
            let heard = heard.lock();
            let (state, status) = match (heard.undecodable, heard.fetching.is_some()) {
                (true, _) => ("STOPPED", "ERROR_OCCURRED"),
                (false, true) => ("PLAYING", "OK"),
                (false, false) => ("STOPPED", "OK"),
            };
            format!(
                "<CurrentTransportState>{state}</CurrentTransportState><CurrentTransportStatus>{status}</CurrentTransportStatus><CurrentSpeed>1</CurrentSpeed>"
            )
        }
        "GetPositionInfo" => {
            let held = heard.lock();
            let heard_for = if held.undecodable {
                Duration::ZERO
            } else {
                held.heard_for()
            };

            drop(held);
            format!("<Track>1</Track><RelTime>{}</RelTime>", clock(heard_for))
        }
        _ => String::new(),
    };
    let envelope = format!(
        "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body><u:{action}Response xmlns:u=\"urn:schemas-upnp-org:service:AVTransport:1\">{arguments}</u:{action}Response></s:Body></s:Envelope>"
    );
    let _ = stream.write_all(&answered_whole(&envelope, misbehaving, false));
}

fn shape_of(fetched: &[u8]) -> Option<(u32, u16)> {
    if fetched.starts_with(b"fLaC") {
        let info = fetched.get(18..21)?;
        let rate = u32::from(info[0]) << 12 | u32::from(info[1]) << 4 | u32::from(info[2]) >> 4;
        let channels = u16::from((info[2] >> 1) & 0b111) + 1;

        return Some((rate, channels));
    }
    if fetched.starts_with(b"RIFF") {
        let channels = u16::from_le_bytes([*fetched.get(22)?, *fetched.get(23)?]);
        let rate = u32::from_le_bytes(fetched.get(24..28)?.try_into().ok()?);

        return Some((rate, channels));
    }
    None
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
        heard.fetch_began = Some(Instant::now());
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
            if heard.decodes() == Some(false) {
                heard.undecodable = true;
                heard.fetch_ended = true;
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
    let backend = Beside::over(Box::new(NoGraph::new()), Arc::<Network>::clone(&network));
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

fn preamped(decibels: f64) -> Arc<Equalisation> {
    Arc::new(Equalisation {
        enabled: true,
        bound: BTreeMap::new(),
        fallback: Some(Arc::new(
            EqProfile::new(
                Preamp::from_decibels(decibels).expect("in range"),
                Vec::new(),
            )
            .expect("a profile of a preamp alone"),
        )),
    })
}

fn loudness_of(fetched: &[u8], from: Duration, lasting: Duration) -> f64 {
    let start = WAVE_HEADER_BYTES + pcm_bytes(from);
    let end = start + pcm_bytes(lasting);

    let lefts: Vec<f64> = fetched[start..end]
        .as_chunks::<STRIDE>()
        .0
        .iter()
        .map(|frame| f64::from(i16::from_le_bytes([frame[0], frame[1]])).abs())
        .collect();
    lefts.iter().sum::<f64>() / lefts.len() as f64
}

fn pcm_as_fetched(pcm: &[u8]) -> Vec<u8> {
    let mut fetched = vec![0; WAVE_HEADER_BYTES];

    fetched.extend_from_slice(pcm);
    fetched
}

fn stops_and_offers(heard: &Heard) -> (usize, usize) {
    let stops = heard
        .actions()
        .iter()
        .filter(|action| **action == "Stop")
        .count();
    (stops, heard.offers.len())
}

#[test]
fn an_equaliser_change_dragged_on_a_renderer_restarts_its_stream_once_from_where_it_is_heard() {
    let fake = Fake::start();
    let network =
        Arc::new(Network::start(fake.searching(), FirstSearch::Awaited).expect("searches"));
    let tree = Tree::new();
    let pcm = music(RATE as usize * 4);

    let path = tree.write("song.wav", &wave_file(&pcm));
    let player = playing_on(&network);
    let unchanged = loudness_of(
        &pcm_as_fetched(&pcm),
        Duration::ZERO,
        Duration::from_secs(3),
    );

    loaded(&player, &path);
    fake.until("a quarter second to be fetched", |heard| {
        heard.fetched.len() >= WAVE_HEADER_BYTES + pcm_bytes(Duration::from_millis(250))
    });

    let before = loudness_of(
        &fake.heard.lock().fetched,
        PAST_THE_FADE_IN,
        Duration::from_millis(150),
    );

    for decibels in [-1.0, -2.0, -3.0, -4.0, -5.0, -6.0] {
        player
            .send(Command::SetEqualisation(preamped(decibels)))
            .expect("equalises");
        thread::sleep(DRAGGED_EVERY);
    }

    let dragged_at = Instant::now();

    fake.until("a second address to be fetched", |heard| {
        heard.offers.len() >= 2 && heard.fetching == heard.offers.last().cloned()
    });

    let restarting_took = dragged_at.elapsed();

    fake.until("a second of the fresh address to be fetched", |heard| {
        heard.fetched.len() >= WAVE_HEADER_BYTES + pcm_bytes(Duration::from_secs(1))
    });

    let after = loudness_of(
        &fake.heard.lock().fetched,
        PAST_THE_FADE_IN * 4,
        Duration::from_millis(500),
    );

    thread::sleep(QUIET_FOR);
    player
        .send(Command::SetVolume(Volume::new(0.5).expect("a volume")))
        .expect("turns");
    thread::sleep(QUIET_FOR);

    let heard = fake.heard.lock();
    let actions = heard.actions();
    let ratio = after / unchanged;

    assert!(
        restarting_took < RESTARTED_WITHIN,
        "the drag took {restarting_took:?} to restart the stream"
    );
    assert!(
        (before / unchanged - 1.0).abs() < 0.05,
        "the stream was changed before the equaliser: {before} against {unchanged}"
    );
    assert!(
        (0.45..0.56).contains(&ratio),
        "the fresh address was not the song at -6 dB: {ratio}"
    );
    assert_ne!(heard.offers[0], heard.offers[1]);
    assert_eq!(
        stops_and_offers(&heard),
        (1, 2),
        "a drag or a volume change restarted the stream more than once: {actions:?}"
    );

    let stopped = actions
        .iter()
        .position(|action| *action == "Stop")
        .expect("the renderer was stopped");

    assert_eq!(
        actions[stopped..stopped + 3],
        ["Stop", "SetAVTransportURI", "Play"],
        "{actions:?}"
    );
}

#[test]
fn a_replay_gain_change_on_a_renderer_restarts_its_stream_and_the_fresh_address_carries_it() {
    let fake = Fake::start();
    let network =
        Arc::new(Network::start(fake.searching(), FirstSearch::Awaited).expect("searches"));
    let tree = Tree::new();
    let pcm = music(RATE as usize * 4);

    let path = tree.write("song.wav", &wave_file(&pcm));
    let player = playing_on(&network);
    let unchanged = loudness_of(
        &pcm_as_fetched(&pcm),
        Duration::ZERO,
        Duration::from_secs(3),
    );

    loaded(&player, &path);
    fake.until("a quarter second to be fetched", |heard| {
        heard.fetched.len() >= WAVE_HEADER_BYTES + pcm_bytes(Duration::from_millis(250))
    });
    player
        .send(Command::SetLevelling(Levelling {
            untagged: Trim::from_decibels(-6.0).expect("a trim"),
            ..Levelling::default()
        }))
        .expect("levels");
    player
        .send(Command::SetReplayGain(ReplayGainMode::Track))
        .expect("levels");
    fake.until("a second address to be fetched", |heard| {
        heard.offers.len() >= 2 && heard.fetching == heard.offers.last().cloned()
    });
    fake.until("a second of the fresh address to be fetched", |heard| {
        heard.fetched.len() >= WAVE_HEADER_BYTES + pcm_bytes(Duration::from_secs(1))
    });

    let after = loudness_of(
        &fake.heard.lock().fetched,
        PAST_THE_FADE_IN * 4,
        Duration::from_millis(500),
    );

    thread::sleep(QUIET_FOR);

    let heard = fake.heard.lock();
    let ratio = after / unchanged;

    assert!(
        (0.45..0.56).contains(&ratio),
        "the fresh address was not the song at -6 dB: {ratio}"
    );
    assert_eq!(stops_and_offers(&heard), (1, 2), "{:?}", heard.actions());
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
    within(fetched, pcm)
}

fn within(fetched: &[u8], pcm: &[u8]) -> bool {
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

const FLAC_AND_WAVE: &str =
    "http-get:*:audio/mpeg:*,http-get:*:audio/wav:*,http-get:*:audio/flac:*";

const FLAC_WITH_SIX_CHANNELS: &str =
    "http-get:*:audio/flac;channels=2:*,http-get:*:audio/flac;channels=6:*";

const WAVE_WITH_SIX_CHANNELS: &str =
    "http-get:*:audio/wav;channels=2:*,http-get:*:audio/wav;channels=6:*";

const EXTENSIBLE_HEADER_BYTES: usize = 68;

const FRONT_AND_REAR_AND_CENTRE_AND_LFE: u32 = 0x3F;

fn decoded(tree: &Tree, flac: &[u8], format: SampleFormat) -> Vec<u8> {
    let path = tree.write("fetched.flac", flac);
    let (mut decoder, info) =
        Decoder::open(&Sources::local(), &MediaLocation::local(&path)).expect("a FLAC stream");

    decoder.set_output_format(format);

    let mut block = AudioBuffer::empty(StreamSpec::new(info.spec.rate, info.spec.channels, format));
    let mut samples = Vec::new();

    while decoder.next_block(&mut block).expect("a decoded block") != DecodeStatus::EndOfStream {
        samples.extend_from_slice(block.as_bytes());
    }
    samples
}

fn decoded_by_the_flac_tool(tree: &Tree, flac: &[u8]) -> Option<Vec<u8>> {
    let path = tree.write("by-the-tool.flac", flac);
    let output = process::Command::new("flac")
        .args([
            "--decode",
            "--silent",
            "--stdout",
            "--force-raw-output",
            "--endian=little",
            "--sign=signed",
        ])
        .arg(&path)
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

struct Pattern {
    channels: u32,
    frame: u32,
    frames: u32,
}

impl Pattern {
    fn word(frame: u32, channel: u32) -> i32 {
        ((frame * 131 + channel * 100_003) % 8_000_000) as i32 - 4_000_000
    }

    fn expected(channels: u32, frames: u32) -> Vec<u8> {
        let mut bytes = Vec::new();

        for frame in 0..frames {
            for channel in 0..channels {
                bytes.extend_from_slice(&Self::word(frame, channel).to_le_bytes());
            }
        }
        bytes
    }
}

impl AudioSource for Pattern {
    fn fill(&mut self, dst: &mut [u8]) -> usize {
        let stride = self.channels as usize * 4;
        let mut filled = 0;

        while filled + stride <= dst.len() && self.frame < self.frames {
            for channel in 0..self.channels {
                let at = filled + channel as usize * 4;

                dst[at..at + 4].copy_from_slice(&Self::word(self.frame, channel).to_le_bytes());
            }
            filled += stride;
            self.frame += 1;
        }
        filled
    }
}

fn packed_words(words: &[u8]) -> Vec<u8> {
    words
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|word| word[..3].to_vec())
        .collect()
}

fn pattern_stream(
    backend: &Beside,
    sink: &SinkInfo,
    spec: StreamSpec,
    frames: u32,
) -> SinkResult<SinkStream> {
    backend.opener().open(
        &StreamRequest {
            target: Some(sink.id),
            spec,
            latency: LatencyRequest::Auto,
            role: MediaRole::Music,
            media_name: "Pattern".to_owned(),
            force_graph_rate: false,
            no_convert: true,
            realtime: true,
        },
        Box::new(Pattern {
            channels: u32::from(spec.channel_count().get()),
            frame: 0,
            frames,
        }),
    )
}

fn surround_stream(backend: &Beside, sink: &SinkInfo, frames: u32) -> SinkResult<SinkStream> {
    pattern_stream(
        backend,
        sink,
        StreamSpec::new(
            SampleRate::HZ_48000,
            ChannelLayout::Surround51,
            SampleFormat::S24,
        ),
        frames,
    )
}

#[test]
fn a_song_played_to_a_renderer_taking_flac_is_sent_flac_that_decodes_to_the_samples_played() {
    let fake = Fake::with(Profile {
        sink: FLAC_AND_WAVE,
        openhome: false,
        ..Profile::wave()
    });
    let network =
        Arc::new(Network::start(fake.searching(), FirstSearch::Awaited).expect("searches"));
    let tree = Tree::new();
    let pcm = music(RATE as usize * 3 / 4);

    let path = tree.write("song.wav", &wave_file(&pcm));
    let player = playing_on(&network);

    loaded(&player, &path);
    finished(&player);
    fake.until("the fetch to end once drained", |heard| heard.fetch_ended);

    let heard = fake.heard.lock();
    let offered = heard.offered.as_deref().expect("an address was offered");

    assert!(offered.ends_with(".flac"), "offered {offered}");
    assert!(
        heard
            .body_of("SetAVTransportURI")
            .is_some_and(|body| body.contains("protocolInfo=&quot;http-get:*:audio/flac:*&quot;"))
    );
    assert_eq!(&heard.fetched[..4], b"fLaC");
    assert!(
        heard.fetched.len() < pcm.len(),
        "{} bytes of FLAC for {} of PCM",
        heard.fetched.len(),
        pcm.len()
    );

    let samples = decoded(&tree, &heard.fetched, SampleFormat::S16);

    assert!(within(&samples, &pcm), "the decoded stream lacks the song");
    if let Some(by_the_tool) = decoded_by_the_flac_tool(&tree, &heard.fetched) {
        assert_eq!(by_the_tool, samples, "two decoders disagree on the stream");
    }
}

#[test]
fn a_renderer_that_states_channels_is_sent_all_six_of_a_surround_stream_in_flac_and_in_wave() {
    let frames = 48_000 / 2;
    let expected = Pattern::expected(6, frames);

    let flac = Fake::with(Profile {
        sink: FLAC_WITH_SIX_CHANNELS,
        openhome: false,
        ..Profile::wave()
    });
    let network = Network::start(flac.searching(), FirstSearch::Awaited).expect("searches");
    let sink = the_renderer(&network);

    assert!(sink.supports(StreamSpec::new(
        SampleRate::HZ_48000,
        ChannelLayout::Surround51,
        SampleFormat::S24
    )));

    let backend = Beside::over(Box::new(NoGraph::new()), Arc::new(network));
    let stream = surround_stream(&backend, &sink, frames).expect("the renderer takes six channels");

    stream.set_active(true).expect("plays");
    stream.drain().expect("drains");
    flac.until("the fetch to end once drained", |heard| heard.fetch_ended);
    stream.close().expect("closes");

    let tree = Tree::new();
    let heard = flac.heard.lock();

    assert_eq!(&heard.fetched[..4], b"fLaC");
    assert_eq!(
        decoded(&tree, &heard.fetched, SampleFormat::S24),
        expected,
        "the six channels did not come back from the FLAC as they went in"
    );

    drop(heard);

    let wave = Fake::with(Profile {
        sink: WAVE_WITH_SIX_CHANNELS,
        openhome: false,
        ..Profile::wave()
    });
    let network = Network::start(wave.searching(), FirstSearch::Awaited).expect("searches");
    let sink = the_renderer(&network);
    let backend = Beside::over(Box::new(NoGraph::new()), Arc::new(network));

    let stream = surround_stream(&backend, &sink, frames).expect("the renderer takes six channels");

    stream.set_active(true).expect("plays");
    stream.drain().expect("drains");
    wave.until("the fetch to end once drained", |heard| heard.fetch_ended);
    stream.close().expect("closes");

    let heard = wave.heard.lock();
    let fetched = &heard.fetched;

    assert_eq!(&fetched[..4], b"RIFF");
    assert_eq!(u16::from_le_bytes([fetched[20], fetched[21]]), 0xFFFE);
    assert_eq!(u16::from_le_bytes([fetched[22], fetched[23]]), 6);
    assert_eq!(
        u16::from_le_bytes([fetched[32], fetched[33]]),
        18,
        "six channels of three bytes make a block of eighteen"
    );
    assert_eq!(u16::from_le_bytes([fetched[34], fetched[35]]), 24);
    assert_eq!(u16::from_le_bytes([fetched[38], fetched[39]]), 24);
    assert_eq!(
        u32::from_le_bytes([fetched[40], fetched[41], fetched[42], fetched[43]]),
        FRONT_AND_REAR_AND_CENTRE_AND_LFE
    );
    assert_eq!(&fetched[44..48], &[1, 0, 0, 0]);
    assert_eq!(&fetched[60..64], b"data");

    let packed = packed_words(&expected);

    assert_eq!(&fetched[EXTENSIBLE_HEADER_BYTES..], packed.as_slice());
}

#[test]
fn a_renderer_that_does_not_state_channels_is_offered_stereo_alone() {
    let fake = Fake::with(Profile {
        sink: FLAC_AND_WAVE,
        openhome: false,
        ..Profile::wave()
    });

    let network = Network::start(fake.searching(), FirstSearch::Awaited).expect("searches");
    let sink = the_renderer(&network);

    assert!(sink.supports(stereo_sixteen()));
    assert!(!sink.supports(StreamSpec::new(
        SampleRate::HZ_48000,
        ChannelLayout::Surround51,
        SampleFormat::S24
    )));
}

#[test]
fn a_renderer_offering_openhome_volume_is_turned_through_it_and_its_playlist_is_left_alone() {
    let fake = Fake::with(Profile {
        sink: WAVE_ONLY,
        openhome: true,
        ..Profile::wave()
    });
    let network = Network::start(fake.searching(), FirstSearch::Awaited).expect("searches");
    let sink = the_renderer(&network);
    let backend = Beside::over(Box::new(NoGraph::new()), Arc::new(network));

    let port = sink.port.as_ref().expect("a renderer names where it is");

    assert_eq!(port.hardware_volume, HardwareVolume::Always);

    let volume = Volume::heard_at(port.volume.expect("the renderer's volume was read"));

    assert!(
        (volume.get() - 0.4).abs() < 1e-4,
        "20 steps of 50 are {volume:?}"
    );

    backend
        .set_device_volume(sink.id, Volume::new(0.5).expect("a volume").to_gain())
        .expect("turns");
    backend.set_device_mute(sink.id, true).expect("mutes");
    fake.until("the renderer to be muted", |heard| {
        heard.openhome.iter().any(|(action, _)| action == "SetMute")
    });

    let heard = fake.heard.lock();
    let openhome: Vec<&str> = heard
        .openhome
        .iter()
        .map(|(action, _)| action.as_str())
        .collect();

    assert_eq!(
        openhome,
        ["Characteristics", "Volume", "SetVolume", "SetMute"]
    );
    assert!(heard.openhome[2].1.contains("<Value>25</Value>"));
    assert!(heard.openhome[3].1.contains("<Value>true</Value>"));
    assert_eq!(
        heard.actions(),
        ["GetProtocolInfo"],
        "RenderingControl was asked beside the OpenHome service that turns the volume"
    );
}

#[test]
fn flac_carries_twenty_four_bits_up_to_ninety_six_kilohertz_bit_for_bit_and_wave_the_rates_above() {
    for rate in [
        SampleRate::HZ_44100,
        SampleRate::HZ_88200,
        SampleRate::HZ_96000,
    ] {
        let fake = Fake::with(Profile {
            sink: FLAC_AND_WAVE,
            openhome: false,
            ..Profile::wave()
        });
        let network = Network::start(fake.searching(), FirstSearch::Awaited).expect("searches");
        let sink = the_renderer(&network);
        let backend = Beside::over(Box::new(NoGraph::new()), Arc::new(network));
        let spec = StreamSpec::new(rate, ChannelLayout::Stereo, SampleFormat::S24);
        let frames = rate.hz() / 3;

        let stream = pattern_stream(&backend, &sink, spec, frames).expect("the renderer takes it");

        stream.set_active(true).expect("plays");
        stream.drain().expect("drains");
        fake.until("the fetch to end once drained", |heard| heard.fetch_ended);
        stream.close().expect("closes");

        let tree = Tree::new();
        let heard = fake.heard.lock();

        assert_eq!(heard.fetched.get(..4), Some(&b"fLaC"[..]), "{rate:?}");
        assert_eq!(
            decoded(&tree, &heard.fetched, SampleFormat::S24),
            Pattern::expected(2, frames),
            "{rate:?} did not come back from the FLAC as it went in"
        );
    }
}

const FLAC_WITH_HIGH_RATES_STATED: &str = "http-get:*:audio/flac;rate=176400:*,http-get:*:audio/flac;rate=192000:*,http-get:*:audio/flac;rate=352800:*,http-get:*:audio/wav:*";

#[test]
fn flac_above_ninety_six_kilohertz_is_sent_where_the_renderer_states_the_rate_and_comes_back_bit_for_bit()
 {
    for (rate, frames) in [
        (SampleRate::HZ_176400, 176_400 / 3),
        (SampleRate::HZ_192000, 192_000 / 3),
        (SampleRate::HZ_192000, 8_192 * 2 + 100),
        (SampleRate::HZ_352800, 352_800 / 3),
    ] {
        let fake = Fake::with(Profile {
            sink: FLAC_WITH_HIGH_RATES_STATED,
            openhome: false,
            ..Profile::wave()
        });
        let network = Network::start(fake.searching(), FirstSearch::Awaited).expect("searches");
        let sink = the_renderer(&network);
        let backend = Beside::over(Box::new(NoGraph::new()), Arc::new(network));
        let spec = StreamSpec::new(rate, ChannelLayout::Stereo, SampleFormat::S24);

        let stream = pattern_stream(&backend, &sink, spec, frames).expect("the renderer takes it");

        stream.set_active(true).expect("plays");
        stream.drain().expect("drains");
        fake.until("the fetch to end once drained", |heard| heard.fetch_ended);
        stream.close().expect("closes");

        let tree = Tree::new();
        let heard = fake.heard.lock();
        let offered = heard.offered.as_deref().expect("an address was offered");

        assert!(offered.ends_with(".flac"), "{rate:?} offered {offered}");
        assert_eq!(heard.fetched.get(..4), Some(&b"fLaC"[..]), "{rate:?}");

        let streaminfo_rate = (u32::from(heard.fetched[18]) << 12)
            | (u32::from(heard.fetched[19]) << 4)
            | (u32::from(heard.fetched[20]) >> 4);

        assert_eq!(
            streaminfo_rate,
            rate.hz(),
            "the STREAMINFO names another rate"
        );
        assert_eq!(
            decoded(&tree, &heard.fetched, SampleFormat::S24),
            Pattern::expected(2, frames),
            "{rate:?} did not come back from the FLAC as it went in"
        );
        if let Some(by_the_tool) = decoded_by_the_flac_tool(&tree, &heard.fetched) {
            assert_eq!(
                by_the_tool,
                packed_words(&Pattern::expected(2, frames)),
                "the flac tool read {rate:?} otherwise"
            );
        }
    }
}

#[test]
fn a_rate_flac_cannot_carry_is_sent_as_wave_to_a_renderer_taking_both() {
    let fake = Fake::with(Profile {
        sink: FLAC_AND_WAVE,
        openhome: false,
        ..Profile::wave()
    });
    let network = Network::start(fake.searching(), FirstSearch::Awaited).expect("searches");
    let sink = the_renderer(&network);
    let backend = Beside::over(Box::new(NoGraph::new()), Arc::new(network));
    let spec = StreamSpec::new(
        SampleRate::HZ_192000,
        ChannelLayout::Stereo,
        SampleFormat::S24,
    );
    let frames = 192_000 / 4;

    let stream = pattern_stream(&backend, &sink, spec, frames).expect("the renderer takes it");

    stream.set_active(true).expect("plays");
    stream.drain().expect("drains");
    fake.until("the fetch to end once drained", |heard| heard.fetch_ended);
    stream.close().expect("closes");

    let heard = fake.heard.lock();
    let offered = heard.offered.as_deref().expect("an address was offered");

    assert!(offered.ends_with(".wav"), "offered {offered}");
    assert_eq!(&heard.fetched[..4], b"RIFF");

    let packed = packed_words(&Pattern::expected(2, frames));

    assert_eq!(&heard.fetched[WAVE_HEADER_BYTES..], packed.as_slice());
}

const FLAC_ALONE: &str = "http-get:*:audio/mpeg:*,http-get:*:audio/flac:*";

fn network_vouching(fake: &Fake, vouched: Vouched) -> Network {
    let network = Network::resting(fake.searching()).expect("rests");

    network.vouch(vouched);
    network.look(FirstSearch::Awaited).expect("searches");
    network
}

fn high_flac() -> StreamSpec {
    StreamSpec::new(
        SampleRate::HZ_192000,
        ChannelLayout::Stereo,
        SampleFormat::S24,
    )
}

#[test]
fn a_renderer_taking_flac_alone_is_offered_it_above_ninety_six_kilohertz_only_when_the_listener_vouches()
 {
    for vouched in [
        Vouched::default(),
        Vouched::flac_at_any_rate([Udn::new("uuid:another-renderer")]),
    ] {
        let fake = Fake::with(Profile {
            sink: FLAC_ALONE,
            openhome: false,
            ..Profile::wave()
        });
        let network = network_vouching(&fake, vouched);
        let sink = the_renderer(&network);
        let backend = Beside::over(Box::new(NoGraph::new()), Arc::new(network));

        assert!(!sink.supports(high_flac()));
        assert!(sink.supports(StreamSpec::new(
            SampleRate::HZ_96000,
            ChannelLayout::Stereo,
            SampleFormat::S24
        )));
        assert!(pattern_stream(&backend, &sink, high_flac(), 1_000).is_err());
        assert_eq!(fake.heard.lock().offered, None);
    }

    let fake = Fake::with(Profile {
        sink: FLAC_ALONE,
        openhome: false,
        ..Profile::wave()
    });
    let network = network_vouching(&fake, Vouched::flac_at_any_rate([Udn::new(UDN)]));
    let sink = the_renderer(&network);
    let backend = Beside::over(Box::new(NoGraph::new()), Arc::new(network));
    let frames = 192_000 / 3;

    assert!(sink.supports(high_flac()));
    assert!(!sink.supports(StreamSpec::new(
        SampleRate::HZ_192000,
        ChannelLayout::Surround51,
        SampleFormat::S24
    )));

    let stream =
        pattern_stream(&backend, &sink, high_flac(), frames).expect("the renderer takes it");

    stream.set_active(true).expect("plays");
    stream.drain().expect("drains");
    fake.until("the fetch to end once drained", |heard| heard.fetch_ended);
    stream.close().expect("closes");

    let tree = Tree::new();
    let heard = fake.heard.lock();
    let offered = heard.offered.as_deref().expect("an address was offered");

    assert!(offered.ends_with(".flac"), "offered {offered}");
    assert_eq!(heard.fetched.get(..4), Some(&b"fLaC"[..]));
    assert_eq!(
        decoded(&tree, &heard.fetched, SampleFormat::S24),
        Pattern::expected(2, frames)
    );
}

#[test]
fn a_vouched_renderer_taking_flac_and_wave_is_sent_flac_at_a_rate_it_would_otherwise_be_sent_as_wave()
 {
    let fake = Fake::with(Profile {
        sink: FLAC_AND_WAVE,
        openhome: false,
        ..Profile::wave()
    });
    let network = network_vouching(&fake, Vouched::flac_at_any_rate([Udn::new(UDN)]));
    let sink = the_renderer(&network);
    let backend = Beside::over(Box::new(NoGraph::new()), Arc::new(network));
    let frames = 192_000 / 4;

    let stream =
        pattern_stream(&backend, &sink, high_flac(), frames).expect("the renderer takes it");

    stream.set_active(true).expect("plays");
    stream.drain().expect("drains");
    fake.until("the fetch to end once drained", |heard| heard.fetch_ended);
    stream.close().expect("closes");

    let tree = Tree::new();
    let heard = fake.heard.lock();
    let offered = heard.offered.as_deref().expect("an address was offered");

    assert!(offered.ends_with(".flac"), "offered {offered}");
    assert_eq!(
        decoded(&tree, &heard.fetched, SampleFormat::S24),
        Pattern::expected(2, frames)
    );
}

struct Deaf {
    held: UdpSocket,
}

impl Deaf {
    fn start() -> Self {
        Self {
            held: UdpSocket::bind("127.0.0.1:0").expect("a loopback socket nobody answers on"),
        }
    }

    fn searching(&self, every: Duration) -> Searching {
        Searching {
            to: self.held.local_addr().expect("the silent address"),
            within: Duration::from_millis(100),
            every,
            serving_on: IpAddr::V4(Ipv4Addr::LOCALHOST),
        }
    }
}

const SSDP_PORT: u16 = 1_900;

fn refused_search() -> Searching {
    Searching {
        to: SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), SSDP_PORT),
        within: Duration::from_millis(100),
        every: Duration::from_secs(3_600),
        serving_on: IpAddr::V4(Ipv4Addr::LOCALHOST),
    }
}

fn nobody_listening_at() -> Located {
    let free = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let at = free.local_addr().expect("its address");

    drop(free);
    Located::parse(&format!("http://{at}/description.xml")).expect("reads")
}

fn changes_of(network: &Network) -> Receiver<SinkChange> {
    Neighbours::subscribe(network)
}

#[test]
fn a_renderer_given_by_the_address_of_its_description_is_listed_though_the_search_hears_nothing() {
    let fake = Fake::start();
    let deaf = Deaf::start();
    let network = Network::resting(deaf.searching(Duration::from_secs(3_600))).expect("rests");

    network
        .locate(fake.description_address())
        .expect("a resting network only holds the address");

    assert!(network.sinks().is_empty());
    assert!(fake.heard.lock().actions().is_empty());

    network.look(FirstSearch::Awaited).expect("looks");

    let sink = the_renderer(&network);

    assert_eq!(sink.name, NodeName::new(format!("upnp:{UDN}")));
    assert_eq!(sink.description, NAME);
    assert!(sink.supports(stereo_sixteen()));
    assert_eq!(fake.heard.lock().searches, 0);
    assert_eq!(network.located(), [fake.description_address()]);
}

#[test]
fn a_renderer_given_by_address_stays_through_every_search_that_misses_it_and_is_described_once() {
    let fake = Fake::start();
    let deaf = Deaf::start();
    let network = Network::resting(deaf.searching(Duration::from_millis(60))).expect("rests");

    network
        .locate(fake.description_address())
        .expect("holds the address");
    network.look(FirstSearch::Awaited).expect("looks");

    let sink = the_renderer(&network);

    thread::sleep(Duration::from_millis(900));

    assert_eq!(the_renderer(&network).id, sink.id);
    assert_eq!(
        fake.heard
            .lock()
            .actions()
            .iter()
            .filter(|action| **action == "GetProtocolInfo")
            .count(),
        1
    );
}

#[test]
fn an_address_given_to_a_looking_network_lists_the_renderer_at_once_and_forgetting_it_removes_it() {
    let fake = Fake::start();
    let deaf = Deaf::start();
    let network = Network::start(
        deaf.searching(Duration::from_secs(3_600)),
        FirstSearch::Awaited,
    )
    .expect("looks");
    let changes = changes_of(&network);

    assert!(network.sinks().is_empty());

    network
        .locate(fake.description_address())
        .expect("the renderer is described");

    let sink = the_renderer(&network);

    network.unlocate(&fake.description_address());

    assert_eq!(changes.try_recv(), Ok(SinkChange::Added(sink.id)));
    assert_eq!(changes.try_recv(), Ok(SinkChange::Removed(sink.id)));
    assert!(network.sinks().is_empty());
    assert!(network.located().is_empty());
}

#[test]
fn a_renderer_both_found_by_the_search_and_given_by_address_is_listed_once_and_kept_when_the_address_goes()
 {
    let fake = Fake::start();
    let network = Network::resting(fake.searching()).expect("rests");

    network
        .locate(fake.description_address())
        .expect("holds the address");
    network.look(FirstSearch::Awaited).expect("looks");

    let sink = the_renderer(&network);

    network.unlocate(&fake.description_address());

    assert_eq!(the_renderer(&network).id, sink.id);
}

#[test]
fn an_address_that_is_not_answering_is_refused_to_a_looking_network_and_still_held_for_the_next_search()
 {
    let deaf = Deaf::start();
    let network = Network::start(
        deaf.searching(Duration::from_secs(3_600)),
        FirstSearch::Awaited,
    )
    .expect("looks");
    let nobody = nobody_listening_at();

    let refused = network.locate(nobody.clone());

    assert!(refused.is_err());
    assert_eq!(network.located(), [nobody]);
    assert!(network.sinks().is_empty());
}

#[test]
fn a_network_that_cannot_send_its_search_still_reaches_the_renderers_given_by_address() {
    let fake = Fake::start();
    let network = Network::resting(refused_search()).expect("rests");

    assert!(network.look(FirstSearch::Awaited).is_err());

    network
        .locate(fake.description_address())
        .expect("holds the address");
    network
        .look(FirstSearch::Awaited)
        .expect("a search that fails is no failure where an address is held");

    assert_eq!(the_renderer(&network).description, NAME);
}

#[test]
fn resting_forgets_the_renderers_and_keeps_the_addresses_to_reach_again() {
    let fake = Fake::start();
    let deaf = Deaf::start();
    let network = Network::resting(deaf.searching(Duration::from_secs(3_600))).expect("rests");

    network
        .locate(fake.description_address())
        .expect("holds the address");
    network.look(FirstSearch::Awaited).expect("looks");

    assert_eq!(network.sinks().len(), 1);

    network.rest();

    assert!(network.sinks().is_empty());

    network.look(FirstSearch::Awaited).expect("looks again");

    assert_eq!(the_renderer(&network).description, NAME);
}

#[test]
fn a_network_reaching_addresses_alone_sends_no_search_and_lists_the_renderer_given() {
    let fake = Fake::start();
    let network = Network::resting(fake.searching()).expect("rests");

    network
        .locate(fake.description_address())
        .expect("holds the address");

    network
        .look_at_addresses(FirstSearch::Awaited)
        .expect("reaches");

    let sink = the_renderer(&network);

    assert_eq!(sink.description, NAME);
    assert_eq!(fake.heard.lock().searches, 0);
    assert!(network.is_reaching());
    assert!(!network.is_looking());
}

#[test]
fn an_address_given_to_a_network_reaching_addresses_alone_is_reached_at_once_without_a_search() {
    let fake = Fake::start();
    let network = Network::resting(fake.searching()).expect("rests");

    network
        .look_at_addresses(FirstSearch::Awaited)
        .expect("reaches");

    network
        .locate(fake.description_address())
        .expect("the renderer is described");

    assert_eq!(the_renderer(&network).description, NAME);
    assert_eq!(fake.heard.lock().searches, 0);
}

#[test]
fn a_network_reaching_addresses_alone_that_is_asked_to_look_searches_the_network_too() {
    let fake = Fake::start();
    let network = Network::resting(fake.searching()).expect("rests");

    network
        .locate(fake.description_address())
        .expect("holds the address");
    network
        .look_at_addresses(FirstSearch::Awaited)
        .expect("reaches");

    network.look(FirstSearch::Awaited).expect("looks");

    assert!(network.is_looking());
    assert!(fake.heard.lock().searches > 0);
    assert_eq!(network.sinks().len(), 1);
}

#[test]
fn a_network_that_stops_multicasting_keeps_the_renderers_given_by_address_and_drops_the_rest() {
    let heard = Fake::start();
    let given = Fake::start();

    given.heard.lock().udn = Some("uuid:renderer-given");

    let network = Network::resting(heard.searching()).expect("rests");

    network.look(FirstSearch::Awaited).expect("looks");
    network
        .locate(given.description_address())
        .expect("the renderer is described");

    assert_eq!(network.sinks().len(), 2);

    network.stop_multicasting();

    assert!(!network.is_looking());
    assert!(network.is_reaching());

    let remaining = network.sinks();

    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].name, NodeName::new("upnp:uuid:renderer-given"));
}

#[test]
fn a_network_with_no_address_that_stops_multicasting_rests() {
    let fake = Fake::start();
    let network = Network::start(fake.searching(), FirstSearch::Awaited).expect("looks");

    assert_eq!(network.sinks().len(), 1);

    network.stop_multicasting();

    assert!(!network.is_reaching());
    assert!(network.sinks().is_empty());
}

#[test]
fn vouching_for_a_renderer_already_found_offers_it_high_rate_flac_at_once_and_vouching_it_away_takes_it_back()
 {
    let fake = Fake::with(Profile {
        sink: FLAC_ALONE,
        openhome: false,
        ..Profile::wave()
    });
    let network = network_vouching(&fake, Vouched::default());
    let changes = changes_of(&network);
    let before = the_renderer(&network);

    assert!(!before.supports(high_flac()));

    network.vouch(Vouched::flac_at_any_rate([Udn::new(UDN)]));

    let vouched = the_renderer(&network);

    assert_eq!(vouched.id, before.id);
    assert!(vouched.supports(high_flac()));
    assert_eq!(changes.try_recv(), Ok(SinkChange::Reformatted(before.id)));

    network.vouch(Vouched::default());

    assert!(!the_renderer(&network).supports(high_flac()));
    assert_eq!(changes.try_recv(), Ok(SinkChange::Reformatted(before.id)));

    network.vouch(Vouched::default());

    assert!(changes.try_recv().is_err());
}

fn surround(layout: ChannelLayout) -> StreamSpec {
    StreamSpec::new(SampleRate::HZ_48000, layout, SampleFormat::S24)
}

#[test]
fn learning_what_a_renderer_plays_offers_exactly_what_played_and_it_is_then_sent_high_rate_flac_and_six_channels_bit_for_bit()
 {
    let fake = Fake::with(Profile {
        sink: FLAC_ALONE,
        openhome: false,
        ..Profile::wave()
    });
    let network = Arc::new(network_vouching(&fake, Vouched::default()));
    let changes = changes_of(&network);
    let before = the_renderer(&network);

    assert!(!before.supports(high_flac()));
    assert!(!before.supports(surround(ChannelLayout::Surround51)));

    let learning = network.learn(before.id).expect("the renderer is tried");

    assert_eq!(
        learning.verdicts(),
        Trial::ALL.map(|trial| (trial, TrialVerdict::Played))
    );
    assert_eq!(learning.plays(), Plays::from_iter(Trial::ALL));
    assert_eq!(changes.try_recv(), Ok(SinkChange::Reformatted(before.id)));

    let learnt = the_renderer(&network);

    assert_eq!(learnt.id, before.id);

    for spec in [
        high_flac(),
        StreamSpec::new(
            SampleRate::HZ_176400,
            ChannelLayout::Stereo,
            SampleFormat::S24,
        ),
        StreamSpec::new(
            SampleRate::HZ_384000,
            ChannelLayout::Stereo,
            SampleFormat::S24,
        ),
        StreamSpec::new(
            SampleRate::HZ_352800,
            ChannelLayout::Stereo,
            SampleFormat::S24,
        ),
        surround(ChannelLayout::Surround51),
        surround(ChannelLayout::Surround71),
        StreamSpec::new(
            SampleRate::HZ_96000,
            ChannelLayout::Surround51,
            SampleFormat::S24,
        ),
        StreamSpec::new(
            SampleRate::HZ_96000,
            ChannelLayout::Surround71,
            SampleFormat::S24,
        ),
    ] {
        assert!(
            learnt.supports(spec),
            "{spec} was learnt and is not offered"
        );
    }
    {
        let heard = fake.heard.lock();

        assert_eq!(
            stops_and_offers(&heard),
            (8, 8),
            "each trial is offered and stopped"
        );
        assert!(
            heard
                .offers
                .iter()
                .all(|offered| offered.ends_with(".flac"))
        );
    }

    let backend = Beside::over(
        Box::new(NoGraph::new()),
        Arc::clone(&network) as Arc<dyn Neighbours>,
    );
    let frames = 192_000 / 4;

    let stream =
        pattern_stream(&backend, &learnt, high_flac(), frames).expect("the renderer takes it");

    stream.set_active(true).expect("plays");
    stream.drain().expect("drains");
    fake.until("the fetch to end once drained", |heard| {
        heard.offers.len() == Trial::ALL.len() + 1
            && heard.fetching.as_ref() == heard.offers.last()
            && heard.fetch_ended
    });
    stream.close().expect("closes");

    let tree = Tree::new();

    {
        let heard = fake.heard.lock();

        assert_eq!(heard.fetched.get(..4), Some(&b"fLaC"[..]));
        assert_eq!(
            decoded(&tree, &heard.fetched, SampleFormat::S24),
            Pattern::expected(2, frames),
            "192 kHz did not come back from the FLAC as it went in"
        );
    }

    let frames = 48_000 / 2;
    let stream = surround_stream(&backend, &learnt, frames).expect("the renderer takes six");

    stream.set_active(true).expect("plays");
    stream.drain().expect("drains");
    fake.until("the second stream's fetch to end once drained", |heard| {
        heard.offers.len() == Trial::ALL.len() + 2
            && heard.fetching.as_ref() == heard.offers.last()
            && heard.fetch_ended
    });
    stream.close().expect("closes");

    let heard = fake.heard.lock();

    assert_eq!(
        decoded(&tree, &heard.fetched, SampleFormat::S24),
        Pattern::expected(6, frames),
        "the six channels did not come back from the FLAC as they went in"
    );
}

#[test]
fn a_renderer_that_stops_on_what_it_cannot_decode_is_learnt_to_play_none_of_it() {
    let fake = Fake::with(Profile {
        sink: FLAC_ALONE,
        openhome: false,
        ..Profile::wave()
    })
    .decoding_at_most(96_000, 2);
    let network = network_vouching(&fake, Vouched::default());
    let changes = changes_of(&network);
    let before = the_renderer(&network);

    let learning = network.learn(before.id).expect("the renderer is tried");

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
            (Trial::FlacAt384kHz, TrialVerdict::Untried),
            (Trial::FlacAt352kHz, TrialVerdict::Untried),
            (
                Trial::SixChannels,
                TrialVerdict::Unplayed(Unplayed::Errored)
            ),
            (Trial::EightChannels, TrialVerdict::Untried),
            (Trial::SixChannelsAt96kHz, TrialVerdict::Untried),
            (Trial::EightChannelsAt96kHz, TrialVerdict::Untried),
        ]
    );
    assert!(learning.plays().is_empty());
    assert!(changes.try_recv().is_err(), "nothing learnt moves nothing");
    assert_eq!(the_renderer(&network), before);
    assert_eq!(stops_and_offers(&fake.heard.lock()), (3, 3));
}

#[test]
fn a_renderer_is_offered_surround_up_to_the_rate_it_played_it_at_and_high_rates_in_stereo_alone() {
    let fake = Fake::with(Profile {
        sink: FLAC_ALONE,
        openhome: false,
        ..Profile::wave()
    })
    .decoding_at_most(192_000, 6);
    let network = network_vouching(&fake, Vouched::default());
    let before = the_renderer(&network);

    let learning = network.learn(before.id).expect("the renderer is tried");

    assert_eq!(
        learning.plays(),
        Plays::from_iter([
            Trial::FlacAt192kHz,
            Trial::FlacAt176kHz,
            Trial::SixChannels,
            Trial::SixChannelsAt96kHz,
        ])
    );
    assert_eq!(
        learning.verdict_on(Trial::EightChannelsAt96kHz),
        Some(TrialVerdict::Untried)
    );

    let learnt = the_renderer(&network);
    let five_one = |rate| StreamSpec::new(rate, ChannelLayout::Surround51, SampleFormat::S24);

    assert!(learnt.supports(five_one(SampleRate::HZ_96000)));
    assert!(learnt.supports(five_one(SampleRate::HZ_44100)));
    assert!(!learnt.supports(five_one(SampleRate::HZ_192000)));
    assert!(!learnt.supports(surround(ChannelLayout::Surround71)));
    assert!(!learnt.supports(StreamSpec::new(
        SampleRate::HZ_384000,
        ChannelLayout::Stereo,
        SampleFormat::S24
    )));
    assert!(learnt.supports(high_flac()));
}

#[test]
fn a_renderer_a_stream_holds_is_not_tried_and_teaching_what_was_learnt_offers_it_at_once() {
    let fake = Fake::with(Profile {
        sink: FLAC_ALONE,
        openhome: false,
        ..Profile::wave()
    });
    let network = Arc::new(network_vouching(&fake, Vouched::default()));
    let sink = the_renderer(&network);
    let backend = Beside::over(
        Box::new(NoGraph::new()),
        Arc::clone(&network) as Arc<dyn Neighbours>,
    );

    let stream =
        pattern_stream(&backend, &sink, stereo_sixteen(), 44_100).expect("the renderer takes it");

    assert!(matches!(
        network.learn(sink.id),
        Err(resonate_upnp::Error::InUse(held)) if held == sink.id
    ));

    stream.close().expect("closes");

    let changes = changes_of(&network);
    let learnt: Learnt = [(sink.name.clone(), Plays::from_iter([Trial::FlacAt192kHz]))]
        .into_iter()
        .collect();

    network.teach(learnt.clone());

    assert_eq!(changes.try_recv(), Ok(SinkChange::Reformatted(sink.id)));
    assert!(the_renderer(&network).supports(high_flac()));
    assert!(!the_renderer(&network).supports(surround(ChannelLayout::Surround51)));

    network.teach(learnt);

    assert!(changes.try_recv().is_err());

    network.teach(Learnt::default());

    assert_eq!(changes.try_recv(), Ok(SinkChange::Reformatted(sink.id)));
    assert!(!the_renderer(&network).supports(high_flac()));
}

fn held_at(network: &Network) -> Located {
    let renderers = network.renderers();

    assert_eq!(renderers.len(), 1);
    renderers[0].1.location().clone()
}

fn until_held_at(network: &Network, wanted: &Located) {
    let since = Instant::now();

    while held_at(network) != *wanted {
        assert!(
            since.elapsed() < WAITED_AT_MOST,
            "waited too long for the renderer to be held at {wanted}"
        );

        thread::sleep(LOOKS_EVERY);
    }
}

#[test]
fn a_renderer_that_moves_from_one_address_to_another_and_back_is_followed_each_way() {
    let home = Fake::start();
    let away = Fake::start();
    let network = Network::resting(Searching {
        every: Duration::from_millis(100),
        ..home.searching()
    })
    .expect("rests");

    network.look(FirstSearch::Awaited).expect("looks");

    let sink = the_renderer(&network);

    assert_eq!(held_at(&network), home.description_address());

    home.heard.lock().answers_from = Some(away.described_at);
    until_held_at(&network, &away.description_address());

    home.heard.lock().answers_from = None;
    until_held_at(&network, &home.description_address());

    assert_eq!(the_renderer(&network).id, sink.id);
}

fn silent_addresses(count: usize) -> (Vec<TcpListener>, Vec<Located>) {
    let listeners: Vec<TcpListener> = (0..count)
        .map(|_| TcpListener::bind("127.0.0.1:0").expect("a loopback listener nobody answers on"))
        .collect();
    let addresses = listeners
        .iter()
        .map(|listener| {
            let at = listener.local_addr().expect("its address");

            Located::parse(&format!("http://{at}/description.xml")).expect("reads")
        })
        .collect();

    (listeners, addresses)
}

const ROUND_DESCRIBES_FOR: Duration = Duration::from_millis(300);

const WELL_PAST_A_ROUND: Duration = Duration::from_millis(1_500);

#[test]
fn a_round_over_five_silent_renderers_is_over_at_its_deadline_and_lists_none() {
    let deaf = Deaf::start();
    let (_silent, addresses) = silent_addresses(5);
    let network = Network::resting_describing_within(
        deaf.searching(Duration::from_secs(3_600)),
        ROUND_DESCRIBES_FOR,
    )
    .expect("rests");

    for address in addresses {
        network.locate(address).expect("a resting network holds it");
    }

    let began = Instant::now();

    network
        .look_at_addresses(FirstSearch::Awaited)
        .expect("reaches");

    assert!(
        began.elapsed() < WELL_PAST_A_ROUND,
        "took {:?}",
        began.elapsed()
    );
    assert!(network.sinks().is_empty());
}

#[test]
fn resting_and_stopping_multicast_do_not_wait_out_a_round_over_silent_renderers() {
    let deaf = Deaf::start();
    let (_silent, addresses) = silent_addresses(5);
    let network = Network::resting_describing_within(
        deaf.searching(Duration::from_secs(3_600)),
        Duration::from_secs(10),
    )
    .expect("rests");

    for address in addresses {
        network.locate(address).expect("a resting network holds it");
    }
    network
        .look_at_addresses(FirstSearch::Behind)
        .expect("reaches");

    thread::sleep(Duration::from_millis(100));

    let began = Instant::now();

    network.rest();

    assert!(
        began.elapsed() < ROUND_DESCRIBES_FOR,
        "took {:?}",
        began.elapsed()
    );
    assert!(network.sinks().is_empty());
}

struct Watched {
    listeners: Mutex<Vec<Sender<SinkChange>>>,
}

impl Watched {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            listeners: Mutex::new(Vec::new()),
        })
    }

    fn forwarders_alive(&self) -> usize {
        self.listeners
            .lock()
            .iter()
            .filter(|listener| listener.send(SinkChange::DefaultChanged).is_ok())
            .count()
    }
}

impl Neighbours for Watched {
    fn owns(&self, _sink: SinkId) -> bool {
        false
    }

    fn sinks(&self) -> Vec<SinkInfo> {
        Vec::new()
    }

    fn subscribe(&self) -> Receiver<SinkChange> {
        let (announce, changes) = unbounded();

        self.listeners.lock().push(announce);
        changes
    }

    fn open(
        &self,
        _request: &StreamRequest,
        _source: Box<dyn AudioSource>,
    ) -> SinkResult<SinkStream> {
        Err(SinkError::NoSink)
    }

    fn turn_volume(&self, _sink: SinkId, _gain: Gain) -> SinkResult<()> {
        Ok(())
    }

    fn turn_mute(&self, _sink: SinkId, _muted: bool) -> SinkResult<()> {
        Ok(())
    }
}

#[test]
fn no_forwarder_outlives_the_backend_whose_sinks_it_told_of() {
    let watched = Watched::new();
    let backend = Beside::over(
        Box::new(NoGraph::new()),
        Arc::clone(&watched) as Arc<dyn Neighbours>,
    );

    let first = backend.subscribe_sinks();
    let second = backend.subscribe_sinks();

    assert_eq!(watched.forwarders_alive(), 2);

    drop(backend);

    assert_eq!(watched.forwarders_alive(), 0);

    drop((first, second));
}

#[test]
fn no_forwarder_outlives_a_backend_that_is_shut_down() {
    let watched = Watched::new();
    let backend = Beside::over(
        Box::new(NoGraph::new()),
        Arc::clone(&watched) as Arc<dyn Neighbours>,
    );

    let changes = backend.subscribe_sinks();

    assert_eq!(watched.forwarders_alive(), 1);

    Box::new(backend).shutdown().expect("shuts down");

    assert_eq!(watched.forwarders_alive(), 0);

    drop(changes);
}

#[test]
fn a_forwarder_whose_receiver_is_dropped_ends_with_the_next_change() {
    let watched = Watched::new();
    let backend = Beside::over(
        Box::new(NoGraph::new()),
        Arc::clone(&watched) as Arc<dyn Neighbours>,
    );

    drop(backend.subscribe_sinks());

    let since = Instant::now();
    while watched.forwarders_alive() > 0 {
        assert!(since.elapsed() < WAITED_AT_MOST, "the forwarder lived on");

        thread::sleep(LOOKS_EVERY);
    }
}

const ANSWER_BYTES_AT_MOST: usize = 1 << 20;

const HEAD_BYTES_AT_MOST: usize = 16 << 10;

const NAME_BYTES_AT_MOST: usize = 256;

const ANSWERED_WITHIN: Duration = Duration::from_secs(3);

const GIVEN_UP_BY: Duration = Duration::from_secs(6);

const A_LONG_NAME_IN_LETTERS: usize = 5_000;

const PAST_THE_ANSWER: Misbehaviour = Misbehaviour::Body(ANSWER_BYTES_AT_MOST + 1);

const PAST_THE_HEAD: Misbehaviour = Misbehaviour::Head(4 * HEAD_BYTES_AT_MOST);

type Expected = fn(&UpnpError) -> bool;

type Ended = fn(&StreamEvent) -> bool;

const SET_ADDRESS: On = On::Action("SetAVTransportURI");

const PLAY: On = On::Action("Play");

fn upnp_error(error: &SinkError) -> Option<&UpnpError> {
    match error {
        SinkError::Backend { source, .. } => source
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<UpnpError>()),
        _ => None,
    }
}

fn beside_the(fake: &Fake) -> (Beside, Arc<Network>, SinkInfo) {
    let network =
        Arc::new(Network::start(fake.searching(), FirstSearch::Awaited).expect("searches"));
    let sink = the_renderer(&network);

    (
        Beside::over(Box::new(NoGraph::new()), Arc::<Network>::clone(&network)),
        network,
        sink,
    )
}

fn long_stream(backend: &Beside, sink: &SinkInfo) -> SinkResult<SinkStream> {
    backend.opener().open(
        &StreamRequest {
            target: Some(sink.id),
            spec: stereo_sixteen(),
            latency: LatencyRequest::Auto,
            role: MediaRole::Music,
            media_name: "Faulty".to_owned(),
            force_graph_rate: false,
            no_convert: true,
            realtime: true,
        },
        Box::new(Counting {
            next: 0,
            left: RATE as usize * STRIDE * 30,
        }),
    )
}

fn is_failed(event: &StreamEvent) -> bool {
    matches!(
        event,
        StreamEvent::StateChanged {
            to: StreamState::Failed,
            ..
        }
    )
}

fn is_refused(event: &StreamEvent) -> bool {
    matches!(event, StreamEvent::Refused { .. })
}

fn events_until(
    events: &Receiver<StreamEvent>,
    wanted: impl Fn(&StreamEvent) -> bool,
) -> Vec<StreamEvent> {
    let since = Instant::now();
    let mut heard = Vec::new();

    while since.elapsed() < WAITED_AT_MOST {
        if let Ok(event) = events.recv_timeout(LOOKS_EVERY) {
            let done = wanted(&event);

            heard.push(event);
            if done {
                break;
            }
        }
    }
    heard
}

fn reached_by_address(fake: &Fake) -> (Deaf, Network, Result<(), UpnpError>) {
    let deaf = Deaf::start();
    let network = Network::start(
        deaf.searching(Duration::from_secs(3_600)),
        FirstSearch::Awaited,
    )
    .expect("starts");

    let located = network.locate(fake.description_address());

    (deaf, network, located)
}

fn round_over(fake: &Fake) -> (Deaf, Network, Duration) {
    let deaf = Deaf::start();
    let network = Network::resting_describing_within(
        deaf.searching(Duration::from_secs(3_600)),
        ROUND_DESCRIBES_FOR,
    )
    .expect("rests");

    network
        .locate(fake.description_address())
        .expect("a resting network holds it");

    let began = Instant::now();

    network
        .look_at_addresses(FirstSearch::Awaited)
        .expect("reaches");
    (deaf, network, began.elapsed())
}

#[test]
fn a_renderer_faulting_the_address_fails_the_open_with_its_code_and_stays_listed_and_free() {
    let fake = Fake::with(Profile::faulting(SET_ADDRESS, Misbehaviour::Fault(714)));
    let (backend, network, sink) = beside_the(&fake);

    let refused = long_stream(&backend, &sink)
        .err()
        .expect("the open was refused");

    assert!(matches!(
        refused,
        SinkError::Backend {
            op: Operation::Open,
            ..
        }
    ));
    assert!(matches!(
        upnp_error(&refused),
        Some(UpnpError::Fault {
            action: Action::SetAvTransportUri,
            code: Some(714)
        })
    ));
    assert_eq!(network.sinks().len(), 1, "a refusal dropped the sink");
    assert_eq!(fake.heard.lock().times("Play"), 0);

    fake.heal();

    let stream = long_stream(&backend, &sink).expect("a healed renderer takes the stream");

    stream.set_active(true).expect("plays");
    fake.until("the healed renderer to be played", |heard| {
        heard.times("Play") == 1
    });
    stream.close().expect("closes");
    fake.until("the healed renderer to be stopped", |heard| {
        heard.times("Stop") >= 1
    });
}

#[test]
fn a_renderer_misbehaving_on_the_address_fails_the_open_as_what_it_did_and_leaves_nothing_played() {
    let cases: [(Misbehaviour, Expected); 5] = [
        (Misbehaviour::Status(503), |error| {
            matches!(
                error,
                UpnpError::Status {
                    asked: Asked::Action(Action::SetAvTransportUri),
                    status: 503
                }
            )
        }),
        (PAST_THE_ANSWER, |error| {
            matches!(
                error,
                UpnpError::TooLong {
                    asked: Asked::Action(Action::SetAvTransportUri),
                    at_most: ANSWER_BYTES_AT_MOST
                }
            )
        }),
        (Misbehaviour::Endless, |error| {
            matches!(
                error,
                UpnpError::TooLong {
                    asked: Asked::Action(Action::SetAvTransportUri),
                    ..
                }
            )
        }),
        (PAST_THE_HEAD, |error| {
            matches!(
                error,
                UpnpError::NotHttp {
                    asked: Asked::Action(Action::SetAvTransportUri)
                }
            )
        }),
        (Misbehaviour::Stall, |error| {
            matches!(
                error,
                UpnpError::Network { asked: Asked::Action(Action::SetAvTransportUri), source }
                    if source.kind() == io::ErrorKind::TimedOut
            )
        }),
    ];

    for (how, expected) in cases {
        let fake = Fake::with(Profile::faulting(SET_ADDRESS, how));
        let (backend, network, sink) = beside_the(&fake);
        let began = Instant::now();

        let refused = long_stream(&backend, &sink)
            .err()
            .expect("the open was refused");

        assert!(began.elapsed() < GIVEN_UP_BY, "the open hung");
        assert!(
            upnp_error(&refused).is_some_and(expected),
            "{refused:?} was not the failure its fault should be"
        );
        assert_eq!(network.sinks().len(), 1, "a refusal dropped the sink");
        assert_eq!(fake.heard.lock().times("Play"), 0);

        fake.heal();

        assert!(
            long_stream(&backend, &sink).is_ok(),
            "the renderer stayed held by the failed open"
        );
    }
}

#[test]
fn a_stalled_address_is_given_up_on_after_the_answer_time_and_the_open_says_it_timed_out() {
    let fake = Fake::with(Profile::faulting(SET_ADDRESS, Misbehaviour::Stall));
    let (backend, _network, sink) = beside_the(&fake);
    let began = Instant::now();

    let refused = long_stream(&backend, &sink)
        .err()
        .expect("the open was refused");

    assert!(began.elapsed() >= ANSWERED_WITHIN - LOOKS_EVERY);
    assert!(began.elapsed() < GIVEN_UP_BY);
    assert!(upnp_error(&refused).is_some());
}

#[test]
fn a_renderer_faulting_play_refuses_the_stream_with_its_code_and_is_not_stopped_for_it() {
    let fake = Fake::with(Profile::faulting(PLAY, Misbehaviour::Fault(701)));
    let (backend, _network, sink) = beside_the(&fake);

    let stream = long_stream(&backend, &sink).expect("the address was taken");

    stream.set_active(true).expect("asks to play");

    let heard = events_until(stream.events(), is_refused);

    assert_eq!(
        heard.last(),
        Some(&StreamEvent::Refused {
            from: StreamState::Paused,
            op: Operation::Start,
            status: 701
        })
    );
    assert_eq!(
        fake.heard.lock().actions(),
        ["GetProtocolInfo", "GetVolume", "SetAVTransportURI", "Play"]
    );
}

#[test]
fn a_renderer_misbehaving_on_play_fails_the_stream_within_the_answer_time() {
    let cases = [
        Misbehaviour::Status(503),
        PAST_THE_ANSWER,
        Misbehaviour::Endless,
        PAST_THE_HEAD,
        Misbehaviour::Stall,
    ];

    for how in cases {
        let fake = Fake::with(Profile::faulting(PLAY, how));
        let (backend, _network, sink) = beside_the(&fake);

        let stream = long_stream(&backend, &sink).expect("the address was taken");
        let began = Instant::now();

        stream.set_active(true).expect("asks to play");

        let heard = events_until(stream.events(), is_failed);

        assert!(
            heard.last().is_some_and(is_failed),
            "the stream was never failed: {heard:?}"
        );
        assert!(began.elapsed() < GIVEN_UP_BY);

        stream.close().expect("closes");
    }
}

#[test]
fn a_renderer_faulting_pause_still_pauses_the_stream_and_plays_on_when_asked() {
    let fake = Fake::with(Profile::faulting(
        On::Action("Pause"),
        Misbehaviour::Fault(701),
    ));
    let (backend, _network, sink) = beside_the(&fake);

    let stream = long_stream(&backend, &sink).expect("the address was taken");

    stream.set_active(true).expect("plays");
    stream.set_active(false).expect("pauses");

    let paused = events_until(stream.events(), |event| {
        matches!(
            event,
            StreamEvent::StateChanged {
                to: StreamState::Paused,
                ..
            }
        )
    });

    stream.set_active(true).expect("plays on");
    fake.until("the renderer to be played again", |heard| {
        heard.times("Play") == 2
    });

    let again = events_until(stream.events(), |event| {
        matches!(
            event,
            StreamEvent::StateChanged {
                to: StreamState::Streaming,
                ..
            }
        )
    });

    assert!(paused.last().is_some(), "the pause was never told");
    assert!(
        paused
            .iter()
            .chain(&again)
            .all(|event| !is_failed(event) && !is_refused(event)),
        "a refused pause failed the stream: {paused:?} {again:?}"
    );
    assert_eq!(fake.heard.lock().times("Pause"), 1);

    stream.close().expect("closes");
    fake.until("the renderer to be stopped", |heard| {
        heard.times("Stop") == 1
    });
}

#[test]
fn a_renderer_faulting_stop_still_has_its_stream_ended_and_withdrawn() {
    let fake = Fake::with(Profile::faulting(
        On::Action("Stop"),
        Misbehaviour::Fault(701),
    ));
    let (backend, _network, sink) = beside_the(&fake);

    let stream = long_stream(&backend, &sink).expect("the address was taken");

    stream.set_active(true).expect("plays");
    fake.until("the renderer to be fetching", |heard| {
        heard.fetching.is_some()
    });
    stream.close().expect("closes");
    fake.until("the stream to be ended and stopped", |heard| {
        heard.fetch_ended && heard.times("Stop") == 1
    });

    let again = long_stream(&backend, &sink).expect("a renderer that faulted a stop is free");

    drop(again);
}

#[test]
fn a_renderer_that_stops_answering_where_it_is_ends_the_stream_after_six_misses() {
    let cases: [(Misbehaviour, Ended); 2] = [
        (Misbehaviour::Fault(501), |event| {
            matches!(
                event,
                StreamEvent::Refused {
                    from: StreamState::Streaming,
                    status: 501,
                    ..
                }
            )
        }),
        (Misbehaviour::Status(503), is_failed),
    ];

    for (how, ended) in cases {
        let fake = Fake::with(Profile::faulting(On::Action("GetPositionInfo"), how));
        let (backend, _network, sink) = beside_the(&fake);

        let stream = long_stream(&backend, &sink).expect("the address was taken");

        stream.set_active(true).expect("plays");

        let heard = events_until(stream.events(), ended);

        assert!(
            heard.last().is_some_and(ended),
            "the stream never ended: {heard:?}"
        );
        assert!(fake.heard.lock().times("GetPositionInfo") >= 6);

        stream.close().expect("closes");
    }
}

#[test]
fn a_renderer_faulting_the_volume_is_asked_without_waiting_and_is_asked_again() {
    let fake = Fake::with(Profile::faulting(
        On::Action("SetVolume"),
        Misbehaviour::Fault(501),
    ));
    let (backend, _network, sink) = beside_the(&fake);
    let half = Volume::new(0.5).expect("a volume").to_gain();

    backend
        .set_device_volume(sink.id, half)
        .expect("a refused volume is not the caller's to wait for");
    fake.until("the volume to reach the renderer", |heard| {
        heard.times("SetVolume") == 1
    });
    fake.heal();
    backend
        .set_device_volume(sink.id, half)
        .expect("turns again");
    fake.until("the volume to reach the renderer again", |heard| {
        heard.times("SetVolume") == 2
    });
}

#[test]
fn a_stalled_volume_never_holds_the_caller_and_the_next_turn_goes_through_once_it_gives_up() {
    let fake = Fake::with(Profile::faulting(
        On::Action("SetVolume"),
        Misbehaviour::Stall,
    ));
    let (backend, _network, sink) = beside_the(&fake);
    let half = Volume::new(0.5).expect("a volume").to_gain();
    let asked_at = Instant::now();

    backend.set_device_volume(sink.id, half).expect("turns");
    backend.set_device_mute(sink.id, true).expect("mutes");

    assert!(
        asked_at.elapsed() < SOONER_THAN,
        "a stalled renderer held the volume's caller for {:?}",
        asked_at.elapsed()
    );

    fake.until("the stalled volume to arrive", |heard| {
        heard.times("SetVolume") == 1
    });
    fake.heal();
    backend
        .set_device_volume(sink.id, half)
        .expect("turns again");
    fake.until(
        "the volume to arrive once the stall was given up",
        |heard| heard.times("SetVolume") == 2,
    );
}

#[test]
fn a_seek_on_a_renderer_that_faults_stop_still_offers_a_fresh_address_and_plays_on() {
    let fake = Fake::with(Profile::faulting(
        On::Action("Stop"),
        Misbehaviour::Fault(701),
    ));
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
    player
        .send(Command::Seek(Frames::from_duration(
            lands_at,
            SampleRate::HZ_44100,
        )))
        .expect("seeks");
    fake.until("a second address to be fetched", |heard| {
        heard.offers.len() >= 2 && heard.fetching == heard.offers.last().cloned()
    });
    finished(&player);

    let rest = &pcm[pcm_bytes(lands_at + PAST_THE_FADE_IN)..];

    fake.until("the fetch to take the song from where it landed", |heard| {
        contains(&heard.fetched, rest)
    });

    let heard = fake.heard.lock();
    let actions = heard.actions();
    let stopped = actions
        .iter()
        .position(|action| *action == "Stop")
        .expect("the renderer was asked to stop");

    assert_eq!(
        actions[stopped..stopped + 3],
        ["Stop", "SetAVTransportURI", "Play"],
        "{actions:?}"
    );
}

#[test]
fn a_renderer_faulting_play_leaves_the_player_told_the_device_refused_rather_than_playing_on() {
    let fake = Fake::with(Profile::faulting(PLAY, Misbehaviour::Fault(701)));
    let network =
        Arc::new(Network::start(fake.searching(), FirstSearch::Awaited).expect("searches"));
    let tree = Tree::new();

    let path = tree.write("song.wav", &wave_file(&music(RATE as usize)));
    let player = playing_on(&network);
    let since = Instant::now();

    loaded(&player, &path);

    let told = loop {
        match player.events().recv_timeout(LOOKS_EVERY) {
            Ok(Event::Failed { error, .. } | Event::Waiting { error, .. }) => break error,
            Ok(Event::QueueFinished) => panic!("a refused Play finished the queue"),
            Ok(_) | Err(_) => {}
        }

        assert!(
            since.elapsed() < WAITED_AT_MOST,
            "the player was never told"
        );
    };

    assert!(
        matches!(
            told,
            EngineError::Sink(SinkError::StreamRefused {
                op: Operation::Start,
                status: 701,
                ..
            })
        ),
        "{told:?}"
    );
}

#[test]
fn a_description_misbehaving_is_refused_as_what_it_did_and_lists_nothing() {
    let cases: [(Misbehaviour, Expected); 5] = [
        (Misbehaviour::Fault(401), |error| {
            matches!(
                error,
                UpnpError::Status {
                    asked: Asked::Description,
                    status: 500
                }
            )
        }),
        (Misbehaviour::Status(503), |error| {
            matches!(
                error,
                UpnpError::Status {
                    asked: Asked::Description,
                    status: 503
                }
            )
        }),
        (PAST_THE_ANSWER, |error| {
            matches!(
                error,
                UpnpError::TooLong {
                    asked: Asked::Description,
                    at_most: ANSWER_BYTES_AT_MOST
                }
            )
        }),
        (Misbehaviour::Endless, |error| {
            matches!(
                error,
                UpnpError::TooLong {
                    asked: Asked::Description,
                    ..
                }
            )
        }),
        (PAST_THE_HEAD, |error| {
            matches!(
                error,
                UpnpError::NotHttp {
                    asked: Asked::Description
                }
            )
        }),
    ];

    for (how, expected) in cases {
        let fake = Fake::with(Profile::faulting(On::Description, how));
        let began = Instant::now();

        let (_deaf, network, located) = reached_by_address(&fake);

        assert!(began.elapsed() < GIVEN_UP_BY, "the description hung");
        assert!(
            located.as_ref().err().is_some_and(expected),
            "{located:?} was not the failure its fault should be"
        );
        assert!(network.sinks().is_empty());
    }
}

#[test]
fn a_description_that_never_comes_is_given_up_on_at_the_rounds_deadline_and_lists_nothing() {
    let fake = Fake::with(Profile::faulting(On::Description, Misbehaviour::Stall));

    let (_deaf, network, took) = round_over(&fake);

    assert!(took < WELL_PAST_A_ROUND, "the round took {took:?}");
    assert!(network.sinks().is_empty());
}

#[test]
fn a_description_of_exactly_the_most_bytes_is_read_and_a_head_under_its_limit_too() {
    for how in [
        Misbehaviour::Body(ANSWER_BYTES_AT_MOST),
        Misbehaviour::Head(HEAD_BYTES_AT_MOST / 2),
    ] {
        let fake = Fake::with(Profile::faulting(On::Description, how));

        let (_deaf, network, located) = reached_by_address(&fake);

        assert!(located.is_ok(), "{located:?}");
        assert_eq!(network.sinks().len(), 1);
    }
}

#[test]
fn a_connection_manager_misbehaving_leaves_the_renderer_listed_as_taking_wave_alone() {
    let cases = [
        Misbehaviour::Fault(501),
        Misbehaviour::Status(503),
        PAST_THE_ANSWER,
        Misbehaviour::Endless,
        PAST_THE_HEAD,
        Misbehaviour::Stall,
    ];

    for how in cases {
        let fake = Fake::with(Profile::faulting(On::Action("GetProtocolInfo"), how));

        let (_deaf, network, took) = round_over(&fake);
        let sink = the_renderer(&network);

        assert!(took < WELL_PAST_A_ROUND, "the round took {took:?}");
        assert!(sink.supports(stereo_sixteen()));
        assert!(
            !sink.supports(StreamSpec::new(
                SampleRate::HZ_44100,
                ChannelLayout::Stereo,
                SampleFormat::F32
            )),
            "a renderer that did not say what it takes was offered more than WAVE"
        );
    }
}

#[test]
fn a_volume_answer_of_exactly_the_most_bytes_is_read_and_one_byte_more_is_not() {
    for (how, read) in [
        (Misbehaviour::Body(ANSWER_BYTES_AT_MOST), true),
        (PAST_THE_ANSWER, false),
        (Misbehaviour::Head(HEAD_BYTES_AT_MOST / 2), true),
        (PAST_THE_HEAD, false),
        (Misbehaviour::Status(503), false),
        (Misbehaviour::Fault(501), false),
    ] {
        let fake = Fake::with(Profile::faulting(On::Action("GetVolume"), how));
        let network = Network::start(fake.searching(), FirstSearch::Awaited).expect("searches");
        let sink = the_renderer(&network);
        let port = sink.port.expect("a renderer names where it is");

        assert_eq!(port.volume.is_some(), read);
        assert_eq!(port.hardware_volume, HardwareVolume::Always);
    }
}

#[test]
fn a_friendly_name_far_past_its_limit_is_shown_cut_on_a_letter_and_the_renderer_still_plays() {
    let fake = Fake::with(Profile::named_by(A_LONG_NAME_IN_LETTERS));
    let (backend, _network, sink) = beside_the(&fake);

    assert_eq!(sink.description.len(), NAME_BYTES_AT_MOST);
    assert!(sink.description.chars().all(|letter| letter == 'é'));
    assert_eq!(sink.name, NodeName::new(format!("upnp:{UDN}")));

    let stream = long_stream(&backend, &sink).expect("a long name does not stop the stream");

    stream.set_active(true).expect("plays");
    fake.until("the renderer to be played", |heard| {
        heard.times("Play") == 1
    });
    stream.close().expect("closes");
}
