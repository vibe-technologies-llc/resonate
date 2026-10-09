mod account;
mod asker;
mod fetched;
mod hifi;
mod manifest;
mod played;
mod remux;
mod sign_in;

use std::time::{Duration, Instant};

use parking_lot::Mutex;
use resonate_core::{Isrc, SourceId};
use resonate_providers::{
    Delivery, Error, Identity, Obtained, Provider, ProviderOp, RefreshToken, Result,
};
use serde::Deserialize;
use ureq::{
    Agent,
    http::header::{ACCEPT, AUTHORIZATION},
};

pub use crate::{
    account::{Account, Endpoints, MediaHosts},
    hifi::HifiApi,
    sign_in::TidalSignIn,
};
use crate::{
    asker::{Asker, Sent, media_agent},
    played::{Finds, Playback, Player, TrackId, named_by},
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
const TRACKS_TRIED_AT_MOST: usize = 5;

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
    refresh_token: Option<String>,
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
        let country = match granted.user.and_then(|user| user.country) {
            Some(country) => country,
            None => self.country_of(&bearer)?,
        };
        Ok(Session {
            bearer,
            country,
            until: Instant::now()
                .checked_add(lasts)
                .ok_or_else(|| self.asker.unreadable(op))?,
        })
    }

    fn country_of(&self, bearer: &str) -> Result<String> {
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
                let sessions: Sessions = self
                    .asker
                    .parsed(op, &self.asker.read_whole(op, response)?)?;
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
            let sent = self.asker.sent(op, || {
                let request = self
                    .asker
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
    fn tracks_named_by(
        &self,
        isrc: &Isrc,
        _title: &str,
        _artist: Option<&str>,
    ) -> Result<Vec<TrackId>> {
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

    fn delivered(&self, track: TrackId) -> Result<Option<Delivery>> {
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
        Player {
            source: &self.asker.source,
            hosts: &self.endpoints.media,
            media: &self.media,
        }
        .delivered(track, &playback)
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
