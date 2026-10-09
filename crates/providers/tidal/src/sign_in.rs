use std::{
    thread,
    time::{Duration, Instant},
};

use resonate_core::SourceId;
use resonate_fetch::unreached;
use resonate_providers::{Authorizing, Client, Error, ProviderOp, RefreshToken, Result, SignsIn};
use serde::Deserialize;
use ureq::{Agent, Body, http};

use crate::{Endpoints, asker::api_agent, source};

const SCOPE: &str = "r_usr";
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
const PENDING: &str = "authorization_pending";
const SLOW_DOWN: &str = "slow_down";
const EXPIRED: &str = "expired_token";
const DENIED: &str = "access_denied";
const SLOWED_BY: Duration = Duration::from_secs(5);
const LOOKED_AT_EVERY: Duration = Duration::from_millis(100);
const ASKED_EVERY_AT_LEAST: Duration = Duration::from_secs(1);
const LASTS_WHEN_UNSAID: Duration = Duration::from_secs(300);
const ASKED_EVERY_WHEN_UNSAID: Duration = Duration::from_secs(2);
const LARGEST_ANSWER: u64 = 64 * 1024;
const BAD_REQUEST: u16 = 400;
const UNAUTHORISED: u16 = 401;
const TOO_MANY_REQUESTS: u16 = 429;
const SERVER_ERRORS_FROM: u16 = 500;
const SECURE: &str = "https://";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Device {
    device_code: String,
    user_code: String,
    #[serde(default)]
    verification_uri: Option<String>,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    interval: Option<u64>,
}

#[derive(Deserialize)]
struct Granted {
    refresh_token: String,
}

#[derive(Deserialize)]
struct Refusal {
    #[serde(default)]
    error: Option<String>,
}

pub struct TidalSignIn {
    source: SourceId,
    endpoints: Endpoints,
    agent: Agent,
}

enum Answered {
    Granted(RefreshToken),
    Pending,
    SlowDown,
}

fn with_scheme(at: &str) -> String {
    let at = at.trim();
    if at.contains("://") {
        at.to_owned()
    } else {
        format!("{SECURE}{at}")
    }
}

impl Default for TidalSignIn {
    fn default() -> Self {
        Self::at(Endpoints::tidal())
    }
}

impl TidalSignIn {
    pub fn at(endpoints: Endpoints) -> Self {
        Self {
            source: source(),
            endpoints,
            agent: api_agent(),
        }
    }

    fn unreachable(&self, error: ureq::Error) -> Error {
        tracing::debug!(%error, "TIDAL's sign-in could not be reached");
        unreached(self.source.clone(), ProviderOp::SignIn, error)
    }

    fn unreadable(&self) -> Error {
        Error::Unreadable {
            provider: self.source.clone(),
            op: ProviderOp::SignIn,
        }
    }

    fn read_whole(&self, response: http::Response<Body>) -> Result<Vec<u8>> {
        response
            .into_body()
            .with_config()
            .limit(LARGEST_ANSWER)
            .read_to_vec()
            .map_err(|error| self.unreachable(error))
    }

    fn refused(&self, status: u16) -> Error {
        let provider = self.source.clone();
        let op = ProviderOp::SignIn;
        if matches!(status, BAD_REQUEST | UNAUTHORISED) {
            Error::Unwelcome {
                provider,
                op,
                code: status,
            }
        } else {
            Error::Refused {
                provider,
                op,
                status,
            }
        }
    }

    fn sent(&self, url: &str, form: &[(&str, &str)]) -> Result<(u16, Vec<u8>)> {
        let response = self
            .agent
            .post(url)
            .send_form(form.iter().copied())
            .map_err(|error| self.unreachable(error))?;
        let status = response.status().as_u16();
        Ok((status, self.read_whole(response)?))
    }

    fn asked(&self, client: &Client, authorizing: &Authorizing) -> Result<Answered> {
        let mut form = vec![
            ("client_id", client.id.as_str()),
            ("device_code", authorizing.device_code()),
            ("grant_type", DEVICE_GRANT),
            ("scope", SCOPE),
        ];
        if let Some(secret) = &client.secret {
            form.push(("client_secret", secret.as_str()));
        }
        let (status, bytes) = self.sent(&self.endpoints.auth, &form)?;
        if (200..300).contains(&status) {
            let granted: Granted = serde_json::from_slice(&bytes).map_err(|_| self.unreadable())?;
            return Ok(Answered::Granted(RefreshToken::new(granted.refresh_token)));
        }
        let error = serde_json::from_slice::<Refusal>(&bytes)
            .ok()
            .and_then(|refusal| refusal.error);
        let provider = self.source.clone();
        match error.as_deref() {
            Some(PENDING) => Ok(Answered::Pending),
            Some(SLOW_DOWN) => Ok(Answered::SlowDown),
            Some(EXPIRED) => Err(Error::AuthorizationLapsed { provider }),
            Some(DENIED) => Err(Error::AuthorizationDenied { provider }),
            _ => Err(self.refused(status)),
        }
    }
}

enum Passing {
    SlowDown,
    Again(Error),
    Ended(Error),
}

impl Passing {
    fn of(error: Error) -> Self {
        match error {
            Error::Refused {
                status: TOO_MANY_REQUESTS,
                ..
            } => Self::SlowDown,
            Error::Refused { status, .. } if status >= SERVER_ERRORS_FROM => Self::Again(error),
            Error::Io { .. } => Self::Again(error),
            error => Self::Ended(error),
        }
    }
}

fn waited(every: Duration, until: Instant, cancelled: &(dyn Fn() -> bool + Sync)) -> bool {
    let next = Instant::now().checked_add(every).unwrap_or(until);
    while Instant::now() < next.min(until) {
        if cancelled() {
            return false;
        }
        thread::sleep(LOOKED_AT_EVERY);
    }
    !cancelled()
}

impl SignsIn for TidalSignIn {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn authorizing(&self, client: &Client) -> Result<Authorizing> {
        let (status, bytes) = self.sent(
            &self.endpoints.device,
            &[("client_id", client.id.as_str()), ("scope", SCOPE)],
        )?;
        if !(200..300).contains(&status) {
            return Err(self.refused(status));
        }
        let device: Device = serde_json::from_slice(&bytes).map_err(|_| self.unreadable())?;
        let verify_at = device
            .verification_uri_complete
            .or(device.verification_uri)
            .map(|at| with_scheme(&at))
            .ok_or_else(|| self.unreadable())?;
        Ok(Authorizing::new(
            device.user_code,
            verify_at,
            device
                .expires_in
                .map_or(LASTS_WHEN_UNSAID, Duration::from_secs),
            device
                .interval
                .map_or(ASKED_EVERY_WHEN_UNSAID, Duration::from_secs)
                .max(ASKED_EVERY_AT_LEAST),
            device.device_code,
        ))
    }

    fn authorized(
        &self,
        client: &Client,
        authorizing: &Authorizing,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Option<RefreshToken>> {
        let until = Instant::now()
            .checked_add(authorizing.lasts)
            .ok_or_else(|| self.unreadable())?;
        let mut every = authorizing.asked_every;
        loop {
            if !waited(every, until, cancelled) {
                return Ok(None);
            }
            if Instant::now() >= until {
                return Err(Error::AuthorizationLapsed {
                    provider: self.source.clone(),
                });
            }
            match self.asked(client, authorizing).map_err(Passing::of) {
                Ok(Answered::Granted(token)) => return Ok(Some(token)),
                Ok(Answered::Pending) => {}
                Ok(Answered::SlowDown) | Err(Passing::SlowDown) => every += SLOWED_BY,
                Err(Passing::Again(error)) => {
                    tracing::debug!(%error, "TIDAL's sign-in did not answer this time; asking again");
                }
                Err(Passing::Ended(error)) => return Err(error),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verification_page_named_without_its_scheme_is_reached_over_https() {
        assert_eq!(
            with_scheme("link.tidal.com/ABCDE"),
            "https://link.tidal.com/ABCDE"
        );
        assert_eq!(
            with_scheme("https://link.tidal.com/ABCDE"),
            "https://link.tidal.com/ABCDE"
        );
    }
}
