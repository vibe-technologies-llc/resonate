use std::sync::Arc;

use gpui::Context;
use resonate_core::{AlbumId, ArtistId};
use resonate_library::{
    Album, Artist, Day, Favoured, Habits, HiddenTracks, MostListened, Statistics, Suggestion,
    Track, Window,
};

use super::{Change, Favourited, LibraryModel, Wanted};
use crate::views::statistics::Chart;

impl LibraryModel {
    pub fn favourite_albums(&self) -> Arc<[Album]> {
        Arc::clone(&self.favours.albums)
    }

    pub fn favourite_artists(&self) -> Arc<[Artist]> {
        Arc::clone(&self.favours.artists)
    }

    pub fn favourite_tracks(&self) -> Arc<[Track]> {
        Arc::clone(&self.favours.tracks)
    }

    pub fn favours(&self, what: Favoured, as_read: bool) -> bool {
        self.favours.favoured.get(&what).copied().unwrap_or(as_read)
    }

    pub fn favours_track(&self, track: &Track) -> bool {
        self.favours(Favoured::Track(track.id), track.favourite.is_some())
    }

    pub fn favoured_album(&self, id: AlbumId) -> bool {
        self.favours(Favoured::Album(id), self.favours.album_ids.contains(&id))
    }

    pub fn favoured_artist(&self, id: ArtistId) -> bool {
        self.favours(Favoured::Artist(id), self.favours.artist_ids.contains(&id))
    }

    pub fn favourited(&self) -> Favourited {
        Favourited {
            artists: self.favours.artists.len(),
            albums: self.favours.albums.len(),
            tracks: self.favours.tracks.len(),
        }
    }

    pub const fn statistics(&self) -> Statistics {
        self.charts.statistics
    }

    pub fn most_listened(&self) -> Arc<MostListened> {
        Arc::clone(&self.charts.most_listened)
    }

    pub fn listening_by_day(&self) -> Arc<[Day]> {
        Arc::clone(&self.charts.by_day)
    }

    pub fn habits(&self) -> Arc<Habits> {
        Arc::clone(&self.charts.habits)
    }

    pub(crate) fn charted(&self) -> Arc<Chart> {
        Arc::clone(&self.charts.charted)
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub fn hidden(&self) -> Arc<HiddenTracks> {
        Arc::clone(&self.hidden)
    }

    pub fn show_every_hidden_track(&mut self, cx: &mut Context<Self>) {
        self.edited(
            Wanted::Everything,
            Change::Hide { hidden: false },
            |library| library.show_every_hidden_track().map(|_| None),
            cx,
        );
    }

    pub fn suggestions(&self) -> Arc<[Suggestion]> {
        Arc::clone(&self.suggestions)
    }

    pub const fn window(&self) -> Window {
        self.window
    }

    pub fn read_over(&mut self, window: Window, cx: &mut Context<Self>) {
        if self.window == window {
            return;
        }
        self.window = window;
        self.reload(cx);
    }
}
