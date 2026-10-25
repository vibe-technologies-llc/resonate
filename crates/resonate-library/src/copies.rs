use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    time::SystemTime,
};

use resonate_core::{MediaLocation, TrackId};
use rusqlite::{Connection, ToSql, Transaction, params};

use crate::{Error, Result, StoreOp, store, take_in};

const NUMBERED_AT_MOST: u32 = 999;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NumberedCopy {
    pub kept: PathBuf,
    pub copy: PathBuf,
    kept_id: TrackId,
    copy_id: TrackId,
}

impl NumberedCopy {
    pub const fn copy_id(&self) -> TrackId {
        self.copy_id
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Copies {
    pub numbered: Vec<NumberedCopy>,
    pub differing: Vec<PathBuf>,
}

#[derive(Clone, Copy, Debug)]
struct Whole {
    id: TrackId,
    size: i64,
}

pub(crate) fn found(connection: &Connection) -> Result<Copies> {
    let wholes = whole_files(connection)?;
    let mut copies = Copies::default();

    for path in wholes.keys() {
        let Some((stem, nth, extension)) = numbered(path) else {
            continue;
        };
        let copy = wholes[path];
        let kept = earlier_names(path, &stem, nth, extension.as_ref())
            .filter_map(|earlier| wholes.get(&earlier).map(|whole| (earlier, *whole)))
            .find(|(earlier, whole)| {
                whole.size == copy.size && take_in::holds_the_same_bytes(earlier, path)
            });
        match kept {
            Some((kept, whole)) => copies.numbered.push(NumberedCopy {
                kept,
                copy: path.clone(),
                kept_id: whole.id,
                copy_id: copy.id,
            }),
            None => copies.differing.push(path.clone()),
        }
    }
    Ok(copies)
}

pub(crate) fn handed_over(transaction: &Transaction<'_>, copy: &NumberedCopy) -> Result<()> {
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

fn numbered(path: &Path) -> Option<(String, u32, Option<OsString>)> {
    let stem = path.file_stem()?.to_str()?;
    let (before, after) = stem.rsplit_once(" (")?;
    let digits = after.strip_suffix(')')?;
    if before.is_empty() || digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let nth = digits.parse().ok().filter(|nth| *nth <= NUMBERED_AT_MOST)?;
    Some((before.to_owned(), nth, path.extension().map(OsString::from)))
}

fn earlier_names<'a>(
    path: &'a Path,
    stem: &'a str,
    nth: u32,
    extension: Option<&'a OsString>,
) -> impl Iterator<Item = PathBuf> + 'a {
    let named = move |stem: String| {
        let mut name = OsString::from(stem);
        if let Some(extension) = extension {
            name.push(".");
            name.push(extension);
        }
        path.with_file_name(name)
    };

    std::iter::once(named(stem.to_owned()))
        .chain((1..nth).map(move |earlier| named(format!("{stem} ({earlier})"))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_numbered_by_a_copy_reads_back_its_stem_and_its_number() {
        assert_eq!(
            numbered(Path::new("/m/Jon Licht - More Love (1).flac")),
            Some(("Jon Licht - More Love".to_owned(), 1, Some("flac".into())))
        );
        assert_eq!(numbered(Path::new("/m/Track (Live).flac")), None);
        assert_eq!(numbered(Path::new("/m/(1).flac")), None);
        assert_eq!(numbered(Path::new("/m/Song.flac")), None);
    }

    #[test]
    fn the_names_a_copy_was_numbered_after_are_the_bare_one_then_every_lower_number() {
        let path = Path::new("/m/Song (3).flac");
        let extension = OsString::from("flac");

        let names: Vec<_> = earlier_names(path, "Song", 3, Some(&extension)).collect();

        assert_eq!(
            names,
            [
                PathBuf::from("/m/Song.flac"),
                PathBuf::from("/m/Song (1).flac"),
                PathBuf::from("/m/Song (2).flac"),
            ]
        );
    }
}
