use gpui::{Context, Div, SharedString, div, prelude::*, px, rgb};

use crate::{
    Notice, ResonateApp,
    icons::Icon,
    motion, theme,
    views::{
        kit,
        root::{Pane, RootView},
        sheets,
        spoken::Spoken as _,
    },
};

const ASKING: &str = "Join this listening party?";

const THIS_COMPUTER: &str = "this computer";

const NOT_JOINING_HINT: &str = keyed!("Stay as you are", key!(leave));

const JOIN_HINT: &str = "Stop what plays and follow the party instead";

const COULD_NOT_JOIN: &str = "That party could not be joined";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TakesOver {
    zone: Option<String>,
}

impl TakesOver {
    pub(crate) const fn of(zone: Option<String>) -> Self {
        Self { zone }
    }

    pub(crate) const fn asking(&self) -> &'static str {
        ASKING
    }

    pub(crate) fn saying(&self) -> String {
        let named = match &self.zone {
            Some(zone) => format!("the zone {zone}"),
            None => THIS_COMPUTER.to_owned(),
        };

        format!(
            "A link asks {named} to stop what it plays and follow someone's party instead. Join only if you were invited."
        )
    }
}

impl RootView {
    pub(crate) fn offer_a_party(&mut self, link: String, cx: &mut Context<Self>) {
        self.close_the_menu(cx);
        self.party_offered = Some(link);
        cx.notify();
    }

    pub(crate) fn not_joining_the_party(&mut self, cx: &mut Context<Self>) {
        self.party_offered = None;
        cx.notify();
    }

    fn join_the_offered_party(&mut self, cx: &mut Context<Self>) {
        let Some(link) = self.party_offered.take() else {
            return;
        };
        cx.notify();
        let Some(together) = cx.global::<ResonateApp>().together.clone() else {
            self.report(Notice::Trouble(COULD_NOT_JOIN.to_owned()), cx);
            return;
        };
        let zone = self.switcher.selected();
        let running = cx.background_spawn(async move { together.join_party(&link, zone) });

        cx.spawn(async move |this, cx| {
            let worked = running.await;
            let told = this.update(cx, |this, cx| {
                if worked {
                    this.set_pane(Pane::Party, cx);
                } else {
                    this.report(Notice::Trouble(COULD_NOT_JOIN.to_owned()), cx);
                }
                cx.notify();
            });
            let _ = told;
        })
        .detach();
    }

    pub(crate) fn party_offer_sheet(&self, cx: &mut Context<Self>) -> Div {
        let takes = TakesOver::of(self.switcher.label().map(|zone| zone.to_string()));
        let asking = takes.asking();
        let saying = takes.saying();

        let card = kit::sheet()
            .id("party-offer-sheet")
            .dialog(asking)
            .track_focus(&self.sheet_panels.holding(sheets::PARTY_OFFER_PANEL, cx))
            .w(px(theme::confirm_width()))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.not_joining_the_party(cx)))
            .child(kit::sheet_heading(
                "party-offer-asking",
                Icon::Party,
                theme::accent(),
                asking,
            ))
            .child(
                div()
                    .id("party-offer-saying")
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
                            sheets::NOT_JOINING,
                            None,
                            "Cancel",
                            NOT_JOINING_HINT,
                            kit::Tone::Outlined,
                        ),
                        |this, _, cx| this.not_joining_the_party(cx),
                        cx,
                    ))
                    .child(self.in_the_pane_ring(
                        kit::button(
                            "join-the-party",
                            Some(Icon::Party),
                            "Join",
                            JOIN_HINT,
                            kit::Tone::Primary,
                        ),
                        |this, _, cx| this.join_the_offered_party(cx),
                        cx,
                    )),
            );

        kit::over_a_scrim(motion::lifted_in(card, "party-offer-arrives"))
    }
}

#[cfg(test)]
pub(crate) mod joined {
    use std::sync::Arc;

    use parking_lot::Mutex;
    use resonate_core::{PlaylistId, ZoneId};

    use crate::together::{PartySeen, SharedSeen, Together};

    #[derive(Default)]
    pub(crate) struct Joining {
        pub(crate) joined: Mutex<Vec<(String, ZoneId)>>,
        pub(crate) refuses: bool,
    }

    impl Joining {
        pub(crate) fn refusing() -> Arc<Self> {
            Arc::new(Self {
                refuses: true,
                ..Self::default()
            })
        }
    }

    impl Together for Joining {
        fn seen(&self, _: PlaylistId) -> Option<SharedSeen> {
            None
        }

        fn shared_playlists(&self) -> Vec<(PlaylistId, bool)> {
            Vec::new()
        }

        fn share(&self, _: PlaylistId) -> bool {
            false
        }

        fn watch(&self, _: Option<PlaylistId>) {}

        fn leave(&self, _: PlaylistId) -> bool {
            false
        }

        fn reset(&self, _: PlaylistId, _: Option<[u8; 16]>) -> bool {
            false
        }

        fn accept(&self, _: &str) -> bool {
            false
        }

        fn join_party(&self, invitation: &str, zone: ZoneId) -> bool {
            self.joined.lock().push((invitation.to_owned(), zone));
            !self.refuses
        }

        fn named(&self, _: &str) {}

        fn party(&self) -> Option<PartySeen> {
            None
        }

        fn begin_party(&self, _: ZoneId) -> bool {
            false
        }

        fn leave_party(&self) {}

        fn turn_out(&self, _: [u8; 16]) {}

        fn play_request_next(&self, _: usize) -> bool {
            false
        }

        fn dismiss_request(&self, _: usize) {}

        fn following(&self, _: ZoneId) -> bool {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_names_what_it_takes_over_as_the_phones_do_and_the_zone_where_there_are_several() {
        let alone = TakesOver::of(None);
        let among = TakesOver::of(Some("Kitchen".to_owned()));

        assert_eq!(alone.asking(), "Join this listening party?");
        assert_eq!(
            alone.saying(),
            "A link asks this computer to stop what it plays and follow someone's party instead. Join only if you were invited."
        );
        assert!(
            among
                .saying()
                .starts_with("A link asks the zone Kitchen to stop")
        );
    }
}
