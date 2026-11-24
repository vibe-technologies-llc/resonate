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
            .wrapping_add(inner.written_elsewhere_or_last_seen().cast_unsigned())
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

#[cfg(test)]
mod tests {
    use std::{env, fs, process};

    use crate::{Error, Library, StoreOp};

    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(named: &str) -> Self {
            let path = env::temp_dir().join(format!("resonate-stamps-{}-{named}", process::id()));
            let _ = fs::remove_dir_all(&path);

            fs::create_dir_all(&path).expect("a writable temporary directory");
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn rooted(library: &Library, path: &str) {
        library
            .inner
            .write(|transaction| {
                transaction
                    .execute("INSERT INTO roots (path) VALUES (?1)", [path])
                    .map_err(|source| Error::store(StoreOp::Insert, source))
            })
            .expect("a root written");
    }

    #[test]
    fn a_revision_read_while_the_writer_is_busy_keeps_the_last_external_part_it_saw() {
        let scratch = Scratch::new("busy");
        let path = scratch.0.join("catalog.db");
        let window = Library::open(&path).expect("a catalog on disc");
        let other = Library::open(&path).expect("the same catalog opened again");

        rooted(&other, "/music");

        let settled = window.catalog_revision();

        assert_eq!(window.catalog_revision(), settled);

        let held = window.inner.writer.lock();

        assert_eq!(window.catalog_revision(), settled);

        drop(held);

        assert_eq!(
            window.catalog_revision(),
            settled,
            "an idle writer after a busy one moved a revision nothing had changed"
        );
    }

    #[test]
    fn a_commit_by_another_connection_moves_the_revision_once_the_writer_is_idle_to_observe_it() {
        let scratch = Scratch::new("elsewhere");
        let path = scratch.0.join("catalog.db");
        let window = Library::open(&path).expect("a catalog on disc");
        let other = Library::open(&path).expect("the same catalog opened again");

        let settled = window.catalog_revision();

        let held = window.inner.writer.lock();

        rooted(&other, "/music");

        assert_eq!(window.catalog_revision(), settled);

        drop(held);

        assert_ne!(
            window.catalog_revision(),
            settled,
            "a write another connection committed went unseen"
        );
    }
}
