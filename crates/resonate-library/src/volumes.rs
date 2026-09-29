use std::{
    fs,
    os::unix::fs::MetadataExt as _,
    path::{Path, PathBuf},
};

use rusqlite::{Connection, Transaction, params};

use crate::{Error, Result, StoreOp, scan::walked_from, store};

pub fn device(path: &Path) -> Option<u64> {
    fs::metadata(path).ok().map(|metadata| metadata.dev())
}

pub fn is_mounted(volume: &Path) -> bool {
    let inner = device(volume);
    let outer = volume.parent().and_then(device);
    inner.is_some() && outer.is_some() && inner != outer
}

pub fn held(connection: &Connection) -> Result<Vec<PathBuf>> {
    connection
        .prepare("SELECT path FROM volumes ORDER BY path")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(0).map(PathBuf::from))
                .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        })
        .map_err(|source| Error::store(StoreOp::Query, source))
}

pub fn absent(connection: &Connection) -> Result<Vec<PathBuf>> {
    Ok(held(connection)?
        .into_iter()
        .filter(|volume| !is_mounted(volume))
        .collect())
}

pub fn is_on_an_absent_one(path: &Path, absent: &[PathBuf]) -> bool {
    absent.iter().any(|volume| path.starts_with(volume))
}

pub fn settle(tx: &Transaction<'_>, walked: &[PathBuf], mounted: &[PathBuf]) -> Result<()> {
    for volume in mounted {
        tx.execute(
            "INSERT OR IGNORE INTO volumes (path) VALUES (?1)",
            params![store::path_text(volume)?],
        )
        .map_err(|source| Error::store(StoreOp::Insert, source))?;
    }

    for volume in held(tx)? {
        let unwalked = !walked.iter().any(|root| volume.starts_with(root));
        if unwalked || mounted.contains(&volume) || holds_a_row(tx, &volume)? {
            continue;
        }
        tx.execute(
            "DELETE FROM volumes WHERE path = ?1",
            params![store::path_text(&volume)?],
        )
        .map_err(|source| Error::store(StoreOp::Delete, source))?;
    }
    Ok(())
}

fn holds_a_row(connection: &Connection, volume: &Path) -> Result<bool> {
    let (from, past) = walked_from(store::path_text(volume)?);
    connection
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM tracks WHERE path >= ?1 AND path < ?2)",
            params![from, past],
            |row| row.get(0),
        )
        .map_err(|source| Error::store(StoreOp::Query, source))
}
