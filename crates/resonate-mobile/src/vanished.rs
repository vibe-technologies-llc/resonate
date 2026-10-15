use crate::{Error, MobilePlayer, Result, StoreOp, documents::location};

const KEPT_FOR_SYNC: &str = "coalesce(d.favourite, 0) = 0 AND NOT EXISTS (SELECT 1 FROM playlist_rows r WHERE r.track = d.id)";
const REFERRING_TABLES: &str = "SELECT m.name, f.\"from\" FROM sqlite_master m, pragma_foreign_key_list(m.name) f WHERE m.type = 'table' AND f.\"table\" = 'documents'";

fn read_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Read, source)
}

fn write_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Write, source)
}

#[uniffi::export]
impl MobilePlayer {
    pub fn forget_missing_songs(&self, prefixes: Vec<String>) -> Result<u32> {
        let candidates = {
            let connection = self.catalog.0.lock();
            let mut query = connection
                .prepare(&format!(
                    "SELECT d.id, d.uri FROM documents d WHERE {KEPT_FOR_SYNC}"
                ))
                .map_err(read_error)?;
            query
                .query_map([], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(read_error)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(read_error)?
        };
        let gone = candidates
            .into_iter()
            .filter(|(_, uri)| prefixes.iter().any(|prefix| uri.starts_with(prefix)))
            .filter(|(_, uri)| location(uri).is_ok_and(|at| self.sources.open(&at).is_err()))
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        if gone.is_empty() {
            return Ok(0);
        }

        let mut connection = self.catalog.0.lock();
        let referring = connection
            .prepare(REFERRING_TABLES)
            .and_then(|mut query| {
                query
                    .query_map([], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(read_error)?;
        let transaction = connection.transaction().map_err(write_error)?;
        for id in &gone {
            for (table, column) in &referring {
                transaction
                    .execute(
                        &format!("DELETE FROM \"{table}\" WHERE \"{column}\" = ?1"),
                        [id],
                    )
                    .map_err(write_error)?;
            }
            transaction
                .execute("DELETE FROM documents WHERE id = ?1", [id])
                .map_err(write_error)?;
        }
        transaction.commit().map_err(write_error)?;
        drop(connection);

        let mut analyses = self.services.analyses.lock();
        for id in &gone {
            analyses.remove(&(*id as u64));
        }
        Ok(u32::try_from(gone.len()).unwrap_or(u32::MAX))
    }
}
