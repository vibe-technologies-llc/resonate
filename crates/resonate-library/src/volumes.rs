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

const WHERE_DESKTOPS_MOUNT: [(&str, usize); 3] = [("/run/media", 2), ("/media", 2), ("/mnt", 1)];

pub fn is_under_a_mount_point_not_there(path: &Path) -> bool {
    WHERE_DESKTOPS_MOUNT
        .iter()
        .any(|(base, deepest)| under_a_mount_point_not_there_below(path, Path::new(base), *deepest))
}

fn under_a_mount_point_not_there_below(path: &Path, base: &Path, deepest: usize) -> bool {
    let Ok(below) = path.strip_prefix(base) else {
        return false;
    };
    let mut candidate = base.to_path_buf();

    for component in below.components().take(deepest) {
        candidate.push(component);
        if !candidate.exists() {
            return true;
        }
        if is_mounted(&candidate) {
            return false;
        }
    }
    false
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
        forget(tx, &volume)?;
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

pub fn retire_at_or_under(tx: &Transaction<'_>, folder: &Path) -> Result<()> {
    for volume in held(tx)? {
        if !volume.starts_with(folder) || holds_a_row(tx, &volume)? {
            continue;
        }
        forget(tx, &volume)?;
    }
    Ok(())
}

fn forget(tx: &Transaction<'_>, volume: &Path) -> Result<()> {
    tx.execute(
        "DELETE FROM volumes WHERE path = ?1",
        params![store::path_text(volume)?],
    )
    .map(drop)
    .map_err(|source| Error::store(StoreOp::Delete, source))
}

#[cfg(test)]
mod tests {
    use std::{env, process};

    use super::*;

    #[test]
    fn a_path_under_a_mount_point_nothing_is_mounted_at_is_out_of_reach_though_its_siblings_stand()
    {
        let base = env::temp_dir().join(format!("resonate-volumes-{}", process::id()));
        fs::create_dir_all(base.join("me/Other")).expect("a writable temporary folder");

        let music = base.join("me/Stick/Music/a.flac");
        let kept = base.join("me/Other/a.flac");

        assert!(under_a_mount_point_not_there_below(&music, &base, 2));
        assert!(!under_a_mount_point_not_there_below(&kept, &base, 2));
        assert!(!under_a_mount_point_not_there_below(
            Path::new("/elsewhere/me/Stick/a.flac"),
            &base,
            2
        ));
        assert!(under_a_mount_point_not_there_below(
            &base.join("Stick/a.flac"),
            &base,
            1
        ));

        fs::remove_dir_all(&base).expect("the temporary folder goes");
    }
}
