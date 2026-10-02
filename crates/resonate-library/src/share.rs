use std::time::Duration;

use resonate_core::TrackId;
use rusqlite::{Connection, OptionalExtension as _, params};

use crate::{
    Error, Isrc, Link, Mbid, Reference, Relation, Result, Service, StoreOp, StreamAsked, db::Inner,
    store,
};

const SONG_LINK: &str = "https://song.link/";
const ALBUM_LINK: &str = "https://album.link/";
const MUSICBRAINZ_RECORDING: &str = "https://musicbrainz.org/recording/";
const HEX: &[u8; 16] = b"0123456789ABCDEF";

const RELATIONS_SONG_LINK_TAKES: [Relation; 5] = [
    Relation::Streaming,
    Relation::FreeStreaming,
    Relation::Youtube,
    Relation::Bandcamp,
    Relation::Soundcloud,
];

const SERVICES_SONG_LINK_RESOLVES: [Service; 10] = [
    Service::Spotify,
    Service::Tidal,
    Service::AppleMusic,
    Service::Deezer,
    Service::AmazonMusic,
    Service::Youtube,
    Service::YoutubeMusic,
    Service::Soundcloud,
    Service::Bandcamp,
    Service::Qobuz,
];

const WHAT_A_SHARE_SAYS: &str = "SELECT t.title, t.artist, t.mbid, a.title, a.year,
            t.isrc, t.duration, t.sample_rate
       FROM tracks t LEFT JOIN albums a ON a.id = t.album_id
      WHERE t.id = ?1";

const THE_RECORDINGS_OWN_LINKS: &str = "SELECT k.relation, k.provider, k.url
       FROM release_track_links k JOIN release_tracks r ON r.id = k.release_track_id
      WHERE r.track_id = ?1
      ORDER BY k.url";

const THE_ALBUMS_LINKS: &str = "SELECT k.relation, k.provider, k.url
       FROM album_links k JOIN tracks t ON t.album_id = k.album_id
      WHERE t.id = ?1
      ORDER BY k.url";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shared {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<i32>,
    pub recording: Option<Mbid>,
    pub isrc: Option<Isrc>,
    pub length: Option<Duration>,
    pub links: Vec<Link>,
}

impl Shared {
    pub fn written(&self) -> Option<String> {
        self.through_song_link()
            .map(|link| {
                ShortForm::of(&link.url).map_or_else(
                    || format!("{SONG_LINK}{}", escaped_for_a_path(&link.url)),
                    |short| short.written(),
                )
            })
            .or_else(|| {
                self.recording
                    .as_ref()
                    .map(|recording| format!("{MUSICBRAINZ_RECORDING}{recording}"))
            })
    }

    pub fn goes_through_song_link(&self) -> bool {
        self.through_song_link().is_some()
    }

    pub fn asked(&self) -> StreamAsked {
        StreamAsked {
            title: self.title.clone(),
            artist: self.artist.clone(),
            isrc: self.isrc.clone(),
            length: self.length,
        }
    }

    pub fn streamed_where_asked(mut self, reference: &dyn Reference) -> Self {
        if self.goes_through_song_link() {
            return self;
        }
        match reference.streamed_at(&self.asked()) {
            Ok(Some(found)) if resolved_by_song_link(&found).is_some() => {
                self.links.insert(0, found);
            }
            Ok(_) => tracing::debug!(
                title = self.title,
                "no service song.link opens holds the shared track"
            ),
            Err(error) => {
                tracing::warn!(%error, "the service a share would open could not be asked");
            }
        }
        self
    }

    fn through_song_link(&self) -> Option<&Link> {
        self.links
            .iter()
            .find(|link| resolved_by_song_link(link).is_some())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Song,
    Album,
}

impl Page {
    fn of(kind: &str) -> Option<Self> {
        match kind {
            "track" | "song" => Some(Self::Song),
            "album" => Some(Self::Album),
            _ => None,
        }
    }

    const fn host(self) -> &'static str {
        match self {
            Self::Song => SONG_LINK,
            Self::Album => ALBUM_LINK,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShortService {
    Spotify,
    Deezer,
    Tidal,
    AppleMusic,
    Youtube,
}

impl ShortService {
    const fn letter(self) -> char {
        match self {
            Self::Spotify => 's',
            Self::Deezer => 'd',
            Self::Tidal => 't',
            Self::AppleMusic => 'i',
            Self::Youtube => 'y',
        }
    }

    fn at(host: &str) -> Option<Self> {
        match host {
            "open.spotify.com" | "play.spotify.com" => Some(Self::Spotify),
            "deezer.com" => Some(Self::Deezer),
            "tidal.com" | "listen.tidal.com" => Some(Self::Tidal),
            "music.apple.com" | "itunes.apple.com" => Some(Self::AppleMusic),
            "youtube.com" | "m.youtube.com" | "music.youtube.com" | "youtu.be" => {
                Some(Self::Youtube)
            }
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ShortForm<'a> {
    service: ShortService,
    page: Page,
    id: &'a str,
}

impl<'a> ShortForm<'a> {
    fn of(url: &'a str) -> Option<Self> {
        let rest = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))?;
        let rest = rest.split('#').next().unwrap_or(rest);
        let (address, query) = rest.split_once('?').unwrap_or((rest, ""));
        let (host, path) = address.split_once('/').unwrap_or((address, ""));
        let host = host.to_ascii_lowercase();
        let service = ShortService::at(host.strip_prefix("www.").unwrap_or(&host))?;
        let segments: Vec<&str> = path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();

        let (page, id) = match service {
            ShortService::Spotify | ShortService::Deezer | ShortService::Tidal => segments
                .windows(2)
                .find_map(|pair| Some((Page::of(pair[0])?, pair[1])))?,
            ShortService::AppleMusic => match asked_for(query, "i") {
                Some(track) => (Page::Song, track),
                None => {
                    let page = segments.iter().find_map(|segment| Page::of(segment))?;
                    let last = segments.last()?;
                    (page, last.strip_prefix("id").unwrap_or(last))
                }
            },
            ShortService::Youtube if host == "youtu.be" => (Page::Song, *segments.first()?),
            ShortService::Youtube => {
                (segments.first() == Some(&"watch")).then_some(())?;
                (Page::Song, asked_for(query, "v")?)
            }
        };
        let names_one = !id.is_empty()
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');

        names_one.then_some(Self { service, page, id })
    }

    fn written(&self) -> String {
        format!("{}{}/{}", self.page.host(), self.service.letter(), self.id)
    }
}

fn asked_for<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
}

pub(crate) fn escaped_for_a_path(url: &str) -> String {
    let mut escaped = String::with_capacity(url.len());
    for byte in url.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                escaped.push(char::from(byte));
            }
            other => {
                escaped.push('%');
                escaped.push(char::from(HEX[(other >> 4) as usize]));
                escaped.push(char::from(HEX[(other & 0x0f) as usize]));
            }
        }
    }
    escaped
}

pub(crate) fn shareable(inner: &Inner, track: TrackId) -> Result<Option<Shared>> {
    let id = track.get() as i64;

    inner.read(|connection| {
        let Some(row) = connection
            .query_row(WHAT_A_SHARE_SAYS, params![id], |row| {
                Ok(Named {
                    title: row.get(0)?,
                    artist: row.get(1)?,
                    recording: row.get(2)?,
                    album: row.get(3)?,
                    year: row.get(4)?,
                    isrc: row.get(5)?,
                    frames: row.get(6)?,
                    rate: row.get(7)?,
                })
            })
            .optional()
            .map_err(|source| Error::store(StoreOp::Query, source))?
        else {
            return Ok(None);
        };

        let mut links =
            in_the_order_a_share_prefers(linked(connection, THE_RECORDINGS_OWN_LINKS, id)?);
        links.extend(in_the_order_a_share_prefers(linked(
            connection,
            THE_ALBUMS_LINKS,
            id,
        )?));

        Ok(Some(Shared {
            title: row.title,
            artist: row.artist,
            album: row.album,
            year: row.year.and_then(|year| i32::try_from(year).ok()),
            recording: row.recording.as_deref().map(Mbid::new).transpose()?,
            isrc: store::isrc_in(row.isrc.as_deref()),
            length: length_of(row.frames, row.rate),
            links,
        }))
    })
}

struct Named {
    title: String,
    artist: Option<String>,
    recording: Option<String>,
    album: Option<String>,
    year: Option<i64>,
    isrc: Option<String>,
    frames: Option<i64>,
    rate: i64,
}

fn length_of(frames: Option<i64>, rate: i64) -> Option<Duration> {
    let frames = u64::try_from(frames?).ok()?;
    let rate = u64::try_from(rate).ok().filter(|rate| *rate > 0)?;

    Some(Duration::from_secs_f64(frames as f64 / rate as f64))
}

fn linked(connection: &Connection, sql: &str, track: i64) -> Result<Vec<Link>> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let held = statement
        .query_map(params![track], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    held.into_iter()
        .map(|(relation, provider, url)| {
            Ok(Link {
                relation: store::relation_of(relation)?,
                service: store::service_of(provider)?,
                url,
            })
        })
        .collect()
}

fn in_the_order_a_share_prefers(mut links: Vec<Link>) -> Vec<Link> {
    links.sort_by_key(|link| resolved_by_song_link(link).unwrap_or(usize::MAX));
    links
}

fn resolved_by_song_link(link: &Link) -> Option<usize> {
    if !RELATIONS_SONG_LINK_TAKES.contains(&link.relation) {
        return None;
    }

    SERVICES_SONG_LINK_RESOLVES
        .iter()
        .position(|service| *service == link.service)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPOTIFY: &str = "https://open.spotify.com/track/1a2b3c";
    const WIKIPEDIA: &str = "https://en.wikipedia.org/wiki/Echoes";
    const RECORDING: &str = "b1a9c0de-1111-4222-8333-444455556666";

    fn echoes() -> Shared {
        Shared {
            title: "Echoes".to_owned(),
            artist: Some("Pink Floyd".to_owned()),
            album: Some("Meddle".to_owned()),
            year: Some(1971),
            recording: None,
            isrc: None,
            length: None,
            links: Vec::new(),
        }
    }

    fn mbid() -> Mbid {
        Mbid::new(RECORDING).expect("the test names a recording")
    }

    #[test]
    fn a_share_with_nothing_song_link_can_open_is_nothing() {
        assert_eq!(echoes().written(), None);
    }

    #[test]
    fn a_service_link_is_handed_to_song_link_by_its_own_id_and_a_recording_is_not() {
        let shared = Shared {
            recording: Some(mbid()),
            links: vec![Link {
                relation: Relation::Streaming,
                service: Service::Spotify,
                url: SPOTIFY.to_owned(),
            }],
            ..echoes()
        };

        assert_eq!(
            shared.written().as_deref(),
            Some("https://song.link/s/1a2b3c")
        );
    }

    #[test]
    fn every_service_song_link_has_a_short_page_for_is_written_by_its_id() {
        for (url, written) in [
            (
                "https://open.spotify.com/intl-de/track/4uLU6hMCjMI75M1A2tKUQC?si=abc",
                "https://song.link/s/4uLU6hMCjMI75M1A2tKUQC",
            ),
            (
                "https://open.spotify.com/album/6N9PS4QXF1D0OWPk0Sxtb4",
                "https://album.link/s/6N9PS4QXF1D0OWPk0Sxtb4",
            ),
            (
                "https://www.deezer.com/en/track/781592622",
                "https://song.link/d/781592622",
            ),
            (
                "https://www.deezer.com/album/119606",
                "https://album.link/d/119606",
            ),
            (
                "https://tidal.com/browse/track/77640618",
                "https://song.link/t/77640618",
            ),
            (
                "https://listen.tidal.com/album/77640617",
                "https://album.link/t/77640617",
            ),
            (
                "https://music.apple.com/us/album/never-gonna/1559523357?i=1559523359",
                "https://song.link/i/1559523359",
            ),
            (
                "https://music.apple.com/us/album/3-originals/1559523357",
                "https://album.link/i/1559523357",
            ),
            (
                "https://itunes.apple.com/gb/album/meddle/id1065975633",
                "https://album.link/i/1065975633",
            ),
            (
                "https://music.apple.com/us/song/never-gonna/1559523359",
                "https://song.link/i/1559523359",
            ),
            (
                "https://www.youtube.com/watch?feature=share&v=dQw4w9WgXcQ",
                "https://song.link/y/dQw4w9WgXcQ",
            ),
            (
                "https://music.youtube.com/watch?v=dQw4w9WgXcQ",
                "https://song.link/y/dQw4w9WgXcQ",
            ),
            (
                "https://youtu.be/dQw4w9WgXcQ",
                "https://song.link/y/dQw4w9WgXcQ",
            ),
        ] {
            assert_eq!(
                ShortForm::of(url).map(|short| short.written()).as_deref(),
                Some(written),
                "{url}"
            );
        }
    }

    #[test]
    fn a_service_with_no_short_page_or_a_url_naming_no_id_is_handed_over_whole() {
        for url in [
            "https://music.amazon.com/albums/B00ZZZ?trackAsin=B01",
            "https://pinkfloyd.bandcamp.com/track/echoes",
            "https://open.spotify.com/artist/0k17h0D3J5VfsdmQ1iZtE9",
            "https://open.spotify.com/track/",
            "https://www.youtube.com/channel/UC1",
            "https://open.spotify.com/track/1a2b%2F..",
        ] {
            assert_eq!(ShortForm::of(url), None, "{url}");
        }

        let shared = Shared {
            links: vec![Link {
                relation: Relation::Streaming,
                service: Service::Qobuz,
                url: "https://open.qobuz.com/track/1?x=y".to_owned(),
            }],
            ..echoes()
        };
        assert_eq!(
            shared.written().as_deref(),
            Some("https://song.link/https%3A%2F%2Fopen.qobuz.com%2Ftrack%2F1%3Fx%3Dy")
        );
    }

    #[test]
    fn a_link_song_link_cannot_resolve_is_passed_over_for_the_recording() {
        let shared = Shared {
            recording: Some(mbid()),
            links: vec![Link {
                relation: Relation::Wikipedia,
                service: Service::Wikipedia,
                url: WIKIPEDIA.to_owned(),
            }],
            ..echoes()
        };

        assert_eq!(
            shared.written().as_deref(),
            Some("https://musicbrainz.org/recording/b1a9c0de-1111-4222-8333-444455556666")
        );
    }

    #[test]
    fn the_services_are_preferred_in_the_order_they_are_declared() {
        let deezer = Link {
            relation: Relation::Streaming,
            service: Service::Deezer,
            url: "https://www.deezer.com/track/1".to_owned(),
        };
        let spotify = Link {
            relation: Relation::Streaming,
            service: Service::Spotify,
            url: SPOTIFY.to_owned(),
        };
        let ordered = in_the_order_a_share_prefers(vec![deezer, spotify.clone()]);

        assert_eq!(ordered.first(), Some(&spotify));
        assert_eq!(resolved_by_song_link(&spotify), Some(0));
    }
}
