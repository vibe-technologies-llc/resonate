use std::{path::Path, time::Duration};

use resonate_core::Mbid;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use super::documents::{Document, words_of};
use crate::{
    Entity, Error, PackOp, Result, Store, StoreOp,
    wire::{MBID_BYTES, mbid_bytes},
};

pub const AMENDED_FILE: &str = "musicbrainz.amended.sqlite";
pub const AMENDED_PACKED_AT: i32 = 15;
const WAITED_FOR_A_WRITER: Duration = Duration::from_secs(5);

pub(crate) const AMENDED_SCHEMA: &str = "
CREATE TABLE amended_from (dump TEXT NOT NULL, sequence INTEGER NOT NULL, at INTEGER NOT NULL);
CREATE TABLE documents (
    id INTEGER PRIMARY KEY,
    kind INTEGER NOT NULL,
    mbid BLOB NOT NULL,
    packed BLOB,
    UNIQUE (kind, mbid)
);
CREATE TABLE isrcs (isrc TEXT NOT NULL, id INTEGER NOT NULL, PRIMARY KEY (isrc, id)) WITHOUT ROWID;
CREATE INDEX isrcs_by_id ON isrcs(id);
CREATE TABLE barcodes (barcode TEXT NOT NULL, id INTEGER NOT NULL, PRIMARY KEY (barcode, id)) WITHOUT ROWID;
CREATE INDEX barcodes_by_id ON barcodes(id);
CREATE VIRTUAL TABLE recording_words USING fts5(words, detail=none, tokenize='unicode61 remove_diacritics 2');
CREATE VIRTUAL TABLE release_words USING fts5(words, detail=none, tokenize='unicode61 remove_diacritics 2');
CREATE VIRTUAL TABLE group_words USING fts5(words, detail=none, tokenize='unicode61 remove_diacritics 2');
CREATE VIRTUAL TABLE artist_words USING fts5(words, detail=none, tokenize='unicode61 remove_diacritics 2');
";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Amendment {
    Rewritten(i64),
    Gone,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AmendedTo {
    pub dump: String,
    pub sequence: u64,
    pub at: u64,
}

fn failed(op: StoreOp) -> impl FnOnce(rusqlite::Error) -> Error {
    Error::store(Store::Amended, op)
}

const fn codes_of(kind: Entity) -> Option<&'static str> {
    match kind {
        Entity::Release => Some("barcodes"),
        Entity::Recording => Some("isrcs"),
        Entity::Group | Entity::Artist => None,
    }
}

const fn code_column(kind: Entity) -> &'static str {
    match kind {
        Entity::Release => "barcode",
        Entity::Recording | Entity::Group | Entity::Artist => "isrc",
    }
}

pub(crate) fn made_at(path: &Path, dump: &str, sequence: u64, at: u64) -> Result<()> {
    let amended = Connection::open(path).map_err(failed(StoreOp::Open))?;
    let laid: bool = amended
        .query_row(
            "SELECT count(*) > 0 FROM sqlite_schema WHERE name = 'amended_from'",
            [],
            |row| row.get(0),
        )
        .map_err(failed(StoreOp::Read))?;
    if !laid {
        amended
            .execute_batch(AMENDED_SCHEMA)
            .map_err(failed(StoreOp::LayOut))?;
        amended
            .execute(
                "INSERT INTO amended_from (dump, sequence, at) VALUES (?1, ?2, ?3)",
                params![
                    dump,
                    i64::try_from(sequence).unwrap_or(i64::MAX),
                    i64::try_from(at).unwrap_or(i64::MAX)
                ],
            )
            .map_err(failed(StoreOp::Write))?;
    }
    Ok(())
}

pub(crate) fn amended_to_in(connection: &Connection, prefix: &str) -> Result<Option<AmendedTo>> {
    connection
        .query_row(
            &format!("SELECT dump, sequence, at FROM {prefix}amended_from"),
            [],
            |row| {
                Ok(AmendedTo {
                    dump: row.get(0)?,
                    sequence: u64::try_from(row.get::<_, i64>(1)?).unwrap_or(0),
                    at: u64::try_from(row.get::<_, i64>(2)?).unwrap_or(0),
                })
            },
        )
        .optional()
        .map_err(failed(StoreOp::Read))
}

pub(crate) struct Amending<'a> {
    pub connection: &'a Connection,
    pub dictionaries: &'a [(Entity, Vec<u8>)],
}

impl Amending<'_> {
    fn packed(&self, kind: Entity, bytes: &[u8]) -> Result<Vec<u8>> {
        let pack = |source| Error::Pack {
            op: PackOp::Pack,
            source,
        };
        let mut compressor = match self.dictionaries.iter().find(|(held, _)| *held == kind) {
            Some((_, dictionary)) => {
                zstd::bulk::Compressor::with_dictionary(AMENDED_PACKED_AT, dictionary)
                    .map_err(pack)?
            }
            None => zstd::bulk::Compressor::new(AMENDED_PACKED_AT).map_err(pack)?,
        };
        compressor.compress(bytes).map_err(pack)
    }

    fn held(&self, kind: Entity, gid: &[u8; MBID_BYTES]) -> Result<Option<i64>> {
        self.connection
            .prepare_cached("SELECT id FROM amended.documents WHERE kind = ?1 AND mbid = ?2")
            .and_then(|mut statement| {
                statement
                    .query_row(params![kind.code(), gid.to_vec()], |row| row.get(0))
                    .optional()
            })
            .map_err(failed(StoreOp::Read))
    }

    fn cleared(&self, kind: Entity, id: i64) -> Result<()> {
        let write = || failed(StoreOp::Write);
        if let Some(codes) = codes_of(kind) {
            self.connection
                .prepare_cached(&format!("DELETE FROM amended.{codes} WHERE id = ?1"))
                .and_then(|mut statement| statement.execute([id]))
                .map_err(write())?;
        }
        self.connection
            .prepare_cached(&format!(
                "DELETE FROM amended.{} WHERE rowid = ?1",
                words_of(kind)
            ))
            .and_then(|mut statement| statement.execute([id]))
            .map_err(write())?;
        Ok(())
    }

    pub(crate) fn rewritten(&self, kind: Entity, built: &Document) -> Result<()> {
        let write = || failed(StoreOp::Write);
        let packed = self.packed(kind, &built.bytes)?;
        let id = match self.held(kind, &built.gid)? {
            Some(id) => {
                self.cleared(kind, id)?;
                self.connection
                    .prepare_cached("UPDATE amended.documents SET packed = ?2 WHERE id = ?1")
                    .and_then(|mut statement| statement.execute(params![id, packed]))
                    .map_err(write())?;
                id
            }
            None => {
                self.connection
                    .prepare_cached(
                        "INSERT INTO amended.documents (kind, mbid, packed) VALUES (?1, ?2, ?3)",
                    )
                    .and_then(|mut statement| {
                        statement.execute(params![kind.code(), built.gid.to_vec(), packed])
                    })
                    .map_err(write())?;
                self.connection.last_insert_rowid()
            }
        };
        if let Some(codes) = codes_of(kind) {
            for code in &built.codes {
                self.connection
                    .prepare_cached(&format!(
                        "INSERT OR IGNORE INTO amended.{codes} ({}, id) VALUES (?1, ?2)",
                        code_column(kind)
                    ))
                    .and_then(|mut statement| statement.execute(params![code, id]))
                    .map_err(write())?;
            }
        }
        self.connection
            .prepare_cached(&format!(
                "INSERT INTO amended.{} (rowid, words) VALUES (?1, ?2)",
                words_of(kind)
            ))
            .and_then(|mut statement| statement.execute(params![id, built.words]))
            .map_err(write())?;
        Ok(())
    }

    pub(crate) fn gone(
        &self,
        kind: Entity,
        gid: &[u8; MBID_BYTES],
        built_holds: bool,
    ) -> Result<()> {
        let write = || failed(StoreOp::Write);
        let held = self.held(kind, gid)?;
        if let Some(id) = held {
            self.cleared(kind, id)?;
        }
        match (held, built_holds) {
            (Some(id), true) => self
                .connection
                .prepare_cached("UPDATE amended.documents SET packed = NULL WHERE id = ?1")
                .and_then(|mut statement| statement.execute([id]))
                .map(drop)
                .map_err(write()),
            (Some(id), false) => self
                .connection
                .prepare_cached("DELETE FROM amended.documents WHERE id = ?1")
                .and_then(|mut statement| statement.execute([id]))
                .map(drop)
                .map_err(write()),
            (None, true) => self
                .connection
                .prepare_cached(
                    "INSERT INTO amended.documents (kind, mbid, packed) VALUES (?1, ?2, NULL)",
                )
                .and_then(|mut statement| statement.execute(params![kind.code(), gid.to_vec()]))
                .map(drop)
                .map_err(write()),
            (None, false) => Ok(()),
        }
    }

    pub(crate) fn sequenced_at(&self, sequence: u64, at: u64) -> Result<()> {
        self.connection
            .execute(
                "UPDATE amended.amended_from SET sequence = ?1, at = ?2",
                params![
                    i64::try_from(sequence).unwrap_or(i64::MAX),
                    i64::try_from(at).unwrap_or(i64::MAX)
                ],
            )
            .map(drop)
            .map_err(failed(StoreOp::Write))
    }
}

pub(crate) struct Amended {
    connection: Connection,
    pub to: AmendedTo,
}

impl Amended {
    pub(crate) fn open(path: &Path, dump: &str) -> Result<Option<Self>> {
        if !path.is_file() {
            return Ok(None);
        }
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(failed(StoreOp::Open))?;
        connection
            .busy_timeout(WAITED_FOR_A_WRITER)
            .map_err(failed(StoreOp::Configure))?;
        let Some(to) = amended_to_in(&connection, "")? else {
            return Ok(None);
        };
        if to.dump != dump {
            tracing::info!(
                amended = %to.dump,
                built = %dump,
                "the changes kept beside the copy of MusicBrainz are of another export"
            );
            return Ok(None);
        }
        Ok(Some(Self { connection, to }))
    }

    pub(crate) fn placed(&self, id: &Mbid, kind: Entity) -> Result<Option<Amendment>> {
        let found: Option<(i64, bool)> = self
            .connection
            .prepare_cached(
                "SELECT id, packed IS NULL FROM documents WHERE kind = ?1 AND mbid = ?2",
            )
            .and_then(|mut statement| {
                statement
                    .query_row(params![kind.code(), mbid_bytes(id).to_vec()], |row| {
                        Ok((row.get(0)?, row.get(1)?))
                    })
                    .optional()
            })
            .map_err(failed(StoreOp::Read))?;
        Ok(found.map(|(at, gone)| {
            if gone {
                Amendment::Gone
            } else {
                Amendment::Rewritten(at)
            }
        }))
    }

    pub(crate) fn packed(&self, at: i64) -> Result<Option<(Entity, Vec<u8>)>> {
        let found: Option<(u8, Option<Vec<u8>>)> = self
            .connection
            .prepare_cached("SELECT kind, packed FROM documents WHERE id = ?1")
            .and_then(|mut statement| {
                statement
                    .query_row([at], |row| Ok((row.get(0)?, row.get(1)?)))
                    .optional()
            })
            .map_err(failed(StoreOp::Read))?;
        Ok(found.and_then(|(kind, packed)| Entity::coded(kind).zip(packed)))
    }

    pub(crate) fn coded(&self, kind: Entity, code: &str) -> Result<Vec<i64>> {
        let Some(codes) = codes_of(kind) else {
            return Ok(Vec::new());
        };
        let mut statement = self
            .connection
            .prepare_cached(&format!(
                "SELECT id FROM {codes} WHERE {} = ?1",
                code_column(kind)
            ))
            .map_err(failed(StoreOp::Read))?;
        let rows = statement
            .query_map([code], |row| row.get(0))
            .map_err(failed(StoreOp::Read))?;
        rows.collect::<rusqlite::Result<Vec<i64>>>()
            .map_err(failed(StoreOp::Read))
    }

    pub(crate) fn matched(&self, table: &str, query: &str, most: usize) -> Result<Vec<i64>> {
        let sql =
            format!("SELECT rowid FROM {table} WHERE {table} MATCH ?1 ORDER BY rank LIMIT ?2");
        let mut statement = self
            .connection
            .prepare_cached(&sql)
            .map_err(failed(StoreOp::Search))?;
        let rows = statement
            .query_map(
                params![query, i64::try_from(most).unwrap_or(i64::MAX)],
                |row| row.get(0),
            )
            .map_err(failed(StoreOp::Search))?;
        rows.collect::<rusqlite::Result<Vec<i64>>>()
            .map_err(failed(StoreOp::Search))
    }

    pub(crate) fn supersedes(&self, kind: Entity, id: &Mbid) -> Result<bool> {
        Ok(self.placed(id, kind)?.is_some())
    }
}
