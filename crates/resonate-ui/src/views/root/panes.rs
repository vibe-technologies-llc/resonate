use std::mem;

use gpui::{App, Context, Point, Window};
use resonate_core::Span;
use resonate_engine::Command;

use super::{
    Following, Landing, Leaving, Pane, RootView, Wayback, in_front_of, landing, stepped_pane,
};
use crate::{
    ResonateApp,
    views::{
        pointed::{self},
        reorder::{Chosen, Reach, Shift, Step},
        search::SearchShows,
        settings::Category,
    },
};

impl RootView {
    pub(super) fn say_what_this_is(&mut self, cx: &mut Context<Self>) {
        self.set_pane(Pane::Settings, cx);
        self.show_settings(Category::About, cx);
    }

    pub(crate) fn focus_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_pane(Pane::Settings, cx);
        self.typed
            .finding
            .update(cx, |finding, cx| finding.take_focus_and_select(window, cx));
        cx.notify();
    }

    pub(crate) fn leave_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.typed.finding.read(cx).is_focused(window) {
            return;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn in_front(&self, cx: &App) -> Pane {
        in_front_of(self.pane, self.library.read(cx).selection())
    }

    pub(crate) fn choose_pane(&mut self, pane: Pane, cx: &mut Context<Self>) {
        let leaving = self.here_now(cx);
        let library = self.library.read(cx);
        let landing = landing(
            pane,
            self.in_front(cx),
            library.selection(),
            library.opened().is_some(),
        );

        match landing {
            Landing::On(pane) => self.set_pane(pane, cx),
            Landing::Unscoped(pane) => {
                let came_from = mem::take(&mut self.came_from);
                self.show_everything(cx);
                self.came_from = came_from;
                self.set_pane(pane, cx);
            }
            Landing::Scoped => self.set_pane(Pane::Tracks, cx),
            Landing::EveryPlaylist => {
                self.show_playlist(None, cx);
                self.set_pane(Pane::Playlists, cx);
            }
        }

        let arrived = (self.pane, self.library.read(cx).selection());
        if arrived != (leaving.pane, leaving.selection) {
            let leaving = Wayback {
                left_by: Leaving::ByATab,
                ..leaving
            };
            Self::keep_wayback(&mut self.came_from, leaving);
            self.goes_forward.clear();
        }
    }

    pub(crate) fn show_in_the_search(&mut self, shows: SearchShows, cx: &mut Context<Self>) {
        if let Some(pane) = shows.pane() {
            self.set_pane(pane, cx);
        }
        if self.shows.search != shows {
            self.orders.ordering = None;
            self.reach = None;
        }
        self.shows.search = shows;
        cx.notify();
    }

    pub(crate) fn set_pane(&mut self, pane: Pane, cx: &mut Context<Self>) {
        let serving = self.server.read(cx).is_open();
        let pane = if pane.is_there(cx.global::<ResonateApp>().tabs, serving) {
            pane
        } else {
            Pane::default()
        };
        if pane != Pane::Tracks {
            self.adding_songs_to = None;
        }
        pointed::forget();
        self.attention.moved();
        self.record = None;
        self.stop_typing(cx);
        let opens_the_queue = pane == Pane::Queue && self.pane != Pane::Queue;
        if self.pane != pane {
            self.orders.ordering = None;
            if let Some(shows) = SearchShows::in_place_of(pane) {
                self.shows.search = shows;
            }
        }
        self.pane = pane;
        self.server
            .update(cx, |server, cx| server.shown(pane == Pane::Server, cx));
        self.library.update(cx, |library, cx| {
            library.statistics_shown(pane == Pane::Statistics, cx);
        });
        if opens_the_queue {
            self.following = Following::default();
            let player = self.player.clone();
            self.follow_the_playing_row(&player, cx);
        }
        self.landing_on = None;
        self.remember_current_tab(cx);
        cx.notify();
    }

    pub(crate) fn shift_rows(
        &mut self,
        shift: Shift,
        rows: Span,
        to: usize,
        cx: &mut Context<Self>,
    ) {
        match shift {
            Shift::Listing(_) => {}
            Shift::Queue => self.send_by_position(Command::Move { rows, to }, cx),
            Shift::Playlist(playlist) => self.library.update(cx, |library, cx| {
                library.move_in_playlist(playlist, rows, to, cx);
            }),
        }
    }

    pub(crate) fn shift_chosen(
        &mut self,
        shift: Shift,
        chosen: &Chosen,
        to: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(rows) = chosen.single() {
            self.shift_rows(shift, rows, to, cx);
            return;
        }
        let Shift::Playlist(playlist) = shift else {
            return;
        };

        let held = self.library.read(cx).entries().len();
        let landed = chosen.gathered_by(to, held);
        let runs = chosen.runs().to_vec();
        self.library.update(cx, |library, cx| {
            library.gather_in_playlist(playlist, runs, to, cx);
        });
        if let Some(landed) = landed {
            self.reach = Some(Reach::spanning(shift, landed.first(), landed.last()));
        }
        self.typing.landed_by_a_jump = None;
        cx.notify();
    }

    pub(super) fn step_pane(&mut self, step: Step, cx: &mut Context<Self>) {
        let tabs = cx.global::<ResonateApp>().tabs;
        let serving = self.server.read(cx).is_open();
        let stepped = stepped_pane(self.in_front(cx), step, tabs, serving);
        self.choose_pane(stepped, cx);
    }

    pub(crate) fn toggle_queue(&mut self, cx: &mut Context<Self>) {
        if self.pane == Pane::Queue {
            self.set_pane(self.behind_queue, cx);
            return;
        }

        self.behind_queue = self.pane;
        self.set_pane(Pane::Queue, cx);
    }

    pub(crate) fn show_settings(&mut self, category: Category, cx: &mut Context<Self>) {
        let left_a_page = self.settings_shown.page.take().is_some();
        let moved = self.settings_shown.category != category;
        if moved {
            self.settings_shown.category = category;
            self.remember_current_settings_category(cx);
        }
        if moved || left_a_page {
            self.scrolls.settings.set_offset(Point::default());
        }
        self.disarm();
        cx.notify();
    }
}
