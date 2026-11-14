use std::{
    collections::BTreeMap,
    os::unix::fs::MetadataExt,
    sync::{Arc, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use resonate_library::{identical_among, kept_first};
use rusqlite::{OptionalExtension, Transaction};

use crate::{
    Documents, Error, MobilePlayer, Result, StoreOp, Track,
    catalog::{Catalog, read},
    catalog_extras::unreferenced_picture_dropped,
    documents::{document_uri, location, owned},
    rewriting::crc_of,
    sync::{RECORDING_COLUMNS, recording_at},
    vanished::{handed_over, referring_tables},
};

const HELD_WITH_THEIR_SIZES: &str =
    "SELECT d.*, d.size AS bytes FROM documents d WHERE d.missing = 0 ORDER BY d.id";

fn read_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Read, source)
}

fn write_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Write, source)
}

#[derive(Default)]
pub(crate) struct Files(OnceLock<Arc<dyn Documents>>);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

impl Files {
    pub(crate) fn reach_through(&self, documents: Arc<dyn Documents>) {
        if self.0.set(documents).is_err() {
            tracing::debug!("the documents were already reached");
        }
    }

    fn identity_of(&self, uri: &str) -> Option<FileIdentity> {
        let descriptor = self.0.get()?.open_document(uri.to_owned());
        if descriptor < 0 {
            return None;
        }
        let metadata = owned(descriptor).metadata().ok()?;

        metadata.is_file().then(|| FileIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct IdenticalSongs {
    pub kept: Track,
    pub copies: Vec<Track>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct SongsForgotten {
    pub forgotten: u32,
    pub left_the_queue: u32,
}

#[uniffi::export]
impl MobilePlayer {
    pub fn identical_songs(&self) -> Result<Vec<IdenticalSongs>> {
        let playing = self.playing_uri();
        let mut found = Vec::new();

        for same_size in self.songs_sharing_a_size()? {
            let one_row_a_file = self.one_row_a_file(same_size, playing.as_deref());
            found.extend(
                identical_among(one_row_a_file, |track| self.opened(&track.uri))
                    .into_iter()
                    .filter_map(|alike| kept_among(alike, playing.as_deref())),
            );
        }
        Ok(found)
    }

    pub fn forget_deleted_songs(&self, ids: Vec<u64>) -> Result<SongsForgotten> {
        let gone = self
            .held_uris(&ids)
            .into_iter()
            .filter(|(_, uri)| !self.stands_again(uri))
            .collect::<BTreeMap<_, _>>();
        let left_the_queue = self.drop_from_queue(&gone.values().cloned().collect());

        let forgotten = self
            .catalog
            .forget(&gone.keys().copied().collect::<Vec<_>>())?;
        self.forget_analyses(gone.keys());
        Ok(SongsForgotten {
            forgotten: counted(forgotten),
            left_the_queue: counted(left_the_queue),
        })
    }

    pub fn fold_identical_songs(&self, kept: u64, copies: Vec<u64>) -> Result<SongsForgotten> {
        self.catalog.track(kept)?;
        let gone = self.held_uris(&copies);
        let gone = gone
            .into_iter()
            .filter(|(id, _)| *id != kept)
            .collect::<BTreeMap<_, _>>();
        let left_the_queue = self.drop_from_queue(&gone.values().cloned().collect());

        let folded = self
            .catalog
            .fold_into(kept, &gone.keys().copied().collect::<Vec<_>>())?;
        self.forget_analyses(gone.keys());
        Ok(SongsForgotten {
            forgotten: counted(folded),
            left_the_queue: counted(left_the_queue),
        })
    }
}

impl MobilePlayer {
    fn songs_sharing_a_size(&self) -> Result<Vec<Vec<Track>>> {
        let mut by_size = BTreeMap::<u64, Vec<Track>>::new();
        for (stamped, track) in self.catalog.held_with_their_sizes()? {
            let size = match stamped {
                0 => self.measured(&track.uri),
                stamped => Some(stamped),
            };
            if let Some(size) = size {
                by_size.entry(size).or_default().push(track);
            }
        }
        Ok(by_size
            .into_values()
            .filter(|tracks| tracks.len() > 1)
            .collect())
    }

    fn one_row_a_file(&self, rows: Vec<Track>, playing: Option<&str>) -> Vec<Track> {
        let mut unidentified = Vec::new();
        let mut by_file = BTreeMap::<FileIdentity, Vec<Track>>::new();
        for row in rows {
            match self.services.files.identity_of(&row.uri) {
                Some(identity) => by_file.entry(identity).or_default().push(row),
                None => unidentified.push(row),
            }
        }

        unidentified.extend(by_file.into_values().map(|mut naming_one_file| {
            let kept_at = kept_at(&mut naming_one_file, playing);
            naming_one_file.swap_remove(kept_at)
        }));
        unidentified
    }

    fn measured(&self, uri: &str) -> Option<u64> {
        self.opened(uri)?.byte_len()
    }

    fn stands_again(&self, uri: &str) -> bool {
        location(uri).is_ok_and(|at| self.sources.open(&at).is_ok())
    }

    fn held_uris(&self, ids: &[u64]) -> BTreeMap<u64, String> {
        ids.iter()
            .filter_map(|id| self.catalog.track(*id).ok().map(|track| (*id, track.uri)))
            .collect()
    }

    fn forget_analyses<'a>(&self, ids: impl Iterator<Item = &'a u64>) {
        let mut analyses = self.services.analyses.lock();
        for id in ids {
            analyses.remove(id);
        }
    }

    fn playing_uri(&self) -> Option<String> {
        let player = self.playing.player().ok()?;
        let state = player.state();
        let queued = player.queued();
        let id = state
            .current
            .map(|track| track.id)
            .or_else(|| crate::row_being_opened(&state, &queued.rows))?;

        queued
            .rows
            .iter()
            .find(|row| row.id == id)
            .and_then(|row| document_uri(&row.location).ok())
    }

    pub(crate) fn whole_crc(&self, uri: &str) -> Option<u32> {
        let mut stream = self.opened(uri)?;
        match crc_of(&mut stream) {
            Ok(crc) => Some(crc),
            Err(error) => {
                tracing::debug!(%error, uri, "a song could not be read to compare");
                None
            }
        }
    }

    fn opened(&self, uri: &str) -> Option<Box<dyn resonate_codec::MediaStream>> {
        let at = location(uri).ok()?;
        match self.sources.open(&at) {
            Ok(media) => Some(media.stream),
            Err(error) => {
                tracing::debug!(%error, uri, "a song could not be opened to compare");
                None
            }
        }
    }
}

fn kept_among(mut alike: Vec<Track>, playing: Option<&str>) -> Option<IdenticalSongs> {
    if alike.len() < 2 {
        return None;
    }
    let kept_at = kept_at(&mut alike, playing);
    let kept = alike.remove(kept_at);
    Some(IdenticalSongs {
        kept,
        copies: alike,
    })
}

fn kept_at(alike: &mut [Track], playing: Option<&str>) -> usize {
    kept_first(alike, |track| track.name.as_str());
    playing
        .and_then(|uri| alike.iter().position(|track| track.uri == uri))
        .unwrap_or(0)
}

fn counted(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

impl Catalog {
    fn held_with_their_sizes(&self) -> Result<Vec<(u64, Track)>> {
        let connection = self.0.lock();
        let mut query = connection
            .prepare(HELD_WITH_THEIR_SIZES)
            .map_err(read_error)?;
        query
            .query_map([], |row| {
                let size = row.get::<_, Option<i64>>("bytes")?.unwrap_or(0);
                Ok((u64::try_from(size).unwrap_or(0), read(row)?))
            })
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)
    }

    fn forget(&self, ids: &[u64]) -> Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        let mut connection = self.0.lock();
        let referring = referring_tables(&connection)?;
        let transaction = connection.transaction().map_err(write_error)?;
        let now = nanos_now();
        let mut forgotten = 0;
        for id in ids {
            let picture = picture_of(&transaction, *id as i64).map_err(read_error)?;
            deletion_noted(&transaction, *id as i64, now).map_err(write_error)?;
            forgotten += forgotten_in(&transaction, &referring, *id as i64).map_err(write_error)?;
            if let Some(picture) = picture {
                unreferenced_picture_dropped(&transaction, picture).map_err(write_error)?;
            }
        }
        transaction.commit().map_err(write_error)?;
        Ok(forgotten)
    }

    fn fold_into(&self, kept: u64, copies: &[u64]) -> Result<usize> {
        if copies.is_empty() {
            return Ok(0);
        }
        let mut connection = self.0.lock();
        let referring = referring_tables(&connection)?;
        let transaction = connection.transaction().map_err(write_error)?;
        for copy in copies {
            let picture = picture_of(&transaction, *copy as i64).map_err(read_error)?;
            handed_over(&transaction, &referring, *copy as i64, kept as i64)
                .map_err(write_error)?;
            if let Some(picture) = picture {
                unreferenced_picture_dropped(&transaction, picture).map_err(write_error)?;
            }
        }
        transaction.commit().map_err(write_error)?;
        Ok(copies.len())
    }
}

fn nanos_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_nanos()).unwrap_or(i64::MAX)
        })
}

fn deletion_noted(transaction: &Transaction<'_>, id: i64, now: i64) -> rusqlite::Result<()> {
    let recording = transaction
        .query_row(
            &format!("SELECT {RECORDING_COLUMNS} FROM documents d WHERE d.id = ?1"),
            [id],
            |row| recording_at(row, 0),
        )
        .optional()?;
    if let Some(recording) = recording {
        for key in [recording.key(), recording.named_key()] {
            transaction.execute(
                "INSERT OR REPLACE INTO songs_deleted (recording, deleted) VALUES (?1, ?2)",
                rusqlite::params![key.as_str(), now],
            )?;
        }
    }
    transaction.execute(
        "INSERT OR REPLACE INTO songs_deleted (recording, deleted)
         SELECT recording, ?2 FROM songs_received WHERE track = ?1",
        [id, now],
    )?;
    Ok(())
}

fn picture_of(transaction: &Transaction<'_>, id: i64) -> rusqlite::Result<Option<i64>> {
    transaction
        .query_row(
            "SELECT picture FROM artwork WHERE track = ?1",
            [id],
            |row| row.get(0),
        )
        .optional()
}

fn forgotten_in(
    transaction: &Transaction<'_>,
    referring: &[(String, String)],
    id: i64,
) -> rusqlite::Result<usize> {
    transaction.execute(
        "UPDATE playlists SET revision = revision + 1 WHERE id IN (SELECT playlist FROM playlist_rows WHERE track = ?1)",
        [id],
    )?;
    for (table, column) in referring {
        transaction.execute(
            &format!("DELETE FROM \"{table}\" WHERE \"{column}\" = ?1"),
            [id],
        )?;
    }
    transaction.execute("DELETE FROM documents WHERE id = ?1", [id])
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn named(name: &str) -> Track {
        Track {
            uri: format!("content://test/{name}"),
            name: name.into(),
            ..Track::default()
        }
    }

    #[test]
    fn the_copy_kept_is_the_one_playing_else_the_one_no_copy_numbered_as_on_the_desktop() {
        let alike = || vec![named("Song (1).flac"), named("Song.flac")];

        let kept = kept_among(alike(), None).expect("two alike");
        let playing = kept_among(alike(), Some("content://test/Song (1).flac")).expect("two alike");

        assert_eq!(kept.kept.name, "Song.flac");
        assert_eq!(playing.kept.name, "Song (1).flac");
    }

    fn queued(titles: &[&str], row: u32, position_ms: u64) -> (Catalog, Vec<Track>) {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let tracks: Vec<Track> = titles
            .iter()
            .map(|title| {
                catalog
                    .upsert(Track {
                        uri: format!("content://test/{title}.flac"),
                        name: format!("{title}.flac"),
                        title: (*title).into(),
                        ..Track::default()
                    })
                    .expect("an indexed song")
            })
            .collect();
        let ids: Vec<u64> = tracks.iter().map(|track| track.id).collect();
        catalog
            .save_queue(&ids, row, position_ms)
            .expect("a saved queue");
        (catalog, tracks)
    }

    #[test]
    fn deleting_a_song_before_the_one_heard_resumes_the_same_song_at_its_place() {
        let (catalog, songs) = queued(&["A", "B", "C"], 1, 90_000);

        catalog.forget(&[songs[0].id]).expect("forgotten");
        let (ids, at, position) = catalog.resume().expect("resumed");

        assert_eq!(ids, vec![songs[1].id, songs[2].id]);
        assert_eq!(at, 0);
        assert_eq!(position, 90_000);
    }

    #[test]
    fn deleting_the_song_heard_resumes_the_next_one_from_its_start() {
        let (catalog, songs) = queued(&["A", "B", "C"], 1, 90_000);

        catalog.forget(&[songs[1].id]).expect("forgotten");
        let (ids, at, position) = catalog.resume().expect("resumed");

        assert_eq!(ids, vec![songs[0].id, songs[2].id]);
        assert_eq!(at, 1);
        assert_eq!(position, 0);
    }

    #[test]
    fn deleting_the_last_song_when_it_was_heard_resumes_the_new_last_from_its_start() {
        let (catalog, songs) = queued(&["A", "B", "C"], 2, 90_000);

        catalog.forget(&[songs[2].id]).expect("forgotten");
        let (ids, at, position) = catalog.resume().expect("resumed");

        assert_eq!(ids, vec![songs[0].id, songs[1].id]);
        assert_eq!(at, 1);
        assert_eq!(position, 0);
    }

    fn deletions_remembered(catalog: &Catalog) -> i64 {
        catalog
            .0
            .lock()
            .query_row("SELECT count(*) FROM songs_deleted", [], |row| row.get(0))
            .expect("a count")
    }

    #[test]
    fn a_song_forgotten_is_remembered_as_deleted_here_and_one_folded_into_its_twin_is_not() {
        let (catalog, songs) = queued(&["A", "B", "C"], 0, 0);

        catalog
            .fold_into(songs[0].id, &[songs[1].id])
            .expect("folded");

        assert_eq!(deletions_remembered(&catalog), 0);

        catalog.forget(&[songs[2].id]).expect("forgotten");

        assert!(deletions_remembered(&catalog) > 0);
    }
}
