use std::time::{Duration, SystemTime};

use ahash::AHashSet;
use resonate_core::{AlbumId, ReleaseTrackId, WantId};
use rusqlite::{OptionalExtension as _, Transaction, params};

use crate::{
    Column, Error, Mbid, RecordingMatch, RecordingRelease, Release, Result, Search, StoreOp,
    enriched, store,
};

pub const FOUND_ELSEWHERE_AT_MOST: usize = 12;

const FEWEST_LETTERS_ASKED_ELSEWHERE: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sung {
    pub query: String,
    pub tracks: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub recording: Mbid,
    pub title: String,
    pub artist: String,
    pub length: Option<Duration>,
    pub release: Option<RecordingRelease>,
}

pub(crate) fn words_asked(text: &str) -> Vec<String> {
    Search::read(text)
        .clauses
        .iter()
        .filter_map(|clause| clause.lone_word())
        .filter(|word| {
            word.column.is_none_or(|column| {
                matches!(column, Column::Title | Column::Artist | Column::Album)
            })
        })
        .map(|word| word.text.clone())
        .filter(|word| !word.trim().is_empty())
        .collect()
}

pub fn asks_elsewhere(text: &str) -> bool {
    words_asked(text)
        .iter()
        .flat_map(|word| word.chars())
        .filter(|letter| letter.is_alphanumeric())
        .count()
        >= FEWEST_LETTERS_ASKED_ELSEWHERE
}

pub(crate) fn found_among(
    matches: Vec<RecordingMatch>,
    held: impl Fn(&Mbid) -> bool,
) -> Vec<Found> {
    let mut seen = AHashSet::new();
    let mut found = Vec::new();
    for matched in matches {
        if held(&matched.recording) {
            continue;
        }
        let artist = matched.credited_as();
        let named = (
            store::folded_letters(&matched.title),
            store::folded_letters(&artist),
        );
        if !seen.insert(named) {
            continue;
        }
        found.push(Found {
            release: first_released(&matched.releases).cloned(),
            recording: matched.recording,
            title: matched.title,
            artist,
            length: matched.length,
        });
        if found.len() == FOUND_ELSEWHERE_AT_MOST {
            break;
        }
    }

    found
}

pub(crate) fn first_released(releases: &[RecordingRelease]) -> Option<&RecordingRelease> {
    releases
        .iter()
        .min_by(|one, other| match (&one.date, &other.date) {
            (Some(one), Some(other)) => one.cmp(other),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        })
}

pub(crate) fn album_of_release(
    tx: &Transaction<'_>,
    release: &Release,
    now: SystemTime,
) -> Result<AlbumId> {
    let held: Option<i64> = tx
        .query_row(
            "SELECT id FROM albums WHERE mbid = ?1 ORDER BY id LIMIT 1",
            params![release.id.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    if let Some(held) = held {
        return Ok(AlbumId::new(held as u64)?);
    }

    let credited = release.credited_as();
    let owner = match credited.trim() {
        "" => None,
        named => Some(store::artist_named(
            tx,
            named,
            release
                .credit
                .first()
                .and_then(|credit| credit.mbid.as_ref()),
        )?),
    };
    let id: i64 = tx
        .query_row(
            "INSERT INTO albums (title, artist_id, mbid, found_elsewhere)
             VALUES (?1, ?2, ?3, ?4) RETURNING id",
            params![
                release.title,
                owner,
                release.id.as_str(),
                store::to_nanos(now)
            ],
            |row| row.get(0),
        )
        .map_err(|source| Error::store(StoreOp::Insert, source))?;
    let album = AlbumId::new(id as u64)?;
    enriched::land_release(tx, album, release, now)?;

    Ok(album)
}

pub(crate) fn release_track_of(
    tx: &Transaction<'_>,
    album: AlbumId,
    recording: &Mbid,
) -> Result<Option<ReleaseTrackId>> {
    let row: Option<i64> = tx
        .query_row(
            "SELECT id FROM release_tracks
              WHERE album_id = ?1 AND recording_mbid = ?2
              ORDER BY disc, position LIMIT 1",
            params![album.get() as i64, recording.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    row.map(|row| ReleaseTrackId::new(row as u64).map_err(Error::from))
        .transpose()
}

pub(crate) fn want_in(
    tx: &Transaction<'_>,
    release_track: ReleaseTrackId,
    now: SystemTime,
) -> Result<WantId> {
    let row = release_track.get() as i64;
    let known = tx
        .query_row(
            "SELECT 1 FROM release_tracks WHERE id = ?1",
            params![row],
            |_| Ok(()),
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    if known.is_none() {
        return Err(Error::UnknownReleaseTrack(release_track));
    }

    tx.execute(
        "INSERT INTO wants (release_track_id, wanted) VALUES (?1, ?2)
         ON CONFLICT(release_track_id) DO NOTHING",
        params![row, store::to_nanos(now)],
    )
    .map_err(|source| Error::store(StoreOp::Insert, source))?;
    let id: i64 = tx
        .query_row(
            "SELECT id FROM wants WHERE release_track_id = ?1",
            params![row],
            |row| row.get(0),
        )
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    Ok(WantId::new(id as u64)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Credit;

    const ONE: &str = "83d91898-7763-47d7-b03b-b92132375c47";
    const TWO: &str = "5b11f4ce-a62d-471e-81fc-a69a8278c7da";
    const THREE: &str = "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d";

    fn mbid(text: &str) -> Mbid {
        Mbid::new(text).expect("a well-formed mbid")
    }

    fn released(id: &str, date: Option<&str>) -> RecordingRelease {
        RecordingRelease {
            id: mbid(id),
            title: format!("release {id}"),
            date: date.map(str::to_owned),
            disc: None,
            position: None,
        }
    }

    fn matched(
        id: &str,
        title: &str,
        artist: &str,
        releases: Vec<RecordingRelease>,
    ) -> RecordingMatch {
        RecordingMatch {
            recording: mbid(id),
            score: 100,
            title: title.to_owned(),
            credit: vec![Credit {
                name: artist.to_owned(),
                joined_by: String::new(),
                mbid: None,
            }],
            length: None,
            isrcs: Vec::new(),
            releases,
        }
    }

    #[test]
    fn only_the_names_a_search_asks_for_are_asked_elsewhere() {
        assert_eq!(words_asked("echoes pink"), vec!["echoes", "pink"]);
        assert_eq!(
            words_asked("title:echoes artist:\"pink floyd\" year:1971 -live genre:rock"),
            vec!["echoes", "pink floyd"]
        );
        assert!(words_asked("year:1971 is:hires").is_empty());
        assert!(words_asked("lyrics:\"overhead the albatross\"").is_empty());
    }

    #[test]
    fn a_search_is_asked_elsewhere_only_once_it_names_enough_to_find() {
        assert!(asks_elsewhere("echoes"));
        assert!(asks_elsewhere("ab c"));
        assert!(!asks_elsewhere("ab"));
        assert!(!asks_elsewhere("year:1971"));
        assert!(!asks_elsewhere(""));
    }

    #[test]
    fn a_song_found_twice_is_offered_once_and_what_the_catalog_holds_not_at_all() {
        let found = found_among(
            vec![
                matched(ONE, "Echoes", "Pink Floyd", Vec::new()),
                matched(TWO, "echoes", "Pink Floyd", Vec::new()),
                matched(THREE, "Echoes (live)", "Pink Floyd", Vec::new()),
            ],
            |recording| recording.as_str() == THREE,
        );

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].recording.as_str(), ONE);
        assert_eq!(found[0].artist, "Pink Floyd");
    }

    #[test]
    fn a_song_is_wanted_from_the_release_it_first_came_out_on() {
        let releases = vec![
            released(ONE, Some("1995-05-01")),
            released(TWO, None),
            released(THREE, Some("1971-10-30")),
        ];

        assert_eq!(
            first_released(&releases).map(|release| release.id.as_str()),
            Some(THREE)
        );
        assert_eq!(
            first_released(&[released(TWO, None)]).map(|release| release.id.as_str()),
            Some(TWO)
        );
        assert!(first_released(&[]).is_none());
    }
}
