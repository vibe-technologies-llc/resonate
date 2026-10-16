use rusqlite::{OptionalExtension, params};

use crate::{
    Error, Result, StoreOp, Track,
    catalog::{Catalog, read},
};

pub(crate) const INDEXING_REVISION: i64 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DocumentStamp {
    size: u64,
    modified_ms: i64,
}

impl DocumentStamp {
    pub(crate) fn of(size: u64, modified_ms: i64) -> Option<Self> {
        (size != 0 || modified_ms != 0).then_some(Self { size, modified_ms })
    }

    pub(crate) fn size_column(self) -> i64 {
        self.size.min(i64::MAX as u64) as i64
    }

    pub(crate) const fn modified_column(self) -> i64 {
        self.modified_ms
    }
}

impl Catalog {
    pub(crate) fn unchanged(&self, uri: &str, stamp: DocumentStamp) -> Result<Option<Track>> {
        self.0
            .lock()
            .query_row(
                "SELECT * FROM documents WHERE uri = ?1 AND missing = 0 AND size = ?2 AND modified = ?3 AND indexed_by = ?4",
                params![
                    uri,
                    stamp.size_column(),
                    stamp.modified_column(),
                    INDEXING_REVISION
                ],
                read,
            )
            .optional()
            .map_err(|source| Error::store(StoreOp::Read, source))
    }
}
