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
use resonate_core::{Mbid, MediaLocation, SourceId};
pub use resonate_fetch::Patience;
use resonate_fetch::{
    Ranged, Seeking, Trusted, asking_agent, asking_to_resume, configured, downloading_agent,
    escaped, is_a_document, retry_after_of, unreached,
};
pub use resonate_providers::{
    Album, AlbumSongs, Artist, ArtistAlbums, Found, Id, Named, PlaysInPlace, Song, Streamed,
};
use resonate_providers::{
    Choosing, Delivery, Error, Extension, Held, Identity, Listed, Obtained, Opening, Pacing,
    Provider, ProviderOp, Reopened, Result, is_a_page,
};
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use serde_json::Value;
use ureq::{Agent, Body, http};

use crate::served::{AlbumDoc, ArtistDoc, ArtistsDoc, FoundDoc, SongDoc, id_spelt, readable};

mod served;

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
const NOT_AS_HELD: [u16; 2] = [412, 416];
const LARGEST_ANSWER: u64 = 4 * 1024 * 1024;
const OK: &str = "ok";
const AS_STORED: &str = "raw";
const BROWSED_AT_MOST: usize = 20;
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
    found: Option<Value>,
    #[serde(default)]
    artists: Option<Value>,
    #[serde(default)]
    artist: Option<Value>,
    #[serde(default)]
    album: Option<Value>,
    #[serde(default)]
    song: Option<Value>,
}

#[derive(Deserialize)]
struct Refusal {
    code: u16,
}

#[derive(Default, Deserialize)]
struct Listings {
    #[serde(default, deserialize_with = "readable")]
    song: Vec<Listing>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
struct Listing {
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

impl Listing {
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

fn weighed(
    identity: &Identity,
    songs: &[Listing],
    choosing: &mut Choosing<Listing>,
) -> Option<Listing> {
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

    fn called_within(
        &self,
        agent: &Agent,
        op: ProviderOp,
        url: &str,
        deadline: Option<Instant>,
        held: Option<&Held>,
    ) -> Result<http::Response<Body>> {
        let mut retried = 0;
        loop {
            self.pacing.paced();
            let mut request = asking_to_resume(agent.get(url), held);
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

    pub const fn server(&self) -> &Server {
        &self.server
    }

    pub fn source_id(&self) -> &SourceId {
        &self.source
    }

    fn browsed(&self, method: &str, asked: &[(&str, &str)]) -> Result<Response> {
        let op = ProviderOp::Browse;
        let deadline = Instant::now() + self.patience.answered_within;
        let url = self.url(method, asked);
        let response = self.called_within(&self.asking, op, &url, Some(deadline), None)?;
        let bytes = self.read_whole(op, response)?;
        self.answered(&bytes, op)
    }

    pub fn artists(&self) -> Result<Vec<Artist>> {
        let answered = self.browsed("getArtists", &[])?;
        let artists: Option<ArtistsDoc> = self.part(answered.artists, ProviderOp::Browse)?;
        Ok(artists.unwrap_or_default().artists())
    }

    pub fn artist(&self, id: &Id) -> Result<ArtistAlbums> {
        let answered = self.browsed("getArtist", &[("id", id.as_str())])?;
        let artist: Option<ArtistDoc> = self.part(answered.artist, ProviderOp::Browse)?;
        artist
            .and_then(ArtistDoc::with_albums)
            .ok_or_else(|| self.unreadable(ProviderOp::Browse))
    }

    pub fn album(&self, id: &Id) -> Result<AlbumSongs> {
        let answered = self.browsed("getAlbum", &[("id", id.as_str())])?;
        let album: Option<AlbumDoc> = self.part(answered.album, ProviderOp::Browse)?;
        album
            .and_then(AlbumDoc::with_songs)
            .ok_or_else(|| self.unreadable(ProviderOp::Browse))
    }

    pub fn song(&self, id: &Id) -> Result<Song> {
        let answered = self.browsed("getSong", &[("id", id.as_str())])?;
        let song: Option<SongDoc> = self.part(answered.song, ProviderOp::Browse)?;
        song.and_then(SongDoc::song)
            .ok_or_else(|| self.unreadable(ProviderOp::Browse))
    }

    pub fn search(&self, words: &str) -> Result<Found> {
        let at_most = BROWSED_AT_MOST.to_string();
        let answered = self.browsed(
            "search3",
            &[
                ("query", words),
                ("artistCount", &at_most),
                ("albumCount", &at_most),
                ("songCount", &at_most),
            ],
        )?;
        let found: Option<FoundDoc> = self.part(answered.found, ProviderOp::Browse)?;
        Ok(found.unwrap_or_default().found())
    }

    pub fn cover_of(&self, named: &Named) -> Result<Id> {
        match named {
            Named::Song(id) => {
                let song = self.song(id)?;
                Ok(song.cover.unwrap_or(song.id))
            }
            Named::Album(id) | Named::Artist(id) => Ok(id.clone()),
        }
    }

    pub fn cover(&self, cover: &Id, side: u32) -> Result<Vec<u8>> {
        let op = ProviderOp::Cover;
        let deadline = Instant::now() + self.patience.answered_within;
        let url = self.url(
            "getCoverArt",
            &[("id", cover.as_str()), ("size", &side.to_string())],
        );
        let response = self.called_within(&self.asking, op, &url, Some(deadline), None)?;
        let media_type = response.body().mime_type().map(str::to_owned);
        let bytes = self.read_whole(op, response)?;
        if is_a_document(media_type.as_deref()) {
            return Err(self.refusal_in(&bytes, op));
        }
        Ok(bytes)
    }

    pub fn songs_of(&self, named: &Named) -> Result<Vec<Song>> {
        match named {
            Named::Song(id) => Ok(vec![self.song(id)?]),
            Named::Album(id) => Ok(self.album(id)?.songs),
            Named::Artist(id) => {
                let mut songs = Vec::new();
                for album in self.artist(id)?.albums {
                    songs.extend(self.album(&album.id)?.songs);
                }
                Ok(songs)
            }
        }
    }

    pub fn location_of(&self, named: &Named) -> MediaLocation {
        named.location(&self.source)
    }

    pub fn named_by(&self, location: &MediaLocation) -> Option<Named> {
        Named::read(location, &self.source)
    }

    pub fn streamed(&self, id: &Id) -> Result<Streamed> {
        let op = ProviderOp::Stream;
        let url = self.url("stream", &[("id", id.as_str()), ("format", AS_STORED)]);
        let response = self.called_within(&self.downloading, op, &url, None, None)?;
        let media_type = response.body().mime_type().map(str::to_owned);
        if is_a_document(media_type.as_deref()) {
            let bytes = self.read_whole(op, response)?;
            return Err(self.refusal_in(&bytes, op));
        }
        let reader = Seeking::continuing(
            self.downloading.clone(),
            url,
            response,
            self.patience.resumed_after,
        );
        let length = reader.length();
        Ok(Streamed::seeking(reader, length, media_type))
    }

    fn searched(&self, words: &str, offset: usize, deadline: Instant) -> Result<Vec<Listing>> {
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
        let response = self.called_within(&self.asking, op, &url, Some(deadline), None)?;
        let bytes = self.read_whole(op, response)?;
        self.read(&bytes, op)
    }

    fn found(&self, identity: &Identity) -> Result<Option<Listing>> {
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

    fn read(&self, bytes: &[u8], op: ProviderOp) -> Result<Vec<Listing>> {
        let found: Option<Listings> = self.part(self.answered(bytes, op)?.found, op)?;
        Ok(found.unwrap_or_default().song)
    }

    fn part<T: DeserializeOwned>(&self, part: Option<Value>, op: ProviderOp) -> Result<Option<T>> {
        part.map(|part| {
            serde_json::from_value(part).map_err(|error| {
                tracing::debug!(%error, "a part of a Subsonic answer was not the document expected");
                self.unreadable(op)
            })
        })
        .transpose()
    }

    fn answered(&self, bytes: &[u8], op: ProviderOp) -> Result<Response> {
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
        Ok(answer.response)
    }

    fn refusal_in(&self, bytes: &[u8], op: ProviderOp) -> Error {
        match self.answered(bytes, op) {
            Err(error) => error,
            Ok(_) => self.unreadable(op),
        }
    }

    fn downloaded(&self, url: &str, held: Option<Held>) -> Result<Reopened> {
        let op = ProviderOp::Download;
        let held = held.filter(Held::can_be_checked);
        let response = match self.called_within(&self.downloading, op, url, None, held.as_ref()) {
            Err(Error::Refused { status, .. })
                if held.is_some() && NOT_AS_HELD.contains(&status) =>
            {
                return self.downloaded(url, None);
            }
            called => called?,
        };
        if is_a_document(response.body().mime_type()) {
            let bytes = self.read_whole(op, response)?;
            return Err(self.refusal_in(&bytes, op));
        }
        let agent = self.downloading.clone();
        let resumed_after = self.patience.resumed_after;
        let ranged = match &held {
            Some(held) => {
                match Ranged::continuing_held(agent, url.to_owned(), response, held, resumed_after)
                {
                    Some(ranged) => ranged,
                    None => return self.downloaded(url, None),
                }
            }
            None => Ranged::continuing(agent, url.to_owned(), response, resumed_after),
        };
        Ok(Reopened::Reading(ranged.into_resumed()))
    }
}

impl PlaysInPlace for Subsonic {
    fn scheme(&self) -> &SourceId {
        &self.source
    }

    fn title(&self) -> &'static str {
        "Subsonic"
    }

    fn artists(&self) -> Result<Vec<Artist>> {
        Self::artists(self)
    }

    fn artist(&self, id: &Id) -> Result<ArtistAlbums> {
        Self::artist(self, id)
    }

    fn album(&self, id: &Id) -> Result<AlbumSongs> {
        Self::album(self, id)
    }

    fn search(&self, words: &str) -> Result<Found> {
        Self::search(self, words)
    }

    fn streamed(&self, song: &Id) -> Result<Streamed> {
        Self::streamed(self, song)
    }

    fn cover(&self, cover: &Id, side: u32) -> Result<Vec<u8>> {
        Self::cover(self, cover, side)
    }

    fn cover_of(&self, named: &Named) -> Result<Id> {
        Self::cover_of(self, named)
    }

    fn songs_of(&self, named: &Named) -> Result<Vec<Song>> {
        Self::songs_of(self, named)
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
            opening: Opening::resumable(move |held| downloading.downloaded(&url, held)),
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
