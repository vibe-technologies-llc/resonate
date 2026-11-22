use gpui::{AnyElement, Context, Div, Stateful};

use crate::{
    icons::Icon,
    theme,
    views::{
        kit,
        kit::Tone,
        root::{RootView, empty},
    },
    words,
};

pub(super) const GRID_PADDING: f32 = 48.0;

pub(super) const A_CELL_GROWS_AT_MOST: f32 = 1.5;

pub(super) fn columns_across(width: f32) -> usize {
    let cell = theme::grid_cover() + theme::grid_gap();

    (((width - GRID_PADDING + theme::grid_gap()) / cell).floor() as usize).max(1)
}

pub(super) fn side_across(width: f32) -> f32 {
    let columns = columns_across(width) as f32;
    let filled = (width - GRID_PADDING - (columns - 1.0) * theme::grid_gap()) / columns;

    filled.floor().clamp(
        theme::grid_cover(),
        theme::grid_cover() * A_CELL_GROWS_AT_MOST,
    )
}

pub(super) const CELL_GROUP: &str = "album-cell";

impl RootView {
    pub(crate) fn grid_columns(&self) -> usize {
        columns_across(f32::from(self.rooms.grid_width.get()))
    }

    pub(crate) fn grid_side(&self) -> f32 {
        side_across(f32::from(self.rooms.grid_width.get()))
    }

    pub(crate) fn grid_row(&self) -> f32 {
        theme::grid_row() - theme::grid_cover() + self.grid_side()
    }

    pub(super) fn add_songs(&mut self, cx: &mut Context<Self>) -> AnyElement {
        kit::empty_offering(
            Icon::Tracks,
            words::browser::add_songs(),
            Some(words::browser::add_songs_more()),
            self.in_the_pane_ring(
                kit::button(
                    "choose-the-primary-folder",
                    Some(Icon::Folder),
                    words::browser::set_primary_folder(),
                    words::browser::choose_primary_hint(),
                    Tone::Primary,
                ),
                |this, _, cx| this.choose_the_primary_folder(cx),
                cx,
            ),
        )
    }

    pub(crate) fn nothing_matched(
        &mut self,
        icon: Icon,
        message: &'static str,
        more: Option<&'static str>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(instead) = self.around_the_rows(cx).instead.clone() else {
            return match self.sung_offer(Tone::Outlined, cx) {
                Some(offer) => kit::empty_offering(icon, message, more, offer),
                None => empty(icon, message, more),
            };
        };
        let offered = instead.clone();

        kit::empty_offering(
            icon,
            message,
            more,
            self.in_the_pane_ring(
                kit::button(
                    "search-instead",
                    Some(Icon::Search),
                    words::browser::did_you_mean(instead.as_str()),
                    words::browser::instead_hint(),
                    Tone::Outlined,
                ),
                move |this, window, cx| {
                    this.search_instead(offered.clone(), window, cx);
                },
                cx,
            ),
        )
    }

    pub(crate) fn sung_offer(&self, tone: Tone, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let sung = self.around_the_rows(cx).sung.clone()?;
        let offered = sung.query;

        Some(self.in_the_pane_ring(
            kit::button(
                "search-the-lyrics",
                Some(Icon::Lyrics),
                words::browser::sung_in(sung.tracks),
                words::browser::sung_hint(),
                tone,
            ),
            move |this, window, cx| {
                this.search_instead(offered.clone(), window, cx);
            },
            cx,
        ))
    }
}
