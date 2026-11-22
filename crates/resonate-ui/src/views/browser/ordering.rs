use gpui::{Context, Div, Stateful};

use crate::{
    icons::Icon,
    views::{kit, root::RootView, sorting::Ordered},
    words,
};

impl RootView {
    pub(crate) fn orders_a_listing(
        &self,
        named: &'static str,
        ordered: Ordered,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        self.in_the_pane_ring_at(
            kit::icon_button(named, Icon::Sort, words::browser::order_hint()),
            move |this, at, cx| this.order_a_listing(ordered, at, cx),
            cx,
        )
    }

    pub(crate) fn tracks_in_order(&self, cx: &mut Context<Self>) -> Div {
        let sorting = self.library.read(cx).sorting();

        self.order_choices(
            sorting.tracks,
            sorting.tracks_read,
            |this, order, cx| {
                this.library
                    .update(cx, |library, cx| library.order_tracks(order, cx));
            },
            |this, reading, cx| {
                this.library
                    .update(cx, |library, cx| library.read_tracks(reading, cx));
            },
            cx,
        )
    }

    pub(crate) fn albums_in_order(&self, cx: &mut Context<Self>) -> Div {
        let sorting = self.library.read(cx).sorting();

        self.order_choices(
            sorting.albums,
            sorting.albums_read,
            |this, order, cx| {
                this.library
                    .update(cx, |library, cx| library.order_albums(order, cx));
            },
            |this, reading, cx| {
                this.library
                    .update(cx, |library, cx| library.read_albums(reading, cx));
            },
            cx,
        )
    }

    pub(crate) fn artists_in_order(&self, cx: &mut Context<Self>) -> Div {
        let sorting = self.library.read(cx).sorting();

        self.order_choices(
            sorting.artists,
            sorting.artists_read,
            |this, order, cx| {
                this.library
                    .update(cx, |library, cx| library.order_artists(order, cx));
            },
            |this, reading, cx| {
                this.library
                    .update(cx, |library, cx| library.read_artists(reading, cx));
            },
            cx,
        )
    }
}
