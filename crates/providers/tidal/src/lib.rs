mod account;
mod fetched;
mod manifest;
mod remux;
mod sign_in;

use std::{
    io::Read,
    thread,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use resonate_core::{Isrc, Service, SourceId};
use resonate_providers::{
    Delivery, Error, Extension, Identity, Obtained, Provider, ProviderOp, Result,
};
use serde::Deserialize;
use ureq::{
    Agent, Body,
    http::{
        self,
        header::{ACCEPT, AUTHORIZATION, RETRY_AFTER},
    },
};

pub use crate::{
    account::{Account, Endpoints, MediaHosts},
    sign_in::TidalSignIn,
};
use crate::{
    fetched::{Fetched, Unfetched, as_io},
    manifest::{Container, Manifest, Media},
    remux::{Remuxed, Unremuxable},
};

const TIDAL: &str = "tidal";
const ASKED_APART: Duration = Duration::from_millis(250);
const RETRIES_AT_MOST: u32 = 3;
const FIRST_RETRY_AFTER: Duration = Duration::from_secs(1);
const LONGEST_RETRY_AFTER: Duration = Duration::from_secs(8);
const ANSWERED_WITHIN: Duration = Duration::from_secs(20);
const CONNECTED_WITHIN: Duration = Duration::from_secs(10);
const MEDIA_ANSWERED_WITHIN: Duration = Duration::from_secs(30);
const MEDIA_READ_WITHIN: Duration = Duration::from_secs(120);
const LARGEST_ANSWER: u64 = 4 * 1024 * 1024;
const RENEWED_BEFORE: Duration = Duration::from_secs(60);
const LASTS_WHEN_UNSAID: Duration = Duration::from_secs(3600);
const TOO_MANY_REQUESTS: u16 = 429;
const UNAVAILABLE: u16 = 503;
const BAD_REQUEST: u16 = 400;
const UNAUTHORISED: u16 = 401;
const FORBIDDEN: u16 = 403;
const NOT_FOUND: u16 = 404;
const NOT_READY_FOR_PLAYBACK: u16 = 4005;
const ASKED_QUALITY: &str = "HI_RES_LOSSLESS";
const FULL: &str = "FULL";
const JSON_API: &str = "application/vnd.api+json";
const TRACKS_TRIED_AT_MOST: usize = 5;
const DELIVERED_AS: &str = "flac";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TrackId(u64);

impl TrackId {
    fn linked(url: &str) -> Option<Self> {
        let path = url.split(['?', '#']).next()?;
        let mut segments = path.split('/');
        segments.by_ref().find(|segment| *segment == "track")?;
        segments.next()?.parse().ok().map(Self)
    }
}

#[derive(Clone)]
struct Session {
    bearer: String,
    country: String,
    until: Instant,
}

#[derive(Deserialize)]
struct Granted {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    user: Option<Holder>,
}

#[derive(Deserialize)]
struct Holder {
    #[serde(default, rename = "countryCode")]
    country: Option<String>,
}

#[derive(Deserialize)]
struct Sessions {
    #[serde(rename = "countryCode")]
    country: String,
}

#[derive(Deserialize)]
struct Complaint {
    #[serde(default, rename = "subStatus")]
    sub_status: Option<u32>,
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Playback {
    asset_presentation: String,
    manifest_mime_type: String,
    manifest: String,
}

enum Sent {
    Answered(http::Response<Body>),
    Refused { status: u16, complaint: Option<u16> },
}

enum Asked {
    Answered(Vec<u8>),
    Unavailable,
}

pub struct Tidal {
    source: SourceId,
    account: Account,
    endpoints: Endpoints,
    agent: Agent,
    media: Agent,
    session: Mutex<Option<Session>>,
    next_asked: Mutex<Instant>,
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

pub(crate) fn api_agent() -> Agent {
    Agent::config_builder()
        .user_agent(concat!("resonate/", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Some(CONNECTED_WITHIN))
        .timeout_recv_response(Some(ANSWERED_WITHIN))
        .http_status_as_error(false)
        .build()
        .new_agent()
}

pub(crate) fn source() -> SourceId {
    SourceId::new(TIDAL).unwrap_or_else(|_| SourceId::local())
}

fn named_by(isrc: &Isrc, held: Option<&str>) -> bool {
    held.is_some_and(|held| Isrc::new(held.trim()).is_ok_and(|held| held == *isrc))
}

impl Tidal {
    pub fn signed_in(account: Account) -> Self {
        Self::at(account, Endpoints::tidal())
    }

    pub fn at(account: Account, endpoints: Endpoints) -> Self {
        let agent = api_agent();
        let media = Agent::config_builder()
            .user_agent(concat!("resonate/", env!("CARGO_PKG_VERSION")))
            .timeout_connect(Some(CONNECTED_WITHIN))
            .timeout_recv_response(Some(MEDIA_ANSWERED_WITHIN))
            .timeout_recv_body(Some(MEDIA_READ_WITHIN))
            .http_status_as_error(false)
            .max_redirects(0)
            .build()
            .new_agent();
        Self {
            source: source(),
            account,
            endpoints,
            agent,
            media,
            session: Mutex::new(None),
            next_asked: Mutex::new(Instant::now()),
        }
    }

    fn paced(&self) {
        let mut next = self.next_asked.lock();
        let wait = next.saturating_duration_since(Instant::now());
        if !wait.is_zero() {
            thread::sleep(wait);
        }
        *next = Instant::now() + ASKED_APART;
    }

    fn unreachable(&self, op: ProviderOp, error: ureq::Error) -> Error {
        tracing::debug!(%error, ?op, "Tidal could not be reached");
        Error::Io {
            provider: self.source.clone(),
            op,
            source: as_io(error),
        }
    }

    fn unreadable(&self, op: ProviderOp) -> Error {
        Error::Unreadable {
            provider: self.source.clone(),
            op,
        }
    }

    fn refusal(&self, op: ProviderOp, status: u16, complaint: Option<u16>) -> Error {
        let provider = self.source.clone();
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

    fn sent(
        &self,
        op: ProviderOp,
        send: impl Fn() -> std::result::Result<http::Response<Body>, ureq::Error>,
    ) -> Result<Sent> {
        let mut retried = 0;
        loop {
            self.paced();
            let response = send().map_err(|error| self.unreachable(op, error))?;
            if response.status().is_success() {
                return Ok(Sent::Answered(response));
            }
            let status = response.status().as_u16();
            if retried < RETRIES_AT_MOST && matches!(status, TOO_MANY_REQUESTS | UNAVAILABLE) {
                let wait = retry_after(&response)
                    .unwrap_or(FIRST_RETRY_AFTER * (1 << retried))
                    .min(LONGEST_RETRY_AFTER);
                tracing::debug!(status, ?wait, ?op, "Tidal asked to be asked later");
                thread::sleep(wait);
                retried += 1;
                continue;
            }
            let complaint = self
                .read_whole(op, response)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Complaint>(&bytes).ok())
                .and_then(|complaint| complaint.sub_status)
                .and_then(|code| u16::try_from(code).ok());
            return Ok(Sent::Refused { status, complaint });
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

    fn parsed<T: for<'de> Deserialize<'de>>(&self, op: ProviderOp, bytes: &[u8]) -> Result<T> {
        serde_json::from_slice(bytes).map_err(|error| {
            tracing::debug!(%error, ?op, "a Tidal answer was not the document expected");
            self.unreadable(op)
        })
    }

    fn sign_in(&self) -> Result<Session> {
        let op = ProviderOp::SignIn;
        let mut form = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", self.account.refresh_token.as_str()),
            ("client_id", self.account.client_id.as_str()),
        ];
        if let Some(secret) = &self.account.client_secret {
            form.push(("client_secret", secret.as_str()));
        }
        let response = match self.sent(op, || {
            self.agent
                .post(&self.endpoints.auth)
                .send_form(form.iter().copied())
        })? {
            Sent::Answered(response) => response,
            Sent::Refused { status, complaint } if matches!(status, BAD_REQUEST | UNAUTHORISED) => {
                return Err(Error::Unwelcome {
                    provider: self.source.clone(),
                    op,
                    code: complaint.unwrap_or(status),
                });
            }
            Sent::Refused { status, complaint } => {
                return Err(self.refusal(op, status, complaint));
            }
        };
        let granted: Granted = self.parsed(op, &self.read_whole(op, response)?)?;
        let lasts = granted
            .expires_in
            .map_or(LASTS_WHEN_UNSAID, Duration::from_secs)
            .saturating_sub(RENEWED_BEFORE);
        let bearer = format!("Bearer {}", granted.access_token);
        let country = match granted.user.and_then(|user| user.country) {
            Some(country) => country,
            None => self.country_of(&bearer)?,
        };
        Ok(Session {
            bearer,
            country,
            until: Instant::now() + lasts,
        })
    }

    fn country_of(&self, bearer: &str) -> Result<String> {
        let op = ProviderOp::SignIn;
        let url = format!("{}sessions", self.endpoints.api);
        match self.sent(op, || {
            self.agent.get(&url).header(AUTHORIZATION, bearer).call()
        })? {
            Sent::Answered(response) => {
                let sessions: Sessions = self.parsed(op, &self.read_whole(op, response)?)?;
                Ok(sessions.country)
            }
            Sent::Refused { status, complaint } => Err(self.refusal(op, status, complaint)),
        }
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
        let mut renewed = false;
        loop {
            let session = self.session()?;
            let url = url(&session.country);
            let sent = self.sent(op, || {
                let request = self
                    .agent
                    .get(&url)
                    .header(AUTHORIZATION, session.bearer.as_str());
                match accept {
                    Some(accept) => request.header(ACCEPT, accept).call(),
                    None => request.call(),
                }
            })?;
            match sent {
                Sent::Answered(response) => {
                    return self.read_whole(op, response).map(Asked::Answered);
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

    fn tracks_named_by(&self, isrc: &Isrc) -> Result<Vec<TrackId>> {
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
        let listed: Listed = self.parsed(op, &bytes)?;
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

    fn media_of(&self, track: TrackId) -> Result<Option<Media>> {
        let op = ProviderOp::Playback;
        let asked = self.asked(
            op,
            |country| {
                format!(
                    "{}tracks/{}/playbackinfopostpaywall?countryCode={country}&audioquality={ASKED_QUALITY}&playbackmode=STREAM&assetpresentation={FULL}",
                    self.endpoints.api, track.0
                )
            },
            None,
        )?;
        let Asked::Answered(bytes) = asked else {
            return Ok(None);
        };
        let playback: Playback = self.parsed(op, &bytes)?;
        if !playback.asset_presentation.eq_ignore_ascii_case(FULL) {
            tracing::debug!(track = track.0, presentation = %playback.asset_presentation, "Tidal offered less than the whole track");
            return Ok(None);
        }
        let media = match manifest::read(&playback.manifest_mime_type, &playback.manifest) {
            Ok(Manifest::Media(media)) => media,
            Ok(Manifest::Withheld(withheld)) => {
                tracing::debug!(
                    track = track.0,
                    ?withheld,
                    "Tidal's stream is not one this provider takes"
                );
                return Ok(None);
            }
            Err(unread) => {
                tracing::debug!(track = track.0, ?unread, mime = %playback.manifest_mime_type, "a Tidal manifest could not be read");
                return Err(self.unreadable(op));
            }
        };
        if let Some(elsewhere) = media
            .urls
            .iter()
            .find(|url| !self.endpoints.media.holds(url))
        {
            tracing::warn!(track = track.0, host = %elsewhere.split('/').nth(2).unwrap_or_default(), "a Tidal manifest named media off Tidal's audio hosts");
            return Err(Error::OffItsHosts {
                provider: self.source.clone(),
                op,
            });
        }
        Ok(Some(media))
    }

    fn downloaded(&self, media: Media) -> Result<Box<dyn Read + Send>> {
        let op = ProviderOp::Download;
        let fetched =
            Fetched::opened(self.media.clone(), media.urls).map_err(
                |unfetched| match unfetched {
                    Unfetched::Io(source) => Error::Io {
                        provider: self.source.clone(),
                        op,
                        source,
                    },
                    Unfetched::Refused(status) => Error::Refused {
                        provider: self.source.clone(),
                        op,
                        status,
                    },
                },
            )?;
        match media.container {
            Container::Flac => Ok(Box::new(fetched)),
            Container::Mp4 => match Remuxed::opened(fetched, media.timeline) {
                Ok(remuxed) => Ok(Box::new(remuxed)),
                Err(Unremuxable::Io(source)) => Err(Error::Io {
                    provider: self.source.clone(),
                    op,
                    source,
                }),
                Err(unremuxable) => {
                    tracing::debug!(?unremuxable, "a Tidal stream held no FLAC track to take");
                    Err(self.unreadable(op))
                }
            },
        }
    }

    fn delivered(&self, track: TrackId) -> Result<Option<Delivery>> {
        let Some(media) = self.media_of(track)? else {
            return Ok(None);
        };
        Ok(Some(Delivery::Stream {
            key: format!("track/{}", track.0).into_boxed_str(),
            extension: Extension::new(DELIVERED_AS)?,
            reader: self.downloaded(media)?,
        }))
    }
}

impl Provider for Tidal {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn obtain(&self, identity: &Identity) -> Result<Obtained> {
        let linked = identity.track_on(Service::Tidal).and_then(TrackId::linked);
        if linked.is_none() && identity.isrc.is_none() {
            return Ok(Obtained::Nothing);
        }
        if let Some(track) = linked
            && let Some(delivery) = self.delivered(track)?
        {
            return Ok(Obtained::Found(delivery));
        }
        let Some(isrc) = &identity.isrc else {
            return Ok(Obtained::Nothing);
        };
        for track in self
            .tracks_named_by(isrc)?
            .into_iter()
            .filter(|track| Some(*track) != linked)
        {
            if let Some(delivery) = self.delivered(track)? {
                return Ok(Obtained::Found(delivery));
            }
        }
        Ok(Obtained::Nothing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tidal_link_names_its_track() {
        for (link, track) in [
            ("https://tidal.com/track/55391743", 55_391_743),
            ("https://tidal.com/browse/track/55391743?u", 55_391_743),
            ("https://listen.tidal.com/track/12/", 12),
        ] {
            assert_eq!(TrackId::linked(link), Some(TrackId(track)), "{link}");
        }
        assert_eq!(TrackId::linked("https://tidal.com/album/55391740"), None);
        assert_eq!(TrackId::linked("https://tidal.com/track/echoes"), None);
    }

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
            tidal.obtain(&Identity::named("Echoes")),
            Ok(Obtained::Nothing)
        ));
    }
}
