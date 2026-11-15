use std::sync::Arc;

use gpui::Context;
use resonate_core::{AlbumId, ArtistId};
use resonate_library::{Album, Artist, Favoured, HiddenTracks, Statistics, Track, Window};

use super::{Change, Favourited, Heard, LibraryModel, Wanted};

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

    pub(crate) const fn take_the_counts(&mut self) -> Option<Statistics> {
        self.landed.counts.take()
    }

    pub(crate) const fn take_what_was_heard(&mut self) -> Option<Heard> {
        self.landed.heard.take()
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
