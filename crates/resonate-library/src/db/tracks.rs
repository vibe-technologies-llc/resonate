use std::{path::Path, time::SystemTime};

use resonate_core::{AlbumId, FrameSpan, TrackId};
use rusqlite::{OptionalExtension as _, params, params_from_iter};

use super::{
    HiddenTracks, Library,
    rows::{RawTrack, favoured, unknown},
    searched::{tracks, tracks_from},
    statements::{IDS_READ_AT_ONCE, THE_FAVOURITES_SEARCH, TRACK_COLUMNS},
};
use crate::{Error, Favoured, Result, StoreOp, Track, TrackQuery, store};

impl Library {
    pub fn track(&self, id: TrackId) -> Result<Option<Track>> {
        let raw = self.inner.read(|connection| {
            connection
                .query_row(
                    &format!("SELECT {TRACK_COLUMNS} FROM tracks WHERE id = ?1"),
                    params![id.get() as i64],
                    RawTrack::read,
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))
        })?;

        raw.map(RawTrack::into_track).transpose()
    }

    pub fn tracks_with_ids(&self, ids: &[TrackId]) -> Result<Vec<Track>> {
        let raw = self.inner.read(|connection| {
            let mut read = Vec::with_capacity(ids.len());
            for batch in ids.chunks(IDS_READ_AT_ONCE) {
                let held = vec!["?"; batch.len()].join(",");
                let mut statement = connection
                    .prepare(&format!(
                        "SELECT {TRACK_COLUMNS} FROM tracks WHERE id IN ({held})"
                    ))
                    .map_err(|source| Error::store(StoreOp::Prepare, source))?;
                let rows = statement
                    .query_map(
                        params_from_iter(batch.iter().map(|id| id.get() as i64)),
                        RawTrack::read,
                    )
                    .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
                    .map_err(|source| Error::store(StoreOp::Query, source))?;
                read.extend(rows);
            }
            Ok(read)
        })?;

        raw.into_iter().map(RawTrack::into_track).collect()
    }

    pub fn album_titles(&self, ids: &[AlbumId]) -> Result<Vec<(AlbumId, String)>> {
        self.inner.read(|connection| {
            let mut read = Vec::with_capacity(ids.len());
            for batch in ids.chunks(IDS_READ_AT_ONCE) {
                let held = vec!["?"; batch.len()].join(",");
                let mut statement = connection
                    .prepare(&format!(
                        "SELECT id, title FROM albums WHERE id IN ({held})"
                    ))
                    .map_err(|source| Error::store(StoreOp::Prepare, source))?;
                let rows = statement
                    .query_map(
                        params_from_iter(batch.iter().map(|id| id.get() as i64)),
                        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                    )
                    .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
                    .map_err(|source| Error::store(StoreOp::Query, source))?;
                read.extend(rows.into_iter().filter_map(|(id, title)| {
                    Some((AlbumId::new(u64::try_from(id).ok()?).ok()?, title))
                }));
            }
            Ok(read)
        })
    }

    pub fn track_at(&self, path: &Path, span: Option<FrameSpan>) -> Result<Option<Track>> {
        let text = store::path_text(path);
        let (start, _) = store::span_columns(span);
        let raw = self.inner.read(|connection| {
            connection
                .query_row(
                    &format!(
                        "SELECT {TRACK_COLUMNS} FROM tracks WHERE path = ?1 AND span_start = ?2"
                    ),
                    params![text, start],
                    RawTrack::read,
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))
        })?;

        raw.map(RawTrack::into_track).transpose()
    }

    pub fn tracks(&self, query: &TrackQuery) -> Result<Vec<Track>> {
        tracks(&self.inner, query, None)
    }

    pub fn tracks_from(&self, query: &TrackQuery, at: TrackId) -> Result<Vec<Track>> {
        tracks_from(&self.inner, query, at)
    }

    pub fn favourite_tracks(&self, query: &TrackQuery) -> Result<Vec<Track>> {
        tracks(&self.inner, query, Some(THE_FAVOURITES_SEARCH))
    }

    pub fn hide_track(&self, id: TrackId, hidden: bool) -> Result<bool> {
        self.inner.write(|transaction| {
            transaction
                .execute(
                    "UPDATE tracks SET hidden = ?2 WHERE id = ?1 AND hidden != ?2",
                    (id.get() as i64, hidden),
                )
                .map(|changed| changed > 0)
                .map_err(|source| Error::store(StoreOp::Update, source))
        })
    }

    pub fn hidden_tracks(&self, at_most: usize) -> Result<HiddenTracks> {
        let (counted, ids) = self.inner.read(|connection| {
            let counted = connection
                .query_row("SELECT count(*) FROM tracks WHERE hidden = 1", [], |row| {
                    row.get::<_, i64>(0)
                })
                .map_err(|source| Error::store(StoreOp::Query, source))?;
            let mut statement = connection
                .prepare_cached(
                    "SELECT id FROM tracks WHERE hidden = 1
                      ORDER BY title COLLATE NOCASE, id LIMIT ?1",
                )
                .map_err(|source| Error::store(StoreOp::Prepare, source))?;
            let ids = statement
                .query_map([i64::try_from(at_most).unwrap_or(i64::MAX)], |row| {
                    row.get::<_, i64>(0)
                })
                .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
                .map_err(|source| Error::store(StoreOp::Query, source))?;
            Ok((counted, ids))
        })?;
        let ids = ids
            .into_iter()
            .map(|id| TrackId::new(id as u64))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut tracks = self.tracks_with_ids(&ids)?;
        tracks.sort_by_key(|track| ids.iter().position(|id| *id == track.id));
        Ok(HiddenTracks {
            counted: u64::try_from(counted).unwrap_or(0),
            tracks,
        })
    }

    pub fn show_every_hidden_track(&self) -> Result<u64> {
        self.inner.write(|transaction| {
            transaction
                .execute("UPDATE tracks SET hidden = 0 WHERE hidden = 1", [])
                .map(|changed| changed as u64)
                .map_err(|source| Error::store(StoreOp::Update, source))
        })
    }

    pub fn favour(&self, what: Favoured, favourite: bool) -> Result<bool> {
        let (table, row) = favoured(what);
        let at = favourite.then(|| store::to_nanos(SystemTime::now()));
        let standing = if favourite { "IS NULL" } else { "IS NOT NULL" };

        self.inner.write(|transaction| {
            transaction
                .execute(
                    &format!(
                        "UPDATE {table} SET favourite = ?1 WHERE id = ?2 AND favourite {standing}"
                    ),
                    params![at, row],
                )
                .map(|changed| changed > 0)
                .map_err(|source| Error::store(StoreOp::Update, source))
        })
    }

    pub fn favour_all(&self, marked: &[Favoured], favourite: bool) -> Result<usize> {
        let at = favourite.then(|| store::to_nanos(SystemTime::now()));
        let standing = if favourite { "IS NULL" } else { "IS NOT NULL" };

        self.inner.write(|transaction| {
            let mut changed = 0;
            for what in marked {
                let (table, row) = favoured(*what);
                let held = transaction
                    .query_row(
                        &format!("SELECT 1 FROM {table} WHERE id = ?1"),
                        params![row],
                        |_| Ok(()),
                    )
                    .optional()
                    .map_err(|source| Error::store(StoreOp::Query, source))?;
                if held.is_none() {
                    return Err(unknown(*what));
                }
                changed += transaction
                    .execute(
                        &format!(
                            "UPDATE {table} SET favourite = ?1 WHERE id = ?2 AND favourite {standing}"
                        ),
                        params![at, row],
                    )
                    .map_err(|source| Error::store(StoreOp::Update, source))?;
            }
            Ok(changed)
        })
    }
}
