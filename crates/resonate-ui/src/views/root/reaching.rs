use gpui::{App, Context, Modifiers, prelude::*};
use resonate_core::{AlbumId, ArtistId, ArtistsDrawn, Span};
use resonate_library::Track;

use super::{Arrangement, RootView};
use crate::{
    theme,
    views::{
        favourites::Shelved,
        menu::{self, AtTheReach},
        playlists::PlaylistsDrawn,
        reorder::{self, Chosen, Listed, Reach, Shift, Step},
        search::{self, TopRun},
        spoken::Spoken,
    },
};

impl RootView {
    pub(super) fn arrangement(&self, cx: &App) -> Arrangement {
        let library = self.library.read(cx);

        Arrangement {
            selection: library.selection(),
            sorting: library.sorting(),
            playlists: (library.playlist_order(), library.playlist_reading()),
            opened_rows: (self.orders.row_order, self.orders.row_reading),
            search: (self.orders.query_sort, self.orders.query_reading),
            shelves: (library.favourited().artists, library.favourited().albums),
        }
    }

    pub(super) fn let_go_of_a_reach_the_rows_left(&mut self, cx: &App) {
        let arranged_as = self.arrangement(cx);
        if arranged_as == self.arranged_as {
            return;
        }
        self.arranged_as = arranged_as;
        if self
            .reach
            .as_ref()
            .is_some_and(|reach| reach.shift != Shift::Queue)
        {
            self.reach = None;
        }
    }

    pub(crate) fn reaches(&self, shift: Shift, row: usize) -> bool {
        self.reach
            .as_ref()
            .is_some_and(|reach| reach.shift == shift && reach.reaches(row))
    }

    pub(crate) fn at_the_reach(&self, shift: Shift, row: usize) -> Option<AtTheReach> {
        self.reach
            .as_ref()
            .filter(|reach| reach.shift == shift && reach.row == row)
            .map(|_| AtTheReach { shift, row })
    }

    pub(super) fn open_the_reached_menu(&mut self, cx: &mut Context<Self>) {
        if self.something_stands_over_the_pane() {
            return;
        }
        self.reach_by_hand(cx);
        let Some((shift, held)) = self.reachable(cx) else {
            return;
        };
        let Some(reach) = self.reach_in(shift, held) else {
            return;
        };
        let at = AtTheReach {
            shift,
            row: reach.row,
        };
        if let Some(menu) = menu::offered_at_the_reach(at, self, cx) {
            self.open_a_menu(menu, cx);
        }
    }

    #[cfg(test)]
    pub(crate) const fn reached(&self) -> Option<&Reach> {
        self.reach.as_ref()
    }

    pub(crate) fn stands_unchosen_on(&self, shift: Shift, row: usize) -> bool {
        self.reach
            .as_ref()
            .is_some_and(|reach| reach.shift == shift && reach.row == row && !reach.is_chosen())
    }

    pub(crate) fn acting_on_chosen(&self, shift: Shift, row: usize) -> Chosen {
        match self.reach.as_ref().filter(|reach| reach.shift == shift) {
            Some(reach) if reach.reaches(row) => reach.selected(),
            _ => Chosen::of(Span::one(row)),
        }
    }

    pub(crate) fn choosable(&self, shift: Shift, chosen: Chosen, cx: &App) -> Chosen {
        chosen.within(self.choosable_rows(shift, cx))
    }

    pub(super) fn choosable_rows(&self, shift: Shift, cx: &App) -> usize {
        match shift {
            Shift::Listing(Listed::Top) => TopRun::of(self.library.read(cx)).songs(),
            _ => usize::MAX,
        }
    }

    pub(crate) fn reached_chosen(&self, shift: Shift) -> Option<Chosen> {
        self.reach
            .as_ref()
            .filter(|reach| reach.shift == shift)
            .map(Reach::selected)
    }

    pub(crate) fn tracks_chosen(&self, shift: Shift, chosen: &Chosen, cx: &App) -> Vec<Track> {
        let library = self.library.read(cx);
        let picked = |tracks: &[Track]| -> Vec<Track> {
            chosen
                .rows()
                .filter_map(|row| tracks.get(row).cloned())
                .collect()
        };

        match shift {
            Shift::Listing(Listed::Tracks) => library.tracks_at(chosen.rows()),
            Shift::Listing(Listed::Favourites) => picked(&library.favourite_tracks()),
            Shift::Listing(Listed::Suggested) => library
                .opened_suggestion()
                .map_or_else(Vec::new, |opened| picked(&opened.tracks)),
            Shift::Listing(Listed::Top) => {
                let songs = TopRun::of(library).songs();
                picked(
                    &library
                        .listing()
                        .iter()
                        .take(songs)
                        .cloned()
                        .collect::<Vec<_>>(),
                )
            }
            _ => Vec::new(),
        }
    }

    pub(crate) fn albums_chosen(&self, shift: Shift, chosen: &Chosen, cx: &App) -> Vec<AlbumId> {
        let library = self.library.read(cx);
        match shift {
            Shift::Listing(Listed::FavouriteAlbums) => {
                let shelf = library.favourite_albums();
                chosen
                    .rows()
                    .filter_map(|row| shelf.get(row).map(|album| album.id))
                    .collect()
            }
            _ => library.albums_at(chosen.rows()),
        }
    }

    pub(crate) fn artists_chosen(&self, shift: Shift, chosen: &Chosen, cx: &App) -> Vec<ArtistId> {
        let library = self.library.read(cx);
        match shift {
            Shift::Listing(Listed::FavouriteArtists) => {
                let shelf = library.favourite_artists();
                chosen
                    .rows()
                    .filter_map(|row| shelf.get(row).map(|artist| artist.id))
                    .collect()
            }
            _ => library.artists_at(chosen.rows()),
        }
    }

    pub(crate) fn marked_where_reached<E: Styled + FluentBuilder + Spoken>(
        &self,
        shift: Shift,
        row: usize,
        listed: E,
    ) -> E {
        let reached = self.reaches(shift, row);
        reorder::cursor_marked(
            reorder::marked(listed.chosen(reached), reached),
            self.stands_unchosen_on(shift, row),
        )
    }

    pub(crate) fn reach_by_click(
        &mut self,
        shift: Shift,
        row: usize,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) -> bool {
        let apart = modifiers.secondary(cx);
        if !shift.chooses_apart() || !(apart || modifiers.shift) {
            return false;
        }
        let choosable = self.choosable_rows(shift, cx);
        if row >= choosable {
            return false;
        }
        self.reach_by_hand(cx);

        let held = self
            .reach
            .as_ref()
            .filter(|reach| reach.shift == shift)
            .and_then(|reach| reach.within(choosable));
        self.reach = Some(match held {
            Some(reach) if apart => reach.toggled(row),
            Some(reach) => reach.widened_to(row),
            None => Reach::at(shift, row),
        });
        cx.notify();

        true
    }

    pub(super) fn choose_the_reached(&mut self, cx: &mut Context<Self>) {
        if self.something_stands_over_the_pane() {
            return;
        }
        self.reach_by_hand(cx);
        let Some((shift, held)) = self.reachable(cx) else {
            return;
        };
        let choosable = self.choosable_rows(shift, cx);
        if !shift.chooses_apart() || choosable == 0 {
            return;
        }

        self.reach = Some(match self.reach_in(shift, held) {
            Some(reach) if reach.row >= choosable => return,
            Some(reach) => reach.toggled(reach.row),
            None => Reach::at(shift, self.opening_row(shift, held, cx)),
        });
        cx.notify();
    }

    pub(super) fn hop_to_a_row(&mut self, step: Step, cx: &mut Context<Self>) {
        if self.something_stands_over_the_pane() {
            return;
        }
        self.reach_by_hand(cx);
        let Some((shift, held)) = self.reachable(cx) else {
            return;
        };
        if !shift.chooses_apart() {
            return;
        }
        let Some(reach) = self.reach_in(shift, held) else {
            return self.reach_row(step, cx);
        };
        let Some(row) = step.landing(Span::one(reach.row), held) else {
            return;
        };
        let choosable = self.choosable_rows(shift, cx);
        if row >= choosable || reach.row >= choosable {
            return;
        }

        self.reach = Some(reach.hopped_to(row));
        self.show_row(shift, row, cx);
        cx.notify();
    }

    pub(crate) fn acting_on(&self, shift: Shift, row: usize) -> Span {
        match self.reaching(shift) {
            Some(rows) if rows.holds(row) => rows,
            _ => Span::one(row),
        }
    }

    pub(crate) fn reaching(&self, shift: Shift) -> Option<Span> {
        self.reach
            .as_ref()
            .filter(|reach| reach.shift == shift)
            .map(Reach::rows)
    }

    pub(crate) fn reach_at(
        &mut self,
        shift: Shift,
        row: usize,
        extending: bool,
        cx: &mut Context<Self>,
    ) {
        let held = self.reach.as_ref().filter(|reach| reach.shift == shift);
        self.typing.landed_by_a_jump = None;
        self.reach = Some(match (extending, held) {
            (true, Some(reach)) => reach.widened_to(row),
            _ => Reach::at(shift, row),
        });
        cx.notify();
    }

    pub(super) fn reach_row(&mut self, step: Step, cx: &mut Context<Self>) {
        self.reach_by_hand(cx);
        if self.step_the_menu(
            match step {
                Step::Above => -1,
                Step::Below => 1,
            },
            cx,
        ) || self.something_stands_over_the_pane()
        {
            return;
        }

        let Some((shift, held)) = self.reachable(cx) else {
            return;
        };
        if self.reach_across_the_favourites(shift, held, step, cx) {
            return;
        }
        let row = match self.reach_in(shift, held) {
            Some(reach) => self.line_beyond(shift, reach, step, held, cx),
            None => self.opening_row(shift, held, cx),
        };

        self.reach = Some(Reach::at(shift, row));
        self.show_row(shift, row, cx);
        cx.notify();
    }

    pub(super) fn reach_across_the_favourites(
        &mut self,
        shift: Shift,
        held: usize,
        step: Step,
        cx: &mut Context<Self>,
    ) -> bool {
        let Shift::Listing(from) = shift else {
            return false;
        };
        let Some(reach) = self.reach_in(shift, held) else {
            return false;
        };
        let shelved = Shelved::of(self.library.read(cx));
        if shelved.rows_of(from) == 0 {
            return false;
        }
        let leaves =
            Shelved::is_a_strip(from) || (step == Step::Above && step.end(reach.rows()) == 0);
        if !leaves {
            return false;
        }

        if let Some(into) = shelved.beyond(from, step) {
            let shift = Shift::Listing(into);
            self.reach = Some(Reach::at(shift, 0));
            self.show_row(shift, 0, cx);
            cx.notify();
        }

        true
    }

    pub(super) fn line_beyond(
        &self,
        shift: Shift,
        reach: Reach,
        step: Step,
        held: usize,
        cx: &App,
    ) -> usize {
        let end = step.end(reach.rows());
        let landing = match shift {
            Shift::Listing(Listed::Top) => TopRun::of(self.library.read(cx)).line_beyond(end, step),
            _ => step.landing(reach.rows(), held),
        };

        landing.unwrap_or(end)
    }

    pub(super) fn reach_along_the_strip(&mut self, step: Step, cx: &mut Context<Self>) -> bool {
        if self.something_stands_over_the_pane() {
            return false;
        }
        let Some((shift @ Shift::Listing(listed), held)) = self.reachable(cx) else {
            return false;
        };
        let Some(reach) = self.reach_in(shift, held) else {
            return false;
        };
        let at = step.end(reach.rows());
        let along = match listed {
            Listed::Top => TopRun::of(self.library.read(cx)).along_the_strip(at, step),
            _ => Shelved::of(self.library.read(cx)).along_the_strip(listed, at, step),
        };
        let Some(along) = along else {
            return false;
        };
        if let Some(row) = along {
            self.reach = Some(Reach::at(shift, row));
            self.show_row(shift, row, cx);
            cx.notify();
        }

        true
    }

    pub(super) fn reach_the_end(&mut self, step: Step, cx: &mut Context<Self>) {
        if self.something_stands_over_the_pane() {
            return;
        }
        self.reach_by_hand(cx);
        let Some((shift, held)) = self.reachable(cx) else {
            return;
        };
        let row = match step {
            Step::Above => 0,
            Step::Below => held - 1,
        };

        self.reach = Some(Reach::at(shift, row));
        self.show_row(shift, row, cx);
        cx.notify();
    }

    pub(super) fn reach_a_page(&mut self, step: Step, cx: &mut Context<Self>) {
        if self.something_stands_over_the_pane() {
            return;
        }
        self.reach_by_hand(cx);
        let Some((shift, held)) = self.reachable(cx) else {
            return;
        };
        let page = self.rows_a_page(shift);
        let from = match self.reach_in(shift, held) {
            Some(reach) => step.end(reach.rows()),
            None => self.opening_row(shift, held, cx),
        };
        let row = match step {
            Step::Above => from.saturating_sub(page),
            Step::Below => from.saturating_add(page).min(held - 1),
        };

        self.reach = Some(Reach::at(shift, row));
        self.show_row(shift, row, cx);
        cx.notify();
    }

    pub(super) fn reach_everything(&mut self, cx: &mut Context<Self>) {
        if self.something_stands_over_the_pane() {
            return;
        }
        self.reach_by_hand(cx);
        let Some((shift, held)) = self.reachable(cx) else {
            return;
        };

        self.reach = Some(Reach::spanning(shift, 0, held - 1));
        cx.notify();
    }

    pub(super) fn drop_reached(&mut self, cx: &mut Context<Self>) -> bool {
        if self.something_stands_over_the_pane() {
            return false;
        }
        let Some((shift, held)) = self.reachable(cx) else {
            return false;
        };
        let Some(reach) = self.reach_in(shift, held) else {
            return false;
        };
        if !shift.is_edited() {
            return false;
        }
        if matches!(shift, Shift::Playlist(_)) && !self.opened_rows(cx).are_edited() {
            return false;
        }

        self.drop_chosen(shift, &reach.acted_on(), cx);
        true
    }

    pub(super) fn rows_a_page(&self, shift: Shift) -> usize {
        let listing = match shift {
            Shift::Queue => &self.scrolls.queue_rows,
            Shift::Playlist(_) => &self.scrolls.playlist_rows,
            Shift::Listing(Listed::Tracks) => &self.scrolls.track_rows,
            Shift::Listing(Listed::Albums) => &self.scrolls.album_rows,
            Shift::Listing(Listed::Artists) => &self.scrolls.artist_rows,
            Shift::Listing(Listed::Playlists) => &self.scrolls.all_playlist_rows,
            Shift::Listing(Listed::Favourites) => &self.scrolls.favourite_rows,
            Shift::Listing(Listed::FavouriteArtists | Listed::FavouriteAlbums) => return 1,
            Shift::Listing(Listed::Missing) => &self.scrolls.missing_rows,
            Shift::Listing(Listed::Suggested) => &self.scrolls.suggestion_rows,
            Shift::Listing(Listed::Top) => {
                return search::SONGS_AT_THE_TOP + search::FOUND_AT_THE_TOP;
            }
            Shift::Listing(Listed::Offered) => return self.cards_a_page(),
            Shift::Listing(Listed::Heard) => return self.heard_a_page(),
        };
        let shown = listing
            .0
            .borrow()
            .last_item_size
            .map_or(0.0, |size| f32::from(size.item.height));

        let in_a_grid = shift == Shift::Listing(Listed::Albums)
            || (shift == Shift::Listing(Listed::Artists)
                && self.shows.artists == ArtistsDrawn::Grid)
            || (shift == Shift::Listing(Listed::Playlists)
                && self.shows.playlists == PlaylistsDrawn::Grid);
        if in_a_grid {
            return ((shown / self.grid_row()) as usize).max(1) * self.grid_columns();
        }
        ((shown / theme::row_height()) as usize).max(1)
    }

    pub(super) fn widen_reach(&mut self, step: Step, cx: &mut Context<Self>) {
        if self.something_stands_over_the_pane() {
            return;
        }
        self.reach_by_hand(cx);
        let Some((shift, held)) = self.reachable(cx) else {
            return;
        };
        let Some(reach) = self.reach_in(shift, held) else {
            return self.reach_row(step, cx);
        };
        let Some(row) = step.landing(Span::one(reach.row), held) else {
            return;
        };

        self.reach = Some(reach.widened_to(row));
        self.show_row(shift, row, cx);
        cx.notify();
    }
}
