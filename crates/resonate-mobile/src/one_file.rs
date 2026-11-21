use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{OptionalExtension, params};

use crate::{
    Error, MobilePlayer, Result, StoreOp, Track,
    catalog::{Catalog, read},
    deleting::FileIdentity,
    documents::document_uri,
};

const LASTING_ALIKE: &str = "SELECT * FROM documents
    WHERE missing = 0 AND rate = ?3 AND duration_ms = ?4 AND ?4 > 0 AND uri != ?1
    ORDER BY id";
const OF_NO_LENGTH_NAMED_ALIKE: &str = "SELECT * FROM documents
    WHERE folded(name) = folded(?2)
      AND missing = 0 AND rate = ?3 AND duration_ms = 0 AND ?4 = 0 AND uri != ?1
    ORDER BY id";
const SHARING_A_LENGTH: &str = "SELECT * FROM documents
    WHERE missing = 0 AND duration_ms > 0 AND (rate, duration_ms) IN (
        SELECT rate, duration_ms FROM documents WHERE missing = 0 AND duration_ms > 0
        GROUP BY rate, duration_ms HAVING count(*) > 1)
    UNION ALL
    SELECT * FROM documents
    WHERE missing = 0 AND duration_ms = 0 AND (rate, folded(name)) IN (
        SELECT rate, folded(name) FROM documents WHERE missing = 0 AND duration_ms = 0
        GROUP BY rate, folded(name) HAVING count(*) > 1)
    ORDER BY id";

fn read_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Read, source)
}

fn write_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Write, source)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Spelled {
    AsListed,
    AsHeld,
    Anew,
}

pub(crate) struct Probed {
    pub(crate) rate: u32,
    pub(crate) duration_ms: u64,
}

impl MobilePlayer {
    pub(crate) fn spelled_as_held(
        &self,
        uri: String,
        name: &str,
        spelled: Spelled,
        probed: &Probed,
    ) -> Result<String> {
        if spelled == Spelled::Anew {
            return Ok(uri);
        }

        let Some(held) = self.row_naming_the_file(&uri, name, probed)? else {
            return Ok(uri);
        };
        let respelled = spelled == Spelled::AsListed
            && !self.queued_uris().contains(&held.uri)
            && self.catalog.respelled(held.id, &uri)?;
        Ok(if respelled { uri } else { held.uri })
    }

    fn row_naming_the_file(&self, uri: &str, name: &str, probed: &Probed) -> Result<Option<Track>> {
        if self.catalog.holds_uri(uri)? {
            return Ok(None);
        }

        let alike = self.catalog.lasting_alike(uri, name, probed)?;

        if alike.is_empty() {
            return Ok(None);
        }

        let Some(file) = self.services.files.identity_of(uri) else {
            return Ok(None);
        };
        Ok(alike
            .into_iter()
            .find(|held| self.services.files.identity_of(&held.uri) == Some(file)))
    }

    pub(crate) fn fold_rows_naming_one_file(&self) -> Result<usize> {
        let rows = self.catalog.sharing_a_length()?;

        if rows.len() < 2 {
            return Ok(0);
        }

        let queued = self.queued_uris();
        let playing = self.playing_uri();

        let mut by_file = BTreeMap::<FileIdentity, Vec<Track>>::new();

        for row in rows {
            if let Some(identity) = self.services.files.identity_of(&row.uri) {
                by_file.entry(identity).or_default().push(row);
            }
        }

        let mut folded = Vec::new();

        for naming_one_file in by_file.into_values().filter(|rows| rows.len() > 1) {
            let kept = kept_of(&naming_one_file, playing.as_deref(), &queued);
            let others = naming_one_file
                .iter()
                .filter(|row| row.id != kept && !queued.contains(&row.uri))
                .map(|row| row.id)
                .collect::<Vec<_>>();

            self.catalog.fold_into(kept, &others)?;
            folded.extend(others);
        }
        self.forget_analyses(folded.iter());
        Ok(folded.len())
    }

    fn queued_uris(&self) -> BTreeSet<String> {
        self.playing
            .listed()
            .iter()
            .flat_map(|zone| {
                zone.player
                    .queued()
                    .rows
                    .iter()
                    .filter_map(|row| document_uri(&row.location).ok())
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

fn kept_of(naming_one_file: &[Track], playing: Option<&str>, queued: &BTreeSet<String>) -> u64 {
    naming_one_file
        .iter()
        .min_by_key(|row| {
            (
                Some(row.uri.as_str()) != playing,
                !queued.contains(&row.uri),
                row.id,
            )
        })
        .map_or(0, |row| row.id)
}

impl Catalog {
    fn holds_uri(&self, uri: &str) -> Result<bool> {
        self.0
            .lock()
            .query_row("SELECT 1 FROM documents WHERE uri = ?1", [uri], |_| Ok(()))
            .optional()
            .map(|held| held.is_some())
            .map_err(read_error)
    }

    fn lasting_alike(&self, uri: &str, name: &str, probed: &Probed) -> Result<Vec<Track>> {
        let connection = self.0.lock();
        let mut found = Vec::new();

        for alike in [LASTING_ALIKE, OF_NO_LENGTH_NAMED_ALIKE] {
            let mut query = connection.prepare_cached(alike).map_err(read_error)?;

            let rows = query
                .query_map(
                    params![
                        uri,
                        name,
                        probed.rate,
                        probed.duration_ms.min(i64::MAX as u64) as i64
                    ],
                    read,
                )
                .map_err(read_error)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(read_error)?;

            found.extend(rows);
        }
        Ok(found)
    }

    fn sharing_a_length(&self) -> Result<Vec<Track>> {
        let connection = self.0.lock();
        let mut query = connection.prepare(SHARING_A_LENGTH).map_err(read_error)?;
        query
            .query_map([], read)
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)
    }

    fn respelled(&self, id: u64, uri: &str) -> Result<bool> {
        self.0
            .lock()
            .execute(
                "UPDATE documents SET uri = ?2 WHERE id = ?1
                   AND NOT EXISTS (SELECT 1 FROM documents WHERE uri = ?2)",
                params![id as i64, uri],
            )
            .map(|changed| changed > 0)
            .map_err(write_error)
    }
}
