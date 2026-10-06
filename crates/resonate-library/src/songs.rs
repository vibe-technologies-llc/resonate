use std::{cmp::Reverse, time::Duration};

use ahash::{AHashMap, AHashSet};
use rusqlite::{Row, Transaction, params};

use crate::{
    Error, Found, Issued, Mbid, RecordingRelease, Release, Result, StoreOp, UnheldRelease,
    elsewhere, store,
};

const OFFICIAL: &str = "Official";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SongsDue {
    pub(crate) artist: Option<Mbid>,
    pub(crate) groups: Vec<Mbid>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlbumNotHeld {
    pub release: UnheldRelease,
    pub pressing: Option<Mbid>,
}

pub(crate) fn pressing_of(releases: Vec<Release>) -> Option<Release> {
    let mut counted: AHashMap<u32, usize> = AHashMap::new();
    for release in &releases {
        let tracks = release.track_count();
        if tracks > 0 {
            *counted.entry(tracks).or_default() += 1;
        }
    }
    let usual = counted
        .into_iter()
        .max_by_key(|(tracks, pressings)| (*pressings, Reverse(*tracks)))
        .map(|(tracks, _)| tracks)?;

    releases
        .into_iter()
        .filter(|release| release.track_count() == usual)
        .min_by(|one, other| first_out(one).cmp(&first_out(other)))
}

fn first_out(release: &Release) -> (bool, &str, Reverse<usize>, &str) {
    let date = release.date.as_deref().unwrap_or_default();
    let year = date.get(..4).unwrap_or(date);
    (date.is_empty(), year, Reverse(date.len()), date)
}

pub(crate) fn land(
    tx: &Transaction<'_>,
    group: &Mbid,
    pressing: Option<&Release>,
    read: i64,
) -> Result<usize> {
    tx.execute(
        "DELETE FROM discography_songs WHERE release_group = ?1",
        params![group.as_str()],
    )
    .map_err(|source| Error::store(StoreOp::Delete, source))?;

    let mut landed = 0;
    if let Some(pressing) = pressing {
        let mut insert = tx
            .prepare_cached(
                "INSERT OR IGNORE INTO discography_songs
                     (release_group, recording_mbid, release_mbid, release_title, released, kind,
                      disc, position, title, artist, length_ms, words, folded)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            )
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;
        let billed = pressing.credited_as();
        for medium in &pressing.media {
            for track in &medium.tracks {
                let Some(recording) = &track.recording else {
                    continue;
                };
                let artist = track.artist.clone().unwrap_or_else(|| billed.clone());
                landed += insert
                    .execute(params![
                        group.as_str(),
                        recording.as_str(),
                        pressing.id.as_str(),
                        pressing.title,
                        pressing.date,
                        pressing.kind,
                        medium.position,
                        track.position,
                        track.title,
                        artist,
                        track.length.map(|length| length.as_millis() as i64),
                        store::words_of(&track.title),
                        haystack(&[&track.title, &artist, &pressing.title]),
                    ])
                    .map_err(|source| Error::store(StoreOp::Insert, source))?;
            }
        }
    }
    tx.execute(
        "INSERT INTO discography_songs_read (release_group, read, refusals) VALUES (?1, ?2, 0)
         ON CONFLICT(release_group) DO UPDATE SET read = excluded.read, refusals = 0",
        params![group.as_str(), read],
    )
    .map_err(|source| Error::store(StoreOp::Insert, source))?;

    Ok(landed)
}

pub(crate) fn refused(tx: &Transaction<'_>, group: &Mbid, read: i64) -> Result<()> {
    tx.execute(
        "INSERT INTO discography_songs_read (release_group, read, refusals) VALUES (?1, ?2, 1)
         ON CONFLICT(release_group) DO UPDATE SET read = excluded.read, refusals = refusals + 1",
        params![group.as_str(), read],
    )
    .map_err(|source| Error::store(StoreOp::Insert, source))?;
    Ok(())
}

fn haystack(named: &[&str]) -> String {
    let words: Vec<String> = named
        .iter()
        .flat_map(|name| name.split_whitespace())
        .map(store::folded_letters)
        .filter(|word| !word.is_empty())
        .collect();
    format!(" {} ", words.join(" "))
}

pub(crate) fn sought_words(text: &str) -> Vec<String> {
    elsewhere::words_asked(text)
        .iter()
        .flat_map(|word| word.split_whitespace())
        .map(store::folded_letters)
        .filter(|word| !word.is_empty())
        .map(|word| format!(" {word}"))
        .collect()
}

pub(crate) const SONG_COLUMNS: &str = "s.recording_mbid, s.title, s.artist, s.length_ms, \
                                       s.release_mbid, s.release_title, s.released, s.kind, \
                                       s.disc, s.position";

pub(crate) struct RawSong {
    recording: String,
    title: String,
    artist: String,
    length_ms: Option<i64>,
    release: String,
    release_title: String,
    released: Option<String>,
    kind: Option<String>,
    disc: Option<i64>,
    position: Option<i64>,
}

impl RawSong {
    pub(crate) fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            recording: row.get(0)?,
            title: row.get(1)?,
            artist: row.get(2)?,
            length_ms: row.get(3)?,
            release: row.get(4)?,
            release_title: row.get(5)?,
            released: row.get(6)?,
            kind: row.get(7)?,
            disc: row.get(8)?,
            position: row.get(9)?,
        })
    }

    pub(crate) fn into_found(self) -> Result<Found> {
        let release = RecordingRelease {
            id: Mbid::new(&self.release)?,
            title: self.release_title,
            date: self.released,
            disc: self.disc.and_then(|disc| u32::try_from(disc).ok()),
            position: self
                .position
                .and_then(|position| u32::try_from(position).ok()),
            issued: Issued {
                kind: self.kind,
                secondary: Vec::new(),
                status: Some(OFFICIAL.to_owned()),
            },
        };

        Ok(Found {
            recording: Mbid::new(&self.recording)?,
            title: self.title,
            artist: self.artist,
            length: self
                .length_ms
                .and_then(|length| u64::try_from(length).ok())
                .map(Duration::from_millis),
            release: Some(release.clone()),
            releases: vec![release],
        })
    }
}

pub(crate) fn once_each(found: Vec<Found>, at_most: Option<usize>) -> Vec<Found> {
    let mut seen = AHashSet::new();
    found
        .into_iter()
        .filter(|found| {
            seen.insert((
                store::folded_letters(&found.title),
                store::folded_letters(&found.artist),
            ))
        })
        .take(at_most.unwrap_or(usize::MAX))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Medium, ReleaseTrack};

    const PIPER: &str = "6792b6d1-4e65-3c3c-9d20-d08aa1dcfc60";

    fn pressed(id: &str, date: Option<&str>, tracks: u32) -> Release {
        Release {
            id: Mbid::new(id).expect("a well-formed mbid"),
            group: Some(Mbid::new(PIPER).expect("a well-formed mbid")),
            title: "The Piper at the Gates of Dawn".to_owned(),
            credit: Vec::new(),
            date: date.map(str::to_owned),
            country: None,
            label: None,
            catalog_number: None,
            barcode: None,
            kind: Some("Album".to_owned()),
            disambiguation: None,
            has_front_cover: false,
            links: Vec::new(),
            media: vec![Medium {
                position: 1,
                format: None,
                title: None,
                tracks: (1..=tracks)
                    .map(|position| ReleaseTrack {
                        position,
                        number: position.to_string(),
                        title: format!("track {position}"),
                        artist: None,
                        recording: None,
                        track: None,
                        length: None,
                        isrc: None,
                        links: Vec::new(),
                    })
                    .collect(),
            }],
        }
    }

    #[test]
    fn a_groups_songs_are_read_off_its_usual_track_list_as_it_first_came_out() {
        let pressings = vec![
            pressed("83d91898-7763-47d7-b03b-b92132375c47", Some("1967"), 12),
            pressed(
                "5b11f4ce-a62d-471e-81fc-a69a8278c7da",
                Some("1967-08-05"),
                11,
            ),
            pressed(
                "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d",
                Some("1967-07-07"),
                11,
            ),
            pressed("1f715142-a0ef-48c8-9e72-c402e5bad13f", None, 11),
            pressed("6c7cdad8-4daf-3248-b6c1-ea896c2e568e", Some("1967"), 9),
        ];

        let pressing = pressing_of(pressings).expect("a pressing");

        assert_eq!(pressing.id.as_str(), "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d");
        assert!(pressing_of(Vec::new()).is_none());
        assert!(pressing_of(vec![pressed(PIPER, Some("1967"), 0)]).is_none());
    }

    #[test]
    fn a_song_is_found_by_the_start_of_any_word_of_its_title_artist_or_release() {
        let held = haystack(&[
            "Astronomy Domine",
            "Pink Floyd",
            "The Piper at the Gates of Dawn",
        ]);

        assert_eq!(
            held,
            " astronomy domine pink floyd the piper at the gates of dawn "
        );
        for word in sought_words("ASTRO floy piper") {
            assert!(held.contains(&word), "{word}");
        }
        assert!(!held.contains(&sought_words("omine")[0]));
    }
}
