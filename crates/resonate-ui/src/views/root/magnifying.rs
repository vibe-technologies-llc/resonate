use gpui::{Context, Div, ObjectFit, Stateful, Window, div, img, prelude::*, px, rgb, rgba};

use super::{Magnified, RootView};
use crate::{
    motion, theme,
    views::{
        kit::{self, Casts as _},
        sheets::MAGNIFIER_PANEL,
        spoken::Spoken,
    },
    words,
};

impl RootView {
    pub(crate) fn magnify(&mut self, magnified: Magnified, cx: &mut Context<Self>) {
        self.magnified = Some(magnified);
        cx.notify();
    }

    pub(super) fn shrink_cover(&mut self, cx: &mut Context<Self>) {
        self.magnified = None;
        cx.notify();
    }

    pub(super) fn magnifier(
        &self,
        magnified: &Magnified,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let art = self.magnified_art(magnified, cx);
        let title = self.magnified_title(magnified, cx);
        let side = theme::magnified_cover(window.viewport_size());

        div()
            .id("magnified-cover")
            .dialog(words::root::magnified_label())
            .track_focus(&self.sheet_panels.holding(MAGNIFIER_PANEL, cx))
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .bg(rgba(theme::scrim()))
            .occlude()
            .cursor_pointer()
            .when_some(art, |overlay, art| {
                overlay.child(motion::lifted_in(
                    div()
                        .size(side)
                        .rounded_xl()
                        .overflow_hidden()
                        .bg(rgb(theme::raised()))
                        .casting(kit::Cast::Long)
                        .child(img(art).size(side).object_fit(ObjectFit::Contain)),
                    "magnified-cover-arrives",
                ))
            })
            .when_some(title, |overlay, title| {
                overlay.child(
                    div()
                        .text_size(px(theme::text_lg()))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(rgb(theme::text()))
                        .child(title),
                )
            })
            .on_click(cx.listener(|this, _, _, cx| this.shrink_cover(cx)))
    }

    pub(crate) fn toggle_bitrate_graph(&mut self, cx: &mut Context<Self>) {
        self.bitrate_graph = !self.bitrate_graph;
        cx.notify();
    }
}
