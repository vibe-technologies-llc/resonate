use std::{collections::BTreeSet, time::Duration};

use resonate_library::{LinkNames, LinkedPlaylist, ListedSong, LookupOp};
use serde_json::{Value, json};

use crate::{
    Client, Error, Host, Result,
    client::{LARGEST_CONTINUATION, LARGEST_DOCUMENT, Posted},
    shared::stated,
};

const PAGE_DATA_OPENS: &str = "var ytInitialData = ";
const PAGE_DATA_CLOSES: &str = ";</script>";
const A_LOCKUP: &str = "lockupViewModel";
const A_PLAYLIST_ROW: &str = "playlistVideoRenderer";
const A_VIDEO: &str = "LOCKUP_CONTENT_TYPE_VIDEO";
const A_BADGE: &str = "thumbnailBadgeViewModel";
const PLAYLIST_TITLE: &str = "/metadata/playlistMetadataRenderer/title";
const LOCKUP_TITLE: &str = "/metadata/lockupMetadataViewModel/title/content";
const LOCKUP_CHANNEL: &str = "/metadata/lockupMetadataViewModel/metadata/contentMetadataViewModel/metadataRows/0/metadataParts/0/text/content";
const ROW_TITLE: [&str; 2] = ["/title/runs/0/text", "/title/simpleText"];
const ROW_CHANNEL: &str = "/shortBylineText/runs/0/text";
const ROW_SECONDS: &str = "/lengthSeconds";
const KEY_OPENS: &str = "\"INNERTUBE_API_KEY\":\"";
const VERSION_OPENS: &str = "\"INNERTUBE_CONTEXT_CLIENT_VERSION\":\"";
const BROWSE: &str = "/youtubei/v1/browse?key=";
const THE_ANSWER: &str = "/onResponseReceivedActions";
const THE_COMMAND: &str = "continuationCommand";
const THE_TOKEN: &str = "token";
const A_CONTINUATION: [&str; 2] = ["continuationItemViewModel", "continuationItemRenderer"];
const THE_CLIENT: &str = "WEB";
const UPLOADS_AT_MOST: usize = 1_000;
const CLOCK_PARTS_AT_MOST: usize = 3;
const SECONDS_A_PLACE: u64 = 60;

pub(crate) fn playlist_named(client: &Client, list: &str) -> Result<Option<LinkedPlaylist>> {
    let page = format!("{}/playlist?list={list}", Host::Youtube.base());
    let held = client.bytes(Host::Youtube, LookupOp::FollowLink, &page, LARGEST_DOCUMENT)?;

    let Some(first) = held.and_then(|held| listed(&String::from_utf8_lossy(&held))) else {
        return Ok(None);
    };
    let mut linked = first.linked;
    if let Some((asking, next)) = first.asking.zip(first.next) {
        continued(client, &asking, next, &mut linked.songs)?;
    }
    linked.songs.truncate(UPLOADS_AT_MOST);

    Ok(Some(linked))
}

#[cfg(fuzzing)]
pub(crate) fn read_listed(page: &str) -> Option<LinkedPlaylist> {
    listed(page).map(|first| first.linked)
}

#[derive(Debug, PartialEq, Eq)]
struct Opening {
    linked: LinkedPlaylist,
    asking: Option<Asking>,
    next: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct Asking {
    key: String,
    version: String,
}

impl Asking {
    fn read(page: &str) -> Option<Self> {
        let key = quoted_after(page, KEY_OPENS)?;
        let version = quoted_after(page, VERSION_OPENS)?;
        let key_fits = key
            .bytes()
            .all(|glyph| glyph.is_ascii_alphanumeric() || matches!(glyph, b'_' | b'-'));
        let version_fits = version
            .bytes()
            .all(|glyph| glyph.is_ascii_digit() || glyph == b'.');

        (!key.is_empty() && key_fits && !version.is_empty() && version_fits).then(|| Self {
            key: key.to_owned(),
            version: version.to_owned(),
        })
    }

    fn body(&self, token: &str) -> Value {
        json!({
            "context": { "client": { "clientName": THE_CLIENT, "clientVersion": self.version } },
            "continuation": token,
        })
    }
}

fn quoted_after<'a>(page: &'a str, opens: &str) -> Option<&'a str> {
    page.split_once(opens)?.1.split('"').next()
}

fn continued(
    client: &Client,
    asking: &Asking,
    first: String,
    songs: &mut Vec<ListedSong>,
) -> Result<()> {
    let url = format!("{}{BROWSE}{}", Host::Youtube.base(), asking.key);
    let mut asked = BTreeSet::new();
    let mut next = Some(first);
    while let Some(token) = next.take() {
        if songs.len() >= UPLOADS_AT_MOST || !asked.insert(token.clone()) {
            break;
        }
        let answered = client.posted_within::<Value>(
            Host::Youtube,
            LookupOp::FollowLink,
            &url,
            &Posted::json(&asking.body(&token)),
            LARGEST_CONTINUATION,
        );
        let document = match answered {
            Ok(Some(document)) => document,
            Ok(None) => break,
            Err(error @ Error::Offline { .. }) => return Err(error),
            Err(error) => {
                tracing::debug!(%error, "YouTube's continuation was not read; the page's own uploads stand");
                break;
            }
        };
        let (more, after) = continuation_read(&document);
        if more.is_empty() {
            break;
        }
        songs.extend(more);
        next = after;
    }

    Ok(())
}

fn continuation_read(document: &Value) -> (Vec<ListedSong>, Option<String>) {
    let Some(answer) = document.pointer(THE_ANSWER) else {
        return (Vec::new(), None);
    };
    let mut songs = Vec::new();
    uploads_in(answer, &mut songs);

    (songs, token_after(answer))
}

fn token_after(value: &Value) -> Option<String> {
    match value {
        Value::Array(items) if items.iter().any(is_an_entry) => items.iter().find_map(|item| {
            A_CONTINUATION
                .iter()
                .find_map(|key| item.get(key))
                .and_then(token_in)
        }),
        Value::Array(items) => items.iter().find_map(token_after),
        Value::Object(fields) => fields.values().find_map(token_after),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    }
}

fn is_an_entry(item: &Value) -> bool {
    item.get(A_LOCKUP).is_some() || item.get(A_PLAYLIST_ROW).is_some()
}

fn token_in(value: &Value) -> Option<String> {
    match value {
        Value::Object(fields) => fields
            .get(THE_COMMAND)
            .and_then(|command| command.get(THE_TOKEN))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| fields.values().find_map(token_in)),
        Value::Array(items) => items.iter().find_map(token_in),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    }
}

fn listed(page: &str) -> Option<Opening> {
    let from = page.find(PAGE_DATA_OPENS)? + PAGE_DATA_OPENS.len();
    let rest = page.get(from..)?;
    let data = rest.get(..rest.find(PAGE_DATA_CLOSES)?)?;
    let document: Value = serde_json::from_str(data)
        .inspect_err(|error| tracing::debug!(%error, "YouTube's page data did not read"))
        .ok()?;
    let name = stated(document.pointer(PLAYLIST_TITLE).and_then(Value::as_str))?;

    let mut songs = Vec::new();
    uploads_in(&document, &mut songs);

    Some(Opening {
        linked: LinkedPlaylist { name, songs },
        asking: Asking::read(page),
        next: token_after(&document),
    })
}

fn uploads_in(value: &Value, songs: &mut Vec<ListedSong>) {
    match value {
        Value::Object(fields) => {
            for (key, inner) in fields {
                let upload = match key.as_str() {
                    A_LOCKUP => Some(of_a_lockup(inner)),
                    A_PLAYLIST_ROW => Some(of_a_row(inner)),
                    _ => None,
                };
                match upload {
                    Some(named) => songs.extend(named.map(ListedSong::Uploaded)),
                    None => uploads_in(inner, songs),
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                uploads_in(item, songs);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn of_a_lockup(lockup: &Value) -> Option<LinkNames> {
    if lockup.get("contentType").and_then(Value::as_str) != Some(A_VIDEO) {
        return None;
    }
    Some(LinkNames {
        isrcs: Vec::new(),
        length: lockup.get("contentImage").and_then(clock_in),
        title: Some(stated(
            lockup.pointer(LOCKUP_TITLE).and_then(Value::as_str),
        )?),
        artist: stated(lockup.pointer(LOCKUP_CHANNEL).and_then(Value::as_str)),
    })
}

fn of_a_row(row: &Value) -> Option<LinkNames> {
    let title = ROW_TITLE
        .iter()
        .find_map(|pointer| stated(row.pointer(pointer).and_then(Value::as_str)))?;
    Some(LinkNames {
        isrcs: Vec::new(),
        length: row
            .pointer(ROW_SECONDS)
            .and_then(Value::as_str)
            .and_then(|seconds| seconds.parse().ok())
            .filter(|seconds| *seconds > 0)
            .map(Duration::from_secs),
        title: Some(title),
        artist: stated(row.pointer(ROW_CHANNEL).and_then(Value::as_str)),
    })
}

fn clock_in(value: &Value) -> Option<Duration> {
    match value {
        Value::Object(fields) => fields.iter().find_map(|(key, inner)| {
            if key == A_BADGE {
                inner.get("text").and_then(Value::as_str).and_then(clock)
            } else {
                clock_in(inner)
            }
        }),
        Value::Array(items) => items.iter().find_map(clock_in),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => None,
    }
}

pub(crate) fn clock(text: &str) -> Option<Duration> {
    let parts: Vec<&str> = text.trim().split(':').collect();
    if !(2..=CLOCK_PARTS_AT_MOST).contains(&parts.len()) {
        return None;
    }
    let mut seconds = 0_u64;
    for part in parts {
        if part.is_empty() || !part.bytes().all(|digit| digit.is_ascii_digit()) {
            return None;
        }
        seconds = seconds
            .checked_mul(SECONDS_A_PLACE)?
            .checked_add(part.parse().ok()?)?;
    }
    (seconds > 0).then(|| Duration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_youtube_playlist_names_its_title_and_each_upload_by_its_title_channel_and_length() {
        let linked = listed(include_str!("../tests/fixtures/youtube_playlist.html"))
            .map(|first| first.linked)
            .expect("a playlist read off the page");

        assert_eq!(linked.songs.len(), 3);
        assert_eq!(
            linked.songs.first(),
            Some(&ListedSong::Uploaded(LinkNames {
                isrcs: Vec::new(),
                length: Some(Duration::from_secs(235)),
                title: Some("BELLAKEO (Video Oficial) - Peso Pluma, Anitta".to_owned()),
                artist: Some("Peso Pluma".to_owned()),
            }))
        );
        assert_eq!(
            linked.songs.get(1),
            Some(&ListedSong::Uploaded(LinkNames {
                isrcs: Vec::new(),
                length: Some(Duration::from_secs(650)),
                title: Some("Arcángel - FN8 ( Video Lyric )".to_owned()),
                artist: Some("Arcangel".to_owned()),
            }))
        );
        assert!(!linked.name.is_empty());
        assert_eq!(listed("<html>no page data</html>"), None);
    }

    #[test]
    fn a_playlist_row_of_the_older_page_is_read_as_an_upload_and_anything_but_a_video_is_not() {
        let page = json!({
            "metadata": { "playlistMetadataRenderer": { "title": "Old" } },
            "contents": [
                { "playlistVideoRenderer": {
                    "title": { "runs": [{ "text": "Rick Astley - Never Gonna Give You Up" }] },
                    "shortBylineText": { "runs": [{ "text": "Rick Astley" }] },
                    "lengthSeconds": "213"
                } },
                { "lockupViewModel": {
                    "contentType": "LOCKUP_CONTENT_TYPE_PLAYLIST",
                    "metadata": { "lockupMetadataViewModel": { "title": { "content": "A mix" } } }
                } }
            ]
        });
        let mut songs = Vec::new();
        uploads_in(&page, &mut songs);

        assert_eq!(
            songs,
            vec![ListedSong::Uploaded(LinkNames {
                isrcs: Vec::new(),
                length: Some(Duration::from_secs(213)),
                title: Some("Rick Astley - Never Gonna Give You Up".to_owned()),
                artist: Some("Rick Astley".to_owned()),
            })]
        );
    }

    #[test]
    fn a_badge_reads_as_a_length_only_where_it_is_a_clock() {
        assert_eq!(clock("3:55"), Some(Duration::from_secs(235)));
        assert_eq!(clock("1:02:03"), Some(Duration::from_secs(3_723)));
        assert_eq!(clock("LIVE"), None);
        assert_eq!(clock("1:2:3:4"), None);
        assert_eq!(clock(":30"), None);
        assert_eq!(clock("0:00"), None);
    }

    #[test]
    fn a_page_holding_more_than_it_lists_names_the_continuation_beside_its_uploads_and_how_to_ask()
    {
        let first = listed(include_str!(
            "../tests/fixtures/youtube_playlist_paged.html"
        ))
        .expect("a playlist read off the page");

        assert_eq!(first.linked.songs.len(), 3);
        assert_eq!(
            first.asking,
            Some(Asking {
                key: "AIzaSyAO_FJ2SlqU8Q4STEHLGCilw_Y9_11qcW8".to_owned(),
                version: "2.20261023.01.00".to_owned(),
            })
        );
        let next = first.next.expect("a continuation");
        assert!(next.starts_with("4qmFsgKBARIkVkxQTEZncXVMbkw1"));
        assert_eq!(
            listed(include_str!("../tests/fixtures/youtube_playlist.html"))
                .and_then(|first| first.next),
            None
        );
    }

    #[test]
    fn a_continuation_adds_its_uploads_once_and_names_the_next_only_where_it_has_one() {
        let last: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/youtube_playlist_continuation.json"
        ))
        .expect("a continuation");
        let (songs, after) = continuation_read(&last);

        assert_eq!(songs.len(), 3);
        assert!(songs.iter().all(|song| matches!(
            song,
            ListedSong::Uploaded(names) if names.title.is_some() && names.length.is_some()
        )));
        assert_eq!(after, None);

        let more: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/youtube_playlist_continuation_more.json"
        ))
        .expect("a continuation");
        let (songs, after) = continuation_read(&more);

        assert_eq!(songs.len(), 3);
        assert!(after.is_some_and(|token| token.starts_with("4qmFsgKBARIkVkxQTEZncXVMbkw1")));
        assert_eq!(
            continuation_read(&json!({ "contents": {} })),
            (Vec::new(), None)
        );
    }

    #[test]
    fn what_a_continuation_is_asked_with_is_the_pages_own_key_and_client_and_nothing_else() {
        let asking = Asking::read(
            r#"ytcfg.set({"INNERTUBE_API_KEY":"AIza_k-1","INNERTUBE_CONTEXT_CLIENT_VERSION":"2.2026.01"});"#,
        )
        .expect("a key and a version");

        assert_eq!(
            asking.body("tok"),
            json!({
                "context": { "client": { "clientName": "WEB", "clientVersion": "2.2026.01" } },
                "continuation": "tok",
            })
        );
        assert_eq!(
            Asking::read(r#""INNERTUBE_API_KEY":"a&b=c","INNERTUBE_CONTEXT_CLIENT_VERSION":"2.1""#),
            None
        );
        assert_eq!(
            Asking::read(r#""INNERTUBE_API_KEY":"abc","INNERTUBE_CONTEXT_CLIENT_VERSION":"2.1x""#),
            None
        );
        assert_eq!(Asking::read(r#""INNERTUBE_API_KEY":"abc""#), None);
    }
}
