use std::{
    thread,
    time::{Duration, Instant},
};

use resonate_core::{Isrc, SourceId};
use resonate_providers::{Delivery, Error, Identity, Obtained, Provider, ProviderOp, Result};
use serde::Deserialize;
use ureq::Agent;

use crate::{
    MediaHosts,
    asker::{Asker, Sent, media_agent, retry_after},
    played::{self, Finds, Playback, Player, TrackId, named_by},
};

const HIFI_API: &str = "hifi-api";
const ASKED_QUALITY: &str = "HI_RES_LOSSLESS";
const LISTED_AT_MOST: usize = 25;
const TRACKS_TRIED_AT_MOST: usize = 5;
const QUEUED_FOR_AT_MOST: Duration = Duration::from_secs(20);
const QUEUE_LOOKED_AT_LEAST_EVERY: Duration = Duration::from_secs(1);
const QUEUE_LOOKED_AT_MOST_EVERY: Duration = Duration::from_secs(5);
const ACCEPTED: u16 = 202;
const UNAUTHORISED: u16 = 401;
const FORBIDDEN: u16 = 403;
const NOT_FOUND: u16 = 404;
const GONE: u16 = 410;

#[derive(Deserialize)]
struct Wrapped<T> {
    data: T,
}

#[derive(Deserialize)]
struct Listed {
    #[serde(default)]
    items: Vec<Listing>,
}

#[derive(Deserialize)]
struct Listing {
    id: u64,
    #[serde(default)]
    isrc: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pending {
    request_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RequestId(String);

impl RequestId {
    fn read(pending: Pending) -> Option<Self> {
        let id = pending.request_id;
        let usable = !id.is_empty()
            && id
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-');
        usable.then_some(Self(id))
    }
}

enum Answered {
    Ready(Vec<u8>),
    Queued(RequestId, Duration),
    Unavailable,
}

pub struct HifiApi {
    asker: Asker,
    server: String,
    hosts: MediaHosts,
    media: Agent,
    patience: Duration,
}

fn source() -> SourceId {
    SourceId::new(HIFI_API).unwrap_or_else(|_| SourceId::local())
}

impl HifiApi {
    pub fn at(server: &str) -> Self {
        Self::fetching_from(server, MediaHosts::tidal())
    }

    pub fn fetching_from(server: &str, hosts: MediaHosts) -> Self {
        Self {
            asker: Asker::new(source()),
            server: server.trim().trim_end_matches('/').to_owned(),
            hosts,
            media: media_agent(),
            patience: QUEUED_FOR_AT_MOST,
        }
    }

    pub fn queued_for_at_most(self, patience: Duration) -> Self {
        Self { patience, ..self }
    }

    fn refusal(&self, op: ProviderOp, status: u16) -> Error {
        let provider = self.asker.source.clone();
        if status == UNAUTHORISED {
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

    fn asked(&self, op: ProviderOp, url: &str) -> Result<Answered> {
        let sent = self.asker.sent(op, || self.asker.agent.get(url).call())?;
        match sent {
            Sent::Answered(response) if response.status().as_u16() == ACCEPTED => {
                let wait = retry_after(&response)
                    .unwrap_or(QUEUE_LOOKED_AT_LEAST_EVERY)
                    .clamp(QUEUE_LOOKED_AT_LEAST_EVERY, QUEUE_LOOKED_AT_MOST_EVERY);
                let pending: Pending = self
                    .asker
                    .parsed(op, &self.asker.read_whole(op, response)?)?;
                let request = RequestId::read(pending).ok_or_else(|| self.asker.unreadable(op))?;
                Ok(Answered::Queued(request, wait))
            }
            Sent::Answered(response) => self.asker.read_whole(op, response).map(Answered::Ready),
            Sent::Refused {
                status: FORBIDDEN | NOT_FOUND,
                ..
            } => Ok(Answered::Unavailable),
            Sent::Refused { status, .. } => Err(self.refusal(op, status)),
        }
    }

    fn queued_at(&self, request: &RequestId) -> String {
        format!("{}/playback/requests/{}", self.server, request.0)
    }

    fn given_up(&self, op: ProviderOp, request: &RequestId) -> Error {
        let url = self.queued_at(request);
        if let Err(error) = self.asker.agent.delete(&url).call() {
            tracing::debug!(%error, "a queued hifi-api request could not be withdrawn");
        }
        Error::StillQueued {
            provider: self.asker.source.clone(),
            op,
        }
    }

    fn waited_out(&self, op: ProviderOp, asked: Answered) -> Result<Option<Vec<u8>>> {
        let until = Instant::now() + self.patience;
        let mut answered = asked;
        loop {
            match answered {
                Answered::Ready(bytes) => return Ok(Some(bytes)),
                Answered::Unavailable => return Ok(None),
                Answered::Queued(request, wait) => {
                    if Instant::now() + wait > until {
                        return Err(self.given_up(op, &request));
                    }
                    thread::sleep(wait);
                    answered = match self.asked(op, &self.queued_at(&request)) {
                        Err(Error::Refused { status: GONE, .. }) => return Ok(None),
                        other => other?,
                    };
                }
            }
        }
    }
}

impl Finds for HifiApi {
    fn tracks_named_by(&self, isrc: &Isrc) -> Result<Vec<TrackId>> {
        let op = ProviderOp::Search;
        let url = format!(
            "{}/search/?i={}&limit={LISTED_AT_MOST}",
            self.server,
            isrc.as_str()
        );
        let Answered::Ready(bytes) = self.asked(op, &url)? else {
            return Ok(Vec::new());
        };
        let listed: Wrapped<Listed> = self.asker.parsed(op, &bytes)?;
        Ok(listed
            .data
            .items
            .into_iter()
            .filter(|listing| named_by(isrc, listing.isrc.as_deref()))
            .map(|listing| TrackId(listing.id))
            .take(TRACKS_TRIED_AT_MOST)
            .collect())
    }

    fn delivered(&self, track: TrackId) -> Result<Option<Delivery>> {
        let op = ProviderOp::Playback;
        let url = format!(
            "{}/track/?id={}&quality={ASKED_QUALITY}",
            self.server, track.0
        );
        let asked = self.asked(op, &url)?;
        let Some(bytes) = self.waited_out(op, asked)? else {
            return Ok(None);
        };
        let playback: Wrapped<Playback> = self.asker.parsed(op, &bytes)?;
        Player {
            source: &self.asker.source,
            hosts: &self.hosts,
            media: &self.media,
        }
        .delivered(track, &playback.data)
    }
}

impl Provider for HifiApi {
    fn source(&self) -> &SourceId {
        &self.asker.source
    }

    fn obtain(&self, identity: &Identity) -> Result<Obtained> {
        played::obtained(self, identity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_queued_request_is_named_only_by_letters_digits_and_dashes() {
        let read = |id: &str| {
            RequestId::read(Pending {
                request_id: id.to_owned(),
            })
        };

        assert_eq!(read("f1b5c0de"), Some(RequestId("f1b5c0de".to_owned())));
        assert_eq!(read(""), None);
        assert_eq!(read("../../admin"), None);
        assert_eq!(read("f1b5?x=1"), None);
    }

    #[test]
    fn the_server_is_named_without_a_trailing_slash() {
        assert_eq!(
            HifiApi::at(" https://hifi.home.arpa/ ").server,
            "https://hifi.home.arpa"
        );
    }
}
