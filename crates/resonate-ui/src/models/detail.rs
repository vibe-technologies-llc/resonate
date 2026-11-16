use std::sync::Arc;

use gpui::SharedString;
use resonate_core::{AlbumId, ReleaseTrackId, WantId};
use resonate_library::{
    Album, ArtistDetail, ArtistTotals, Found, HeldReleaseTrack, Mbid, ReleaseDetail,
};

use super::{LibraryModel, ListedRow, ShortOf};
use crate::downloads::{self, Fetched, Fetching};

impl LibraryModel {
    pub fn rows(&self) -> Arc<[ListedRow]> {
        Arc::clone(&self.browse.rows)
    }

    pub fn release_tracks(&self) -> Arc<[HeldReleaseTrack]> {
        Arc::clone(&self.detail.release_tracks)
    }

    pub const fn release(&self) -> Option<&ReleaseDetail> {
        self.detail.release.as_ref()
    }

    pub const fn artist_detail(&self) -> Option<&ArtistDetail> {
        self.detail.artist.as_ref()
    }

    pub fn artist_albums(&self) -> Arc<[Album]> {
        Arc::clone(&self.detail.artist_albums)
    }

    pub const fn artist_totals(&self) -> ArtistTotals {
        self.detail.artist_totals
    }

    pub fn scoped_album(&self, id: AlbumId) -> Option<&Album> {
        self.detail.album.as_ref().filter(|album| album.id == id)
    }

    pub fn wanted(&self, release_track: ReleaseTrackId) -> Option<WantId> {
        self.wishes.wanted.get(&release_track).copied()
    }

    pub fn recording_of(&self, release_track: ReleaseTrackId) -> Option<Mbid> {
        self.detail
            .release_tracks
            .iter()
            .find(|row| row.id == release_track)
            .and_then(|row| row.recording.clone())
    }

    pub(crate) fn told_want(&self, release_track: ReleaseTrackId) -> Option<SharedString> {
        let want = self.wanted(release_track)?;
        let fetching = self.fetching_want(release_track)?;
        Some(downloads::saying_while(
            fetching,
            self.fetched_for(want).as_ref(),
        ))
    }

    pub(crate) fn told_found(&self, found: &Found) -> Option<SharedString> {
        let download = self.delivered.downloads.of(&found.recording)?;
        Some(downloads::saying_while(
            self.fetching(download),
            self.fetched(download).as_ref(),
        ))
    }

    pub(super) fn fetched_for(&self, want: WantId) -> Option<Fetched> {
        let progress = self.running.work.polling()?;
        Some(Fetched {
            provider: progress.provider_of(want)?,
            opening: progress.is_opening(want),
            received: progress.received_for(want),
        })
    }

    pub fn fetching_want(&self, release_track: ReleaseTrackId) -> Option<Fetching> {
        let want = self.wanted(release_track)?;
        let fetching = self.wishes.standings.get(&want)?.fetching();

        Some(fetching.while_polling(Some(want), &self.polling()))
    }

    pub(crate) const fn take_what_is_short(&mut self) -> Option<ShortOf> {
        self.landed.short_of.take()
    }
}
