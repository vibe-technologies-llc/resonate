use std::{
    fmt::{self, Write as _},
    io,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use md5::{Digest, Md5};
use resonate_core::{Mbid, SourceId};
pub use resonate_fetch::Patience;
use resonate_fetch::{
    Ranged, Trusted, asking_agent, configured, downloading_agent, escaped, is_a_document,
    retry_after_of, unreached,
};
use resonate_providers::{
    Choosing, Delivery, Error, Extension, Identity, Listed, Obtained, Opened, Opening, Pacing,
    Provider, ProviderOp, Result, is_a_page,
};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use ureq::{Agent, Body, http};

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

fn downloading(patience: Patience) -> Agent {
    downloading_agent(
        configured(patience.answered_within, Trusted::SystemStoreToo).build(),
        patience.broken_off_after,
    )
}

fn songs_readable<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Vec<Song>, D::Error> {
    let listed = Option::<Vec<Value>>::deserialize(deserializer)?.unwrap_or_default();
    Ok(listed
        .into_iter()
        .filter_map(|song| {
            serde_json::from_value(song)
                .inspect_err(|error| {
                    tracing::debug!(%error, "a Subsonic song that does not read is passed over");
                })
                .ok()
        })
        .collect())
}

fn id_spelt<'de, D: Deserializer<'de>>(deserializer: D) -> std::result::Result<String, D::Error> {
    match Value::deserialize(deserializer)? {
        Value::String(id) => Ok(id),
        Value::Number(id) => Ok(id.to_string()),
        _ => Err(serde::de::Error::custom(
            "a song id is a string or a number",
        )),
    }
}

fn isrcs_readable<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Vec<String>, D::Error> {
    Ok(match Option::<Value>::deserialize(deserializer)? {
        Some(Value::String(one)) => vec![one],
        Some(Value::Array(many)) => many
            .into_iter()
            .filter_map(|held| match held {
                Value::String(held) => Some(held),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    })
}

#[derive(Clone)]
pub struct Subsonic {
    source: SourceId,
    server: Server,
    asking: Agent,
    downloading: Agent,
    patience: Patience,
    salted: Arc<AtomicU64>,
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
    #[serde(default, deserialize_with = "songs_readable")]
    song: Vec<Song>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
struct Song {
    #[serde(deserialize_with = "id_spelt")]
    id: String,
    #[serde(default)]
    suffix: Option<String>,
    #[serde(default, rename = "musicBrainzId")]
    recording: Option<String>,
    #[serde(default, deserialize_with = "isrcs_readable")]
    isrc: Vec<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    artist: Option<String>,
    #[serde(default)]
    duration: Option<u64>,
}

impl Song {
    fn listed(&self) -> Listed<'_> {
        Listed {
            isrcs: self.isrc.iter().map(String::as_str).collect(),
            title: self.title.as_deref().unwrap_or_default(),
            version: None,
            artists: self.artist.as_deref().into_iter().collect(),
            length: self.duration.map(Duration::from_secs),
        }
    }

    fn is_the_recording(&self, recording: &Mbid) -> bool {
        self.recording
            .as_deref()
            .is_some_and(|held| held.trim().eq_ignore_ascii_case(recording.as_str()))
    }
}

fn weighed(identity: &Identity, songs: &[Song], choosing: &mut Choosing<Song>) -> Option<Song> {
    let by_recording = identity
        .recording
        .as_ref()
        .and_then(|recording| songs.iter().find(|song| song.is_the_recording(recording)));
    if let Some(song) = by_recording {
        return Some(song.clone());
    }
    for song in songs {
        choosing.weigh(identity, &song.listed(), song.clone());
    }
    None
}

fn told(error: &ureq::Error) -> Option<String> {
    match error {
        ureq::Error::BadUri(_) | ureq::Error::Http(_) => None,
        error => Some(error.to_string()),
    }
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
            asking: asking_agent(Patience::default(), Trusted::SystemStoreToo),
            downloading: downloading(Patience::default()),
            patience: Patience::default(),
            salted: Arc::new(AtomicU64::new(0)),
            pacing: Arc::new(Pacing::new(ASKED_APART)),
        }
    }

    #[must_use]
    pub fn waiting(self, patience: Patience) -> Self {
        Self {
            asking: asking_agent(patience, Trusted::SystemStoreToo),
            downloading: downloading(patience),
            patience,
            ..self
        }
    }

    fn called(&self, agent: &Agent, op: ProviderOp, url: &str) -> Result<http::Response<Body>> {
        self.called_within(agent, op, url, None)
    }

    fn called_within(
        &self,
        agent: &Agent,
        op: ProviderOp,
        url: &str,
        deadline: Option<Instant>,
    ) -> Result<http::Response<Body>> {
        let mut retried = 0;
        loop {
            self.pacing.paced();
            let mut request = agent.get(url);
            if let Some(deadline) = deadline {
                request = request
                    .config()
                    .timeout_global(Some(self.left_before(deadline, op)?))
                    .build();
            }
            let response = request
                .call()
                .map_err(|error| self.unreachable(op, error))?;
            if response.status().is_success() {
                return Ok(response);
            }
            let status = response.status().as_u16();
            if retried < RETRIES_AT_MOST && matches!(status, TOO_MANY_REQUESTS | UNAVAILABLE) {
                let wait = retry_after_of(&response)
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
        match told(&error) {
            Some(told) => {
                tracing::debug!(
                    error = told,
                    ?op,
                    "the Subsonic server could not be reached"
                );
            }
            None => {
                tracing::debug!(
                    ?op,
                    "the Subsonic server's address does not read as one; it may lack its scheme"
                );
            }
        }
        unreached(self.source.clone(), op, error)
    }

    fn unreadable(&self, op: ProviderOp) -> Error {
        Error::Unreadable {
            provider: self.source.clone(),
            op,
        }
    }

    fn left_before(&self, deadline: Instant, op: ProviderOp) -> Result<Duration> {
        deadline
            .checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())
            .ok_or_else(|| Error::Io {
                provider: self.source.clone(),
                op,
                source: io::Error::from(io::ErrorKind::TimedOut),
            })
    }

    fn searched(&self, words: &str, offset: usize, deadline: Instant) -> Result<Vec<Song>> {
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
        let response = self.called_within(&self.asking, op, &url, Some(deadline))?;
        let bytes = self.read_whole(op, response)?;
        self.read(&bytes, op)
    }

    fn found(&self, identity: &Identity) -> Result<Option<Song>> {
        let deadline = Instant::now() + self.patience.answered_within;
        let mut choosing = Choosing::default();
        let wordings = identity.wordings();
        for words in &wordings {
            for page in 0..PAGES_AT_MOST {
                let songs = self.searched(words, page * SONGS_A_PAGE, deadline)?;
                if let Some(song) = weighed(identity, &songs, &mut choosing) {
                    return Ok(Some(song));
                }
                if songs.len() < SONGS_A_PAGE {
                    break;
                }
            }
            if choosing.holds_a_coded_listing() {
                break;
            }
        }
        let seen = choosing.seen();
        let chosen = choosing.chosen().map(|(song, taken)| {
            tracing::debug!(song = song.id, ?taken, "the Subsonic server holds the song");
            song
        });
        if chosen.is_none() {
            tracing::debug!(
                title = identity.title,
                asked = wordings.len(),
                seen,
                "the Subsonic server holds nothing coded or named as the song"
            );
        }
        Ok(chosen)
    }

    fn read(&self, bytes: &[u8], op: ProviderOp) -> Result<Vec<Song>> {
        let answer: Answer = serde_json::from_slice(bytes).map_err(|error| {
            tracing::debug!(%error, "a Subsonic answer was not the document expected");
            if is_a_page(bytes) {
                Error::NotTheService {
                    provider: self.source.clone(),
                    op,
                }
            } else {
                self.unreadable(op)
            }
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

    fn downloaded(&self, url: &str) -> Result<Opened> {
        let op = ProviderOp::Download;
        let response = self.called(&self.downloading, op, url)?;
        if is_a_document(response.body().mime_type()) {
            let bytes = self.read_whole(op, response)?;
            return Err(self.refusal_in(&bytes, op));
        }
        Ok(Opened::Reading(Box::new(Ranged::continuing(
            self.downloading.clone(),
            url.to_owned(),
            response,
            self.patience.resumed_after,
        ))))
    }
}

impl Provider for Subsonic {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn find(&self, identity: &Identity) -> Result<Obtained> {
        if identity.recording.is_none() && !identity.may_be_listed() {
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
        let url = self.url("download", &[("id", &song.id)]);
        let downloading = self.clone();
        Ok(Obtained::Found(Delivery::Stream {
            key: song.id.into_boxed_str(),
            extension,
            opening: Opening::new(move || downloading.downloaded(&url)),
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
    fn an_address_without_a_scheme_is_never_told_to_the_log_with_its_token() {
        let subsonic = Subsonic::at(Server {
            url: "music.local:4533".to_owned(),
            user: "listener".to_owned(),
            password: "sesame".to_owned(),
        });
        let url = subsonic.url("ping", &[]);
        let refused = subsonic
            .asking
            .get(&url)
            .call()
            .expect_err("an address with no scheme is refused");

        assert!(super::told(&refused).is_none(), "{refused}");
        assert!(
            super::told(&ureq::Error::HostNotFound).is_some_and(|told| !told.contains("t=")),
            "an error naming no address is still told"
        );
    }

    #[test]
    fn two_requests_are_salted_apart() {
        let subsonic = subsonic();
        assert_ne!(subsonic.salt(), subsonic.salt());
    }

    #[test]
    fn a_song_is_taken_by_its_recording_then_by_any_of_its_codes_then_by_its_name_and_length() {
        let songs = subsonic()
            .read(SEARCHED.as_bytes(), ProviderOp::Search)
            .expect("a search answer");
        assert_eq!(songs.len(), 3);
        let id = |identity: &Identity| {
            let mut choosing = Choosing::default();
            weighed(identity, &songs, &mut choosing)
                .or_else(|| choosing.chosen().map(|(song, _)| song))
                .map(|song| song.id)
        };

        let by_recording = Identity {
            recording: Some(Mbid::new(ECHOES).expect("an mbid")),
            ..Identity::named("Echoes")
        };
        assert_eq!(id(&by_recording).as_deref(), Some("song-2"));

        let by_isrc = Identity {
            isrcs: vec![
                resonate_core::Isrc::new("GBN9Y1100000").expect("an isrc"),
                resonate_core::Isrc::new(ECHOES_ISRC).expect("an isrc"),
            ],
            ..Identity::named("Echoes")
        };
        assert_eq!(id(&by_isrc).as_deref(), Some("song-3"));

        let named = Identity {
            artist: Some("Pink Floyd".to_owned()),
            length: Some(Duration::from_secs(1412)),
            ..Identity::named("Echoes")
        };
        assert_eq!(id(&named).as_deref(), Some("song-2"));
        assert_eq!(id(&Identity::named("Echoes")), None);
    }

    #[test]
    fn a_server_that_turns_the_listener_away_says_why_by_its_code() {
        assert!(matches!(
            subsonic().read(TURNED_AWAY.as_bytes(), ProviderOp::Search),
            Err(Error::Unwelcome { code: 40, .. })
        ));
    }

    #[test]
    fn a_want_with_no_identifier_and_nothing_to_name_it_by_is_not_searched_for() {
        assert!(matches!(
            subsonic().find(&Identity::named("Echoes")),
            Ok(Obtained::Nothing)
        ));
    }
}
