use std::sync::atomic::Ordering;

use resonate_core::{PlaylistId, QueueStamp};

use super::{CatalogStamp, Library, WrittenElsewhere};
use crate::{Playing, renamed::NamesMoved};

impl Library {
    pub fn synced_revision(&self) -> u64 {
        self.inner.arrivals()
    }

    pub fn catalog_revision(&self) -> u64 {
        let inner = &self.inner;
        inner
            .playlists_revision()
            .wrapping_add(inner.arrivals())
            .wrapping_add(inner.written.load(Ordering::Acquire))
            .wrapping_add(inner.named.load(Ordering::Acquire))
            .wrapping_add(
                inner
                    .written_elsewhere()
                    .unwrap_or_default()
                    .cast_unsigned(),
            )
    }

    pub fn playlists_revision(&self) -> u64 {
        self.inner.playlists_revision()
    }

    pub fn written_elsewhere(&self) -> Option<WrittenElsewhere> {
        self.inner.written_elsewhere().map(WrittenElsewhere)
    }

    pub fn plans_stamp(&self) -> CatalogStamp {
        self.inner.plans_stamp()
    }

    pub fn names_stamp(&self) -> CatalogStamp {
        self.inner.names_stamp()
    }

    pub fn rows_stamp(&self) -> CatalogStamp {
        self.inner.rows_stamp()
    }

    pub fn names_moved_since(&self, then: CatalogStamp) -> NamesMoved {
        let now = self.inner.names_stamp();
        self.inner.renamed.lock().since(then, now)
    }

    pub fn playing_playlist(&self, queue: QueueStamp) -> Option<PlaylistId> {
        self.inner
            .playing()
            .filter(|playing| playing.queue == queue)
            .map(|playing| playing.playlist)
    }

    pub fn set_playing_playlist(&self, playing: Option<Playing>) {
        self.inner.set_playing(playing);
    }
}
