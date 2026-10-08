mod fetched;
mod stall;
mod trust;

use std::{fmt::Write as _, sync::Arc, time::Duration};

use resonate_core::{Isrc, SourceId};
use resonate_providers::{
    Delivery, Error, Extension, Identity, Obtained, Opened, Opening, Pacing, Provider, ProviderOp,
    Result, is_a_page,
};
use serde::Deserialize;
use ureq::{
    Agent, Body,
    config::{Config, ConfigBuilder},
    http::{self, header::RETRY_AFTER},
    typestate::AgentScope,
    unversioned::{
        resolver::DefaultResolver,
        transport::{Connector as _, DefaultConnector},
    },
};

use crate::{
    fetched::{Fetched, as_io},
    stall::BrokenOffAfter,
};

const MONOCHROME: &str = "monochrome";
const HOSTED_SERVER: &str = "https://tracks.monochrome.st";
const DELIVERED_AS: &str = "flac";
const LISTED_AT_MOST: usize = 25;
const ASKED_APART: Duration = Duration::from_millis(250);
const RETRIES_AT_MOST: u32 = 3;
const FIRST_RETRY_AFTER: Duration = Duration::from_secs(1);
const LONGEST_RETRY_AFTER: Duration = Duration::from_secs(8);
const ASKED_LATER: [u16; 4] = [429, 502, 503, 504];
const GONE_FROM_THE_SERVER: [u16; 2] = [404, 410];
const DOCUMENT_TYPES: [&str; 3] = ["text/", "json", "xml"];
const ANSWERED_WITHIN: Duration = Duration::from_secs(20);
const BROKEN_OFF_AFTER: Duration = Duration::from_secs(30);
const CONNECTED_WITHIN: Duration = Duration::from_secs(10);
const USER_AGENT: &str = concat!("resonate/", env!("CARGO_PKG_VERSION"));
const LARGEST_ANSWER: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Patience {
    pub answered_within: Duration,
    pub broken_off_after: Duration,
}

impl Default for Patience {
    fn default() -> Self {
        Self {
            answered_within: ANSWERED_WITHIN,
            broken_off_after: BROKEN_OFF_AFTER,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Trusted {
    BuiltInRoots,
    SystemStoreToo,
}

fn configured(patience: Patience, trusted: Trusted) -> ConfigBuilder<AgentScope> {
    let builder = Agent::config_builder()
        .user_agent(USER_AGENT)
        .timeout_connect(Some(CONNECTED_WITHIN))
        .timeout_recv_response(Some(patience.answered_within))
        .http_status_as_error(false);
    match trusted {
        Trusted::BuiltInRoots => builder,
        Trusted::SystemStoreToo => builder.tls_config(trust::system_and_built_in()),
    }
}

fn asking_agent(patience: Patience, trusted: Trusted) -> Agent {
    configured(patience, trusted)
        .timeout_global(Some(patience.answered_within))
        .build()
        .new_agent()
}

fn downloading_agent(patience: Patience, trusted: Trusted) -> Agent {
    let config: Config = configured(patience, trusted).build();
    Agent::with_parts(
        config,
        DefaultConnector::new().chain(BrokenOffAfter(patience.broken_off_after)),
        DefaultResolver::default(),
    )
}

#[derive(Deserialize)]
struct Searched {
    #[serde(default)]
    tracks: Vec<Listing>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct Listing {
    #[serde(default)]
    track_id: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    isrc: Option<String>,
    #[serde(default)]
    playable: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TrackId(String);

impl TrackId {
    fn read(id: &str) -> Option<Self> {
        let id = id.trim();
        let digits = !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit());
        digits.then(|| Self(id.to_owned()))
    }
}

impl Listing {
    fn is_the_recording(&self, isrc: &Isrc) -> bool {
        self.playable != Some(false)
            && self
                .isrc
                .as_deref()
                .is_some_and(|held| Isrc::new(held.trim()).is_ok_and(|held| held == *isrc))
    }

    fn track(&self) -> Option<TrackId> {
        self.track_id
            .as_deref()
            .or(self.id.as_deref())
            .and_then(TrackId::read)
    }
}

fn the_one_asked_for(isrc: &Isrc, listings: &[Listing]) -> Option<TrackId> {
    listings
        .iter()
        .filter(|listing| listing.is_the_recording(isrc))
        .find_map(Listing::track)
}

fn wordings(identity: &Identity) -> Vec<String> {
    let title = identity.title.trim();
    if title.is_empty() {
        return Vec::new();
    }
    let artist = identity
        .artist
        .as_deref()
        .map(str::trim)
        .filter(|artist| !artist.is_empty());

    artist
        .map(|artist| format!("{title} {artist}"))
        .into_iter()
        .chain([title.to_owned()])
        .collect()
}

fn escaped(text: &str) -> String {
    text.bytes()
        .fold(String::with_capacity(text.len()), |mut written, byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                written.push(char::from(byte));
            } else {
                let _ = write!(written, "%{byte:02X}");
            }
            written
        })
}

fn is_a_document(mime: Option<&str>) -> bool {
    mime.is_some_and(|mime| {
        let mime = mime.to_ascii_lowercase();
        DOCUMENT_TYPES.iter().any(|kind| mime.contains(kind))
    })
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

#[derive(Clone)]
pub struct Monochrome {
    source: SourceId,
    server: String,
    trusted: Trusted,
    asking: Agent,
    downloading: Agent,
    pacing: Arc<Pacing>,
}

impl Monochrome {
    pub fn hosted() -> Self {
        Self::reaching(HOSTED_SERVER, Trusted::BuiltInRoots)
    }

    pub fn at(server: &str) -> Self {
        Self::reaching(server, Trusted::SystemStoreToo)
    }

    fn reaching(server: &str, trusted: Trusted) -> Self {
        let patience = Patience::default();
        Self {
            source: SourceId::new(MONOCHROME).unwrap_or_else(|_| SourceId::local()),
            server: server.trim().trim_end_matches('/').to_owned(),
            trusted,
            asking: asking_agent(patience, trusted),
            downloading: downloading_agent(patience, trusted),
            pacing: Arc::new(Pacing::new(ASKED_APART)),
        }
    }

    #[must_use]
    pub fn waiting(self, patience: Patience) -> Self {
        Self {
            asking: asking_agent(patience, self.trusted),
            downloading: downloading_agent(patience, self.trusted),
            ..self
        }
    }

    fn called(&self, agent: &Agent, op: ProviderOp, url: &str) -> Result<http::Response<Body>> {
        let mut retried = 0;
        loop {
            self.pacing.paced();
            let response = agent
                .get(url)
                .call()
                .map_err(|error| self.unreachable(op, error))?;
            if response.status().is_success() {
                return Ok(response);
            }
            let status = response.status().as_u16();
            if retried < RETRIES_AT_MOST && ASKED_LATER.contains(&status) {
                let wait = retry_after(&response)
                    .unwrap_or(FIRST_RETRY_AFTER * (1 << retried))
                    .min(LONGEST_RETRY_AFTER);
                tracing::debug!(
                    status,
                    ?wait,
                    ?op,
                    "the Monochrome server asked to be asked later"
                );
                self.pacing.cool_for(wait);
                retried += 1;
                continue;
            }
            return Err(Error::Refused {
                provider: self.source.clone(),
                op,
                status,
            });
        }
    }

    fn unreachable(&self, op: ProviderOp, error: ureq::Error) -> Error {
        tracing::debug!(%error, ?op, "the Monochrome server could not be reached");
        if trust::certificate_refused(&error) {
            return Error::Untrusted {
                provider: self.source.clone(),
                op,
            };
        }
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

    fn search_url(&self, words: &str) -> String {
        format!(
            "{}/search/tracks?q={}&limit={LISTED_AT_MOST}",
            self.server,
            escaped(words)
        )
    }

    fn track_url(&self, track: &TrackId) -> String {
        format!("{}/track/{}", self.server, track.0)
    }

    fn read(&self, bytes: &[u8]) -> Result<Vec<Listing>> {
        serde_json::from_slice::<Searched>(bytes)
            .map(|searched| searched.tracks)
            .map_err(|error| {
                tracing::debug!(%error, "a Monochrome search answer was not the document expected");
                if is_a_page(bytes) {
                    Error::NotTheService {
                        provider: self.source.clone(),
                        op: ProviderOp::Search,
                    }
                } else {
                    self.unreadable(ProviderOp::Search)
                }
            })
    }

    fn searched(&self, words: &str) -> Result<Vec<Listing>> {
        let op = ProviderOp::Search;
        let response = self.called(&self.asking, op, &self.search_url(words))?;
        let bytes = response
            .into_body()
            .with_config()
            .limit(LARGEST_ANSWER)
            .read_to_vec()
            .map_err(|error| self.unreachable(op, error))?;
        self.read(&bytes)
    }

    fn found(&self, identity: &Identity, isrc: &Isrc) -> Result<Option<TrackId>> {
        for words in wordings(identity) {
            if let Some(track) = the_one_asked_for(isrc, &self.searched(&words)?) {
                return Ok(Some(track));
            }
        }
        Ok(None)
    }

    fn downloaded(&self, track: &TrackId) -> Result<Opened> {
        let op = ProviderOp::Download;
        let url = self.track_url(track);
        let response = match self.called(&self.downloading, op, &url) {
            Err(Error::Refused { status, .. }) if GONE_FROM_THE_SERVER.contains(&status) => {
                return Ok(Opened::Gone);
            }
            called => called?,
        };
        if is_a_document(response.body().mime_type()) {
            return Err(self.unreadable(op));
        }
        Ok(Opened::Reading(Box::new(Fetched::continuing(
            self.downloading.clone(),
            url,
            response,
        ))))
    }
}

impl Provider for Monochrome {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn find(&self, identity: &Identity) -> Result<Obtained> {
        let Some(isrc) = &identity.isrc else {
            return Ok(Obtained::Nothing);
        };
        let Some(track) = self.found(identity, isrc)? else {
            return Ok(Obtained::Nothing);
        };
        let downloading = self.clone();
        Ok(Obtained::Found(Delivery::Stream {
            key: format!("track/{}", track.0).into_boxed_str(),
            extension: Extension::new(DELIVERED_AS)?,
            opening: Opening::new(move || downloading.downloaded(&track)),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEARCHED: &str = include_str!("../tests/fixtures/search.json");
    const ECHOES_ISRC: &str = "GBN9Y1100065";

    #[test]
    fn the_hosted_service_is_asked_unless_a_server_is_named_and_a_trailing_slash_is_dropped() {
        let hosted = Monochrome::hosted();
        let named = Monochrome::at(" https://tracks.home.arpa:8443/ ");

        assert_eq!(hosted.server, HOSTED_SERVER);
        assert_eq!(hosted.trusted, Trusted::BuiltInRoots);
        assert_eq!(named.server, "https://tracks.home.arpa:8443");
        assert_eq!(named.trusted, Trusted::SystemStoreToo);
        assert_eq!(hosted.source().as_str(), MONOCHROME);
    }

    #[test]
    fn a_search_names_its_words_escaped_and_a_track_is_asked_for_by_its_id() {
        let monochrome = Monochrome::at("http://music.local");

        assert_eq!(
            monochrome.search_url("Echoes & more"),
            "http://music.local/search/tracks?q=Echoes%20%26%20more&limit=25"
        );
        assert_eq!(
            monochrome.track_url(&TrackId("154140652551016448".to_owned())),
            "http://music.local/track/154140652551016448"
        );
    }

    #[test]
    fn only_digits_name_a_track() {
        assert_eq!(
            TrackId::read(" 154140652551016448 "),
            Some(TrackId("154140652551016448".to_owned()))
        );
        assert_eq!(TrackId::read(""), None);
        assert_eq!(TrackId::read("../admin"), None);
        assert_eq!(TrackId::read("12?x=1"), None);
    }

    #[test]
    fn a_listing_is_taken_by_its_isrc_and_never_by_its_title() {
        let listings = Monochrome::hosted()
            .read(SEARCHED.as_bytes())
            .expect("a search answer");
        assert_eq!(listings.len(), 3);
        let isrc = Isrc::new(ECHOES_ISRC).expect("an isrc");
        let another = Isrc::new("USSM12409299").expect("an isrc");

        assert_eq!(
            the_one_asked_for(&isrc, &listings),
            Some(TrackId("154140652551016448".to_owned()))
        );
        assert_eq!(the_one_asked_for(&another, &listings), None);
    }

    #[test]
    fn a_listing_that_cannot_be_played_is_passed_over() {
        let isrc = Isrc::new(ECHOES_ISRC).expect("an isrc");
        let withheld = Listing {
            track_id: Some("1".to_owned()),
            id: None,
            isrc: Some(ECHOES_ISRC.to_owned()),
            playable: Some(false),
        };
        let playable = Listing {
            track_id: Some("2".to_owned()),
            playable: None,
            ..withheld.clone()
        };

        assert_eq!(
            the_one_asked_for(&isrc, &[withheld, playable]),
            Some(TrackId("2".to_owned()))
        );
    }

    #[test]
    fn a_search_answer_that_is_not_the_document_expected_is_unreadable_and_a_page_the_server_away()
    {
        assert!(matches!(
            Monochrome::hosted().read(br#"{"tracks": 7}"#),
            Err(Error::Unreadable {
                op: ProviderOp::Search,
                ..
            })
        ));
        assert!(matches!(
            Monochrome::hosted().read(b"<html>busy</html>"),
            Err(Error::NotTheService {
                op: ProviderOp::Search,
                ..
            })
        ));
    }

    #[test]
    fn a_want_with_no_isrc_is_not_searched_for() {
        assert!(matches!(
            Monochrome::at("http://127.0.0.1:9").find(&Identity::named("Echoes")),
            Ok(Obtained::Nothing)
        ));
    }
}
