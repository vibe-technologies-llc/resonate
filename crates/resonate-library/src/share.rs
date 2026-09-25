use resonate_core::TrackId;
use rusqlite::{Connection, OptionalExtension as _, params};

use crate::{Error, Link, Mbid, Relation, Result, Service, StoreOp, db::Inner, store};

const SONG_LINK: &str = "https://song.link/";
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

const WHAT_A_SHARE_SAYS: &str = "SELECT t.title, t.artist, t.mbid, a.title, a.year
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
    pub links: Vec<Link>,
}

impl Shared {
    pub fn written(&self) -> Option<String> {
        self.links
            .iter()
            .find(|link| resolved_by_song_link(link).is_some())
            .map(|link| format!("{SONG_LINK}{}", escaped_for_a_path(&link.url)))
            .or_else(|| {
                self.recording
                    .as_ref()
                    .map(|recording| format!("{MUSICBRAINZ_RECORDING}{recording}"))
            })
    }
}

fn escaped_for_a_path(url: &str) -> String {
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
    fn a_service_link_is_handed_to_song_link_escaped_and_a_recording_is_not() {
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
            Some("https://song.link/https%3A%2F%2Fopen.spotify.com%2Ftrack%2F1a2b3c")
        );
    }

    #[test]
    fn a_query_in_a_service_url_stays_part_of_the_path_song_link_reads() {
        let shared = Shared {
            links: vec![Link {
                relation: Relation::Streaming,
                service: Service::AppleMusic,
                url: "https://music.apple.com/us/album/time/1?i=2".to_owned(),
            }],
            ..echoes()
        };

        assert_eq!(
            shared.written().as_deref(),
            Some("https://song.link/https%3A%2F%2Fmusic.apple.com%2Fus%2Falbum%2Ftime%2F1%3Fi%3D2")
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
