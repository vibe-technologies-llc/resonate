use std::{
    collections::BTreeSet,
    path::PathBuf,
    time::{Duration, SystemTime},
};

use resonate_core::{Mbid, Recording, TrackId};
use rusqlite::{Connection, Transaction, params};

use crate::{Error, Result, StoreOp, db::Inner, store, synced};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeletedEverywhere {
    pub id: i64,
    pub recording: Recording,
}

pub(crate) struct Copy {
    pub(crate) track: TrackId,
    pub(crate) added: i64,
    pub(crate) received: bool,
    pub(crate) whole_file: bool,
}

pub(crate) fn to_be_told(tx: &Transaction<'_>, recording: &Recording) -> Result<()> {
    let length = recording
        .length
        .map(|length| i64::try_from(length.as_millis()).unwrap_or(i64::MAX));
    tx.execute(
        "INSERT INTO songs_deleted_everywhere (title, artist, album, mbid, length, deleted)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            recording.title,
            recording.artist,
            recording.album,
            recording.mbid.as_ref().map(Mbid::as_str),
            length,
            store::to_nanos(SystemTime::now())
        ],
    )
    .map(drop)
    .map_err(|source| Error::store(StoreOp::Insert, source))
}

pub(crate) fn waiting_to_be_told(inner: &Inner) -> Result<Vec<DeletedEverywhere>> {
    inner.read(|connection| {
        let mut statement = connection
            .prepare_cached(
                "SELECT id, title, artist, album, mbid, length FROM songs_deleted_everywhere ORDER BY id",
            )
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;
        statement
            .query_map([], |row| {
                Ok(DeletedEverywhere {
                    id: row.get(0)?,
                    recording: Recording {
                        title: row.get(1)?,
                        artist: row.get(2)?,
                        album: row.get(3)?,
                        mbid: row
                            .get::<_, Option<String>>(4)?
                            .and_then(|mbid| Mbid::new(&mbid).ok()),
                        length: row
                            .get::<_, Option<i64>>(5)?
                            .and_then(|millis| u64::try_from(millis).ok())
                            .map(Duration::from_millis),
                    },
                })
            })
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))
    })
}

pub(crate) fn told(inner: &Inner, ids: &[i64]) -> Result<()> {
    inner.write(|transaction| {
        let mut statement = transaction
            .prepare_cached("DELETE FROM songs_deleted_everywhere WHERE id = ?1")
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;

        for id in ids {
            statement
                .execute(params![id])
                .map_err(|source| Error::store(StoreOp::Delete, source))?;
        }
        Ok(())
    })
}

pub(crate) fn copies_of(inner: &Inner, recording: &Recording) -> Result<Vec<Copy>> {
    inner.read(|connection| {
        let paths: BTreeSet<PathBuf> = synced::every_row_holding(connection, recording)?
            .into_iter()
            .map(|row| row.path)
            .collect();
        let mut copies = Vec::new();

        for path in paths {
            copies.extend(copies_at(connection, &store::path_text(&path))?);
        }
        Ok(copies)
    })
}

fn copies_at(connection: &Connection, path: &str) -> Result<Vec<Copy>> {
    let mut statement = connection
        .prepare_cached(
            "SELECT t.id, t.added,
                    EXISTS (SELECT 1 FROM songs_received s WHERE s.path = t.path),
                    (SELECT count(*) FROM tracks u WHERE u.path = t.path) = 1 AND t.span_frames IS NULL
               FROM tracks t WHERE t.path = ?1",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;

    let rows = statement
        .query_map(params![path], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, bool>(2)?,
                row.get::<_, bool>(3)?,
            ))
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, added, received, whole_file)| {
            let track = u64::try_from(id)
                .ok()
                .and_then(|id| TrackId::new(id).ok())?;
            Some(Copy {
                track,
                added,
                received,
                whole_file,
            })
        })
        .collect())
}
