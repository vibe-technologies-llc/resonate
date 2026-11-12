use gpui::{App, Context};
use resonate_core::{AlbumId, ArtistId, TrackId};
use resonate_engine::QueueItem;
use resonate_library::Track;

use super::{Change, LibraryModel, Wanted, arranged::Named};
use crate::words;

impl LibraryModel {
    pub fn remember(&mut self, tracks: &[Track]) {
        self.revision = self.revision.wrapping_add(1);
        for track in tracks {
            self.named.insert(track.id, Named::of(track));
        }
    }

    pub fn alternatives_of(&self, id: TrackId) -> Vec<Track> {
        self.library.alternatives_of(id).unwrap_or_else(|error| {
            tracing::warn!(%error, "the other copies of a track could not be read");
            Vec::new()
        })
    }

    pub fn tracks_named(&self, words: &str, at_most: usize) -> Vec<Track> {
        let query = resonate_library::TrackQuery {
            text: Some(words.to_owned()),
            limit: Some(at_most),
            ..Default::default()
        };
        self.library.tracks(&query).unwrap_or_else(|error| {
            tracing::warn!(%error, "the songs named alike could not be read");
            Vec::new()
        })
    }

    pub fn choose_version(
        &mut self,
        track: TrackId,
        version: resonate_library::Version,
        title: String,
        cx: &mut Context<Self>,
    ) {
        self.edited(
            Wanted::ThePage,
            Change::ChooseVersion,
            move |library| {
                library
                    .choose_version(track, &version)
                    .map(|chosen| chosen.then(|| words::better::chosen(title.as_str())))
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
        self.edited_then(
            Wanted::ThePage,
            Change::ForgetTheVersionChosen,
            move |library| {
                library.forget_the_version_chosen(track).map(|forgotten| {
                    forgotten.then(|| words::better::own_copy_again(title.as_str()))
                })
            },
            move |_, _, cx| cx.defer(then),
            cx,
        );
    }

    pub fn has_a_version_chosen(&self, track: TrackId) -> bool {
        self.library
            .version_chosen_for(track)
            .inspect_err(|error| tracing::warn!(%error, "the version chosen could not be read"))
            .ok()
            .flatten()
            .is_some()
    }

    pub(crate) fn chosen_among(&self, items: &[QueueItem]) -> resonate_library::Chosen {
        let queued: Vec<TrackId> = items.iter().map(|item| item.id).collect();

        self.library
            .chosen_among(&queued)
            .inspect_err(|error| tracing::warn!(%error, "the versions chosen could not be read"))
            .unwrap_or_default()
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
