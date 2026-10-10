use std::{sync::Arc, time::Duration};

use resonate_core::SourceId;
pub use resonate_fetch::Patience;
use resonate_fetch::{
    ASKED_LATER, Ranged, Trusted, asking_agent, configured, downloading_agent, escaped,
    is_a_document, retry_after_of, unreached,
};
use resonate_providers::{
    Choosing, Delivery, Error, Extension, Identity, Listed, Obtained, Opened, Opening, Pacing,
    Provider, ProviderOp, Result, is_a_page,
};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use ureq::{Agent, Body, http};

const MONOCHROME: &str = "monochrome";
const HOSTED_SERVER: &str = "https://tracks.monochrome.st";
const DELIVERED_AS: &str = "flac";
const LISTED_AT_MOST: usize = 25;
const ASKED_APART: Duration = Duration::from_millis(250);
const RETRIES_AT_MOST: u32 = 3;
const FIRST_RETRY_AFTER: Duration = Duration::from_secs(1);
const LONGEST_RETRY_AFTER: Duration = Duration::from_secs(8);
const GONE_FROM_THE_SERVER: [u16; 2] = [404, 410];
const LARGEST_ANSWER: u64 = 4 * 1024 * 1024;

fn downloading(patience: Patience, trusted: Trusted) -> Agent {
    downloading_agent(
        configured(patience.answered_within, trusted).build(),
        patience.broken_off_after,
    )
}

fn listings_readable<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Vec<Listing>, D::Error> {
    let listed = Option::<Vec<Value>>::deserialize(deserializer)?.unwrap_or_default();
    Ok(listed
        .into_iter()
        .filter_map(|listing| {
            serde_json::from_value(listing)
                .inspect_err(|error| {
                    tracing::debug!(%error, "a Monochrome listing that does not read is passed over");
                })
                .ok()
        })
        .collect())
}

fn id_spelt<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error> {
    Ok(match Option::<Value>::deserialize(deserializer)? {
        Some(Value::String(id)) => Some(id),
        Some(Value::Number(id)) => id.as_u64().map(|id| id.to_string()),
        _ => None,
    })
}

#[derive(Deserialize)]
struct Searched {
    #[serde(default, deserialize_with = "listings_readable")]
    tracks: Vec<Listing>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct Listing {
    #[serde(default, deserialize_with = "id_spelt")]
    track_id: Option<String>,
    #[serde(default, deserialize_with = "id_spelt")]
    id: Option<String>,
    #[serde(default)]
    isrc: Option<String>,
    #[serde(default)]
    playable: Option<bool>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    artist_names: Vec<String>,
    #[serde(default)]
    duration: Option<u64>,
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
    fn weigh(&self, identity: &Identity, choosing: &mut Choosing<TrackId>) {
        if self.playable == Some(false) {
            return;
        }
        let Some(track) = self.track() else {
            return;
        };
        let listed = Listed {
            isrcs: self.isrc.as_deref().into_iter().collect(),
            title: self.title.as_deref().unwrap_or_default(),
            version: None,
            artists: self.artist_names.iter().map(String::as_str).collect(),
            length: self.duration.map(Duration::from_millis),
        };
        choosing.weigh(identity, &listed, track);
    }

    fn track(&self) -> Option<TrackId> {
        self.track_id
            .as_deref()
            .or(self.id.as_deref())
            .and_then(TrackId::read)
    }
}

#[derive(Clone)]
pub struct Monochrome {
    source: SourceId,
    server: String,
    trusted: Trusted,
    patience: Patience,
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
            patience,
            asking: asking_agent(patience, trusted),
            downloading: downloading(patience, trusted),
            pacing: Arc::new(Pacing::new(ASKED_APART)),
        }
    }

    #[must_use]
    pub fn waiting(self, patience: Patience) -> Self {
        Self {
            patience,
            asking: asking_agent(patience, self.trusted),
            downloading: downloading(patience, self.trusted),
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
                let wait = retry_after_of(&response)
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
        unreached(self.source.clone(), op, error)
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

    fn found(&self, identity: &Identity) -> Result<Option<TrackId>> {
        let mut choosing = Choosing::default();
        let wordings = identity.wordings();
        for words in &wordings {
            for listing in self.searched(words)? {
                listing.weigh(identity, &mut choosing);
            }
            if choosing.holds_a_coded_listing() {
                break;
            }
        }
        let seen = choosing.seen();
        match choosing.chosen() {
            Some((track, taken)) => {
                tracing::debug!(track = track.0, ?taken, "Monochrome lists the song");
                Ok(Some(track))
            }
            None => {
                tracing::debug!(
                    title = identity.title,
                    asked = wordings.len(),
                    seen,
                    "Monochrome lists nothing coded or named as the song"
                );
                Ok(None)
            }
        }
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
        Ok(Opened::Reading(Box::new(Ranged::continuing(
            self.downloading.clone(),
            url,
            response,
            self.patience.resumed_after,
        ))))
    }
}

impl Provider for Monochrome {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn find(&self, identity: &Identity) -> Result<Obtained> {
        if !identity.may_be_listed() {
            return Ok(Obtained::Nothing);
        }
        let Some(track) = self.found(identity)? else {
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
    use resonate_providers::Taken;

    use super::*;

    fn the_one_asked_for(identity: &Identity, listings: &[Listing]) -> Option<(TrackId, Taken)> {
        let mut choosing = Choosing::default();
        for listing in listings {
            listing.weigh(identity, &mut choosing);
        }
        choosing.chosen()
    }

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

    fn coded(code: &str) -> Identity {
        Identity {
            isrcs: vec![resonate_core::Isrc::new(code).expect("an isrc")],
            ..Identity::named("Echoes")
        }
    }

    #[test]
    fn a_listing_is_taken_by_any_of_the_wants_codes_or_by_its_name_where_none_is_coded() {
        let listings = Monochrome::hosted()
            .read(SEARCHED.as_bytes())
            .expect("a search answer");
        assert_eq!(listings.len(), 3);
        let echoes = Identity {
            isrcs: vec![
                resonate_core::Isrc::new("USSM12409299").expect("an isrc"),
                resonate_core::Isrc::new(ECHOES_ISRC).expect("an isrc"),
            ],
            ..coded("USSM12409299")
        };
        let named = Identity {
            artist: Some("Pink Floyd".to_owned()),
            length: Some(Duration::from_millis(1_413_000)),
            ..coded("USSM12409299")
        };
        let named_but_longer = Identity {
            length: Some(Duration::from_millis(1_420_000)),
            ..named.clone()
        };

        assert_eq!(
            the_one_asked_for(&echoes, &listings),
            Some((TrackId("154140652551016448".to_owned()), Taken::ByItsCode))
        );
        assert_eq!(the_one_asked_for(&coded("USSM12409299"), &listings), None);
        assert_eq!(
            the_one_asked_for(&named, &listings),
            Some((TrackId("154140652551016448".to_owned()), Taken::NamedAlike))
        );
        assert_eq!(the_one_asked_for(&named_but_longer, &listings), None);
    }

    #[test]
    fn a_listing_that_cannot_be_played_is_passed_over() {
        let withheld = Listing {
            track_id: Some("1".to_owned()),
            id: None,
            isrc: Some(ECHOES_ISRC.to_owned()),
            playable: Some(false),
            title: None,
            artist_names: Vec::new(),
            duration: None,
        };
        let playable = Listing {
            track_id: Some("2".to_owned()),
            playable: None,
            ..withheld.clone()
        };

        assert_eq!(
            the_one_asked_for(&coded(ECHOES_ISRC), &[withheld, playable]),
            Some((TrackId("2".to_owned()), Taken::ByItsCode))
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
    fn a_numeric_id_is_read_an_odd_listing_passed_over_and_a_null_list_is_none() {
        let read = Monochrome::hosted()
            .read(
                br#"{"tracks": [
                    {"trackId": 154140652551016448, "isrc": "GBN9Y1100065"},
                    {"trackId": ["odd"], "isrc": 7},
                    "not a listing"
                ]}"#,
            )
            .expect("a search answer");
        let none = Monochrome::hosted()
            .read(br#"{"tracks": null}"#)
            .expect("a search answer");
        assert_eq!(
            the_one_asked_for(&coded(ECHOES_ISRC), &read),
            Some((TrackId("154140652551016448".to_owned()), Taken::ByItsCode))
        );
        assert_eq!(read.len(), 1);
        assert!(none.is_empty());
    }

    #[test]
    fn a_want_with_no_code_and_nothing_to_name_it_by_is_not_searched_for() {
        assert!(matches!(
            Monochrome::at("http://127.0.0.1:9").find(&Identity::named("Echoes")),
            Ok(Obtained::Nothing)
        ));
    }
}
