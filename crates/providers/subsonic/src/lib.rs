mod stall;
mod trust;

use std::{
    fmt::{self, Write as _},
    io::Read,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use md5::{Digest, Md5};
use resonate_core::{Isrc, Mbid, SourceId};
use resonate_providers::{
    Delivery, Error, Extension, Identity, Obtained, Pacing, Provider, ProviderOp, Result,
};
use serde::Deserialize;
use ureq::{
    Agent, Body,
    config::{Config, ConfigBuilder},
    http::{self, header::RETRY_AFTER},
    typestate::AgentScope,
    unversioned::{
        resolver::DefaultResolver,
        transport::{Connector as _, DefaultConnector},
    },
};

use crate::stall::BrokenOffAfter;

const SUBSONIC: &str = "subsonic";
const SPOKEN_AS: &str = "1.16.1";
const CALLED: &str = "resonate";
const SONGS_A_PAGE: usize = 40;
const PAGES_AT_MOST: usize = 5;
const ASKED_APART: Duration = Duration::from_millis(250);
const RETRIES_AT_MOST: u32 = 3;
const FIRST_RETRY_AFTER: Duration = Duration::from_secs(1);
const LONGEST_RETRY_AFTER: Duration = Duration::from_secs(8);
const TOO_MANY_REQUESTS: u16 = 429;
const UNAVAILABLE: u16 = 503;
const DOCUMENT_TYPES: [&str; 3] = ["text/", "json", "xml"];
const ANSWERED_WITHIN: Duration = Duration::from_secs(20);
const BROKEN_OFF_AFTER: Duration = Duration::from_secs(30);
const USER_AGENT: &str = concat!("resonate/", env!("CARGO_PKG_VERSION"));
const CONNECTED_WITHIN: Duration = Duration::from_secs(10);
const LARGEST_ANSWER: u64 = 4 * 1024 * 1024;
const OK: &str = "ok";
const REST: &str = "rest";
const ACCOUNT_REFUSALS: [u16; 9] = [20, 30, 40, 41, 42, 43, 44, 50, 60];

#[derive(Clone, PartialEq, Eq)]
pub struct Server {
    pub url: String,
    pub user: String,
    pub password: String,
}

struct Withheld;

impl fmt::Debug for Withheld {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<withheld>")
    }
}

impl fmt::Debug for Server {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Server")
            .field("url", &self.url)
            .field("user", &self.user)
            .field("password", &Withheld)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Patience {
    pub answered_within: Duration,
    pub broken_off_after: Duration,
}

impl Default for Patience {
    fn default() -> Self {
        Self {
            answered_within: ANSWERED_WITHIN,
            broken_off_after: BROKEN_OFF_AFTER,
        }
    }
}

fn configured(patience: Patience) -> ConfigBuilder<AgentScope> {
    Agent::config_builder()
        .user_agent(USER_AGENT)
        .tls_config(trust::system_and_built_in())
        .timeout_connect(Some(CONNECTED_WITHIN))
        .timeout_recv_response(Some(patience.answered_within))
        .http_status_as_error(false)
}

fn asking_agent(patience: Patience) -> Agent {
    configured(patience)
        .timeout_global(Some(patience.answered_within))
        .build()
        .new_agent()
}

fn downloading_agent(patience: Patience) -> Agent {
    let config: Config = configured(patience).build();
    Agent::with_parts(
        config,
        DefaultConnector::new().chain(BrokenOffAfter(patience.broken_off_after)),
        DefaultResolver::default(),
    )
}

pub struct Subsonic {
    source: SourceId,
    server: Server,
    asking: Agent,
    downloading: Agent,
    salted: AtomicU64,
    pacing: Arc<Pacing>,
}

#[derive(Deserialize)]
struct Answer {
    #[serde(rename = "subsonic-response")]
    response: Response,
}

#[derive(Deserialize)]
struct Response {
    status: String,
    #[serde(default)]
    error: Option<Refusal>,
    #[serde(default, rename = "searchResult3")]
    found: Option<Found>,
}

#[derive(Deserialize)]
struct Refusal {
    code: u16,
}

#[derive(Default, Deserialize)]
struct Found {
    #[serde(default)]
    song: Vec<Song>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
struct Song {
    id: String,
    #[serde(default)]
    suffix: Option<String>,
    #[serde(default, rename = "musicBrainzId")]
    recording: Option<String>,
    #[serde(default)]
    isrc: Isrcs,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
enum Isrcs {
    #[default]
    None,
    One(String),
    Many(Vec<String>),
}

impl Isrcs {
    fn holds(&self, isrc: &Isrc) -> bool {
        let named = |held: &String| Isrc::new(held.trim()).is_ok_and(|held| held == *isrc);
        match self {
            Self::None => false,
            Self::One(held) => named(held),
            Self::Many(held) => held.iter().any(named),
        }
    }
}

impl Song {
    fn is_the_recording(&self, recording: &Mbid) -> bool {
        self.recording
            .as_deref()
            .is_some_and(|held| held.trim().eq_ignore_ascii_case(recording.as_str()))
    }
}

fn the_one_asked_for<'a>(identity: &Identity, songs: &'a [Song]) -> Option<&'a Song> {
    let by_recording = identity
        .recording
        .as_ref()
        .and_then(|recording| songs.iter().find(|song| song.is_the_recording(recording)));
    by_recording.or_else(|| {
        identity
            .isrc
            .as_ref()
            .and_then(|isrc| songs.iter().find(|song| song.isrc.holds(isrc)))
    })
}

fn wordings(identity: &Identity) -> Vec<String> {
    let title = identity.title.trim();
    if title.is_empty() {
        return Vec::new();
    }
    let artist = identity
        .artist
        .as_deref()
        .map(str::trim)
        .filter(|artist| !artist.is_empty());

    artist
        .map(|artist| format!("{title} {artist}"))
        .into_iter()
        .chain([title.to_owned()])
        .collect()
}

fn is_a_document(mime: Option<&str>) -> bool {
    mime.is_some_and(|mime| {
        let mime = mime.to_ascii_lowercase();
        DOCUMENT_TYPES.iter().any(|kind| mime.contains(kind))
    })
}

fn retry_after(response: &http::Response<Body>) -> Option<Duration> {
    response
        .headers()
        .get(RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
        .map(Duration::from_secs)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(
        String::with_capacity(bytes.len() * 2),
        |mut written, byte| {
            let _ = write!(written, "{byte:02x}");
            written
        },
    )
}

fn token(password: &str, salt: &str) -> String {
    let mut digest = Md5::new();
    digest.update(password.as_bytes());
    digest.update(salt.as_bytes());
    hex(&digest.finalize())
}

impl Subsonic {
    pub fn at(server: Server) -> Self {
        Self {
            source: SourceId::new(SUBSONIC).unwrap_or_else(|_| SourceId::local()),
            server,
            asking: asking_agent(Patience::default()),
            downloading: downloading_agent(Patience::default()),
            salted: AtomicU64::new(0),
            pacing: Arc::new(Pacing::new(ASKED_APART)),
        }
    }

    #[must_use]
    pub fn waiting(self, patience: Patience) -> Self {
        Self {
            asking: asking_agent(patience),
            downloading: downloading_agent(patience),
            ..self
        }
    }

    fn called(&self, agent: &Agent, op: ProviderOp, url: &str) -> Result<http::Response<Body>> {
        let mut retried = 0;
        loop {
            self.pacing.paced();
            let response = agent
                .get(url)
                .call()
                .map_err(|error| self.unreachable(op, error))?;
            if response.status().is_success() {
                return Ok(response);
            }
            let status = response.status().as_u16();
            if retried < RETRIES_AT_MOST && matches!(status, TOO_MANY_REQUESTS | UNAVAILABLE) {
                let wait = retry_after(&response)
                    .unwrap_or(FIRST_RETRY_AFTER * (1 << retried))
                    .min(LONGEST_RETRY_AFTER);
                tracing::debug!(
                    status,
                    ?wait,
                    ?op,
                    "the Subsonic server asked to be asked later"
                );
                self.pacing.cool_for(wait);
                retried += 1;
                continue;
            }
            return Err(Error::Refused {
                provider: self.source.clone(),
                op,
                status,
            });
        }
    }

    fn read_whole(&self, op: ProviderOp, response: http::Response<Body>) -> Result<Vec<u8>> {
        response
            .into_body()
            .with_config()
            .limit(LARGEST_ANSWER)
            .read_to_vec()
            .map_err(|error| self.unreachable(op, error))
    }

    fn salt(&self) -> String {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos() as u64);
        let nth = self.salted.fetch_add(1, Ordering::Relaxed);
        hex(&Md5::digest(format!("{now}:{nth}:{}", std::process::id())))[..12].to_owned()
    }

    fn url(&self, method: &str, asked: &[(&str, &str)]) -> String {
        let salt = self.salt();
        let mut url = format!(
            "{}/{REST}/{method}?u={}&t={}&s={salt}&v={SPOKEN_AS}&c={CALLED}&f=json",
            self.server.url.trim_end_matches('/'),
            escaped(&self.server.user),
            token(&self.server.password, &salt),
        );
        for (name, value) in asked {
            let _ = write!(url, "&{name}={}", escaped(value));
        }
        url
    }

    fn unreachable(&self, op: ProviderOp, error: ureq::Error) -> Error {
        tracing::debug!(%error, ?op, "the Subsonic server could not be reached");
        if trust::certificate_refused(&error) {
            return Error::Untrusted {
                provider: self.source.clone(),
                op,
            };
        }
        let source = match error {
            ureq::Error::Io(source) => source,
            ureq::Error::Timeout(_) => std::io::Error::from(std::io::ErrorKind::TimedOut),
            ureq::Error::HostNotFound => std::io::Error::from(std::io::ErrorKind::NotFound),
            _ => std::io::Error::from(std::io::ErrorKind::ConnectionRefused),
        };
        Error::Io {
            provider: self.source.clone(),
            op,
            source,
        }
    }

    fn unreadable(&self, op: ProviderOp) -> Error {
        Error::Unreadable {
            provider: self.source.clone(),
            op,
        }
    }

    fn searched(&self, words: &str, offset: usize) -> Result<Vec<Song>> {
        let op = ProviderOp::Search;
        let url = self.url(
            "search3",
            &[
                ("query", words),
                ("songCount", &SONGS_A_PAGE.to_string()),
                ("songOffset", &offset.to_string()),
                ("artistCount", "0"),
                ("albumCount", "0"),
            ],
        );
        let response = self.called(&self.asking, op, &url)?;
        let bytes = self.read_whole(op, response)?;
        self.read(&bytes, op)
    }

    fn found(&self, identity: &Identity) -> Result<Option<Song>> {
        for words in wordings(identity) {
            for page in 0..PAGES_AT_MOST {
                let songs = self.searched(&words, page * SONGS_A_PAGE)?;
                if let Some(song) = the_one_asked_for(identity, &songs) {
                    return Ok(Some(song.clone()));
                }
                if songs.len() < SONGS_A_PAGE {
                    break;
                }
            }
        }
        Ok(None)
    }

    fn read(&self, bytes: &[u8], op: ProviderOp) -> Result<Vec<Song>> {
        let answer: Answer = serde_json::from_slice(bytes).map_err(|error| {
            tracing::debug!(%error, "a Subsonic answer was not the document expected");
            self.unreadable(op)
        })?;
        if answer.response.status != OK {
            let code = answer.response.error.map_or(0, |refusal| refusal.code);
            let provider = self.source.clone();
            return Err(if ACCOUNT_REFUSALS.contains(&code) {
                Error::Unwelcome { provider, op, code }
            } else {
                Error::TurnedAway { provider, op, code }
            });
        }
        Ok(answer.response.found.unwrap_or_default().song)
    }

    fn refusal_in(&self, bytes: &[u8], op: ProviderOp) -> Error {
        match self.read(bytes, op) {
            Err(error) => error,
            Ok(_) => self.unreadable(op),
        }
    }

    fn downloaded(&self, song: &Song) -> Result<Box<dyn Read + Send>> {
        let op = ProviderOp::Download;
        let url = self.url("download", &[("id", &song.id)]);
        let response = self.called(&self.downloading, op, &url)?;
        if is_a_document(response.body().mime_type()) {
            let bytes = self.read_whole(op, response)?;
            return Err(self.refusal_in(&bytes, op));
        }
        Ok(Box::new(response.into_body().into_reader()))
    }
}

fn escaped(text: &str) -> String {
    text.bytes()
        .fold(String::with_capacity(text.len()), |mut written, byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                written.push(char::from(byte));
            } else {
                let _ = write!(written, "%{byte:02X}");
            }
            written
        })
}

impl Provider for Subsonic {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn obtain(&self, identity: &Identity) -> Result<Obtained> {
        if identity.recording.is_none() && identity.isrc.is_none() {
            return Ok(Obtained::Nothing);
        }
        let Some(song) = self.found(identity)? else {
            return Ok(Obtained::Nothing);
        };
        let Some(extension) = song
            .suffix
            .as_deref()
            .and_then(|suffix| Extension::new(suffix).ok())
        else {
            return Ok(Obtained::Nothing);
        };
        Ok(Obtained::Found(Delivery::Stream {
            key: song.id.clone().into_boxed_str(),
            extension,
            reader: self.downloaded(&song)?,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEARCHED: &str = include_str!("../tests/fixtures/search3.json");
    const TURNED_AWAY: &str = include_str!("../tests/fixtures/wrong_password.json");
    const ECHOES: &str = "83d91898-7763-47d7-b03b-b92132375c47";
    const ECHOES_ISRC: &str = "GBN9Y1100089";

    fn subsonic() -> Subsonic {
        Subsonic::at(Server {
            url: "http://music.local:4533/".to_owned(),
            user: "listener".to_owned(),
            password: "sesame".to_owned(),
        })
    }

    #[test]
    fn a_token_is_the_digest_of_the_password_and_the_salt() {
        assert_eq!(
            token("sesame", "c19b2d"),
            "26719a1196d2a940705a59634eb18eab"
        );
    }

    #[test]
    fn a_request_names_the_build_and_carries_a_token_rather_than_the_password() {
        let url = subsonic().url("search3", &[("query", "Echoes & more")]);

        assert!(url.starts_with("http://music.local:4533/rest/search3?u=listener&t="));
        assert!(url.contains("&c=resonate&f=json"));
        assert!(url.ends_with("&query=Echoes%20%26%20more"));
        assert!(!url.contains("sesame"));
    }

    #[test]
    fn a_server_never_prints_its_password() {
        let printed = format!("{:?}", subsonic().server);

        assert!(printed.contains("listener"));
        assert!(printed.contains("music.local"));
        assert!(!printed.contains("sesame"));
    }

    #[test]
    fn a_certificate_the_client_does_not_trust_is_told_apart_from_a_refused_connection() {
        let refused_certificate = ureq::Error::Rustls(rustls::Error::InvalidCertificate(
            rustls::CertificateError::UnknownIssuer,
        ));
        let refused_connection =
            ureq::Error::Io(std::io::Error::from(std::io::ErrorKind::ConnectionRefused));
        let subsonic = subsonic();

        let untrusted = subsonic.unreachable(ProviderOp::Search, refused_certificate);
        let unreached = subsonic.unreachable(ProviderOp::Search, refused_connection);

        assert!(matches!(
            untrusted,
            Error::Untrusted {
                op: ProviderOp::Search,
                ..
            }
        ));
        assert!(untrusted.is_the_provider_away());
        assert!(matches!(unreached, Error::Io { .. }));
    }

    #[test]
    fn two_requests_are_salted_apart() {
        let subsonic = subsonic();
        assert_ne!(subsonic.salt(), subsonic.salt());
    }

    #[test]
    fn a_song_is_taken_by_its_recording_and_then_by_its_isrc_and_never_by_its_title() {
        let songs = subsonic()
            .read(SEARCHED.as_bytes(), ProviderOp::Search)
            .expect("a search answer");
        assert_eq!(songs.len(), 3);

        let by_recording = Identity {
            recording: Some(Mbid::new(ECHOES).expect("an mbid")),
            ..Identity::named("Echoes")
        };
        assert_eq!(
            the_one_asked_for(&by_recording, &songs).map(|song| song.id.as_str()),
            Some("song-2")
        );

        let by_isrc = Identity {
            isrc: Some(Isrc::new(ECHOES_ISRC).expect("an isrc")),
            ..Identity::named("Echoes")
        };
        assert_eq!(
            the_one_asked_for(&by_isrc, &songs).map(|song| song.id.as_str()),
            Some("song-3")
        );

        assert_eq!(the_one_asked_for(&Identity::named("Echoes"), &songs), None);
    }

    #[test]
    fn a_server_that_turns_the_listener_away_says_why_by_its_code() {
        assert!(matches!(
            subsonic().read(TURNED_AWAY.as_bytes(), ProviderOp::Search),
            Err(Error::Unwelcome { code: 40, .. })
        ));
    }

    #[test]
    fn a_want_with_no_identifier_is_not_searched_for() {
        assert!(matches!(
            subsonic().obtain(&Identity::named("Echoes")),
            Ok(Obtained::Nothing)
        ));
    }
}
