use resonate_core::PlaylistId;
use rusqlite::{Transaction, params};

use crate::{
    Error, OrderedColumn, Result, SavedQuery, StoreOp,
    db::Inner,
    playlist, store,
    undo::{Edit, Held, Maker, ROWS_HELD, Reached, STEPS_HELD, Standing, Step},
};

const UNMOVED: i64 = 0;

const WHOLE: i64 = 1;

const FROM: i64 = 2;

const WINDOW: i64 = 3;

pub(crate) fn offered_if_asked(
    inner: &Inner,
    transaction: &Transaction<'_>,
    step: Step,
) -> Result<Step> {
    if !inner.offers() || step.rows() > ROWS_HELD {
        return Ok(step);
    }

    let offer = offer(transaction, &step)?;
    trimmed(transaction)?;
    Ok(Step {
        offered: Some(offer),
        ..step
    })
}

pub(crate) fn withdrawn(transaction: &Transaction<'_>, offer: i64) -> Result<()> {
    store::cached(
        transaction,
        "DELETE FROM offered_steps WHERE id = ?1",
        params![offer],
    )
    .map_err(|source| Error::store(StoreOp::Delete, source))?;

    Ok(())
}

pub(crate) fn taken(transaction: &Transaction<'_>) -> Result<Vec<Step>> {
    let offers = offers_in(transaction)?;

    let mut steps = Vec::with_capacity(offers.len());
    for offer in offers {
        let rows = if offer.rows_held {
            Some(rows_of(transaction, offer.id)?)
        } else {
            None
        };
        match offer.into_step(rows) {
            Some(step) => steps.push(step),
            None => tracing::warn!(
                "an edit a model offered was written by another build and is passed over"
            ),
        }
    }

    transaction
        .execute("DELETE FROM offered_steps", [])
        .map_err(|source| Error::store(StoreOp::Delete, source))?;
    Ok(steps)
}

fn offer(transaction: &Transaction<'_>, step: &Step) -> Result<i64> {
    let (reached, first, holds, leaves) = match step.reached {
        Reached::Unmoved => (UNMOVED, 0, 0, 0),
        Reached::Whole => (WHOLE, 0, 0, 0),
        Reached::From(first) => (FROM, first, 0, 0),
        Reached::Window {
            first,
            holds,
            leaves,
        } => (WINDOW, first, holds, leaves),
    };
    let held = match &step.standing {
        Standing::Was(held) => Some(held),
        Standing::Fresh(_) => None,
    };
    let query = held.and_then(|held| held.query.as_ref());
    let kept = held.and_then(|held| held.kept);
    let rows = held.and_then(|held| held.rows.as_deref());

    transaction
        .execute(
            "INSERT INTO offered_steps
                 (edit, playlist_id, reached, first, holds, leaves, left_at, name, was_held,
                  created, modified, played, plays, pinned, kept_order, kept_reading, queried,
                  query_text, query_sort, query_reading, query_max_rows, rows_held, row_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                     ?18, ?19, ?20, ?21, ?22, ?23)",
            params![
                step.edit.code(),
                step.playlist.get() as i64,
                reached,
                first,
                holds,
                leaves,
                step.left_at,
                step.named(),
                held.is_some(),
                held.map_or(0, |held| held.created),
                held.map_or(0, |held| held.modified),
                held.and_then(|held| held.played),
                held.map_or(0, |held| held.plays),
                held.and_then(|held| held.pinned),
                kept.map(|kept| store::row_order_code(kept.order)),
                kept.map(|kept| store::direction_code(kept.reading)),
                query.is_some(),
                query.and_then(|query| query.text.as_deref()),
                query.map(|query| store::sort_code(query.sort)),
                query.map(|query| store::direction_code(query.reading)),
                query.and_then(|query| query.limit.map(|limit| limit as i64)),
                rows.is_some(),
                rows.map_or(0, |rows| rows.len() as i64)
            ],
        )
        .map_err(|source| Error::store(StoreOp::Insert, source))?;
    let offer = transaction.last_insert_rowid();

    for (position, row) in (0_i64..).zip(rows.unwrap_or_default()) {
        store::cached(
            transaction,
            "INSERT INTO offered_step_rows (step, position, path, span_start, span_frames)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![offer, position, row.path, row.start, row.frames],
        )
        .map_err(|source| Error::store(StoreOp::Insert, source))?;
    }
    Ok(offer)
}

fn trimmed(transaction: &Transaction<'_>) -> Result<()> {
    let mut statement = transaction
        .prepare_cached("SELECT id, row_count FROM offered_steps ORDER BY id DESC")
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    let held = statement
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    let mut rows: i64 = 0;
    let first_dropped = held.iter().enumerate().find_map(|(at, (id, count))| {
        rows = rows.saturating_add(*count);
        let over = at >= STEPS_HELD || (at > 0 && rows > ROWS_HELD as i64);
        over.then_some(*id)
    });

    if let Some(dropped) = first_dropped {
        transaction
            .execute("DELETE FROM offered_steps WHERE id <= ?1", params![dropped])
            .map_err(|source| Error::store(StoreOp::Delete, source))?;
    }
    Ok(())
}

struct Offer {
    id: i64,
    edit: i64,
    playlist: i64,
    reached: (i64, i64, i64, i64),
    left_at: Option<i64>,
    name: String,
    was_held: bool,
    created: i64,
    modified: i64,
    played: Option<i64>,
    plays: i64,
    pinned: Option<i64>,
    kept: (Option<i64>, Option<i64>),
    queried: bool,
    query_text: Option<String>,
    query_sort: Option<i64>,
    query_reading: Option<i64>,
    query_max_rows: Option<i64>,
    rows_held: bool,
}

impl Offer {
    fn into_step(self, rows: Option<Vec<playlist::Row>>) -> Option<Step> {
        let edit = Edit::coded(self.edit)?;
        let playlist = PlaylistId::new(u64::try_from(self.playlist).ok()?).ok()?;
        let reached = match self.reached {
            (UNMOVED, ..) => Reached::Unmoved,
            (WHOLE, ..) => Reached::Whole,
            (FROM, first, ..) => Reached::From(first),
            (WINDOW, first, holds, leaves) => Reached::Window {
                first,
                holds,
                leaves,
            },
            _ => return None,
        };

        let standing = if self.was_held {
            let kept = match self.kept {
                (Some(order), Some(reading)) => {
                    Some(playlist::wanted_kept(playlist, order, reading).ok()?)
                }
                _ => None,
            };
            let query = if self.queried {
                Some(self.query(playlist)?)
            } else {
                None
            };
            Standing::Was(Held {
                name: self.name,
                created: self.created,
                modified: self.modified,
                played: self.played,
                plays: self.plays,
                pinned: self.pinned,
                kept,
                query,
                rows,
            })
        } else {
            Standing::Fresh(self.name)
        };

        Some(Step {
            edit,
            playlist,
            reached,
            standing,
            left_at: self.left_at,
            made_by: Maker::AModel,
            offered: None,
        })
    }

    fn query(&self, playlist: PlaylistId) -> Option<SavedQuery> {
        let sort = store::sort_of(playlist, self.query_sort?).ok()?;
        let reading =
            store::direction_of(playlist, OrderedColumn::Reading, self.query_reading?).ok()?;

        Some(SavedQuery {
            text: self.query_text.clone(),
            sort,
            reading,
            limit: self
                .query_max_rows
                .and_then(|rows| usize::try_from(rows).ok()),
        })
    }
}

fn offers_in(transaction: &Transaction<'_>) -> Result<Vec<Offer>> {
    let mut statement = transaction
        .prepare_cached(
            "SELECT id, edit, playlist_id, reached, first, holds, leaves, left_at, name, was_held,
                    created, modified, played, plays, pinned, kept_order, kept_reading, queried,
                    query_text, query_sort, query_reading, query_max_rows, rows_held
             FROM offered_steps ORDER BY id DESC",
        )
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    statement
        .query_map([], |row| {
            Ok(Offer {
                id: row.get(0)?,
                edit: row.get(1)?,
                playlist: row.get(2)?,
                reached: (row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?),
                left_at: row.get(7)?,
                name: row.get(8)?,
                was_held: row.get(9)?,
                created: row.get(10)?,
                modified: row.get(11)?,
                played: row.get(12)?,
                plays: row.get(13)?,
                pinned: row.get(14)?,
                kept: (row.get(15)?, row.get(16)?),
                queried: row.get(17)?,
                query_text: row.get(18)?,
                query_sort: row.get(19)?,
                query_reading: row.get(20)?,
                query_max_rows: row.get(21)?,
                rows_held: row.get(22)?,
            })
        })
        .and_then(Iterator::collect)
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn rows_of(transaction: &Transaction<'_>, offer: i64) -> Result<Vec<playlist::Row>> {
    let mut statement = transaction
        .prepare_cached(
            "SELECT path, span_start, span_frames FROM offered_step_rows
             WHERE step = ?1 ORDER BY position",
        )
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    statement
        .query_map(params![offer], |row| {
            Ok(playlist::Row {
                path: row.get(0)?,
                start: row.get(1)?,
                frames: row.get(2)?,
            })
        })
        .and_then(Iterator::collect)
        .map_err(|source| Error::store(StoreOp::Query, source))
}
