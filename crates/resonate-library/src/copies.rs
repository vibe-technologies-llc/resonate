use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    path::{Path, PathBuf},
    time::SystemTime,
};

use resonate_core::{MediaLocation, TrackId};
use rusqlite::{Connection, OptionalExtension as _, ToSql, Transaction, params};

use crate::{
    Error, Result, StoreOp,
    identical::{identical_among, names_a_numbered_copy},
    store,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdenticalCopy {
    pub kept: PathBuf,
    pub copy: PathBuf,
    kept_id: TrackId,
    copy_id: TrackId,
}

impl IdenticalCopy {
    pub const fn copy_id(&self) -> TrackId {
        self.copy_id
    }

    pub(crate) fn hand_over(&self) -> HandOver<'_> {
        HandOver {
            kept: self.kept_id,
            kept_path: &self.kept,
            taken: self.copy_id,
            taken_path: &self.copy,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct HandOver<'a> {
    pub(crate) kept: TrackId,
    pub(crate) kept_path: &'a Path,
    pub(crate) taken: TrackId,
    pub(crate) taken_path: &'a Path,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Copies {
    pub identical: Vec<IdenticalCopy>,
    pub differing: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug)]
struct Whole {
    id: TrackId,
    size: i64,
}

#[derive(Clone, Copy, Debug)]
struct Member<'a> {
    path: &'a PathBuf,
    whole: Whole,
}

pub(crate) fn found(connection: &Connection) -> Result<Copies> {
    let wholes = whole_files(connection)?;
    let mut copies = Copies::default();
    let mut held_in_a_class: BTreeSet<&PathBuf> = BTreeSet::new();

    for class in identical_classes(&wholes) {
        let (kept, rest) = class
            .split_first()
            .expect("a class holds at least two files");

        for copy in rest {
            held_in_a_class.insert(copy.path);
            copies.identical.push(IdenticalCopy {
                kept: kept.path.clone(),
                copy: copy.path.clone(),
                kept_id: kept.whole.id,
                copy_id: copy.whole.id,
            });
        }
        held_in_a_class.insert(kept.path);
    }
    copies
        .identical
        .sort_by(|one, other| one.copy.cmp(&other.copy));

    copies.differing = wholes
        .keys()
        .filter(|path| is_numbered(path) && !held_in_a_class.contains(path))
        .cloned()
        .collect();
    Ok(copies)
}

fn identical_classes(wholes: &BTreeMap<PathBuf, Whole>) -> Vec<Vec<Member<'_>>> {
    let mut by_size: BTreeMap<i64, Vec<Member<'_>>> = BTreeMap::new();

    for (path, whole) in wholes {
        if whole.size > 0 {
            by_size.entry(whole.size).or_default().push(Member {
                path,
                whole: *whole,
            });
        }
    }

    let mut classes = Vec::new();

    for sharing_a_size in by_size.into_values().filter(|members| members.len() > 1) {
        classes.extend(identical_among(sharing_a_size, |member| {
            File::open(member.path).ok()
        }));
    }
    for class in &mut classes {
        class.sort_by_key(|member| (is_numbered(member.path), member.path));
    }
    classes
}

pub(crate) fn handed_over(transaction: &Transaction<'_>, over: &HandOver<'_>) -> Result<()> {
    history_handed_over(transaction, over.kept, over.taken)?;

    let kept_path = store::path_text(over.kept_path).into_owned();
    let copy_path = store::path_text(over.taken_path).into_owned();
    let changed = |sql: &str, values: &[&dyn ToSql]| changed(transaction, sql, values);

    let renamed = changed(
        "UPDATE playlist_entries SET path = ?1, span_start = 0, span_frames = NULL
          WHERE path = ?2",
        params![kept_path, copy_path],
    )?;

    if renamed > 0 {
        changed(
            "UPDATE playlists SET modified = ?2
              WHERE id IN (SELECT playlist_id FROM playlist_entries WHERE path = ?1)",
            params![kept_path, store::to_nanos(SystemTime::now())],
        )?;
    }
    changed(
        "UPDATE kept_songs SET landed = ?1 WHERE landed = ?2",
        params![kept_path, copy_path],
    )?;
    changed(
        "UPDATE resume_rows SET uri = ?1 WHERE uri = ?2",
        params![
            MediaLocation::local(over.kept_path).to_uri(),
            MediaLocation::local(over.taken_path).to_uri()
        ],
    )?;
    changed(
        "UPDATE songs_received SET path = ?1 WHERE path = ?2",
        params![kept_path, copy_path],
    )?;
    Ok(())
}

pub(crate) fn history_handed_over(
    transaction: &Transaction<'_>,
    kept: TrackId,
    taken: TrackId,
) -> Result<()> {
    let kept = kept.get() as i64;
    let taken = taken.get() as i64;

    changed(
        transaction,
        "UPDATE tracks SET
             plays = plays + (SELECT plays FROM tracks WHERE id = ?2),
             played = max(
                 coalesce(played, (SELECT played FROM tracks WHERE id = ?2)),
                 coalesce((SELECT played FROM tracks WHERE id = ?2), played)
             ),
             favourite = coalesce(favourite, (SELECT favourite FROM tracks WHERE id = ?2))
         WHERE id = ?1",
        params![kept, taken],
    )?;
    changed(
        transaction,
        "UPDATE tracks SET plays = 0 WHERE id = ?1",
        params![taken],
    )?;
    changed(
        transaction,
        "UPDATE listens SET track_id = ?1 WHERE track_id = ?2",
        params![kept, taken],
    )?;
    Ok(())
}

pub(crate) fn unreached_links_hand_over_their_history(
    transaction: &Transaction<'_>,
    scoped: &str,
    generation: i64,
) -> Result<()> {
    let unreached: Vec<(i64, String, i64)> = transaction
        .prepare(&format!(
            "SELECT id, path, span_start FROM tracks
              WHERE seen = -?1 AND root_id IN ({scoped}) AND vault_key IS NULL"
        ))
        .and_then(|mut statement| {
            statement
                .query_map(params![generation], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    for (link, path, span_start) in unreached {
        let Some(named) = file_a_link_names(&store::path_of(&path)) else {
            continue;
        };
        let Some(held) = row_at(transaction, &named, span_start)? else {
            continue;
        };
        if held != link {
            history_handed_over(
                transaction,
                TrackId::new(held as u64)?,
                TrackId::new(link as u64)?,
            )?;
        }
    }
    Ok(())
}

fn file_a_link_names(path: &Path) -> Option<PathBuf> {
    let is_a_link = fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_symlink());
    let names_a_file = fs::metadata(path).is_ok_and(|metadata| metadata.is_file());

    if !(is_a_link && names_a_file) {
        return None;
    }
    fs::canonicalize(path).ok()
}

fn row_at(transaction: &Transaction<'_>, path: &Path, span_start: i64) -> Result<Option<i64>> {
    transaction
        .query_row(
            "SELECT id FROM tracks WHERE path = ?1 AND span_start = ?2",
            params![store::path_text(path), span_start],
            |row| row.get(0),
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn changed(transaction: &Transaction<'_>, sql: &str, values: &[&dyn ToSql]) -> Result<usize> {
    transaction
        .execute(sql, values)
        .map_err(|source| Error::store(StoreOp::Update, source))
}

fn whole_files(connection: &Connection) -> Result<BTreeMap<PathBuf, Whole>> {
    let mut statement = connection
        .prepare(
            "SELECT min(id), path, min(file_size) FROM tracks
              WHERE vault_key IS NULL
              GROUP BY path
             HAVING count(*) = 1 AND min(span_frames) IS NULL",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;

    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    rows.into_iter()
        .map(|(id, path, size)| {
            let id = TrackId::new(id as u64).map_err(Error::from)?;
            Ok((store::path_of(&path), Whole { id, size }))
        })
        .collect()
}

fn is_numbered(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(names_a_numbered_copy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_numbered_by_a_copy_is_told_from_one_that_is_not() {
        assert!(is_numbered(Path::new("/m/Jon Licht - More Love (1).flac")));
        assert!(!is_numbered(Path::new("/m/Track (Live).flac")));
        assert!(!is_numbered(Path::new("/m/(1).flac")));
        assert!(!is_numbered(Path::new("/m/Song.flac")));
        assert!(!is_numbered(Path::new("/m/Song (1000).flac")));
    }
}
