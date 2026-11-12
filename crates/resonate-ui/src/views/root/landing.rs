use gpui::{App, Context, ScrollStrategy, Window};
use resonate_core::{AlbumId, ArtistsDrawn};
use resonate_library::{Cut, Track};

use super::{Pane, RootView, listed, read_each};
use crate::{
    Selection,
    toast::{self},
    views::{
        favourites::Shelved,
        playlists::{PlaylistsDrawn, Rows},
        reorder::{Lift, Lifted, LiftedTo, Listed, Reach, Shift},
        search::{SearchShows, TopEntry, TopRun},
    },
    words,
};

impl RootView {
    pub(super) fn press_at_the_top(
        &mut self,
        row: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let library = self.library.read(cx);
        let Some(entry) = TopRun::of(library).at(row) else {
            return;
        };
        match entry {
            TopEntry::Artist(at) => {
                if let Some(artist) = library.artists().get(at).map(|held| held.id) {
                    self.opened(Selection::Artist(artist), cx);
                }
            }
            TopEntry::ArtistFound(at) => {
                if let Some(found) = library.artists_found().get(at).cloned() {
                    self.open_artist_found(found, cx);
                }
            }
            TopEntry::Song(at) => {
                if let Some((played, start)) = library.played_from(at) {
                    self.play_the_listing_from(&played, start, window, cx);
                }
            }
            TopEntry::Found(at) => {
                if let Some(found) = library.found().get(at).cloned() {
                    self.library
                        .update(cx, |library, cx| library.fetch_and_play(found, cx));
                }
            }
            TopEntry::Album(at) => {
                if let Some(album) = library.albums().get(at).map(|held| held.id) {
                    self.opened(Selection::Album(album), cx);
                }
            }
            TopEntry::AlbumFound(at) => {
                if let Some(group) = library
                    .albums_found()
                    .get(at)
                    .map(|found| found.group.clone())
                {
                    self.open_album_not_held(group, cx);
                }
            }
        }
    }

    pub(crate) fn lifted_into(&mut self, lifted: &Lifted, into: LiftedTo, cx: &mut Context<Self>) {
        match &lifted.lift {
            Lift::Tracks(run) => self.landed(&run.tracks(), into, cx),
            Lift::Album(album, _) => self.landed_albums(vec![*album], into, cx),
            Lift::Albums(run) => self.landed_albums(run.albums(), into, cx),
        }
    }

    pub(super) fn landed_albums(
        &mut self,
        albums: Vec<AlbumId>,
        into: LiftedTo,
        cx: &mut Context<Self>,
    ) {
        let library = self.library.read(cx).catalog();
        let scopes = albums.into_iter().map(Selection::Album).collect();

        cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { read_each(&library, scopes) })
                .await;
            let _ = this.update(cx, |this, cx| match read {
                Ok(tracks) => this.landed(&tracks, into, cx),
                Err(error) => {
                    tracing::error!(%error, "a dragged album could not be read");
                    toast::tell(toast::could_not(words::root::read_that_album(), &error), cx);
                }
            });
        })
        .detach();
    }

    pub(super) fn landed(&mut self, tracks: &[Track], into: LiftedTo, cx: &mut Context<Self>) {
        if tracks.is_empty() {
            return;
        }
        match into {
            LiftedTo::Queue(at) => self.queue(&listed(tracks), at, cx),
            LiftedTo::Playlist(playlist) => {
                let cuts = tracks.iter().map(Cut::of).collect();
                self.library.update(cx, |library, cx| {
                    library.add_to_playlist(playlist, cuts, cx)
                });
            }
            LiftedTo::PlaylistAt(playlist, at) => {
                let cuts = tracks.iter().map(Cut::of).collect();
                self.library.update(cx, |library, cx| {
                    library.insert_into_playlist(playlist, cuts, at, cx);
                });
            }
        }
    }

    pub(crate) fn reach_further(&mut self, drawn_to: usize, held: usize, cx: &mut Context<Self>) {
        self.library.update(cx, |library, cx| {
            library.reach_further(drawn_to, held, cx);
        });
    }

    pub(crate) fn opened_rows(&self, cx: &App) -> Rows {
        let library = self.library.read(cx);
        let narrowed = library.narrowing().is_some();

        library
            .opened_playlist()
            .map_or(Rows::InHand, |held| Rows::of(held, narrowed))
    }

    pub(super) fn movable(&self, cx: &App) -> Option<(Shift, usize)> {
        let reached = self.reachable(cx)?;
        match reached.0 {
            Shift::Queue => Some(reached),
            Shift::Playlist(_) => self.opened_rows(cx).are_moved().then_some(reached),
            Shift::Listing(_) => None,
        }
    }

    pub(super) fn reachable(&self, cx: &App) -> Option<(Shift, usize)> {
        match self.search_in_front(cx) {
            Some(SearchShows::Top) => {
                let rows = TopRun::of(self.library.read(cx)).len();
                return (rows > 0).then_some((Shift::Listing(Listed::Top), rows));
            }
            Some(SearchShows::Songs | SearchShows::Albums | SearchShows::Artists) | None => {}
        }

        match self.pane {
            Pane::Queue => {
                let rows = self.player.read(cx).queue().len();
                (rows > 0).then_some((Shift::Queue, rows))
            }
            Pane::Playlists => {
                let library = self.library.read(cx);
                let Some(opened) = library.opened() else {
                    let rows = library.playlists().len();
                    return (rows > 0).then_some((Shift::Listing(Listed::Playlists), rows));
                };
                let rows = library.entries().len();

                (self.opened_rows(cx).are_reached() && rows > 0)
                    .then_some((Shift::Playlist(opened), rows))
            }
            Pane::Tracks => {
                let rows = self.library.read(cx).listed_rows();

                (rows > 0).then_some((Shift::Listing(Listed::Tracks), rows))
            }
            Pane::Artists => {
                let rows = self.library.read(cx).artists_listed();

                (rows > 0).then_some((Shift::Listing(Listed::Artists), rows))
            }
            Pane::Albums => {
                let rows = self.library.read(cx).albums_listed();

                (rows > 0).then_some((Shift::Listing(Listed::Albums), rows))
            }
            Pane::Favourites => {
                let shelved = Shelved::of(self.library.read(cx));
                let reached = self.reach.as_ref().and_then(|reach| match reach.shift {
                    Shift::Listing(listed) if shelved.rows_of(listed) > 0 => Some(listed),
                    _ => None,
                });
                let listed = reached.or_else(|| shelved.opening())?;

                Some((Shift::Listing(listed), shelved.rows_of(listed)))
            }
            Pane::Missing => {
                let rows = self.missing_rows_shown(cx).len();

                (rows > 0).then_some((Shift::Listing(Listed::Missing), rows))
            }
            Pane::Suggestions => {
                let library = self.library.read(cx);
                let offered = library.suggestions();
                let Some(opened) = library
                    .opened_suggestion()
                    .filter(|opened| offered.iter().any(|held| held.query == opened.query))
                else {
                    let rows = offered.len();
                    return (rows > 0).then_some((Shift::Listing(Listed::Offered), rows));
                };
                let rows = opened.tracks.len();

                (rows > 0).then_some((Shift::Listing(Listed::Suggested), rows))
            }
            Pane::Statistics => {
                let rows = self.heard_rows(cx);

                (rows > 0).then_some((Shift::Listing(Listed::Heard), rows))
            }
            Pane::Party
            | Pane::Server
            | Pane::Lyrics
            | Pane::Inspector
            | Pane::Visualiser
            | Pane::Analysis
            | Pane::Settings => None,
        }
    }

    pub(super) fn reach_in(&self, shift: Shift, held: usize) -> Option<Reach> {
        self.reach
            .as_ref()
            .filter(|reach| reach.shift == shift)
            .and_then(|reach| reach.within(held))
    }

    pub(super) fn opening_row(&self, shift: Shift, rows: usize, cx: &App) -> usize {
        match shift {
            Shift::Queue => self
                .player
                .read(cx)
                .state()
                .queue_position
                .unwrap_or_default()
                .min(rows - 1),
            Shift::Playlist(_) | Shift::Listing(_) => 0,
        }
    }

    pub(super) fn show_row(&self, shift: Shift, row: usize, cx: &App) {
        match shift {
            Shift::Queue => self
                .scrolls
                .queue_rows
                .scroll_to_item(self.queue_parts(cx).line_of(row), ScrollStrategy::Center),
            Shift::Playlist(_) => self
                .scrolls
                .playlist_rows
                .scroll_to_item(row, ScrollStrategy::Center),
            Shift::Listing(Listed::Tracks) => {
                self.scrolls
                    .track_rows
                    .scroll_to_item(row, ScrollStrategy::Center);
            }
            Shift::Listing(Listed::Albums) => {
                self.scrolls
                    .album_rows
                    .scroll_to_item(row / self.grid_columns(), ScrollStrategy::Center);
            }
            Shift::Listing(Listed::Artists) => {
                let at = match self.shows.artists {
                    ArtistsDrawn::List => row,
                    ArtistsDrawn::Grid => row / self.grid_columns(),
                };
                self.scrolls
                    .artist_rows
                    .scroll_to_item(at, ScrollStrategy::Center);
            }
            Shift::Listing(Listed::Playlists) => {
                let at = match self.shows.playlists {
                    PlaylistsDrawn::List => row,
                    PlaylistsDrawn::Grid => row / self.grid_columns(),
                };
                self.scrolls
                    .all_playlist_rows
                    .scroll_to_item(at, ScrollStrategy::Center);
            }
            Shift::Listing(Listed::Favourites) => {
                self.scrolls
                    .favourite_rows
                    .scroll_to_item(row, ScrollStrategy::Center);
            }
            Shift::Listing(Listed::FavouriteArtists | Listed::FavouriteAlbums) => {
                self.rooms.reached_unseen.set(true);
            }
            Shift::Listing(Listed::Missing) => {
                self.scrolls
                    .missing_rows
                    .scroll_to_item(row, ScrollStrategy::Center);
            }
            Shift::Listing(Listed::Suggested) => {
                self.scrolls
                    .suggestion_rows
                    .scroll_to_item(row, ScrollStrategy::Center);
            }
            Shift::Listing(Listed::Top | Listed::Offered | Listed::Heard) => {
                self.rooms.reached_unseen.set(true);
            }
        }
    }
}
