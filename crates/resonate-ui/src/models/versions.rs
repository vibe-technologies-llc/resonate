use std::sync::Arc;

use gpui::{App, Context};
use resonate_core::{AlbumId, ArtistId, TrackId};
use resonate_engine::QueueItem;
use resonate_library::Track;

use super::{Change, Edited, LibraryModel, Wanted, arranged::Named};
use crate::words;

impl LibraryModel {
    pub fn remember(&mut self, tracks: &[Track]) {
        self.revision = self.revision.wrapping_add(1);
        for track in tracks {
            self.naming.named.insert(track.id, Named::of(track));
        }
    }

    pub fn copies_held(&self, id: TrackId) -> Option<Arc<[Track]>> {
        self.versions
            .copies
            .as_ref()
            .filter(|(held, _)| *held == id)
            .map(|(_, copies)| Arc::clone(copies))
    }

    pub fn keep_the_copies(&mut self, id: TrackId, copies: Vec<Track>) {
        self.versions.copies = Some((id, copies.into()));
    }

    pub fn forget_the_copies(&mut self) {
        self.versions.copies = None;
    }

    pub fn choose_version(
        &mut self,
        track: TrackId,
        version: resonate_library::Version,
        title: String,
        cx: &mut Context<Self>,
    ) {
        self.versions.touched = true;
        self.edited_then(
            Wanted::ThePage,
            Change::ChooseVersion,
            move |library| {
                library
                    .choose_version(track, &version)
                    .map(|chosen| chosen.then(|| words::better::chosen(title.as_str())))
            },
            move |this, edited, cx| {
                if edited == Edited::Landed {
                    this.versions.chosen.insert(track);
                }
                this.read_the_versions_chosen(cx);
            },
            cx,
        );
    }

    pub fn forget_the_version_chosen(
        &mut self,
        track: TrackId,
        title: String,
        then: impl FnOnce(&mut App) + 'static,
        cx: &mut Context<Self>,
    ) {
        self.versions.touched = true;
        self.edited_then(
            Wanted::ThePage,
            Change::ForgetTheVersionChosen,
            move |library| {
                library.forget_the_version_chosen(track).map(|forgotten| {
                    forgotten.then(|| words::better::own_copy_again(title.as_str()))
                })
            },
            move |this, edited, cx| {
                if edited == Edited::Landed {
                    this.versions.chosen.remove(&track);
                }
                this.read_the_versions_chosen(cx);
                cx.defer(then);
            },
            cx,
        );
    }

    pub fn has_a_version_chosen(&self, track: TrackId) -> bool {
        if self.versions.read {
            return self.versions.chosen.contains(&track);
        }
        self.library
            .version_chosen_for(track)
            .inspect_err(|error| tracing::warn!(%error, "the version chosen could not be read"))
            .ok()
            .flatten()
            .is_some()
    }

    pub(crate) fn chosen_among(&self, items: &[QueueItem]) -> resonate_library::Chosen {
        let queued: Vec<TrackId> = items
            .iter()
            .map(|item| item.id)
            .filter(|id| !self.versions.read || self.versions.chosen.contains(id))
            .collect();
        if queued.is_empty() {
            return resonate_library::Chosen::default();
        }

        self.library
            .chosen_among(&queued)
            .inspect_err(|error| tracing::warn!(%error, "the versions chosen could not be read"))
            .unwrap_or_default()
    }

    pub(super) fn read_the_versions_chosen(&mut self, cx: &mut Context<Self>) {
        self.versions.touched = false;
        let library = Arc::clone(&self.library);
        self.versions._reading = cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { library.versions_chosen() })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.versions.touched {
                    this.read_the_versions_chosen(cx);
                    return;
                }
                match read {
                    Ok(rows) => {
                        this.versions.chosen = rows.iter().map(|row| row.track).collect();
                        this.versions.read = true;
                    }
                    Err(error) => {
                        tracing::warn!(%error, "the versions chosen could not be read");
                    }
                }
            });
        });
    }

    pub fn standing_of(
        &self,
        track: Option<TrackId>,
    ) -> Option<(Option<AlbumId>, Option<ArtistId>)> {
        match self.library.track(track?) {
            Ok(held) => held.map(|held| (held.album_id, held.artist_id)),
            Err(error) => {
                tracing::warn!(%error, "where a track stands now could not be read");
                None
            }
        }
    }
}
