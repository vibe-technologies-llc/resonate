use std::{
    thread,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use resonate_core::SourceId;
use resonate_providers::{Error, ProviderOp, Result};
use serde::Deserialize;
use ureq::{
    Agent, Body,
    http::{self, header::RETRY_AFTER},
};

use crate::fetched::as_io;

const ASKED_APART: Duration = Duration::from_millis(250);
const RETRIES_AT_MOST: u32 = 3;
const FIRST_RETRY_AFTER: Duration = Duration::from_secs(1);
const LONGEST_RETRY_AFTER: Duration = Duration::from_secs(8);
const ANSWERED_WITHIN: Duration = Duration::from_secs(20);
const CONNECTED_WITHIN: Duration = Duration::from_secs(10);
const MEDIA_ANSWERED_WITHIN: Duration = Duration::from_secs(30);
const MEDIA_READ_WITHIN: Duration = Duration::from_secs(120);
const LARGEST_ANSWER: u64 = 4 * 1024 * 1024;
const TOO_MANY_REQUESTS: u16 = 429;
const UNAVAILABLE: u16 = 503;

#[derive(Deserialize)]
struct Complaint {
    #[serde(default, rename = "subStatus")]
    sub_status: Option<u32>,
}

pub(crate) enum Sent {
    Answered(http::Response<Body>),
    Refused { status: u16, complaint: Option<u16> },
}

pub(crate) struct Asker {
    pub(crate) source: SourceId,
    pub(crate) agent: Agent,
    next_asked: Mutex<Instant>,
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

pub(crate) fn media_agent() -> Agent {
    Agent::config_builder()
        .user_agent(concat!("resonate/", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Some(CONNECTED_WITHIN))
        .timeout_recv_response(Some(MEDIA_ANSWERED_WITHIN))
        .timeout_recv_body(Some(MEDIA_READ_WITHIN))
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .new_agent()
}

pub(crate) fn retry_after(response: &http::Response<Body>) -> Option<Duration> {
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

impl Asker {
    pub(crate) fn new(source: SourceId) -> Self {
        Self {
            source,
            agent: api_agent(),
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

    pub(crate) fn unreachable(&self, op: ProviderOp, error: ureq::Error) -> Error {
        tracing::debug!(%error, ?op, provider = %self.source, "a provider's server could not be reached");
        Error::Io {
            provider: self.source.clone(),
            op,
            source: as_io(error),
        }
    }

    pub(crate) fn unreadable(&self, op: ProviderOp) -> Error {
        Error::Unreadable {
            provider: self.source.clone(),
            op,
        }
    }

    pub(crate) fn sent(
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
                tracing::debug!(status, ?wait, ?op, provider = %self.source, "a provider's server asked to be asked later");
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

    pub(crate) fn read_whole(
        &self,
        op: ProviderOp,
        response: http::Response<Body>,
    ) -> Result<Vec<u8>> {
        response
            .into_body()
            .with_config()
            .limit(LARGEST_ANSWER)
            .read_to_vec()
            .map_err(|error| self.unreachable(op, error))
    }

    pub(crate) fn parsed<T: for<'de> Deserialize<'de>>(
        &self,
        op: ProviderOp,
        bytes: &[u8],
    ) -> Result<T> {
        serde_json::from_slice(bytes).map_err(|error| {
            tracing::debug!(%error, ?op, provider = %self.source, "a provider's answer was not the document expected");
            self.unreadable(op)
        })
    }
}
