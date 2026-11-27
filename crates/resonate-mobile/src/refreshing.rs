use resonate_codec::{TagSet, probe};
use rusqlite::{OptionalExtension, params};

use crate::{
    Error, MobilePlayer, Result, StoreOp, Track, catalog::Catalog,
    catalog_extras::unreferenced_picture_dropped, documents::location, enrichment::Asking,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CarriedIds {
    recording: Option<String>,
    release: Option<String>,
    artist: Option<String>,
}

impl CarriedIds {
    fn of(tags: TagSet) -> Self {
        Self {
            recording: tags.musicbrainz_track_id,
            release: tags.musicbrainz_album_id,
            artist: tags.musicbrainz_artist_id,
        }
    }
}

type AlbumMatch = (Option<String>, Option<String>, Option<i64>);

#[uniffi::export]
impl MobilePlayer {
    pub fn refresh_track(&self, id: u64) -> Result<Track> {
        self.require_online()?;
        self.refresh_one(&self.asking_for_one_song(), id)?;
        self.catalog.track(id)
    }
}

impl MobilePlayer {
    pub(crate) fn refresh_one(&self, asking: &Asking, id: u64) -> Result<()> {
        asking.may_be_asked()?;

        let carried = self.ids_the_file_carries(id)?;
        let cleared = self.catalog.forget_what_was_fetched(id, &carried)?;

        for song in cleared {
            match self.ids_the_file_carries(song) {
                Ok(carried) => self.catalog.carried_ids_read_again(song, &carried)?,
                Err(error) => tracing::debug!(
                    %error,
                    track = song,
                    "a song of the album looked up again was left for the next scan to read"
                ),
            }
        }
        self.enrich_one(asking, id)
    }

    fn ids_the_file_carries(&self, id: u64) -> Result<CarriedIds> {
        let track = self.catalog.track(id)?;
        let info = probe(&self.sources, &location(&track.uri)?)?;

        Ok(CarriedIds::of(info.tags))
    }
}

impl Catalog {
    pub(crate) fn forget_what_was_fetched(
        &self,
        id: u64,
        carried: &CarriedIds,
    ) -> Result<Vec<u64>> {
        let mut connection = self.0.lock();

        let transaction = connection.transaction().map_err(write_error)?;
        let album: Option<String> = transaction
            .query_row(
                "UPDATE documents SET recording = ?2, release = ?3, release_group = NULL,
                 artist_mbid = ?4, asked = NULL, answered = NULL, asks = 0, refusals = 0
                 WHERE id = ?1 RETURNING album_key",
                params![
                    id as i64,
                    carried.recording,
                    carried.release,
                    carried.artist
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(write_error)?
            .ok_or(Error::UnknownTrack)?;

        let Some(album) = album else {
            transaction.commit().map_err(write_error)?;
            return Ok(Vec::new());
        };

        let matched: Option<AlbumMatch> = transaction
            .query_row(
                "SELECT release, release_group, picture FROM album_art WHERE album_key = ?1",
                [&album],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(write_error)?;
        let (release, group, picture) = matched.unwrap_or_default();

        let cleared = {
            let mut clearing = transaction
                .prepare(
                    "UPDATE documents SET
                     release = CASE WHEN release = ?3 THEN NULL ELSE release END,
                     release_group = CASE WHEN release_group = ?4 THEN NULL ELSE release_group END,
                     indexed_by = NULL
                     WHERE album_key = ?1 AND id != ?2 AND (release = ?3 OR release_group = ?4)
                     RETURNING id",
                )
                .map_err(write_error)?;

            clearing
                .query_map(params![album, id as i64, release, group], |row| {
                    row.get::<_, i64>(0)
                })
                .map_err(write_error)?
                .map(|row| row.map(|id| id as u64))
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(write_error)?
        };

        transaction
            .execute("DELETE FROM album_art WHERE album_key = ?1", [&album])
            .map_err(write_error)?;
        transaction
            .execute("DELETE FROM release_songs WHERE album_key = ?1", [&album])
            .map_err(write_error)?;
        if let Some(picture) = picture {
            unreferenced_picture_dropped(&transaction, picture).map_err(write_error)?;
            transaction
                .execute(
                    "UPDATE documents SET art_revision = art_revision + 1 WHERE album_key = ?1",
                    [&album],
                )
                .map_err(write_error)?;
        }
        transaction.commit().map_err(write_error)?;
        Ok(cleared)
    }

    fn carried_ids_read_again(&self, id: u64, carried: &CarriedIds) -> Result<()> {
        self.0
            .lock()
            .execute(
                "UPDATE documents SET recording = coalesce(?2, recording),
                 release = coalesce(?3, release), artist_mbid = coalesce(?4, artist_mbid)
                 WHERE id = ?1",
                params![
                    id as i64,
                    carried.recording,
                    carried.release,
                    carried.artist
                ],
            )
            .map_err(write_error)?;
        Ok(())
    }
}

fn write_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Write, source)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::catalog_extras::kept_picture;

    const WRONG_RECORDING: &str = "0b2e1a52-6f1d-4c33-9a5e-6a4b8f7d2e01";

    const WRONG_RELEASE: &str = "7c0e3d1b-2a4f-4b8e-8d6c-1f2e3a4b5c6d";

    const WRONG_GROUP: &str = "3d4e5f6a-7b8c-4d9e-8f0a-1b2c3d4e5f6a";

    const TAGGED_ARTIST: &str = "9a8b7c6d-5e4f-4a3b-8c2d-1e0f9a8b7c6d";

    const OWN_RELEASE: &str = "5f4e3d2c-1b0a-4f9e-8d7c-6b5a4f3e2d1c";

    fn album_of_three(catalog: &Catalog) -> (u64, u64, u64) {
        let song = |number: u32| {
            catalog
                .upsert(Track {
                    uri: format!("content://test/{number}.flac"),
                    name: format!("{number}.flac"),
                    title: format!("Song {number}"),
                    artist: "Artist".into(),
                    album: "Album".into(),
                    duration_ms: 180_000,
                    ..Track::default()
                })
                .expect("a song")
                .id
        };

        (song(1), song(2), song(3))
    }

    fn matched_wrongly(catalog: &Catalog, songs: [u64; 3]) {
        let mut connection = catalog.0.lock();

        let transaction = connection.transaction().expect("a transaction");
        let picture = kept_picture(&transaction, &[7, 7, 7, 7]).expect("a picture");

        transaction
            .execute(
                "UPDATE documents SET recording = ?1, release = ?2, release_group = ?3,
                 artist_mbid = ?4, asked = 1, answered = 1, asks = 2, refusals = 1,
                 indexed_by = 2",
                params![WRONG_RECORDING, WRONG_RELEASE, WRONG_GROUP, TAGGED_ARTIST],
            )
            .expect("a wrong match");
        transaction
            .execute(
                "UPDATE documents SET release = ?1 WHERE id = ?2",
                params![OWN_RELEASE, songs[2] as i64],
            )
            .expect("a release of its own");
        transaction
            .execute(
                "INSERT INTO album_art (album_key, release, release_group, origin, asked,
                 answered, cover_asked, picture)
                 SELECT album_key, ?1, ?2, 'coverartarchive', 1, 1, 1, ?3 FROM documents
                 WHERE id = ?4",
                params![WRONG_RELEASE, WRONG_GROUP, picture, songs[0] as i64],
            )
            .expect("the album's match");
        transaction
            .execute(
                "INSERT INTO release_songs (album_key, disc, position, recording, title, artist,
                 artist_id, album, release, release_group, isrcs)
                 SELECT album_key, 1, 1, ?1, 'Song 1', 'Artist', '', 'Album', ?2, ?3, ''
                 FROM documents WHERE id = ?4",
                params![WRONG_RECORDING, WRONG_RELEASE, WRONG_GROUP, songs[0] as i64],
            )
            .expect("the release's songs");
        transaction.commit().expect("committed");
    }

    type Held = (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<i64>,
        i64,
        i64,
        Option<i64>,
    );

    fn held(catalog: &Catalog, id: u64) -> Held {
        catalog
            .0
            .lock()
            .query_row(
                "SELECT recording, release, release_group, artist_mbid, asked, answered, asks,
                 refusals, indexed_by FROM documents WHERE id = ?1",
                [id as i64],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                    ))
                },
            )
            .expect("the song's row")
    }

    fn rows_of(catalog: &Catalog, table: &str) -> i64 {
        catalog
            .0
            .lock()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("a count")
    }

    #[test]
    fn what_was_fetched_is_forgotten_and_only_the_ids_the_file_carries_stay() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");

        let (song, other, own) = album_of_three(&catalog);

        matched_wrongly(&catalog, [song, other, own]);

        let carried = CarriedIds {
            artist: Some(TAGGED_ARTIST.into()),
            ..CarriedIds::default()
        };
        let cleared = catalog
            .forget_what_was_fetched(song, &carried)
            .expect("forgotten");

        assert_eq!(cleared, vec![other, own]);
        assert_eq!(
            held(&catalog, song),
            (
                None,
                None,
                None,
                Some(TAGGED_ARTIST.into()),
                None,
                None,
                0,
                0,
                Some(2)
            )
        );
        assert_eq!(
            held(&catalog, other),
            (
                Some(WRONG_RECORDING.into()),
                None,
                None,
                Some(TAGGED_ARTIST.into()),
                Some(1),
                Some(1),
                2,
                1,
                None
            )
        );
        assert_eq!(held(&catalog, own).1.as_deref(), Some(OWN_RELEASE));
        assert_eq!(rows_of(&catalog, "album_art"), 0);
        assert_eq!(rows_of(&catalog, "release_songs"), 0);
        assert_eq!(rows_of(&catalog, "pictures"), 0);
    }

    #[test]
    fn a_song_of_the_album_read_again_takes_back_the_ids_its_file_carries() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");

        let (song, other, own) = album_of_three(&catalog);

        matched_wrongly(&catalog, [song, other, own]);
        catalog
            .forget_what_was_fetched(song, &CarriedIds::default())
            .expect("forgotten");
        catalog
            .carried_ids_read_again(
                other,
                &CarriedIds {
                    release: Some(OWN_RELEASE.into()),
                    ..CarriedIds::default()
                },
            )
            .expect("read again");

        assert_eq!(held(&catalog, other).1.as_deref(), Some(OWN_RELEASE));
        assert_eq!(
            held(&catalog, other).0.as_deref(),
            Some(WRONG_RECORDING),
            "a song's own recording is its own lookup's, not the album's"
        );
    }

    #[test]
    fn forgetting_a_song_the_catalog_does_not_hold_is_refused() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");

        assert!(matches!(
            catalog.forget_what_was_fetched(404, &CarriedIds::default()),
            Err(Error::UnknownTrack)
        ));
    }
}
