use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    io::{Read as _, Write as _},
    sync::{Arc, LazyLock},
    thread,
    time::{Duration, Instant},
};

use flate2::{Compression, write::GzEncoder};
use parking_lot::{Mutex, RwLock};
use resonate_library::LookupOp;
use serde::de::DeserializeOwned;
use ureq::{
    Agent, Body,
    http::{
        HeaderMap, Response, StatusCode,
        header::{AUTHORIZATION, RETRY_AFTER, USER_AGENT},
    },
};

use crate::{Error, Host, Result, query::Params};

pub(crate) const LARGEST_DOCUMENT: usize = 4 * 1024 * 1024;
pub(crate) const LARGEST_PICTURE: usize = 8 * 1024 * 1024;

const REFUSED_FOR_GOOD: u16 = 500;

pub(crate) fn passed_over_when_refused<T>(answered: Result<Option<T>>) -> Result<Option<T>> {
    match answered {
        Err(Error::Refused { host, status, .. }) if status < REFUSED_FOR_GOOD => {
            tracing::debug!(
                ?host,
                status,
                "a picture was refused; the next link is tried"
            );
            Ok(None)
        }
        answered => answered,
    }
}
pub(crate) const LARGEST_INDEX: usize = 2 * 1024 * 1024;

const NAME: &str = "resonate";
const CONTENT_LANGUAGE: &str = "en_US";
const FORM: &str = "application/x-www-form-urlencoded";
const GZIP: &str = "gzip";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Encoded {
    Plain,
    Gzip,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Posted {
    pub(crate) content_type: String,
    pub(crate) encoded: Encoded,
    pub(crate) bytes: Vec<u8>,
    pub(crate) authorization: Option<String>,
}

impl Posted {
    pub(crate) fn packed_form(fields: Params) -> Self {
        let plain = fields.finish_as_form().into_bytes();
        let mut packing = GzEncoder::new(Vec::new(), Compression::best());
        let packed = packing.write_all(&plain).and_then(|()| packing.finish());

        match packed {
            Ok(bytes) => Self {
                content_type: FORM.to_owned(),
                encoded: Encoded::Gzip,
                bytes,
                authorization: None,
            },
            Err(error) => {
                tracing::debug!(%error, "a form could not be packed and is sent as it stands");
                Self {
                    content_type: FORM.to_owned(),
                    encoded: Encoded::Plain,
                    bytes: plain,
                    authorization: None,
                }
            }
        }
    }
}
const CONNECT_WITHIN: Duration = Duration::from_secs(10);
const ANSWER_WITHIN: Duration = Duration::from_secs(30);
const MUSICBRAINZ_INTERVAL: Duration = Duration::from_secs(1);
const OTHERS_INTERVAL: Duration = Duration::from_millis(250);
const ACOUSTID_INTERVAL: Duration = Duration::from_millis(334);
const SHAZAM_INTERVAL: Duration = Duration::from_secs(3);
const AUDD_INTERVAL: Duration = Duration::from_secs(1);
const LISTENBRAINZ_INTERVAL: Duration = Duration::from_secs(1);
const RETRY_AFTER_AT_MOST: Duration = Duration::from_secs(10);
const RETRY_AFTER_BY_DEFAULT: Duration = Duration::from_secs(2);
const BUSY_RETRIES: u32 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub version: &'static str,
    pub contact: Option<String>,
}

impl Identity {
    pub fn of_this_build() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION"),
            contact: None,
        }
    }

    pub fn user_agent(&self) -> String {
        let contact = self
            .contact
            .as_deref()
            .map(str::trim)
            .filter(|contact| !contact.is_empty());

        match contact {
            Some(contact) => format!("{} ( {contact} )", self.named()),
            None => self.named(),
        }
    }

    pub fn user_agent_to(&self, host: Host) -> String {
        if host.asks_who_is_asking() {
            self.user_agent()
        } else {
            self.named()
        }
    }

    fn named(&self) -> String {
        format!("{NAME}/{}", self.version)
    }
}

impl Host {
    const fn asks_who_is_asking(self) -> bool {
        match self {
            Self::MusicBrainz
            | Self::CoverArtArchive
            | Self::ListenBrainz
            | Self::Commons
            | Self::Wikidata => true,
            Self::Lrclib
            | Self::AutoEq
            | Self::AcoustId
            | Self::Shazam
            | Self::AppleArtwork
            | Self::AppleMusic
            | Self::Audd
            | Self::Deezer
            | Self::DeezerPictures
            | Self::Spotify
            | Self::SpotifyPictures
            | Self::SoundCloud
            | Self::SoundCloudPictures => false,
        }
    }
}

pub type Reintroduction = Arc<dyn Fn() -> Option<Identity> + Send + Sync>;

#[derive(Clone)]
pub struct Introduction {
    said: Arc<RwLock<Identity>>,
    follows: Option<Reintroduction>,
}

impl fmt::Debug for Introduction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Introduction")
            .field("said", &self.said.read().user_agent())
            .field("follows", &self.follows.is_some())
            .finish()
    }
}

impl Introduction {
    pub fn as_(identity: &Identity) -> Self {
        Self {
            said: Arc::new(RwLock::new(identity.clone())),
            follows: None,
        }
    }

    pub fn following(identity: &Identity, follows: Reintroduction) -> Self {
        Self {
            follows: Some(follows),
            ..Self::as_(identity)
        }
    }

    pub fn change_to(&self, identity: &Identity) {
        *self.said.write() = identity.clone();
    }

    pub fn user_agent(&self) -> String {
        self.heard_again();
        self.said.read().user_agent()
    }

    pub fn user_agent_to(&self, host: Host) -> String {
        self.heard_again();
        self.said.read().user_agent_to(host)
    }

    fn heard_again(&self) {
        if let Some(identity) = self.follows.as_ref().and_then(|follows| follows()) {
            self.change_to(&identity);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Carried {
    Encrypted,
    #[cfg(test)]
    Plain,
}

pub(crate) trait Clock: Send + Sync {
    fn now(&self) -> Instant;

    fn sleep(&self, span: Duration);
}

struct WallClock;

impl Clock for WallClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep(&self, span: Duration) {
        thread::sleep(span);
    }
}

impl Host {
    fn interval(self) -> Duration {
        match self {
            Self::MusicBrainz => MUSICBRAINZ_INTERVAL,
            Self::AcoustId => ACOUSTID_INTERVAL,
            Self::Shazam => SHAZAM_INTERVAL,
            Self::ListenBrainz => LISTENBRAINZ_INTERVAL,
            Self::Audd => AUDD_INTERVAL,
            Self::AppleArtwork
            | Self::AppleMusic
            | Self::Deezer
            | Self::DeezerPictures
            | Self::Spotify
            | Self::SpotifyPictures
            | Self::SoundCloud
            | Self::SoundCloudPictures
            | Self::CoverArtArchive
            | Self::Commons
            | Self::Wikidata
            | Self::Lrclib
            | Self::AutoEq => OTHERS_INTERVAL,
        }
    }
}

#[derive(Default)]
struct Turns {
    next: BTreeMap<Host, Instant>,
    stayed_busy: BTreeSet<Host>,
}

#[derive(Clone, Default)]
struct Pacing(Arc<Mutex<Turns>>);

static EVERY_CLIENT_IN_THE_PROCESS: LazyLock<Pacing> = LazyLock::new(Pacing::default);

impl Pacing {
    fn reserve(&self, host: Host, now: Instant) -> Instant {
        let mut turns = self.0.lock();
        let slot = turns
            .next
            .get(&host)
            .map_or(now, |last| (*last + host.interval()).max(now));
        turns.next.insert(host, slot);
        slot
    }

    fn retries_owed(&self, host: Host) -> u32 {
        if self.0.lock().stayed_busy.contains(&host) {
            0
        } else {
            BUSY_RETRIES
        }
    }

    fn heard(&self, host: Host, status: StatusCode) {
        let mut turns = self.0.lock();
        if busy(status) {
            turns.stayed_busy.insert(host);
        } else {
            turns.stayed_busy.remove(&host);
        }
    }
}

pub struct Client {
    agent: Agent,
    introduction: Introduction,
    pacing: Pacing,
    clock: Arc<dyn Clock>,
}

impl Client {
    pub fn new(identity: Identity) -> Self {
        Self::introduced(Introduction::as_(&identity))
    }

    pub fn introduced(introduction: Introduction) -> Self {
        Self::paced_by(
            introduction,
            Arc::new(WallClock),
            Carried::Encrypted,
            EVERY_CLIENT_IN_THE_PROCESS.clone(),
        )
    }

    #[cfg(test)]
    pub(crate) fn on_clock(
        introduction: Introduction,
        clock: Arc<dyn Clock>,
        carried: Carried,
    ) -> Self {
        Self::paced_by(introduction, clock, carried, Pacing::default())
    }

    fn paced_by(
        introduction: Introduction,
        clock: Arc<dyn Clock>,
        carried: Carried,
        pacing: Pacing,
    ) -> Self {
        let config = Agent::config_builder()
            .user_agent(Identity::of_this_build().named().as_str())
            .https_only(carried == Carried::Encrypted)
            .timeout_connect(Some(CONNECT_WITHIN))
            .timeout_global(Some(ANSWER_WITHIN))
            .http_status_as_error(false)
            .build();

        Self {
            agent: config.new_agent(),
            introduction,
            pacing,
            clock,
        }
    }

    pub fn user_agent(&self) -> String {
        self.introduction.user_agent()
    }

    pub(crate) fn json<T: DeserializeOwned>(
        &self,
        host: Host,
        op: LookupOp,
        path_and_query: &str,
    ) -> Result<Option<T>> {
        let url = format!("{}{path_and_query}", host.base());
        let Some(bytes) = self.bytes(host, op, &url, LARGEST_DOCUMENT)? else {
            return Ok(None);
        };

        serde_json::from_slice(&bytes).map(Some).map_err(|error| {
            tracing::debug!(%error, ?host, ?op, url, "an answer was not the document expected");
            Error::Unreadable { host, op }
        })
    }

    pub(crate) fn posted<T: DeserializeOwned>(
        &self,
        host: Host,
        op: LookupOp,
        url: &str,
        body: &Posted,
    ) -> Result<Option<T>> {
        let response = self.exchange(host, op, url, Some(body))?;
        let Some(bytes) = Self::read(response, host, op, LARGEST_DOCUMENT)? else {
            return Ok(None);
        };
        serde_json::from_slice(&bytes).map(Some).map_err(|error| {
            tracing::debug!(%error, ?host, ?op, url, "an answer was not the document expected");
            Error::Unreadable { host, op }
        })
    }

    pub(crate) fn bytes(
        &self,
        host: Host,
        op: LookupOp,
        url: &str,
        limit: usize,
    ) -> Result<Option<Vec<u8>>> {
        let response = self.exchange(host, op, url, None)?;
        Self::read(response, host, op, limit)
    }

    fn read(
        mut response: Response<Body>,
        host: Host,
        op: LookupOp,
        limit: usize,
    ) -> Result<Option<Vec<u8>>> {
        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(Error::Refused {
                host,
                op,
                status: status.as_u16(),
            });
        }

        let mut read = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take((limit as u64).saturating_add(1))
            .read_to_end(&mut read)
            .map_err(|error| Error::from_ureq(host, op, ureq::Error::from(error)))?;
        if read.len() > limit {
            return Err(Error::TooLarge { host, op, limit });
        }
        Ok(Some(read))
    }

    fn exchange(
        &self,
        host: Host,
        op: LookupOp,
        url: &str,
        body: Option<&Posted>,
    ) -> Result<Response<Body>> {
        let owed = self.pacing.retries_owed(host);
        let mut retried = 0;
        let mut by_default = RETRY_AFTER_BY_DEFAULT;
        loop {
            self.pace(host);
            let introduced = self.introduction.user_agent_to(host);
            let sent = match body {
                None => self.agent.get(url).header(USER_AGENT, &introduced).call(),
                Some(posted) => {
                    let request = self
                        .agent
                        .post(url)
                        .header(USER_AGENT, &introduced)
                        .header("Content-Type", posted.content_type.as_str())
                        .header("Content-Language", CONTENT_LANGUAGE);
                    let request = match &posted.authorization {
                        Some(authorization) => request.header(AUTHORIZATION, authorization),
                        None => request,
                    };
                    match posted.encoded {
                        Encoded::Plain => request,
                        Encoded::Gzip => request.header("Content-Encoding", GZIP),
                    }
                    .send(posted.bytes.as_slice())
                }
            };
            let response = sent.map_err(|error| Error::from_ureq(host, op, error))?;

            if !busy(response.status()) || retried == owed {
                self.pacing.heard(host, response.status());
                return Ok(response);
            }

            retried += 1;
            let wait = cooling_off(response.headers(), by_default);
            by_default = (by_default * 2).min(RETRY_AFTER_AT_MOST);
            tracing::debug!(
                ?host,
                ?op,
                url,
                ?wait,
                retried,
                "the service is unavailable; trying again"
            );
            self.clock.sleep(wait);
        }
    }

    fn pace(&self, host: Host) {
        let now = self.clock.now();
        let slot = self.pacing.reserve(host, now);

        if slot > now {
            self.clock.sleep(slot - now);
        }
    }
}

fn busy(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::TOO_MANY_REQUESTS
            | StatusCode::BAD_GATEWAY
            | StatusCode::GATEWAY_TIMEOUT
    )
}

fn cooling_off(headers: &HeaderMap, by_default: Duration) -> Duration {
    headers
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map_or(by_default, Duration::from_secs)
        .min(RETRY_AFTER_AT_MOST)
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        thread::JoinHandle,
    };

    use ureq::http::HeaderValue;

    use super::*;

    struct Answer {
        status: u16,
        retry_after: Option<u64>,
    }

    const BUSY: Answer = Answer {
        status: 503,
        retry_after: None,
    };
    const FINE: Answer = Answer {
        status: 200,
        retry_after: None,
    };

    fn busy_for(seconds: u64) -> Answer {
        Answer {
            status: 503,
            retry_after: Some(seconds),
        }
    }

    fn answer(mut stream: TcpStream, answer: &Answer) {
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).expect("a request") == 1 {
            head.push(byte[0]);
        }
        let reason = match answer.status {
            200 => "OK",
            _ => "Service Unavailable",
        };
        let mut response = format!("HTTP/1.1 {} {reason}\r\n", answer.status);
        if let Some(seconds) = answer.retry_after {
            response.push_str(&format!("Retry-After: {seconds}\r\n"));
        }
        response.push_str("Content-Length: 0\r\nConnection: close\r\n\r\n");
        stream.write_all(response.as_bytes()).expect("an answer");
    }

    fn serving_gzipped(decoded: Vec<u8>) -> (String, JoinHandle<()>) {
        let mut packed = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        packed.write_all(&decoded).expect("a packed body");
        let packed = packed.finish().expect("a packed body");

        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let url = format!(
            "http://{}/",
            listener.local_addr().expect("a bound address")
        );
        let served = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("a connection");
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).expect("a request") == 1 {
                head.push(byte[0]);
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n",
                packed.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(&packed);
        });
        (url, served)
    }

    #[test]
    fn a_cap_bounds_the_body_as_it_is_decoded_rather_than_as_it_arrived() {
        const CAP: usize = 64 * 1024;

        let client = Client::on_clock(
            Introduction::as_(&identity(None)),
            Faked::new(),
            Carried::Plain,
        );
        let (url, served) = serving_gzipped(vec![0; CAP * 16]);
        let read = client.bytes(Host::CoverArtArchive, LookupOp::Cover, &url, CAP);
        served.join().expect("the server");
        assert!(
            matches!(read, Err(Error::TooLarge { limit: CAP, .. })),
            "a body sixteen times the cap was read whole: {read:?}"
        );

        let (url, served) = serving_gzipped(vec![7; CAP]);
        let read = client.bytes(Host::CoverArtArchive, LookupOp::Cover, &url, CAP);
        served.join().expect("the server");
        assert_eq!(
            read.expect("a body at the cap").map(|held| held.len()),
            Some(CAP)
        );
    }

    fn serving_one_post() -> (String, JoinHandle<(String, Vec<u8>)>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let url = format!(
            "http://{}/",
            listener.local_addr().expect("a bound address")
        );
        let served = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("a connection");
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).expect("a request") == 1 {
                head.push(byte[0]);
            }
            let head = String::from_utf8(head)
                .expect("a head in ASCII")
                .to_ascii_lowercase();
            let length = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .and_then(|length| length.trim().parse::<usize>().ok())
                .expect("a declared length");
            let mut body = vec![0; length];
            stream.read_exact(&mut body).expect("the body");
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
            (head, body)
        });
        (url, served)
    }

    #[test]
    fn a_contact_given_after_the_client_was_built_is_what_the_next_request_says() {
        let introduction = Introduction::as_(&identity(None));
        let client = Client::on_clock(introduction.clone(), Faked::new(), Carried::Plain);
        introduction.change_to(&identity(Some("someone who typed a contact")));
        let (url, served) = serving_one_post();

        let _: Option<serde_json::Value> = client
            .posted(
                Host::MusicBrainz,
                LookupOp::Recognise,
                &url,
                &Posted::packed_form(Params::new().with("client", "a key")),
            )
            .expect("an answer");
        let (head, _) = served.join().expect("the server");

        assert_eq!(head.matches("user-agent: ").count(), 1, "{head}");
        assert!(
            head.contains("user-agent: resonate/9.9.9 ( someone who typed a contact )\r\n"),
            "{head}"
        );
        assert_eq!(
            client.user_agent(),
            "resonate/9.9.9 ( someone who typed a contact )"
        );
    }

    #[test]
    fn a_contact_is_told_to_the_hosts_that_ask_who_is_asking_and_no_other() {
        let introduction = Introduction::as_(&identity(Some("someone who typed a contact")));
        let client = Client::on_clock(introduction, Faked::new(), Carried::Plain);

        for (host, told) in [
            (Host::MusicBrainz, true),
            (Host::CoverArtArchive, true),
            (Host::ListenBrainz, true),
            (Host::Wikidata, true),
            (Host::Shazam, false),
            (Host::Audd, false),
            (Host::AcoustId, false),
            (Host::Spotify, false),
            (Host::AutoEq, false),
        ] {
            let (url, served) = serving_one_post();
            let _: Option<serde_json::Value> = client
                .posted(
                    host,
                    LookupOp::Recognise,
                    &url,
                    &Posted::packed_form(Params::new().with("client", "a key")),
                )
                .expect("an answer");
            let (head, _) = served.join().expect("the server");

            let said = if told {
                "user-agent: resonate/9.9.9 ( someone who typed a contact )\r\n"
            } else {
                "user-agent: resonate/9.9.9\r\n"
            };
            assert!(head.contains(said), "{host:?} was told {head}");
            assert_eq!(head.matches("user-agent: ").count(), 1, "{head}");
        }
    }

    #[test]
    fn an_introduction_that_follows_a_file_says_what_the_file_says_by_the_next_request() {
        let written: Arc<Mutex<Option<Option<String>>>> = Arc::new(Mutex::new(None));
        let read = Arc::clone(&written);
        let introduction = Introduction::following(
            &identity(None),
            Arc::new(move || {
                read.lock()
                    .take()
                    .map(|contact| identity(contact.as_deref()))
            }),
        );
        let client = Client::on_clock(introduction, Faked::new(), Carried::Plain);

        assert_eq!(client.user_agent(), "resonate/9.9.9");

        *written.lock() = Some(Some("someone who edited the file".to_owned()));
        let (url, served) = serving_one_post();
        let _: Option<serde_json::Value> = client
            .posted(
                Host::ListenBrainz,
                LookupOp::Recognise,
                &url,
                &Posted::packed_form(Params::new().with("client", "a key")),
            )
            .expect("an answer");
        let (head, _) = served.join().expect("the server");
        assert!(
            head.contains("user-agent: resonate/9.9.9 ( someone who edited the file )\r\n"),
            "{head}"
        );

        assert_eq!(
            client.user_agent(),
            "resonate/9.9.9 ( someone who edited the file )",
            "a file that has not moved since changed what is said"
        );

        *written.lock() = Some(None);
        assert_eq!(client.user_agent(), "resonate/9.9.9");
    }

    #[test]
    fn a_packed_form_is_posted_gzipped_and_says_so() {
        let client = Client::on_clock(
            Introduction::as_(&identity(None)),
            Faked::new(),
            Carried::Plain,
        );
        let (url, served) = serving_one_post();
        let fields = Params::new()
            .with("client", "a key")
            .with("fingerprint", "AQAA-_x");

        let answer: Option<serde_json::Value> = client
            .posted(
                Host::AcoustId,
                LookupOp::Recognise,
                &url,
                &Posted::packed_form(fields),
            )
            .expect("an answer");
        let (head, body) = served.join().expect("the server");

        assert_eq!(answer, Some(serde_json::json!({})));
        assert!(head.contains("content-encoding: gzip\r\n"), "{head}");
        assert!(
            head.contains(&format!("content-type: {FORM}\r\n")),
            "{head}"
        );
        let mut unpacked = String::new();
        flate2::read::GzDecoder::new(body.as_slice())
            .read_to_string(&mut unpacked)
            .expect("a gzipped body");
        assert_eq!(unpacked, "client=a%20key&fingerprint=AQAA-_x");
    }

    #[test]
    fn a_client_that_meets_the_network_refuses_a_plain_address() {
        let client = Client::new(identity(None));
        let (url, _served) = serving_gzipped(Vec::new());
        let read = client.bytes(Host::CoverArtArchive, LookupOp::Cover, &url, 1);
        assert!(matches!(read, Err(Error::Unreachable { .. })), "{read:?}");
    }

    fn serving(answers: Vec<Answer>) -> (String, JoinHandle<usize>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let url = format!(
            "http://{}/",
            listener.local_addr().expect("a bound address")
        );
        let served = thread::spawn(move || {
            let mut served = 0;
            for scripted in &answers {
                let (stream, _) = listener.accept().expect("a connection");
                answer(stream, scripted);
                served += 1;
            }
            served
        });
        (url, served)
    }

    fn fetched(clock: &Arc<Faked>, answers: Vec<Answer>) -> (u16, usize) {
        let client = Client::on_clock(
            Introduction::as_(&identity(None)),
            clock.clone(),
            Carried::Plain,
        );
        fetched_by(&client, answers)
    }

    fn fetched_by(client: &Client, answers: Vec<Answer>) -> (u16, usize) {
        let (url, served) = serving(answers);
        let response = client
            .exchange(Host::MusicBrainz, LookupOp::FindRecording, &url, None)
            .expect("an answer");
        (
            response.status().as_u16(),
            served.join().expect("the server"),
        )
    }

    struct Faked {
        now: Mutex<Instant>,
        slept: Mutex<Vec<Duration>>,
    }

    impl Faked {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                now: Mutex::new(Instant::now()),
                slept: Mutex::new(Vec::new()),
            })
        }

        fn slept(&self) -> Vec<Duration> {
            self.slept.lock().clone()
        }
    }

    impl Clock for Faked {
        fn now(&self) -> Instant {
            *self.now.lock()
        }

        fn sleep(&self, span: Duration) {
            *self.now.lock() += span;
            self.slept.lock().push(span);
        }
    }

    fn identity(contact: Option<&str>) -> Identity {
        Identity {
            version: "9.9.9",
            contact: contact.map(str::to_owned),
        }
    }

    #[test]
    fn the_user_agent_names_the_build_and_nothing_else_unless_a_contact_is_given() {
        assert_eq!(identity(None).user_agent(), "resonate/9.9.9");
        assert_eq!(identity(Some("")).user_agent(), "resonate/9.9.9");
        assert_eq!(identity(Some("   ")).user_agent(), "resonate/9.9.9");
        assert_eq!(
            identity(Some("a contact the user typed")).user_agent(),
            "resonate/9.9.9 ( a contact the user typed )"
        );

        let built = Identity::of_this_build();
        assert_eq!(built.contact, None);
        assert_eq!(
            Client::new(built).user_agent(),
            format!("resonate/{}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn two_musicbrainz_requests_are_a_second_apart_and_other_hosts_a_quarter() {
        let clock = Faked::new();
        let client = Client::on_clock(
            Introduction::as_(&identity(None)),
            clock.clone(),
            Carried::Plain,
        );

        client.pace(Host::MusicBrainz);
        client.pace(Host::MusicBrainz);
        assert_eq!(clock.slept(), vec![MUSICBRAINZ_INTERVAL]);

        client.pace(Host::Lrclib);
        client.pace(Host::Lrclib);
        assert_eq!(clock.slept(), vec![MUSICBRAINZ_INTERVAL, OTHERS_INTERVAL]);

        client.pace(Host::CoverArtArchive);
        client.pace(Host::Commons);
        assert_eq!(clock.slept(), vec![MUSICBRAINZ_INTERVAL, OTHERS_INTERVAL]);

        client.clock.sleep(MUSICBRAINZ_INTERVAL);
        client.pace(Host::MusicBrainz);
        assert_eq!(
            clock.slept(),
            vec![MUSICBRAINZ_INTERVAL, OTHERS_INTERVAL, MUSICBRAINZ_INTERVAL]
        );
    }

    #[test]
    fn a_busy_service_is_asked_three_more_times_with_the_wait_doubling_between() {
        let clock = Faked::new();
        let (status, served) = fetched(&clock, vec![BUSY, BUSY, BUSY, FINE]);

        assert_eq!(status, 200);
        assert_eq!(served, 4);
        assert_eq!(
            clock.slept(),
            vec![
                Duration::from_secs(2),
                Duration::from_secs(4),
                Duration::from_secs(8)
            ]
        );
    }

    #[test]
    fn every_client_in_the_process_takes_its_turn_in_one_queue_per_host() {
        let clock = Faked::new();
        let pacing = Pacing::default();

        let reference = Client::paced_by(
            Introduction::as_(&identity(None)),
            clock.clone(),
            Carried::Plain,
            pacing.clone(),
        );
        let recogniser = Client::paced_by(
            Introduction::as_(&identity(None)),
            clock.clone(),
            Carried::Plain,
            pacing,
        );

        reference.pace(Host::MusicBrainz);
        recogniser.pace(Host::MusicBrainz);
        reference.pace(Host::MusicBrainz);
        assert_eq!(
            clock.slept(),
            vec![MUSICBRAINZ_INTERVAL, MUSICBRAINZ_INTERVAL]
        );

        let first = Client::new(identity(None));
        let second = Client::introduced(Introduction::as_(&identity(Some("a contact"))));
        assert!(Arc::ptr_eq(&first.pacing.0, &second.pacing.0));
    }

    #[test]
    fn a_gateway_that_failed_or_timed_out_is_asked_again_like_a_busy_service() {
        let clock = Faked::new();
        let bad_gateway = Answer {
            status: 502,
            retry_after: None,
        };
        let gateway_timeout = Answer {
            status: 504,
            retry_after: None,
        };
        let (status, served) = fetched(&clock, vec![bad_gateway, gateway_timeout, FINE]);

        assert_eq!(status, 200);
        assert_eq!(served, 3);
        assert_eq!(
            clock.slept(),
            vec![Duration::from_secs(2), Duration::from_secs(4)]
        );
    }

    #[test]
    fn a_host_busy_through_every_retry_is_asked_once_until_it_answers() {
        let clock = Faked::new();
        let client = Client::on_clock(
            Introduction::as_(&identity(None)),
            clock.clone(),
            Carried::Plain,
        );

        let (status, served) = fetched_by(&client, vec![BUSY, BUSY, BUSY, BUSY]);
        assert_eq!((status, served), (503, 4));
        assert_eq!(
            clock.slept(),
            vec![
                Duration::from_secs(2),
                Duration::from_secs(4),
                Duration::from_secs(8)
            ]
        );

        let (status, served) = fetched_by(&client, vec![BUSY]);
        assert_eq!((status, served), (503, 1));
        let (status, served) = fetched_by(&client, vec![BUSY]);
        assert_eq!((status, served), (503, 1));
        assert_eq!(
            clock.slept()[3..],
            [MUSICBRAINZ_INTERVAL, MUSICBRAINZ_INTERVAL]
        );

        let (status, served) = fetched_by(&client, vec![FINE]);
        assert_eq!((status, served), (200, 1));
        let (status, served) = fetched_by(&client, vec![BUSY, FINE]);
        assert_eq!((status, served), (200, 2));
    }

    #[test]
    fn a_service_asking_for_fewer_requests_is_waited_on_like_a_busy_one() {
        let clock = Faked::new();
        let slow_down = Answer {
            status: 429,
            retry_after: Some(5),
        };
        let (status, served) = fetched(&clock, vec![slow_down, FINE]);

        assert_eq!(status, 200);
        assert_eq!(served, 2);
        assert_eq!(clock.slept(), vec![Duration::from_secs(5)]);
    }

    #[test]
    fn a_fourth_refusal_is_returned_as_it_is() {
        let clock = Faked::new();
        let (status, served) = fetched(&clock, vec![BUSY, BUSY, BUSY, BUSY]);

        assert_eq!(status, 503);
        assert_eq!(served, 4);
        assert_eq!(
            clock.slept(),
            vec![
                Duration::from_secs(2),
                Duration::from_secs(4),
                Duration::from_secs(8)
            ]
        );
    }

    #[test]
    fn a_wait_the_service_names_is_taken_over_the_doubling_default() {
        let clock = Faked::new();
        let (status, served) = fetched(&clock, vec![busy_for(3), BUSY, busy_for(30), FINE]);

        assert_eq!(status, 200);
        assert_eq!(served, 4);
        assert_eq!(
            clock.slept(),
            vec![
                Duration::from_secs(3),
                Duration::from_secs(4),
                RETRY_AFTER_AT_MOST
            ]
        );
    }

    #[test]
    fn a_retry_after_is_honoured_in_seconds_and_capped_at_ten() {
        let with = |value: Option<&str>| {
            let mut headers = HeaderMap::new();
            if let Some(value) = value {
                headers.insert(RETRY_AFTER, HeaderValue::from_str(value).expect("ascii"));
            }
            cooling_off(&headers, RETRY_AFTER_BY_DEFAULT)
        };

        assert_eq!(with(None), RETRY_AFTER_BY_DEFAULT);
        assert_eq!(with(Some("3")), Duration::from_secs(3));
        assert_eq!(with(Some(" 7 ")), Duration::from_secs(7));
        assert_eq!(with(Some("600")), RETRY_AFTER_AT_MOST);
        assert_eq!(
            with(Some("Wed, 21 Oct 2015 07:28:00 GMT")),
            RETRY_AFTER_BY_DEFAULT
        );
    }
}
