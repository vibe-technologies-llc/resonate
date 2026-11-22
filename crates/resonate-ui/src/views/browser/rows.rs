use gpui::{
    App, Context, Div, ElementId, SharedString, Stateful, Window, div, prelude::*, px, rgb,
};
use resonate_engine::Placement;
use resonate_library::{Favoured, Track};

use super::{Chooses, Plays, ROW_CONTROLS, controls_width, tracks::queueing};
use crate::{
    held::Rows,
    theme,
    views::{kit, playlists::ROW_GROUP, root::RootView},
    words,
};

pub(super) const CONTROL_GAP: f32 = 2.0;

const ROW_CONTROLS_AT_REST: f32 = 0.4;

impl RootView {
    pub(super) fn queue_track_control(
        &self,
        id: (&'static str, usize),
        at: Placement,
        tracks: &Rows<Track>,
        index: usize,
        chooses: Option<Chooses>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        match chooses {
            Some(Chooses { shift, row }) => self.queue_chosen_control(id, at, shift, row, cx),
            None => self.queue_control(id, at, queueing(tracks, index), cx),
        }
    }

    pub(super) fn play_the_row(
        &mut self,
        plays: Plays,
        played: &Rows<Track>,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match plays {
            Plays::TheseRows => self.play_rows(played, index, cx),
            Plays::AsTheListingIsDrawn { .. } => {
                let Some((drawn, start)) = self.library.read(cx).played_from_held(index) else {
                    return;
                };

                self.play_the_listing_from(&drawn, start, window, cx);
            }
        }
    }

    pub(crate) fn favour_mark(
        &self,
        id: impl Into<ElementId>,
        what: Favoured,
        already: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let saying = match already {
            true => words::browser::unfavour_hint(),
            false => words::browser::favour_hint(),
        };
        let mark = kit::star(id, already, SharedString::from(format!("{what:?}")), saying);

        self.in_the_pane_ring(
            mark,
            move |this, _, cx| {
                cx.stop_propagation();
                this.library
                    .update(cx, |library, cx| library.favour(what, !already, cx));
            },
            cx,
        )
    }

    pub(crate) fn row_controls(&self, id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
        let id = id.into();
        let within = self.controls.holding(id.to_string(), cx);

        div()
            .id(id)
            .track_focus(&within)
            .flex()
            .flex_none()
            .items_center()
            .gap_0p5()
            .opacity(ROW_CONTROLS_AT_REST)
            .group_hover(ROW_GROUP, |controls| controls.opacity(1.0))
    }

    pub(crate) fn favour_mark_under(
        &self,
        id: impl Into<ElementId>,
        what: Favoured,
        already: bool,
        group: &'static str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        self.favour_mark(id, what, already, cx)
            .when(!already, |mark| {
                mark.opacity(0.0)
                    .group_hover(group, |mark| mark.opacity(1.0))
            })
    }
}

pub(crate) fn controls_place() -> Div {
    controls_place_of(ROW_CONTROLS)
}

pub(super) fn controls_place_of(controls: usize) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_end()
        .w(px(controls_width(controls)))
}

pub(crate) fn trailing_controls() -> Div {
    div().flex().flex_none().items_center().gap_0p5()
}

pub(crate) fn reached_ring() -> Div {
    div()
        .absolute()
        .inset(px(-REACHED_RING - 1.0))
        .rounded(px(REACHED_ROUNDING))
        .border(px(REACHED_RING))
        .border_color(rgb(theme::accent()))
}

pub(crate) fn cursor_ring() -> Div {
    reached_ring().border_color(rgb(theme::focus_ring()))
}

const REACHED_ROUNDING: f32 = 10.0;

pub(super) const REACHED_RING: f32 = 2.0;
