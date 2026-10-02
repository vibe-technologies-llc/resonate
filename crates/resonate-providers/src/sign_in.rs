use std::{fmt, time::Duration};

use resonate_core::SourceId;

use crate::Result;

struct Withheld;

impl fmt::Debug for Withheld {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<withheld>")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Client {
    pub id: String,
    pub secret: Option<String>,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("id", &self.id)
            .field("secret", &self.secret.as_ref().map(|_| Withheld))
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Authorizing {
    pub user_code: String,
    pub verify_at: String,
    pub lasts: Duration,
    pub asked_every: Duration,
    device_code: String,
}

impl Authorizing {
    pub fn new(
        user_code: String,
        verify_at: String,
        lasts: Duration,
        asked_every: Duration,
        device_code: String,
    ) -> Self {
        Self {
            user_code,
            verify_at,
            lasts,
            asked_every,
            device_code,
        }
    }

    pub fn device_code(&self) -> &str {
        &self.device_code
    }
}

impl fmt::Debug for Authorizing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Authorizing")
            .field("user_code", &self.user_code)
            .field("verify_at", &self.verify_at)
            .field("lasts", &self.lasts)
            .field("asked_every", &self.asked_every)
            .field("device_code", &Withheld)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RefreshToken(String);

impl RefreshToken {
    pub fn new(token: String) -> Self {
        Self(token)
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Debug for RefreshToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RefreshToken").field(&Withheld).finish()
    }
}

pub trait SignsIn: Send + Sync {
    fn source(&self) -> &SourceId;

    fn authorizing(&self, client: &Client) -> Result<Authorizing>;

    fn authorized(
        &self,
        client: &Client,
        authorizing: &Authorizing,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Option<RefreshToken>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_signing_in_prints_a_secret() {
        let client = Client {
            id: "client".to_owned(),
            secret: Some("hush".to_owned()),
        };
        let authorizing = Authorizing::new(
            "ABCDE".to_owned(),
            "https://link.tidal.com/ABCDE".to_owned(),
            Duration::from_secs(300),
            Duration::from_secs(2),
            "device-sesame".to_owned(),
        );
        let token = RefreshToken::new("refresh-sesame".to_owned());

        let printed = format!("{client:?} {authorizing:?} {token:?}");

        assert!(printed.contains("ABCDE"));
        assert!(!printed.contains("hush"));
        assert!(!printed.contains("sesame"));
    }
}
