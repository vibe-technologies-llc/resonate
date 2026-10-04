use std::{
    thread,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use resonate_core::{Isrc, SourceId};
use resonate_providers::{Delivery, Error, Identity, Obtained, Provider, ProviderOp, Result};
use serde::Deserialize;
use ureq::{Agent, http::header::AUTHORIZATION};

use crate::{
    MediaHosts,
    asker::{Asker, Sent, media_agent, retry_after},
    played::{self, Finds, Playback, Player, TrackId, named_by},
};

const HIFI_API: &str = "hifi-api";
const HOSTED_SERVER: &str = "https://hifi.odskyler.com";
const HIFI_TOKEN: &str = "https://hifi.odskyler.com/token";
const TIDAL_TOKEN: &str = "https://auth.odskyler.workers.dev/token";
const TIDAL_API: &str = "https://api.tidal.com/v1/";
const TIDAL_ORIGIN: &str = "https://tidal.odskyler.com";
const TIDAL_REFERER: &str = "https://tidal.odskyler.com/";
const MANIFEST_DOMAIN: &str = "manifest.tidal.com";
const ASKED_QUALITY: &str = "HI_RES_LOSSLESS";
const LISTED_AT_MOST: usize = 25;
const TRACKS_TRIED_AT_MOST: usize = 5;
const TOKEN_LASTS_WHEN_UNSAID: Duration = Duration::from_secs(300);
const RENEWED_BEFORE: Duration = Duration::from_secs(30);
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
struct WebListed {
    #[serde(default)]
    items: Vec<Listing>,
}

#[derive(Deserialize)]
struct Granted {
    access_token: String,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Clone)]
struct AccessToken {
    bearer: String,
    until: Instant,
}

#[derive(Default)]
struct Tokens {
    tidal: Option<AccessToken>,
    hifi: Option<AccessToken>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenService {
    Tidal,
    Hifi,
}

#[derive(Deserialize)]
struct HostedManifestResponse {
    #[serde(default)]
    data: Option<HostedManifestData>,
}

#[derive(Deserialize)]
struct HostedManifestData {
    #[serde(default)]
    attributes: Option<HostedManifestAttributes>,
    #[serde(default)]
    data: Option<HostedManifestNested>,
}

#[derive(Deserialize)]
struct HostedManifestNested {
    #[serde(default)]
    attributes: Option<HostedManifestAttributes>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostedManifestAttributes {
    #[serde(default)]
    uri: Option<String>,
    #[serde(default)]
    track_presentation: String,
}

impl HostedManifestResponse {
    fn attributes(self) -> Option<HostedManifestAttributes> {
        let data = self.data?;
        data.attributes
            .or_else(|| data.data.and_then(|nested| nested.attributes))
    }
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
    hosted: bool,
    tokens: Mutex<Tokens>,
}

fn source() -> SourceId {
    SourceId::new(HIFI_API).unwrap_or_else(|_| SourceId::local())
}

fn encoded_query(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    let digits = b"0123456789ABCDEF";
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(digits[usize::from(byte >> 4)]));
            encoded.push(char::from(digits[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

fn title_artist_query(title: &str, artist: Option<&str>) -> Option<String> {
    let title = title.trim();
    let artist = artist.map(str::trim).filter(|artist| !artist.is_empty());
    match (title.is_empty(), artist) {
        (true, None) => None,
        (true, Some(artist)) => Some(artist.to_owned()),
        (false, None) => Some(title.to_owned()),
        (false, Some(artist)) => Some(format!("{title} {artist}")),
    }
}

fn manifest_hosts() -> MediaHosts {
    MediaHosts {
        scheme: "https".to_owned(),
        domain: MANIFEST_DOMAIN.to_owned(),
    }
}

impl HifiApi {
    pub fn at(server: &str) -> Self {
        Self::building(server, MediaHosts::tidal(), false)
    }

    pub fn hosted() -> Self {
        Self::building(HOSTED_SERVER, MediaHosts::tidal(), true)
    }

    pub fn fetching_from(server: &str, hosts: MediaHosts) -> Self {
        Self::building(server, hosts, false)
    }

    fn building(server: &str, hosts: MediaHosts, hosted: bool) -> Self {
        let asker = if hosted {
            Asker::new(source())
        } else {
            Asker::of_the_listeners_server(source())
        };
        Self {
            asker,
            server: server.trim().trim_end_matches('/').to_owned(),
            hosts,
            media: media_agent(),
            patience: QUEUED_FOR_AT_MOST,
            hosted,
            tokens: Mutex::new(Tokens::default()),
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

    fn token(&self, op: ProviderOp, endpoint: &str, service: TokenService) -> Result<String> {
        let now = Instant::now();
        let cached = {
            let tokens = self.tokens.lock();
            match service {
                TokenService::Tidal => tokens.tidal.as_ref(),
                TokenService::Hifi => tokens.hifi.as_ref(),
            }
            .filter(|token| token.until > now)
            .map(|token| token.bearer.clone())
        };
        if let Some(bearer) = cached {
            return Ok(bearer);
        }

        let sent = self.asker.sent(op, || {
            let mut request = self.asker.agent.get(endpoint);
            if service == TokenService::Tidal {
                request = request
                    .header("Origin", TIDAL_ORIGIN)
                    .header("Referer", TIDAL_REFERER);
            }
            request.call()
        })?;
        let response = match sent {
            Sent::Answered(response) => response,
            Sent::Refused { status, .. } => return Err(self.refusal(op, status)),
        };
        let granted: Granted = self
            .asker
            .parsed(op, &self.asker.read_whole(op, response)?)?;
        let lasts = granted
            .expires_in
            .map_or(TOKEN_LASTS_WHEN_UNSAID, Duration::from_secs)
            .saturating_sub(RENEWED_BEFORE);
        let token = AccessToken {
            bearer: format!("Bearer {}", granted.access_token),
            until: Instant::now() + lasts,
        };
        let bearer = token.bearer.clone();
        let mut tokens = self.tokens.lock();
        match service {
            TokenService::Tidal => tokens.tidal = Some(token),
            TokenService::Hifi => tokens.hifi = Some(token),
        }
        Ok(bearer)
    }

    fn hosted_tracks_named_by(
        &self,
        isrc: &Isrc,
        title: &str,
        artist: Option<&str>,
    ) -> Result<Vec<TrackId>> {
        let op = ProviderOp::Search;
        let Some(query) = title_artist_query(title, artist) else {
            return Ok(Vec::new());
        };
        let bearer = self.token(op, TIDAL_TOKEN, TokenService::Tidal)?;
        let url = format!(
            "{TIDAL_API}search/tracks?query={}&limit={LISTED_AT_MOST}&offset=0&countryCode=US",
            encoded_query(&query)
        );
        let sent = self.asker.sent(op, || {
            self.asker
                .agent
                .get(&url)
                .header(AUTHORIZATION, bearer.as_str())
                .call()
        })?;
        let bytes = match sent {
            Sent::Answered(response) => self.asker.read_whole(op, response)?,
            Sent::Refused {
                status: FORBIDDEN | NOT_FOUND,
                ..
            } => return Ok(Vec::new()),
            Sent::Refused { status, .. } => return Err(self.refusal(op, status)),
        };
        let listed: WebListed = self.asker.parsed(op, &bytes)?;
        Ok(listed
            .items
            .into_iter()
            .filter(|listing| named_by(isrc, listing.isrc.as_deref()))
            .map(|listing| TrackId(listing.id))
            .take(TRACKS_TRIED_AT_MOST)
            .collect())
    }

    fn hosted_delivery(&self, track: TrackId) -> Result<Option<Delivery>> {
        let op = ProviderOp::Playback;
        let bearer = self.token(op, HIFI_TOKEN, TokenService::Hifi)?;
        let url = format!(
            "{}/manifests?id={}&quality={ASKED_QUALITY}",
            self.server, track.0
        );
        let sent = self.asker.sent(op, || {
            self.asker
                .agent
                .get(&url)
                .header(AUTHORIZATION, bearer.as_str())
                .call()
        })?;
        let bytes = match sent {
            Sent::Answered(response) => self.asker.read_whole(op, response)?,
            Sent::Refused {
                status: FORBIDDEN | NOT_FOUND,
                ..
            } => return Ok(None),
            Sent::Refused { status, .. } => return Err(self.refusal(op, status)),
        };
        let answer: HostedManifestResponse = self.asker.parsed(op, &bytes)?;
        let attributes = answer
            .attributes()
            .ok_or_else(|| self.asker.unreadable(op))?;
        let uri = attributes.uri.ok_or_else(|| self.asker.unreadable(op))?;
        if !manifest_hosts().holds(&uri) {
            tracing::warn!(track = track.0, provider = %self.asker.source, "a manifest URL was outside TIDAL's manifest hosts");
            return Err(Error::OffItsHosts {
                provider: self.asker.source.clone(),
                op,
            });
        }
        let response = match self.asker.sent(op, || self.media.get(&uri).call())? {
            Sent::Answered(response) => response,
            Sent::Refused {
                status: FORBIDDEN | NOT_FOUND,
                ..
            } => return Ok(None),
            Sent::Refused { status, .. } => return Err(self.refusal(op, status)),
        };
        let mime = response
            .headers()
            .get("Content-Type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned();
        let manifest = self.asker.read_whole(op, response)?;
        Player {
            source: &self.asker.source,
            hosts: &self.hosts,
            media: &self.media,
        }
        .delivered_manifest(track, &attributes.track_presentation, &mime, &manifest)
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
    fn tracks_named_by(
        &self,
        isrc: &Isrc,
        title: &str,
        artist: Option<&str>,
    ) -> Result<Vec<TrackId>> {
        if self.hosted {
            return self.hosted_tracks_named_by(isrc, title, artist);
        }
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
        if self.hosted {
            return self.hosted_delivery(track);
        }
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

    #[test]
    fn a_server_of_the_listeners_is_trusted_by_the_systems_certificates_and_the_hosted_one_by_the_built_in()
     {
        let roots = |hifi: &HifiApi| hifi.asker.agent.config().tls_config().root_certs().clone();

        assert!(matches!(
            roots(&HifiApi::at("https://hifi.home.arpa")),
            ureq::tls::RootCerts::Specific(_)
        ));
        assert!(matches!(
            roots(&HifiApi::hosted()),
            ureq::tls::RootCerts::WebPki
        ));
    }

    #[test]
    fn a_hosted_search_uses_the_names_and_encodes_them_as_a_query() {
        let query = title_artist_query(" Heroes Tonight ", Some(" Janji & Johnning "))
            .expect("a title and artist");

        assert_eq!(query, "Heroes Tonight Janji & Johnning");
        assert_eq!(
            encoded_query(&query),
            "Heroes%20Tonight%20Janji%20%26%20Johnning"
        );
        assert_eq!(title_artist_query("  ", None), None);
    }

    #[test]
    fn a_hosted_manifest_url_must_be_on_tidals_manifest_hosts() {
        let hosts = manifest_hosts();

        assert!(hosts.holds("https://im-fa.manifest.tidal.com/path"));
        assert!(!hosts.holds("https://manifest.tidal.com.evil.example/path"));
        assert!(!hosts.holds("http://im-fa.manifest.tidal.com/path"));
    }
}
