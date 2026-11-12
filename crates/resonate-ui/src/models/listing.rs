use std::{path::PathBuf, sync::Arc, time::Duration};

use gpui::Context;
use resonate_core::{AlbumId, ArtistId, PlaylistId};
use resonate_library::{
    Album, AlbumOrder, Artist, ArtistOrder, Direction, Library, Meant, Measured, NamedPlaylist,
    Playlist, PlaylistEntry, PlaylistOrder, SavedQuery, Search, SortOrder, Track, TrackQuery,
};

use super::{
    ASKED_ELSEWHERE_AFTER, LOOK_AHEAD, LibraryModel, PAGE, SEARCH_SETTLE, Selection, Sorting,
    Wanted,
    arranged::{Arranging, AsDrawn, held_at, held_in},
    watched::reach_covering,
};
use crate::held::Rows;

impl LibraryModel {
    pub(crate) const fn listed(&self) -> Measured {
        match self.selection {
            Selection::Everything => self.browse.tracks_measured,
            Selection::Album(_) | Selection::Artist(_) => self.browse.scoped_measured,
        }
    }

    pub(crate) const fn unreadable(&self) -> bool {
        self.unreadable
    }

    pub(crate) const fn albums_counted(&self) -> u32 {
        self.browse.albums_counted
    }

    pub(crate) const fn artists_counted(&self) -> u32 {
        self.browse.artists_counted
    }

    pub(crate) const fn tracks_counted(&self) -> u32 {
        self.browse.tracks_measured.rows
    }

    pub(crate) fn catalog(&self) -> Arc<Library> {
        Arc::clone(&self.library)
    }

    pub(crate) fn listing_whole(&self) -> (Arc<Library>, TrackQuery) {
        let (album, artist) = match self.selection {
            Selection::Everything => (None, None),
            Selection::Album(album) => (Some(album), None),
            Selection::Artist(artist) => (None, Some(artist)),
        };

        (
            Arc::clone(&self.library),
            TrackQuery {
                album,
                artist,
                text: (!self.query.is_empty()).then(|| self.query.clone()),
                sort: self.sorting.tracks,
                reading: self.sorting.tracks_read,
                limit: None,
                offset: 0,
            },
        )
    }

    pub(crate) fn whole_library(&self) -> (Arc<Library>, TrackQuery) {
        (
            Arc::clone(&self.library),
            TrackQuery {
                album: None,
                artist: None,
                text: None,
                sort: self.sorting.tracks,
                reading: self.sorting.tracks_read,
                limit: None,
                offset: 0,
            },
        )
    }

    pub(crate) fn is_searching(&self) -> bool {
        !self.query.is_empty()
    }

    pub(crate) fn is_searching_the_whole_library(&self) -> bool {
        self.is_searching() && self.selection == Selection::Everything
    }

    pub(crate) fn rows_of(&self, query: &SavedQuery) -> (Arc<Library>, TrackQuery) {
        (Arc::clone(&self.library), TrackQuery::from(query))
    }

    pub(crate) fn reach_further(&mut self, drawn_to: usize, held: usize, cx: &mut Context<Self>) {
        if held < self.browse.reach || drawn_to + LOOK_AHEAD < held {
            return;
        }
        self.browse.reach = reach_covering(self.browse.reach, drawn_to);
        self.read(Wanted::ThePage, cx);
    }

    pub(crate) fn is_the_whole_listing(&self, played: &[Track]) -> bool {
        played.len() as u64 >= u64::from(self.listed().rows)
    }

    pub fn played_from(&self, row: usize) -> Option<(Arc<[Track]>, usize)> {
        if self.browse.rows.is_empty() {
            return Some((self.listing().whole(), row));
        }
        self.played_from_held(held_at(&self.browse.rows, row)?)
    }

    pub fn played_from_held(&self, held: usize) -> Option<(Arc<[Track]>, usize)> {
        let listing = self.listing();
        if self.browse.rows.is_empty() {
            return Some((listing.whole(), held));
        }
        let order = held_in(&self.browse.rows);
        let start = order.iter().position(|at| *at == held)?;
        let played = order
            .iter()
            .filter_map(|at| listing.get(*at).cloned())
            .collect();

        Some((played, start))
    }

    pub(crate) fn as_drawn(&self) -> AsDrawn {
        match self.selection {
            Selection::Album(_) => AsDrawn::Album {
                release_tracks: Arc::clone(&self.detail.release_tracks),
                arranging: self.arranging(),
            },
            Selection::Everything | Selection::Artist(_) => AsDrawn::Listed,
        }
    }

    pub(super) const fn arranging(&self) -> Arranging {
        Arranging {
            sort: self.sorting.tracks,
            reading: self.sorting.tracks_read,
        }
    }

    pub fn listed_rows(&self) -> usize {
        match self.browse.rows.is_empty() {
            true => self.listing().len().max(self.listed().rows as usize),
            false => self.browse.rows.len(),
        }
    }

    pub fn tracks_at(&self, rows: impl IntoIterator<Item = usize>) -> Vec<Track> {
        let listing = self.listing();

        rows.into_iter()
            .filter_map(|row| {
                let held = if self.browse.rows.is_empty() {
                    row
                } else {
                    held_at(&self.browse.rows, row)?
                };
                listing.get(held).cloned()
            })
            .collect()
    }

    pub fn albums_at(&self, rows: impl IntoIterator<Item = usize>) -> Vec<AlbumId> {
        rows.into_iter()
            .filter_map(|row| self.browse.albums.get(row).map(|album| album.id))
            .collect()
    }

    pub fn playlists_at(&self, rows: impl IntoIterator<Item = usize>) -> Vec<PlaylistId> {
        rows.into_iter()
            .filter_map(|row| self.shelf.playlists.get(row).map(|playlist| playlist.id))
            .collect()
    }

    pub fn artists_at(&self, rows: impl IntoIterator<Item = usize>) -> Vec<ArtistId> {
        rows.into_iter()
            .filter_map(|row| self.browse.artists.get(row).map(|artist| artist.id))
            .collect()
    }

    pub(crate) fn albums_listed(&self) -> usize {
        self.browse
            .albums
            .len()
            .max(self.browse.albums_counted as usize)
    }

    pub(crate) fn artists_listed(&self) -> usize {
        self.browse
            .artists
            .len()
            .max(self.browse.artists_counted as usize)
    }

    pub fn albums(&self) -> Rows<Album> {
        self.browse.albums.clone()
    }

    pub fn artists(&self) -> Rows<Artist> {
        self.browse.artists.clone()
    }

    pub fn tracks(&self) -> Rows<Track> {
        self.browse.tracks.clone()
    }

    pub fn listing(&self) -> Rows<Track> {
        match self.selection {
            Selection::Everything => self.browse.tracks.clone(),
            Selection::Album(_) | Selection::Artist(_) => self.browse.scoped.clone(),
        }
    }

    pub fn album_of(&self, id: AlbumId) -> Option<&Album> {
        self.scoped_album(id).or_else(|| {
            self.browse
                .album_at
                .get(&id)
                .and_then(|at| self.browse.albums.get(*at))
        })
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    pub fn playlists(&self) -> &[Playlist] {
        &self.shelf.playlists
    }

    pub fn saved_playlists(&self) -> Arc<[Playlist]> {
        Arc::clone(&self.shelf.playlists)
    }

    pub fn pinned(&self) -> Arc<[NamedPlaylist]> {
        Arc::clone(&self.shelf.pinned)
    }

    pub fn pictured(&self, id: PlaylistId) -> Arc<[AlbumId]> {
        self.shelf
            .pictured
            .get(&id)
            .map_or_else(Arc::default, Arc::clone)
    }

    pub fn lists(&self) -> Arc<[Playlist]> {
        Arc::clone(&self.shelf.lists)
    }

    pub fn entries(&self) -> Arc<[PlaylistEntry]> {
        Arc::clone(&self.shelf.entries)
    }

    pub const fn opened(&self) -> Option<PlaylistId> {
        self.shelf.opened
    }

    pub const fn playlist_order(&self) -> PlaylistOrder {
        self.shelf.order
    }

    pub const fn playlist_reading(&self) -> Direction {
        self.shelf.reading
    }

    pub fn order_playlists(&mut self, order: PlaylistOrder, cx: &mut Context<Self>) {
        if self.shelf.order == order {
            return;
        }
        self.shelf.order = order;
        self.shelf.reading = order.reads();
        self.reload_playlists(cx);
    }

    pub fn read_playlists(&mut self, reading: Direction, cx: &mut Context<Self>) {
        if self.shelf.reading == reading {
            return;
        }
        self.shelf.reading = reading;
        self.reload_playlists(cx);
    }

    pub const fn sorting(&self) -> Sorting {
        self.sorting
    }

    pub fn order_tracks(&mut self, sort: SortOrder, cx: &mut Context<Self>) {
        if self.sorting.tracks == sort {
            return;
        }
        self.sorting.tracks = sort;
        self.sorting.tracks_read = sort.reads();
        self.read_listings_again(cx);
    }

    pub fn read_tracks(&mut self, reading: Direction, cx: &mut Context<Self>) {
        if self.sorting.tracks_read == reading {
            return;
        }
        self.sorting.tracks_read = reading;
        self.read_listings_again(cx);
    }

    pub fn order_favourites(&mut self, sort: SortOrder, cx: &mut Context<Self>) {
        if self.sorting.favourites == sort {
            return;
        }
        self.sorting.favourites = sort;
        self.sorting.favourites_read = sort.reads();
        self.read_listings_again(cx);
    }

    pub fn read_favourites(&mut self, reading: Direction, cx: &mut Context<Self>) {
        if self.sorting.favourites_read == reading {
            return;
        }
        self.sorting.favourites_read = reading;
        self.read_listings_again(cx);
    }

    pub fn order_albums(&mut self, sort: AlbumOrder, cx: &mut Context<Self>) {
        if self.sorting.albums == sort {
            return;
        }
        self.sorting.albums = sort;
        self.sorting.albums_read = sort.reads();
        self.read_listings_again(cx);
    }

    pub fn read_albums(&mut self, reading: Direction, cx: &mut Context<Self>) {
        if self.sorting.albums_read == reading {
            return;
        }
        self.sorting.albums_read = reading;
        self.read_listings_again(cx);
    }

    pub fn order_artists(&mut self, sort: ArtistOrder, cx: &mut Context<Self>) {
        if self.sorting.artists == sort {
            return;
        }
        self.sorting.artists = sort;
        self.sorting.artists_read = sort.reads();
        self.read_listings_again(cx);
    }

    pub fn read_artists(&mut self, reading: Direction, cx: &mut Context<Self>) {
        if self.sorting.artists_read == reading {
            return;
        }
        self.sorting.artists_read = reading;
        self.read_listings_again(cx);
    }

    fn read_listings_again(&mut self, cx: &mut Context<Self>) {
        self.browse.reach = PAGE;
        self.read(Wanted::TheSearch, cx);
    }

    pub fn opened_playlist(&self) -> Option<&Playlist> {
        self.shelf.held.as_ref()
    }

    pub fn saved_playlist(&self, id: PlaylistId) -> Option<&Playlist> {
        self.shelf
            .held
            .as_ref()
            .filter(|held| held.id == id)
            .or_else(|| self.shelf.playlists.iter().find(|held| held.id == id))
    }

    pub fn narrowing(&self) -> Option<&str> {
        (!self.query.is_empty()).then_some(self.query.as_str())
    }

    pub const fn meant(&self) -> Option<&Meant> {
        self.outreach.meant.as_ref()
    }

    pub fn search_as_typed(&mut self, cx: &mut Context<Self>) {
        self.outreach.as_typed = true;
        self.outreach.meant = None;
        self.search = Search::read(&self.query);
        self.read_after(Duration::ZERO, Wanted::TheSearch, cx);
        cx.notify();
    }

    pub fn instead(&self) -> Option<&str> {
        self.instead.as_deref()
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub const fn search(&self) -> &Search {
        &self.search
    }

    pub const fn selection(&self) -> Selection {
        self.selection
    }

    pub fn set_query(&mut self, query: String, cx: &mut Context<Self>) {
        if self.query == query {
            return;
        }
        self.outreach.as_typed = false;
        self.search = Search::read(&query);
        self.query = query;
        self.browse.reach = PAGE;
        self.read_after(SEARCH_SETTLE, Wanted::TheSearch, cx);
        self.ask_elsewhere_after(ASKED_ELSEWHERE_AFTER, cx);
        self.restate_the_listing();
    }

    pub fn select(&mut self, selection: Selection, cx: &mut Context<Self>) {
        if self.selection == selection {
            return;
        }
        match selection {
            Selection::Everything => {}
            Selection::Album(album) => {
                let owner = self.album_owner(album);
                self.ask_about(Some(album), owner);
            }
            Selection::Artist(artist) => {
                self.ask_about(None, Some(artist));
                self.learn_the_songs_of(artist, cx);
            }
        }
        self.selection = selection;
        self.unheld.songs = Arc::default();
        self.unheld.albums = Arc::default();
        self.ask_elsewhere_after(Duration::ZERO, cx);
        self.restate_the_listing();
        self.browse.reach = PAGE;
        self.read(Wanted::TheSearch, cx);
    }
}
