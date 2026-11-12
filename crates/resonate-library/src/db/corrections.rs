use std::time::SystemTime;

use rusqlite::{OptionalExtension as _, params};

use super::Library;
use crate::{Error, KeptCorrection, KeptIndex, Result, StoreOp, store};

impl Library {
    pub fn kept_corrections_index(&self) -> Result<Option<KeptIndex>> {
        self.inner.read(|connection| {
            connection
                .query_row(
                    "SELECT text, taken FROM corrections_index WHERE id = 1",
                    [],
                    |row| {
                        Ok(KeptIndex {
                            text: row.get(0)?,
                            taken: store::from_nanos(row.get(1)?),
                        })
                    },
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))
        })
    }

    pub fn keep_corrections_index(&self, text: &str) -> Result<()> {
        self.inner.write(|transaction| {
            transaction
                .execute(
                    "INSERT INTO corrections_index (id, text, taken)
                     VALUES (1, ?1, ?2)
                     ON CONFLICT(id) DO UPDATE SET
                         text  = excluded.text,
                         taken = excluded.taken",
                    params![text, store::to_nanos(SystemTime::now())],
                )
                .map(drop)
                .map_err(|source| Error::store(StoreOp::Insert, source))
        })
    }

    pub fn kept_correction(&self, device: &str) -> Result<Option<KeptCorrection>> {
        self.inner.read(|connection| {
            connection
                .query_row(
                    "SELECT text, taken FROM corrections_kept WHERE device = ?1",
                    params![device],
                    |row| {
                        Ok(KeptCorrection {
                            text: row.get(0)?,
                            taken: store::from_nanos(row.get(1)?),
                        })
                    },
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))
        })
    }

    pub fn keep_correction(&self, device: &str, text: Option<&str>) -> Result<()> {
        self.inner.write(|transaction| {
            transaction
                .execute(
                    "INSERT INTO corrections_kept (device, text, taken)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(device) DO UPDATE SET
                         text  = excluded.text,
                         taken = excluded.taken",
                    params![device, text, store::to_nanos(SystemTime::now())],
                )
                .map(drop)
                .map_err(|source| Error::store(StoreOp::Insert, source))
        })
    }
}
