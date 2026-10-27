use std::{collections::BTreeSet, io};

use resonate_core::{Recording, Span};
use resonate_engine::Command;
use rusqlite::{Connection, Transaction};

use crate::{
    Error, MobilePlayer, Result, StoreOp,
    catalog::Catalog,
    documents::{document_uri, location},
    sync::recording_at,
};

const REFERRING_TABLES: &str = "SELECT m.name, f.\"from\" FROM sqlite_master m, pragma_foreign_key_list(m.name) f WHERE m.type = 'table' AND f.\"table\" = 'documents'";
const QUEUE_EDITS_AT_MOST: usize = 64;
const DISPLACED_SCHEME: &str = "resonate-displaced";

pub(crate) fn is_vanished(error: &resonate_codec::Error) -> bool {
    matches!(
        error,
        resonate_codec::Error::Io { source, .. } if source.kind() == io::ErrorKind::NotFound
    )
}

fn read_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Read, source)
}

fn write_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Write, source)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Absorbing {
    EveryHidden,
    FoundGone,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct MissingSongs {
    pub hidden: u32,
    pub restored: u32,
    pub left_the_queue: u32,
}

#[uniffi::export]
impl MobilePlayer {
    pub fn hide_missing_songs(&self, prefixes: Vec<String>) -> Result<MissingSongs> {
        self.hide_unlisted_songs(prefixes, Vec::new())
    }

    pub fn hide_unlisted_songs(
        &self,
        prefixes: Vec<String>,
        listed: Vec<String>,
    ) -> Result<MissingSongs> {
        let documents = self.catalog.documents_under(&prefixes)?;
        let listed = listed.iter().map(String::as_str).collect::<BTreeSet<_>>();
        let mut hide = Vec::new();
        let mut restore = Vec::new();
        for (id, uri, missing) in documents {
            let gone = if listed.contains(uri.as_str()) {
                false
            } else {
                let Ok(at) = location(&uri) else { continue };
                match self.sources.open(&at) {
                    Ok(_) => false,
                    Err(error) if is_vanished(&error) => true,
                    Err(_) => continue,
                }
            };
            if gone && !missing {
                hide.push(id);
            } else if !gone && missing {
                restore.push(id);
            }
        }
        self.catalog.mark_missing(&hide, &restore)?;

        Ok(MissingSongs {
            hidden: counted(hide.len()),
            restored: counted(restore.len()),
            left_the_queue: counted(self.drop_missing_from_queue()),
        })
    }

    pub fn hide_songs_under(&self, prefix: String) -> Result<MissingSongs> {
        let hide = self
            .catalog
            .documents_under(&[prefix])?
            .into_iter()
            .filter_map(|(id, _, missing)| (!missing).then_some(id))
            .collect::<Vec<_>>();
        self.catalog.withdraw(&hide)?;

        Ok(MissingSongs {
            hidden: counted(hide.len()),
            restored: 0,
            left_the_queue: counted(self.drop_missing_from_queue()),
        })
    }
}

impl MobilePlayer {
    fn drop_missing_from_queue(&self) -> usize {
        match self.catalog.missing_uris() {
            Ok(missing) => self.drop_from_queue(&missing),
            Err(error) => {
                tracing::debug!(%error, "the missing songs could not be read");
                0
            }
        }
    }

    pub(crate) fn drop_from_queue(&self, missing: &BTreeSet<String>) -> usize {
        if missing.is_empty() {
            return 0;
        }

        let mut dropped = 0;
        for _ in 0..QUEUE_EDITS_AT_MOST {
            let mut run = None;
            let edited = self.edit_queue(|queued, _| {
                let gone = |row: usize| {
                    document_uri(&queued.rows[row].location).is_ok_and(|uri| missing.contains(&uri))
                };
                let Some(last) = (0..queued.rows.len()).rev().find(|row| gone(*row)) else {
                    return Ok(None);
                };
                let first = (0..last)
                    .rev()
                    .take_while(|row| gone(*row))
                    .last()
                    .unwrap_or(last);
                let span = Span::between(first, last);
                run = Some(span.rows());
                Ok(Some(Command::Remove(span)))
            });
            match (edited, run) {
                (Ok(()), Some(rows)) => dropped += rows,
                (Ok(()), None) => break,
                (Err(error), _) => {
                    tracing::debug!(%error, "a song could not leave the queue");
                    break;
                }
            }
        }
        dropped
    }

    pub(crate) fn take_the_place_of_missing(&self, id: u64, absorbing: Absorbing) -> Result<()> {
        let replaced = self.catalog.take_the_place_of_missing(id, absorbing)?;
        let mut analyses = self.services.analyses.lock();
        for old in replaced {
            analyses.remove(&old);
        }
        Ok(())
    }
}

impl Catalog {
    pub(crate) fn documents_under(&self, prefixes: &[String]) -> Result<Vec<(i64, String, bool)>> {
        let connection = self.0.lock();
        let mut query = connection
            .prepare("SELECT id, uri, missing FROM documents ORDER BY id")
            .map_err(read_error)?;
        let documents = query
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)? != 0,
                ))
            })
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)?;
        Ok(documents
            .into_iter()
            .filter(|(_, uri, _)| prefixes.iter().any(|prefix| uri.starts_with(prefix)))
            .collect())
    }

    fn missing_uris(&self) -> Result<BTreeSet<String>> {
        let connection = self.0.lock();
        let mut query = connection
            .prepare("SELECT uri FROM documents WHERE missing = 1")
            .map_err(read_error)?;
        query
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(read_error)?
            .collect::<rusqlite::Result<BTreeSet<_>>>()
            .map_err(read_error)
    }

    pub(crate) fn take_the_place_of_missing(
        &self,
        id: u64,
        absorbing: Absorbing,
    ) -> Result<Vec<u64>> {
        let mut connection = self.0.lock();
        if !any_missing(&connection)? {
            return Ok(Vec::new());
        }
        let Some(arrived) = recording_of(&connection, id as i64)? else {
            return Ok(Vec::new());
        };
        let twins = missing_recordings(&connection, absorbing)?
            .into_iter()
            .filter(|(_, recording)| recording.names_the_same_song_as(&arrived))
            .map(|(old, _)| old)
            .collect::<Vec<_>>();
        if twins.is_empty() {
            return Ok(Vec::new());
        }

        let referring = referring_tables(&connection)?;
        let transaction = connection.transaction().map_err(write_error)?;
        for old in &twins {
            handed_over(&transaction, &referring, *old, id as i64).map_err(write_error)?;
        }
        transaction.commit().map_err(write_error)?;
        Ok(twins.into_iter().map(|old| old as u64).collect())
    }

    pub(crate) fn withdraw(&self, hide: &[i64]) -> Result<()> {
        if hide.is_empty() {
            return Ok(());
        }
        let mut connection = self.0.lock();
        let transaction = connection.transaction().map_err(write_error)?;
        for id in hide {
            transaction
                .execute(
                    "UPDATE documents SET missing = 1, withdrawn = 1 WHERE id = ?1",
                    [*id],
                )
                .map_err(write_error)?;
        }
        transaction.commit().map_err(write_error)
    }

    pub(crate) fn mark_missing(&self, hide: &[i64], restore: &[i64]) -> Result<()> {
        if hide.is_empty() && restore.is_empty() {
            return Ok(());
        }
        let mut connection = self.0.lock();
        let transaction = connection.transaction().map_err(write_error)?;
        for (ids, flag) in [(hide, 1), (restore, 0)] {
            for id in ids {
                transaction
                    .execute(
                        "UPDATE documents SET missing = ?2 WHERE id = ?1",
                        [*id, flag],
                    )
                    .map_err(write_error)?;
            }
        }
        transaction.commit().map_err(write_error)
    }
}

pub(crate) fn referring_tables(connection: &Connection) -> Result<Vec<(String, String)>> {
    connection
        .prepare(REFERRING_TABLES)
        .and_then(|mut query| {
            query
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(read_error)
}

fn recording_of(connection: &Connection, id: i64) -> Result<Option<Recording>> {
    recording_where(connection, id, "AND d.missing = 0")
}

fn recording_held(connection: &Connection, id: i64) -> Result<Option<Recording>> {
    recording_where(connection, id, "")
}

fn recording_where(connection: &Connection, id: i64, condition: &str) -> Result<Option<Recording>> {
    connection
        .query_row(
            &format!(
                "SELECT {} FROM documents d WHERE d.id = ?1 {condition}",
                crate::sync::RECORDING_COLUMNS
            ),
            [id],
            |row| recording_at(row, 0),
        )
        .map(Some)
        .or_else(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            error => Err(error),
        })
        .map_err(read_error)
}

pub(crate) enum Vacated {
    HandedOver(u64),
    Displaced,
}

pub(crate) fn vacated(
    transaction: &Transaction<'_>,
    referring: &[(String, String)],
    holder: i64,
    taker: i64,
) -> Result<Vacated> {
    let names_the_same_song = match (
        recording_held(transaction, holder)?,
        recording_held(transaction, taker)?,
    ) {
        (Some(held), Some(taking)) => held.names_the_same_song_as(&taking),
        _ => false,
    };
    if names_the_same_song {
        handed_over(transaction, referring, holder, taker).map_err(write_error)?;
        return Ok(Vacated::HandedOver(holder as u64));
    }
    transaction
        .execute(
            "UPDATE documents SET uri = ?2, missing = 1 WHERE id = ?1",
            rusqlite::params![holder, format!("{DISPLACED_SCHEME}:{holder}")],
        )
        .map_err(write_error)?;
    Ok(Vacated::Displaced)
}

fn any_missing(connection: &Connection) -> Result<bool> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM documents WHERE missing = 1)",
            [],
            |row| row.get(0),
        )
        .map_err(read_error)
}

fn missing_recordings(
    connection: &Connection,
    absorbing: Absorbing,
) -> Result<Vec<(i64, Recording)>> {
    let withdrawn = match absorbing {
        Absorbing::EveryHidden => "",
        Absorbing::FoundGone => "AND d.withdrawn = 0",
    };
    let mut query = connection
        .prepare(&format!(
            "SELECT d.id, {} FROM documents d WHERE d.missing = 1 {withdrawn} ORDER BY d.id",
            crate::sync::RECORDING_COLUMNS
        ))
        .map_err(read_error)?;
    query
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, recording_at(row, 1)?)))
        .map_err(read_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(read_error)
}

pub(crate) fn handed_over(
    transaction: &Transaction<'_>,
    referring: &[(String, String)],
    old: i64,
    new: i64,
) -> rusqlite::Result<()> {
    transaction.execute(
        "UPDATE playlists SET revision = revision + 1 WHERE id IN (SELECT playlist FROM playlist_rows WHERE track = ?1)",
        [old],
    )?;
    transaction.execute(
        "UPDATE documents SET favourite = 1 WHERE id = ?2 AND (SELECT favourite FROM documents WHERE id = ?1) = 1",
        [old, new],
    )?;
    for (table, column) in referring {
        transaction.execute(
            &format!("UPDATE OR IGNORE \"{table}\" SET \"{column}\" = ?2 WHERE \"{column}\" = ?1"),
            [old, new],
        )?;
        transaction.execute(
            &format!("DELETE FROM \"{table}\" WHERE \"{column}\" = ?1"),
            [old],
        )?;
    }
    transaction.execute("DELETE FROM documents WHERE id = ?1", [old])?;
    Ok(())
}

fn counted(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(catalog: &Catalog, name: &str) -> crate::Track {
        catalog
            .upsert(crate::Track {
                uri: format!("resonate-folder://local/{name}"),
                name: name.into(),
                title: name.into(),
                ..crate::Track::default()
            })
            .expect("upserted")
    }

    #[test]
    fn a_missing_song_leaves_the_lists_and_returns_when_its_file_does() {
        let folder = std::env::temp_dir().join(format!("resonate-vanished-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("a folder");
        let catalog = Catalog::open(&folder.join("catalog.sqlite")).expect("opened");
        let kept = track(&catalog, "kept.flac");
        let lost = track(&catalog, "lost.flac");

        catalog
            .mark_missing(&[lost.id as i64], &[])
            .expect("hidden");

        let shown = catalog.tracks("", 0, 10).expect("listed");
        assert_eq!(
            shown.iter().map(|song| song.id).collect::<Vec<_>>(),
            [kept.id]
        );

        track(&catalog, "lost.flac");
        let shown = catalog.tracks("", 0, 10).expect("listed");
        assert_eq!(shown.len(), 2);

        let _ = std::fs::remove_dir_all(&folder);
    }

    fn song(catalog: &Catalog, name: &str, title: &str) -> crate::Track {
        catalog
            .upsert(crate::Track {
                uri: format!("resonate-folder://local/{name}"),
                name: name.into(),
                title: title.into(),
                artist: "Björk".into(),
                album: "Homogenic".into(),
                duration_ms: 255_000,
                ..crate::Track::default()
            })
            .expect("upserted")
    }

    #[test]
    fn a_song_fetched_again_under_another_name_takes_over_the_missing_one() {
        let folder =
            std::env::temp_dir().join(format!("resonate-vanished-twin-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("a folder");
        let catalog = Catalog::open(&folder.join("catalog.sqlite")).expect("opened");
        let lost = song(&catalog, "Hunter.flac", "Hunter");
        let other = song(&catalog, "Joga.flac", "Jóga");
        catalog.favourite(lost.id, true).expect("favoured");
        catalog
            .0
            .lock()
            .execute_batch(&format!(
                "INSERT INTO playlists(id, name, folded) VALUES (1, 'Mix', 'mix');
                 INSERT INTO playlist_rows(playlist, position, track) VALUES (1, 0, {lost}), (1, 1, {other});
                 INSERT INTO listens(track, began, heard_ms, counted) VALUES ({lost}, 1, 200000, 1);",
                lost = lost.id,
                other = other.id
            ))
            .expect("a playlist and a listen");
        catalog
            .mark_missing(&[lost.id as i64], &[])
            .expect("hidden");

        let back = song(&catalog, "Hunter (1).flac", "Hunter");
        let replaced = catalog
            .take_the_place_of_missing(back.id, Absorbing::EveryHidden)
            .expect("taken over");

        assert_eq!(replaced, [lost.id]);
        assert!(catalog.track(back.id).expect("the song").favourite);
        assert!(catalog.track(lost.id).is_err());
        let connection = catalog.0.lock();
        let rows = connection
            .prepare("SELECT track FROM playlist_rows WHERE playlist = 1 ORDER BY position")
            .and_then(|mut query| {
                query
                    .query_map([], |row| row.get::<_, i64>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .expect("the rows");
        let listened = connection
            .query_row("SELECT track FROM listens", [], |row| row.get::<_, i64>(0))
            .expect("the listen");
        drop(connection);

        assert_eq!(rows, [back.id as i64, other.id as i64]);
        assert_eq!(listened, back.id as i64);

        let _ = std::fs::remove_dir_all(&folder);
    }
}
