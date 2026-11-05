use std::{
    fmt,
    io::{self, ErrorKind, Read as _, Write as _},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use parking_lot::Mutex;
use resonate_library::{
    Isrc, LinkNames, LinkedPlaylist, LinkedProfile, ListedSong, LookupOp, PlaylistLink,
    ProfilePlaylist,
};
use serde::{Deserialize, de::DeserializeOwned};
use sha2::{Digest as _, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    Client, Error, Host, Result,
    client::{LARGEST_CONTINUATION, Posted},
    query::Params,
    shared::stated,
};

const SCOPES: &str = "playlist-read-private playlist-read-collaborative user-library-read";
const REDIRECT_PATH: &str = "/spotify";
const REDIRECT_PORT: u16 = 43_879;
const A_TRACK: &str = "track";
const LIKED_SONGS: &str = "Liked Songs";
const ID_LENGTH: usize = 22;
const ASKED_A_PAGE: usize = 50;
const SONGS_READ_AT_MOST: usize = 50_000;
const PLAYLISTS_READ_AT_MOST: usize = 1_000;
const REFUSED_THE_TOKEN: u16 = 401;
const NOT_THE_LISTENERS: u16 = 403;
const RENEWED_BEFORE_IT_LAPSES: Duration = Duration::from_secs(60);
const A_SIGN_IN_LAPSES_AFTER: Duration = Duration::from_secs(10 * 60);
const LOOKED_AT_EVERY: Duration = Duration::from_millis(100);
const A_REQUEST_ARRIVES_WITHIN: Duration = Duration::from_secs(2);
const LONGEST_REQUEST_HEAD: usize = 8 * 1024;
const BACK_IN_THE_APP: &str = "<!doctype html><meta charset=\"utf-8\"><title>Resonate</title>\
                               <p>Signed in to Spotify. You can go back to Resonate.</p>";
const TOLD_NOT_TO: &str = "<!doctype html><meta charset=\"utf-8\"><title>Resonate</title>\
                           <p>Spotify was told not to sign in. You can go back to Resonate.</p>";
const NOT_THIS_SIGN_IN: &str = "<!doctype html><meta charset=\"utf-8\"><title>Resonate</title>\
                                <p>This page is not the sign-in Resonate is waiting for.</p>";

pub const SPOTIFY_REDIRECT: &str = "http://127.0.0.1:43879/spotify";

#[derive(Clone, PartialEq, Eq)]
pub struct SpotifyGrant {
    pub client_id: String,
    pub refresh_token: String,
}

impl fmt::Debug for SpotifyGrant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpotifyGrant")
            .field("client_id", &self.client_id)
            .finish_non_exhaustive()
    }
}

pub trait SpotifyGrants: Send + Sync {
    fn grant(&self) -> Option<SpotifyGrant>;

    fn renewed(&self, grant: &SpotifyGrant, token: &str);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpotifyEndpoints {
    pub accounts: String,
    pub api: String,
    pub pages: String,
}

impl SpotifyEndpoints {
    pub fn spotify() -> Self {
        Self {
            accounts: Host::SpotifyAccounts.base().to_owned(),
            api: Host::SpotifyApi.base().to_owned(),
            pages: Host::Spotify.base().to_owned(),
        }
    }
}

struct Access {
    token: String,
    until: Instant,
}

struct Session {
    given: SpotifyGrant,
    refresh_token: String,
    access: Option<Access>,
}

impl Session {
    fn holds(&self, grant: &SpotifyGrant) -> bool {
        self.given.client_id == grant.client_id
            && (self.given.refresh_token == grant.refresh_token
                || self.refresh_token == grant.refresh_token)
    }
}

pub struct SpotifyAccount {
    grants: Arc<dyn SpotifyGrants>,
    endpoints: SpotifyEndpoints,
    session: Mutex<Option<Session>>,
}

impl SpotifyAccount {
    pub fn new(grants: Arc<dyn SpotifyGrants>) -> Self {
        Self::at(grants, SpotifyEndpoints::spotify())
    }

    pub fn at(grants: Arc<dyn SpotifyGrants>, endpoints: SpotifyEndpoints) -> Self {
        Self {
            grants,
            endpoints,
            session: Mutex::new(None),
        }
    }

    pub(crate) fn pages(&self) -> &str {
        &self.endpoints.pages
    }

    pub(crate) fn account_named(&self, client: &Client) -> Result<Option<LinkedProfile>> {
        if self.grants.grant().is_none() {
            return Ok(None);
        }
        let me = format!("{}/me", self.endpoints.api);
        let Some(me) = self.asked::<Me>(client, &me)? else {
            return Ok(None);
        };
        let Some(name) = stated(me.display_name.as_deref()).or(stated(me.id.as_deref())) else {
            return Ok(None);
        };
        let mut playlists = vec![ProfilePlaylist {
            title: LIKED_SONGS.to_owned(),
            link: PlaylistLink::SpotifyLiked,
        }];
        let listed = format!("{}/me/playlists", self.endpoints.api);
        let mut read = 0;
        loop {
            let page = self
                .asked::<Page<PlaylistListed>>(client, &paged(&listed, read))?
                .ok_or(Error::Unreadable {
                    host: Host::SpotifyApi,
                    op: LookupOp::FollowLink,
                })?;
            read += page.items.len();
            let last = page.next.is_none() || page.items.is_empty();
            playlists.extend(page.items.into_iter().flatten().filter_map(|listed| {
                let id = listed.id.filter(|id| names_a_playlist(id))?;
                Some(ProfilePlaylist {
                    title: stated(listed.name.as_deref())?,
                    link: PlaylistLink::Spotify(id),
                })
            }));
            if last || read >= PLAYLISTS_READ_AT_MOST {
                break;
            }
        }

        Ok(Some(LinkedProfile { name, playlists }))
    }

    pub(crate) fn liked(&self, client: &Client) -> Result<Option<LinkedPlaylist>> {
        if self.grants.grant().is_none() {
            return Ok(None);
        }
        let saved = format!("{}/me/tracks", self.endpoints.api);
        let Some(songs) = self.songs::<Saved>(client, &saved)? else {
            return Ok(None);
        };

        Ok(Some(LinkedPlaylist {
            name: LIKED_SONGS.to_owned(),
            songs,
        }))
    }

    pub(crate) fn playlist(
        &self,
        client: &Client,
        playlist: &str,
    ) -> Result<Option<LinkedPlaylist>> {
        if self.grants.grant().is_none() || !names_a_playlist(playlist) {
            return Ok(None);
        }
        let items = format!("{}/playlists/{playlist}/items", self.endpoints.api);
        let songs = match self.songs::<Item>(client, &items) {
            Ok(songs) => songs,
            Err(Error::Refused {
                status: NOT_THE_LISTENERS,
                ..
            }) => {
                tracing::debug!(
                    playlist,
                    "the Web API lets this app read only the playlists the account owns or \
                     collaborates on; the public page is read"
                );
                None
            }
            Err(error) => return Err(error),
        };
        let Some(songs) = songs else {
            return Ok(None);
        };
        let named = format!("{}/playlists/{playlist}?fields=name", self.endpoints.api);
        let name = self
            .asked::<Named>(client, &named)?
            .and_then(|named| stated(named.name.as_deref()))
            .ok_or(Error::Unreadable {
                host: Host::SpotifyApi,
                op: LookupOp::FollowLink,
            })?;

        Ok(Some(LinkedPlaylist { name, songs }))
    }

    fn songs<T: Listing>(&self, client: &Client, listed: &str) -> Result<Option<Vec<ListedSong>>> {
        let mut songs = Vec::new();
        let mut read = 0;
        loop {
            let page = self.asked::<Page<T>>(client, &paged(listed, read))?;
            let Some(page) = page else {
                if read == 0 {
                    return Ok(None);
                }
                return Err(Error::Unreadable {
                    host: Host::SpotifyApi,
                    op: LookupOp::FollowLink,
                });
            };
            read += page.items.len();
            let last = page.next.is_none() || page.items.is_empty();
            songs.extend(
                page.items
                    .into_iter()
                    .flatten()
                    .filter_map(Listing::song)
                    .filter_map(Track::named)
                    .map(ListedSong::Named),
            );
            if last {
                return Ok(Some(songs));
            }
            if read >= SONGS_READ_AT_MOST {
                tracing::warn!(read, "a Spotify list holds more songs than are read of one");
                return Ok(Some(songs));
            }
        }
    }

    fn asked<T: DeserializeOwned>(&self, client: &Client, url: &str) -> Result<Option<T>> {
        let Some(token) = self.access(client)? else {
            return Ok(None);
        };
        match client.json_as(
            Host::SpotifyApi,
            LookupOp::FollowLink,
            url,
            &format!("Bearer {token}"),
            LARGEST_CONTINUATION,
        ) {
            Err(Error::Refused {
                status: REFUSED_THE_TOKEN,
                ..
            }) => {
                self.lapse(&token);
                let Some(token) = self.access(client)? else {
                    return Ok(None);
                };
                client.json_as(
                    Host::SpotifyApi,
                    LookupOp::FollowLink,
                    url,
                    &format!("Bearer {token}"),
                    LARGEST_CONTINUATION,
                )
            }
            answered => answered,
        }
    }

    fn lapse(&self, token: &str) {
        if let Some(session) = self.session.lock().as_mut()
            && session
                .access
                .as_ref()
                .is_some_and(|access| access.token == token)
        {
            session.access = None;
        }
    }

    fn access(&self, client: &Client) -> Result<Option<String>> {
        let mut session = self.session.lock();
        let Some(grant) = self.grants.grant() else {
            *session = None;
            return Ok(None);
        };
        if !session.as_ref().is_some_and(|held| held.holds(&grant)) {
            *session = Some(Session {
                refresh_token: grant.refresh_token.clone(),
                given: grant,
                access: None,
            });
        }
        let Some(held) = session.as_mut() else {
            return Ok(None);
        };
        if let Some(access) = held
            .access
            .as_ref()
            .filter(|access| access.until > Instant::now())
        {
            return Ok(Some(access.token.clone()));
        }
        if client.is_held_by_the_meter(Host::SpotifyApi) {
            return Err(Error::Metered {
                host: Host::SpotifyApi,
                op: LookupOp::FollowLink,
            });
        }

        let answer = tokens(
            client,
            &self.endpoints,
            Params::new()
                .with("grant_type", "refresh_token")
                .with("refresh_token", &held.refresh_token)
                .with("client_id", &held.given.client_id),
        )?;
        let access = answer.access()?;
        if let Some(rotated) = answer.refresh_token.filter(|token| !token.is_empty())
            && rotated != held.refresh_token
        {
            self.grants.renewed(&held.given, &rotated);
            held.refresh_token = rotated;
        }
        let token = access.token.clone();
        held.access = Some(access);

        Ok(Some(token))
    }
}

fn paged(listed: &str, read: usize) -> String {
    format!("{listed}?limit={ASKED_A_PAGE}&offset={read}")
}

fn names_a_playlist(id: &str) -> bool {
    id.len() == ID_LENGTH && id.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn tokens(client: &Client, endpoints: &SpotifyEndpoints, fields: Params) -> Result<Tokens> {
    let url = format!("{}/api/token", endpoints.accounts);
    client
        .posted(
            Host::SpotifyAccounts,
            LookupOp::SpotifyAccount,
            &url,
            &Posted::form(fields),
        )?
        .ok_or(Error::Unreadable {
            host: Host::SpotifyAccounts,
            op: LookupOp::SpotifyAccount,
        })
}

#[derive(Deserialize)]
struct Tokens {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

impl Tokens {
    fn access(&self) -> Result<Access> {
        let unreadable = || Error::Unreadable {
            host: Host::SpotifyAccounts,
            op: LookupOp::SpotifyAccount,
        };
        let token = self
            .access_token
            .clone()
            .filter(|token| !token.is_empty())
            .ok_or_else(unreadable)?;
        let lasts = Duration::from_secs(self.expires_in.ok_or_else(unreadable)?)
            .saturating_sub(RENEWED_BEFORE_IT_LAPSES);
        let until = Instant::now().checked_add(lasts).ok_or_else(unreadable)?;

        Ok(Access { token, until })
    }
}

#[derive(Deserialize)]
struct Me {
    id: Option<String>,
    display_name: Option<String>,
}

#[derive(Deserialize)]
struct Page<T> {
    next: Option<String>,
    items: Vec<Option<T>>,
}

#[derive(Deserialize)]
struct PlaylistListed {
    id: Option<String>,
    name: Option<String>,
}

#[derive(Deserialize)]
struct Named {
    name: Option<String>,
}

trait Listing: DeserializeOwned {
    fn song(self) -> Option<Track>;
}

#[derive(Deserialize)]
struct Item {
    item: Option<Track>,
    track: Option<Track>,
}

impl Listing for Item {
    fn song(self) -> Option<Track> {
        self.item.or(self.track)
    }
}

#[derive(Deserialize)]
struct Saved {
    track: Option<Track>,
}

impl Listing for Saved {
    fn song(self) -> Option<Track> {
        self.track
    }
}

#[derive(Deserialize)]
struct Track {
    #[serde(rename = "type")]
    kind: Option<String>,
    name: Option<String>,
    artists: Option<Vec<Option<Billed>>>,
    duration_ms: Option<u64>,
    external_ids: Option<ExternalIds>,
}

#[derive(Deserialize)]
struct Billed {
    name: Option<String>,
}

#[derive(Deserialize)]
struct ExternalIds {
    isrc: Option<String>,
}

impl Track {
    fn named(self) -> Option<LinkNames> {
        if self.kind.as_deref().is_some_and(|kind| kind != A_TRACK) {
            return None;
        }
        let artists: Vec<String> = self
            .artists
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|artist| stated(artist.name.as_deref()))
            .collect();

        Some(LinkNames {
            isrcs: self
                .external_ids
                .and_then(|ids| ids.isrc)
                .and_then(|code| Isrc::new(&code).ok())
                .into_iter()
                .collect(),
            length: self
                .duration_ms
                .filter(|millis| *millis > 0)
                .map(Duration::from_millis),
            title: Some(stated(self.name.as_deref())?),
            artist: (!artists.is_empty()).then(|| artists.join(", ")),
        })
    }
}

#[derive(Debug, Error)]
pub enum SpotifySignInError {
    #[error("the sign-in cannot come back to {at}, which could not be opened")]
    NoPlaceToComeBack {
        at: SocketAddr,
        #[source]
        source: io::Error,
    },

    #[error("the Spotify sign-in was not approved in time")]
    Lapsed,

    #[error("Spotify was told not to sign in")]
    Denied,

    #[error(transparent)]
    Asked(#[from] Error),
}

pub struct SpotifyAuthorizing {
    pub authorize_at: String,
    listener: TcpListener,
    client_id: String,
    redirect: String,
    verifier: String,
    state: String,
    began: Instant,
}

impl fmt::Debug for SpotifyAuthorizing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpotifyAuthorizing")
            .field("redirect", &self.redirect)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct SpotifySignedIn {
    pub refresh_token: String,
}

impl fmt::Debug for SpotifySignedIn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpotifySignedIn").finish_non_exhaustive()
    }
}

pub struct SpotifySignIn {
    client: Arc<Client>,
    endpoints: SpotifyEndpoints,
    port: u16,
    lapses_after: Duration,
}

impl SpotifySignIn {
    pub fn new(client: Arc<Client>) -> Self {
        Self {
            client,
            endpoints: SpotifyEndpoints::spotify(),
            port: REDIRECT_PORT,
            lapses_after: A_SIGN_IN_LAPSES_AFTER,
        }
    }

    pub fn authorizing(
        &self,
        client_id: &str,
    ) -> std::result::Result<SpotifyAuthorizing, SpotifySignInError> {
        let at = SocketAddr::from((Ipv4Addr::LOCALHOST, self.port));
        let no_place = |source| SpotifySignInError::NoPlaceToComeBack { at, source };
        let listener = TcpListener::bind(at).map_err(no_place)?;
        listener.set_nonblocking(true).map_err(no_place)?;
        let port = listener.local_addr().map_err(no_place)?.port();

        let redirect = format!("http://127.0.0.1:{port}{REDIRECT_PATH}");
        let verifier = a_verifier();
        let state = Uuid::new_v4().simple().to_string();
        let authorize_at = format!(
            "{}/authorize{}",
            self.endpoints.accounts,
            Params::new()
                .with("response_type", "code")
                .with("client_id", client_id)
                .with("scope", SCOPES)
                .with("redirect_uri", &redirect)
                .with("code_challenge_method", "S256")
                .with("code_challenge", &challenge_of(&verifier))
                .with("state", &state)
                .finish()
        );

        Ok(SpotifyAuthorizing {
            authorize_at,
            listener,
            client_id: client_id.to_owned(),
            redirect,
            verifier,
            state,
            began: Instant::now(),
        })
    }

    pub fn authorized(
        &self,
        authorizing: &SpotifyAuthorizing,
        cancelled: &dyn Fn() -> bool,
    ) -> std::result::Result<Option<SpotifySignedIn>, SpotifySignInError> {
        let Some(code) = self.caught(authorizing, cancelled)? else {
            return Ok(None);
        };
        let answer = tokens(
            &self.client,
            &self.endpoints,
            Params::new()
                .with("grant_type", "authorization_code")
                .with("code", &code)
                .with("redirect_uri", &authorizing.redirect)
                .with("client_id", &authorizing.client_id)
                .with("code_verifier", &authorizing.verifier),
        )?;
        let refresh_token = answer
            .refresh_token
            .filter(|token| !token.is_empty())
            .ok_or(Error::Unreadable {
                host: Host::SpotifyAccounts,
                op: LookupOp::SpotifyAccount,
            })?;

        Ok(Some(SpotifySignedIn { refresh_token }))
    }

    fn caught(
        &self,
        authorizing: &SpotifyAuthorizing,
        cancelled: &dyn Fn() -> bool,
    ) -> std::result::Result<Option<String>, SpotifySignInError> {
        loop {
            if cancelled() {
                return Ok(None);
            }
            if authorizing.began.elapsed() >= self.lapses_after {
                return Err(SpotifySignInError::Lapsed);
            }
            match authorizing.listener.accept() {
                Ok((stream, _)) => {
                    if let Some(code) = heard(stream, &authorizing.state)? {
                        return Ok(Some(code));
                    }
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    thread::sleep(LOOKED_AT_EVERY);
                }
                Err(error) => {
                    tracing::debug!(%error, "a connection to the sign-in's page was lost");
                    thread::sleep(LOOKED_AT_EVERY);
                }
            }
        }
    }
}

fn heard(
    mut stream: TcpStream,
    state: &str,
) -> std::result::Result<Option<String>, SpotifySignInError> {
    let answered = request_head(&mut stream)
        .inspect_err(|error| tracing::debug!(%error, "a request to the sign-in's page was lost"));
    let Ok(head) = answered else {
        return Ok(None);
    };
    let Some(query) = redirect_query(&head) else {
        answer(&mut stream, "404 Not Found", NOT_THIS_SIGN_IN);
        return Ok(None);
    };
    if field_of(query, "state").as_deref() != Some(state) {
        answer(&mut stream, "400 Bad Request", NOT_THIS_SIGN_IN);
        return Ok(None);
    }
    if let Some(code) = field_of(query, "code") {
        answer(&mut stream, "200 OK", BACK_IN_THE_APP);
        return Ok(Some(code));
    }
    answer(&mut stream, "200 OK", TOLD_NOT_TO);

    Err(SpotifySignInError::Denied)
}

fn request_head(stream: &mut TcpStream) -> io::Result<String> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(A_REQUEST_ARRIVES_WITHIN))?;
    stream.set_write_timeout(Some(A_REQUEST_ARRIVES_WITHIN))?;
    let mut head = Vec::new();
    let mut piece = [0_u8; 1024];
    while !head.windows(4).any(|end| end == b"\r\n\r\n") && head.len() < LONGEST_REQUEST_HEAD {
        let read = stream.read(&mut piece)?;
        if read == 0 {
            break;
        }
        head.extend_from_slice(&piece[..read]);
    }

    Ok(String::from_utf8_lossy(&head).into_owned())
}

fn redirect_query(head: &str) -> Option<&str> {
    let mut asked = head.lines().next()?.split_ascii_whitespace();
    if asked.next()? != "GET" {
        return None;
    }
    let (path, query) = asked.next()?.split_once('?')?;

    (path == REDIRECT_PATH).then_some(query)
}

fn field_of(query: &str, wanted: &str) -> Option<String> {
    query
        .split('&')
        .filter_map(|field| field.split_once('='))
        .find(|(name, _)| *name == wanted)
        .map(|(_, value)| percent_decoded(value))
}

fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = (bytes[at] == b'%')
            .then(|| text.get(at + 1..at + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match hex {
            Some(byte) => {
                decoded.push(byte);
                at += 3;
            }
            None => {
                decoded.push(bytes[at]);
                at += 1;
            }
        }
    }

    String::from_utf8_lossy(&decoded).into_owned()
}

fn answer(stream: &mut TcpStream, status: &str, page: &str) {
    let written = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{page}",
        page.len()
    );
    if let Err(error) = written {
        tracing::debug!(%error, "the sign-in's page was not answered");
    }
}

fn a_verifier() -> String {
    (0..3)
        .map(|_| Uuid::new_v4().simple().to_string())
        .collect()
}

fn challenge_of(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use resonate_core::{Gate, Meter, Metering};
    use resonate_library::Reference as _;

    use super::*;
    use crate::{
        Online,
        client::{Carried, Clock},
    };

    const SEEN: &str = "37i9dQZF1DX0XUsuxWHRQd";
    const OWN: &str = "3cEYpjA9oz9GiPac4AsH4n";
    const TOKEN: &str = include_str!("../tests/fixtures/spotify_token.json");
    const ME: &str = include_str!("../tests/fixtures/spotify_me.json");
    const PLAYLISTS: &str = include_str!("../tests/fixtures/spotify_my_playlists.json");
    const ITEMS: &str = include_str!("../tests/fixtures/spotify_playlist_items.json");
    const SAVED: &str = include_str!("../tests/fixtures/spotify_saved_tracks.json");
    const EMBED: &str = include_str!("../tests/fixtures/spotify_playlist_embed.html");

    struct NoWaiting;

    impl Clock for NoWaiting {
        fn now(&self) -> Instant {
            Instant::now()
        }

        fn sleep(&self, _span: Duration) {}
    }

    #[derive(Clone, Debug)]
    struct Asked {
        path: String,
        head: String,
        body: String,
    }

    struct Fake {
        endpoints: SpotifyEndpoints,
        asked: Arc<Mutex<Vec<Asked>>>,
    }

    impl Fake {
        fn asked(&self) -> Vec<Asked> {
            self.asked.lock().clone()
        }

        fn paths(&self) -> Vec<String> {
            self.asked().into_iter().map(|asked| asked.path).collect()
        }
    }

    type Route = Box<dyn FnMut(&Asked) -> (u16, String) + Send>;

    fn serving(mut route: Route) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let at = listener.local_addr().expect("a bound address");
        let base = format!("http://{at}");
        let asked = Arc::new(Mutex::new(Vec::new()));
        let noted = Arc::clone(&asked);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else {
                    return;
                };
                let Some(request) = read_request(&mut stream) else {
                    continue;
                };
                noted.lock().push(request.clone());
                let (status, body) = route(&request);
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} Answered\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });

        Fake {
            endpoints: SpotifyEndpoints {
                accounts: format!("{base}/accounts"),
                api: format!("{base}/v1"),
                pages: format!("{base}/pages"),
            },
            asked,
        }
    }

    fn read_request(stream: &mut TcpStream) -> Option<Asked> {
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
        let mut held = Vec::new();
        let mut byte = [0_u8; 1];
        while !held.ends_with(b"\r\n\r\n") {
            if stream.read(&mut byte).ok()? != 1 {
                return None;
            }
            held.push(byte[0]);
        }
        let head = String::from_utf8_lossy(&held).into_owned();
        let length = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        let mut body = vec![0_u8; length];
        stream.read_exact(&mut body).ok()?;

        Some(Asked {
            path: head.split_ascii_whitespace().nth(1)?.to_owned(),
            head,
            body: String::from_utf8_lossy(&body).into_owned(),
        })
    }

    fn plain_client() -> Arc<Client> {
        Arc::new(Client::on_clock(
            crate::Introduction::as_(&crate::Identity::of_this_build()),
            Arc::new(NoWaiting),
            Carried::Plain,
        ))
    }

    struct Given {
        grant: Mutex<Option<SpotifyGrant>>,
        renewals: Mutex<Vec<String>>,
    }

    impl Given {
        fn signed_in() -> Arc<Self> {
            Arc::new(Self {
                grant: Mutex::new(Some(SpotifyGrant {
                    client_id: "registered".to_owned(),
                    refresh_token: "first-refresh".to_owned(),
                })),
                renewals: Mutex::new(Vec::new()),
            })
        }

        fn nobody() -> Arc<Self> {
            Arc::new(Self {
                grant: Mutex::new(None),
                renewals: Mutex::new(Vec::new()),
            })
        }
    }

    impl SpotifyGrants for Given {
        fn grant(&self) -> Option<SpotifyGrant> {
            self.grant.lock().clone()
        }

        fn renewed(&self, _grant: &SpotifyGrant, token: &str) {
            self.renewals.lock().push(token.to_owned());
        }
    }

    fn online_over(fake: &Fake, grants: Arc<Given>) -> Online {
        online_on(fake, grants, plain_client())
    }

    fn online_on(fake: &Fake, grants: Arc<Given>, client: Arc<Client>) -> Online {
        Online::with_client(client)
            .with_spotify(Arc::new(SpotifyAccount::at(grants, fake.endpoints.clone())))
    }

    fn a_song(number: usize) -> String {
        format!(
            r#"{{"added_at":"2024-01-01T00:00:00Z","is_local":false,"item":{{"type":"track",
            "name":"Song {number}","duration_ms":200000,"artists":[{{"name":"Billed"}}],
            "is_local":false,"external_ids":{{"isrc":"USRC1{number:07}"}}}}}}"#
        )
    }

    fn a_page_of(from: usize, count: usize, more: bool) -> String {
        let items: Vec<String> = (from..from + count).map(a_song).collect();
        let next = match more {
            true => r#""https://api.spotify.com/v1/more""#,
            false => "null",
        };
        format!(r#"{{"next":{next},"items":[{}]}}"#, items.join(","))
    }

    fn account_route() -> Route {
        Box::new(|asked| {
            let path = asked.path.as_str();
            if path.ends_with("/api/token") {
                (200, TOKEN.to_owned())
            } else if path.starts_with("/v1/me/playlists") {
                (200, PLAYLISTS.to_owned())
            } else if path.starts_with("/v1/me/tracks") {
                (200, SAVED.to_owned())
            } else if path.starts_with("/v1/me") {
                (200, ME.to_owned())
            } else if path.starts_with(&format!("/v1/playlists/{OWN}/items")) {
                (200, ITEMS.to_owned())
            } else if path.starts_with(&format!("/v1/playlists/{OWN}")) {
                (200, r#"{"name":"Kept for the road"}"#.to_owned())
            } else if path.starts_with(&format!("/v1/playlists/{SEEN}/items")) {
                (403, r#"{"error":{"status":403}}"#.to_owned())
            } else if path.starts_with(&format!("/pages/embed/playlist/{SEEN}")) {
                (200, EMBED.to_owned())
            } else {
                (404, String::new())
            }
        })
    }

    fn read_back(uri: &str, field: &str) -> Option<String> {
        field_of(uri.split_once('?')?.1, field)
    }

    fn sign_in_over(fake: &Fake, lapses_after: Duration) -> SpotifySignIn {
        SpotifySignIn {
            client: plain_client(),
            endpoints: fake.endpoints.clone(),
            port: 0,
            lapses_after,
        }
    }

    fn browse_to(redirect: &str, query: &str) -> String {
        let at = redirect
            .strip_prefix("http://")
            .and_then(|rest| rest.split('/').next())
            .expect("a loopback address");
        let mut stream = TcpStream::connect(at).expect("the sign-in listens");
        write!(
            stream,
            "GET {REDIRECT_PATH}?{query} HTTP/1.1\r\nHost: {at}\r\n\r\n"
        )
        .expect("a request");
        let mut answer = String::new();
        stream.read_to_string(&mut answer).expect("an answer");
        answer
    }

    #[test]
    fn a_sign_in_asks_with_pkce_catches_the_redirect_and_keeps_the_refresh_token() {
        let fake = serving(Box::new(|_| (200, TOKEN.to_owned())));
        let signing = sign_in_over(&fake, Duration::from_secs(60));

        let authorizing = signing
            .authorizing("registered")
            .expect("a place to listen");
        let uri = authorizing.authorize_at.clone();
        let redirect = read_back(&uri, "redirect_uri").expect("a redirect");
        let state = read_back(&uri, "state").expect("a state");

        assert!(uri.starts_with(&format!("{}/authorize?", fake.endpoints.accounts)));
        assert_eq!(read_back(&uri, "response_type").as_deref(), Some("code"));
        assert_eq!(read_back(&uri, "client_id").as_deref(), Some("registered"));
        assert_eq!(read_back(&uri, "scope").as_deref(), Some(SCOPES));
        assert_eq!(
            read_back(&uri, "code_challenge_method").as_deref(),
            Some("S256")
        );
        assert!(redirect.starts_with("http://127.0.0.1:"));
        assert!(redirect.ends_with(REDIRECT_PATH));

        let browser = {
            let redirect = redirect.clone();
            let state = state.clone();
            thread::spawn(move || {
                let stranger = browse_to(&redirect, "code=stolen&state=someone-else");
                let picture = browse_to(&redirect, &format!("state={state}&code=theirs%2Fcode"));
                (stranger, picture)
            })
        };
        let signed_in = signing
            .authorized(&authorizing, &|| false)
            .expect("a sign-in")
            .expect("not cancelled");
        let (stranger, approved) = browser.join().expect("a browser");
        let asked = fake.asked();

        assert_eq!(signed_in.refresh_token, "first-refresh");
        assert!(stranger.starts_with("HTTP/1.1 400"), "{stranger}");
        assert!(approved.starts_with("HTTP/1.1 200"), "{approved}");
        assert!(approved.contains(BACK_IN_THE_APP));
        assert_eq!(asked.len(), 1, "the stranger's code is never exchanged");
        assert_eq!(asked[0].path, "/accounts/api/token");

        let form = format!("?{}", asked[0].body);
        let verifier = read_back(&form, "code_verifier").expect("a verifier");
        assert_eq!(
            read_back(&form, "grant_type").as_deref(),
            Some("authorization_code")
        );
        assert_eq!(read_back(&form, "code").as_deref(), Some("theirs/code"));
        assert_eq!(
            read_back(&form, "redirect_uri").as_deref(),
            Some(redirect.as_str())
        );
        assert_eq!(read_back(&form, "client_id").as_deref(), Some("registered"));
        assert!((43..=128).contains(&verifier.len()), "{}", verifier.len());
        assert_eq!(
            read_back(&uri, "code_challenge"),
            Some(challenge_of(&verifier))
        );
        assert!(
            !asked[0]
                .head
                .to_ascii_lowercase()
                .contains("authorization:"),
            "a PKCE client has no secret to send"
        );
    }

    #[test]
    fn the_challenge_is_the_url_safe_unpadded_sha_256_of_the_verifier() {
        assert_eq!(
            challenge_of("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn a_sign_in_refused_cancelled_or_left_alone_ends_with_no_token() {
        let fake = serving(Box::new(|_| (200, TOKEN.to_owned())));
        let signing = sign_in_over(&fake, Duration::from_secs(60));

        let authorizing = signing
            .authorizing("registered")
            .expect("a place to listen");
        let redirect = read_back(&authorizing.authorize_at, "redirect_uri").expect("a redirect");
        let state = read_back(&authorizing.authorize_at, "state").expect("a state");
        let browser = thread::spawn(move || {
            browse_to(&redirect, &format!("error=access_denied&state={state}"))
        });
        assert!(matches!(
            signing.authorized(&authorizing, &|| false),
            Err(SpotifySignInError::Denied)
        ));
        browser.join().expect("a browser");

        let authorizing = signing
            .authorizing("registered")
            .expect("a place to listen");
        assert!(matches!(
            signing.authorized(&authorizing, &|| true),
            Ok(None)
        ));

        let lapsing = sign_in_over(&fake, Duration::ZERO);
        let authorizing = lapsing
            .authorizing("registered")
            .expect("a place to listen");
        assert!(matches!(
            lapsing.authorized(&authorizing, &|| false),
            Err(SpotifySignInError::Lapsed)
        ));

        assert!(fake.asked().is_empty(), "no code, nothing exchanged");
    }

    #[test]
    fn a_port_already_taken_says_so_before_anything_is_asked() {
        let fake = serving(Box::new(|_| (200, TOKEN.to_owned())));
        let taken = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let signing = SpotifySignIn {
            port: taken.local_addr().expect("an address").port(),
            ..sign_in_over(&fake, Duration::from_secs(60))
        };

        assert!(matches!(
            signing.authorizing("registered"),
            Err(SpotifySignInError::NoPlaceToComeBack { .. })
        ));
        assert!(fake.asked().is_empty());
    }

    #[test]
    fn a_signed_in_account_offers_its_liked_songs_and_every_playlist_it_lists() {
        let fake = serving(account_route());
        let online = online_over(&fake, Given::signed_in());

        let profile = online
            .profile_linked(&resonate_library::ProfileLink::SpotifyAccount)
            .expect("an answer")
            .expect("an account");

        assert_eq!(profile.name, "A Listener");
        assert_eq!(profile.playlists[0].link, PlaylistLink::SpotifyLiked);
        assert_eq!(profile.playlists[0].title, LIKED_SONGS);
        assert_eq!(
            profile
                .playlists
                .iter()
                .skip(1)
                .map(|listed| (listed.title.as_str(), listed.link.clone()))
                .collect::<Vec<_>>(),
            [
                ("Kept for the road", PlaylistLink::Spotify(OWN.to_owned())),
                (
                    "Followed, not owned",
                    PlaylistLink::Spotify(SEEN.to_owned())
                ),
            ]
        );
    }

    #[test]
    fn an_owned_playlists_songs_come_with_their_isrcs_and_an_episode_or_gap_is_passed_over() {
        let fake = serving(account_route());
        let online = online_over(&fake, Given::signed_in());

        let read = online
            .playlist_linked(&PlaylistLink::Spotify(OWN.to_owned()))
            .expect("an answer")
            .expect("a playlist");
        let mut songs = read.songs.into_iter().map(|song| match song {
            ListedSong::Named(names) => names,
            other => panic!("{other:?}"),
        });
        let first = songs.next().expect("a song");
        let local = songs.next().expect("a local file");

        assert_eq!(read.name, "Kept for the road");
        assert_eq!(first.title.as_deref(), Some("Sea of Glass"));
        assert_eq!(first.artist.as_deref(), Some("First Billed, Second Billed"));
        assert_eq!(first.length, Some(Duration::from_millis(234_567)));
        assert_eq!(
            first.isrcs,
            [Isrc::new("USRC17607839").expect("a code")],
            "the code the Web API names"
        );
        assert_eq!(local.title.as_deref(), Some("Ripped from a tape"));
        assert!(local.isrcs.is_empty(), "named alone, for the strict search");
        assert_eq!(songs.next(), None, "the episode and the gap are no songs");
    }

    #[test]
    fn every_page_of_a_long_list_is_read_past_what_the_embed_page_holds() {
        let fake = serving(Box::new(|asked| {
            let path = asked.path.as_str();
            if path.ends_with("/api/token") {
                return (200, TOKEN.to_owned());
            }
            if let Some(offset) = path.split("offset=").nth(1) {
                let from: usize = offset.parse().expect("an offset");
                return (200, a_page_of(from, 50, from < 200));
            }
            (200, r#"{"name":"Long"}"#.to_owned())
        }));
        let online = online_over(&fake, Given::signed_in());

        let read = online
            .playlist_linked(&PlaylistLink::Spotify(OWN.to_owned()))
            .expect("an answer")
            .expect("a playlist");

        assert_eq!(read.songs.len(), 250);
        assert_eq!(
            fake.paths()
                .iter()
                .filter(|path| path.contains("/items?limit=50"))
                .count(),
            5
        );
    }

    #[test]
    fn the_liked_songs_are_read_with_their_isrcs() {
        let fake = serving(account_route());
        let online = online_over(&fake, Given::signed_in());

        let read = online
            .playlist_linked(&PlaylistLink::SpotifyLiked)
            .expect("an answer")
            .expect("a list");

        assert_eq!(read.name, LIKED_SONGS);
        assert_eq!(
            read.songs,
            [ListedSong::Named(LinkNames {
                isrcs: vec![Isrc::new("GBAYE0601498").expect("a code")],
                length: Some(Duration::from_millis(180_000)),
                title: Some("Saved Song".to_owned()),
                artist: Some("Saved Artist".to_owned()),
            })]
        );
        assert!(
            fake.paths()
                .contains(&"/v1/me/tracks?limit=50&offset=0".to_owned())
        );
    }

    #[test]
    fn a_playlist_the_api_will_not_list_is_read_off_the_public_page_with_no_token() {
        let fake = serving(account_route());
        let online = online_over(&fake, Given::signed_in());

        let read = online
            .playlist_linked(&PlaylistLink::Spotify(SEEN.to_owned()))
            .expect("an answer")
            .expect("what the page holds");
        let page = fake
            .asked()
            .into_iter()
            .find(|asked| asked.path.starts_with("/pages/embed/playlist/"))
            .expect("the page was read");

        assert!(!read.songs.is_empty());
        assert!(
            !page.head.to_ascii_lowercase().contains("authorization:"),
            "the listener's token never goes to the page"
        );
    }

    #[test]
    fn a_page_failing_part_way_fails_the_read_rather_than_answering_a_short_list() {
        let fake = serving(Box::new(|asked| {
            let path = asked.path.as_str();
            if path.ends_with("/api/token") {
                (200, TOKEN.to_owned())
            } else if path.ends_with("offset=0") {
                (200, a_page_of(0, 50, true))
            } else {
                (403, String::new())
            }
        }));
        let online = online_over(&fake, Given::signed_in());

        assert!(matches!(
            online.playlist_linked(&PlaylistLink::SpotifyLiked),
            Err(resonate_library::Error::Refused { status: 403, .. })
        ));
    }

    #[test]
    fn no_account_asks_nothing_of_the_api_and_a_pasted_link_keeps_its_public_page() {
        let fake = serving(account_route());
        let online = online_over(&fake, Given::nobody());

        assert_eq!(
            online
                .profile_linked(&resonate_library::ProfileLink::SpotifyAccount)
                .expect("no question"),
            None
        );
        assert_eq!(
            online
                .playlist_linked(&PlaylistLink::SpotifyLiked)
                .expect("no question"),
            None
        );
        assert!(fake.asked().is_empty());

        let read = online
            .playlist_linked(&PlaylistLink::Spotify(SEEN.to_owned()))
            .expect("the public page")
            .expect("what the page holds");

        assert!(!read.songs.is_empty());
        assert_eq!(
            fake.paths(),
            [format!("/pages/embed/playlist/{SEEN}")],
            "the page alone"
        );
    }

    #[test]
    fn an_online_with_no_account_at_all_names_neither_the_account_nor_the_liked_songs() {
        let online = Online::with_client(plain_client());

        assert_eq!(
            online
                .profile_linked(&resonate_library::ProfileLink::SpotifyAccount)
                .expect("no question"),
            None
        );
        assert_eq!(
            online
                .playlist_linked(&PlaylistLink::SpotifyLiked)
                .expect("no question"),
            None
        );
    }

    #[test]
    fn a_token_is_renewed_once_when_refused_and_a_rotated_one_handed_back() {
        let refusals = Arc::new(Mutex::new(VecDeque::from([401_u16])));
        let held = Arc::clone(&refusals);
        let fake = serving(Box::new(move |asked| {
            let path = asked.path.as_str();
            if path.ends_with("/api/token") {
                let renewed = asked.body.contains("refresh_token=rotated");
                return (
                    200,
                    format!(
                        r#"{{"access_token":"{}","refresh_token":"rotated","expires_in":3600,
                        "token_type":"Bearer"}}"#,
                        if renewed { "third" } else { "second" }
                    ),
                );
            }
            match held.lock().pop_front() {
                Some(status) if path.starts_with("/v1/me/tracks") => (status, String::new()),
                _ => (200, SAVED.to_owned()),
            }
        }));
        let grants = Given::signed_in();
        let online = online_over(&fake, Arc::clone(&grants));

        online
            .playlist_linked(&PlaylistLink::SpotifyLiked)
            .expect("an answer after a second token");
        let tokened: Vec<String> = fake
            .asked()
            .into_iter()
            .filter(|asked| asked.path.ends_with("/api/token"))
            .map(|asked| asked.body)
            .collect();

        assert_eq!(tokened.len(), 2);
        assert!(tokened[0].contains("grant_type=refresh_token"));
        assert!(tokened[0].contains("refresh_token=first-refresh"));
        assert!(tokened[0].contains("client_id=registered"));
        assert!(
            tokened[1].contains("refresh_token=rotated"),
            "the second sign-in uses the rotated token: {}",
            tokened[1]
        );
        assert_eq!(*grants.renewals.lock(), ["rotated"]);
    }

    #[test]
    fn an_access_token_is_used_until_it_lapses_and_settings_that_moved_start_afresh() {
        let fake = serving(account_route());
        let grants = Given::signed_in();
        let online = online_over(&fake, Arc::clone(&grants));
        let tokens_asked = |fake: &Fake| {
            fake.paths()
                .iter()
                .filter(|path| path.ends_with("/api/token"))
                .count()
        };

        online
            .playlist_linked(&PlaylistLink::SpotifyLiked)
            .expect("an answer");
        online
            .playlist_linked(&PlaylistLink::SpotifyLiked)
            .expect("an answer");
        assert_eq!(tokens_asked(&fake), 1);

        *grants.grant.lock() = Some(SpotifyGrant {
            client_id: "registered".to_owned(),
            refresh_token: "another-sign-in".to_owned(),
        });
        online
            .playlist_linked(&PlaylistLink::SpotifyLiked)
            .expect("an answer");
        assert_eq!(tokens_asked(&fake), 2);

        *grants.grant.lock() = None;
        assert_eq!(
            online
                .playlist_linked(&PlaylistLink::SpotifyLiked)
                .expect("no question"),
            None
        );
        assert_eq!(tokens_asked(&fake), 2, "signed out, nothing is asked");
    }

    #[test]
    fn a_refresh_token_refused_is_said_as_the_accounts_refusal() {
        let fake = serving(Box::new(|asked| {
            if asked.path.ends_with("/api/token") {
                (400, r#"{"error":"invalid_grant"}"#.to_owned())
            } else {
                (200, SAVED.to_owned())
            }
        }));
        let online = online_over(&fake, Given::signed_in());

        assert!(matches!(
            online.playlist_linked(&PlaylistLink::SpotifyLiked),
            Err(resonate_library::Error::Refused {
                op: LookupOp::SpotifyAccount,
                status: 400
            })
        ));
    }

    #[test]
    fn an_answer_with_no_access_token_or_an_impossible_lifetime_is_unreadable() {
        let answer = Tokens {
            access_token: None,
            refresh_token: None,
            expires_in: Some(3600),
        };
        assert!(matches!(answer.access(), Err(Error::Unreadable { .. })));

        let forever = Tokens {
            access_token: Some("token".to_owned()),
            refresh_token: None,
            expires_in: Some(u64::MAX),
        };
        assert!(matches!(forever.access(), Err(Error::Unreadable { .. })));
    }

    #[test]
    fn a_switch_off_or_a_metered_connection_sends_nothing() {
        let fake = serving(account_route());
        let client = plain_client();
        client.reach(false);
        let online = online_on(&fake, Given::signed_in(), Arc::clone(&client));

        assert!(online.playlist_linked(&PlaylistLink::SpotifyLiked).is_err());
        assert!(fake.asked().is_empty(), "online is off");

        let metering = Metering::new();
        metering.report(Meter::Metered);
        let gated = Arc::new(
            Client::on_clock(
                crate::Introduction::as_(&crate::Identity::of_this_build()),
                Arc::new(NoWaiting),
                Carried::Plain,
            )
            .gated_by(Gate::new(metering, true)),
        );
        let online = online_on(&fake, Given::signed_in(), gated);

        assert!(online.playlist_linked(&PlaylistLink::SpotifyLiked).is_err());
        assert!(fake.asked().is_empty(), "the connection is metered");
    }

    #[test]
    fn a_request_says_what_this_build_is_and_a_grant_prints_no_token() {
        let fake = serving(account_route());
        let online = online_over(&fake, Given::signed_in());

        online
            .playlist_linked(&PlaylistLink::SpotifyLiked)
            .expect("an answer");
        let said = format!("user-agent: resonate/{}", env!("CARGO_PKG_VERSION"));

        for asked in fake.asked() {
            assert!(
                asked.head.to_ascii_lowercase().contains(&said),
                "{}",
                asked.head
            );
        }
        let printed = format!(
            "{:?} {:?}",
            SpotifyGrant {
                client_id: "registered".to_owned(),
                refresh_token: "first-refresh".to_owned(),
            },
            SpotifySignedIn {
                refresh_token: "first-refresh".to_owned(),
            }
        );
        assert!(!printed.contains("first-refresh"), "{printed}");
    }

    #[test]
    fn a_sign_in_failure_stays_small() {
        assert!(
            size_of::<SpotifySignInError>() <= 128,
            "{}",
            size_of::<SpotifySignInError>()
        );
    }
}
