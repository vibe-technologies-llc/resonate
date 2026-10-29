mod account;
mod asker;
mod catalog;
mod fetched;
mod hifi;
mod hifi_served;
mod manifest;
mod played;
mod remux;
mod searched;
mod sign_in;

use std::time::{Duration, Instant};

use parking_lot::Mutex;
use resonate_core::{Isrc, SourceId};
use resonate_fetch::escaped;
use resonate_providers::{
    AlbumSongs, Artist, ArtistAlbums, Choosing, Delivery, Error, Found, Id, Identity, MixSongs,
    Named, Obtained, PlaysInPlace, Provider, ProviderOp, RefreshToken, Result, Streamed,
};
use serde::Deserialize;
use ureq::{
    Agent,
    http::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE},
};

#[cfg(fuzzing)]
pub fn read_a_manifest(bytes: &[u8]) {
    for mime in ["application/vnd.tidal.bts", "application/dash+xml"] {
        let _ = crate::manifest::read_document(mime, bytes);
    }
}

pub use crate::{
    account::{Account, Endpoints, MediaHosts},
    hifi::{HifiApi, Hosting, OWN_SERVER},
    sign_in::TidalSignIn,
};
use crate::{
    asker::{Asker, Sent, media_agent},
    catalog::AsksV1,
    played::{Finds, Playback, Player, TrackId, named_by},
    searched::Searched,
};

const TIDAL: &str = "tidal";
const RENEWED_BEFORE: Duration = Duration::from_secs(60);
const LASTS_WHEN_UNSAID: Duration = Duration::from_secs(3600);
const BAD_REQUEST: u16 = 400;
const UNAUTHORISED: u16 = 401;
const FORBIDDEN: u16 = 403;
const NOT_FOUND: u16 = 404;
const NOT_READY_FOR_PLAYBACK: u16 = 4005;
const ASKED_QUALITY: &str = "HI_RES_LOSSLESS";
const JSON_API: &str = "application/vnd.api+json";
const FORM: &str = "application/x-www-form-urlencoded";
const TRACKS_TRIED_AT_MOST: usize = 5;
const CODES_ASKED_AT_MOST: usize = 4;
const LISTED_AT_MOST: usize = 25;

#[derive(Clone)]
struct Session {
    bearer: String,
    country: String,
    user: Option<u64>,
    until: Instant,
}

#[derive(Deserialize)]
struct Granted {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    user: Option<Holder>,
    #[serde(default, deserialize_with = "catalog::user_spelt")]
    user_id: Option<u64>,
}

#[derive(Deserialize)]
struct Holder {
    #[serde(default, rename = "countryCode")]
    country: Option<String>,
    #[serde(default, rename = "userId", deserialize_with = "catalog::user_spelt")]
    user: Option<u64>,
}

#[derive(Deserialize)]
struct Sessions {
    #[serde(rename = "countryCode")]
    country: String,
    #[serde(default, rename = "userId", deserialize_with = "catalog::user_spelt")]
    user: Option<u64>,
}

#[derive(Deserialize)]
struct Listed {
    #[serde(default)]
    data: Vec<Listing>,
}

#[derive(Deserialize)]
struct Listing {
    id: String,
    #[serde(default)]
    attributes: Option<Attributes>,
}

#[derive(Deserialize)]
struct Attributes {
    #[serde(default)]
    isrc: Option<String>,
}

enum Verb {
    Get,
    Add(String),
    Remove,
}

enum Asked {
    Answered(Vec<u8>),
    Unavailable,
}

type Renewed = Box<dyn Fn(RefreshToken) + Send + Sync>;

pub struct Tidal {
    asker: Asker,
    account: Mutex<Account>,
    endpoints: Endpoints,
    media: Agent,
    session: Mutex<Option<Session>>,
    renewed: Option<Renewed>,
}

pub(crate) fn source() -> SourceId {
    SourceId::new(TIDAL).unwrap_or_else(|_| SourceId::local())
}

impl Tidal {
    pub fn signed_in(account: Account) -> Self {
        Self::at(account, Endpoints::tidal())
    }

    pub fn at(account: Account, endpoints: Endpoints) -> Self {
        Self {
            asker: Asker::new(source()),
            account: Mutex::new(account),
            endpoints,
            media: media_agent(),
            session: Mutex::new(None),
            renewed: None,
        }
    }

    #[must_use]
    pub fn telling(self, renewed: impl Fn(RefreshToken) + Send + Sync + 'static) -> Self {
        Self {
            renewed: Some(Box::new(renewed)),
            ..self
        }
    }

    fn rotated_to(&self, rotated: String) {
        {
            let mut account = self.account.lock();
            if rotated.is_empty() || account.refresh_token == rotated {
                return;
            }
            account.refresh_token.clone_from(&rotated);
        }
        tracing::info!(provider = %self.asker.source, "TIDAL rotated the refresh token");
        if let Some(renewed) = &self.renewed {
            renewed(RefreshToken::new(rotated));
        }
    }

    fn refusal(&self, op: ProviderOp, status: u16, complaint: Option<u16>) -> Error {
        let provider = self.asker.source.clone();
        match (status, complaint) {
            (UNAUTHORISED, Some(NOT_READY_FOR_PLAYBACK)) => Error::TurnedAway {
                provider,
                op,
                code: NOT_READY_FOR_PLAYBACK,
            },
            (UNAUTHORISED, _) => Error::Unwelcome {
                provider,
                op,
                code: complaint.unwrap_or(status),
            },
            _ => Error::Refused {
                provider,
                op,
                status,
            },
        }
    }

    fn sign_in(&self) -> Result<Session> {
        let op = ProviderOp::SignIn;
        let account = self.account.lock().clone();
        let mut form = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", account.refresh_token.as_str()),
            ("client_id", account.client_id.as_str()),
        ];
        if let Some(secret) = &account.client_secret {
            form.push(("client_secret", secret.as_str()));
        }
        let response = match self.asker.sent(op, || {
            self.asker
                .agent
                .post(&self.endpoints.auth)
                .send_form(form.iter().copied())
        })? {
            Sent::Answered(response) => response,
            Sent::Refused { status, complaint } if matches!(status, BAD_REQUEST | UNAUTHORISED) => {
                return Err(Error::Unwelcome {
                    provider: self.asker.source.clone(),
                    op,
                    code: complaint.unwrap_or(status),
                });
            }
            Sent::Refused { status, complaint } => {
                return Err(self.refusal(op, status, complaint));
            }
        };
        let granted: Granted = self
            .asker
            .parsed(op, &self.asker.read_whole(op, response)?)?;
        if let Some(rotated) = granted.refresh_token {
            self.rotated_to(rotated);
        }
        let lasts = granted
            .expires_in
            .map_or(LASTS_WHEN_UNSAID, Duration::from_secs)
            .saturating_sub(RENEWED_BEFORE);
        let bearer = format!("Bearer {}", granted.access_token);
        let (country, user) = match granted.user {
            Some(Holder {
                country: Some(country),
                user,
            }) => (country, user.or(granted.user_id)),
            _ => {
                let sessions = self.sessions_of(&bearer)?;
                (sessions.country, sessions.user.or(granted.user_id))
            }
        };
        Ok(Session {
            bearer,
            country,
            user,
            until: Instant::now()
                .checked_add(lasts)
                .ok_or_else(|| self.asker.unreadable(op))?,
        })
    }

    fn sessions_of(&self, bearer: &str) -> Result<Sessions> {
        let op = ProviderOp::SignIn;
        let url = format!("{}sessions", self.endpoints.api);
        match self.asker.sent(op, || {
            self.asker
                .agent
                .get(&url)
                .header(AUTHORIZATION, bearer)
                .call()
        })? {
            Sent::Answered(response) => {
                self.asker.parsed(op, &self.asker.read_whole(op, response)?)
            }
            Sent::Refused { status, complaint } => Err(self.refusal(op, status, complaint)),
        }
    }

    fn user(&self) -> Result<Option<u64>> {
        let session = self.session()?;
        if session.user.is_some() {
            return Ok(session.user);
        }
        let user = self.sessions_of(&session.bearer)?.user;
        if let Some(held) = self.session.lock().as_mut() {
            held.user = user;
        }
        Ok(user)
    }

    fn session(&self) -> Result<Session> {
        let mut held = self.session.lock();
        if let Some(session) = held
            .as_ref()
            .filter(|session| session.until > Instant::now())
        {
            return Ok(session.clone());
        }
        let fresh = self.sign_in()?;
        *held = Some(fresh.clone());
        Ok(fresh)
    }

    fn asked(
        &self,
        op: ProviderOp,
        url: impl Fn(&str) -> String,
        accept: Option<&str>,
    ) -> Result<Asked> {
        self.exchanged(op, &Verb::Get, url, accept)
    }

    fn exchanged(
        &self,
        op: ProviderOp,
        verb: &Verb,
        url: impl Fn(&str) -> String,
        accept: Option<&str>,
    ) -> Result<Asked> {
        let mut renewed = false;
        loop {
            let session = self.session()?;
            let url = url(&session.country);
            let sent = self.asker.sent(op, || match verb {
                Verb::Get => {
                    let request = self
                        .asker
                        .agent
                        .get(&url)
                        .header(AUTHORIZATION, session.bearer.as_str());
                    match accept {
                        Some(accept) => request.header(ACCEPT, accept).call(),
                        None => request.call(),
                    }
                }
                Verb::Add(form) => self
                    .asker
                    .agent
                    .post(&url)
                    .header(AUTHORIZATION, session.bearer.as_str())
                    .header(CONTENT_TYPE, FORM)
                    .send(form.as_str()),
                Verb::Remove => self
                    .asker
                    .agent
                    .delete(&url)
                    .header(AUTHORIZATION, session.bearer.as_str())
                    .call(),
            })?;
            match sent {
                Sent::Answered(response) => {
                    return self.asker.read_whole(op, response).map(Asked::Answered);
                }
                Sent::Refused {
                    status: UNAUTHORISED,
                    complaint,
                } if !renewed && complaint != Some(NOT_READY_FOR_PLAYBACK) => {
                    *self.session.lock() = None;
                    renewed = true;
                }
                Sent::Refused {
                    status: FORBIDDEN | NOT_FOUND,
                    ..
                } => return Ok(Asked::Unavailable),
                Sent::Refused { status, complaint } => {
                    return Err(self.refusal(op, status, complaint));
                }
            }
        }
    }
}

impl Finds for Tidal {
    fn provider(&self) -> &SourceId {
        &self.asker.source
    }

    fn tracks_for(&self, identity: &Identity) -> Result<Vec<TrackId>> {
        let mut tracks: Vec<TrackId> = Vec::new();
        for isrc in identity.isrcs.iter().take(CODES_ASKED_AT_MOST) {
            for track in self.tracks_coded(isrc)? {
                if !tracks.contains(&track) {
                    tracks.push(track);
                }
            }
        }
        if tracks.is_empty() {
            return self.tracks_named(identity);
        }
        tracks.truncate(TRACKS_TRIED_AT_MOST);
        Ok(tracks)
    }

    fn delivered(&self, track: TrackId) -> Result<Option<Delivery>> {
        self.delivery(track)
    }
}

impl Tidal {
    fn tracks_coded(&self, isrc: &Isrc) -> Result<Vec<TrackId>> {
        let op = ProviderOp::Search;
        let asked = self.asked(
            op,
            |country| {
                format!(
                    "{}tracks?countryCode={country}&filter%5Bisrc%5D={}",
                    self.endpoints.openapi,
                    isrc.as_str()
                )
            },
            Some(JSON_API),
        )?;
        let Asked::Answered(bytes) = asked else {
            return Ok(Vec::new());
        };
        let listed: Listed = self.asker.parsed(op, &bytes)?;
        Ok(listed
            .data
            .into_iter()
            .filter(|listing| {
                named_by(
                    isrc,
                    listing
                        .attributes
                        .as_ref()
                        .and_then(|attributes| attributes.isrc.as_deref()),
                )
            })
            .filter_map(|listing| listing.id.trim().parse().ok().map(TrackId))
            .take(TRACKS_TRIED_AT_MOST)
            .collect())
    }

    fn tracks_named(&self, identity: &Identity) -> Result<Vec<TrackId>> {
        let mut choosing = Choosing::default();
        let wordings = identity.wordings();
        for words in &wordings {
            for track in self.tracks_worded(words)? {
                choosing.weigh(identity, &track.weighed(), track.id);
            }
            if choosing.holds_a_taken_listing() {
                break;
            }
        }
        let seen = choosing.seen();
        let ranked: Vec<TrackId> = choosing
            .ranked()
            .into_iter()
            .map(|(track, taken)| {
                tracing::debug!(track = track.0, ?taken, "TIDAL lists the song by its name");
                track
            })
            .take(TRACKS_TRIED_AT_MOST)
            .collect();
        if ranked.is_empty() {
            tracing::debug!(
                title = identity.title,
                asked = wordings.len(),
                seen,
                "TIDAL lists nothing coded or named as the song"
            );
        }
        Ok(ranked)
    }

    fn tracks_worded(&self, words: &str) -> Result<Vec<searched::Track>> {
        let op = ProviderOp::Search;
        let asked = self.asked(
            op,
            |country| {
                format!(
                    "{}searchResults/{}?countryCode={country}&include=tracks%2Ctracks.artists",
                    self.endpoints.openapi,
                    escaped(words)
                )
            },
            Some(JSON_API),
        )?;
        let Asked::Answered(bytes) = asked else {
            return Ok(Vec::new());
        };
        let searched: Searched = self.asker.parsed(op, &bytes)?;
        let mut tracks = searched.tracks();
        tracks.truncate(LISTED_AT_MOST);
        Ok(tracks)
    }

    fn delivery(&self, track: TrackId) -> Result<Option<Delivery>> {
        let media = self.media(track)?;
        self.player().delivered_media(track, media)
    }

    fn player(&self) -> Player<'_> {
        Player {
            source: &self.asker.source,
            hosts: &self.endpoints.media,
            media: &self.media,
        }
    }

    fn media(&self, track: TrackId) -> Result<Option<manifest::Media>> {
        let op = ProviderOp::Playback;
        let asked = self.asked(
            op,
            |country| {
                format!(
                    "{}tracks/{}/playbackinfopostpaywall?countryCode={country}&audioquality={ASKED_QUALITY}&playbackmode=STREAM&assetpresentation=FULL",
                    self.endpoints.api, track.0
                )
            },
            None,
        )?;
        let Asked::Answered(bytes) = asked else {
            return Ok(None);
        };
        let playback: Playback = self.asker.parsed(op, &bytes)?;
        self.player().media_in(track, &playback)
    }
}

impl AsksV1 for Tidal {
    fn provider(&self) -> &SourceId {
        &self.asker.source
    }

    fn v1(&self, op: ProviderOp, path: &str) -> Result<Option<Vec<u8>>> {
        let joined = if path.contains('?') { '&' } else { '?' };
        match self.asked(
            op,
            |country| format!("{}{path}{joined}countryCode={country}", self.endpoints.api),
            None,
        )? {
            Asked::Answered(bytes) => Ok(Some(bytes)),
            Asked::Unavailable => Ok(None),
        }
    }

    fn read<T: serde::de::DeserializeOwned>(&self, op: ProviderOp, bytes: &[u8]) -> Result<T> {
        self.asker.parsed(op, bytes)
    }
}

impl PlaysInPlace for Tidal {
    fn scheme(&self) -> &SourceId {
        &self.asker.source
    }

    fn title(&self) -> &'static str {
        "TIDAL"
    }

    fn artists(&self) -> Result<Vec<Artist>> {
        Ok(Vec::new())
    }

    fn artist(&self, id: &Id) -> Result<ArtistAlbums> {
        self.artist_albums(id)
    }

    fn album(&self, id: &Id) -> Result<AlbumSongs> {
        self.album_songs(id)
    }

    fn search(&self, words: &str) -> Result<Found> {
        self.searched(words)
    }

    fn mix(&self, id: &Id) -> Result<MixSongs> {
        self.mix_songs(id)
    }

    fn streamed(&self, song: &Id) -> Result<Streamed> {
        let track = TrackId(catalog::track_id(
            &self.asker.source,
            ProviderOp::Stream,
            song,
        )?);
        let media = self.media(track)?;
        self.player().streamed(media)
    }

    fn cover(&self, cover: &Id, side: u32) -> Result<Vec<u8>> {
        self.asker.picture(&self.endpoints.images, cover, side)
    }

    fn cover_of(&self, named: &Named) -> Result<Id> {
        self.cover_named(named)
    }

    fn favourites(&self) -> Result<Found> {
        match self.user()? {
            Some(user) => self.favoured_by(user),
            None => Ok(Found::default()),
        }
    }

    fn keeps_favourites(&self) -> bool {
        true
    }

    fn favour(&self, song: &Id, favoured: bool) -> Result<()> {
        let op = ProviderOp::Favourite;
        let track = catalog::track_id(&self.asker.source, op, song)?;
        let user = self.user()?.ok_or_else(|| self.asker.unreadable(op))?;
        let (verb, path) = if favoured {
            (
                Verb::Add(format!("trackIds={track}&onArtifactNotFound=FAIL")),
                format!("users/{user}/favorites/tracks"),
            )
        } else {
            (
                Verb::Remove,
                format!("users/{user}/favorites/tracks/{track}"),
            )
        };
        let api = &self.endpoints.api;
        match self.exchanged(
            op,
            &verb,
            |country| format!("{api}{path}?countryCode={country}"),
            None,
        )? {
            Asked::Answered(_) => Ok(()),
            Asked::Unavailable if !favoured => Ok(()),
            Asked::Unavailable => Err(Error::TurnedAway {
                provider: self.asker.source.clone(),
                op,
                code: NOT_FOUND,
            }),
        }
    }
}

impl Provider for Tidal {
    fn source(&self) -> &SourceId {
        &self.asker.source
    }

    fn find(&self, identity: &Identity) -> Result<Obtained> {
        played::found(self, identity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_want_with_neither_a_link_nor_an_isrc_is_not_asked_about() {
        let tidal = Tidal::at(
            Account {
                client_id: "client".to_owned(),
                client_secret: None,
                refresh_token: "token".to_owned(),
            },
            Endpoints {
                auth: "http://127.0.0.1:9/".to_owned(),
                ..Endpoints::tidal()
            },
        );

        assert!(matches!(
            tidal.find(&Identity::named("Echoes")),
            Ok(Obtained::Nothing)
        ));
    }
}
