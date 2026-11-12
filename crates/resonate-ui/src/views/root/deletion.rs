use gpui::{Context, Div, SharedString, div, prelude::*, px, rgb};

use super::{Deleting, RootView};
use crate::{
    icons::Icon,
    motion, theme,
    views::{
        kit::{self},
        sheets::{self},
        spoken::Spoken,
    },
    words,
};

impl RootView {
    pub(crate) fn ask_to_delete(&mut self, deleting: Deleting, cx: &mut Context<Self>) {
        self.close_the_menu(cx);
        self.deleting = Some(deleting);
        cx.notify();
    }

    pub(super) fn keep_the_track(&mut self, cx: &mut Context<Self>) {
        self.deleting = None;
        cx.notify();
    }

    pub(super) fn delete_the_track(&mut self, cx: &mut Context<Self>) {
        let Some(Deleting { tracks, title, .. }) = self.deleting.take() else {
            return;
        };
        self.library
            .update(cx, |library, cx| library.delete_tracks(tracks, title, cx));
        cx.notify();
    }

    pub(super) fn deletion_sheet(&self, deleting: &Deleting, cx: &mut Context<Self>) -> Div {
        let card = kit::sheet()
            .id("delete-sheet")
            .dialog(&deleting.asking())
            .track_focus(&self.sheet_panels.holding(sheets::DELETE_PANEL, cx))
            .w(px(theme::confirm_width()))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.keep_the_track(cx)))
            .child(kit::sheet_heading(
                "delete-sheet-asking",
                Icon::Delete,
                theme::failure(),
                deleting.asking(),
            ))
            .child(
                div()
                    .id("delete-sheet-saying")
                    .read_plainly_as(|| SharedString::from(deleting.saying()))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .text_size(px(theme::text_sm()))
                    .text_color(rgb(theme::muted()))
                    .child(SharedString::from(deleting.saying()))
                    .when(deleting.a_cut, |body| {
                        body.child(words::root::a_cut_goes_whole())
                    })
                    .child(
                        div()
                            .text_color(rgb(theme::text()))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(words::root::moved_to_the_trash()),
                    ),
            )
            .child(
                kit::sheet_actions()
                    .child(self.in_the_pane_ring(
                        kit::button(
                            "keep-the-track",
                            None,
                            words::root::cancel(),
                            words::root::keep_the_track_hint(key!(leave)),
                            kit::Tone::Outlined,
                        ),
                        |this, _, cx| this.keep_the_track(cx),
                        cx,
                    ))
                    .child(self.in_the_pane_ring(
                        kit::button(
                            "delete-the-track",
                            Some(Icon::Delete),
                            words::root::delete(),
                            words::root::delete_the_track_hint(),
                            kit::Tone::Destructive,
                        ),
                        |this, _, cx| this.delete_the_track(cx),
                        cx,
                    )),
            );

        kit::over_a_scrim(motion::lifted_in(card, "delete-sheet-arrives"))
    }
}
