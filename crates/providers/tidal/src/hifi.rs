use std::{
    num::NonZeroUsize,
    thread,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use resonate_core::{Isrc, SourceId};
use resonate_fetch::{
    Patience, RESUMES_AT_MOST, Ranged, Trusted, asking_to_resume, configured, downloading_agent,
    escaped, retry_after_of, unreached,
};
use resonate_providers::{
    Choosing, Delivery, Error, Extension, Held, Identity, Listed as Weighed, Obtained, Opening,
    Provider, ProviderOp, Reopened, Result,
};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use ureq::{Agent, http::header::AUTHORIZATION};

use crate::{
    MediaHosts,
    asker::{Asker, Sent, media_agent},
    catalog::IMAGES,
    manifest::Media,
    played::{self, Finds, Playback, Player, TrackId, named_by},
};

pub const OWN_SERVER: &str = resonate_core::RESONATE_WEB_ADDRESS;
const RELAYED_ANSWERED_WITHIN: Duration = Duration::from_secs(60);
const RELAYED_AS: &str = "flac";
const HIFI_API: &str = "hifi-api";
const RESONATE: &str = "resonate";
const HOSTED_SERVER: &str = "https://hifi.odskyler.com";
const HIFI_TOKEN: &str = "https://hifi.odskyler.com/token";
const TIDAL_TOKEN: &str = "https://auth.odskyler.workers.dev/token";
const TIDAL_API: &str = "https://api.tidal.com/v1/";
const MANIFEST_SCHEME: &str = "https";
const MANIFEST_DOMAIN: &str = "manifest.tidal.com";
const ASKED_QUALITY: &str = "HI_RES_LOSSLESS";
const LISTED_AT_MOST: usize = 25;
const TRACKS_TRIED_AT_MOST: usize = 5;
const CODES_ASKED_AT_MOST: usize = 4;
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
const SERVICE_UNAVAILABLE: u16 = 503;
const NOT_AS_HELD: [u16; 2] = [412, 416];
const BUSY_ASKED_AGAIN_AFTER_UNSAID: Duration = Duration::from_secs(5);
const BUSY_ASKED_AGAIN_AFTER_AT_MOST: Duration = Duration::from_secs(10);
const ONE_DOWNLOAD_AT_ONCE: NonZeroUsize = NonZeroUsize::MIN;

#[derive(Deserialize)]
pub(crate) struct Wrapped<T> {
    pub(crate) data: T,
}

#[derive(Deserialize)]
struct Listed {
    #[serde(default, deserialize_with = "listings_readable")]
    items: Vec<Listing>,
}

#[derive(Deserialize)]
struct Listing {
    #[serde(deserialize_with = "id_spelt")]
    id: u64,
    #[serde(default)]
    isrc: Option<String>,
}

#[derive(Deserialize)]
struct WebListed {
    #[serde(default, deserialize_with = "listings_readable")]
    items: Vec<WebListing>,
}

#[derive(Deserialize)]
struct WebListing {
    #[serde(deserialize_with = "id_spelt")]
    id: u64,
    #[serde(default)]
    isrc: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    duration: Option<u64>,
    #[serde(default)]
    artists: Vec<Named>,
}

#[derive(Deserialize)]
struct Named {
    #[serde(default)]
    name: String,
}

impl WebListing {
    fn weighed(&self) -> Weighed<'_> {
        Weighed {
            isrcs: self.isrc.as_deref().into_iter().collect(),
            title: self.title.as_deref().unwrap_or_default(),
            version: self.version.as_deref(),
            artists: self
                .artists
                .iter()
                .map(|artist| artist.name.as_str())
                .collect(),
            length: self.duration.map(Duration::from_secs),
        }
    }
}

pub(crate) fn listings_readable<'de, D: Deserializer<'de>, T: serde::de::DeserializeOwned>(
    deserializer: D,
) -> std::result::Result<Vec<T>, D::Error> {
    let listed = Option::<Vec<Value>>::deserialize(deserializer)?.unwrap_or_default();
    Ok(listed
        .into_iter()
        .filter_map(|listing| {
            serde_json::from_value(listing)
                .inspect_err(|error| {
                    tracing::debug!(%error, "a listing that does not read is passed over");
                })
                .ok()
        })
        .collect())
}

pub(crate) fn id_spelt<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<u64, D::Error> {
    match Value::deserialize(deserializer)? {
        Value::Number(id) => id
            .as_u64()
            .ok_or_else(|| serde::de::Error::custom("a track id is a whole number")),
        Value::String(id) => id
            .trim()
            .parse()
            .map_err(|_| serde::de::Error::custom("a track id is a whole number")),
        _ => Err(serde::de::Error::custom("a track id is a whole number")),
    }
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
pub(crate) enum TokenService {
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
pub(crate) struct RequestId(String);

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

pub(crate) enum Answered {
    Ready(Vec<u8>),
    Queued(RequestId, Duration),
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hosting {
    pub server: String,
    pub hifi_token: String,
    pub tidal_token: String,
    pub tidal_api: String,
    pub images: String,
    pub manifests: MediaHosts,
}

impl Hosting {
    pub fn live() -> Self {
        Self {
            server: HOSTED_SERVER.to_owned(),
            hifi_token: HIFI_TOKEN.to_owned(),
            tidal_token: TIDAL_TOKEN.to_owned(),
            tidal_api: TIDAL_API.to_owned(),
            images: IMAGES.to_owned(),
            manifests: MediaHosts {
                scheme: MANIFEST_SCHEME.to_owned(),
                domain: MANIFEST_DOMAIN.to_owned(),
            },
        }
    }
}

pub struct HifiApi {
    pub(crate) asker: Asker,
    pub(crate) server: String,
    hosts: MediaHosts,
    media: Agent,
    patience: Duration,
    pub(crate) hosted: Option<Hosting>,
    pub(crate) secret: Option<Keyed>,
    pub(crate) images: String,
    tokens: Mutex<Tokens>,
}

pub(crate) struct Keyed {
    secret: String,
    pub(crate) relay: Agent,
}

impl Keyed {
    pub(crate) fn bearer(&self) -> String {
        format!("Bearer {}", self.secret)
    }
}

fn relayed_download(
    provider: &SourceId,
    agent: &Agent,
    url: &str,
    bearer: &str,
    held: Option<Held>,
) -> Result<Reopened> {
    let op = ProviderOp::Download;
    let held = held.filter(Held::can_be_checked);
    let mut asked_again = 0;
    let response = loop {
        let response = asking_to_resume(agent.get(url), held.as_ref())
            .header(AUTHORIZATION, bearer)
            .call()
            .map_err(|error| unreached(provider.clone(), op, error))?;
        if response.status().as_u16() != SERVICE_UNAVAILABLE {
            break response;
        }
        if asked_again == RESUMES_AT_MOST {
            return Err(Error::Busy {
                provider: provider.clone(),
                op,
            });
        }
        asked_again += 1;
        let wait = retry_after_of(&response)
            .unwrap_or(BUSY_ASKED_AGAIN_AFTER_UNSAID)
            .min(BUSY_ASKED_AGAIN_AFTER_AT_MOST);
        tracing::debug!(
            ?wait,
            "the server is busy with another download and is asked again"
        );
        thread::sleep(wait);
    };
    let status = response.status().as_u16();
    match status {
        200 | 206 => {
            let resumed_after = Patience::default().resumed_after;
            let ranged = match &held {
                Some(held) => Ranged::continuing_held(
                    agent.clone(),
                    url.to_owned(),
                    response,
                    held,
                    resumed_after,
                ),
                None if status == 200 => Some(Ranged::continuing(
                    agent.clone(),
                    url.to_owned(),
                    response,
                    resumed_after,
                )),
                None => None,
            };
            match ranged {
                Some(ranged) => Ok(Reopened::Reading(
                    ranged.bearing(bearer.to_owned()).into_resumed(),
                )),
                None => relayed_download(provider, agent, url, bearer, None),
            }
        }
        status if held.is_some() && NOT_AS_HELD.contains(&status) => {
            relayed_download(provider, agent, url, bearer, None)
        }
        FORBIDDEN | NOT_FOUND | GONE => Ok(Reopened::Gone),
        UNAUTHORISED => Err(Error::Unwelcome {
            provider: provider.clone(),
            op,
            code: status,
        }),
        _ => Err(Error::Refused {
            provider: provider.clone(),
            op,
            status,
        }),
    }
}

fn relay_agent() -> Agent {
    downloading_agent(
        configured(RELAYED_ANSWERED_WITHIN, Trusted::SystemStoreToo)
            .max_redirects(0)
            .build(),
        Patience::default().broken_off_after,
    )
}

fn carries_secret(server: &str) -> bool {
    if server.starts_with("https://") {
        return true;
    }
    let Some(authority) = server.strip_prefix("http://") else {
        return false;
    };
    let host = authority.split(['/', '?']).next().unwrap_or_default();
    ["localhost", "127.0.0.1", "[::1]"].iter().any(|loopback| {
        host == *loopback
            || host
                .strip_prefix(loopback)
                .is_some_and(|port| port.starts_with(':'))
    })
}

fn source() -> SourceId {
    SourceId::new(HIFI_API).unwrap_or_else(|_| SourceId::local())
}

fn resonate_source() -> SourceId {
    SourceId::new(RESONATE).unwrap_or_else(|_| SourceId::local())
}

impl HifiApi {
    pub fn known_as_resonate(self) -> Self {
        Self {
            asker: Asker::of_the_listeners_server(resonate_source()),
            ..self
        }
    }

    pub fn at(server: &str) -> Self {
        Self::building(server, MediaHosts::tidal(), None)
    }

    pub fn with_secret(self, secret: &str) -> Self {
        let secret = carries_secret(&self.server).then(|| Keyed {
            secret: secret.trim().to_owned(),
            relay: relay_agent(),
        });
        Self { secret, ..self }
    }

    pub fn hosted() -> Self {
        Self::hosted_through(Hosting::live(), MediaHosts::tidal())
    }

    pub fn hosted_through(hosting: Hosting, hosts: MediaHosts) -> Self {
        let server = hosting.server.clone();
        Self::building(&server, hosts, Some(hosting))
    }

    pub fn fetching_from(server: &str, hosts: MediaHosts) -> Self {
        Self::building(server, hosts, None)
    }

    fn building(server: &str, hosts: MediaHosts, hosted: Option<Hosting>) -> Self {
        let asker = if hosted.is_some() {
            Asker::new(source())
        } else {
            Asker::of_the_listeners_server(source())
        };
        let images = hosted
            .as_ref()
            .map_or_else(|| IMAGES.to_owned(), |hosting| hosting.images.clone());
        Self {
            asker,
            server: server.trim().trim_end_matches('/').to_owned(),
            hosts,
            media: media_agent(),
            patience: QUEUED_FOR_AT_MOST,
            hosted,
            images,
            secret: None,
            tokens: Mutex::new(Tokens::default()),
        }
    }

    pub fn pictured_from(self, images: &str) -> Self {
        Self {
            images: images.to_owned(),
            ..self
        }
    }

    pub fn queued_for_at_most(self, patience: Duration) -> Self {
        Self { patience, ..self }
    }

    pub(crate) fn refusal(&self, op: ProviderOp, status: u16) -> Error {
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

    pub(crate) fn token(
        &self,
        op: ProviderOp,
        endpoint: &str,
        service: TokenService,
    ) -> Result<String> {
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

        let sent = self
            .asker
            .sent(op, || self.asker.agent.get(endpoint).call())?;
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
            until: Instant::now()
                .checked_add(lasts)
                .ok_or_else(|| self.asker.unreadable(op))?,
        };
        let bearer = token.bearer.clone();
        let mut tokens = self.tokens.lock();
        match service {
            TokenService::Tidal => tokens.tidal = Some(token),
            TokenService::Hifi => tokens.hifi = Some(token),
        }
        Ok(bearer)
    }

    fn hosted_tracks_for(&self, hosting: &Hosting, identity: &Identity) -> Result<Vec<TrackId>> {
        let mut choosing = Choosing::default();
        let wordings = identity.wordings();
        for words in &wordings {
            for listing in self.hosted_listings(hosting, words)? {
                choosing.weigh(identity, &listing.weighed(), TrackId(listing.id));
            }
            if choosing.holds_a_coded_listing() {
                break;
            }
        }
        let seen = choosing.seen();
        let ranked: Vec<TrackId> = choosing
            .ranked()
            .into_iter()
            .map(|(track, taken)| {
                tracing::debug!(track = track.0, ?taken, "TIDAL lists the song");
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

    fn hosted_listings(&self, hosting: &Hosting, words: &str) -> Result<Vec<WebListing>> {
        let op = ProviderOp::Search;
        let bearer = self.token(op, &hosting.tidal_token, TokenService::Tidal)?;
        let url = format!(
            "{}search/tracks?query={}&limit={LISTED_AT_MOST}&offset=0&countryCode=US",
            hosting.tidal_api,
            escaped(words)
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
        Ok(listed.items)
    }

    pub(crate) fn player(&self) -> Player<'_> {
        Player {
            source: &self.asker.source,
            hosts: &self.hosts,
            media: &self.media,
        }
    }

    pub(crate) fn hosted_media(&self, hosting: &Hosting, track: TrackId) -> Result<Option<Media>> {
        let op = ProviderOp::Playback;
        let bearer = self.token(op, &hosting.hifi_token, TokenService::Hifi)?;
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
        if !hosting.manifests.holds(&uri) {
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
        self.player()
            .media_in_manifest(track, &attributes.track_presentation, &mime, &manifest)
    }

    pub(crate) fn asked(&self, op: ProviderOp, url: &str) -> Result<Answered> {
        let sent = self.asker.sent(op, || {
            let request = self.asker.agent.get(url);
            match &self.secret {
                Some(secret) => request
                    .header(AUTHORIZATION, secret.bearer().as_str())
                    .call(),
                None => request.call(),
            }
        })?;
        match sent {
            Sent::Answered(response) if response.status().as_u16() == ACCEPTED => {
                let wait = retry_after_of(&response)
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
        let request = self.asker.agent.delete(&url);
        let withdrawn = match &self.secret {
            Some(secret) => request
                .header(AUTHORIZATION, secret.bearer().as_str())
                .call(),
            None => request.call(),
        };
        if let Err(error) = withdrawn {
            tracing::debug!(%error, "a queued hifi-api request could not be withdrawn");
        }
        Error::StillQueued {
            provider: self.asker.source.clone(),
            op,
        }
    }

    pub(crate) fn waited_out(&self, op: ProviderOp, asked: Answered) -> Result<Option<Vec<u8>>> {
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
    fn provider(&self) -> &SourceId {
        &self.asker.source
    }

    fn tracks_for(&self, identity: &Identity) -> Result<Vec<TrackId>> {
        if let Some(hosting) = &self.hosted {
            return self.hosted_tracks_for(hosting, identity);
        }
        if identity.isrcs.is_empty() {
            return self.tracks_named(identity);
        }
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
        if let Some(keyed) = &self.secret {
            return self.relayed(keyed, track).map(Some);
        }
        let media = self.media(track)?;
        self.player().delivered_media(track, media)
    }
}

impl HifiApi {
    pub(crate) fn media(&self, track: TrackId) -> Result<Option<Media>> {
        if let Some(hosting) = &self.hosted {
            return self.hosted_media(hosting, track);
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
        self.player().media_in(track, &playback.data)
    }
}

impl HifiApi {
    fn tracks_named(&self, identity: &Identity) -> Result<Vec<TrackId>> {
        if !identity.may_be_listed() {
            return Ok(Vec::new());
        }
        let mut choosing = Choosing::default();
        for words in identity.wordings() {
            for listing in self.listed_by_words(&words)? {
                choosing.weigh(identity, &listing.weighed(), TrackId(listing.id));
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
                tracing::debug!(
                    track = track.0,
                    ?taken,
                    "the server lists the song by its name"
                );
                track
            })
            .take(TRACKS_TRIED_AT_MOST)
            .collect();
        if ranked.is_empty() {
            tracing::debug!(
                title = identity.title,
                seen,
                "the server lists nothing named as the song"
            );
        }
        Ok(ranked)
    }

    fn listed_by_words(&self, words: &str) -> Result<Vec<WebListing>> {
        let op = ProviderOp::Search;
        let url = format!(
            "{}/search/?s={}&limit={LISTED_AT_MOST}",
            self.server,
            escaped(words)
        );
        let Answered::Ready(bytes) = self.asked(op, &url)? else {
            return Ok(Vec::new());
        };
        let listed: Wrapped<WebListed> = self.asker.parsed(op, &bytes)?;
        Ok(listed.data.items)
    }

    pub fn track(&self, id: u64) -> Result<Option<Delivery>> {
        self.delivered(TrackId(id))
    }

    fn relayed(&self, keyed: &Keyed, track: TrackId) -> Result<Delivery> {
        let provider = self.asker.source.clone();
        let agent = keyed.relay.clone();
        let bearer = keyed.bearer();
        let url = format!("{}/file/?id={}", self.server, track.0);
        Ok(Delivery::Stream {
            key: track.key(),
            extension: Extension::new(RELAYED_AS)?,
            opening: Opening::resumable(move |held| {
                relayed_download(&provider, &agent, &url, &bearer, held)
            }),
        })
    }

    fn tracks_coded(&self, isrc: &Isrc) -> Result<Vec<TrackId>> {
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
}

impl Provider for HifiApi {
    fn source(&self) -> &SourceId {
        &self.asker.source
    }

    fn find(&self, identity: &Identity) -> Result<Obtained> {
        played::found(self, identity)
    }

    fn downloads_at_once(&self) -> NonZeroUsize {
        if self.secret.is_some() {
            ONE_DOWNLOAD_AT_ONCE
        } else {
            NonZeroUsize::MAX
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listing_spelling_its_id_as_text_is_read_and_one_that_does_not_read_passed_over() {
        let listed: WebListed = serde_json::from_str(
            r#"{"items": [{"id": "77", "isrc": "GBN9Y1100089"}, {"id": null}, 5, {"id": 78}]}"#,
        )
        .expect("a listing");
        let none: WebListed = serde_json::from_str(r#"{"items": null}"#).expect("a listing");

        assert_eq!(
            listed
                .items
                .iter()
                .map(|listing| listing.id)
                .collect::<Vec<_>>(),
            [77, 78]
        );
        assert!(none.items.is_empty());
    }

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
    fn a_secret_travels_only_over_https_or_to_the_machine_itself() {
        assert!(carries_secret("https://resonate.example"));
        assert!(carries_secret("http://localhost:8000"));
        assert!(carries_secret("http://127.0.0.1"));
        assert!(carries_secret("http://[::1]:8000"));
        assert!(!carries_secret("http://hifi.home.arpa"));
        assert!(!carries_secret("http://localhost.evil.example"));
        assert!(!carries_secret("ftp://localhost"));
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
    fn a_hosted_listing_is_weighed_by_its_title_version_artists_and_length() {
        let listed: WebListed = serde_json::from_str(
            r#"{"items": [{"id": 77, "isrc": "GBN9Y1100089", "title": "Heroes Tonight",
                "version": "Remastered", "duration": 208,
                "artists": [{"name": "Janji"}, {"name": "Johnning"}]}]}"#,
        )
        .expect("a listing");
        let weighed = listed.items[0].weighed();

        assert_eq!(weighed.isrcs, ["GBN9Y1100089"]);
        assert_eq!(weighed.version, Some("Remastered"));
        assert_eq!(weighed.artists, ["Janji", "Johnning"]);
        assert_eq!(weighed.length, Some(Duration::from_secs(208)));
        assert!(
            Identity {
                artist: Some("Janji & Johnning".to_owned()),
                length: Some(Duration::from_secs(209)),
                ..Identity::named("Heroes Tonight")
            }
            .named_alike(&weighed)
        );
    }

    #[test]
    fn a_hosted_manifest_url_must_be_on_tidals_manifest_hosts() {
        let hosts = Hosting::live().manifests;

        assert!(hosts.holds("https://im-fa.manifest.tidal.com/path"));
        assert!(!hosts.holds("https://manifest.tidal.com.evil.example/path"));
        assert!(!hosts.holds("http://im-fa.manifest.tidal.com/path"));
    }

    #[test]
    fn a_hosted_token_is_asked_for_in_this_builds_name_alone() {
        use std::{
            io::{BufRead, BufReader, Write},
            net::TcpListener,
            thread,
        };

        let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
        let endpoint = format!(
            "http://{}/token",
            listener.local_addr().expect("an address")
        );
        let served = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("a connection");
            let mut reader = BufReader::new(stream);
            let mut head = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).expect("a request");
                if line == "\r\n" {
                    break;
                }
                head.push_str(&line.to_ascii_lowercase());
            }
            let body = r#"{"access_token":"granted","expires_in":3600}"#;
            let answer = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n{body}",
                body.len()
            );
            reader
                .into_inner()
                .write_all(answer.as_bytes())
                .expect("an answer");
            head
        });
        let hifi = HifiApi::fetching_from("http://127.0.0.1:1", MediaHosts::tidal());

        let bearer = hifi
            .token(ProviderOp::Search, &endpoint, TokenService::Tidal)
            .expect("a token");
        let head = served.join().expect("the server");

        assert_eq!(bearer, "Bearer granted");
        assert!(!head.contains("origin:"), "{head}");
        assert!(!head.contains("referer:"), "{head}");
    }
}
