use resonate_core::{Mbid, folded_letters};
use resonate_library::{LookupOp, Reference, Release};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    Error, MobilePlayer, Result, StoreOp, catalog::Catalog, collections::local_match,
    features::SearchSong,
};

const SONGS_KEPT: &str =
    "SELECT recording, title, artist, artist_id, album, release, release_group,
        disc, position, duration_ms, isrcs
    FROM release_songs WHERE album_key = ?1 ORDER BY disc, position";

const SONGS_HELD: &str = "SELECT coalesce(recording, ''), title, disc_number, track_number
    FROM documents WHERE album_key = ?1 AND missing = 0";

const RELEASE_OF_THE_ALBUM: &str = "SELECT coalesce(
        (SELECT release FROM album_art WHERE album_key = ?1),
        (SELECT min(release) FROM documents WHERE album_key = ?1 AND missing = 0)
    )";

const ANY_SONG_KEPT: &str = "SELECT EXISTS (SELECT 1 FROM release_songs WHERE album_key = ?1)";

#[uniffi::export]
impl MobilePlayer {
    pub fn album_songs_not_held(&self, key: String) -> Result<Vec<SearchSong>> {
        self.catalog.album_songs_not_held(&key)
    }

    pub fn learn_album_songs(&self, key: String) -> Result<bool> {
        if self.require_online().is_err() {
            return Ok(false);
        }

        learned_from(&self.catalog, &key, |release| {
            self.services.online.release(release)
        })
    }
}

pub(crate) fn songs_of(release: &Release) -> Vec<SearchSong> {
    let artist = release.credited_as();
    let artist_id = release
        .credit
        .first()
        .and_then(|credit| credit.mbid.as_ref())
        .map_or_else(String::new, ToString::to_string);
    let group = release
        .group
        .as_ref()
        .map_or_else(String::new, ToString::to_string);

    release
        .media
        .iter()
        .flat_map(|medium| {
            medium
                .tracks
                .iter()
                .map(move |track| (medium.position, track))
        })
        .map(|(disc, track)| SearchSong {
            recording: track
                .recording
                .as_ref()
                .map_or_else(String::new, ToString::to_string),
            title: track.title.clone(),
            artist: track.artist.clone().unwrap_or_else(|| artist.clone()),
            artist_id: if track.artist.is_none() {
                artist_id.clone()
            } else {
                String::new()
            },
            album: release.title.clone(),
            release: release.id.to_string(),
            release_group: group.clone(),
            disc: Some(disc),
            position: Some(track.position),
            duration_ms: track
                .length
                .map(|length| length.as_millis().min(u128::from(u64::MAX)) as u64),
            isrcs: track.isrcs.clone(),
            held: false,
        })
        .collect()
}

pub(crate) fn keep_the_songs_of(
    connection: &Connection,
    key: &str,
    release: &Release,
) -> Result<()> {
    connection
        .execute("DELETE FROM release_songs WHERE album_key = ?1", [key])
        .map_err(write_error)?;
    let mut keeping = connection
        .prepare(
            "INSERT INTO release_songs (album_key, disc, position, recording, title, artist,
                artist_id, album, release, release_group, duration_ms, isrcs)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        )
        .map_err(write_error)?;
    for song in songs_of(release) {
        keeping
            .execute(params![
                key,
                song.disc,
                song.position,
                song.recording,
                song.title,
                song.artist,
                song.artist_id,
                song.album,
                song.release,
                song.release_group,
                song.duration_ms
                    .map(|value| value.min(i64::MAX as u64) as i64),
                song.isrcs.join("\n"),
            ])
            .map_err(write_error)?;
    }

    Ok(())
}

fn learned_from(
    catalog: &Catalog,
    key: &str,
    asked: impl FnOnce(&Mbid) -> resonate_library::Result<Option<Release>>,
) -> Result<bool> {
    let Some(release) = catalog.release_not_yet_read(key)? else {
        return Ok(false);
    };
    let Some(read) =
        asked(&release).map_err(|source| Error::reference(LookupOp::Release, source))?
    else {
        return Ok(false);
    };

    keep_the_songs_of(&catalog.0.lock(), key, &read)?;

    Ok(read.track_count() > 0)
}

struct HeldSong {
    recording: String,
    title: String,
    disc: Option<u32>,
    position: Option<u32>,
}

impl HeldSong {
    fn stands_at(&self, song: &SearchSong, one_medium: bool) -> bool {
        let same_disc = match self.disc {
            Some(disc) => song.disc == Some(disc),
            None => one_medium,
        };

        same_disc && self.position.is_some() && self.position == song.position
    }
}

fn kept_song(row: &rusqlite::Row<'_>) -> rusqlite::Result<SearchSong> {
    Ok(SearchSong {
        recording: row.get(0)?,
        title: row.get(1)?,
        artist: row.get(2)?,
        artist_id: row.get(3)?,
        album: row.get(4)?,
        release: row.get(5)?,
        release_group: row.get(6)?,
        disc: row.get(7)?,
        position: row.get(8)?,
        duration_ms: row
            .get::<_, Option<i64>>(9)?
            .map(|value| value.max(0) as u64),
        isrcs: row
            .get::<_, String>(10)?
            .split('\n')
            .filter(|isrc| !isrc.is_empty())
            .map(str::to_owned)
            .collect(),
        held: false,
    })
}

fn held_song(row: &rusqlite::Row<'_>) -> rusqlite::Result<HeldSong> {
    Ok(HeldSong {
        recording: row.get(0)?,
        title: folded_letters(row.get::<_, String>(1)?.trim()),
        disc: row.get(2)?,
        position: row.get(3)?,
    })
}

type Pairing<'a> = &'a dyn Fn(usize, &HeldSong) -> bool;

fn left_unpaired(songs: Vec<SearchSong>, held: &[HeldSong]) -> Vec<SearchSong> {
    let one_medium = songs.windows(2).all(|pair| pair[0].disc == pair[1].disc);
    let titles: Vec<String> = songs
        .iter()
        .map(|song| folded_letters(song.title.trim()))
        .collect();
    let same_recording = |at: usize, file: &HeldSong| {
        !songs[at].recording.is_empty() && songs[at].recording == file.recording
    };
    let same_title_there = |at: usize, file: &HeldSong| {
        titles[at] == file.title && file.stands_at(&songs[at], one_medium)
    };
    let same_title = |at: usize, file: &HeldSong| titles[at] == file.title;
    let same_place = |at: usize, file: &HeldSong| file.stands_at(&songs[at], one_medium);
    let passes: [Pairing<'_>; 4] = [&same_recording, &same_title_there, &same_title, &same_place];

    let mut paired_song = vec![false; songs.len()];
    let mut paired_file = vec![false; held.len()];
    for pairs in passes {
        for (at, paired) in paired_song.iter_mut().enumerate() {
            if *paired {
                continue;
            }
            let found = held
                .iter()
                .enumerate()
                .find(|(place, file)| !paired_file[*place] && pairs(at, file));
            if let Some((place, _)) = found {
                *paired = true;
                paired_file[place] = true;
            }
        }
    }

    songs
        .into_iter()
        .zip(paired_song)
        .filter_map(|(song, paired)| (!paired).then_some(song))
        .collect()
}

impl Catalog {
    pub(crate) fn album_songs_not_held(&self, key: &str) -> Result<Vec<SearchSong>> {
        let connection = self.0.lock();
        let songs = connection
            .prepare_cached(SONGS_KEPT)
            .map_err(read_error)?
            .query_map([key], kept_song)
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)?;
        let held = connection
            .prepare_cached(SONGS_HELD)
            .map_err(read_error)?
            .query_map([key], held_song)
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)?;

        left_unpaired(songs, &held)
            .into_iter()
            .filter_map(|song| match local_match(&connection, &song) {
                Ok(Some(_)) => None,
                Ok(None) => Some(Ok(song)),
                Err(failure) => Some(Err(failure)),
            })
            .collect()
    }

    pub(crate) fn release_not_yet_read(&self, key: &str) -> Result<Option<Mbid>> {
        let connection = self.0.lock();
        let kept: bool = connection
            .query_row(ANY_SONG_KEPT, [key], |row| row.get(0))
            .map_err(read_error)?;
        if kept {
            return Ok(None);
        }
        let release: Option<String> = connection
            .query_row(RELEASE_OF_THE_ALBUM, [key], |row| row.get(0))
            .optional()
            .map_err(read_error)?
            .flatten();

        Ok(release.and_then(|release| Mbid::new(&release).ok()))
    }
}

fn read_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Read, source)
}

fn write_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Write, source)
}

#[cfg(test)]
mod tests {
    use std::{path::Path, time::Duration};

    use resonate_library::{Credit, Medium, ReleaseTrack};

    use super::*;
    use crate::Track;

    const KEY: &str = "the album";
    const RELEASE: &str = "11111111-1111-4111-8111-111111111111";
    const FIRST: &str = "22222222-2222-4222-8222-222222222222";
    const SECOND: &str = "33333333-3333-4333-8333-333333333333";

    fn track(position: u32, title: &str, recording: Option<&str>) -> ReleaseTrack {
        ReleaseTrack {
            position,
            number: position.to_string(),
            title: title.into(),
            artist: None,
            recording: recording.and_then(|id| Mbid::new(id).ok()),
            track: None,
            length: Some(Duration::from_secs(100 + u64::from(position))),
            isrcs: vec!["GBN9Y1100089".into()],
            links: Vec::new(),
        }
    }

    fn release(media: Vec<Medium>) -> Release {
        Release {
            id: Mbid::new(RELEASE).expect("an mbid"),
            group: None,
            title: "The album".into(),
            credit: vec![Credit {
                name: "The band".into(),
                joined_by: String::new(),
                mbid: None,
            }],
            date: None,
            country: None,
            label: None,
            catalog_number: None,
            barcode: None,
            kind: None,
            disambiguation: None,
            has_front_cover: false,
            links: Vec::new(),
            media,
        }
    }

    fn medium(position: u32, tracks: Vec<ReleaseTrack>) -> Medium {
        Medium {
            position,
            format: None,
            title: None,
            tracks,
        }
    }

    fn held(catalog: &Catalog, title: &str, disc: Option<u32>, number: Option<u32>) -> Track {
        let song = catalog
            .upsert(Track {
                uri: format!("content://test/{title}.flac"),
                name: format!("{title}.flac"),
                title: title.into(),
                artist: "The band".into(),
                album: "The album".into(),
                duration_ms: 90_000,
                ..Track::default()
            })
            .expect("a song");
        catalog
            .0
            .lock()
            .execute(
                "UPDATE documents SET disc_number = ?1, track_number = ?2 WHERE id = ?3",
                params![disc, number, song.id as i64],
            )
            .expect("its place");

        song
    }

    fn key_of(catalog: &Catalog, song: &Track) -> String {
        catalog
            .0
            .lock()
            .query_row(
                "SELECT album_key FROM documents WHERE id = ?1",
                [song.id as i64],
                |row| row.get(0),
            )
            .expect("a key")
    }

    fn opened() -> Catalog {
        Catalog::open(Path::new(":memory:")).expect("a catalog")
    }

    fn titles(songs: &[SearchSong]) -> Vec<&str> {
        songs.iter().map(|song| song.title.as_str()).collect()
    }

    #[test]
    fn a_kept_release_lists_only_the_songs_the_album_does_not_hold_in_their_order() {
        let catalog = opened();
        let song = held(&catalog, "Two", None, Some(2));
        let key = key_of(&catalog, &song);
        let read = release(vec![medium(
            1,
            vec![
                track(1, "One", Some(FIRST)),
                track(2, "Two", None),
                track(3, "Three", Some(SECOND)),
            ],
        )]);

        keep_the_songs_of(&catalog.0.lock(), &key, &read).expect("kept");

        let missing = catalog.album_songs_not_held(&key).expect("the rows");
        assert_eq!(titles(&missing), vec!["One", "Three"]);
        assert_eq!(missing[0].recording, FIRST);
        assert_eq!(missing[0].artist, "The band");
        assert_eq!(missing[0].album, "The album");
        assert_eq!(missing[0].release, RELEASE);
        assert_eq!((missing[0].disc, missing[0].position), (Some(1), Some(1)));
        assert_eq!(missing[0].duration_ms, Some(101_000));
        assert_eq!(missing[0].isrcs, vec!["GBN9Y1100089".to_owned()]);
        assert!(!missing[0].held);
    }

    #[test]
    fn a_song_is_held_by_its_recording_its_title_or_its_place() {
        let catalog = opened();
        let by_title = held(&catalog, "JÓGA", None, None);
        let by_place = held(&catalog, "Track 02", Some(1), Some(2));
        let by_recording = held(&catalog, "Renamed", None, None);
        catalog
            .0
            .lock()
            .execute(
                "UPDATE documents SET recording = ?1 WHERE id = ?2",
                params![SECOND, by_recording.id as i64],
            )
            .expect("a recording");
        let key = key_of(&catalog, &by_title);
        let read = release(vec![medium(
            1,
            vec![
                track(1, "Joga", None),
                track(2, "Hunter", None),
                track(3, "Pluto", Some(SECOND)),
                track(4, "Unravel", None),
            ],
        )]);

        keep_the_songs_of(&catalog.0.lock(), &key, &read).expect("kept");

        assert_eq!(key_of(&catalog, &by_place), key);
        assert_eq!(
            titles(&catalog.album_songs_not_held(&key).expect("the rows")),
            vec!["Unravel"]
        );
    }

    #[test]
    fn one_file_answers_for_one_song_of_a_name_a_release_holds_twice() {
        let read = release(vec![
            medium(1, vec![track(1, "Intro", None)]),
            medium(2, vec![track(1, "Intro", None)]),
        ]);

        let songs = left_unpaired(
            songs_of(&read),
            &[HeldSong {
                recording: String::new(),
                title: folded_letters("Intro"),
                disc: None,
                position: None,
            }],
        );
        assert_eq!(songs.len(), 1);
        assert_eq!(songs[0].disc, Some(2));
    }

    #[test]
    fn a_song_landed_under_another_album_is_held_by_its_recording() {
        let catalog = opened();
        let song = held(&catalog, "Two", None, Some(2));
        let key = key_of(&catalog, &song);
        let read = release(vec![medium(
            1,
            vec![track(1, "One", Some(FIRST)), track(2, "Two", None)],
        )]);
        keep_the_songs_of(&catalog.0.lock(), &key, &read).expect("kept");

        let elsewhere = catalog
            .upsert(Track {
                uri: "content://test/elsewhere.flac".into(),
                name: "elsewhere.flac".into(),
                title: "One".into(),
                artist: "The band".into(),
                album: "Another album".into(),
                ..Track::default()
            })
            .expect("a song");
        catalog
            .0
            .lock()
            .execute(
                "UPDATE documents SET recording = ?1 WHERE id = ?2",
                params![FIRST, elsewhere.id as i64],
            )
            .expect("a recording");

        assert!(
            catalog
                .album_songs_not_held(&key)
                .expect("the rows")
                .is_empty()
        );
    }

    #[test]
    fn a_kept_list_is_replaced_whole_when_another_release_lands() {
        let catalog = opened();
        let read = release(vec![medium(1, vec![track(1, "One", None)])]);
        let other = release(vec![medium(1, vec![track(1, "Another", None)])]);

        keep_the_songs_of(&catalog.0.lock(), KEY, &read).expect("kept");
        keep_the_songs_of(&catalog.0.lock(), KEY, &other).expect("kept");

        assert_eq!(
            titles(&catalog.album_songs_not_held(KEY).expect("the rows")),
            vec!["Another"]
        );
    }

    #[test]
    fn an_album_with_a_release_is_asked_once_and_one_without_is_never_asked() {
        let catalog = opened();
        let song = held(&catalog, "Two", None, Some(2));
        let key = key_of(&catalog, &song);
        let answer = release(vec![medium(
            1,
            vec![track(1, "One", None), track(2, "Two", None)],
        )]);
        let mut asked = 0;
        let mut ask = |_: &Mbid| {
            asked += 1;
            Ok(Some(answer.clone()))
        };

        assert!(!learned_from(&catalog, &key, &mut ask).expect("nothing to ask"));

        catalog
            .0
            .lock()
            .execute(
                "INSERT INTO album_art (album_key, release) VALUES (?1, ?2)",
                params![key, RELEASE],
            )
            .expect("a release");

        assert!(learned_from(&catalog, &key, &mut ask).expect("asked"));
        assert!(!learned_from(&catalog, &key, &mut ask).expect("kept"));
        assert_eq!(asked, 1);
        assert_eq!(
            titles(&catalog.album_songs_not_held(&key).expect("the rows")),
            vec!["One"]
        );
    }

    #[test]
    fn a_release_read_from_the_songs_tags_answers_as_the_albums_own_does() {
        let catalog = opened();
        let song = held(&catalog, "Two", None, Some(2));
        let key = key_of(&catalog, &song);
        catalog
            .0
            .lock()
            .execute(
                "UPDATE documents SET release = ?1 WHERE id = ?2",
                params![RELEASE, song.id as i64],
            )
            .expect("a tagged release");

        assert_eq!(
            catalog.release_not_yet_read(&key).expect("a release"),
            Mbid::new(RELEASE).ok()
        );
    }
}
