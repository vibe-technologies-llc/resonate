use std::time::Duration;

use gpui::{Context, Div, SharedString, div, prelude::*, px, rgb};

use crate::{
    Notice, ResonateApp, clipboard, theme,
    together::{PartyReached, PartySeen, PartyStanding, SharedPerson},
    views::{
        kit::{self, EndsInAnEllipsis as _, Tone},
        root::RootView,
        settings::sync::scanned,
    },
};

const PANE_REDRAWN_EVERY: Duration = Duration::from_millis(500);

const BEGIN_HINT: &str = "Begin a party others can join by a link";

const LEAVE_HINT: &str = "Leave the party; your own queue comes back";

const END_HINT: &str = "End the party for everyone";

const COPY_HINT: &str = "Copy the link people join by";

const TURN_OUT_HINT: &str = "Take this person out of the party";

const PLAY_NEXT_HINT: &str = "Play this next, for everyone";

const DISMISS_HINT: &str = "Pass over this request";

const NO_PARTY: &str = "Listen together with friends. Everyone hears the same song at the same \
                        moment from their own copy or their own service, and no sound passes \
                        between you. To join someone's party, paste their link into the search \
                        field.";

const NO_SERVICE: &str = "Parties need the sync server, and this build or setting has none.";

const SUGGESTING: &str = "Add a song to your queue as you always do, and the host is asked to \
                          play it.";

const A_NAMELESS_PERSON: &str = "Someone";

const YOU: &str = "You";

const QR_SIDE: f32 = 180.0;

fn reach_said(reach: PartyReached) -> Option<&'static str> {
    match reach {
        PartyReached::Here => None,
        PartyReached::Connecting => Some("Reaching the server…"),
        PartyReached::Reconnecting => Some("Lost the server; trying again…"),
        PartyReached::Ended => Some("The party is over."),
        PartyReached::NoSuchParty => Some("That party has ended, or was never begun."),
        PartyReached::TurnedAway => Some("The server turned that link away."),
        PartyReached::ServerTooOld => Some("The server is too old to hold parties."),
    }
}

fn standing_said(standing: PartyStanding) -> &'static str {
    match standing {
        PartyStanding::WaitingForTheHost => "Waiting for the host to play something",
        PartyStanding::Finding => "Looking for this song",
        PartyStanding::NotHeld => "You don't have this song, so you sit it out",
        PartyStanding::CannotStartMidSong => {
            "This song can't start part way through here, so you join at the next"
        }
        PartyStanding::SatOut => "Paused here; press play to rejoin",
        PartyStanding::HostPaused => "The host has paused",
        PartyStanding::InStep => "In step with the host",
    }
}

fn person_said(person: &SharedPerson) -> SharedString {
    if person.me {
        return SharedString::new_static(YOU);
    }
    if person.name.is_empty() {
        return SharedString::new_static(A_NAMELESS_PERSON);
    }

    SharedString::from(person.name.clone())
}

fn said(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(theme::text_xs()))
        .text_color(rgb(theme::muted()))
        .child(text.into())
}

fn row_text(text: impl Into<SharedString>) -> Div {
    div()
        .flex_1()
        .min_w(px(0.0))
        .text_size(px(theme::text_base()))
        .text_color(rgb(theme::text()))
        .truncate()
        .ends_in_an_ellipsis()
        .child(text.into())
}

impl RootView {
    pub(crate) fn watch_the_party(&mut self, cx: &mut Context<Self>) {
        self.party_redraws = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(PANE_REDRAWN_EVERY).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }
        }));
    }

    pub(crate) fn party_pane(&mut self, cx: &mut Context<Self>) -> Div {
        if self.party_redraws.is_none() {
            self.watch_the_party(cx);
        }
        let together = cx.global::<ResonateApp>().together.clone();
        let seen = together.as_ref().and_then(|together| together.party());

        let body = match (together.is_some(), seen) {
            (false, _) => kit::section_body().child(said(NO_SERVICE)),
            (true, None) => self.nobody_here(cx),
            (true, Some(seen)) => self.at_the_party(&seen, cx),
        };

        kit::pane()
            .child(
                kit::heading()
                    .child(kit::eyebrow("COLLECTION"))
                    .child(kit::title("Listening party")),
            )
            .child(div().p_6().child(body))
    }

    fn nobody_here(&self, cx: &mut Context<Self>) -> Div {
        kit::section_body()
            .child(said(NO_PARTY))
            .child(div().flex().child(self.in_the_pane_ring(
                kit::button(
                    "party-begin",
                    None,
                    "Begin a party",
                    BEGIN_HINT,
                    Tone::Primary,
                ),
                |this, _, cx| this.begin_a_party(cx),
                cx,
            )))
    }

    fn at_the_party(&self, seen: &PartySeen, cx: &mut Context<Self>) -> Div {
        let mut body = kit::section_body();
        let hosting = seen.hosting;

        if let Some(reach) = reach_said(seen.reach) {
            body = body.child(said(reach));
        }
        if let Some((title, artist)) = &seen.song {
            body = body.child(row_text(format!("{title} · {artist}")));
        }
        if !hosting {
            body = body
                .child(said(standing_said(seen.standing)))
                .child(said(SUGGESTING));
        }
        if hosting {
            let link = seen.link.clone();
            let copied = link.clone();

            body = body
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .child(kit::figure(link.clone()).truncate().ends_in_an_ellipsis()),
                        )
                        .child(self.in_the_pane_ring(
                            kit::button("party-copy", None, "Copy", COPY_HINT, Tone::Outlined),
                            move |this, _, cx| {
                                clipboard::copy(copied.clone(), cx);
                                this.report(
                                    Notice::Done("The party's link is on the clipboard".to_owned()),
                                    cx,
                                );
                            },
                            cx,
                        )),
                )
                .child(div().size(px(QR_SIDE)).child(scanned(&link)));
        }

        let mut people = div().flex().flex_col().gap_1().child(kit::eyebrow("HERE"));
        for (at, person) in seen.people.iter().enumerate() {
            let id = person.id;
            people = people.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(row_text(person_said(person)))
                    .when(hosting && !person.me, |row| {
                        row.child(self.in_the_pane_ring(
                            kit::button(
                                ("party-turn-out", at),
                                None,
                                "Remove",
                                TURN_OUT_HINT,
                                Tone::Ghost,
                            ),
                            move |this, _, cx| this.turn_out_of_the_party(id, cx),
                            cx,
                        ))
                    }),
            );
        }
        body = body.child(people);

        if hosting && !seen.asked_for.is_empty() {
            let mut asked = div()
                .flex()
                .flex_col()
                .gap_1()
                .child(kit::eyebrow("ASKED FOR"));
            for (at, (title, artist)) in seen.asked_for.iter().enumerate() {
                asked = asked.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(row_text(format!("{title} · {artist}")))
                        .child(self.in_the_pane_ring(
                            kit::button(
                                ("party-play-next", at),
                                None,
                                "Play next",
                                PLAY_NEXT_HINT,
                                Tone::Outlined,
                            ),
                            move |this, _, cx| this.play_a_request_next(at, cx),
                            cx,
                        ))
                        .child(self.in_the_pane_ring(
                            kit::button(
                                ("party-dismiss", at),
                                None,
                                "Pass",
                                DISMISS_HINT,
                                Tone::Ghost,
                            ),
                            move |this, _, cx| this.dismiss_a_request(at, cx),
                            cx,
                        )),
                );
            }
            body = body.child(asked);
        }

        body.child(div().flex().child(self.in_the_pane_ring(
            kit::button(
                "party-leave",
                None,
                if hosting { "End the party" } else { "Leave" },
                if hosting { END_HINT } else { LEAVE_HINT },
                Tone::Destructive,
            ),
            |this, _, cx| this.leave_the_party(cx),
            cx,
        )))
    }

    fn begin_a_party(&mut self, cx: &mut Context<Self>) {
        let Some(together) = cx.global::<ResonateApp>().together.clone() else {
            return;
        };
        let shown = self.player.entity_id();
        let Some(zone) = self
            .zones
            .iter()
            .find(|zone| zone.player.entity_id() == shown)
            .map(|zone| zone.id)
        else {
            return;
        };
        let running = cx.background_spawn(async move { together.begin_party(zone) });

        cx.spawn(async move |this, cx| {
            let worked = running.await;
            let told = this.update(cx, |this, cx| {
                if !worked {
                    this.report(Notice::Trouble("Couldn't begin a party".to_owned()), cx);
                }
                cx.notify();
            });
            let _ = told;
        })
        .detach();
    }

    fn leave_the_party(&mut self, cx: &mut Context<Self>) {
        let Some(together) = cx.global::<ResonateApp>().together.clone() else {
            return;
        };

        cx.background_spawn(async move { together.leave_party() })
            .detach();
        cx.notify();
    }

    fn turn_out_of_the_party(&mut self, person: [u8; 16], cx: &mut Context<Self>) {
        if let Some(together) = cx.global::<ResonateApp>().together.clone() {
            together.turn_out(person);
        }
        cx.notify();
    }

    fn play_a_request_next(&mut self, at: usize, cx: &mut Context<Self>) {
        let Some(together) = cx.global::<ResonateApp>().together.clone() else {
            return;
        };
        let running = cx.background_spawn(async move { together.play_request_next(at) });

        cx.spawn(async move |this, cx| {
            let worked = running.await;
            let told = this.update(cx, |this, cx| {
                if !worked {
                    this.report(
                        Notice::Trouble("You don't have that song to play".to_owned()),
                        cx,
                    );
                }
                cx.notify();
            });
            let _ = told;
        })
        .detach();
    }

    fn dismiss_a_request(&mut self, at: usize, cx: &mut Context<Self>) {
        if let Some(together) = cx.global::<ResonateApp>().together.clone() {
            together.dismiss_request(at);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn being_at_the_party_says_nothing_and_anything_else_says_why() {
        assert_eq!(reach_said(PartyReached::Here), None);
        for reach in [
            PartyReached::Connecting,
            PartyReached::Reconnecting,
            PartyReached::Ended,
            PartyReached::NoSuchParty,
            PartyReached::TurnedAway,
            PartyReached::ServerTooOld,
        ] {
            assert!(reach_said(reach).is_some());
        }
    }
}
