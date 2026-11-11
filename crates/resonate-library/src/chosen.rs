use std::time::SystemTime;

use ahash::AHashMap;
use resonate_core::{FrameSpan, MediaLocation, TrackId};
use rusqlite::{Connection, OptionalExtension, Transaction, params, params_from_iter};

use crate::{Cut, Error, Result, StoreOp, Track, store};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Version {
    Held(TrackId),
    Served(MediaLocation),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChosenVersion {
    pub track: TrackId,
    pub version: Version,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Played {
    Held { track: TrackId, cut: Cut },
    Served(MediaLocation),
}

#[derive(Clone, Debug, Default)]
pub struct Chosen {
    for_track: AHashMap<TrackId, (Cut, Played)>,
}

impl Chosen {
    pub fn is_empty(&self) -> bool {
        self.for_track.is_empty()
    }

    pub fn in_place_of(
        &self,
        track: TrackId,
        location: &MediaLocation,
        span: Option<FrameSpan>,
    ) -> Option<&Played> {
        self.for_track
            .get(&track)
            .filter(|(held, _)| held.location == *location && held.span == span)
            .map(|(_, played)| played)
    }

    pub fn of(versions: Vec<ChosenVersion>, held: &[Track]) -> Self {
        let held: AHashMap<TrackId, &Track> = held.iter().map(|track| (track.id, track)).collect();
        let for_track = versions
            .into_iter()
            .filter_map(|chosen| {
                let song = Cut::of(held.get(&chosen.track)?);
                let played = match chosen.version {
                    Version::Held(copy) => Played::Held {
                        track: copy,
                        cut: Cut::of(held.get(&copy)?),
                    },
                    Version::Served(location) => Played::Served(location),
                };
                Some((chosen.track, (song, played)))
            })
            .collect();

        Self { for_track }
    }
}

const THE_TRACK_HELD: &str = "SELECT 1 FROM tracks WHERE id = ?1";

const CHOSEN: &str = "INSERT INTO versions_chosen (track, held, served, chosen)
     VALUES (?1, ?2, ?3, ?4)
     ON CONFLICT (track) DO UPDATE
        SET held = excluded.held, served = excluded.served, chosen = excluded.chosen";

const FORGOTTEN: &str = "DELETE FROM versions_chosen WHERE track = ?1";

const EVERY_VERSION_CHOSEN: &str = "SELECT track, held, served FROM versions_chosen";

const THE_VERSION_CHOSEN: &str = "SELECT held, served FROM versions_chosen WHERE track = ?1";

pub(crate) fn choose(
    transaction: &Transaction<'_>,
    track: TrackId,
    version: &Version,
    at: SystemTime,
) -> Result<bool> {
    held(transaction, track)?;
    if let Version::Held(copy) = version {
        held(transaction, *copy)?;
        if *copy == track {
            return forget(transaction, track);
        }
    }

    let (copy, served) = match version {
        Version::Held(copy) => (Some(copy.get() as i64), None),
        Version::Served(location) => (None, Some(location.to_uri())),
    };
    transaction
        .execute(
            CHOSEN,
            params![track.get() as i64, copy, served, store::to_nanos(at)],
        )
        .map(|_| true)
        .map_err(|source| Error::store(StoreOp::Update, source))
}

pub(crate) fn forget(transaction: &Transaction<'_>, track: TrackId) -> Result<bool> {
    transaction
        .execute(FORGOTTEN, params![track.get() as i64])
        .map(|removed| removed > 0)
        .map_err(|source| Error::store(StoreOp::Delete, source))
}

pub(crate) fn every(connection: &Connection) -> Result<Vec<ChosenVersion>> {
    let mut statement = connection
        .prepare_cached(EVERY_VERSION_CHOSEN)
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let rows = statement
        .query_map([], chosen_row)
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    Ok(rows.into_iter().filter_map(chosen_version).collect())
}

pub(crate) fn among(
    connection: &Connection,
    tracks: &[TrackId],
    at_once: usize,
) -> Result<Vec<ChosenVersion>> {
    let mut chosen = Vec::new();
    for batch in tracks.chunks(at_once) {
        let held = vec!["?"; batch.len()].join(",");
        let mut statement = connection
            .prepare(&format!(
                "SELECT track, held, served FROM versions_chosen WHERE track IN ({held})"
            ))
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;
        let rows = statement
            .query_map(
                params_from_iter(batch.iter().map(|track| track.get() as i64)),
                chosen_row,
            )
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))?;
        chosen.extend(rows.into_iter().filter_map(chosen_version));
    }
    Ok(chosen)
}

fn chosen_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(i64, Option<i64>, Option<String>)> {
    Ok((
        row.get::<_, i64>(0)?,
        row.get::<_, Option<i64>>(1)?,
        row.get::<_, Option<String>>(2)?,
    ))
}

fn chosen_version(
    (track, copy, served): (i64, Option<i64>, Option<String>),
) -> Option<ChosenVersion> {
    Some(ChosenVersion {
        track: track_id(track)?,
        version: version_of(copy, served.as_deref())?,
    })
}

pub(crate) fn of(connection: &Connection, track: TrackId) -> Result<Option<Version>> {
    let row = connection
        .query_row(THE_VERSION_CHOSEN, params![track.get() as i64], |row| {
            Ok((
                row.get::<_, Option<i64>>(0)?,
                row.get::<_, Option<String>>(1)?,
            ))
        })
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    Ok(row.and_then(|(copy, served)| version_of(copy, served.as_deref())))
}

fn held(transaction: &Transaction<'_>, track: TrackId) -> Result<()> {
    transaction
        .query_row(THE_TRACK_HELD, params![track.get() as i64], |_| Ok(()))
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?
        .ok_or(Error::UnknownTrack(track))
}

fn version_of(copy: Option<i64>, served: Option<&str>) -> Option<Version> {
    match (copy, served) {
        (Some(copy), None) => track_id(copy).map(Version::Held),
        (None, Some(uri)) => MediaLocation::from_uri(uri).map(Version::Served),
        _ => None,
    }
}

fn track_id(id: i64) -> Option<TrackId> {
    u64::try_from(id).ok().and_then(|id| TrackId::new(id).ok())
}
