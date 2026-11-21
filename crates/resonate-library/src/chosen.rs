use std::time::SystemTime;

use ahash::AHashMap;
use resonate_core::{FrameSpan, MediaLocation, TrackId};
use rusqlite::{Connection, OptionalExtension, Transaction, params, params_from_iter};

use crate::{Cut, Error, Result, StoreOp, Track, store};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChosenVersion {
    pub track: TrackId,
    pub copy: TrackId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Played {
    pub track: TrackId,
    pub cut: Cut,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Variants {
    pub song: Track,
    pub held: Vec<Track>,
    pub chosen: Option<TrackId>,
}

impl Variants {
    pub fn plays(&self) -> TrackId {
        self.chosen.unwrap_or(self.song.id)
    }
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
                let played = Played {
                    track: chosen.copy,
                    cut: Cut::of(held.get(&chosen.copy)?),
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

const A_VERSION_IS_HELD_BESIDE_ITS_SONG: &str =
    "INSERT OR IGNORE INTO versions_held (track, copy) VALUES (?1, ?2)";

const EVERY_VERSION_CHOSEN: &str = "SELECT track, held FROM versions_chosen WHERE held IS NOT NULL";

const THE_VERSION_CHOSEN: &str = "SELECT held FROM versions_chosen WHERE track = ?1";

const THE_SONG_A_VERSION_IS_HELD_FOR: &str =
    "SELECT track FROM versions_held WHERE copy = ?1 ORDER BY track LIMIT 1";

const EVERY_TRACK_WITH_VERSIONS: &str =
    "SELECT track FROM versions_held UNION SELECT copy FROM versions_held";

const THE_VERSIONS_HELD_FOR_A_SONG: &str =
    "SELECT copy FROM versions_held WHERE track = ?1 ORDER BY copy";

pub(crate) fn choose(
    transaction: &Transaction<'_>,
    track: TrackId,
    copy: TrackId,
    at: SystemTime,
) -> Result<bool> {
    held(transaction, track)?;
    held(transaction, copy)?;
    if copy == track {
        return forget(transaction, track);
    }

    transaction
        .execute(
            A_VERSION_IS_HELD_BESIDE_ITS_SONG,
            params![track.get() as i64, copy.get() as i64],
        )
        .map_err(|source| Error::store(StoreOp::Insert, source))?;
    transaction
        .execute(
            CHOSEN,
            params![
                track.get() as i64,
                copy.get() as i64,
                Option::<String>::None,
                store::to_nanos(at)
            ],
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
                "SELECT track, held FROM versions_chosen WHERE held IS NOT NULL AND track IN ({held})"
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

fn chosen_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(i64, i64)> {
    Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
}

fn chosen_version((track, copy): (i64, i64)) -> Option<ChosenVersion> {
    Some(ChosenVersion {
        track: track_id(track)?,
        copy: track_id(copy)?,
    })
}

pub(crate) fn of(connection: &Connection, track: TrackId) -> Result<Option<TrackId>> {
    let copy = connection
        .query_row(THE_VERSION_CHOSEN, params![track.get() as i64], |row| {
            row.get::<_, Option<i64>>(0)
        })
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    Ok(copy.flatten().and_then(track_id))
}

pub(crate) fn song_of(connection: &Connection, copy: TrackId) -> Result<Option<TrackId>> {
    let song = connection
        .query_row(
            THE_SONG_A_VERSION_IS_HELD_FOR,
            params![copy.get() as i64],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    Ok(song.and_then(track_id))
}

pub(crate) fn held_for(connection: &Connection, song: TrackId) -> Result<Vec<TrackId>> {
    let mut statement = connection
        .prepare_cached(THE_VERSIONS_HELD_FOR_A_SONG)
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;

    let copies = statement
        .query_map(params![song.get() as i64], |row| row.get::<_, i64>(0))
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    Ok(copies.into_iter().filter_map(track_id).collect())
}

pub(crate) fn varied(connection: &Connection) -> Result<Vec<TrackId>> {
    let mut statement = connection
        .prepare_cached(EVERY_TRACK_WITH_VERSIONS)
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;

    let tracks = statement
        .query_map([], |row| row.get::<_, i64>(0))
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    Ok(tracks.into_iter().filter_map(track_id).collect())
}

fn held(transaction: &Transaction<'_>, track: TrackId) -> Result<()> {
    transaction
        .query_row(THE_TRACK_HELD, params![track.get() as i64], |_| Ok(()))
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?
        .ok_or(Error::UnknownTrack(track))
}

fn track_id(id: i64) -> Option<TrackId> {
    u64::try_from(id).ok().and_then(|id| TrackId::new(id).ok())
}
