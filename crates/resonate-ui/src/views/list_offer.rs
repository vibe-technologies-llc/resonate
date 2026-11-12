use gpui::{Context, Div, SharedString, div, prelude::*, px, rgb};

use crate::{
    ListOffered,
    icons::Icon,
    keys, motion, theme,
    views::{kit, root::RootView, sheets, spoken::Spoken as _},
    words,
};

impl ListOffered {
    pub(crate) fn asking(&self) -> &'static str {
        words::list_offer::asking()
    }

    pub(crate) fn saying(&self) -> String {
        words::list_offer::saying(self.server.as_str())
    }
}

impl RootView {
    pub(crate) fn offer_a_shared_list(&mut self, offered: ListOffered, cx: &mut Context<Self>) {
        self.close_the_menu(cx);
        self.social.list_offered = Some(offered);
        cx.notify();
    }

    pub(crate) fn not_joining_the_list(&mut self, cx: &mut Context<Self>) {
        self.social.list_offered = None;
        cx.notify();
    }

    fn join_the_offered_list(&mut self, cx: &mut Context<Self>) {
        let Some(offered) = self.social.list_offered.take() else {
            return;
        };
        cx.notify();
        self.accept_an_invitation(&offered.link, cx);
    }

    pub(crate) fn list_offer_sheet(&self, cx: &mut Context<Self>) -> Div {
        let Some(offered) = self.social.list_offered.as_ref() else {
            return div();
        };
        let asking = offered.asking();
        let saying = offered.saying();

        let card = kit::sheet()
            .id("list-offer-sheet")
            .dialog(asking)
            .track_focus(&self.sheet_panels.holding(sheets::LIST_OFFER_PANEL, cx))
            .w(px(theme::confirm_width()))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.not_joining_the_list(cx)))
            .child(kit::sheet_heading(
                "list-offer-asking",
                Icon::Share,
                theme::accent(),
                asking,
            ))
            .child(
                div()
                    .id("list-offer-saying")
                    .read_plainly_as({
                        let saying = saying.clone();
                        move || SharedString::from(saying)
                    })
                    .text_size(px(theme::text_sm()))
                    .text_color(rgb(theme::muted()))
                    .child(SharedString::from(saying)),
            )
            .child(
                kit::sheet_actions()
                    .child(self.in_the_pane_ring(
                        kit::button(
                            sheets::NOT_JOINING_THE_LIST,
                            None,
                            words::list_offer::cancel(),
                            keys::keyed(words::list_offer::not_joining(), &[key!(leave)]),
                            kit::Tone::Outlined,
                        ),
                        |this, _, cx| this.not_joining_the_list(cx),
                        cx,
                    ))
                    .child(self.in_the_pane_ring(
                        kit::button(
                            "join-the-list",
                            Some(Icon::Share),
                            words::list_offer::join(),
                            words::list_offer::join_hint(),
                            kit::Tone::Primary,
                        ),
                        |this, _, cx| this.join_the_offered_list(cx),
                        cx,
                    )),
            );

        kit::over_a_scrim(motion::lifted_in(card, "list-offer-arrives"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_names_the_server_it_asks_this_computer_to_reach() {
        let offered = ListOffered {
            link: "resonate://shared/E-ABCDE?server=wss%3A%2F%2Fstranger.example%2F".to_owned(),
            server: "wss://stranger.example/".to_owned(),
        };

        assert_eq!(offered.asking(), "Join a playlist on another server?");
        assert!(offered.saying().contains("wss://stranger.example/"));
    }
}
