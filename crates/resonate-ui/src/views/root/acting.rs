use std::sync::Arc;

use gpui::{Context, Window};
use resonate_core::{Span, TrackId};
use resonate_engine::{Command, Placement};
use resonate_library::{SavedQuery, Track};

use super::{RootView, listed, read_each};
use crate::{
    Selection,
    toast::{self},
    views::{
        queue::took_out,
        reorder::{Chosen, Listed, Reach, Shift, Step},
    },
    words,
};

impl RootView {
    pub(super) fn move_reached_rows(&mut self, step: Step, cx: &mut Context<Self>) {
        if self.something_stands_over_the_pane() {
            return;
        }
        let Some((shift, held)) = self.movable(cx) else {
            return;
        };
        let Some(reach) = self.reach_in(shift, held) else {
            return;
        };
        let chosen = reach.acted_on();
        let Some(to) = chosen
            .bounds()
            .and_then(|bounds| step.landing(bounds, held))
        else {
            return;
        };
        let landed = chosen.gathered_by(to, held);

        self.shift_chosen(shift, &chosen, to, cx);
        match (chosen.single(), landed) {
            (Some(rows), _) if rows == reach.rows() => self.reach = Some(reach.stepped(step)),
            (Some(_), Some(landed)) => {
                self.reach = Some(Reach::spanning(shift, landed.first(), landed.last()));
            }
            _ => {}
        }
        self.landed_by_a_jump = None;
        self.show_row(shift, to, cx);
        cx.notify();
    }

    pub(super) fn play_reached_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.press_the_menu(window, cx) || self.something_stands_over_the_pane() {
            return;
        }

        let Some((shift, held)) = self.reachable(cx) else {
            return;
        };
        let Some(reach) = self.reach_in(shift, held) else {
            return;
        };
        let chosen = self.choosable(shift, reach.selected(), cx);
        let row = if shift.chooses_apart() {
            chosen.first().unwrap_or(reach.row)
        } else {
            reach.rows().first()
        };

        if shift.plays_the_chosen_rows() && chosen.count() > 1 {
            self.play_the_chosen(shift, &chosen, window, cx);
            return;
        }

        match shift {
            Shift::Queue => self.send_by_position(Command::JumpTo(row), cx),
            Shift::Playlist(playlist) => {
                let entries = self.library.read(cx).entries().to_vec();
                self.play_playlist(playlist, &entries, row, true, cx);
            }
            Shift::Listing(Listed::Tracks) => {
                if let Some(found) = self.library.read(cx).found_at(row) {
                    self.library
                        .update(cx, |library, cx| library.fetch_and_play(found, cx));
                    return;
                }
                let Some((played, start)) = self.library.read(cx).played_from(row) else {
                    return;
                };
                self.play_the_listing_from(&played, start, window, cx);
            }
            Shift::Listing(Listed::Albums) => {
                let Some(album) = self.library.read(cx).albums().get(row).map(|held| held.id)
                else {
                    return;
                };
                self.opened(Selection::Album(album), cx);
            }
            Shift::Listing(Listed::Artists) => {
                let Some(artist) = self.library.read(cx).artists().get(row).map(|held| held.id)
                else {
                    return;
                };
                self.opened(Selection::Artist(artist), cx);
            }
            Shift::Listing(Listed::Playlists) => {
                let Some(playlist) = self
                    .library
                    .read(cx)
                    .playlists()
                    .get(row)
                    .map(|held| held.id)
                else {
                    return;
                };
                self.show_playlist(Some(playlist), cx);
            }
            Shift::Listing(Listed::Favourites) => {
                let tracks = self.library.read(cx).favourite_tracks();
                self.play(&tracks, row, cx);
            }
            Shift::Listing(Listed::FavouriteArtists) => {
                let Some(artist) = self
                    .library
                    .read(cx)
                    .favourite_artists()
                    .get(row)
                    .map(|held| held.id)
                else {
                    return;
                };
                self.opened(Selection::Artist(artist), cx);
            }
            Shift::Listing(Listed::FavouriteAlbums) => {
                let Some(album) = self
                    .library
                    .read(cx)
                    .favourite_albums()
                    .get(row)
                    .map(|held| held.id)
                else {
                    return;
                };
                self.opened(Selection::Album(album), cx);
            }
            Shift::Listing(Listed::Missing) => self.open_what_is_missing_at(row, cx),
            Shift::Listing(Listed::Top) => self.press_at_the_top(row, window, cx),
            Shift::Listing(Listed::Suggested) => {
                let Some(tracks) = self
                    .library
                    .read(cx)
                    .opened_suggestion()
                    .map(|opened| Arc::clone(&opened.tracks))
                else {
                    return;
                };
                self.play(&tracks, row, cx);
            }
            Shift::Listing(Listed::Offered) => {
                let Some(query) = self.offered_in_shelf_order(cx).into_iter().nth(row) else {
                    return;
                };
                self.open_offered(query, cx);
            }
            Shift::Listing(Listed::Heard) => self.open_what_was_heard_at(row, cx),
        }
    }

    pub(super) fn play_the_chosen(
        &mut self,
        shift: Shift,
        chosen: &Chosen,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match shift {
            Shift::Listing(Listed::Albums | Listed::FavouriteAlbums) => {
                let albums = self.albums_chosen(shift, chosen, cx);
                self.plays_everything_in_each(
                    albums.into_iter().map(Selection::Album).collect(),
                    Placement::Queued,
                    true,
                    window,
                    cx,
                );
            }
            Shift::Listing(Listed::Artists | Listed::FavouriteArtists) => {
                let artists = self.artists_chosen(shift, chosen, cx);
                self.plays_everything_in_each(
                    artists.into_iter().map(Selection::Artist).collect(),
                    Placement::Queued,
                    true,
                    window,
                    cx,
                );
            }
            Shift::Listing(Listed::Playlists) => {
                let playlists = self.library.read(cx).playlists_at(chosen.rows());
                self.queue_the_playlists(playlists, Placement::Queued, true, cx);
            }
            _ => {
                let tracks = self.tracks_chosen(shift, chosen, cx);
                self.play(&tracks, 0, cx);
            }
        }
    }

    pub(crate) fn drop_queue_runs(&mut self, runs: Vec<Span>, cx: &mut Context<Self>) {
        let queued = self.player.read(cx).queue();
        let kept = self.took_out.keeping_all(&queued, &runs);

        self.reach = Reach::after_dropping_runs(Shift::Queue, &runs, queued.len());
        self.landed_by_a_jump = None;

        self.send_by_position(Command::RemoveAll(runs), cx);
        if let Some(kept) = kept {
            toast::tell(took_out(kept), cx);
        }
        cx.notify();
    }

    pub(crate) fn drop_chosen(&mut self, shift: Shift, chosen: &Chosen, cx: &mut Context<Self>) {
        if let Some(rows) = chosen.single() {
            self.drop_rows(shift, rows, cx);
            return;
        }

        match shift {
            Shift::Queue => self.drop_queue_runs(chosen.runs().to_vec(), cx),
            Shift::Playlist(playlist) => {
                let held = self.library.read(cx).entries().len();
                let runs = chosen.runs().to_vec();
                self.reach = Reach::after_dropping_runs(shift, &runs, held);
                self.landed_by_a_jump = None;
                self.library.update(cx, |library, cx| {
                    library.remove_runs_from_playlist(playlist, runs, cx);
                });
                cx.notify();
            }
            Shift::Listing(_) => {}
        }
    }

    pub(crate) fn drop_rows(&mut self, shift: Shift, rows: Span, cx: &mut Context<Self>) {
        let held = match shift {
            Shift::Queue => {
                self.drop_queue_runs(vec![rows], cx);
                return;
            }
            Shift::Playlist(playlist) => {
                let held = self.library.read(cx).entries().len();
                self.library.update(cx, |library, cx| {
                    library.remove_from_playlist(playlist, rows, cx);
                });
                held
            }
            Shift::Listing(_) => return,
        };

        self.reach = Reach::after_dropping(shift, rows, held);
        self.landed_by_a_jump = None;
        cx.notify();
    }

    pub(crate) fn with_everything_listed(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Arc<[Track]>, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let (library, asked) = self.library.read(cx).listing_whole();
        let drawn = self.library.read(cx).as_drawn();

        self.listing_whole = cx.spawn_in(window, async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { library.tracks(&asked) })
                .await;

            let outcome = this.update_in(cx, |this, window, cx| match read {
                Ok(listing) => then(this, drawn.ordered(listing.into()), window, cx),
                Err(error) => {
                    tracing::error!(%error, "the whole listing could not be read");
                    toast::tell(
                        toast::could_not(words::root::read_that_listing(), &error),
                        cx,
                    );
                }
            });
            let _ = outcome;
        });
    }

    pub(super) fn with_the_whole_library(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Arc<[Track]>, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let (library, asked) = self.library.read(cx).whole_library();

        self.listing_whole = cx.spawn_in(window, async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { library.tracks(&asked) })
                .await;

            let outcome = this.update_in(cx, |this, window, cx| match read {
                Ok(tracks) => then(this, tracks.into(), window, cx),
                Err(error) => {
                    tracing::error!(%error, "the whole library could not be read");
                    toast::tell(
                        toast::could_not(words::root::read_the_library(), &error),
                        cx,
                    );
                }
            });
            let _ = outcome;
        });
    }

    pub(crate) fn play_the_listing_from(
        &mut self,
        held: &[Track],
        start: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pressed = held.get(start).map(|track| track.id);
        let shuffled = self.player.read(cx).state().shuffle;
        if shuffled && self.library.read(cx).is_searching_the_whole_library() {
            self.with_the_whole_library(window, cx, move |this, library, window, cx| {
                let found = pressed
                    .and_then(|pressed| library.iter().position(|track| track.id == pressed));
                match found {
                    Some(start) => this.play(&library, start, cx),
                    None => this.play_from_the_listing_read_whole(pressed, start, window, cx),
                }
            });
            return;
        }
        if self.library.read(cx).is_the_whole_listing(held) {
            self.play(held, start, cx);
            return;
        }
        self.play_from_the_listing_read_whole(pressed, start, window, cx);
    }

    pub(super) fn play_from_the_listing_read_whole(
        &mut self,
        pressed: Option<TrackId>,
        start: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.with_everything_listed(window, cx, move |this, listing, _, cx| {
            let found = match pressed {
                Some(pressed) => listing.iter().position(|track| track.id == pressed),
                None => (start < listing.len()).then_some(start),
            };
            let Some(start) = found else {
                tracing::warn!(
                    ?pressed,
                    start,
                    "the row pressed is not in the listing read whole"
                );
                return;
            };
            this.play(&listing, start, cx);
        });
    }

    pub(crate) fn with_the_rows_of(
        &mut self,
        query: &SavedQuery,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Arc<[Track]>, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let (library, asked) = self.library.read(cx).rows_of(query);

        self.listing_whole = cx.spawn_in(window, async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { library.tracks(&asked) })
                .await;

            let outcome = this.update_in(cx, |this, window, cx| match read {
                Ok(rows) => then(this, rows.into(), window, cx),
                Err(error) => {
                    tracing::error!(%error, "a suggestion's rows could not be read");
                    toast::tell(
                        toast::could_not(words::root::read_that_suggestion(), &error),
                        cx,
                    );
                }
            });
            let _ = outcome;
        });
    }

    pub(crate) fn plays_everything_in(
        &mut self,
        scoped: Selection,
        at: Placement,
        now: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.plays_everything_in_each(vec![scoped], at, now, window, cx);
    }

    pub(crate) fn plays_everything_in_each(
        &mut self,
        scopes: Vec<Selection>,
        at: Placement,
        now: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let library = self.library.read(cx).catalog();

        self.listing_whole = cx.spawn_in(window, async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { read_each(&library, scopes) })
                .await;

            let outcome = this.update(cx, |this, cx| match read {
                Ok(listing) if now => this.play(&listing, 0, cx),
                Ok(listing) => this.queue(&listed(&listing), at, cx),
                Err(error) => {
                    tracing::error!(%error, "a scoped listing could not be read");
                    toast::tell(
                        toast::could_not(words::root::read_those_tracks(), &error),
                        cx,
                    );
                }
            });
            let _ = outcome;
        });
    }
}
