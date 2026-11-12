use std::hash::{Hash as _, Hasher as _};

use ahash::{AHashMap, AHasher};
use resonate_core::{AlbumId, ArtistId, TrackId};
use resonate_engine::QueueItem;
use resonate_library::{Album, Favoured, Track};

use super::{
    LibraryModel,
    arranged::{Named, ScannedPlaying},
};

impl LibraryModel {
    pub fn held_tracks(&self, ids: &[TrackId]) -> Vec<Track> {
        let mut read: AHashMap<TrackId, Track> = match self.library.tracks_with_ids(ids) {
            Ok(read) => read.into_iter().map(|track| (track.id, track)).collect(),
            Err(error) => {
                tracing::warn!(%error, "the tracks could not be read by id");
                AHashMap::new()
            }
        };

        ids.iter().filter_map(|id| read.remove(id)).collect()
    }

    #[cfg(test)]
    pub(crate) fn names_held(&self) -> usize {
        self.named.entries().count()
    }

    #[cfg(test)]
    pub(crate) fn is_drawing_covers(&self) -> bool {
        !self.decoding.is_empty()
    }

    pub fn track_of(&mut self, item: &QueueItem) -> Option<Track> {
        self.named_track(item).cloned()
    }

    fn named_track(&mut self, item: &QueueItem) -> Option<&Track> {
        let stands = self
            .named
            .get(&item.id)
            .is_some_and(|held| held.names(item));
        if !stands {
            let found = self.scanned(item);
            self.named.insert(
                item.id,
                Named {
                    location: item.location.clone(),
                    span: item.span,
                    track: found,
                },
            );
        }

        self.named.peek(&item.id)?.track.as_ref()
    }

    pub(crate) fn scanned_playing(&mut self, item: &QueueItem) -> Option<ScannedPlaying> {
        let track = self.named_track(item)?;
        let mut scanned = ScannedPlaying {
            id: track.id,
            title: track.title.clone(),
            artist: track.artist.clone(),
            artist_id: track.artist_id,
            album_id: track.album_id,
            album: None,
            codec: track.codec,
            plays: track.plays,
            played: track.played,
            added: track.added,
            favourite: false,
        };
        let stored = track.favourite.is_some();

        scanned.favourite = self.favours(Favoured::Track(scanned.id), stored);
        scanned.album = scanned.album_id.and_then(|album| self.album_title(album));
        Some(scanned)
    }

    pub(crate) fn scanned_playing_stamp(&mut self, item: &QueueItem) -> Option<u64> {
        let mut stamp = AHasher::default();
        let track = self.named_track(item)?;
        (
            track.id,
            &track.title,
            &track.artist,
            track.artist_id,
            track.codec,
            track.plays,
            track.played,
            track.added,
        )
            .hash(&mut stamp);
        let (id, album, stored) = (track.id, track.album_id, track.favourite.is_some());

        self.favours(Favoured::Track(id), stored).hash(&mut stamp);
        album.hash(&mut stamp);
        if let Some(album) = album {
            self.with_album(album, |album| album.title.hash(&mut stamp));
        }
        Some(stamp.finish())
    }

    pub(super) fn scanned(&self, item: &QueueItem) -> Option<Track> {
        match self.library.track(item.id) {
            Ok(Some(track)) if track.location == item.location => return Some(track),
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "a queued track could not be read by id"),
        }
        match self.library.track_at(item.location.as_path()?, item.span) {
            Ok(track) => track,
            Err(error) => {
                tracing::warn!(%error, "a queued track could not be read by path");
                None
            }
        }
    }

    pub fn album_title(&mut self, id: AlbumId) -> Option<String> {
        self.with_album(id, |album| album.title.clone())
    }

    pub(super) fn album_owner(&mut self, id: AlbumId) -> Option<ArtistId> {
        self.with_album(id, |album| album.artist_id).flatten()
    }

    fn with_album<T>(&mut self, id: AlbumId, from: impl FnOnce(&Album) -> T) -> Option<T> {
        if let Some(listed) = self.album_of(id) {
            return Some(from(listed));
        }
        if let Some(held) = self.read_albums.get(&id) {
            return held.as_ref().map(from);
        }
        let read = self.library.album(id).unwrap_or_else(|error| {
            tracing::warn!(%error, album = %id, "an album the listing does not hold could not be read");
            None
        });
        let answer = read.as_ref().map(from);
        self.read_albums.insert(id, read);
        answer
    }
}
