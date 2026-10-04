use std::time::Duration;

use resonate_core::{Isrc, Mbid};
use rusqlite::{Connection, OptionalExtension as _, params};

use crate::{Error, Found, Recording, Result, StoreOp, share};

const SECURE: &str = "https://";
const PLAIN: &str = "http://";
const SPOTIFY_TRACK: &str = "spotify:track:";
const SPOTIFY_TRACK_PAGE: &str = "https://open.spotify.com/track/";
const SONG_LINK_PAGES: &str = "https://song.link/";
const HELD_BY_RECORDING: &str = "SELECT title, artist FROM tracks WHERE mbid = ?1 LIMIT 1";
const HELD_BY_ISRC: &str = "SELECT title, artist FROM tracks WHERE isrc = ?1 LIMIT 1";
pub(crate) const LENGTHS_AGREE_WITHIN: Duration = Duration::from_secs(5);
const SOUNDCLOUD_PAGES_NAMING_NO_SONG: [&str; 9] = [
    "sets",
    "likes",
    "tracks",
    "albums",
    "reposts",
    "followers",
    "following",
    "popular-tracks",
    "comments",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SongLink {
    MusicBrainz(Mbid),
    Deezer(u64),
    Elsewhere(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkNames {
    pub isrcs: Vec<Isrc>,
    pub length: Option<Duration>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Linked {
    Held {
        title: String,
        artist: Option<String>,
    },
    Found(Box<Found>),
    Unnamed,
}

impl SongLink {
    pub fn read(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() || text.contains(char::is_whitespace) {
            return None;
        }
        if let Some(id) = text.strip_prefix(SPOTIFY_TRACK) {
            return names_an_id(id).then(|| Self::Elsewhere(format!("{SPOTIFY_TRACK_PAGE}{id}")));
        }

        let rest = text
            .strip_prefix(SECURE)
            .or_else(|| text.strip_prefix(PLAIN))?;
        let rest = rest.split('#').next().unwrap_or(rest);
        let (address, query) = rest.split_once('?').unwrap_or((rest, ""));
        let (host, path) = address.split_once('/').unwrap_or((address, ""));
        let host = host.to_ascii_lowercase();
        let host = host.strip_prefix("www.").unwrap_or(&host);
        let segments: Vec<&str> = path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();

        if host == "musicbrainz.org" || host.ends_with(".musicbrainz.org") {
            return match segments.as_slice() {
                ["recording", id, ..] => Mbid::new(id).ok().map(Self::MusicBrainz),
                _ => None,
            };
        }
        if host == "deezer.com" {
            let id = segments
                .windows(2)
                .find(|pair| pair[0] == "track")
                .map(|pair| pair[1])?;
            return id.parse().ok().map(Self::Deezer);
        }

        names_a_song(host, &segments, query).then(|| Self::Elsewhere(format!("{SECURE}{rest}")))
    }

    pub fn page(&self) -> Option<String> {
        match self {
            Self::Elsewhere(url) if url.starts_with(SONG_LINK_PAGES) => Some(url.clone()),
            Self::Elsewhere(url) => Some(format!(
                "{SONG_LINK_PAGES}{}",
                share::escaped_for_a_path(url)
            )),
            Self::MusicBrainz(_) | Self::Deezer(_) => None,
        }
    }
}

pub fn is_a_song_link(text: &str) -> bool {
    SongLink::read(text).is_some()
}

fn names_a_song(host: &str, segments: &[&str], query: &str) -> bool {
    let after = |page: &str| {
        segments
            .windows(2)
            .any(|pair| pair[0] == page && names_an_id(pair[1]))
    };
    let first = segments.first().copied();

    match host {
        "open.spotify.com" | "play.spotify.com" | "tidal.com" | "listen.tidal.com" => {
            after("track")
        }
        "music.apple.com" | "itunes.apple.com" => {
            asked_for(query, "i").is_some_and(names_an_id) || segments.contains(&"song")
        }
        "youtube.com" | "m.youtube.com" | "music.youtube.com" => {
            first == Some("watch") && asked_for(query, "v").is_some_and(names_an_id)
        }
        "youtu.be" => segments.len() == 1 && names_an_id(segments[0]),
        "soundcloud.com" | "m.soundcloud.com" => match segments {
            [_, track] => !SOUNDCLOUD_PAGES_NAMING_NO_SONG.contains(track),
            _ => false,
        },
        "song.link" => !segments.is_empty(),
        "play.anghami.com" => after("song"),
        "boomplay.com" => after("songs"),
        "audiomack.com" => segments.get(1) == Some(&"song"),
        _ if host.starts_with("music.amazon.") => {
            first == Some("tracks") || asked_for(query, "trackAsin").is_some()
        }
        _ if host.starts_with("music.yandex.") => after("track"),
        _ => false,
    }
}

fn names_an_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn asked_for<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
}

pub(crate) enum HeldBy<'a> {
    Recording(&'a Mbid),
    Isrc(&'a Isrc),
}

pub(crate) fn held_as(connection: &Connection, held_by: HeldBy<'_>) -> Result<Option<Linked>> {
    let (sql, key) = match held_by {
        HeldBy::Recording(recording) => (HELD_BY_RECORDING, recording.as_str()),
        HeldBy::Isrc(isrc) => (HELD_BY_ISRC, isrc.as_str()),
    };

    connection
        .query_row(sql, params![key], |row| {
            Ok(Linked::Held {
                title: row.get(0)?,
                artist: row.get(1)?,
            })
        })
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))
}

pub(crate) fn the_take_linked(
    takes: Vec<Recording>,
    length: Option<Duration>,
) -> Option<Recording> {
    let Some(length) = length else {
        return takes.into_iter().next();
    };
    let apart = |take: &Recording| take.length.map(|held| held.abs_diff(length));

    takes
        .into_iter()
        .filter(|take| apart(take).is_none_or(|apart| apart <= LENGTHS_AGREE_WITHIN))
        .min_by_key(|take| apart(take).unwrap_or(LENGTHS_AGREE_WITHIN))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECORDING: &str = "a6ab8ab1-5d0a-4c3c-a1a7-d6e8e5ec33a0";

    fn elsewhere(url: &str) -> Option<SongLink> {
        Some(SongLink::Elsewhere(url.to_owned()))
    }

    #[test]
    fn a_link_to_a_song_on_a_service_is_read_as_one() {
        assert_eq!(
            SongLink::read("https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT?si=0a1b2c"),
            elsewhere("https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT?si=0a1b2c")
        );
        assert_eq!(
            SongLink::read("spotify:track:4cOdK2wGLETKBW3PvgPWqT"),
            elsewhere("https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT")
        );
        assert_eq!(
            SongLink::read("  https://tidal.com/browse/track/491206012  "),
            elsewhere("https://tidal.com/browse/track/491206012")
        );
        assert_eq!(
            SongLink::read("https://song.link/s/4cOdK2wGLETKBW3PvgPWqT"),
            elsewhere("https://song.link/s/4cOdK2wGLETKBW3PvgPWqT")
        );
        assert!(
            SongLink::read("https://music.apple.com/us/album/x/1559523357?i=1559523359").is_some()
        );
        assert!(SongLink::read("https://music.youtube.com/watch?v=dQw4w9WgXcQ").is_some());
        assert!(SongLink::read("https://youtu.be/dQw4w9WgXcQ").is_some());
        assert!(
            SongLink::read("https://soundcloud.com/rick-astley-official/never-gonna-give-you-up-4")
                .is_some()
        );
    }

    #[test]
    fn a_deezer_track_and_a_musicbrainz_recording_are_read_by_their_ids() {
        assert_eq!(
            SongLink::read("https://www.deezer.com/en/track/781592622"),
            Some(SongLink::Deezer(781_592_622))
        );
        assert_eq!(
            SongLink::read(&format!("https://musicbrainz.org/recording/{RECORDING}")),
            Some(SongLink::MusicBrainz(
                Mbid::new(RECORDING).expect("a well-formed mbid")
            ))
        );
    }

    #[test]
    fn words_albums_artists_and_other_pages_are_not_song_links() {
        for text in [
            "never gonna give you up",
            "https://open.spotify.com/album/6eUW0wxWtzkFdaEFsTJto6",
            "https://open.spotify.com/artist/0gxyHStUsqpMadRV0Di1Qt",
            "https://www.deezer.com/album/12345",
            "https://soundcloud.com/rick-astley-official/sets/whenever",
            "https://soundcloud.com/rick-astley-official",
            "https://www.youtube.com/@RickAstleyYT",
            "https://example.com/track/1",
            "https://musicbrainz.org/release/a6ab8ab1-5d0a-4c3c-a1a7-d6e8e5ec33a0",
            "https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT and more",
        ] {
            assert_eq!(SongLink::read(text), None, "{text}");
        }
    }

    fn take(id: &str, seconds: Option<u64>) -> Recording {
        Recording {
            id: Mbid::new(id).expect("a well-formed mbid"),
            title: "Never Gonna Give You Up".to_owned(),
            credit: Vec::new(),
            length: seconds.map(Duration::from_secs),
            isrcs: Vec::new(),
            releases: Vec::new(),
        }
    }

    #[test]
    fn of_the_takes_an_isrc_names_the_one_whose_length_agrees_is_taken() {
        let video = take("11111111-1111-4111-8111-111111111111", Some(232));
        let single = take("22222222-2222-4222-8222-222222222222", Some(213));
        let untimed = take("33333333-3333-4333-8333-333333333333", None);

        assert_eq!(
            the_take_linked(
                vec![video.clone(), untimed.clone(), single.clone()],
                Some(Duration::from_secs(214))
            ),
            Some(single)
        );
        assert_eq!(
            the_take_linked(vec![video.clone()], Some(Duration::from_secs(214))),
            None
        );
        assert_eq!(
            the_take_linked(vec![video.clone(), untimed], None),
            Some(video)
        );
    }
}
