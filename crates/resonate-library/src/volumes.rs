use std::{
    fs,
    os::unix::fs::MetadataExt as _,
    path::{Path, PathBuf},
};

use ahash::AHashSet;
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

pub(crate) const MOUNT_TABLE: &str = "/proc/self/mounts";
const FILESYSTEM_TABLE: &str = "/etc/fstab";
const OCTAL_ESCAPE: char = '\\';
const COMMENT: char = '#';
const THE_ROOT: &str = "/";

pub(crate) fn mount_points_in(table: &str) -> impl Iterator<Item = (PathBuf, &str)> {
    table.lines().filter_map(|line| {
        if line.trim_start().starts_with(COMMENT) {
            return None;
        }
        let mut fields = line.split_whitespace();
        let _device = fields.next()?;
        let point = PathBuf::from(unescaped_mount(fields.next()?));
        let kind = fields.next()?;
        Some((point, kind))
    })
}

fn unescaped_mount(field: &str) -> String {
    let mut written = String::with_capacity(field.len());
    let mut characters = field.chars().peekable();
    while let Some(character) = characters.next() {
        if character != OCTAL_ESCAPE {
            written.push(character);
            continue;
        }
        let digits: String = (0..3).filter_map(|_| characters.next()).collect();
        match u8::from_str_radix(&digits, 8) {
            Ok(byte) => written.push(char::from(byte)),
            Err(_) => {
                written.push(character);
                written.push_str(&digits);
            }
        }
    }
    written
}

pub fn listed_and_not_mounted() -> Vec<PathBuf> {
    let (Ok(listed), Ok(mounted)) = (
        fs::read_to_string(FILESYSTEM_TABLE),
        fs::read_to_string(MOUNT_TABLE),
    ) else {
        return Vec::new();
    };
    not_mounted_of(&listed, &mounted)
}

fn not_mounted_of(listed: &str, mounted: &str) -> Vec<PathBuf> {
    let mounted: AHashSet<PathBuf> = mount_points_in(mounted).map(|(point, _)| point).collect();
    mount_points_in(listed)
        .map(|(point, _)| point)
        .filter(|point| point.is_absolute() && point != Path::new(THE_ROOT))
        .filter(|point| !mounted.contains(point))
        .collect()
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

    #[test]
    fn a_mount_point_the_filesystem_table_lists_and_nothing_is_mounted_at_is_out_of_reach() {
        let listed = "# /etc/fstab\n\
                      UUID=1 / btrfs subvol=@ 0 0\n\
                      UUID=1 /home btrfs subvol=@home 0 0\n\
                      UUID=2 /data/Music\\040Drive ext4 noauto 0 2\n\
                      UUID=3 none swap sw 0 0\n\
                      \n\
                      //nas/media /srv/nas cifs noauto 0 0\n";
        let mounted = "/dev/nvme0n1p2 / btrfs rw 0 0\n\
                       /dev/nvme0n1p2 /home btrfs rw 0 0\n\
                       //nas/media /srv/nas cifs rw 0 0\n";

        assert_eq!(
            not_mounted_of(listed, mounted),
            vec![PathBuf::from("/data/Music Drive")]
        );
    }
}
