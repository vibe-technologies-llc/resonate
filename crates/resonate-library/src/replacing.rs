use std::path::PathBuf;

use resonate_core::{MediaLocation, ReleaseTrackId, TrackId, WantId};
use rusqlite::{OptionalExtension as _, ToSql, Transaction, params};

use crate::{
    Deleted, Error, Result, StoreOp, Track,
    copies::{self, HandOver},
    store,
};

#[derive(Clone, Debug)]
pub struct Replaceable {
    pub track: Track,
    pub held: Option<Track>,
    pub release_track: Option<ReleaseTrackId>,
    pub pending: bool,
}

impl Replaceable {
    pub const fn can_be_ticked(&self) -> bool {
        !self.pending && (self.held.is_some() || self.release_track.is_some())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Replacing {
    pub folded: u64,
    pub wanted: u64,
    pub unmatched: u64,
    pub trashed: Deleted,
    pub folds: Vec<Fold>,
    pub wants: Vec<(ReleaseTrackId, WantId)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fold {
    pub taken: MediaLocation,
    pub kept: MediaLocation,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Replaced {
    pub trashed: Deleted,
    pub folds: Vec<Fold>,
}

const THE_RELEASE_ROW_GOES_TO_THE_KEPT_COPY_WHERE_IT_HOLDS_NONE: &str = "UPDATE release_tracks
        SET track_id = ?1
      WHERE track_id = ?2
        AND NOT EXISTS (SELECT 1 FROM release_tracks WHERE track_id = ?1)";

const THE_WANT_OF_THE_REPLACEMENT_GOES: &str = "DELETE FROM wants
      WHERE release_track_id IN (SELECT release_track_id FROM replacements WHERE track_id = ?1)";

const THE_REPLACEMENT_GOES: &str = "DELETE FROM replacements WHERE track_id = ?1";

pub(crate) fn folded(transaction: &Transaction<'_>, kept: TrackId, taken: TrackId) -> Result<Fold> {
    let kept_path = path_of(transaction, kept)?;
    let taken_path = path_of(transaction, taken)?;
    copies::handed_over(
        transaction,
        &HandOver {
            kept,
            kept_path: &kept_path,
            taken,
            taken_path: &taken_path,
        },
    )?;

    let (kept, taken) = (kept.get() as i64, taken.get() as i64);
    let changed = |sql: &str, values: &[&dyn ToSql], op: StoreOp| {
        transaction
            .execute(sql, values)
            .map(drop)
            .map_err(|source| Error::store(op, source))
    };
    changed(
        THE_RELEASE_ROW_GOES_TO_THE_KEPT_COPY_WHERE_IT_HOLDS_NONE,
        params![kept, taken],
        StoreOp::Update,
    )?;
    changed(
        THE_WANT_OF_THE_REPLACEMENT_GOES,
        params![taken],
        StoreOp::Delete,
    )?;
    changed(THE_REPLACEMENT_GOES, params![taken], StoreOp::Delete)?;
    Ok(Fold {
        taken: MediaLocation::local(taken_path),
        kept: MediaLocation::local(kept_path),
    })
}

pub(crate) fn path_of(transaction: &Transaction<'_>, track: TrackId) -> Result<PathBuf> {
    transaction
        .query_row(
            "SELECT path FROM tracks WHERE id = ?1",
            params![track.get() as i64],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?
        .map(|path| store::path_of(&path))
        .ok_or(Error::UnknownTrack(track))
}
