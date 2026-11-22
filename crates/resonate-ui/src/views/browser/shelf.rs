use std::rc::Rc;

use gpui::{AnyElement, App, Context, Div, Stateful, div, prelude::*, px, rgb};
use resonate_library::{Album, Artist};

use super::{Chooses, SHELF_AT_MOST};
use crate::{
    theme,
    views::{
        kit,
        reorder::{Listed, Shift},
        root::RootView,
        scrollbar::{SHELF_INSET, Scrollbars},
    },
    words,
};

impl RootView {
    fn slid_into_view(
        &self,
        cell: Stateful<Div>,
        listed: Listed,
        at: usize,
        scroll: &gpui::ScrollHandle,
        cx: &App,
    ) -> AnyElement {
        if !self.reaches(Shift::Listing(listed), at, cx) {
            return cell.into_any_element();
        }

        div()
            .relative()
            .flex_none()
            .child(cell)
            .child(kit::brought_into_view_within(
                None,
                Some(scroll.clone()),
                Rc::clone(&self.rooms.reached_unseen),
            ))
            .into_any_element()
    }

    pub(crate) fn album_shelf(
        &self,
        id: &'static str,
        named: String,
        albums: &[Album],
        listed: Listed,
        cx: &mut Context<Self>,
    ) -> Div {
        let scroll = self.browsing.read(cx).shelf(id);
        let mut cells = Vec::new();

        for (at, album) in albums.iter().take(SHELF_AT_MOST).enumerate() {
            let cell = self.album_cell_at(
                album,
                theme::shelf_cover(),
                Some(Chooses::among(listed, at)),
                cx,
            );

            cells.push(self.slid_into_view(cell, listed, at, &scroll, cx));
        }

        shelf(id, named, albums.len(), cells, scroll, Scrollbars::of(cx))
    }

    pub(crate) fn artist_shelf_of(
        &self,
        id: &'static str,
        named: String,
        artists: &[Artist],
        listed: Listed,
        cx: &mut Context<Self>,
    ) -> Div {
        let scroll = self.browsing.read(cx).shelf(id);
        let mut cells = Vec::new();

        for (at, artist) in artists.iter().take(SHELF_AT_MOST).enumerate() {
            let cell = self.artist_cell_at(
                artist,
                theme::shelf_cover(),
                Some(Chooses::among(listed, at)),
                cx,
            );

            cells.push(self.slid_into_view(cell, listed, at, &scroll, cx));
        }

        shelf(id, named, artists.len(), cells, scroll, Scrollbars::of(cx))
    }
}

pub(super) fn shelf(
    id: &'static str,
    named: String,
    held: usize,
    cells: Vec<AnyElement>,
    scroll: gpui::ScrollHandle,
    bars: Scrollbars,
) -> Div {
    let drawn = cells.len();
    let eyebrow = if held > drawn {
        words::browser::shown(drawn, named)
    } else {
        named
    };
    let strip = div()
        .id(id)
        .flex()
        .flex_none()
        .items_start()
        .gap_4()
        .px(px(SHELF_INSET))
        .pb_3()
        .overflow_x_scroll()
        .track_scroll(&scroll)
        .children(cells);

    div()
        .flex()
        .flex_col()
        .flex_none()
        .border_b_1()
        .border_color(rgb(theme::border()))
        .child(div().px_6().pt_3().pb_2().child(kit::eyebrow(eyebrow)))
        .child(
            div()
                .relative()
                .child(strip)
                .child(bars.horizontal(id, scroll)),
        )
}
