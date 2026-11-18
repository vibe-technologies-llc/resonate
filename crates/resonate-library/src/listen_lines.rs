use std::{
    str,
    time::{Duration, UNIX_EPOCH},
};

use resonate_core::{Mbid, Recording};

use crate::{Error, HeardElsewhere, Result, json::Json};

const BYTE_ORDER_MARK: &[u8] = "\u{feff}".as_bytes();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListenFault {
    NotUtf8,
    NotJson,
    NotAnObject,
    NoMoment,
    NoTrackMetadata,
}

pub fn heard_in_lines(bytes: &[u8]) -> Result<Vec<HeardElsewhere>> {
    let bytes = bytes.strip_prefix(BYTE_ORDER_MARK).unwrap_or(bytes);
    let mut heard = Vec::new();
    for (index, raw) in bytes.split(|byte| *byte == b'\n').enumerate() {
        let refused = |fault| Error::ListenLine {
            line: index + 1,
            fault,
        };
        let text = str::from_utf8(raw).map_err(|_| refused(ListenFault::NotUtf8))?;
        if text.trim().is_empty() {
            continue;
        }
        let json = Json::read(text).ok_or(refused(ListenFault::NotJson))?;
        heard.extend(listen_in(&json).map_err(refused)?);
    }
    Ok(heard)
}

fn listen_in(json: &Json) -> std::result::Result<Option<HeardElsewhere>, ListenFault> {
    if !json.is_object() {
        return Err(ListenFault::NotAnObject);
    }
    let moment = json
        .get("listened_at")
        .and_then(Json::as_whole)
        .and_then(|seconds| UNIX_EPOCH.checked_add(Duration::from_secs(seconds)))
        .ok_or(ListenFault::NoMoment)?;
    let metadata = json
        .get("track_metadata")
        .filter(|metadata| metadata.is_object())
        .ok_or(ListenFault::NoTrackMetadata)?;

    let information = metadata.get("additional_info");
    let recording_mbid = information
        .and_then(|information| information.get("recording_mbid"))
        .or_else(|| {
            metadata
                .get("mbid_mapping")
                .and_then(|mapping| mapping.get("recording_mbid"))
        })
        .and_then(Json::as_str)
        .and_then(|mbid| Mbid::new(mbid).ok());
    let length = information
        .and_then(|information| information.get("duration_ms"))
        .and_then(Json::as_whole)
        .filter(|milliseconds| *milliseconds > 0)
        .map(Duration::from_millis);

    let text = |key| metadata.get(key).and_then(Json::as_str).unwrap_or_default();
    let recording = Recording {
        mbid: recording_mbid,
        title: text("track_name").to_owned(),
        artist: text("artist_name").to_owned(),
        album: text("release_name").to_owned(),
        length,
    };
    let named = !recording.title.trim().is_empty() && !recording.artist.trim().is_empty();
    Ok(named.then_some(HeardElsewhere {
        at: moment,
        recording,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: &str = r#"{"listened_at":1700000000,"track_metadata":{"artist_name":"Low","track_name":"Say \"hi\"","release_name":"Words","additional_info":{"recording_mbid":"0a1b2c3d-1111-4222-8333-444455556666","duration_ms":185000}}}"#;

    #[test]
    fn a_line_is_a_listen_with_its_names_id_and_length() {
        let heard = heard_in_lines(ONE.as_bytes()).expect("reads");

        assert_eq!(heard.len(), 1);
        assert_eq!(heard[0].at, UNIX_EPOCH + Duration::from_secs(1_700_000_000));
        assert_eq!(heard[0].recording.title, "Say \"hi\"");
        assert_eq!(heard[0].recording.artist, "Low");
        assert_eq!(heard[0].recording.album, "Words");
        assert_eq!(heard[0].recording.length, Some(Duration::from_secs(185)));
        assert_eq!(
            heard[0].recording.mbid.as_ref().map(Mbid::as_str),
            Some("0a1b2c3d-1111-4222-8333-444455556666")
        );
    }

    #[test]
    fn blank_lines_a_byte_order_mark_and_carriage_returns_are_passed_over() {
        let text = format!("\u{feff}\n{ONE}\r\n\n  \n{ONE}\r\n");

        assert_eq!(heard_in_lines(text.as_bytes()).expect("reads").len(), 2);
    }

    #[test]
    fn a_listen_naming_no_song_or_no_artist_is_passed_over() {
        let text = concat!(
            r#"{"listened_at":1,"track_metadata":{"artist_name":"Low"}}"#,
            "\n",
            r#"{"listened_at":2,"track_metadata":{"track_name":" ","artist_name":"Low"}}"#,
            "\n",
            r#"{"listened_at":3,"track_metadata":{"track_name":"A"}}"#,
            "\n",
            r#"{"listened_at":4,"track_metadata":{"track_name":"A","artist_name":"B","mbid_mapping":{"recording_mbid":"0a1b2c3d-1111-4222-8333-444455556666"}}}"#,
        );

        let heard = heard_in_lines(text.as_bytes()).expect("reads");

        assert_eq!(heard.len(), 1);
        assert!(heard[0].recording.mbid.is_some());
    }

    fn refused(text: &[u8]) -> (usize, ListenFault) {
        match heard_in_lines(text) {
            Err(Error::ListenLine { line, fault }) => (line, fault),
            other => panic!("not refused as a line: {other:?}"),
        }
    }

    #[test]
    fn a_bad_line_is_refused_with_its_number_whatever_the_fault() {
        let good = ONE.as_bytes();
        let with = |bad: &[u8]| [good, b"\n", good, b"\n", bad].concat();

        assert_eq!(
            refused(&with(b"{\"listened_at\":")),
            (3, ListenFault::NotJson)
        );
        assert_eq!(refused(&with(b"[1]")), (3, ListenFault::NotAnObject));
        assert_eq!(
            refused(&with(br#"{"track_metadata":{}}"#)),
            (3, ListenFault::NoMoment)
        );
        assert_eq!(
            refused(&with(br#"{"listened_at":-1,"track_metadata":{}}"#)),
            (3, ListenFault::NoMoment)
        );
        assert_eq!(
            refused(&with(
                br#"{"listened_at":18446744073709551615,"track_metadata":{}}"#
            )),
            (3, ListenFault::NoMoment)
        );
        assert_eq!(
            refused(&with(br#"{"listened_at":5}"#)),
            (3, ListenFault::NoTrackMetadata)
        );
        assert_eq!(
            refused(&with(
                b"{\"listened_at\":5,\"track_metadata\":{\"track_name\":\"\xff\"}}"
            )),
            (3, ListenFault::NotUtf8)
        );
    }
}
