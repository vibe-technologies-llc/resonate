use std::{
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

use parking_lot::Mutex;
use resonate_core::{Mbid, Recording};
use resonate_fetch::escaped;
use resonate_library::{HeardElsewhere, HeardPage, History, ListeningService, LookupOp};
use serde::{Deserialize, Deserializer};

use crate::{Client, Host, lastfm::Application, query::Params};

const LISTENS_A_PAGE: usize = 1_000;
const SCROBBLES_A_PAGE: usize = 200;
const RECENT_TRACKS: &str = "user.getrecenttracks";
const ANSWERED_AS: &str = "json";

pub struct ListenBrainzHistory {
    client: Arc<Client>,
    user: String,
    edge: Mutex<Edge>,
}

#[derive(Default)]
struct Edge {
    second: u64,
    heard: Vec<HeardElsewhere>,
}

impl Edge {
    fn passes_over(
        &mut self,
        asked_before: Option<u64>,
        second: u64,
        heard: &HeardElsewhere,
    ) -> bool {
        if asked_before != self.second.checked_add(1) || second != self.second {
            return false;
        }

        let Some(seen) = self.heard.iter().position(|seen| seen == heard) else {
            return false;
        };

        self.heard.swap_remove(seen);
        true
    }
}

impl ListenBrainzHistory {
    pub fn new(client: Arc<Client>, user: String) -> Self {
        Self {
            client,
            user,
            edge: Mutex::new(Edge::default()),
        }
    }

    fn asked(&self, before: Option<u64>) -> String {
        let count = LISTENS_A_PAGE.to_string();
        let query = Params::new().with("count", &count);
        let query = match before {
            Some(before) => query.with("max_ts", &before.to_string()),
            None => query,
        };
        format!(
            "/1/user/{}/listens{}",
            escaped(self.user.trim()),
            query.finish()
        )
    }
}

impl History for ListenBrainzHistory {
    fn service(&self) -> ListeningService {
        ListeningService::ListenBrainz
    }

    fn page(&self, from: Option<u64>) -> resonate_library::Result<HeardPage> {
        let answer: Option<ListensAnswer> =
            self.client
                .json(Host::ListenBrainz, LookupOp::History, &self.asked(from))?;
        let mut edge = self.edge.lock();
        Ok(answer.map_or_else(nothing_more, |answer| answer.page(from, &mut edge)))
    }
}

pub struct LastfmHistory {
    client: Arc<Client>,
    application: Application,
    user: String,
}

impl LastfmHistory {
    pub fn new(client: Arc<Client>, application: Application, user: String) -> Self {
        Self {
            client,
            application,
            user,
        }
    }

    fn asked(&self, page: u64) -> String {
        Params::new()
            .with("method", RECENT_TRACKS)
            .with("user", self.user.trim())
            .with("api_key", &self.application.key)
            .with("limit", &SCROBBLES_A_PAGE.to_string())
            .with("page", &page.to_string())
            .with("format", ANSWERED_AS)
            .finish()
    }
}

impl History for LastfmHistory {
    fn service(&self) -> ListeningService {
        ListeningService::LastFm
    }

    fn page(&self, from: Option<u64>) -> resonate_library::Result<HeardPage> {
        let page = from.unwrap_or(1).max(1);
        let answer: Option<RecentAnswer> =
            self.client
                .json(Host::LastFm, LookupOp::History, &self.asked(page))?;
        match answer {
            Some(RecentAnswer {
                recenttracks: Some(recent),
                ..
            }) => Ok(recent.page(page)),
            Some(RecentAnswer { error: Some(_), .. }) => Err(resonate_library::Error::Refused {
                op: LookupOp::History,
                status: REFUSED,
            }),
            Some(_) | None => Ok(nothing_more()),
        }
    }
}

const REFUSED: u16 = 400;

const fn nothing_more() -> HeardPage {
    HeardPage {
        heard: Vec::new(),
        next: None,
    }
}

#[derive(Deserialize)]
struct ListensAnswer {
    payload: ListensPayload,
}

#[derive(Deserialize)]
struct ListensPayload {
    #[serde(default)]
    listens: Vec<ListenDoc>,
}

#[derive(Deserialize)]
struct ListenDoc {
    #[serde(default)]
    listened_at: Stamp,
    track_metadata: TrackMetadata,
}

#[derive(Clone, Copy, Default)]
struct Stamp(Option<u64>);

impl<'de> Deserialize<'de> for Stamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self(serde_json::Value::deserialize(deserializer)?.as_u64()))
    }
}

#[derive(Deserialize)]
struct TrackMetadata {
    #[serde(default)]
    artist_name: String,
    #[serde(default)]
    track_name: String,
    #[serde(default)]
    release_name: Option<String>,
    #[serde(default)]
    additional_info: Option<AdditionalInfo>,
    #[serde(default)]
    mbid_mapping: Option<MbidMapping>,
}

#[derive(Deserialize)]
struct AdditionalInfo {
    #[serde(default)]
    recording_mbid: Option<String>,
    #[serde(default)]
    duration_ms: Option<u64>,
}

#[derive(Deserialize)]
struct MbidMapping {
    #[serde(default)]
    recording_mbid: Option<String>,
}

impl ListensAnswer {
    fn page(self, asked_before: Option<u64>, edge: &mut Edge) -> HeardPage {
        let listens = self.payload.listens;
        let full = listens.len() >= LISTENS_A_PAGE;
        let stamped = listens
            .into_iter()
            .filter_map(|listen| match listen.listened_at.0 {
                Some(second) => Some((second, listen)),
                None => {
                    tracing::debug!(
                        title = listen.track_metadata.track_name,
                        "a listen stamped with no whole second was passed over"
                    );
                    None
                }
            })
            .collect::<Vec<_>>();
        let Some(earliest) = stamped.iter().map(|(second, _)| *second).min() else {
            return nothing_more();
        };

        let heard = stamped
            .into_iter()
            .filter_map(|(second, listen)| listen.heard(second).map(|heard| (second, heard)))
            .filter(|(second, heard)| !edge.passes_over(asked_before, *second, heard))
            .collect::<Vec<_>>();
        let including_the_earliest = earliest.saturating_add(1);
        let next = if asked_before == Some(including_the_earliest) {
            earliest
        } else {
            including_the_earliest
        };

        *edge = Edge {
            second: earliest,
            heard: heard
                .iter()
                .filter(|(second, _)| *second == earliest)
                .map(|(_, heard)| heard.clone())
                .collect(),
        };
        HeardPage {
            heard: heard.into_iter().map(|(_, heard)| heard).collect(),
            next: full.then_some(next),
        }
    }
}

impl ListenDoc {
    fn heard(self, second: u64) -> Option<HeardElsewhere> {
        let metadata = self.track_metadata;
        let told = metadata.additional_info;
        let recording = told
            .as_ref()
            .and_then(|told| told.recording_mbid.as_deref())
            .or_else(|| {
                metadata
                    .mbid_mapping
                    .as_ref()
                    .and_then(|mapping| mapping.recording_mbid.as_deref())
            })
            .and_then(|mbid| Mbid::new(mbid).ok());
        let length = told
            .and_then(|told| told.duration_ms)
            .filter(|ms| *ms > 0)
            .map(Duration::from_millis);
        named(
            second,
            Recording {
                mbid: recording,
                title: metadata.track_name,
                artist: metadata.artist_name,
                album: metadata.release_name.unwrap_or_default(),
                length,
            },
        )
    }
}

#[derive(Deserialize)]
struct RecentAnswer {
    #[serde(default)]
    recenttracks: Option<RecentTracks>,
    #[serde(default)]
    error: Option<u32>,
}

#[derive(Deserialize)]
struct RecentTracks {
    #[serde(default)]
    track: Vec<Scrobbled>,
    #[serde(rename = "@attr")]
    attributes: Option<RecentAttributes>,
}

#[derive(Deserialize)]
struct RecentAttributes {
    #[serde(rename = "totalPages", default)]
    total_pages: String,
}

#[derive(Deserialize)]
struct Scrobbled {
    #[serde(default)]
    name: String,
    #[serde(default)]
    mbid: String,
    artist: Texted,
    album: Option<Texted>,
    date: Option<Dated>,
}

#[derive(Deserialize)]
struct Texted {
    #[serde(rename = "#text", default)]
    text: String,
}

#[derive(Deserialize)]
struct Dated {
    uts: String,
}

impl RecentTracks {
    fn page(self, page: u64) -> HeardPage {
        let pages = self
            .attributes
            .and_then(|attributes| attributes.total_pages.parse::<u64>().ok())
            .unwrap_or(0);
        HeardPage {
            heard: self
                .track
                .into_iter()
                .filter_map(Scrobbled::heard)
                .collect(),
            next: (page < pages).then_some(page + 1),
        }
    }
}

impl Scrobbled {
    fn heard(self) -> Option<HeardElsewhere> {
        let at = self.date?.uts.parse::<u64>().ok()?;
        named(
            at,
            Recording {
                mbid: Mbid::new(&self.mbid).ok(),
                title: self.name,
                artist: self.artist.text,
                album: self.album.map(|album| album.text).unwrap_or_default(),
                length: None,
            },
        )
    }
}

fn named(at: u64, recording: Recording) -> Option<HeardElsewhere> {
    let named = !recording.title.trim().is_empty() && !recording.artist.trim().is_empty();
    let at = UNIX_EPOCH.checked_add(Duration::from_secs(at))?;
    named.then_some(HeardElsewhere { at, recording })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_of_listenbrainz_listens_names_each_song_and_where_the_next_page_starts() {
        let answer: ListensAnswer =
            serde_json::from_str(include_str!("../tests/fixtures/listenbrainz_listens.json"))
                .expect("the listens read");

        let page = answer.page(None, &mut Edge::default());

        assert_eq!(page.heard.len(), 2);
        assert_eq!(page.heard[0].recording.title, "Echoes");
        assert_eq!(page.heard[0].recording.album, "Meddle");
        assert_eq!(
            page.heard[0].recording.mbid.as_ref().map(Mbid::as_str),
            Some("0a1b2c3d-1111-4222-8333-444455556666")
        );
        assert_eq!(
            page.heard[0].recording.length,
            Some(Duration::from_secs(1_412))
        );
        assert_eq!(
            page.heard[1].recording.mbid.as_ref().map(Mbid::as_str),
            Some("9f8e7d6c-1111-4222-8333-444455556666")
        );
        assert_eq!(
            page.heard[1].at,
            UNIX_EPOCH + Duration::from_secs(1_700_000_000)
        );
        assert_eq!(page.next, None);
    }

    fn listen(at: u64, title: &str) -> ListenDoc {
        ListenDoc {
            listened_at: Stamp(Some(at)),
            track_metadata: TrackMetadata {
                artist_name: "Low".to_owned(),
                track_name: title.to_owned(),
                release_name: None,
                additional_info: None,
                mbid_mapping: None,
            },
        }
    }

    fn answered(listens: Vec<ListenDoc>) -> ListensAnswer {
        ListensAnswer {
            payload: ListensPayload { listens },
        }
    }

    fn titles(page: &HeardPage) -> Vec<&str> {
        page.heard
            .iter()
            .map(|heard| heard.recording.title.as_str())
            .collect()
    }

    #[test]
    fn listens_sharing_the_second_a_full_page_ends_on_are_read_once_each_from_the_next() {
        const EDGE: u64 = 1_700_000_000;

        let mut edge = Edge::default();
        let mut first = (1..LISTENS_A_PAGE - 1)
            .map(|later| listen(EDGE + later as u64, "Earlier on the page"))
            .collect::<Vec<_>>();

        first.extend([listen(EDGE, "Words"), listen(EDGE, "Lullaby")]);

        let second = vec![
            listen(EDGE, "Words"),
            listen(EDGE, "Lullaby"),
            listen(EDGE, "Sunflower"),
            listen(EDGE - 1, "Laser Beam"),
        ];

        let first = answered(first).page(None, &mut edge);
        let second = answered(second).page(first.next, &mut edge);

        assert_eq!(first.heard.len(), LISTENS_A_PAGE);
        assert_eq!(first.next, Some(EDGE + 1));
        assert_eq!(titles(&second), ["Sunflower", "Laser Beam"]);
        assert_eq!(second.next, None);
    }

    #[test]
    fn a_full_page_all_of_one_second_moves_on_past_it_rather_than_asking_it_again() {
        const EDGE: u64 = 1_700_000_000;

        let mut edge = Edge::default();
        let all_at_once = (0..LISTENS_A_PAGE)
            .map(|_| listen(EDGE, "Imported at once"))
            .collect::<Vec<_>>();

        let page = answered(all_at_once).page(Some(EDGE + 1), &mut edge);

        assert_eq!(page.heard.len(), LISTENS_A_PAGE);
        assert_eq!(page.next, Some(EDGE));
    }

    #[test]
    fn a_listen_stamped_past_what_the_clock_holds_is_passed_over_and_the_rest_are_read() {
        const HEARD: u64 = 1_700_000_000;

        let mut edge = Edge::default();
        let mixed = answered(vec![
            listen(u64::MAX, "Stamped past the clock"),
            listen(HEARD, "Words"),
        ]);
        let mut beyond = (0..LISTENS_A_PAGE)
            .map(|_| listen(u64::MAX, "Stamped past the clock"))
            .collect::<Vec<_>>();

        beyond.push(listen(HEARD, "Lullaby"));

        let read = mixed.page(None, &mut edge);
        let all_beyond = answered(beyond).page(None, &mut Edge::default());
        let past_the_edge = answered(vec![listen(HEARD, "Sunflower")]).page(
            Some(u64::MAX),
            &mut Edge {
                second: u64::MAX,
                heard: Vec::new(),
            },
        );

        assert_eq!(titles(&read), ["Words"]);
        assert_eq!(read.heard[0].at, UNIX_EPOCH + Duration::from_secs(HEARD));
        assert_eq!(titles(&all_beyond), ["Lullaby"]);
        assert_eq!(titles(&past_the_edge), ["Sunflower"]);
    }

    #[test]
    fn a_listen_stamped_below_zero_or_with_a_fraction_is_passed_over_and_the_rest_are_read() {
        let answer: ListensAnswer = serde_json::from_str(
            r#"{"payload":{"listens":[
                {"listened_at":1700000300,"track_metadata":{"artist_name":"Low","track_name":"Words"}},
                {"listened_at":-5,"track_metadata":{"artist_name":"Low","track_name":"Below zero"}},
                {"listened_at":1700000200.5,"track_metadata":{"artist_name":"Low","track_name":"Fractional"}},
                {"listened_at":"1700000150","track_metadata":{"artist_name":"Low","track_name":"Written as text"}},
                {"track_metadata":{"artist_name":"Low","track_name":"Unstamped"}},
                {"listened_at":1700000100,"track_metadata":{"artist_name":"Low","track_name":"Lullaby"}}
            ]}}"#,
        )
        .expect("the listens read");

        let page = answer.page(None, &mut Edge::default());

        assert_eq!(titles(&page), ["Words", "Lullaby"]);
        assert_eq!(
            page.heard[1].at,
            UNIX_EPOCH + Duration::from_secs(1_700_000_100)
        );
        assert_eq!(page.next, None);
    }

    #[test]
    fn a_full_page_stamped_past_the_clock_moves_on_rather_than_overflowing() {
        let all_beyond = (0..LISTENS_A_PAGE)
            .map(|_| listen(u64::MAX, "Stamped past the clock"))
            .collect::<Vec<_>>();

        let page = answered(all_beyond).page(None, &mut Edge::default());

        assert!(page.heard.is_empty());
        assert_eq!(page.next, Some(u64::MAX));
    }

    #[test]
    fn a_scrobble_stamped_past_what_the_clock_holds_is_passed_over_and_the_rest_are_read() {
        let answer: RecentAnswer = serde_json::from_str(
            r##"{"recenttracks":{"track":[
                {"name":"Time","artist":{"#text":"Pink Floyd"},"date":{"uts":"18446744073709551615"}},
                {"name":"Money","artist":{"#text":"Pink Floyd"},"date":{"uts":"1699999000"}}
            ],"@attr":{"totalPages":"1"}}}"##,
        )
        .expect("the scrobbles read");

        let page = answer.recenttracks.expect("a page").page(1);

        assert_eq!(titles(&page), ["Money"]);
        assert_eq!(page.next, None);
    }

    #[test]
    fn a_page_of_lastfm_scrobbles_leaves_out_the_song_playing_now_and_names_the_next_page() {
        let answer: RecentAnswer =
            serde_json::from_str(include_str!("../tests/fixtures/lastfm_recent_tracks.json"))
                .expect("the scrobbles read");

        let page = answer.recenttracks.expect("a page").page(1);

        assert_eq!(page.heard.len(), 2);
        assert_eq!(page.heard[0].recording.title, "Time");
        assert_eq!(page.heard[0].recording.artist, "Pink Floyd");
        assert_eq!(page.heard[0].recording.mbid, None);
        assert_eq!(
            page.heard[1].at,
            UNIX_EPOCH + Duration::from_secs(1_699_999_000)
        );
        assert_eq!(page.next, Some(2));
    }

    #[test]
    fn a_listenbrainz_page_is_asked_by_the_user_named_and_before_the_last_one_heard() {
        let history = ListenBrainzHistory::new(Arc::new(Client::default()), "a b".to_owned());

        assert_eq!(history.asked(None), "/1/user/a%20b/listens?count=1000");
        assert_eq!(
            history.asked(Some(1_700_000_000)),
            "/1/user/a%20b/listens?count=1000&max_ts=1700000000"
        );
    }
}
