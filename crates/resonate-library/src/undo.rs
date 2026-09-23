use parking_lot::Mutex;
use resonate_core::PlaylistId;
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

impl Edit {
    const fn moves_rows(self) -> bool {
        !matches!(self, Self::Renamed | Self::Revised)
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
    standing: Standing,
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
    change: impl FnOnce(&Transaction<'_>) -> Result<Change<T>>,
) -> Result<T> {
    let (value, held) = inner.write(|transaction| {
        let before = held_in(transaction, id, edit)?;

        Ok(match change(transaction)? {
            Change::Made(value) => (value, before),
            Change::Nothing(value) => (value, None),
        })
    })?;

    if let Some(held) = held {
        note(
            inner,
            Step {
                edit,
                playlist: id,
                standing: Standing::Was(held),
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
    let (id, value) = inner.write(change)?;

    note(
        inner,
        Step {
            edit,
            playlist: id,
            standing: Standing::Fresh(name.as_str().to_owned()),
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

    let put_back = inner.write(|transaction| {
        let standing = standing_in(transaction, id, step.named(), step.edit)?;
        match &step.standing {
            Standing::Was(held) => restored(transaction, id, held)?,
            Standing::Fresh(_) => discarded(transaction, id)?,
        }

        Ok(Step {
            edit: step.edit,
            playlist: id,
            standing,
        })
    });
    let inverse = match put_back {
        Ok(inverse) => inverse,
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
    edit: Edit,
) -> Result<Standing> {
    Ok(match held_in(transaction, id, edit)? {
        Some(held) => Standing::Was(held),
        None => Standing::Fresh(named.to_owned()),
    })
}

fn restored(transaction: &Transaction<'_>, id: PlaylistId, held: &Held) -> Result<()> {
    let name = PlaylistName::new(held.name.as_str());
    playlist::refuse_duplicate(transaction, &name, Some(id))?;

    match held.rows.as_deref() {
        Some(rows) => rewritten(transaction, id, held, rows),
        None => written_over(transaction, id, held),
    }
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

fn held_in(transaction: &Transaction<'_>, id: PlaylistId, edit: Edit) -> Result<Option<Held>> {
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
        rows: edit
            .moves_rows()
            .then(|| playlist::rows(transaction, id))
            .transpose()?,
    }))
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
