use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};

use ahash::{AHashMap, AHashSet};
use rusqlite::{Connection, Transaction, params};

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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Sound {
    frames: i64,
    codec: i64,
    rate: i64,
    channels: i64,
}

#[derive(Clone, Debug)]
struct Row {
    path: String,
    root: Option<i64>,
    album: Option<i64>,
    likeness: Likeness,
    rate: i64,
    channels: i64,
    cut: (i64, Option<i64>),
    packets: Option<i64>,
    print: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CutLikeness {
    size: i64,
    codec: i64,
    cuts: Vec<(i64, Option<i64>)>,
}

struct Cut {
    path: String,
    rows: Vec<Row>,
}

impl Cut {
    fn likeness(&self) -> CutLikeness {
        CutLikeness {
            size: self.rows[0].likeness.size,
            codec: self.rows[0].likeness.codec,
            cuts: self.rows.iter().map(|row| row.cut).collect(),
        }
    }

    fn named_alike(&self, other: &Self) -> bool {
        let titles = |cut: &Self| {
            cut.rows
                .iter()
                .map(|row| row.likeness.title.clone())
                .collect::<Vec<_>>()
        };
        titles(self) == titles(other)
            || Path::new(&self.path).file_name() == Path::new(&other.path).file_name()
    }
}

impl Row {
    fn sound(&self) -> Option<Sound> {
        Some(Sound {
            frames: self.likeness.duration.filter(|frames| *frames > 0)?,
            codec: self.likeness.codec,
            rate: self.rate,
            channels: self.channels,
        })
    }

    fn named_alike(&self, other: &Self) -> bool {
        let agree = |one: &Option<String>, other: &Option<String>| {
            one.as_deref()
                .zip(other.as_deref())
                .is_some_and(|(one, other)| !one.trim().is_empty() && one == other)
        };

        agree(&self.likeness.title, &other.likeness.title)
            || agree(&self.likeness.artist, &other.likeness.artist)
            || Path::new(&self.path).file_name() == Path::new(&other.path).file_name()
    }
}

struct Paired {
    from: Row,
    to: Row,
    rows: usize,
}

impl Paired {
    const fn whole(from: Row, to: Row) -> Self {
        Self { from, to, rows: 1 }
    }
}

const WHOLE_AND_ALONE: &str = "span_frames IS NULL AND span_start = 0
     AND NOT EXISTS (SELECT 1 FROM tracks o WHERE o.path = t.path AND o.id != t.id)";

const CUT_OR_SHARED: &str = "(span_frames IS NOT NULL OR span_start != 0
     OR EXISTS (SELECT 1 FROM tracks o WHERE o.path = t.path AND o.id != t.id))";

pub(crate) fn to_be_heard(
    connection: &Connection,
    roots: &[i64],
    generation: i64,
) -> Result<Vec<PathBuf>> {
    if roots.is_empty() {
        return Ok(Vec::new());
    }
    let asked = RefCell::new(Vec::new());
    wholes_moved(connection, &scoped(roots), generation, &|path| {
        asked.borrow_mut().push(path.to_path_buf());
        None
    })?;
    Ok(asked.into_inner())
}

fn scoped(roots: &[i64]) -> String {
    roots
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

pub(crate) fn follow_the_moved(
    tx: &Transaction<'_>,
    roots: &[i64],
    generation: i64,
    heard: &dyn Fn(&Path) -> Option<String>,
) -> Result<u64> {
    if roots.is_empty() {
        return Ok(0);
    }
    let scoped = scoped(roots);

    let mut paired = wholes_moved(tx, &scoped, generation, heard)?;
    paired.extend(cuts_moved(tx, &scoped, generation)?);
    if paired.is_empty() {
        return Ok(0);
    }
    follow(tx, &paired, generation)?;
    Ok(paired.iter().map(|pair| pair.rows as u64).sum())
}

fn wholes_moved(
    tx: &Connection,
    scoped: &str,
    generation: i64,
    heard: &dyn Fn(&Path) -> Option<String>,
) -> Result<Vec<Paired>> {
    let gone: Vec<Row> = rows(
        tx,
        &format!("seen != ?1 AND root_id IN ({scoped}) AND {WHOLE_AND_ALONE}"),
        generation,
    )?
    .into_iter()
    .filter(|row| !Path::new(&row.path).exists())
    .collect();
    if gone.is_empty() {
        return Ok(Vec::new());
    }
    let arrived = rows(
        tx,
        &format!("seen = ?1 AND added >= ?1 AND root_id IN ({scoped}) AND {WHOLE_AND_ALONE}"),
        generation,
    )?;

    let (mut paired, gone, arrived) = pairs(gone, arrived);
    paired.extend(pairs_by_sound(gone, arrived, heard));
    Ok(paired)
}

fn cuts_moved(tx: &Connection, scoped: &str, generation: i64) -> Result<Vec<Paired>> {
    let gone: Vec<Cut> = cuts(rows(
        tx,
        &format!("seen != ?1 AND root_id IN ({scoped}) AND {CUT_OR_SHARED}"),
        generation,
    )?)
    .into_iter()
    .filter(|cut| !Path::new(&cut.path).exists())
    .collect();
    if gone.is_empty() {
        return Ok(Vec::new());
    }
    let arrived = cuts(rows(
        tx,
        &format!(
            "seen = ?1 AND added >= ?1 AND root_id IN ({scoped}) AND {CUT_OR_SHARED}
             AND NOT EXISTS (SELECT 1 FROM tracks o WHERE o.path = t.path AND o.added < ?1)"
        ),
        generation,
    )?);

    let mut left: AHashMap<CutLikeness, Vec<Cut>> = AHashMap::new();
    for cut in gone {
        left.entry(cut.likeness()).or_default().push(cut);
    }
    let mut came: AHashMap<CutLikeness, Vec<Cut>> = AHashMap::new();
    for cut in arrived {
        came.entry(cut.likeness()).or_default().push(cut);
    }

    Ok(left
        .into_iter()
        .filter_map(|(likeness, from)| {
            let to = came.remove(&likeness)?;
            let [from] = <[Cut; 1]>::try_from(from).ok()?;
            let [to] = <[Cut; 1]>::try_from(to).ok()?;
            if !from.named_alike(&to) {
                return None;
            }
            let rows = from.rows.len();
            Some(Paired {
                from: from.rows.into_iter().next()?,
                to: to.rows.into_iter().next()?,
                rows,
            })
        })
        .collect())
}

fn cuts(rows: Vec<Row>) -> Vec<Cut> {
    let mut by_path: AHashMap<String, Vec<Row>> = AHashMap::new();
    for row in rows {
        by_path.entry(row.path.clone()).or_default().push(row);
    }
    by_path
        .into_iter()
        .map(|(path, mut rows)| {
            rows.sort_by_key(|row| row.cut);
            Cut { path, rows }
        })
        .collect()
}

fn rows(tx: &Connection, narrowed: &str, generation: i64) -> Result<Vec<Row>> {
    let mut statement = tx
        .prepare(&format!(
            "SELECT t.path, t.root_id, t.album_id, t.file_size, t.duration, t.codec,
                    t.tagged_title, t.tagged_artist, t.sample_rate, t.channels, t.span_start,
                    t.span_frames, s.print, t.packets
               FROM tracks t
               LEFT JOIN track_studies s ON s.track_id = t.id
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
                rate: row.get(8)?,
                channels: row.get(9)?,
                cut: (row.get(10)?, row.get(11)?),
                print: row.get(12)?,
                packets: row.get(13)?,
            })
        })
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn pairs(gone: Vec<Row>, arrived: Vec<Row>) -> (Vec<Paired>, Vec<Row>, Vec<Row>) {
    let mut left: AHashMap<Likeness, Vec<Row>> = AHashMap::new();
    for row in gone {
        left.entry(row.likeness.clone()).or_default().push(row);
    }
    let mut came: AHashMap<Likeness, Vec<Row>> = AHashMap::new();
    for row in arrived {
        came.entry(row.likeness.clone()).or_default().push(row);
    }

    let mut paired = Vec::new();
    let mut unpaired_gone = Vec::new();
    for (likeness, from) in left {
        let Some(to) = came.remove(&likeness) else {
            unpaired_gone.extend(from);
            continue;
        };
        let (found, from_left, to_left) = told_apart(from, to);
        paired.extend(found);
        unpaired_gone.extend(from_left);
        came.insert(likeness, to_left);
    }
    let unpaired_arrived = came.into_values().flatten().collect();

    (paired, unpaired_gone, unpaired_arrived)
}

fn pairs_by_sound(
    gone: Vec<Row>,
    arrived: Vec<Row>,
    heard: &dyn Fn(&Path) -> Option<String>,
) -> Vec<Paired> {
    let mut left: AHashMap<Sound, Vec<Row>> = AHashMap::new();
    for row in gone {
        if let Some(sound) = row.sound() {
            left.entry(sound).or_default().push(row);
        }
    }
    let mut came: AHashMap<Sound, Vec<Row>> = AHashMap::new();
    for row in arrived {
        if let Some(sound) = row.sound() {
            came.entry(sound).or_default().push(row);
        }
    }

    left.into_iter()
        .filter_map(|(sound, from)| {
            let to = came.remove(&sound)?;
            let [from] = <[Row; 1]>::try_from(from).ok()?;
            let [to] = <[Row; 1]>::try_from(to).ok()?;

            (from.named_alike(&to) || sounds_alike(&from, &to, heard))
                .then(|| Paired::whole(from, to))
        })
        .collect()
}

fn sounds_alike(from: &Row, to: &Row, heard: &dyn Fn(&Path) -> Option<String>) -> bool {
    if from.packets.is_some() && to.packets.is_some() {
        return from.packets == to.packets;
    }
    let Some(studied) = from.print.as_deref() else {
        return false;
    };
    heard(Path::new(&to.path)).as_deref() == Some(studied)
}

fn told_apart(from: Vec<Row>, to: Vec<Row>) -> (Vec<Paired>, Vec<Row>, Vec<Row>) {
    if let ([_], [_]) = (from.as_slice(), to.as_slice()) {
        let paired = from
            .into_iter()
            .zip(to)
            .map(|(from, to)| Paired::whole(from, to))
            .collect();
        return (paired, Vec::new(), Vec::new());
    }

    let shared: Vec<Vec<usize>> = from
        .iter()
        .map(|gone| {
            to.iter()
                .map(|came| trailing_names_shared(&gone.path, &came.path))
                .collect()
        })
        .collect();
    let mut taken: Vec<(usize, usize)> = Vec::new();
    for (was, scores) in shared.iter().enumerate() {
        let Some(is) = the_one_best(scores.iter().copied()) else {
            continue;
        };
        if the_one_best(shared.iter().map(|scores| scores[is])) == Some(was) {
            taken.push((was, is));
        }
    }

    let mut from: Vec<Option<Row>> = from.into_iter().map(Some).collect();
    let mut to: Vec<Option<Row>> = to.into_iter().map(Some).collect();
    let paired = taken
        .into_iter()
        .filter_map(|(was, is)| Some(Paired::whole(from[was].take()?, to[is].take()?)))
        .collect();

    (
        paired,
        from.into_iter().flatten().collect(),
        to.into_iter().flatten().collect(),
    )
}

fn the_one_best(scores: impl Iterator<Item = usize>) -> Option<usize> {
    let mut best: Option<(usize, usize)> = None;
    let mut tied = false;
    for (at, score) in scores.enumerate() {
        match best {
            Some((_, held)) if score < held => {}
            Some((_, held)) if score == held => tied = true,
            _ => {
                best = Some((at, score));
                tied = false;
            }
        }
    }
    best.filter(|(_, score)| *score > 0 && !tied)
        .map(|(at, _)| at)
}

fn trailing_names_shared(one: &str, other: &str) -> usize {
    Path::new(one)
        .components()
        .rev()
        .zip(Path::new(other).components().rev())
        .take_while(|(mine, theirs)| mine == theirs)
        .count()
}

fn follow(tx: &Transaction<'_>, paired: &[Paired], generation: i64) -> Result<()> {
    let moves: Vec<Move> = paired
        .iter()
        .map(|pair| Move {
            from: PathBuf::from(&pair.from.path),
            to: PathBuf::from(&pair.to.path),
            rows: u32::try_from(pair.rows).unwrap_or(u32::MAX),
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
