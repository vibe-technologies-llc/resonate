use std::time::Duration;

use resonate_fetch::unreached;
use resonate_providers::{
    Choosing, Error, Identity, Listed, ProviderOp, Rating, Result, is_a_page,
};
use serde_json::{Value, json};
use ureq::{Agent, Body, http};

use crate::{YtMusic, tool::VideoId};

const THE_CLIENT: &str = "WEB_REMIX";
const THE_LANGUAGE: &str = "en";
const THE_SONGS_FILTER: &str = "EgWKAQIIAWoKEAkQBRAKEAMQBA==";
const THE_ORIGIN: &str = "https://music.youtube.com";
const SEARCHED_AT: &str = "/youtubei/v1/search?prettyPrint=false";
const CLIENT_DATA_AT: &str = "/sw.js_data";
const SECTIONS: &str =
    "/contents/tabbedSearchResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents";
const A_SHELFS_ROWS: &str = "/musicShelfRenderer/contents";
const A_ROW: &str = "musicResponsiveListItemRenderer";
const A_ROWS_VIDEO: &str = "/playlistItemData/videoId";
const A_ROWS_PICTURES: &str = "/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails";
const A_COLUMNS_RUNS: &str = "/musicResponsiveListItemFlexColumnRenderer/text/runs";
const A_BADGES_ICON: &str = "/musicInlineBadgeRenderer/icon/iconType";
const THE_EXPLICIT_BADGE: &str = "MUSIC_EXPLICIT_BADGE";
const BYLINE_PARTS_APART: &str = " • ";
const ARTISTS_APART: [&str; 2] = [" & ", ", "];
const TITLE_COLUMN: usize = 0;
const BYLINE_COLUMN: usize = 1;
const LISTED_AT_MOST: usize = 25;
const LARGEST_ANSWER: u64 = 8 * 1024 * 1024;
const CLOCK_PARTS_AT_MOST: usize = 3;
const SECONDS_A_PLACE: u64 = 60;
const A_VERSION_BEGINS: &str = "1";
const DIGITS_OF_A_VERSIONS_DATE: usize = 8;
const PARTS_OF_A_VERSION: usize = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) video: VideoId,
    pub(crate) title: String,
    pub(crate) artist: Option<String>,
    pub(crate) album: Option<String>,
    pub(crate) length: Option<Duration>,
    pub(crate) explicit: bool,
    pub(crate) thumbnail: Option<String>,
}

fn all_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

fn reads_as_a_client_version(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == PARTS_OF_A_VERSION
        && parts[0] == A_VERSION_BEGINS
        && parts[1].len() == DIGITS_OF_A_VERSIONS_DATE
        && parts.iter().all(|part| all_digits(part))
}

fn client_version_in(data: &str) -> Option<&str> {
    data.split('"')
        .find(|quoted| reads_as_a_client_version(quoted))
}

fn length_read(clock: &str) -> Option<Duration> {
    let parts: Vec<&str> = clock.split(':').collect();
    if !(2..=CLOCK_PARTS_AT_MOST).contains(&parts.len()) {
        return None;
    }
    parts
        .into_iter()
        .try_fold(0_u64, |seconds, part| {
            let place = all_digits(part).then(|| part.parse::<u64>().ok())??;
            seconds.checked_mul(SECONDS_A_PLACE)?.checked_add(place)
        })
        .map(Duration::from_secs)
}

fn column_text(row: &Value, column: usize) -> Option<String> {
    let runs = row
        .get("flexColumns")?
        .get(column)?
        .pointer(A_COLUMNS_RUNS)?
        .as_array()?;
    let text: String = runs
        .iter()
        .filter_map(|run| run.get("text")?.as_str())
        .collect();
    (!text.trim().is_empty()).then_some(text)
}

fn badged_explicit(row: &Value) -> bool {
    row.get("badges")
        .and_then(Value::as_array)
        .is_some_and(|badges| {
            badges.iter().any(|badge| {
                badge.pointer(A_BADGES_ICON).and_then(Value::as_str) == Some(THE_EXPLICIT_BADGE)
            })
        })
}

fn largest_picture(row: &Value) -> Option<String> {
    row.pointer(A_ROWS_PICTURES)?
        .as_array()?
        .last()?
        .get("url")?
        .as_str()
        .map(str::to_owned)
}

impl Row {
    fn read(item: &Value) -> Option<Self> {
        let row = item.get(A_ROW)?;
        let video = VideoId::read(row.pointer(A_ROWS_VIDEO)?.as_str()?)?;
        let title = column_text(row, TITLE_COLUMN)?;
        let byline = column_text(row, BYLINE_COLUMN).unwrap_or_default();
        let mut parts: Vec<&str> = byline
            .split(BYLINE_PARTS_APART)
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect();
        let length = parts.last().copied().and_then(length_read);
        if length.is_some() {
            parts.pop();
        }
        let artist = (!parts.is_empty()).then(|| parts.remove(0).to_owned());

        Some(Self {
            video,
            title,
            artist,
            album: (!parts.is_empty()).then(|| parts.join(BYLINE_PARTS_APART)),
            length,
            explicit: badged_explicit(row),
            thumbnail: largest_picture(row),
        })
    }

    fn artists(&self) -> Vec<&str> {
        let mut artists: Vec<&str> = self.artist.as_deref().into_iter().collect();
        for apart in ARTISTS_APART {
            artists = artists
                .into_iter()
                .flat_map(|artist| artist.split(apart))
                .collect();
        }
        artists
    }

    pub(crate) fn weigh(&self, identity: &Identity, choosing: &mut Choosing<Self>) {
        let listed = Listed {
            title: &self.title,
            artists: self.artists(),
            length: self.length,
            stated: Rating::flagged(Some(self.explicit)),
            ..Listed::default()
        };
        choosing.weigh(identity, &listed, self.clone());
    }
}

impl YtMusic {
    fn answered(
        &self,
        response: std::result::Result<http::Response<Body>, ureq::Error>,
    ) -> Result<Vec<u8>> {
        let op = ProviderOp::Search;
        let response = response.map_err(|error| self.unreachable(op, error))?;
        let status = response.status();
        if !status.is_success() {
            return Err(Error::Refused {
                provider: self.source.clone(),
                op,
                status: status.as_u16(),
            });
        }
        response
            .into_body()
            .with_config()
            .limit(LARGEST_ANSWER)
            .read_to_vec()
            .map_err(|error| self.unreachable(op, error))
    }

    fn unreachable(&self, op: ProviderOp, error: ureq::Error) -> Error {
        tracing::debug!(%error, ?op, "YouTube Music could not be reached");
        unreached(self.source.clone(), op, error)
    }

    fn not_the_service(&self) -> Error {
        Error::NotTheService {
            provider: self.source.clone(),
            op: ProviderOp::Search,
        }
    }

    fn unreadable(&self) -> Error {
        Error::Unreadable {
            provider: self.source.clone(),
            op: ProviderOp::Search,
        }
    }

    fn client_version(&self, agent: &Agent) -> Result<String> {
        let mut held = self.client_version.lock();
        if let Some(version) = &*held {
            return Ok(version.clone());
        }
        self.pacing.paced();
        let data = self.answered(
            agent
                .get(format!("{}{CLIENT_DATA_AT}", self.music))
                .header("Origin", THE_ORIGIN)
                .call(),
        )?;
        let data = String::from_utf8_lossy(&data);
        let version = client_version_in(&data)
            .ok_or_else(|| self.not_the_service())?
            .to_owned();
        *held = Some(version.clone());
        Ok(version)
    }

    pub(crate) fn songs(&self, words: &str) -> Result<Vec<Row>> {
        let agent = &self.asking;
        let body = json!({
            "context": { "client": {
                "clientName": THE_CLIENT,
                "clientVersion": self.client_version(agent)?,
                "hl": THE_LANGUAGE,
            } },
            "query": words,
            "params": THE_SONGS_FILTER,
        });
        self.pacing.paced();
        let answer = self.answered(
            agent
                .post(format!("{}{SEARCHED_AT}", self.music))
                .header("Origin", THE_ORIGIN)
                .header("Content-Type", "application/json")
                .send(body.to_string()),
        )?;
        self.rows_of(&answer)
    }

    fn rows_of(&self, answer: &[u8]) -> Result<Vec<Row>> {
        let document: Value = serde_json::from_slice(answer).map_err(|error| {
            tracing::debug!(%error, "a YouTube Music search answer was not a document");
            if is_a_page(answer) {
                self.not_the_service()
            } else {
                self.unreadable()
            }
        })?;
        let sections = document
            .pointer(SECTIONS)
            .and_then(Value::as_array)
            .ok_or_else(|| self.unreadable())?;
        Ok(sections
            .iter()
            .filter_map(|section| section.pointer(A_SHELFS_ROWS)?.as_array())
            .flatten()
            .filter_map(Row::read)
            .take(LISTED_AT_MOST)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Cookies, Tool};

    const SONGS: &[u8] = include_bytes!("../tests/fixtures/songs.json");

    fn provider() -> YtMusic {
        YtMusic::new(Tool::new(None, Cookies::None))
    }

    fn rows() -> Vec<Row> {
        provider().rows_of(SONGS).expect("the rows")
    }

    fn fixture_with(change: impl FnOnce(&mut Vec<Value>)) -> Vec<u8> {
        let mut document: Value = serde_json::from_slice(SONGS).expect("a document");
        let rows = document
            .pointer_mut(&format!("{SECTIONS}/0{A_SHELFS_ROWS}"))
            .and_then(Value::as_array_mut)
            .expect("the rows");
        change(rows);
        document.to_string().into_bytes()
    }

    #[test]
    fn a_song_row_names_its_video_title_artist_album_length_and_badge() {
        let rows = rows();

        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].video.as_str(), "9RfVp-GhKfs");
        assert_eq!(rows[0].title, "Creep");
        assert_eq!(rows[0].artist.as_deref(), Some("Radiohead"));
        assert_eq!(rows[0].album.as_deref(), Some("Creep"));
        assert_eq!(rows[0].length, Some(Duration::from_secs(239)));
        assert!(rows[0].explicit);
        assert!(
            rows[0]
                .thumbnail
                .as_deref()
                .is_some_and(|url| url.contains("w120-h120"))
        );
        assert!(!rows[2].explicit);
    }

    #[test]
    fn a_row_credits_each_artist_its_byline_names() {
        let rows = rows();

        assert_eq!(
            rows[1].artists(),
            ["Mimi", "Josy", "The Voice Kids - Germany"]
        );
        assert_eq!(rows[0].artists(), ["Radiohead"]);
    }

    #[test]
    fn a_row_that_does_not_read_is_passed_over() {
        let answer = fixture_with(|rows| {
            rows[0]
                .pointer_mut(&format!("/{A_ROW}/playlistItemData"))
                .expect("the video")
                .take();
            rows[1]
                .pointer_mut(&format!("/{A_ROW}/flexColumns"))
                .expect("the columns")
                .take();
            rows.push(json!("not a row"));
            rows.push(json!({ A_ROW: { "playlistItemData": { "videoId": "a b" } } }));
        });

        let read = provider().rows_of(&answer).expect("the rows");

        assert_eq!(read.len(), 2);
        assert_eq!(read[0].title, "You And Whose Army?");
    }

    #[test]
    fn no_more_rows_are_read_than_a_search_lists() {
        let answer = fixture_with(|rows| {
            let first = rows[0].clone();
            rows.extend(std::iter::repeat_n(first, LISTED_AT_MOST));
        });

        assert_eq!(
            provider().rows_of(&answer).expect("the rows").len(),
            LISTED_AT_MOST
        );
    }

    #[test]
    fn a_byline_that_names_no_album_or_no_clock_leaves_those_unsaid() {
        let row = |byline: &str| {
            Row::read(&json!({ A_ROW: {
                "playlistItemData": { "videoId": "abc" },
                "flexColumns": [
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": "Song" }] } } },
                    { "musicResponsiveListItemFlexColumnRenderer": { "text": { "runs": [{ "text": byline }] } } },
                ],
            } }))
            .expect("a row")
        };

        let no_album = row("Ada • 1:02:03");
        let no_clock = row("Ada • 2017");
        let an_album_with_the_mark = row("Ada • One • Two • 3:00");

        assert_eq!(no_album.artist.as_deref(), Some("Ada"));
        assert_eq!(no_album.album, None);
        assert_eq!(no_album.length, Some(Duration::from_secs(3_723)));
        assert_eq!(no_clock.album.as_deref(), Some("2017"));
        assert_eq!(no_clock.length, None);
        assert_eq!(an_album_with_the_mark.album.as_deref(), Some("One • Two"));
    }

    #[test]
    fn a_clock_is_read_only_as_minutes_and_seconds_or_hours_with_them() {
        assert_eq!(length_read("3:59"), Some(Duration::from_secs(239)));
        assert_eq!(length_read("1:00:00"), Some(Duration::from_secs(3_600)));
        for not_a_clock in ["239", "", ":", "1:2:3:4", "a:bc", "-1:00", "+1:00"] {
            assert_eq!(length_read(not_a_clock), None, "{not_a_clock:?}");
        }
        assert_eq!(length_read("99999999999999999999:00"), None);
        assert_eq!(length_read("18446744073709551615:59:59"), None);
    }

    #[test]
    fn a_client_version_is_told_from_an_address_and_every_other_quoted_thing() {
        let data = r#")]}'
[[null,"78.57.30.54",67,"1.2.3.4","2.20261104.00.00","1.20261104.00.00","1.20261105.00.00"]]"#;

        assert_eq!(client_version_in(data), Some("1.20261104.00.00"));
        assert_eq!(client_version_in(r#"["78.57.30.54"]"#), None);
        assert_eq!(client_version_in("<html></html>"), None);
    }
}
