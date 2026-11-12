use gpui::{App, Context, SharedString, UniformListScrollHandle, point};
use resonate_core::ArtistsDrawn;

use super::{Leaving, LeftAt, Pane, RootView, WAYS_BACK, Wayback, in_front_of};
use crate::{Selection, theme};

impl RootView {
    pub(super) fn here(&self, cx: &App) -> SharedString {
        let library = self.library.read(cx);
        let named = match (self.pane, library.selection()) {
            (Pane::Tracks, Selection::Album(id)) => {
                library.album_of(id).map(|album| album.title.clone())
            }
            (Pane::Tracks, Selection::Artist(id)) => library
                .artists()
                .iter()
                .find(|artist| artist.id == id)
                .map(|artist| artist.name.clone())
                .or_else(|| library.artist_detail().map(|detail| detail.name.clone())),
            (pane, _) => Some(pane.label().to_owned()),
        };

        named.map_or_else(
            || SharedString::new_static(self.in_front(cx).label()),
            SharedString::from,
        )
    }

    pub(super) fn here_now(&self, cx: &App) -> Wayback {
        Wayback {
            pane: self.pane,
            selection: self.library.read(cx).selection(),
            at: self.top_row(),
            named: self.here(cx),
            left_by: Leaving::ByALink,
        }
    }

    pub(super) fn keep_wayback(history: &mut Vec<Wayback>, wayback: Wayback) {
        if history.len() == WAYS_BACK {
            history.remove(0);
        }
        history.push(wayback);
    }

    pub(super) fn restore_wayback(&mut self, wayback: Wayback, cx: &mut Context<Self>) {
        let Wayback {
            pane,
            selection,
            at,
            named: _,
            left_by: _,
        } = wayback;
        self.library
            .update(cx, |library, cx| library.select(selection, cx));
        self.set_pane(pane, cx);
        self.landing_on = pane.lands_where_it_was_left().then_some(at);
    }

    pub(super) fn way_back_within(&self, standing: (Pane, Selection)) -> Option<usize> {
        let category = in_front_of(standing.0, standing.1);
        let mut crossed_a_tab = false;
        for (at, back) in self.came_from.iter().enumerate().rev() {
            if (back.pane, back.selection) == standing {
                continue;
            }
            let under_it = in_front_of(back.pane, back.selection) == category;
            if under_it || (!crossed_a_tab && back.left_by == Leaving::ByALink) {
                return Some(at);
            }
            crossed_a_tab |= back.left_by == Leaving::ByATab;
        }
        None
    }

    pub(super) fn page_shown(&self, cx: &App) -> (Pane, Selection) {
        (self.pane, self.library.read(cx).selection())
    }

    pub(crate) fn way_back_to(&self, cx: &App) -> Option<SharedString> {
        self.way_back_within(self.page_shown(cx))
            .map(|at| self.came_from[at].named.clone())
    }

    pub(crate) fn step_back(&mut self, cx: &mut Context<Self>) {
        match self.way_back_within(self.page_shown(cx)) {
            Some(at) => self.go_back_to(at, cx),
            None => {
                let category = self.in_front(cx);
                self.choose_pane(category, cx);
            }
        }
    }

    pub(super) fn go_back_to(&mut self, at: usize, cx: &mut Context<Self>) {
        let back = self.came_from.remove(at);
        let current = self.here_now(cx);
        Self::keep_wayback(&mut self.goes_forward, current);
        self.restore_wayback(back, cx);
    }

    pub(super) fn goes_back_to_a_scope(&self, cx: &App) -> bool {
        self.way_back_within(self.page_shown(cx))
            .is_some_and(|at| self.came_from[at].selection != Selection::Everything)
    }

    pub(crate) fn go_back(&mut self, cx: &mut Context<Self>) {
        let Some(back) = self.came_from.pop() else {
            return;
        };
        let current = self.here_now(cx);
        Self::keep_wayback(&mut self.goes_forward, current);
        self.restore_wayback(back, cx);
    }

    pub(crate) fn go_forward(&mut self, cx: &mut Context<Self>) {
        let Some(forward) = self.goes_forward.pop() else {
            return;
        };
        let current = self.here_now(cx);
        Self::keep_wayback(&mut self.came_from, current);
        self.restore_wayback(forward, cx);
    }

    pub(crate) fn land_where_it_was_left(&mut self, held: usize) {
        let Some(left_at) = self.landing_on else {
            return;
        };
        if left_at.row >= held {
            if held > 0 {
                self.landing_on = None;
            }
            return;
        }

        let base = self.scroll_of(self.pane).0.borrow().base_handle.clone();
        let offset = base.offset();
        base.set_offset(point(
            offset.x,
            left_at.offset_at(self.row_height_of(self.pane)),
        ));
        self.landing_on = None;
    }

    pub(super) fn top_row(&self) -> LeftAt {
        let offset = self.scroll_of(self.pane).0.borrow().base_handle.offset().y;
        LeftAt::read_off(offset, self.row_height_of(self.pane))
    }

    pub(super) fn row_height_of(&self, pane: Pane) -> f32 {
        match pane {
            Pane::Albums => self.grid_row(),
            Pane::Artists if self.artists_drawn == ArtistsDrawn::Grid => self.grid_row(),
            Pane::Artists => theme::tall_row_height(),
            _ => theme::row_height(),
        }
    }

    pub(super) fn scroll_of(&self, pane: Pane) -> &UniformListScrollHandle {
        match pane {
            Pane::Artists => &self.artist_rows,
            Pane::Albums => &self.album_rows,
            Pane::Queue => &self.queue_rows,
            Pane::Playlists => &self.playlist_rows,
            Pane::Tracks
            | Pane::Statistics
            | Pane::Party
            | Pane::Favourites
            | Pane::Suggestions
            | Pane::Missing
            | Pane::Lyrics
            | Pane::Inspector
            | Pane::Visualiser
            | Pane::Analysis
            | Pane::Settings => &self.track_rows,
            Pane::Server => &self.server_rows,
        }
    }
}
