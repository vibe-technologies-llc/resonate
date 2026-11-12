use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::SystemTime,
};

use resonate_core::{MediaLocation, TrackId};
use rusqlite::{Connection, ToSql, Transaction, params};

use crate::{Error, Result, StoreOp, store, take_in};

const NUMBERED_AT_MOST: u32 = 999;

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
        let mut by_digest: BTreeMap<u64, Vec<Member<'_>>> = BTreeMap::new();
        for member in sharing_a_size {
            if let Some(digest) = take_in::content_digest(member.path) {
                by_digest.entry(digest).or_default().push(member);
            }
        }
        for sharing_a_digest in by_digest.into_values().filter(|members| members.len() > 1) {
            classes.extend(
                verified_classes(sharing_a_digest)
                    .into_iter()
                    .filter(|class| class.len() > 1),
            );
        }
    }
    classes
}

fn verified_classes(members: Vec<Member<'_>>) -> Vec<Vec<Member<'_>>> {
    let mut classes: Vec<Vec<Member<'_>>> = Vec::new();
    for member in members {
        match classes
            .iter_mut()
            .find(|class| take_in::holds_the_same_bytes(class[0].path, member.path))
        {
            Some(class) => class.push(member),
            None => classes.push(vec![member]),
        }
    }
    for class in &mut classes {
        class.sort_by_key(|member| (is_numbered(member.path), member.path));
    }
    classes
}

pub(crate) fn handed_over(transaction: &Transaction<'_>, copy: &IdenticalCopy) -> Result<()> {
    let kept = copy.kept_id.get() as i64;
    let taken = copy.copy_id.get() as i64;
    let kept_path = store::path_text(&copy.kept).into_owned();
    let copy_path = store::path_text(&copy.copy).into_owned();
    let changed = |sql: &str, values: &[&dyn ToSql]| {
        transaction
            .execute(sql, values)
            .map_err(|source| Error::store(StoreOp::Update, source))
    };

    changed(
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
    changed("UPDATE tracks SET plays = 0 WHERE id = ?1", params![taken])?;
    changed(
        "UPDATE listens SET track_id = ?1 WHERE track_id = ?2",
        params![kept, taken],
    )?;
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
            MediaLocation::local(&copy.kept).to_uri(),
            MediaLocation::local(&copy.copy).to_uri()
        ],
    )?;
    changed(
        "UPDATE songs_received SET path = ?1 WHERE path = ?2",
        params![kept_path, copy_path],
    )?;
    Ok(())
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
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return false;
    };
    let Some((before, after)) = stem.rsplit_once(" (") else {
        return false;
    };
    let Some(digits) = after.strip_suffix(')') else {
        return false;
    };
    !before.is_empty()
        && !digits.is_empty()
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && digits
            .parse::<u32>()
            .is_ok_and(|nth| nth <= NUMBERED_AT_MOST)
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
