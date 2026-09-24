use std::sync::Arc;

use parking_lot::Mutex;
use resonate_core::PlaylistId;
use resonate_engine::{Command, Player, QueueItem, Unclaimed, stamp_of};
use resonate_library::{Direction, Library, Playing, PlaylistEntry};
use resonate_mpris::{Opened, PlaylistInfo, PlaylistOrder, Playlists};

pub(crate) struct Collection {
    library: Arc<Library>,
    player: Arc<Player>,
    named_playing: Mutex<Option<NamedAt>>,
}

#[derive(Clone)]
struct NamedAt {
    playlist: PlaylistId,
    revision: u64,
    info: Option<PlaylistInfo>,
}

impl Collection {
    pub(crate) fn new(library: &Arc<Library>, player: &Arc<Player>) -> Self {
        Self {
            library: Arc::clone(library),
            player: Arc::clone(player),
            named_playing: Mutex::new(None),
        }
    }

    fn named(&self, playlist: PlaylistId) -> Option<PlaylistInfo> {
        match self.library.playlist(playlist) {
            Ok(found) => found.map(|found| PlaylistInfo {
                id: found.id,
                name: found.name,
            }),
            Err(error) => {
                tracing::warn!(%error, %playlist, "a playlist could not be read");
                None
            }
        }
    }
}

impl Playlists for Collection {
    fn revision(&self) -> u64 {
        self.library.playlists_revision()
    }

    fn count(&self) -> usize {
        match self.library.playlist_count(None) {
            Ok(held) => held as usize,
            Err(error) => {
                tracing::warn!(%error, "the playlists could not be counted");
                0
            }
        }
    }

    fn listing(
        &self,
        order: PlaylistOrder,
        reverse: bool,
        from: usize,
        most: Option<usize>,
    ) -> Vec<PlaylistInfo> {
        let held =
            match self
                .library
                .playlist_names(ordering(order), asked_for(reverse), from, most)
            {
                Ok(held) => held,
                Err(error) => {
                    tracing::warn!(%error, "the playlists could not be read");
                    return Vec::new();
                }
            };

        held.into_iter()
            .map(|playlist| PlaylistInfo {
                id: playlist.id,
                name: playlist.name,
            })
            .collect()
    }

    fn playing(&self) -> Option<PlaylistInfo> {
        let queue = self.player.state().queue_stamp;
        let playlist = self.library.playing_playlist(queue)?;
        let revision = self.library.playlists_revision();
        if let Some(held) = self.named_playing.lock().as_ref()
            && held.playlist == playlist
            && held.revision == revision
        {
            return held.info.clone();
        }

        let info = self.named(playlist);
        *self.named_playing.lock() = Some(NamedAt {
            playlist,
            revision,
            info: info.clone(),
        });
        info
    }

    fn activate(&self, playlist: PlaylistId) -> Opened {
        let entries = match self.library.playlist_entries(playlist, None) {
            Ok(entries) => entries,
            Err(error) => {
                tracing::warn!(%error, %playlist, "a playlist from the session bus could not be read");
                return Opened::Refused;
            }
        };

        let items = queue_items(&entries);
        if items.is_empty() {
            return Opened::Refused;
        }
        let queue = stamp_of(&items);
        match self.player.send(Command::Load {
            items,
            start_at: 0,
            autoplay: true,
        }) {
            Ok(()) => {
                self.library
                    .set_playing_playlist(Some(Playing { playlist, queue }));
                Opened::Accepted
            }
            Err(error) => {
                tracing::warn!(%error, "a playlist from the session bus did not reach the engine");
                Opened::Refused
            }
        }
    }
}

pub(crate) fn queue_items(entries: &[PlaylistEntry]) -> Vec<QueueItem> {
    let mut minting = Unclaimed::beside(&[]);

    entries
        .iter()
        .map(|entry| QueueItem {
            id: entry
                .track
                .as_ref()
                .map_or_else(|| minting.mint(), |track| track.id),
            location: entry.location().clone(),
            span: entry.span(),
        })
        .collect()
}

const fn asked_for(reverse: bool) -> Direction {
    if reverse {
        Direction::Descending
    } else {
        Direction::Ascending
    }
}

const fn ordering(order: PlaylistOrder) -> resonate_library::PlaylistOrder {
    match order {
        PlaylistOrder::Alphabetical => resonate_library::PlaylistOrder::Name,
        PlaylistOrder::CreationDate => resonate_library::PlaylistOrder::Created,
        PlaylistOrder::ModifiedDate => resonate_library::PlaylistOrder::Modified,
        PlaylistOrder::LastPlayDate => resonate_library::PlaylistOrder::Played,
    }
}
