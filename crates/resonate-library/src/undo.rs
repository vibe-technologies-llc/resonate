use parking_lot::Mutex;
use resonate_core::{PlaylistId, Span};
use rusqlite::{OptionalExtension as _, Transaction, params};

use crate::{Error, Kept, PlaylistName, Result, SavedQuery, StoreOp, db::Inner, playlist, store};

const STEPS_HELD: usize = 32;

const ROWS_HELD: usize = 50_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Edit {
    Started,
    Renamed,
    Revised,
    Discarded,
    Added,
    Copied,
    Imported,
    Removed,
    Dropped,
    Tidied,
    Folded,
    Moved,
    Ordered,
    Kept,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reach {
    Unmoved,
    Whole,
    Appended,
    Emptied(Span),
    Shuffled(Span),
    From(usize),
}

impl Reach {
    fn settled(self, transaction: &Transaction<'_>, id: PlaylistId) -> Result<Reached> {
        Ok(match self {
            Self::Unmoved => Reached::Unmoved,
            Self::Whole => Reached::Whole,
            Self::Emptied(span) => {
                let (first, holds) = clipped(span, playlist::tail(transaction, id)?);
                Reached::Window {
                    first,
                    holds,
                    leaves: 0,
                }
            }
            Self::Shuffled(span) => {
                let (first, holds) = clipped(span, playlist::tail(transaction, id)?);
                Reached::Window {
                    first,
                    holds,
                    leaves: holds,
                }
            }
            Self::Appended => match playlist::kept_in(transaction, id)? {
                Some(_) => Reached::Whole,
                None => Reached::From(playlist::tail(transaction, id)?),
            },
            Self::From(at) => match playlist::kept_in(transaction, id)? {
                Some(_) => Reached::Whole,
                None => Reached::From(
                    i64::try_from(at)
                        .unwrap_or(i64::MAX)
                        .min(playlist::tail(transaction, id)?),
                ),
            },
        })
    }
}

fn clipped(span: Span, tail: i64) -> (i64, i64) {
    let first = i64::try_from(span.first()).unwrap_or(i64::MAX).min(tail);
    let last = i64::try_from(span.last()).unwrap_or(i64::MAX).min(tail - 1);
    (first, (last - first + 1).max(0))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reached {
    Unmoved,
    Whole,
    From(i64),
    Window { first: i64, holds: i64, leaves: i64 },
}

impl Reached {
    const fn turned(self) -> Self {
        match self {
            Self::Window {
                first,
                holds,
                leaves,
            } => Self::Window {
                first,
                holds: leaves,
                leaves: holds,
            },
            reached => reached,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Undoable {
    pub edit: Edit,
    pub playlist: PlaylistId,
    pub name: String,
    pub behind: usize,
}

pub(crate) struct Step {
    edit: Edit,
    playlist: PlaylistId,
    reached: Reached,
    standing: Standing,
    left_at: Option<i64>,
}

impl Step {
    fn rows(&self) -> usize {
        match &self.standing {
            Standing::Was(held) => held.rows.as_ref().map_or(0, Vec::len),
            Standing::Fresh(_) => 0,
        }
    }

    fn named(&self) -> &str {
        match &self.standing {
            Standing::Was(held) => &held.name,
            Standing::Fresh(name) => name,
        }
    }

    fn asked(&self, behind: usize) -> Undoable {
        Undoable {
            edit: self.edit,
            playlist: self.playlist,
            name: self.named().to_owned(),
            behind,
        }
    }
}

enum Standing {
    Was(Held),
    Fresh(String),
}

struct Held {
    name: String,
    created: i64,
    modified: i64,
    played: Option<i64>,
    plays: i64,
    pinned: Option<i64>,
    kept: Option<Kept>,
    query: Option<SavedQuery>,
    rows: Option<Vec<playlist::Row>>,
}

pub(crate) enum Change<T> {
    Made(T),
    Nothing(T),
}

pub(crate) fn edited<T>(
    inner: &Inner,
    id: PlaylistId,
    edit: Edit,
    reach: Reach,
    change: impl FnOnce(&Transaction<'_>) -> Result<Change<T>>,
) -> Result<T> {
    let (value, held) = inner.write(|transaction| {
        let reached = reach.settled(transaction, id)?;
        let before = held_in(transaction, id, reached)?;

        Ok(match change(transaction)? {
            Change::Made(value) => {
                let left_at = modified_in(transaction, id)?;
                (value, before.map(|held| (reached, held, left_at)))
            }
            Change::Nothing(value) => (value, None),
        })
    })?;

    if let Some((reached, held, left_at)) = held {
        note(
            inner,
            Step {
                edit,
                playlist: id,
                reached,
                standing: Standing::Was(held),
                left_at,
            },
        );
    }
    Ok(value)
}

pub(crate) fn started<T>(
    inner: &Inner,
    edit: Edit,
    name: &PlaylistName,
    change: impl FnOnce(&Transaction<'_>) -> Result<(PlaylistId, T)>,
) -> Result<T> {
    started_under_a_name_found(inner, edit, |transaction| {
        let (id, value) = change(transaction)?;
        Ok((id, name.clone(), value))
    })
}

pub(crate) fn started_under_a_name_found<T>(
    inner: &Inner,
    edit: Edit,
    change: impl FnOnce(&Transaction<'_>) -> Result<(PlaylistId, PlaylistName, T)>,
) -> Result<T> {
    let (id, name, value, left_at) = inner.write(|transaction| {
        let (id, name, value) = change(transaction)?;
        let left_at = modified_in(transaction, id)?;
        Ok((id, name, value, left_at))
    })?;

    note(
        inner,
        Step {
            edit,
            playlist: id,
            reached: Reached::Whole,
            standing: Standing::Fresh(name.as_str().to_owned()),
            left_at,
        },
    );
    Ok(value)
}

pub(crate) fn undoable(inner: &Inner) -> Option<Undoable> {
    topmost(&inner.steps().lock())
}

pub(crate) fn redoable(inner: &Inner) -> Option<Undoable> {
    topmost(&inner.walked().lock())
}

fn topmost(steps: &[Step]) -> Option<Undoable> {
    steps
        .split_last()
        .map(|(step, behind)| step.asked(behind.len()))
}

pub(crate) fn undo(inner: &Inner) -> Result<Option<Undoable>> {
    walk(inner, inner.steps(), inner.walked())
}

pub(crate) fn redo(inner: &Inner) -> Result<Option<Undoable>> {
    walk(inner, inner.walked(), inner.steps())
}

fn walk(
    inner: &Inner,
    from: &Mutex<Vec<Step>>,
    onto: &Mutex<Vec<Step>>,
) -> Result<Option<Undoable>> {
    let (step, behind) = {
        let mut steps = from.lock();
        let Some(step) = steps.pop() else {
            return Ok(None);
        };
        let behind = steps.len();
        (step, behind)
    };
    let id = step.playlist;
    let back = step.reached.turned();

    let put_back = inner.write(|transaction| {
        if modified_in(transaction, id)? != step.left_at {
            return Err(Error::PlaylistChanged(id));
        }

        let standing = standing_in(transaction, id, step.named(), back)?;
        match &step.standing {
            Standing::Was(held) => restored(transaction, id, held, step.reached)?,
            Standing::Fresh(_) => discarded(transaction, id)?,
        }

        let left_at = match &step.standing {
            Standing::Was(held) => Some(held.modified),
            Standing::Fresh(_) => None,
        };
        Ok(Step {
            edit: step.edit,
            playlist: id,
            reached: back,
            standing,
            left_at,
        })
    });
    let inverse = match put_back {
        Ok(inverse) => inverse,
        Err(error @ Error::PlaylistChanged(_)) => return Err(error),
        Err(error) => {
            from.lock().push(step);
            return Err(error);
        }
    };

    if matches!(step.standing, Standing::Fresh(_))
        && inner
            .playing()
            .is_some_and(|playing| playing.playlist == id)
    {
        inner.set_playing(None);
    }
    stacked(&mut onto.lock(), inverse);
    inner.playlists_changed();
    Ok(Some(step.asked(behind)))
}

fn note(inner: &Inner, step: Step) {
    inner.walked().lock().clear();
    stacked(&mut inner.steps().lock(), step);
}

fn stacked(steps: &mut Vec<Step>, step: Step) {
    steps.push(step);

    let mut rows: usize = steps.iter().map(Step::rows).sum();
    while steps.len() > STEPS_HELD || (steps.len() > 1 && rows > ROWS_HELD) {
        rows -= steps.remove(0).rows();
    }
}

fn standing_in(
    transaction: &Transaction<'_>,
    id: PlaylistId,
    named: &str,
    reached: Reached,
) -> Result<Standing> {
    Ok(match held_in(transaction, id, reached)? {
        Some(held) => Standing::Was(held),
        None => Standing::Fresh(named.to_owned()),
    })
}

fn restored(
    transaction: &Transaction<'_>,
    id: PlaylistId,
    held: &Held,
    reached: Reached,
) -> Result<()> {
    let name = PlaylistName::new(held.name.as_str());
    playlist::refuse_duplicate(transaction, &name, Some(id))?;

    let rows = held.rows.as_deref().unwrap_or_default();
    match reached {
        Reached::Unmoved => written_over(transaction, id, held),
        Reached::Whole => rewritten(transaction, id, held, rows),
        Reached::From(first) => {
            written_over(transaction, id, held)?;
            rewritten_from(transaction, id, first, rows)
        }
        Reached::Window { first, leaves, .. } => {
            written_over(transaction, id, held)?;
            rewritten_within(transaction, id, first, leaves, rows)
        }
    }
}

fn rewritten_within(
    transaction: &Transaction<'_>,
    id: PlaylistId,
    first: i64,
    leaves: i64,
    rows: &[playlist::Row],
) -> Result<()> {
    transaction
        .execute(
            "DELETE FROM playlist_entries
             WHERE playlist_id = ?1 AND position >= ?2 AND position < ?3",
            params![id.get() as i64, first, first + leaves],
        )
        .map_err(|source| Error::store(StoreOp::Delete, source))?;

    let holds = rows.len() as i64;
    if holds != leaves {
        playlist::closed_up(transaction, id, first + leaves, leaves - holds)?;
    }
    for (position, row) in (first..).zip(rows) {
        playlist::insert(transaction, id, position, row)?;
    }
    Ok(())
}

fn rewritten_from(
    transaction: &Transaction<'_>,
    id: PlaylistId,
    first: i64,
    rows: &[playlist::Row],
) -> Result<()> {
    transaction
        .execute(
            "DELETE FROM playlist_entries WHERE playlist_id = ?1 AND position >= ?2",
            params![id.get() as i64, first],
        )
        .map_err(|source| Error::store(StoreOp::Delete, source))?;

    let landing = playlist::tail(transaction, id)?;
    for (position, row) in (landing..).zip(rows) {
        playlist::insert(transaction, id, position, row)?;
    }
    Ok(())
}

fn written_over(transaction: &Transaction<'_>, id: PlaylistId, held: &Held) -> Result<()> {
    let standing = transaction
        .execute(
            "UPDATE playlists
                SET name = ?2, folded = ?3, created = ?4, modified = ?5,
                    kept_order = ?6, kept_reading = ?7
              WHERE id = ?1",
            params![
                id.get() as i64,
                held.name.as_str(),
                playlist::folded(held.name.as_str()),
                held.created,
                held.modified,
                held.kept.map(|kept| store::row_order_code(kept.order)),
                held.kept.map(|kept| store::direction_code(kept.reading))
            ],
        )
        .map_err(|source| Error::store(StoreOp::Update, source))?;

    if standing == 0 {
        return Err(Error::UnknownPlaylist(id));
    }
    transaction
        .execute(
            "DELETE FROM playlist_queries WHERE playlist_id = ?1",
            params![id.get() as i64],
        )
        .map_err(|source| Error::store(StoreOp::Delete, source))?;

    if let Some(query) = held.query.as_ref() {
        playlist::write_query(transaction, id, query)?;
    }
    Ok(())
}

fn rewritten(
    transaction: &Transaction<'_>,
    id: PlaylistId,
    held: &Held,
    rows: &[playlist::Row],
) -> Result<()> {
    let Unedited {
        played,
        plays,
        pinned,
    } = unedited_in(transaction, id)?.unwrap_or(Unedited {
        played: held.played,
        plays: held.plays,
        pinned: held.pinned,
    });

    transaction
        .execute(
            "DELETE FROM playlists WHERE id = ?1",
            params![id.get() as i64],
        )
        .map_err(|source| Error::store(StoreOp::Delete, source))?;
    transaction
        .execute(
            "INSERT INTO playlists
                 (id, name, folded, created, modified, played, plays, kept_order, kept_reading,
                  pinned)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                id.get() as i64,
                held.name.as_str(),
                playlist::folded(held.name.as_str()),
                held.created,
                held.modified,
                played,
                plays,
                held.kept.map(|kept| store::row_order_code(kept.order)),
                held.kept.map(|kept| store::direction_code(kept.reading)),
                pinned
            ],
        )
        .map_err(|source| Error::store(StoreOp::Insert, source))?;

    if let Some(query) = held.query.as_ref() {
        playlist::write_query(transaction, id, query)?;
    }
    for (position, row) in (0..).zip(rows) {
        playlist::insert(transaction, id, position, row)?;
    }
    Ok(())
}

fn discarded(transaction: &Transaction<'_>, id: PlaylistId) -> Result<()> {
    transaction
        .execute(
            "DELETE FROM playlists WHERE id = ?1",
            params![id.get() as i64],
        )
        .map_err(|source| Error::store(StoreOp::Delete, source))?;

    Ok(())
}

fn held_in(
    transaction: &Transaction<'_>,
    id: PlaylistId,
    reached: Reached,
) -> Result<Option<Held>> {
    let standing = transaction
        .query_row(
            "SELECT name, created, modified, played, plays, kept_order, kept_reading, pinned
             FROM playlists WHERE id = ?1",
            params![id.get() as i64],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                ))
            },
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    let Some((name, created, modified, played, plays, kept_order, kept_reading, pinned)) = standing
    else {
        return Ok(None);
    };

    Ok(Some(Held {
        name,
        created,
        modified,
        played,
        plays,
        pinned,
        kept: kept_order
            .zip(kept_reading)
            .map(|(order, reading)| playlist::wanted_kept(id, order, reading))
            .transpose()?,
        query: playlist::asked_in(transaction, id)?,
        rows: match reached {
            Reached::Unmoved => None,
            Reached::Whole => Some(playlist::rows(transaction, id)?),
            Reached::From(first) => Some(playlist::rows_from(transaction, id, first)?),
            Reached::Window { first, holds, .. } => {
                Some(playlist::rows_within(transaction, id, first, holds)?)
            }
        },
    }))
}

fn modified_in(transaction: &Transaction<'_>, id: PlaylistId) -> Result<Option<i64>> {
    transaction
        .query_row(
            "SELECT modified FROM playlists WHERE id = ?1",
            params![id.get() as i64],
            |row| row.get(0),
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))
}

struct Unedited {
    played: Option<i64>,
    plays: i64,
    pinned: Option<i64>,
}

fn unedited_in(transaction: &Transaction<'_>, id: PlaylistId) -> Result<Option<Unedited>> {
    transaction
        .query_row(
            "SELECT played, plays, pinned FROM playlists WHERE id = ?1",
            params![id.get() as i64],
            |row| {
                Ok(Unedited {
                    played: row.get(0)?,
                    plays: row.get(1)?,
                    pinned: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))
}
