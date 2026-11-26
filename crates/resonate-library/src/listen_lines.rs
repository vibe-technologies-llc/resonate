use std::{
    io::{self, BufRead},
    str,
    time::{Duration, UNIX_EPOCH},
};

use resonate_core::{Mbid, Recording};

use crate::{Billed, Error, HeardElsewhere, HeardOnce, Result, json::Json};

const BYTE_ORDER_MARK: &[u8] = "\u{feff}".as_bytes();

const NAMED_AS: &str = "resonate";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListenFault {
    NotUtf8,
    NotJson,
    NotAnObject,
    NoMoment,
    NoTrackMetadata,
}

pub fn heard_in_lines(bytes: &[u8]) -> Result<Vec<HeardElsewhere>> {
    let mut heard = Vec::new();

    each_listen_in_bytes(bytes, &mut |listen| {
        heard.push(listen);
        Ok(())
    })?;
    Ok(heard)
}

pub(crate) fn each_listen_in_bytes(
    bytes: &[u8],
    each: &mut dyn FnMut(HeardElsewhere) -> Result<()>,
) -> Result<()> {
    for (index, raw) in bytes.split(|byte| *byte == b'\n').enumerate() {
        if let Some(heard) = listen_on(index + 1, raw)? {
            each(heard)?;
        }
    }
    Ok(())
}

pub(crate) fn each_listen_read(
    mut read: impl BufRead,
    unread: &dyn Fn(io::Error) -> Error,
    each: &mut dyn FnMut(HeardElsewhere) -> Result<()>,
) -> Result<()> {
    let mut raw = Vec::new();
    let mut number = 0;
    loop {
        raw.clear();
        if read.read_until(b'\n', &mut raw).map_err(unread)? == 0 {
            return Ok(());
        }
        number += 1;

        let line = raw.strip_suffix(b"\n").unwrap_or(&raw);

        if let Some(heard) = listen_on(number, line)? {
            each(heard)?;
        }
    }
}

fn listen_on(number: usize, raw: &[u8]) -> Result<Option<HeardElsewhere>> {
    let raw = match number {
        1 => raw.strip_prefix(BYTE_ORDER_MARK).unwrap_or(raw),
        _ => raw,
    };
    let refused = |fault| Error::ListenLine {
        line: number,
        fault,
    };

    let text = str::from_utf8(raw).map_err(|_| refused(ListenFault::NotUtf8))?;

    if text.trim().is_empty() {
        return Ok(None);
    }

    let json = Json::read(text).ok_or(refused(ListenFault::NotJson))?;

    listen_in(&json).map_err(refused)
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

pub fn line_of(heard: &HeardOnce) -> String {
    let mut told = Object::default();

    if let Some(billed) = &heard.billed {
        told.text(
            "recording_mbid",
            billed.recording.as_ref().map(|mbid| mbid.as_str()),
        );
        told.text(
            "release_mbid",
            billed.release.as_ref().map(|mbid| mbid.as_str()),
        );
        told.text(
            "release_group_mbid",
            billed.release_group.as_ref().map(|mbid| mbid.as_str()),
        );
        if let Some(artist) = &billed.artist_mbid {
            told.raw("artist_mbids", &format!("[{}]", quoted(artist.as_str())));
        }
        told.text("isrc", billed.isrc.as_ref().map(|isrc| isrc.as_str()));
        told.number("tracknumber", billed.number.map(u64::from));
        told.number("duration_ms", billed.length.map(milliseconds));
    }
    told.number("listened_ms", Some(milliseconds(heard.heard)));
    told.raw(
        "heard_elsewhere",
        if heard.elsewhere { "true" } else { "false" },
    );
    told.text("origin_url", Some(&heard.location));
    told.text("media_player", Some(NAMED_AS));
    told.text("submission_client", Some(NAMED_AS));
    told.text("submission_client_version", Some(env!("CARGO_PKG_VERSION")));

    let mut metadata = Object::default();

    if let Some(Billed {
        title,
        artist,
        album,
        ..
    }) = &heard.billed
    {
        metadata.text("artist_name", Some(artist));
        metadata.text("track_name", Some(title));
        metadata.text("release_name", album.as_deref());
    }
    metadata.raw("additional_info", &told.closed());

    let mut listen = Object::default();
    let at = heard
        .began
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());

    listen.number("listened_at", Some(at));
    listen.raw("track_metadata", &metadata.closed());
    listen.closed()
}

fn milliseconds(span: Duration) -> u64 {
    u64::try_from(span.as_millis()).unwrap_or(u64::MAX)
}

#[derive(Default)]
struct Object(Vec<String>);

impl Object {
    fn raw(&mut self, name: &str, value: &str) {
        self.0.push(format!("{}:{value}", quoted(name)));
    }

    fn text(&mut self, name: &str, value: Option<&str>) {
        if let Some(value) = value {
            self.raw(name, &quoted(value));
        }
    }

    fn number(&mut self, name: &str, value: Option<u64>) {
        if let Some(value) = value {
            self.raw(name, &value.to_string());
        }
    }

    fn closed(&self) -> String {
        format!("{{{}}}", self.0.join(","))
    }
}

fn quoted(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);

    quoted.push('"');
    for letter in text.chars() {
        match letter {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            control if u32::from(control) < 0x20 => {
                quoted.push_str(&format!("\\u{:04x}", u32::from(control)));
            }
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heard(billed: Option<Billed>) -> HeardOnce {
        HeardOnce {
            began: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            heard: Duration::from_millis(61_500),
            elsewhere: false,
            billed,
            location: "file:///music/a%22b.flac".to_owned(),
        }
    }

    fn billed_as(title: &str) -> Billed {
        Billed {
            title: title.to_owned(),
            artist: "Low".to_owned(),
            album: Some("Words".to_owned()),
            recording: Some(Mbid::new("0a1b2c3d-1111-4222-8333-444455556666").expect("an mbid")),
            release: None,
            release_group: None,
            artist_mbid: None,
            isrc: None,
            number: Some(3),
            length: Some(Duration::from_secs(185)),
        }
    }

    #[test]
    fn a_listen_held_is_written_as_listenbrainz_writes_one_with_its_names_and_ids() {
        let line = line_of(&heard(Some(billed_as("Say \"hi\"\n"))));

        assert!(line.starts_with("{\"listened_at\":1700000000,\"track_metadata\":{"));
        assert!(line.contains("\"artist_name\":\"Low\",\"track_name\":\"Say \\\"hi\\\"\\n\",\"release_name\":\"Words\""));
        assert!(line.contains("\"recording_mbid\":\"0a1b2c3d-1111-4222-8333-444455556666\""));
        assert!(line.contains("\"tracknumber\":3,\"duration_ms\":185000,\"listened_ms\":61500"));
        assert!(line.contains("\"heard_elsewhere\":false"));
        assert!(!line.contains('\n'));
        assert!(line.ends_with("}}}"));
    }

    #[test]
    fn a_listen_of_a_song_no_longer_held_names_only_where_it_was() {
        let line = line_of(&heard(None));

        assert!(!line.contains("track_name"));
        assert!(line.contains("\"origin_url\":\"file:///music/a%22b.flac\""));
    }

    #[test]
    fn every_control_character_is_escaped() {
        assert_eq!(quoted("a\u{1}b\\"), "\"a\\u0001b\\\\\"");
    }

    #[test]
    fn a_line_written_reads_back_as_the_listen_it_was_and_a_nameless_one_is_passed_over() {
        let written = [
            line_of(&heard(Some(billed_as("Say \"hi\"")))),
            line_of(&heard(None)),
        ]
        .join("\n");

        let read = heard_in_lines(written.as_bytes()).expect("reads");

        assert_eq!(read.len(), 1);
        assert_eq!(read[0].at, UNIX_EPOCH + Duration::from_secs(1_700_000_000));
        assert_eq!(read[0].recording.title, "Say \"hi\"");
        assert_eq!(read[0].recording.album, "Words");
        assert_eq!(read[0].recording.length, Some(Duration::from_secs(185)));
        assert!(read[0].recording.mbid.is_some());
    }

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
