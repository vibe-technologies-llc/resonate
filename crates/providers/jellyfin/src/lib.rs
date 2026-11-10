use std::{
    fmt::{self, Write as _},
    io,
    sync::Arc,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use resonate_core::{MediaLocation, SourceId};
pub use resonate_fetch::Patience;
use resonate_fetch::{
    Seeking, Trusted, asking_agent, configured, downloading_agent, escaped, is_a_document,
    retry_after_of, unreached,
};
pub use resonate_providers::{
    Album, AlbumSongs, Artist, ArtistAlbums, Found, Id, Named, PlaysInPlace, Song, Streamed,
};
use resonate_providers::{Error, Pacing, ProviderOp, Result, is_a_page, not_offered};
use serde::de::DeserializeOwned;
use ureq::{Agent, Body, http};
use uuid::Uuid;

pub use crate::quick_connect::{
    A_QUICK_CONNECT_LAPSES_AFTER, Approving, QUICK_CONNECT_ASKED_EVERY, QuickConnect, SignedIn,
};
use crate::served::{Authenticated, ItemDoc, ItemsDoc, found};

mod quick_connect;
mod served;

const JELLYFIN: &str = "jellyfin";
const CLIENT: &str = "Resonate";
const DEVICE: &str = "Resonate";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const EMBY_ROUTE: &str = "/emby";
const ITEM_FIELDS: &str = "ChildCount,ItemCounts";
const ARTISTS_A_PAGE: usize = 500;
const ARTIST_PAGES_AT_MOST: usize = 40;
const BROWSED_AT_MOST: usize = 20;
const ALBUMS_OF_AN_ARTIST_AT_MOST: usize = 1000;
const SONGS_OF_AN_ALBUM_AT_MOST: usize = 2000;
const ASKED_APART: Duration = Duration::from_millis(250);
const RETRIES_AT_MOST: u32 = 3;
const FIRST_RETRY_AFTER: Duration = Duration::from_secs(1);
const LONGEST_RETRY_AFTER: Duration = Duration::from_secs(8);
const TOO_MANY_REQUESTS: u16 = 429;
const UNAVAILABLE: u16 = 503;
const UNAUTHORIZED: u16 = 401;
const FORBIDDEN: u16 = 403;
const NOT_FOUND: u16 = 404;
const LARGEST_ANSWER: u64 = 8 * 1024 * 1024;

#[derive(Clone, PartialEq, Eq)]
pub struct Server {
    pub url: String,
    pub user: String,
    pub password: String,
    pub kept: Option<KeptSession>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct KeptSession {
    pub token: String,
    pub user: String,
    pub device: String,
}

impl fmt::Debug for KeptSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeptSession")
            .field("token", &Withheld)
            .field("user", &self.user)
            .field("device", &self.device)
            .finish()
    }
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
            .field("kept", &self.kept)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavour {
    Jellyfin,
    Emby,
}

struct Session {
    root: String,
    token: String,
    user: String,
    flavour: Flavour,
}

impl Session {
    fn url(&self, path: &str, asked: &[(&str, &str)]) -> String {
        let mut url = format!("{}{path}?api_key={}", self.root, escaped(&self.token));
        for (name, value) in asked {
            let _ = write!(url, "&{name}={}", escaped(value));
        }
        url
    }
}

enum Call<'a> {
    Get,
    Post,
    Delete,
    SigningIn(&'a str),
    Introducing,
}

fn downloading(patience: Patience) -> Agent {
    downloading_agent(
        configured(patience.answered_within, Trusted::SystemStoreToo).build(),
        patience.broken_off_after,
    )
}

fn told(error: &ureq::Error) -> Option<String> {
    match error {
        ureq::Error::BadUri(_) | ureq::Error::Http(_) => None,
        error => Some(error.to_string()),
    }
}

fn flavour_of(root: &str) -> Flavour {
    if root.to_ascii_lowercase().ends_with(EMBY_ROUTE) {
        Flavour::Emby
    } else {
        Flavour::Jellyfin
    }
}

#[derive(Clone)]
pub struct Jellyfin {
    source: SourceId,
    server: Server,
    asking: Agent,
    downloading: Agent,
    patience: Patience,
    device: Arc<str>,
    session: Arc<Mutex<Option<Arc<Session>>>>,
    pacing: Arc<Pacing>,
}

impl Jellyfin {
    pub fn at(server: Server) -> Self {
        let device = server.kept.as_ref().map_or_else(
            || Arc::from(Uuid::new_v4().simple().to_string()),
            |kept| Arc::from(kept.device.as_str()),
        );
        Self {
            source: SourceId::new(JELLYFIN).unwrap_or_else(|_| SourceId::local()),
            server,
            asking: asking_agent(Patience::default(), Trusted::SystemStoreToo),
            downloading: downloading(Patience::default()),
            patience: Patience::default(),
            device,
            session: Arc::new(Mutex::new(None)),
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

    fn issued_to(mut self, device: &str) -> Self {
        self.device = Arc::from(device);
        self
    }

    pub const fn server(&self) -> &Server {
        &self.server
    }

    pub fn source_id(&self) -> &SourceId {
        &self.source
    }

    pub fn flavour(&self) -> Option<Flavour> {
        self.session.lock().as_ref().map(|session| session.flavour)
    }

    fn authorization(&self) -> String {
        format!(
            r#"MediaBrowser Client="{CLIENT}", Device="{DEVICE}", DeviceId="{}", Version="{VERSION}""#,
            self.device
        )
    }

    fn roots(&self) -> (String, Option<String>) {
        let base = self.server.url.trim().trim_end_matches('/').to_owned();
        if flavour_of(&base) == Flavour::Emby {
            (base, None)
        } else {
            let emby = format!("{base}{EMBY_ROUTE}");
            (base, Some(emby))
        }
    }

    fn unreachable(&self, op: ProviderOp, error: ureq::Error) -> Error {
        match told(&error) {
            Some(told) => {
                tracing::debug!(
                    error = told,
                    ?op,
                    "the Jellyfin server could not be reached"
                );
            }
            None => {
                tracing::debug!(
                    ?op,
                    "the Jellyfin server's address does not read as one; it may lack its scheme"
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

    fn sent(
        &self,
        agent: &Agent,
        call: &Call<'_>,
        url: &str,
        timeout: Option<Duration>,
    ) -> std::result::Result<http::Response<Body>, ureq::Error> {
        match call {
            Call::Get | Call::Delete => {
                let request = if matches!(call, Call::Get) {
                    agent.get(url)
                } else {
                    agent.delete(url)
                };
                match timeout {
                    Some(timeout) => request
                        .config()
                        .timeout_global(Some(timeout))
                        .build()
                        .call(),
                    None => request.call(),
                }
            }
            Call::Post => {
                let request = agent.post(url);
                match timeout {
                    Some(timeout) => request
                        .config()
                        .timeout_global(Some(timeout))
                        .build()
                        .send_empty(),
                    None => request.send_empty(),
                }
            }
            Call::Introducing => {
                let authorization = self.authorization();
                let request = agent
                    .get(url)
                    .header("Authorization", &authorization)
                    .header("X-Emby-Authorization", &authorization);
                match timeout {
                    Some(timeout) => request
                        .config()
                        .timeout_global(Some(timeout))
                        .build()
                        .call(),
                    None => request.call(),
                }
            }
            Call::SigningIn(body) => {
                let authorization = self.authorization();
                let request = agent
                    .post(url)
                    .header("Authorization", &authorization)
                    .header("X-Emby-Authorization", &authorization)
                    .header("Content-Type", "application/json");
                match timeout {
                    Some(timeout) => request
                        .config()
                        .timeout_global(Some(timeout))
                        .build()
                        .send(*body),
                    None => request.send(*body),
                }
            }
        }
    }

    fn called_within(
        &self,
        agent: &Agent,
        op: ProviderOp,
        call: &Call<'_>,
        url: &str,
        deadline: Option<Instant>,
    ) -> Result<http::Response<Body>> {
        let mut retried = 0;
        loop {
            self.pacing.paced();
            let timeout = deadline
                .map(|deadline| self.left_before(deadline, op))
                .transpose()?;
            let response = self
                .sent(agent, call, url, timeout)
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
                    "the Jellyfin server asked to be asked later"
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

    fn document<T: DeserializeOwned>(&self, bytes: &[u8], op: ProviderOp) -> Result<T> {
        serde_json::from_slice(bytes).map_err(|error| {
            tracing::debug!(%error, "a Jellyfin answer was not the document expected");
            if is_a_page(bytes) {
                Error::NotTheService {
                    provider: self.source.clone(),
                    op,
                }
            } else {
                self.unreadable(op)
            }
        })
    }

    fn authenticated(&self, root: &str, deadline: Option<Instant>) -> Result<Session> {
        let op = ProviderOp::SignIn;
        let body = serde_json::json!({
            "Username": self.server.user,
            "Pw": self.server.password,
        })
        .to_string();
        let url = format!("{root}/Users/AuthenticateByName");
        let response = self
            .called_within(&self.asking, op, &Call::SigningIn(&body), &url, deadline)
            .map_err(|error| match error {
                Error::Refused { status, .. } if matches!(status, UNAUTHORIZED | FORBIDDEN) => {
                    Error::Unwelcome {
                        provider: self.source.clone(),
                        op,
                        code: status,
                    }
                }
                error => error,
            })?;
        let bytes = self.read_whole(op, response)?;
        let signed_in: Authenticated = self.document(&bytes, op)?;
        Ok(Session {
            flavour: flavour_of(root),
            root: root.to_owned(),
            token: signed_in.token,
            user: signed_in.user.id,
        })
    }

    fn sign_in(&self, deadline: Option<Instant>) -> Result<Session> {
        let (plain, emby) = self.roots();
        if let Some(kept) = &self.server.kept {
            return Ok(Session {
                flavour: flavour_of(&plain),
                root: plain,
                token: kept.token.clone(),
                user: kept.user.clone(),
            });
        }
        match (self.authenticated(&plain, deadline), emby) {
            (
                Err(Error::Refused {
                    status: NOT_FOUND, ..
                }),
                Some(emby),
            ) => self.authenticated(&emby, deadline),
            (signed, _) => signed,
        }
    }

    fn session(&self, deadline: Option<Instant>) -> Result<Arc<Session>> {
        let mut held = self.session.lock();
        if let Some(session) = held.as_ref() {
            return Ok(Arc::clone(session));
        }
        let session = Arc::new(self.sign_in(deadline)?);
        *held = Some(Arc::clone(&session));
        Ok(session)
    }

    fn forget(&self, session: &Arc<Session>) {
        let mut held = self.session.lock();
        if held
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, session))
        {
            *held = None;
        }
    }

    fn asked(
        &self,
        agent: &Agent,
        op: ProviderOp,
        call: &Call<'_>,
        deadline: Option<Instant>,
        url: impl Fn(&Session) -> String,
    ) -> Result<http::Response<Body>> {
        let mut renewed = false;
        loop {
            let session = self.session(deadline)?;
            match self.called_within(agent, op, call, &url(&session), deadline) {
                Err(Error::Refused {
                    status: UNAUTHORIZED,
                    ..
                }) if self.server.kept.is_some() => {
                    self.forget(&session);
                    return Err(Error::Unwelcome {
                        provider: self.source.clone(),
                        op: ProviderOp::SignIn,
                        code: UNAUTHORIZED,
                    });
                }
                Err(Error::Refused {
                    status: UNAUTHORIZED,
                    ..
                }) if !renewed => {
                    tracing::debug!(?op, "the Jellyfin token was turned down; signing in again");
                    self.forget(&session);
                    renewed = true;
                }
                answered => return answered,
            }
        }
    }

    fn item_id<'a>(&self, id: &'a Id, op: ProviderOp) -> Result<&'a str> {
        let id = id.as_str();
        id.bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            .then_some(id)
            .ok_or_else(|| self.unreadable(op))
    }

    fn browsed<T: DeserializeOwned>(&self, url: impl Fn(&Session) -> String) -> Result<T> {
        let op = ProviderOp::Browse;
        let deadline = Instant::now() + self.patience.answered_within;
        let response = self.asked(&self.asking, op, &Call::Get, Some(deadline), url)?;
        let bytes = self.read_whole(op, response)?;
        self.document(&bytes, op)
    }

    fn item(&self, id: &Id) -> Result<ItemDoc> {
        let id = self.item_id(id, ProviderOp::Browse)?;
        self.browsed(|session| session.url(&format!("/Users/{}/Items/{id}", session.user), &[]))
    }

    fn children(&self, parameters: Vec<(&'static str, String)>, limit: usize) -> Result<ItemsDoc> {
        let mut asked = parameters;
        asked.push(("Recursive", "true".to_owned()));
        asked.push(("Fields", ITEM_FIELDS.to_owned()));
        asked.push(("Limit", limit.to_string()));
        let op = ProviderOp::Browse;
        let deadline = Instant::now() + self.patience.answered_within;
        let response = self.asked(&self.asking, op, &Call::Get, Some(deadline), |session| {
            let asked: Vec<(&str, &str)> = asked
                .iter()
                .map(|(name, value)| (*name, value.as_str()))
                .collect();
            session.url(&format!("/Users/{}/Items", session.user), &asked)
        })?;
        let bytes = self.read_whole(op, response)?;
        self.document(&bytes, op)
    }

    pub fn artists(&self) -> Result<Vec<Artist>> {
        let mut artists = Vec::new();
        for page in 0..ARTIST_PAGES_AT_MOST {
            let start = page * ARTISTS_A_PAGE;
            let listed: ItemsDoc = self.browsed(|session| {
                session.url(
                    "/Artists/AlbumArtists",
                    &[
                        ("UserId", &session.user),
                        ("SortBy", "SortName"),
                        ("SortOrder", "Ascending"),
                        ("Fields", ITEM_FIELDS),
                        ("StartIndex", &start.to_string()),
                        ("Limit", &ARTISTS_A_PAGE.to_string()),
                    ],
                )
            })?;
            let listed_here = listed.listed();
            let finished = match listed.total() {
                Some(total) => start + ARTISTS_A_PAGE >= total,
                None => listed_here < ARTISTS_A_PAGE,
            };
            artists.extend(listed.artists());
            if listed_here == 0 || finished {
                break;
            }
        }
        Ok(artists)
    }

    pub fn artist(&self, id: &Id) -> Result<ArtistAlbums> {
        let artist = self.item(id)?;
        let albums = self
            .children(
                vec![
                    ("IncludeItemTypes", "MusicAlbum".to_owned()),
                    (
                        "ArtistIds",
                        self.item_id(id, ProviderOp::Browse)?.to_owned(),
                    ),
                    ("SortBy", "ProductionYear,SortName".to_owned()),
                ],
                ALBUMS_OF_AN_ARTIST_AT_MOST,
            )?
            .albums();
        artist
            .with_albums(albums)
            .ok_or_else(|| self.unreadable(ProviderOp::Browse))
    }

    pub fn album(&self, id: &Id) -> Result<AlbumSongs> {
        let album = self.item(id)?;
        let songs = self
            .children(
                vec![
                    ("IncludeItemTypes", "Audio".to_owned()),
                    ("ParentId", self.item_id(id, ProviderOp::Browse)?.to_owned()),
                    (
                        "SortBy",
                        "ParentIndexNumber,IndexNumber,SortName".to_owned(),
                    ),
                ],
                SONGS_OF_AN_ALBUM_AT_MOST,
            )?
            .songs();
        album
            .with_songs(songs)
            .ok_or_else(|| self.unreadable(ProviderOp::Browse))
    }

    pub fn song(&self, id: &Id) -> Result<Song> {
        self.item(id)?
            .song()
            .ok_or_else(|| self.unreadable(ProviderOp::Browse))
    }

    pub fn search(&self, words: &str) -> Result<Found> {
        let at_most = BROWSED_AT_MOST.to_string();
        let artists: ItemsDoc = self.browsed(|session| {
            session.url(
                "/Artists",
                &[
                    ("UserId", &session.user),
                    ("SearchTerm", words),
                    ("Fields", ITEM_FIELDS),
                    ("Limit", &at_most),
                ],
            )
        })?;
        let kind = |kind: &str| {
            self.children(
                vec![
                    ("IncludeItemTypes", kind.to_owned()),
                    ("SearchTerm", words.to_owned()),
                ],
                BROWSED_AT_MOST,
            )
        };
        Ok(found(artists, kind("MusicAlbum")?, kind("Audio")?))
    }

    pub fn favour(&self, id: &Id, favoured: bool) -> Result<()> {
        let op = ProviderOp::Favourite;
        let id = self.item_id(id, op)?;
        let call = if favoured { Call::Post } else { Call::Delete };
        let deadline = Instant::now() + self.patience.answered_within;
        let response = self.asked(&self.asking, op, &call, Some(deadline), |session| {
            session.url(&format!("/Users/{}/FavoriteItems/{id}", session.user), &[])
        })?;
        self.read_whole(op, response).map(drop)
    }

    pub fn cover_of(&self, named: &Named) -> Result<Id> {
        match named {
            Named::Song(id) => {
                let song = self.song(id)?;
                Ok(song.cover.unwrap_or(song.id))
            }
            Named::Album(id) | Named::Artist(id) => Ok(id.clone()),
            Named::Mix(_) => Err(not_offered(&self.source, ProviderOp::Cover)),
        }
    }

    pub fn cover(&self, cover: &Id, side: u32) -> Result<Vec<u8>> {
        let op = ProviderOp::Cover;
        let id = self.item_id(cover, op)?;
        let side = side.to_string();
        let deadline = Instant::now() + self.patience.answered_within;
        let response = self.asked(&self.asking, op, &Call::Get, Some(deadline), |session| {
            session.url(
                &format!("/Items/{id}/Images/Primary"),
                &[("maxWidth", &side), ("maxHeight", &side)],
            )
        })?;
        let media_type = response.body().mime_type().map(str::to_owned);
        let bytes = self.read_whole(op, response)?;
        if is_a_document(media_type.as_deref()) {
            return Err(self.unreadable(op));
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
            Named::Mix(_) => Err(not_offered(&self.source, ProviderOp::Browse)),
        }
    }

    pub fn location_of(&self, named: &Named) -> MediaLocation {
        named.location(&self.source)
    }

    pub fn named_by(&self, location: &MediaLocation) -> Option<Named> {
        Named::read(location, &self.source).filter(|named| !matches!(named, Named::Mix(_)))
    }

    pub fn streamed(&self, id: &Id) -> Result<Streamed> {
        let op = ProviderOp::Stream;
        let id = self.item_id(id, op)?;
        let url_of =
            |session: &Session| session.url(&format!("/Audio/{id}/stream"), &[("static", "true")]);
        let response = self.asked(&self.downloading, op, &Call::Get, None, url_of)?;
        let media_type = response.body().mime_type().map(str::to_owned);
        if is_a_document(media_type.as_deref()) {
            return Err(self.unreadable(op));
        }
        let session = self.session(None)?;
        let reader = Seeking::continuing(
            self.downloading.clone(),
            url_of(&session),
            response,
            self.patience.resumed_after,
        );
        let length = reader.length();
        Ok(Streamed::seeking(reader, length, media_type))
    }
}

impl PlaysInPlace for Jellyfin {
    fn scheme(&self) -> &SourceId {
        &self.source
    }

    fn title(&self) -> &'static str {
        "Jellyfin"
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
        Self::search(self, words).map(Found::explicit_first)
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

    fn keeps_favourites(&self) -> bool {
        true
    }

    fn favour(&self, song: &Id, favoured: bool) -> Result<()> {
        Self::favour(self, song, favoured)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jellyfin() -> Jellyfin {
        Jellyfin::at(Server {
            url: "http://music.local:8096/".to_owned(),
            user: "listener".to_owned(),
            password: "sesame".to_owned(),
            kept: None,
        })
    }

    fn session(root: &str) -> Session {
        Session {
            root: root.to_owned(),
            token: "tok/en".to_owned(),
            user: "u1".to_owned(),
            flavour: flavour_of(root),
        }
    }

    #[test]
    fn a_request_carries_the_token_escaped_and_never_the_password() {
        let url =
            session("http://music.local:8096").url("/Items", &[("SearchTerm", "Echoes & more")]);

        assert_eq!(
            url,
            "http://music.local:8096/Items?api_key=tok%2Fen&SearchTerm=Echoes%20%26%20more"
        );
        assert!(!url.contains("sesame"));
    }

    #[test]
    fn a_server_never_prints_its_password() {
        let printed = format!("{:?}", jellyfin().server);

        assert!(printed.contains("listener"));
        assert!(printed.contains("music.local"));
        assert!(!printed.contains("sesame"));
    }

    #[test]
    fn the_authorization_names_the_build_and_a_generated_device_and_nothing_of_the_listener() {
        let one = jellyfin();
        let another = jellyfin();
        let header = one.authorization();

        assert!(
            header.starts_with(r#"MediaBrowser Client="Resonate", Device="Resonate", DeviceId=""#)
        );
        assert!(header.ends_with(&format!(r#"Version="{VERSION}""#)));
        assert!(!header.contains("listener"));
        assert!(!header.contains("sesame"));
        assert_ne!(header, another.authorization());
        assert_eq!(header, one.clone().authorization());
    }

    #[test]
    fn a_kept_session_names_the_device_it_was_issued_to_and_withholds_its_token() {
        let server = Server {
            url: "http://music.local:8096".to_owned(),
            user: "listener".to_owned(),
            password: String::new(),
            kept: Some(KeptSession {
                token: "very-secret-token".to_owned(),
                user: "0a1b".to_owned(),
                device: "kept-device".to_owned(),
            }),
        };

        let shown = format!("{server:?}");
        let header = Jellyfin::at(server).authorization();

        assert!(header.contains(r#"DeviceId="kept-device""#), "{header}");
        assert!(!shown.contains("very-secret-token"), "{shown}");
    }

    #[test]
    fn an_address_is_tried_plain_and_then_under_the_emby_route_unless_it_names_it() {
        let plain = jellyfin().roots();
        let named = Jellyfin::at(Server {
            url: "https://music.local/EMBY/".to_owned(),
            user: String::new(),
            password: String::new(),
            kept: None,
        })
        .roots();

        assert_eq!(
            plain,
            (
                "http://music.local:8096".to_owned(),
                Some("http://music.local:8096/emby".to_owned())
            )
        );
        assert_eq!(named, ("https://music.local/EMBY".to_owned(), None));
        assert_eq!(flavour_of("https://music.local/EMBY"), Flavour::Emby);
        assert_eq!(flavour_of("https://music.local"), Flavour::Jellyfin);
    }

    #[test]
    fn an_address_without_a_scheme_is_never_told_to_the_log_with_its_token() {
        let jellyfin = Jellyfin::at(Server {
            url: "music.local:8096".to_owned(),
            user: "listener".to_owned(),
            password: "sesame".to_owned(),
            kept: None,
        });
        let url = session(&jellyfin.server.url).url("/Items", &[]);
        let refused = jellyfin
            .asking
            .get(&url)
            .call()
            .expect_err("an address with no scheme is refused");

        assert!(told(&refused).is_none(), "{refused}");
        assert!(
            told(&ureq::Error::HostNotFound).is_some_and(|told| !told.contains("api_key")),
            "an error naming no address is still told"
        );
    }

    #[test]
    fn an_id_that_could_leave_its_path_is_never_asked() {
        let jellyfin = jellyfin();

        for odd in ["../Users", "a/b", "a?b", "a b", "%2e"] {
            let id = Id::new(odd).expect("an id");
            assert!(matches!(
                jellyfin.item_id(&id, ProviderOp::Browse),
                Err(Error::Unreadable { .. })
            ));
        }
        let id = Id::new("3f2a-9c_1").expect("an id");
        assert_eq!(
            jellyfin.item_id(&id, ProviderOp::Browse).ok(),
            Some("3f2a-9c_1")
        );
    }
}
