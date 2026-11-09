use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, SystemTime},
};

use resonate_core::{MediaLocation, TrackId};
use resonate_engine::{EngineConfig, Kin, KinSource, Kinship, QueueItem};
use resonate_library::{Library, Track};

const HEARD_LATELY_WITHIN: Duration = Duration::from_secs(7 * 24 * 60 * 60);

pub(crate) struct Catalogued(Arc<Library>);

impl Kinship for Catalogued {
    fn kin_of(&self, rows: &[Arc<QueueItem>]) -> Vec<Option<Kin>> {
        let ids: Vec<TrackId> = rows.iter().map(|row| row.id).collect();
        let tracks = match self.0.tracks_with_ids(&ids) {
            Ok(tracks) => tracks,
            Err(error) => {
                tracing::warn!(%error, "the queue's artists and albums were not read");
                return vec![None; rows.len()];
            }
        };
        let now = SystemTime::now();

        let held: HashMap<TrackId, (MediaLocation, Kin)> = tracks
            .into_iter()
            .map(|track| (track.id, (track.location.clone(), kin_of(&track, now))))
            .collect();
        rows.iter()
            .map(|row| {
                held.get(&row.id)
                    .filter(|(location, _)| *location == row.location)
                    .map(|(_, kin)| *kin)
            })
            .collect()
    }
}

fn kin_of(track: &Track, now: SystemTime) -> Kin {
    Kin {
        artist: track.artist_id.map(|artist| artist.get()),
        album: track.album_id.map(|album| album.get()),
        heard_lately: track.played.is_some_and(|played| {
            now.duration_since(played)
                .is_ok_and(|since| since < HEARD_LATELY_WITHIN)
        }),
    }
}

pub(crate) fn kin_from(mut config: EngineConfig, library: Option<&Arc<Library>>) -> EngineConfig {
    if let Some(library) = library {
        config.kinship = KinSource::by(Arc::new(Catalogued(Arc::clone(library))));
    }
    config
}
