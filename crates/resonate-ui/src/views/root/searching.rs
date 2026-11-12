use ahash::AHashSet;
use gpui::{App, Context, Entity, Focusable, UniformListScrollHandle, Window, point, px};
use resonate_core::PlaylistId;

use super::{Pane, RootView};
use crate::{
    LibraryModel, ResonateApp, Selection,
    views::{
        pointed::{self},
        reorder::{Shift, Step},
        search::SearchShows,
    },
};

impl RootView {
    pub(super) fn enter_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search
            .update(cx, |search, cx| search.take_focus_and_select(window, cx));
        cx.notify();
    }

    pub(super) fn go_to_the_results(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.search.read(cx).is_focused(window) {
            return;
        }
        if self.library.read(cx).is_reading_the_search() {
            self.waiting_for_the_results = true;
            self.library
                .update(cx, |library, cx| library.read_the_search_now(cx));
            return;
        }
        self.reach_the_first_result(window, cx);
    }

    pub(super) fn reach_the_first_result(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.search.read(cx).is_focused(window) {
            return;
        }
        window.focus(&self.focus, cx);
        self.reach = None;
        self.reach_row(Step::Below, cx);
        cx.notify();
    }

    pub(super) fn tab_onward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.search.read(cx).is_focused(window) {
            self.go_to_the_results(window, cx);
        } else {
            self.step_the_focus(true, window, cx);
        }
    }

    pub(super) fn tab_past_the_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.focus.is_focused(window) && !self.something_stands_over_the_pane() {
            let search = self.search.focus_handle(cx);
            search.focus(window, cx);
        }
        self.step_the_focus(true, window, cx);
    }

    pub(super) fn tab_back_to_the_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.focus.is_focused(window) && !self.something_stands_over_the_pane() {
            let search = self.search.focus_handle(cx);
            search.focus(window, cx);
            return;
        }
        self.step_the_focus(false, window, cx);
    }

    pub(super) fn leave_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.search.read(cx).is_focused(window) {
            return;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(super) fn dismiss_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.close_the_menu(cx) {
            return;
        }
        if self.zone_name.read(cx).is_focused(window) {
            self.stop_naming_the_zone(window, cx);
            return;
        }
        if self.finding.read(cx).is_focused(window) {
            self.finding.update(cx, |finding, cx| finding.clear(cx));
            self.leave_filter(window, cx);
            return;
        }
        if self.server_search.read(cx).is_focused(window) {
            self.server_search.update(cx, |field, cx| field.clear(cx));
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        if self.contact.read(cx).is_focused(window) {
            self.put_back_contact(window, cx);
            return;
        }
        if self.acoustid.read(cx).is_focused(window) {
            self.put_back_acoustid(window, cx);
            return;
        }
        if self.audd.read(cx).is_focused(window) {
            self.put_back_audd(window, cx);
            return;
        }
        if self.listenbrainz.read(cx).is_focused(window) {
            self.put_back_listenbrainz(window, cx);
            return;
        }
        if self.an_account_field_is_focused(window, cx) {
            self.put_back_the_account(window, cx);
            return;
        }
        if self.discord_app.read(cx).is_focused(window) {
            self.leave_discord_app(window, cx);
            return;
        }
        if self.discord_icon.read(cx).is_focused(window) {
            self.leave_discord_icon(window, cx);
            return;
        }
        if [&self.sync_server, &self.sync_device, &self.sync_code]
            .iter()
            .any(|field| field.read(cx).is_focused(window))
        {
            self.leave_sync_field(window, cx);
            return;
        }
        if self.minimum_length.read(cx).is_focused(window) {
            let length = cx
                .global::<ResonateApp>()
                .library
                .music_filters()
                .minimum_length;
            self.minimum_length
                .update(cx, |field, cx| field.hold(length.seconds().to_string(), cx));
            window.focus(&self.focus, cx);
            cx.notify();
            return;
        }
        if self.organising.read(cx).is_focused(window) {
            self.put_back_organising(window, cx);
            return;
        }
        if self.typed_root.read(cx).is_focused(window) {
            self.typed_root
                .update(cx, |typed, cx| typed.hold(String::new(), cx));
            self.leave_typed_root(window, cx);
            return;
        }
        if self.figure.read(cx).is_focused(window) {
            self.leave_figure(window, cx);
            return;
        }
        if self.looking.read(cx).is_focused(window) {
            self.leave_looking(window, cx);
            return;
        }
        if self.matching.is_some() {
            self.leave_the_matching(window, cx);
            return;
        }
        if self.naming.is_some() || self.adding.is_some() {
            self.stop_naming(window, cx);
            return;
        }
        if self.adding_songs_to.is_some() {
            self.finish_adding_songs(cx);
            return;
        }
        if self.pane == Pane::Settings
            && self.finding.read(cx).text().is_empty()
            && self.leave_the_settings_page(cx)
        {
            window.focus(&self.focus, cx);
            return;
        }

        self.reach = None;
        window.focus(&self.focus, cx);
        if self.search.read(cx).text().is_empty() {
            let scoped = self.library.read(cx).selection() != Selection::Everything;
            if scoped || self.goes_back_to_a_scope(cx) {
                self.step_back(cx);
            }
        } else {
            self.search.update(cx, |search, cx| search.clear(cx));
        }
        cx.notify();
    }

    pub(crate) fn show_everything(&mut self, cx: &mut Context<Self>) {
        pointed::forget();
        self.came_from.clear();
        self.goes_forward.clear();
        if self.library.read(cx).selection() == Selection::Everything {
            return;
        }

        self.attention.moved();
        self.read_from_the_top(cx);
        self.library
            .update(cx, |library, cx| library.select(Selection::Everything, cx));
    }

    pub(crate) fn search_instead(
        &mut self,
        instead: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_everything(cx);
        self.search.update(cx, |search, cx| {
            search.take_focus(window, cx);
            search.set_text(instead, cx);
        });
    }

    pub(crate) fn let_go_of_the_reach_in(&mut self, shift: Shift) {
        if self
            .reach
            .as_ref()
            .is_some_and(|reach| reach.shift == shift)
        {
            self.reach = None;
        }
    }

    pub(super) fn open_the_search(&mut self, cx: &mut Context<Self>) {
        let browsing = matches!(self.pane, Pane::Albums | Pane::Artists | Pane::Tracks);
        if !browsing {
            if self.library.read(cx).selection() != Selection::Everything {
                self.show_everything(cx);
            }
            self.set_pane(Pane::Tracks, cx);
        }
        self.search_shows = SearchShows::Top;
        self.ordering = None;
    }

    pub(super) fn leave_the_scroll_for_the_search(&mut self) {
        self.left_for_the_search = self
            .pane
            .lands_where_it_was_left()
            .then(|| (self.pane, self.top_row()));
    }

    pub(super) fn land_where_the_search_began(&mut self) {
        let began = self.left_for_the_search.take();
        self.landing_on = began
            .filter(|(pane, _)| *pane == self.pane)
            .map(|(_, at)| at)
            .or(self.landing_on);
    }

    pub(super) fn set_query(&mut self, query: String, cx: &mut Context<Self>) {
        let was = self
            .library
            .read(cx)
            .narrowing()
            .unwrap_or_default()
            .to_owned();
        if was == query {
            return;
        }
        self.attention.moved();
        if was.is_empty() {
            self.leave_the_scroll_for_the_search();
            self.open_the_search(cx);
        }
        if query.is_empty() {
            self.land_where_the_search_began();
        }
        self.search_scroll.set_offset(point(px(0.0), px(0.0)));
        self.read_from_the_top(cx);
        if self
            .reach
            .as_ref()
            .is_some_and(|reach| matches!(reach.shift, Shift::Listing(_)))
        {
            self.reach = None;
        }
        self.library
            .update(cx, |library, cx| library.set_query(query, cx));
        cx.notify();
    }

    pub(crate) fn show_playlist(&mut self, opened: Option<PlaylistId>, cx: &mut Context<Self>) {
        if self.library.read(cx).opened() != opened {
            self.read_the_kept_order(opened, cx);
        }
        self.playlist_rows = match opened {
            Some(id) => self.left_at.entry(id).or_default().clone(),
            None => UniformListScrollHandle::default(),
        };
        self.library
            .update(cx, |library, cx| library.open_playlist(opened, cx));
    }

    pub(super) fn forget_what_has_gone(&mut self, library: &Entity<LibraryModel>, cx: &App) {
        let listing = library.read(cx);
        if listing.narrowing().is_some() {
            return;
        }

        let held: AHashSet<PlaylistId> = listing
            .playlists()
            .iter()
            .map(|playlist| playlist.id)
            .collect();
        self.left_at.retain(|id, _| held.contains(id));
    }

    pub(super) fn read_from_the_top(&mut self, cx: &App) {
        self.playlist_rows = UniformListScrollHandle::default();
        if let Some(opened) = self.library.read(cx).opened() {
            self.left_at.insert(opened, self.playlist_rows.clone());
        }
    }
}
