use std::{path::PathBuf, time::SystemTime};

use resonate_core::{MediaLocation, ReleaseTrackId, SourceId, TrackId, WantId};
use resonate_providers::{Delivered, Delivery};
use rusqlite::{OptionalExtension as _, Transaction, params};

use crate::{Error, Result, StoreOp, Track, chosen, store};

#[derive(Clone, Debug)]
pub struct Replaceable {
    pub track: Track,
    pub held: Option<Track>,
    pub release_track: Option<ReleaseTrackId>,
    pub pending: bool,
    pub looked: Option<Looked>,
}

impl Replaceable {
    pub fn can_be_ticked(&self) -> bool {
        !self.pending
            && (self.held.is_some() || self.release_track.is_some() || self.offer().is_some())
    }

    pub const fn can_be_looked_for(&self) -> bool {
        !self.pending && self.held.is_none()
    }

    pub fn is_offered_nowhere(&self) -> bool {
        self.looked == Some(Looked::Unoffered)
    }

    pub fn offer(&self) -> Option<(&SourceId, &MediaLocation)> {
        match &self.looked {
            Some(Looked::Offered { by, at }) => Some((by, at)),
            Some(Looked::Unoffered | Looked::Unheard) | None => None,
        }
    }
}

const NAMED_ONLY_FOR_LOSSY_SOUND: [&str; 12] = [
    "aac", "ac3", "eac3", "m4b", "mp2", "mp3", "mpc", "ogg", "opus", "spx", "webm", "wma",
];

pub(crate) fn offered_for_lossy_sound(delivered: &Delivered) -> bool {
    let extension = match &delivered.delivery {
        Delivery::File(path) => path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default(),
        Delivery::Stream { extension, .. } => extension.as_str(),
    };
    NAMED_ONLY_FOR_LOSSY_SOUND
        .iter()
        .any(|lossy| lossy.eq_ignore_ascii_case(extension))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Looked {
    Offered { by: SourceId, at: MediaLocation },
    Unoffered,
    Unheard,
}

pub struct LookingFor<'a> {
    pub cancelled: &'a (dyn Fn() -> bool + Sync),
    pub told: &'a (dyn Fn(TrackId, Looked) + Sync),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Replacing {
    pub beside: u64,
    pub wanted: u64,
    pub kept: u64,
    pub unfiled: u64,
    pub unmatched: u64,
    pub wants: Vec<(ReleaseTrackId, WantId)>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Replaced {
    pub beside: u64,
}

const THE_WANT_OF_THE_REPLACEMENT_GOES: &str = "DELETE FROM wants
      WHERE release_track_id IN (SELECT release_track_id FROM replacements WHERE track_id = ?1)";

const THE_REPLACEMENT_GOES: &str = "DELETE FROM replacements WHERE track_id = ?1";

pub(crate) fn kept_beside(
    transaction: &Transaction<'_>,
    song: TrackId,
    copy: TrackId,
    at: SystemTime,
) -> Result<()> {
    chosen::choose(transaction, song, copy, at)?;

    let id = song.get() as i64;

    for (sql, op) in [
        (THE_WANT_OF_THE_REPLACEMENT_GOES, StoreOp::Delete),
        (THE_REPLACEMENT_GOES, StoreOp::Delete),
    ] {
        transaction
            .execute(sql, params![id])
            .map_err(|source| Error::store(op, source))?;
    }
    Ok(())
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
