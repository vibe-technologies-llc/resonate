use std::path::{Path, PathBuf};

use ahash::{AHashMap, AHashSet};
use rusqlite::{Transaction, params};

use crate::{
    enriched,
    error::{Error, Result, StoreOp},
    organise::{self, Move},
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Likeness {
    size: i64,
    duration: Option<i64>,
    codec: i64,
    title: Option<String>,
    artist: Option<String>,
}

#[derive(Clone, Debug)]
struct Row {
    path: String,
    root: Option<i64>,
    album: Option<i64>,
    likeness: Likeness,
}

struct Paired {
    from: Row,
    to: Row,
}

const WHOLE_AND_ALONE: &str = "span_frames IS NULL AND span_start = 0
     AND NOT EXISTS (SELECT 1 FROM tracks o WHERE o.path = t.path AND o.id != t.id)";

pub(crate) fn follow_the_moved(
    tx: &Transaction<'_>,
    roots: &[i64],
    generation: i64,
) -> Result<u64> {
    if roots.is_empty() {
        return Ok(0);
    }
    let scoped = roots
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");

    let gone: Vec<Row> = rows(
        tx,
        &format!("seen != ?1 AND root_id IN ({scoped}) AND {WHOLE_AND_ALONE}"),
        generation,
    )?
    .into_iter()
    .filter(|row| !Path::new(&row.path).exists())
    .collect();
    if gone.is_empty() {
        return Ok(0);
    }
    let arrived = rows(
        tx,
        &format!("seen = ?1 AND added >= ?1 AND root_id IN ({scoped}) AND {WHOLE_AND_ALONE}"),
        generation,
    )?;

    let paired = pairs(gone, arrived);
    if paired.is_empty() {
        return Ok(0);
    }
    follow(tx, &paired, generation)?;
    Ok(paired.len() as u64)
}

fn rows(tx: &Transaction<'_>, narrowed: &str, generation: i64) -> Result<Vec<Row>> {
    let mut statement = tx
        .prepare(&format!(
            "SELECT path, root_id, album_id, file_size, duration, codec, tagged_title,
                    tagged_artist
               FROM tracks t
              WHERE {narrowed}"
        ))
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;

    statement
        .query_map(params![generation], |row| {
            Ok(Row {
                path: row.get(0)?,
                root: row.get(1)?,
                album: row.get(2)?,
                likeness: Likeness {
                    size: row.get(3)?,
                    duration: row.get(4)?,
                    codec: row.get(5)?,
                    title: row.get(6)?,
                    artist: row.get(7)?,
                },
            })
        })
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn pairs(gone: Vec<Row>, arrived: Vec<Row>) -> Vec<Paired> {
    let mut left: AHashMap<Likeness, Vec<Row>> = AHashMap::new();
    for row in gone {
        left.entry(row.likeness.clone()).or_default().push(row);
    }
    let mut came: AHashMap<Likeness, Vec<Row>> = AHashMap::new();
    for row in arrived {
        came.entry(row.likeness.clone()).or_default().push(row);
    }

    let mut paired = Vec::new();
    for (likeness, mut from) in left {
        let Some(to) = came.get_mut(&likeness) else {
            continue;
        };
        if from.len() != 1 || to.len() != 1 {
            continue;
        }
        if let (Some(from), Some(to)) = (from.pop(), to.pop()) {
            paired.push(Paired { from, to });
        }
    }
    paired
}

fn follow(tx: &Transaction<'_>, paired: &[Paired], generation: i64) -> Result<()> {
    let moves: Vec<Move> = paired
        .iter()
        .map(|pair| Move {
            from: PathBuf::from(&pair.from.path),
            to: PathBuf::from(&pair.to.path),
            rows: 1,
            companions: Vec::new(),
            sidecars: Vec::new(),
        })
        .collect();
    organise::files_moved(tx, &moves)?;

    for pair in paired {
        tx.execute(
            "UPDATE tracks SET root_id = ?2, seen = ?3 WHERE path = ?1",
            params![pair.to.path, pair.to.root, generation],
        )
        .map_err(|source| Error::store(StoreOp::Update, source))?;
    }

    let mut into_album: AHashMap<Option<i64>, Vec<&Paired>> = AHashMap::new();
    for pair in paired {
        into_album.entry(pair.to.album).or_default().push(pair);
    }
    for (album, came) in into_album {
        settle_the_album(tx, album, &came)?;
    }
    Ok(())
}

fn settle_the_album(tx: &Transaction<'_>, album: Option<i64>, came: &[&Paired]) -> Result<()> {
    let from: AHashSet<Option<i64>> = came.iter().map(|pair| pair.from.album).collect();
    if let (Some(album), [Some(from)]) = (album, from.into_iter().collect::<Vec<_>>().as_slice())
        && *from != album
        && holds_nothing(tx, album)?
    {
        return enriched::gather(tx, *from, album);
    }

    for pair in came {
        tx.execute(
            "UPDATE tracks SET album_id = ?2 WHERE path = ?1",
            params![pair.to.path, album],
        )
        .map_err(|source| Error::store(StoreOp::Update, source))?;
    }
    Ok(())
}

fn holds_nothing(tx: &Transaction<'_>, album: i64) -> Result<bool> {
    tx.query_row(
        "SELECT NOT EXISTS (SELECT 1 FROM tracks WHERE album_id = ?1)",
        params![album],
        |row| row.get(0),
    )
    .map_err(|source| Error::store(StoreOp::Query, source))
}
