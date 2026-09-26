use std::{
    ffi::OsStr,
    path::Path,
    time::{Duration, SystemTime},
};

use ahash::{AHashMap, AHashSet};
use resonate_core::{MediaLocation, PlaylistId, Span};
use rusqlite::{
    Connection, OptionalExtension as _, Row as SqlRow, Transaction, params, params_from_iter,
    types::Value,
};
use unicode_normalization::UnicodeNormalization as _;

use crate::{
    Clause, Cut, Direction, Error, Exported, Imported, Kept, NamedPlaylist, Playlist,
    PlaylistEntry, PlaylistName, PlaylistOrder, Result, RowOrder, SavedQuery, Search, StoreOp,
    Track, TrackQuery,
    db::{self, BESIDE_A_TRACK, Inner, RawTrack, TRACK_COLUMNS},
    sheet, store,
    undo::{self, Change, Edit},
};

const COLUMNS: &str = "p.id, p.name, p.created, p.modified, p.played, p.plays,
     count(e.path), sum(t.duration * 1.0 / t.sample_rate), q.text, q.sort, q.max_rows,
     p.kept_order, p.kept_reading, p.pinned";

const COUNTED: &str = "FROM playlists p
     LEFT JOIN playlist_entries e ON e.playlist_id = p.id
     LEFT JOIN tracks t ON t.path = e.path AND t.span_start = e.span_start
     LEFT JOIN playlist_queries q ON q.playlist_id = p.id";

const PER_PLAYLIST: &str = "GROUP BY p.id";

const BENEATH_EVERY_POSITION: i64 = -1;

const UNSCANNED_LAST: &str = "tracks.id IS NULL";

const AS_THEY_STAND: &str = "e.position";

const HELD_BY_THE_PLAYLIST: &str = "e.playlist_id = ?";

const ROW: &str = "e";

const ROW_COLUMNS: &str = "e.path, e.span_start, e.span_frames";

const BESIDE_A_ROW: usize = 3;

const ON_THE_SAME_CUT: &str = "tracks.path = e.path AND tracks.span_start = e.span_start";

const HOLDS_THE_WORD: &str = "p.folded LIKE ? ESCAPE '\\'";

const HOLDS_A_LIST: &str = "q.playlist_id IS NULL";

const NO_LIMIT: i64 = -1;

const LIKE_ESCAPE: char = '\\';

const ANY_RUN: char = '%';

pub fn all(
    inner: &Inner,
    order: PlaylistOrder,
    direction: Direction,
    named: Option<&str>,
) -> Result<Vec<Playlist>> {
    let (filters, binds) = narrowed_by(named);
    let sql = format!(
        "SELECT {COLUMNS} {COUNTED}{} {PER_PLAYLIST} ORDER BY {}",
        db::clause(&filters),
        order_by(order, direction)
    );

    let listed: Vec<Playlist> = inner.read(|connection| {
        let mut statement = connection
            .prepare(&sql)
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;
        let found = statement
            .query_map(params_from_iter(binds), read)
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))?;

        found.into_iter().collect()
    })?;

    listed
        .into_iter()
        .map(|found| filled(inner, found))
        .collect()
}

pub fn lists(inner: &Inner, order: PlaylistOrder, direction: Direction) -> Result<Vec<Playlist>> {
    let sql = format!(
        "SELECT {COLUMNS} {COUNTED} WHERE {HOLDS_A_LIST} {PER_PLAYLIST} ORDER BY {}",
        order_by(order, direction)
    );

    inner.read(|connection| {
        let mut statement = connection
            .prepare(&sql)
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;
        let found = statement
            .query_map([], read)
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))?;

        found.into_iter().collect()
    })
}

pub fn names(
    inner: &Inner,
    order: PlaylistOrder,
    direction: Direction,
    from: usize,
    most: Option<usize>,
) -> Result<Vec<NamedPlaylist>> {
    let sql = format!(
        "SELECT p.id, p.name FROM playlists p ORDER BY {} LIMIT ?1 OFFSET ?2",
        order_by(order, direction)
    );
    let taking = most.map_or(NO_LIMIT, |most| i64::try_from(most).unwrap_or(i64::MAX));

    inner.read(|connection| {
        let mut statement = connection
            .prepare(&sql)
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;
        let found = statement
            .query_map(
                params![taking, i64::try_from(from).unwrap_or(i64::MAX)],
                named_row,
            )
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))?;

        found.into_iter().collect()
    })
}

fn named_row(row: &SqlRow<'_>) -> rusqlite::Result<Result<NamedPlaylist>> {
    let id: i64 = row.get(0)?;
    let name: String = row.get(1)?;

    Ok(PlaylistId::new(id as u64)
        .map_err(Error::from)
        .map(|id| NamedPlaylist { id, name }))
}

pub fn count(inner: &Inner, named: Option<&str>) -> Result<u32> {
    let (filters, binds) = narrowed_by(named);
    let sql = format!("SELECT count(*) FROM playlists p{}", db::clause(&filters));

    inner.read(|connection| {
        connection
            .query_row(&sql, params_from_iter(binds), |row| row.get::<_, i64>(0))
            .map(|held| held as u32)
            .map_err(|source| Error::store(StoreOp::Query, source))
    })
}

fn narrowed_by(named: Option<&str>) -> (Vec<String>, Vec<Value>) {
    let words = words_of(named);

    (
        words.iter().map(|_| HOLDS_THE_WORD.to_owned()).collect(),
        words
            .iter()
            .map(|word| Value::Text(anywhere(&folded(word))))
            .collect(),
    )
}

pub(crate) fn words_of(named: Option<&str>) -> Vec<String> {
    let Some(text) = named else {
        return Vec::new();
    };

    Search::read(text)
        .clauses
        .iter()
        .filter_map(Clause::lone_word)
        .filter(|word| word.column.is_none())
        .map(|word| word.text.clone())
        .collect()
}

pub(crate) fn anywhere(word: &str) -> String {
    let mut held = String::with_capacity(word.len() + 2);
    held.push(ANY_RUN);
    for character in word.chars() {
        if matches!(character, ANY_RUN | '_' | LIKE_ESCAPE) {
            held.push(LIKE_ESCAPE);
        }
        held.push(character);
    }
    held.push(ANY_RUN);
    held
}

pub fn one(inner: &Inner, id: PlaylistId) -> Result<Option<Playlist>> {
    let found = inner.read(|connection| {
        connection
            .query_row(
                &format!("SELECT {COLUMNS} {COUNTED} WHERE p.id = ?1 {PER_PLAYLIST}"),
                params![id.get() as i64],
                read,
            )
            .optional()
            .map_err(|source| Error::store(StoreOp::Query, source))?
            .transpose()
    })?;

    found.map(|found| filled(inner, found)).transpose()
}

pub fn named(inner: &Inner, name: &str) -> Result<Option<Playlist>> {
    let found = inner.read(|connection| {
        connection
            .query_row(
                &format!("SELECT {COLUMNS} {COUNTED} WHERE p.folded = ?1 {PER_PLAYLIST}"),
                params![folded(name.trim())],
                read,
            )
            .optional()
            .map_err(|source| Error::store(StoreOp::Query, source))?
            .transpose()
    })?;

    found.map(|found| filled(inner, found)).transpose()
}

pub fn create(inner: &Inner, name: &str) -> Result<PlaylistId> {
    start(inner, name, &[])
}

pub fn start(inner: &Inner, name: &str, cuts: &[Cut]) -> Result<PlaylistId> {
    let name = wanted_name(name)?;
    let wanted = local_rows(cuts)?;

    let id = undo::started(inner, Edit::Started, &name, |transaction| {
        let id = created(transaction, &name)?;
        append(transaction, id, &wanted)?;
        Ok((id, id))
    })?;

    inner.playlists_changed();
    Ok(id)
}

fn created(transaction: &Transaction<'_>, name: &PlaylistName) -> Result<PlaylistId> {
    refuse_duplicate(transaction, name, None)?;
    transaction
        .execute(
            "INSERT INTO playlists (name, folded, created, modified) VALUES (?1, ?2, ?3, ?3)",
            params![
                name.as_str(),
                folded(name.as_str()),
                store::to_nanos(SystemTime::now())
            ],
        )
        .map_err(|source| Error::store(StoreOp::Insert, source))?;

    Ok(PlaylistId::new(transaction.last_insert_rowid() as u64)?)
}

pub fn rename(inner: &Inner, id: PlaylistId, name: &str) -> Result<()> {
    let name = wanted_name(name)?;

    undo::edited(inner, id, Edit::Renamed, |transaction| {
        refuse_duplicate(transaction, &name, Some(id))?;
        renamed(transaction, id, &name)?;
        Ok(Change::Made(()))
    })?;

    inner.playlists_changed();
    Ok(())
}

fn renamed(transaction: &Transaction<'_>, id: PlaylistId, name: &PlaylistName) -> Result<()> {
    let known = transaction
        .execute(
            "UPDATE playlists SET name = ?2, folded = ?3, modified = ?4 WHERE id = ?1",
            params![
                id.get() as i64,
                name.as_str(),
                folded(name.as_str()),
                store::to_nanos(SystemTime::now())
            ],
        )
        .map_err(|source| Error::store(StoreOp::Update, source))?;

    if known == 0 {
        return Err(Error::UnknownPlaylist(id));
    }
    Ok(())
}

pub fn remove(inner: &Inner, id: PlaylistId) -> Result<bool> {
    let dropped = undo::edited(inner, id, Edit::Discarded, |transaction| {
        let dropped = transaction
            .execute(
                "DELETE FROM playlists WHERE id = ?1",
                params![id.get() as i64],
            )
            .map_err(|source| Error::store(StoreOp::Delete, source))?;

        Ok(if dropped == 0 {
            Change::Nothing(false)
        } else {
            Change::Made(true)
        })
    })?;

    if !dropped {
        return Ok(false);
    }
    if inner
        .playing()
        .is_some_and(|playing| playing.playlist == id)
    {
        inner.set_playing(None);
    }
    inner.playlists_changed();
    Ok(true)
}

pub fn played_now(inner: &Inner, id: PlaylistId) -> Result<()> {
    let played = inner.write(|transaction| {
        transaction
            .execute(
                "UPDATE playlists SET played = ?2, plays = plays + 1 WHERE id = ?1",
                params![id.get() as i64, store::to_nanos(SystemTime::now())],
            )
            .map_err(|source| Error::store(StoreOp::Update, source))
    })?;

    if played == 0 {
        return Err(Error::UnknownPlaylist(id));
    }
    inner.playlists_changed();
    Ok(())
}

pub fn pin(inner: &Inner, id: PlaylistId, pinned: bool) -> Result<bool> {
    let at = pinned.then(|| store::to_nanos(SystemTime::now()));
    let changed = inner.write(|transaction| {
        transaction
            .execute(
                "UPDATE playlists SET pinned = ?2 WHERE id = ?1",
                params![id.get() as i64, at],
            )
            .map_err(|source| Error::store(StoreOp::Update, source))
    })?;

    if changed == 0 {
        return Ok(false);
    }
    inner.playlists_changed();
    Ok(true)
}

pub fn save_query(inner: &Inner, name: &str, query: &SavedQuery) -> Result<PlaylistId> {
    let name = wanted_name(name)?;

    let id = undo::started(inner, Edit::Started, &name, |transaction| {
        let id = created(transaction, &name)?;
        write_query(transaction, id, query)?;
        Ok((id, id))
    })?;

    inner.playlists_changed();
    Ok(id)
}

pub(crate) fn write_query(
    transaction: &Transaction<'_>,
    id: PlaylistId,
    query: &SavedQuery,
) -> Result<()> {
    transaction
        .execute(
            "INSERT INTO playlist_queries (playlist_id, text, sort, max_rows)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                id.get() as i64,
                query.text.as_deref(),
                store::sort_code(query.sort, query.reading),
                query.limit.map(|limit| limit as i64)
            ],
        )
        .map_err(|source| Error::store(StoreOp::Insert, source))?;

    Ok(())
}

pub fn revise(inner: &Inner, id: PlaylistId, name: &str, query: &SavedQuery) -> Result<()> {
    let name = wanted_name(name)?;

    undo::edited(inner, id, Edit::Revised, |transaction| {
        let revised = transaction
            .execute(
                "UPDATE playlist_queries SET text = ?2, sort = ?3, max_rows = ?4
                 WHERE playlist_id = ?1",
                params![
                    id.get() as i64,
                    query.text.as_deref(),
                    store::sort_code(query.sort, query.reading),
                    query.limit.map(|limit| limit as i64)
                ],
            )
            .map_err(|source| Error::store(StoreOp::Update, source))?;

        if revised == 0 {
            return Err(if known(transaction, id)? {
                Error::NotAQuery { playlist: id }
            } else {
                Error::UnknownPlaylist(id)
            });
        }
        refuse_duplicate(transaction, &name, Some(id))?;
        renamed(transaction, id, &name)?;
        Ok(Change::Made(()))
    })?;

    inner.playlists_changed();
    Ok(())
}

pub fn entries(
    inner: &Inner,
    id: PlaylistId,
    matching: Option<&str>,
) -> Result<Vec<PlaylistEntry>> {
    if let Some(query) = inner.read(|connection| asked_in(connection, id))? {
        return Ok(db::tracks(inner, &TrackQuery::from(&query), matching)?
            .into_iter()
            .zip(0..)
            .map(|(track, position)| listed(position, track))
            .collect());
    }

    let mut filters = vec![HELD_BY_THE_PLAYLIST.to_owned()];
    let mut binds = vec![Value::Integer(id.get() as i64)];
    if let Some(text) = matching {
        let Some(narrowed) = db::cuts_matching(text, ROW) else {
            return Ok(Vec::new());
        };
        filters.push(narrowed.sql);
        binds.extend(narrowed.binds);
    }

    let sql = format!(
        "SELECT {TRACK_COLUMNS}, {ROW_COLUMNS}, e.position FROM playlist_entries e
         LEFT JOIN tracks ON {ON_THE_SAME_CUT}{} ORDER BY e.position",
        db::clause(&filters)
    );

    let raw = inner.read(|connection| {
        let mut statement = connection
            .prepare(&sql)
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;
        statement
            .query_map(params_from_iter(binds), |row| {
                Ok((
                    RawTrack::read_where_joined(row)?,
                    Row::read_at(row, BESIDE_A_TRACK)?,
                    row.get::<_, i64>(BESIDE_A_TRACK + BESIDE_A_ROW)?,
                ))
            })
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))
    })?;

    raw.into_iter()
        .map(|(track, row, position)| {
            Ok(PlaylistEntry {
                position: position as usize,
                cut: row.into_cut(),
                track: track.map(RawTrack::into_track).transpose()?,
            })
        })
        .collect()
}

pub fn cuts(inner: &Inner, id: PlaylistId) -> Result<Vec<Cut>> {
    if inner.read(|connection| asked_in(connection, id))?.is_some() {
        return Ok(entries(inner, id, None)?
            .into_iter()
            .map(|entry| entry.cut)
            .collect());
    }

    inner.read(|connection| {
        Ok(rows(connection, id)?
            .into_iter()
            .map(Row::into_cut)
            .collect())
    })
}

pub fn add(inner: &Inner, id: PlaylistId, cuts: &[Cut]) -> Result<usize> {
    add_as(inner, id, Edit::Added, cuts)
}

fn add_as(inner: &Inner, id: PlaylistId, edit: Edit, cuts: &[Cut]) -> Result<usize> {
    let wanted = local_rows(cuts)?;
    let added = undo::edited(inner, id, edit, |transaction| {
        only_a_list(transaction, id)?;
        if wanted.is_empty() {
            return Ok(Change::Nothing(0));
        }
        Ok(Change::Made(append(transaction, id, &wanted)?))
    })?;

    if added > 0 {
        inner.playlists_changed();
    }
    Ok(added)
}

fn append(transaction: &Transaction<'_>, id: PlaylistId, wanted: &[Row]) -> Result<usize> {
    for (position, row) in (tail(transaction, id)?..).zip(wanted) {
        insert(transaction, id, position, row)?;
    }
    if let Some(kept) = kept_in(transaction, id)? {
        in_order(transaction, id, kept)?;
    }
    touch(transaction, id)?;
    Ok(wanted.len())
}

fn local_rows(cuts: &[Cut]) -> Result<Vec<Row>> {
    let mut wanted = Vec::with_capacity(cuts.len());
    for cut in cuts {
        wanted.push(Row::of(cut)?);
    }
    Ok(wanted)
}

pub fn copy(
    inner: &Inner,
    from: PlaylistId,
    into: PlaylistId,
    matching: Option<&str>,
) -> Result<usize> {
    if from == into {
        return Err(Error::IntoItself { playlist: from });
    }
    refuse_an_unknown(inner, from)?;

    let taken = match matching {
        None => cuts(inner, from)?,
        Some(_) => entries(inner, from, matching)?
            .into_iter()
            .map(|entry| entry.cut)
            .collect(),
    };

    add_as(inner, into, Edit::Copied, &taken)
}

pub fn remove_rows(inner: &Inner, id: PlaylistId, rows: Span) -> Result<bool> {
    let changed = undo::edited(inner, id, Edit::Removed, |transaction| {
        only_a_list(transaction, id)?;
        let held = length(transaction, id)?;
        let Some(rows) = up_to_the_end(rows, held) else {
            return Ok(Change::Nothing(false));
        };

        let dropped = transaction
            .execute(
                "DELETE FROM playlist_entries
                 WHERE playlist_id = ?1 AND position BETWEEN ?2 AND ?3",
                params![id.get() as i64, rows.first() as i64, rows.last() as i64],
            )
            .map_err(|source| Error::store(StoreOp::Delete, source))?;

        if dropped == 0 {
            return Ok(Change::Nothing(false));
        }
        closed_up(transaction, id, rows.last() as i64 + 1, dropped as i64)?;
        touch(transaction, id)?;
        Ok(Change::Made(true))
    })?;

    if changed {
        inner.playlists_changed();
    }
    Ok(changed)
}

pub fn move_rows(inner: &Inner, id: PlaylistId, rows: Span, to: usize) -> Result<bool> {
    let changed = undo::edited(inner, id, Edit::Moved, |transaction| {
        only_a_list(transaction, id)?;
        refuse_a_kept_order(transaction, id)?;
        let held = length(transaction, id)?;
        if rows.holds(to) || rows.last() >= held || to >= held {
            return Ok(Change::Nothing(false));
        }

        reseated(transaction, id, rows, to as i64)?;
        touch(transaction, id)?;
        Ok(Change::Made(true))
    })?;

    if changed {
        inner.playlists_changed();
    }
    Ok(changed)
}

pub fn sort_rows(
    inner: &Inner,
    id: PlaylistId,
    order: RowOrder,
    direction: Direction,
) -> Result<usize> {
    let moved = undo::edited(inner, id, Edit::Ordered, |transaction| {
        only_a_list(transaction, id)?;
        refuse_a_kept_order(transaction, id)?;
        let moved = in_order(
            transaction,
            id,
            Kept {
                order,
                reading: direction,
            },
        )?;
        if moved == 0 {
            return Ok(Change::Nothing(0));
        }
        touch(transaction, id)?;
        Ok(Change::Made(moved))
    })?;

    if moved > 0 {
        inner.playlists_changed();
    }
    Ok(moved)
}

pub fn keep(inner: &Inner, id: PlaylistId, kept: Option<Kept>) -> Result<usize> {
    let moved = undo::edited(inner, id, Edit::Kept, |transaction| {
        only_a_list(transaction, id)?;
        let standing = kept_in(transaction, id)?;
        let moved = match kept {
            Some(kept) => in_order(transaction, id, kept)?,
            None => 0,
        };
        if standing == kept && moved == 0 {
            return Ok(Change::Nothing(None));
        }

        transaction
            .execute(
                "UPDATE playlists SET kept_order = ?2, kept_reading = ?3 WHERE id = ?1",
                params![
                    id.get() as i64,
                    kept.map(|kept| store::row_order_code(kept.order)),
                    kept.map(|kept| store::direction_code(kept.reading))
                ],
            )
            .map_err(|source| Error::store(StoreOp::Update, source))?;
        touch(transaction, id)?;
        Ok(Change::Made(Some(moved)))
    })?;

    if moved.is_some() {
        inner.playlists_changed();
    }
    Ok(moved.unwrap_or_default())
}

fn in_order(transaction: &Transaction<'_>, id: PlaylistId, kept: Kept) -> Result<usize> {
    let sql = format!(
        "SELECT {ROW_COLUMNS} FROM playlist_entries e
         LEFT JOIN tracks ON {ON_THE_SAME_CUT}
         WHERE e.playlist_id = ?1 ORDER BY {}",
        rows_in_order(kept.order, kept.reading)
    );
    let held = rows(transaction, id)?;
    let wanted = rows_by(transaction, &sql, id)?;
    let Some(first) = out_of_place(&held, &wanted) else {
        return Ok(0);
    };
    let from = first as i64;

    transaction
        .execute(
            "DELETE FROM playlist_entries WHERE playlist_id = ?1 AND position >= ?2",
            params![id.get() as i64, from],
        )
        .map_err(|source| Error::store(StoreOp::Delete, source))?;
    for (position, row) in (from..).zip(&wanted[first..]) {
        insert(transaction, id, position, row)?;
    }

    Ok(moved_from(&held[first..], &wanted[first..]))
}

fn out_of_place(held: &[Row], wanted: &[Row]) -> Option<usize> {
    held.iter()
        .zip(wanted)
        .position(|(held, wanted)| held != wanted)
}

fn moved_from(held: &[Row], wanted: &[Row]) -> usize {
    held.iter()
        .zip(wanted)
        .filter(|(held, wanted)| held != wanted)
        .count()
}

pub fn prune(inner: &Inner, id: PlaylistId) -> Result<usize> {
    dropped_where(inner, id, Edit::Tidied, Going::Gone)
}

pub fn tidy(inner: &Inner, id: PlaylistId) -> Result<usize> {
    dropped_where(inner, id, Edit::Tidied, Going::Unwanted)
}

pub fn fold_doubles(inner: &Inner, id: PlaylistId) -> Result<usize> {
    dropped_where(inner, id, Edit::Folded, Going::Doubled)
}

pub fn remove_matching(inner: &Inner, id: PlaylistId, matching: &str) -> Result<usize> {
    dropped_where(inner, id, Edit::Dropped, Going::Matching(matching))
}

enum Going<'a> {
    Gone,
    Doubled,
    Unwanted,
    Matching(&'a str),
}

fn asked_of(
    transaction: &Transaction<'_>,
    id: PlaylistId,
    held: &[(i64, Row)],
    question: Going<'_>,
) -> Result<Vec<bool>> {
    Ok(match question {
        Going::Gone => held.iter().map(|(_, row)| has_gone(&row.path)).collect(),
        Going::Doubled => {
            let mut seen = AHashSet::with_capacity(held.len());
            held.iter().map(|(_, row)| !seen.insert(row)).collect()
        }
        Going::Unwanted => {
            let mut seen = AHashSet::with_capacity(held.len());
            held.iter()
                .map(|(_, row)| has_gone(&row.path) || !seen.insert(row))
                .collect()
        }
        Going::Matching(text) => {
            let matched = matched_in(transaction, id, text)?;
            held.iter()
                .map(|(position, _)| matched.contains(position))
                .collect()
        }
    })
}

fn matched_in(transaction: &Transaction<'_>, id: PlaylistId, text: &str) -> Result<AHashSet<i64>> {
    let Some(narrowed) = db::cuts_matching(text, ROW) else {
        return Ok(AHashSet::new());
    };
    let sql = format!(
        "SELECT e.position FROM playlist_entries e{}",
        db::clause(&[HELD_BY_THE_PLAYLIST.to_owned(), narrowed.sql])
    );
    let mut binds = vec![Value::Integer(id.get() as i64)];
    binds.extend(narrowed.binds);

    let mut statement = transaction
        .prepare(&sql)
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    statement
        .query_map(params_from_iter(binds), |row| row.get::<_, i64>(0))
        .and_then(|rows| rows.collect::<rusqlite::Result<AHashSet<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn dropped_where(inner: &Inner, id: PlaylistId, edit: Edit, question: Going<'_>) -> Result<usize> {
    let dropped = undo::edited(inner, id, edit, |transaction| {
        only_a_list(transaction, id)?;
        let held = numbered(transaction, id)?;
        let going = asked_of(transaction, id, &held, question)?;
        let Some(first) = going.iter().position(|going| *going) else {
            return Ok(Change::Nothing(0));
        };

        let from = held[first].0;
        transaction
            .execute(
                "DELETE FROM playlist_entries WHERE playlist_id = ?1 AND position >= ?2",
                params![id.get() as i64, from],
            )
            .map_err(|source| Error::store(StoreOp::Delete, source))?;

        let tail = &held[first..];
        let kept: Vec<&Row> = tail
            .iter()
            .zip(&going[first..])
            .filter(|(_, going)| !**going)
            .map(|((_, row), _)| row)
            .collect();
        for (position, row) in (from..).zip(&kept) {
            insert(transaction, id, position, row)?;
        }
        touch(transaction, id)?;

        Ok(Change::Made(tail.len() - kept.len()))
    })?;

    if dropped > 0 {
        inner.playlists_changed();
    }
    Ok(dropped)
}

pub fn import(inner: &Inner, path: &Path, name: Option<&str>) -> Result<Imported> {
    let reading = sheet::read(path)?;
    let short = reading.sheet.short();
    let wanted = name
        .map(str::to_owned)
        .or(reading.sheet.declared)
        .or_else(|| stem_of(path))
        .ok_or(Error::UnnamedPlaylist)?;

    let found = named(inner, &wanted)?;
    let mut held: AHashMap<Row, usize> = AHashMap::new();
    if let Some(found) = found.as_ref() {
        for row in inner.read(|connection| rows(connection, found.id))? {
            *held.entry(row).or_default() += 1;
        }
    }
    let mut fresh = Vec::with_capacity(reading.sheet.locations.len());
    let mut already = 0;

    for location in reading.sheet.locations {
        let named = Row::whole(local_path(&location)?.to_owned());
        match held.get_mut(&named) {
            Some(times) if *times > 0 => {
                *times -= 1;
                already += 1;
            }
            _ => fresh.push(Cut::whole(location)),
        }
    }
    let missing = fresh
        .iter()
        .filter(|cut| !is_on_disk(&cut.location))
        .count();

    let (id, name, added) = match found {
        Some(found) => (
            found.id,
            found.name,
            add_as(inner, found.id, Edit::Imported, &fresh)?,
        ),
        None => started_from(inner, &wanted, &fresh)?,
    };

    Ok(Imported {
        id,
        name,
        added,
        already,
        elsewhere: reading.sheet.elsewhere,
        missing,
        short,
        format: reading.format,
        encoding: reading.encoding,
    })
}

pub fn export(inner: &Inner, id: PlaylistId, path: &Path) -> Result<Exported> {
    let found = one(inner, id)?.ok_or(Error::UnknownPlaylist(id))?;
    let entries = entries(inner, id, None)?;

    Ok(Exported {
        rows: entries.len(),
        format: sheet::write(path, &found.name, &entries)?,
    })
}

fn started_from(inner: &Inner, wanted: &str, fresh: &[Cut]) -> Result<(PlaylistId, String, usize)> {
    let name = wanted_name(wanted)?;
    let wanted_rows = local_rows(fresh)?;

    let (id, added) = undo::started(inner, Edit::Imported, &name, |transaction| {
        let id = created(transaction, &name)?;
        let added = append(transaction, id, &wanted_rows)?;
        Ok((id, (id, added)))
    })?;

    inner.playlists_changed();
    Ok((id, name.as_str().to_owned(), added))
}

fn filled(inner: &Inner, mut found: Playlist) -> Result<Playlist> {
    let Some(query) = found.query.as_ref() else {
        return Ok(found);
    };

    let measured = db::measured(inner, &TrackQuery::from(query))?;
    let (entries, duration) = (measured.rows, measured.length);
    found.entries = entries;
    found.duration = duration;
    Ok(found)
}

fn listed(position: usize, track: Track) -> PlaylistEntry {
    PlaylistEntry {
        position,
        cut: Cut::of(&track),
        track: Some(track),
    }
}

pub(crate) fn asked_in(connection: &Connection, id: PlaylistId) -> Result<Option<SavedQuery>> {
    connection
        .query_row(
            "SELECT text, sort, max_rows FROM playlist_queries WHERE playlist_id = ?1",
            params![id.get() as i64],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?
        .map(|(text, sort, max_rows)| wanted_query(id, text, sort, max_rows))
        .transpose()
}

fn wanted_query(
    id: PlaylistId,
    text: Option<String>,
    sort: i64,
    max_rows: Option<i64>,
) -> Result<SavedQuery> {
    let (sort, reading) = store::sort_of(id, sort)?;

    Ok(SavedQuery {
        text,
        sort,
        reading,
        limit: max_rows.map(|rows| rows as usize),
    })
}

fn known(connection: &Connection, id: PlaylistId) -> Result<bool> {
    connection
        .query_row(
            "SELECT count(*) FROM playlists WHERE id = ?1",
            params![id.get() as i64],
            |row| row.get::<_, i64>(0),
        )
        .map(|held| held > 0)
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn refuse_an_unknown(inner: &Inner, id: PlaylistId) -> Result<()> {
    if inner.read(|connection| known(connection, id))? {
        return Ok(());
    }
    Err(Error::UnknownPlaylist(id))
}

fn only_a_list(transaction: &Transaction<'_>, id: PlaylistId) -> Result<()> {
    if !known(transaction, id)? {
        return Err(Error::UnknownPlaylist(id));
    }
    match asked_in(transaction, id)? {
        Some(_) => Err(Error::NotAList { playlist: id }),
        None => Ok(()),
    }
}

fn refuse_a_kept_order(transaction: &Transaction<'_>, id: PlaylistId) -> Result<()> {
    match kept_in(transaction, id)? {
        Some(_) => Err(Error::KeptInOrder { playlist: id }),
        None => Ok(()),
    }
}

fn kept_in(transaction: &Transaction<'_>, id: PlaylistId) -> Result<Option<Kept>> {
    let held = transaction
        .query_row(
            "SELECT kept_order, kept_reading FROM playlists WHERE id = ?1",
            params![id.get() as i64],
            |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?)),
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    let Some((Some(order), Some(reading))) = held else {
        return Ok(None);
    };
    wanted_kept(id, order, reading).map(Some)
}

pub(crate) fn wanted_kept(id: PlaylistId, order: i64, reading: i64) -> Result<Kept> {
    Ok(Kept {
        order: store::row_order_of(id, order)?,
        reading: store::direction_of(id, reading)?,
    })
}

fn is_on_disk(location: &MediaLocation) -> bool {
    location.as_path().is_some_and(Path::is_file)
}

fn has_gone(path: &str) -> bool {
    !Path::new(path).is_file()
}

fn stem_of(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(OsStr::to_str)
        .map(ToOwned::to_owned)
}

fn closed_up(transaction: &Transaction<'_>, id: PlaylistId, from: i64, by: i64) -> Result<()> {
    parked(transaction, id, from, i64::MAX)?;
    transaction
        .execute(
            "UPDATE playlist_entries SET position = ?2 - position - ?3
             WHERE playlist_id = ?1 AND position < 0",
            params![id.get() as i64, BENEATH_EVERY_POSITION, by],
        )
        .map_err(|source| Error::store(StoreOp::Update, source))?;

    Ok(())
}

fn reseated(transaction: &Transaction<'_>, id: PlaylistId, rows: Span, to: i64) -> Result<()> {
    let (first, last) = (rows.first() as i64, rows.last() as i64);
    let held = rows.rows() as i64;
    let (moved, crossed) = if to > last {
        (to - last, -held)
    } else {
        (to - first, held)
    };

    parked(transaction, id, first.min(to), last.max(to))?;
    transaction
        .execute(
            "UPDATE playlist_entries
                SET position = CASE
                    WHEN ?2 - position BETWEEN ?3 AND ?4 THEN ?2 - position + ?5
                    ELSE ?2 - position + ?6
                END
              WHERE playlist_id = ?1 AND position < 0",
            params![
                id.get() as i64,
                BENEATH_EVERY_POSITION,
                first,
                last,
                moved,
                crossed
            ],
        )
        .map_err(|source| Error::store(StoreOp::Update, source))?;

    Ok(())
}

fn parked(transaction: &Transaction<'_>, id: PlaylistId, from: i64, to: i64) -> Result<()> {
    transaction
        .execute(
            "UPDATE playlist_entries SET position = ?4 - position
             WHERE playlist_id = ?1 AND position BETWEEN ?2 AND ?3",
            params![id.get() as i64, from, to, BENEATH_EVERY_POSITION],
        )
        .map_err(|source| Error::store(StoreOp::Update, source))?;

    Ok(())
}

pub(crate) fn insert(
    transaction: &Transaction<'_>,
    id: PlaylistId,
    position: i64,
    row: &Row,
) -> Result<()> {
    transaction
        .execute(
            "INSERT INTO playlist_entries (playlist_id, position, path, span_start, span_frames)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id.get() as i64, position, row.path, row.start, row.frames],
        )
        .map_err(|source| Error::store(StoreOp::Insert, source))?;

    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Row {
    pub(crate) path: String,
    start: i64,
    frames: Option<i64>,
}

impl Row {
    pub(crate) fn of(cut: &Cut) -> Result<Self> {
        let (start, frames) = store::span_columns(cut.span);

        Ok(Self {
            path: local_path(&cut.location)?.to_owned(),
            start,
            frames,
        })
    }

    pub(crate) fn whole(path: String) -> Self {
        Self {
            path,
            start: 0,
            frames: None,
        }
    }

    fn read_at(row: &SqlRow<'_>, first: usize) -> rusqlite::Result<Self> {
        Ok(Self {
            path: row.get(first)?,
            start: row.get(first + 1)?,
            frames: row.get(first + 2)?,
        })
    }

    pub(crate) fn into_cut(self) -> Cut {
        Cut {
            location: MediaLocation::local(self.path),
            span: store::span(self.start, self.frames),
        }
    }
}

pub(crate) fn rows(connection: &Connection, id: PlaylistId) -> Result<Vec<Row>> {
    rows_by(
        connection,
        &format!(
            "SELECT {ROW_COLUMNS} FROM playlist_entries e
             WHERE e.playlist_id = ?1 ORDER BY e.position"
        ),
        id,
    )
}

fn rows_by(connection: &Connection, sql: &str, id: PlaylistId) -> Result<Vec<Row>> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;

    statement
        .query_map(params![id.get() as i64], |row| Row::read_at(row, 0))
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn numbered(connection: &Connection, id: PlaylistId) -> Result<Vec<(i64, Row)>> {
    let mut statement = connection
        .prepare(&format!(
            "SELECT e.position, {ROW_COLUMNS} FROM playlist_entries e
             WHERE e.playlist_id = ?1 ORDER BY e.position"
        ))
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;

    statement
        .query_map(params![id.get() as i64], |row| {
            Ok((row.get::<_, i64>(0)?, Row::read_at(row, 1)?))
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn length(transaction: &Transaction<'_>, id: PlaylistId) -> Result<usize> {
    transaction
        .query_row(
            "SELECT count(*) FROM playlist_entries WHERE playlist_id = ?1",
            params![id.get() as i64],
            |row| row.get::<_, i64>(0),
        )
        .map(|held| held as usize)
        .map_err(|source| Error::store(StoreOp::Query, source))
}

const fn up_to_the_end(rows: Span, held: usize) -> Option<Span> {
    let Some(last) = held.checked_sub(1) else {
        return None;
    };
    if rows.first() > last {
        return None;
    }

    Some(Span::between(
        rows.first(),
        if rows.last() < last {
            rows.last()
        } else {
            last
        },
    ))
}

fn tail(transaction: &Transaction<'_>, id: PlaylistId) -> Result<i64> {
    transaction
        .query_row(
            "SELECT coalesce(max(position), -1) + 1 FROM playlist_entries WHERE playlist_id = ?1",
            params![id.get() as i64],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn touch(transaction: &Transaction<'_>, id: PlaylistId) -> Result<()> {
    let touched = transaction
        .execute(
            "UPDATE playlists SET modified = ?2 WHERE id = ?1",
            params![id.get() as i64, store::to_nanos(SystemTime::now())],
        )
        .map_err(|source| Error::store(StoreOp::Update, source))?;

    if touched == 0 {
        return Err(Error::UnknownPlaylist(id));
    }
    Ok(())
}

pub(crate) fn local_path(location: &MediaLocation) -> Result<&str> {
    let path = location.as_path().ok_or_else(|| Error::NotALocalFile {
        location: Box::new(location.clone()),
    })?;
    store::path_text(path)
}

fn wanted_name(name: &str) -> Result<PlaylistName> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(Error::UnnamedPlaylist);
    }
    Ok(PlaylistName::new(trimmed))
}

pub(crate) fn folded(name: &str) -> String {
    name.to_lowercase().nfc().collect()
}

pub(crate) fn refuse_duplicate(
    transaction: &Transaction<'_>,
    name: &PlaylistName,
    keeping: Option<PlaylistId>,
) -> Result<()> {
    let held = transaction
        .query_row(
            "SELECT id FROM playlists WHERE folded = ?1",
            params![folded(name.as_str())],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    match held.map(|held| PlaylistId::new(held as u64)) {
        None => Ok(()),
        Some(Ok(held)) if Some(held) == keeping => Ok(()),
        Some(_) => Err(Error::DuplicatePlaylist { name: name.clone() }),
    }
}

fn rows_in_order(order: RowOrder, direction: Direction) -> String {
    let sense = sense(direction);
    let mut reading: Vec<String> = Vec::new();

    if order.reads_the_catalog() {
        reading.push(UNSCANNED_LAST.to_owned());
    }
    reading.extend(
        sorted_by(order)
            .iter()
            .map(|column| format!("{column} {sense}")),
    );
    reading.push(AS_THEY_STAND.to_owned());

    reading.join(", ")
}

const fn sorted_by(order: RowOrder) -> &'static [&'static str] {
    match order {
        RowOrder::Album => &[
            "tracks.album_id",
            "tracks.disc_number",
            "tracks.track_number",
            "tracks.title COLLATE NOCASE",
        ],
        RowOrder::Artist => &[
            "tracks.artist COLLATE NOCASE",
            "tracks.album_id",
            "tracks.disc_number",
            "tracks.track_number",
        ],
        RowOrder::Title => &["tracks.title COLLATE NOCASE"],
        RowOrder::Length => &["tracks.duration * 1.0 / tracks.sample_rate"],
        RowOrder::File => &["e.path COLLATE NOCASE"],
    }
}

const PINNED_FIRST: &str = "CASE WHEN p.pinned IS NULL THEN 1 ELSE 0 END, p.pinned DESC";

fn order_by(order: PlaylistOrder, direction: Direction) -> String {
    let column = match order {
        PlaylistOrder::Name => "p.folded",
        PlaylistOrder::Created => "p.created",
        PlaylistOrder::Modified => "p.modified",
        PlaylistOrder::Played => "p.played",
        PlaylistOrder::Plays => "p.plays",
    };

    format!(
        "{PINNED_FIRST}, {column} {sense}, p.id {sense}",
        sense = sense(direction)
    )
}

const fn sense(direction: Direction) -> &'static str {
    match direction {
        Direction::Ascending => "ASC",
        Direction::Descending => "DESC",
    }
}

fn read(row: &SqlRow<'_>) -> rusqlite::Result<Result<Playlist>> {
    let id: i64 = row.get(0)?;
    let name: String = row.get(1)?;
    let created: i64 = row.get(2)?;
    let modified: i64 = row.get(3)?;
    let played: Option<i64> = row.get(4)?;
    let plays: u32 = row.get(5)?;
    let entries: u32 = row.get(6)?;
    let seconds: Option<f64> = row.get(7)?;
    let text: Option<String> = row.get(8)?;
    let sort: Option<i64> = row.get(9)?;
    let max_rows: Option<i64> = row.get(10)?;
    let kept_order: Option<i64> = row.get(11)?;
    let kept_reading: Option<i64> = row.get(12)?;
    let pinned: Option<i64> = row.get(13)?;

    Ok(PlaylistId::new(id as u64)
        .map_err(Error::from)
        .and_then(|id| -> Result<Playlist> {
            Ok(Playlist {
                id,
                name,
                entries,
                duration: seconds
                    .filter(|seconds| *seconds > 0.0)
                    .map(|seconds| Duration::try_from_secs_f64(seconds).unwrap_or_default()),
                created: store::from_nanos(created),
                modified: store::from_nanos(modified),
                played: played.map(store::from_nanos),
                plays,
                query: sort
                    .map(|sort| wanted_query(id, text, sort, max_rows))
                    .transpose()?,
                kept: kept_order
                    .zip(kept_reading)
                    .map(|(order, reading)| wanted_kept(id, order, reading))
                    .transpose()?,
                pinned: pinned.map(store::from_nanos),
            })
        }))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use proptest::prelude::*;

    use super::*;
    use crate::db::Library;

    #[derive(Clone, Copy, Debug)]
    enum Shuffle {
        Move { rows: Span, to: usize },
        Take { rows: Span },
    }

    fn a_playlist_and_its_shuffling() -> impl Strategy<Value = (usize, Vec<Shuffle>)> {
        (4usize..9).prop_flat_map(|held| {
            let reach = held + 2;
            (
                Just(held),
                prop::collection::vec(
                    prop_oneof![
                        3 => (0..reach, 0..reach, 0..reach).prop_map(|(one, other, to)| {
                            Shuffle::Move {
                                rows: Span::between(one, other),
                                to,
                            }
                        }),
                        1 => (0..reach, 0..reach).prop_map(|(one, other)| Shuffle::Take {
                            rows: Span::between(one, other),
                        }),
                    ],
                    1..6,
                ),
            )
        })
    }

    fn a_playlist_a_span_and_a_row_beside_it() -> impl Strategy<Value = (usize, Span, usize)> {
        (2usize..9)
            .prop_flat_map(|held| (Just(held), 0..held, 0..held, 0..held))
            .prop_map(|(held, one, other, to)| (held, Span::between(one, other), to))
            .prop_filter("a row the span does not already hold", |(_, rows, to)| {
                !rows.holds(*to)
            })
    }

    fn a_playlist_of(library: &Library, held: usize) -> (PlaylistId, Vec<String>) {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let named = NEXT.fetch_add(1, Ordering::Relaxed);
        let id = library
            .create_playlist(&format!("Evening {named}"))
            .expect("a playlist named after no other is created");
        let wanted = (0..held)
            .map(|row| format!("/music/{named}-{row}.wav"))
            .collect::<Vec<_>>();
        let cuts = wanted
            .iter()
            .map(|path| Cut::whole(MediaLocation::local(path)))
            .collect::<Vec<_>>();
        library
            .add_to_playlist(id, &cuts)
            .expect("a playlist takes the rows it is given");

        (id, wanted)
    }

    fn positions(library: &Library, id: PlaylistId) -> Vec<(i64, String)> {
        library
            .inner()
            .read(|connection| numbered(connection, id))
            .expect("the catalog answers with the rows it holds")
            .into_iter()
            .map(|(position, row)| (position, row.path))
            .collect()
    }

    fn as_listed(model: &[String]) -> Vec<(i64, String)> {
        model
            .iter()
            .enumerate()
            .map(|(position, path)| (position as i64, path.clone()))
            .collect()
    }

    fn dropped_on(model: &mut Vec<String>, rows: Span, to: usize) -> bool {
        let held = model.len();
        if rows.holds(to) || rows.last() >= held || to >= held {
            return false;
        }

        let under = model[to].clone();
        let taken = model.drain(rows.range()).collect::<Vec<_>>();
        let beside = model
            .iter()
            .position(|row| *row == under)
            .expect("the row a span is dropped on is none of its own");
        let at = if to < rows.first() {
            beside
        } else {
            beside + 1
        };
        model.splice(at..at, taken);
        true
    }

    fn taken_out(model: &mut Vec<String>, rows: Span) -> bool {
        let Some(last) = model.len().checked_sub(1) else {
            return false;
        };
        if rows.first() > last {
            return false;
        }

        model.drain(rows.first()..=rows.last().min(last));
        true
    }

    #[test]
    fn every_edit_leaves_the_rows_numbered_from_zero_with_no_gap_and_none_lost() {
        let library = Library::open_in_memory().expect("an in-memory catalog opens");

        proptest!(|((held, shuffling) in a_playlist_and_its_shuffling())| {
            let (id, mut model) = a_playlist_of(&library, held);

            for shuffle in shuffling {
                let (answered, expected) = match shuffle {
                    Shuffle::Move { rows, to } => (
                        move_rows(library.inner(), id, rows, to)
                            .expect("the catalog answers a move"),
                        dropped_on(&mut model, rows, to),
                    ),
                    Shuffle::Take { rows } => (
                        remove_rows(library.inner(), id, rows)
                            .expect("the catalog answers a removal"),
                        taken_out(&mut model, rows),
                    ),
                };

                prop_assert_eq!(answered, expected);
                prop_assert_eq!(positions(&library, id), as_listed(&model));
            }
        });
    }

    #[test]
    fn a_move_and_the_move_back_leave_the_rows_where_they_were() {
        let library = Library::open_in_memory().expect("an in-memory catalog opens");

        proptest!(|((held, rows, to) in a_playlist_a_span_and_a_row_beside_it())| {
            let (id, before) = a_playlist_of(&library, held);
            let landing = rows.landing(to);
            let back = Span::between(landing, landing + rows.rows() - 1);
            let onto = if landing > rows.first() {
                rows.first()
            } else {
                rows.last()
            };

            prop_assert!(move_rows(library.inner(), id, rows, to)
                .expect("the catalog answers a move"));
            prop_assert_ne!(positions(&library, id), as_listed(&before));
            prop_assert!(move_rows(library.inner(), id, back, onto)
                .expect("the catalog answers the move back"));
            prop_assert_eq!(positions(&library, id), as_listed(&before));
        });
    }
}
