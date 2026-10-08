use std::time::Duration;

use resonate_library::{LinkNames, LinkedPlaylist, ListedSong, LookupOp};
use serde_json::Value;

use crate::{Client, Host, Result, client::LARGEST_DOCUMENT, shared::stated};

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
const CLOCK_PARTS_AT_MOST: usize = 3;
const SECONDS_A_PLACE: u64 = 60;

pub(crate) fn playlist_named(client: &Client, list: &str) -> Result<Option<LinkedPlaylist>> {
    let page = format!("{}/playlist?list={list}", Host::Youtube.base());
    let held = client.bytes(Host::Youtube, LookupOp::FollowLink, &page, LARGEST_DOCUMENT)?;

    Ok(held.and_then(|held| listed(&String::from_utf8_lossy(&held))))
}

fn listed(page: &str) -> Option<LinkedPlaylist> {
    let from = page.find(PAGE_DATA_OPENS)? + PAGE_DATA_OPENS.len();
    let rest = page.get(from..)?;
    let data = rest.get(..rest.find(PAGE_DATA_CLOSES)?)?;
    let document: Value = serde_json::from_str(data)
        .inspect_err(|error| tracing::debug!(%error, "YouTube's page data did not read"))
        .ok()?;
    let name = stated(document.pointer(PLAYLIST_TITLE).and_then(Value::as_str))?;

    let mut songs = Vec::new();
    uploads_in(&document, &mut songs);
    Some(LinkedPlaylist { name, songs })
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

fn clock(text: &str) -> Option<Duration> {
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
}
