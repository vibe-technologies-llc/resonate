use std::{
    fmt, thread,
    time::{Duration, Instant},
};

use resonate_fetch::{Patience, escaped};
use resonate_providers::{Error, ProviderOp, Result};

use crate::{
    Call, FORBIDDEN, Flavour, Jellyfin, KeptSession, NOT_FOUND, Server, UNAUTHORIZED, Withheld,
    flavour_of,
    served::{Authenticated, Initiated, Polled},
};

pub const QUICK_CONNECT_ASKED_EVERY: Duration = Duration::from_secs(5);

pub const A_QUICK_CONNECT_LAPSES_AFTER: Duration = Duration::from_secs(10 * 60);

const LOOKED_AT_EVERY: Duration = Duration::from_millis(100);

const A_POLL_ANSWERED_WITHIN: Duration = Duration::from_secs(3);

const METHOD_NOT_ALLOWED: u16 = 405;

const FIRST_SERVER_ERROR: u16 = 500;

const ENABLED: &str = "true";

const ASKED_WITH_NOTHING: &str = "{}";

#[derive(Clone)]
pub struct QuickConnect {
    jellyfin: Jellyfin,
    asked_every: Duration,
    lapses_after: Duration,
}

pub struct Approving {
    pub code: String,
    secret: String,
    device: String,
    began: Instant,
}

impl fmt::Debug for Approving {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Approving")
            .field("code", &self.code)
            .field("secret", &Withheld)
            .field("device", &self.device)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedIn {
    pub session: KeptSession,
    pub name: String,
}

impl QuickConnect {
    pub fn at(address: &str) -> Self {
        Self {
            jellyfin: Jellyfin::at(Server {
                url: address.to_owned(),
                user: String::new(),
                password: String::new(),
                kept: None,
            }),
            asked_every: QUICK_CONNECT_ASKED_EVERY,
            lapses_after: A_QUICK_CONNECT_LAPSES_AFTER,
        }
    }

    #[must_use]
    pub fn asked_every(self, every: Duration) -> Self {
        Self {
            asked_every: every,
            ..self
        }
    }

    #[must_use]
    pub fn lapsing_after(self, after: Duration) -> Self {
        Self {
            lapses_after: after,
            ..self
        }
    }

    #[must_use]
    pub fn waiting(self, patience: Patience) -> Self {
        Self {
            jellyfin: self.jellyfin.waiting(patience),
            ..self
        }
    }

    fn root(&self) -> String {
        self.jellyfin.roots().0
    }

    pub fn offered(&self) -> Result<bool> {
        let root = self.root();
        if flavour_of(&root) == Flavour::Emby {
            return Ok(false);
        }
        let op = ProviderOp::SignIn;
        let url = format!("{root}/QuickConnect/Enabled");
        let response =
            match self
                .jellyfin
                .called_within(&self.jellyfin.asking, op, &Call::Get, &url, None)
            {
                Ok(response) => response,
                Err(Error::Refused { status, .. }) if status < FIRST_SERVER_ERROR => {
                    return Ok(false);
                }
                Err(error) => return Err(error),
            };
        let bytes = self.jellyfin.read_whole(op, response)?;
        Ok(String::from_utf8_lossy(&bytes).trim() == ENABLED)
    }

    pub fn begun(&self) -> Result<Approving> {
        let op = ProviderOp::SignIn;
        let url = format!("{}/QuickConnect/Initiate", self.root());
        let agent = &self.jellyfin.asking;
        let response = match self.jellyfin.called_within(
            agent,
            op,
            &Call::SigningIn(ASKED_WITH_NOTHING),
            &url,
            None,
        ) {
            Err(Error::Refused {
                status: METHOD_NOT_ALLOWED,
                ..
            }) => self
                .jellyfin
                .called_within(agent, op, &Call::Introducing, &url, None),
            answered => answered,
        }
        .map_err(|error| self.denied_where_refused(error))?;
        let bytes = self.jellyfin.read_whole(op, response)?;
        let initiated: Initiated = self.jellyfin.document(&bytes, op)?;
        Ok(Approving {
            code: initiated.code,
            secret: initiated.secret,
            device: self.jellyfin.device.to_string(),
            began: Instant::now(),
        })
    }

    pub fn approved(
        &self,
        approving: &Approving,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Option<SignedIn>> {
        let asking = self.jellyfin.clone().issued_to(&approving.device);
        let until = approving.began + self.lapses_after;
        loop {
            if cancelled() {
                return Ok(None);
            }
            if Instant::now() >= until {
                return Err(self.lapsed());
            }
            match Self::polled(&asking, approving) {
                Ok(true) if cancelled() => return Ok(None),
                Ok(true) => {
                    let signed = Self::signed_in(&asking, approving)?;
                    return Ok((!cancelled()).then_some(signed));
                }
                Ok(false) => {}
                Err(Error::Refused {
                    status: NOT_FOUND, ..
                }) => return Err(self.lapsed()),
                Err(error) if error.is_the_provider_away() => {
                    tracing::debug!(%error, "the Jellyfin server did not answer the poll; asking again");
                }
                Err(error) => return Err(error),
            }
            if !waited(self.asked_every, until, cancelled) {
                return Ok(None);
            }
        }
    }

    fn polled(asking: &Jellyfin, approving: &Approving) -> Result<bool> {
        let op = ProviderOp::SignIn;
        let url = format!(
            "{}/QuickConnect/Connect?secret={}",
            asking.roots().0,
            escaped(&approving.secret)
        );
        let deadline = Instant::now() + A_POLL_ANSWERED_WITHIN;
        let response =
            asking.called_within(&asking.asking, op, &Call::Get, &url, Some(deadline))?;
        let bytes = asking.read_whole(op, response)?;
        let polled: Polled = asking.document(&bytes, op)?;
        Ok(polled.authenticated)
    }

    fn signed_in(asking: &Jellyfin, approving: &Approving) -> Result<SignedIn> {
        let op = ProviderOp::SignIn;
        let url = format!("{}/Users/AuthenticateWithQuickConnect", asking.roots().0);
        let body = serde_json::json!({ "Secret": approving.secret }).to_string();
        let response =
            asking.called_within(&asking.asking, op, &Call::SigningIn(&body), &url, None)?;
        let bytes = asking.read_whole(op, response)?;
        let signed: Authenticated = asking.document(&bytes, op)?;
        Ok(SignedIn {
            session: KeptSession {
                token: signed.token,
                user: signed.user.id,
                device: approving.device.clone(),
            },
            name: signed.user.name,
        })
    }

    fn lapsed(&self) -> Error {
        Error::AuthorizationLapsed {
            provider: self.jellyfin.source.clone(),
        }
    }

    fn denied_where_refused(&self, error: Error) -> Error {
        match error {
            Error::Refused {
                status: UNAUTHORIZED | FORBIDDEN,
                ..
            } => Error::AuthorizationDenied {
                provider: self.jellyfin.source.clone(),
            },
            error => error,
        }
    }
}

fn waited(every: Duration, until: Instant, cancelled: &dyn Fn() -> bool) -> bool {
    let end = (Instant::now() + every).min(until);
    loop {
        if cancelled() {
            return false;
        }
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return true;
        }
        thread::sleep(LOOKED_AT_EVERY.min(left));
    }
}
