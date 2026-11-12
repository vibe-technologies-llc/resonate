use std::time::SystemTime;

use ahash::AHashSet;
use resonate_core::TrackId;
use rusqlite::types::Value;

use super::{
    Library,
    searched::collect,
    statements::{IDS_READ_AT_ONCE, TRACK_COLUMNS},
};
use crate::{Chosen, ChosenVersion, Result, Track, Version, chosen};

impl Library {
    pub fn alternatives_of(&self, id: TrackId) -> Result<Vec<Track>> {
        collect(
            &self.inner,
            &format!(
                "SELECT {TRACK_COLUMNS} FROM tracks WHERE alternative_of = ?1
                  ORDER BY codec, sample_rate DESC, sample_format DESC, id"
            ),
            vec![Value::Integer(id.get() as i64)],
        )
    }

    pub fn choose_version(&self, track: TrackId, version: &Version) -> Result<bool> {
        self.inner
            .write(|transaction| chosen::choose(transaction, track, version, SystemTime::now()))
    }

    pub fn forget_the_version_chosen(&self, track: TrackId) -> Result<bool> {
        self.inner
            .write(|transaction| chosen::forget(transaction, track))
    }

    pub fn versions_chosen(&self) -> Result<Vec<ChosenVersion>> {
        self.inner.read(chosen::every)
    }

    pub fn versions_chosen_among(&self, tracks: &[TrackId]) -> Result<Vec<ChosenVersion>> {
        self.inner
            .read(|connection| chosen::among(connection, tracks, IDS_READ_AT_ONCE))
    }

    pub fn chosen_among(&self, tracks: &[TrackId]) -> Result<Chosen> {
        let asked: Vec<TrackId> = tracks
            .iter()
            .copied()
            .collect::<AHashSet<_>>()
            .into_iter()
            .collect();
        let versions = self.versions_chosen_among(&asked)?;
        if versions.is_empty() {
            return Ok(Chosen::default());
        }

        let wanted: Vec<TrackId> = versions
            .iter()
            .flat_map(|chosen| match chosen.version {
                Version::Held(copy) => [Some(chosen.track), Some(copy)],
                Version::Served(_) => [Some(chosen.track), None],
            })
            .flatten()
            .collect();
        let held = self.tracks_with_ids(&wanted)?;

        Ok(Chosen::of(versions, &held))
    }

    pub fn version_chosen_for(&self, track: TrackId) -> Result<Option<Version>> {
        self.inner.read(|connection| chosen::of(connection, track))
    }
}
