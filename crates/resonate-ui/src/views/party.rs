use std::time::Duration;

use gpui::{
    AnyElement, Context, Div, FontWeight, SharedString, Stateful, div, prelude::*, px, rgb,
};

use crate::{
    Notice, ResonateApp, clipboard, format,
    icons::{self, Icon},
    theme,
    together::{PartyReached, PartySeen, PartyStanding, SharedPerson},
    views::{
        kit::{self, EndsInAnEllipsis as _, Tone},
        root::RootView,
        scrollbar::Scrollbars,
        settings::sync::scanned,
        spoken::Spoken as _,
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

const WELCOME_TITLE: &str = "Listen together";

const WELCOME: &str = "Friends anywhere hear the same song at the same moment.";

const BEGIN: &str = "Start a listening party";

const JOINING: &str = "Have a link? Paste it into the search field to join.";

const WHAT_HAPPENS: [(Icon, &str, &str); 3] = [
    (
        Icon::Play,
        "You play",
        "Whatever you queue is what the party hears.",
    ),
    (
        Icon::Share,
        "They join",
        "Send the link or show the code. No account is needed.",
    ),
    (
        Icon::Party,
        "Everyone hears it",
        "Each from their own copy or service. No sound passes between you.",
    ),
];

const NO_SERVICE: &str = "Parties need the sync server";

const NO_SERVICE_MORE: &str =
    "This build has none, or Online is off in the settings. Turn it on to listen together.";

const SUGGESTING: &str = "Add a song to your queue as you always do, and the host is asked to \
                          play it.";

const INVITING: &str = "Scan this with a phone or send the link. Anyone holding it can join.";

const NOTHING_YET: &str = "Nothing playing yet";

const HOST_PLAYS: &str = "Play something and everyone hears it.";

const HOSTING: &str = "You're hosting";

const A_NAMELESS_PERSON: &str = "Someone";

const YOU: &str = "You";

const QR_SIDE: f32 = 168.0;

const COVER_SIDE: f32 = 132.0;

const MARK_ICON: f32 = 34.0;

const WELCOME_WIDTH: f32 = 680.0;

const STEP_AT_LEAST: f32 = 170.0;

const COLUMN_AT_LEAST: f32 = 320.0;

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

fn standing_shade(standing: PartyStanding) -> u32 {
    match standing {
        PartyStanding::InStep => theme::done(),
        PartyStanding::Finding => theme::accent(),
        PartyStanding::NotHeld | PartyStanding::CannotStartMidSong => theme::dithered(),
        PartyStanding::WaitingForTheHost | PartyStanding::SatOut | PartyStanding::HostPaused => {
            theme::faint()
        }
    }
}

const fn reach_is_the_end(reach: PartyReached) -> bool {
    match reach {
        PartyReached::Here | PartyReached::Connecting | PartyReached::Reconnecting => false,
        PartyReached::Ended
        | PartyReached::NoSuchParty
        | PartyReached::TurnedAway
        | PartyReached::ServerTooOld => true,
    }
}

fn said(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(theme::text_sm()))
        .text_color(rgb(theme::muted()))
        .child(text.into())
}

fn card(name: impl Into<SharedString>) -> Div {
    kit::section().child(kit::section_header().child(kit::section_name(name)))
}

fn song_lines(title: impl Into<SharedString>, artist: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.0))
        .child(
            div()
                .text_size(px(theme::text_sm()))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(theme::text()))
                .truncate()
                .ends_in_an_ellipsis()
                .child(title.into()),
        )
        .child(
            div()
                .text_size(px(theme::text_xs()))
                .text_color(rgb(theme::muted()))
                .truncate()
                .ends_in_an_ellipsis()
                .child(artist.into()),
        )
}

fn no_art() -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(COVER_SIDE))
        .rounded_lg()
        .bg(rgb(theme::raised()))
        .border_1()
        .border_color(rgb(theme::border()))
        .child(icons::icon(Icon::Disc, MARK_ICON, theme::faint()))
}

fn step_card((icon, name, what): (Icon, &'static str, &'static str)) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .gap_1p5()
        .min_w(px(STEP_AT_LEAST))
        .p_4()
        .rounded_lg()
        .bg(rgb(theme::surface()))
        .border_1()
        .border_color(rgb(theme::border()))
        .child(icons::icon(
            icon,
            theme::row_control_icon(),
            theme::accent(),
        ))
        .child(
            div()
                .text_size(px(theme::text_sm()))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(theme::text()))
                .child(name),
        )
        .child(
            div()
                .text_size(px(theme::text_xs()))
                .text_color(rgb(theme::muted()))
                .child(what),
        )
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
        let heading = self.party_heading(seen.as_ref(), cx);

        let body: AnyElement = match (together.is_some(), &seen) {
            (false, _) => kit::empty(Icon::Party, NO_SERVICE, Some(NO_SERVICE_MORE)),
            (true, None) => self.party_page(self.nobody_here(cx), cx),
            (true, Some(seen)) => self.party_page(self.at_the_party(seen, cx), cx),
        };

        kit::pane().child(heading).child(body)
    }

    fn party_page(&self, page: Stateful<Div>, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .child(Scrollbars::of(cx).around(
                "party-scrollbar",
                self.party_scroll.clone(),
                page.overflow_y_scroll().track_scroll(&self.party_scroll),
            ))
            .into_any_element()
    }

    fn party_heading(&self, seen: Option<&PartySeen>, cx: &mut Context<Self>) -> Div {
        let under = match seen {
            None => "Hear the same song at the same moment as your friends".to_owned(),
            Some(seen) => format::counted(seen.people.len(), "person here", "people here"),
        };
        let leaving = seen.map(|seen| {
            self.in_the_pane_ring(
                kit::button(
                    "party-leave",
                    None,
                    if seen.hosting {
                        "End the party"
                    } else {
                        "Leave"
                    },
                    if seen.hosting { END_HINT } else { LEAVE_HINT },
                    Tone::Destructive,
                ),
                |this, _, cx| this.leave_the_party(cx),
                cx,
            )
        });

        kit::heading().child(
            kit::heading_row()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(theme::heading_name()))
                        .gap_1()
                        .child(kit::title("Listening Party"))
                        .child(kit::subtitle(under)),
                )
                .child(kit::actions().children(leaving)),
        )
    }

    fn nobody_here(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let mark = kit::empty_mark(Icon::Party, theme::accent());
        let welcome = div()
            .flex()
            .flex_col()
            .items_center()
            .gap_2()
            .child(mark)
            .child(
                div()
                    .id("party-welcome")
                    .headed(2, WELCOME_TITLE)
                    .pt_2()
                    .text_size(px(theme::text_title()))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(theme::text()))
                    .child(WELCOME_TITLE),
            )
            .child(
                said(WELCOME)
                    .text_size(px(theme::text_base()))
                    .text_center(),
            );
        let begin = self.in_the_pane_ring(
            kit::button(
                "party-begin",
                Some(Icon::Party),
                BEGIN,
                BEGIN_HINT,
                Tone::Primary,
            ),
            |this, _, cx| this.begin_a_party(cx),
            cx,
        );
        let joining = div()
            .flex()
            .items_center()
            .gap_1p5()
            .child(icons::icon(
                Icon::Search,
                theme::row_control_icon(),
                theme::faint(),
            ))
            .child(
                div()
                    .text_size(px(theme::text_sm()))
                    .text_color(rgb(theme::faint()))
                    .child(JOINING),
            );

        div()
            .id("party-page")
            .flex()
            .flex_col()
            .flex_1()
            .items_center()
            .justify_center()
            .min_w(px(0.0))
            .px_6()
            .py_8()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_6()
                    .w_full()
                    .max_w(px(WELCOME_WIDTH))
                    .child(welcome)
                    .child(begin)
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_3()
                            .w_full()
                            .children(WHAT_HAPPENS.map(step_card)),
                    )
                    .child(joining),
            )
    }

    fn at_the_party(&self, seen: &PartySeen, cx: &mut Context<Self>) -> Stateful<Div> {
        let main = div()
            .flex()
            .flex_col()
            .flex_1()
            .gap_4()
            .min_w(px(COLUMN_AT_LEAST))
            .child(self.party_song(seen, cx))
            .when(seen.hosting && !seen.asked_for.is_empty(), |column| {
                column.child(self.party_requests(seen, cx))
            });
        let beside = div()
            .flex()
            .flex_col()
            .flex_1()
            .gap_4()
            .min_w(px(COLUMN_AT_LEAST))
            .when(seen.hosting, |column| {
                column.child(self.party_invitation(&seen.link, cx))
            })
            .child(self.party_people(seen, cx));

        div()
            .id("party-page")
            .flex()
            .flex_col()
            .flex_1()
            .gap_4()
            .min_w(px(0.0))
            .p_6()
            .when_some(reach_said(seen.reach), |page, reach| {
                let shade = if reach_is_the_end(seen.reach) {
                    theme::failure()
                } else {
                    theme::muted()
                };

                page.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_4()
                        .py_2p5()
                        .rounded_lg()
                        .bg(theme::tinted(shade, 0x14))
                        .border_1()
                        .border_color(theme::tinted(shade, 0x40))
                        .text_size(px(theme::text_sm()))
                        .text_color(rgb(theme::text()))
                        .child(icons::icon(Icon::Alert, theme::row_control_icon(), shade))
                        .child(reach),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_start()
                    .gap_4()
                    .child(main)
                    .child(beside),
            )
    }

    fn party_song(&self, seen: &PartySeen, cx: &mut Context<Self>) -> Div {
        let shown_is_the_partys = self
            .zones
            .iter()
            .find(|zone| zone.id == seen.zone)
            .is_some_and(|zone| zone.player.entity_id() == self.player.entity_id());
        let heard_here = seen.hosting || seen.standing == PartyStanding::InStep;
        let art = (shown_is_the_partys && heard_here && seen.song.is_some())
            .then(|| self.art_of_what_plays(COVER_SIDE, cx))
            .flatten()
            .unwrap_or_else(no_art);
        let (title, artist) = seen.song.clone().unwrap_or_else(|| {
            (
                NOTHING_YET.to_owned(),
                if seen.hosting {
                    HOST_PLAYS.to_owned()
                } else {
                    standing_said(PartyStanding::WaitingForTheHost).to_owned()
                },
            )
        });
        let (shade, standing) = if seen.hosting {
            (theme::done(), HOSTING)
        } else {
            (standing_shade(seen.standing), standing_said(seen.standing))
        };

        kit::section().child(
            div().flex().items_center().gap_4().p_4().child(art).child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .gap_1()
                    .min_w(px(0.0))
                    .child(kit::eyebrow("Now playing"))
                    .child(
                        div()
                            .text_size(px(theme::text_lg()))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(theme::text()))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(title),
                    )
                    .child(said(artist).truncate().ends_in_an_ellipsis())
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_x_3()
                            .gap_y_1()
                            .pt_2()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1p5()
                                    .child(kit::mode_dot(shade))
                                    .child(said(standing).text_size(px(theme::text_xs()))),
                            )
                            .when_some(seen.round_trip, |line, round_trip| {
                                line.child(
                                    kit::figure(SharedString::from(format!(
                                        "{} ms round trip",
                                        round_trip.as_millis().max(1)
                                    )))
                                    .text_color(rgb(theme::faint())),
                                )
                            }),
                    )
                    .when(!seen.hosting, |column| {
                        column.child(
                            said(SUGGESTING)
                                .pt_1()
                                .text_size(px(theme::text_xs()))
                                .text_color(rgb(theme::faint())),
                        )
                    }),
            ),
        )
    }

    fn party_invitation(&self, link: &str, cx: &mut Context<Self>) -> Div {
        let copied = link.to_owned();

        card("Invite people").child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_4()
                .p_4()
                .child(div().flex_none().size(px(QR_SIDE)).child(scanned(link)))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .gap_3()
                        .min_w(px(STEP_AT_LEAST))
                        .child(said(INVITING))
                        .child(
                            div()
                                .px_2p5()
                                .py_1p5()
                                .rounded_md()
                                .bg(rgb(theme::raised()))
                                .border_1()
                                .border_color(rgb(theme::border()))
                                .child(
                                    kit::figure(link.to_owned())
                                        .truncate()
                                        .ends_in_an_ellipsis(),
                                ),
                        )
                        .child(div().flex().child(self.in_the_pane_ring(
                            kit::button("party-copy", None, "Copy link", COPY_HINT, Tone::Outlined),
                            move |this, _, cx| {
                                clipboard::copy(copied.clone(), cx);
                                this.report(
                                    Notice::Done("The party's link is on the clipboard".to_owned()),
                                    cx,
                                );
                            },
                            cx,
                        ))),
                ),
        )
    }

    fn party_people(&self, seen: &PartySeen, cx: &mut Context<Self>) -> Div {
        let mut people = div().flex().flex_col().gap_1().p_2();
        for (at, person) in seen.people.iter().enumerate() {
            let id = person.id;
            let name = person_said(person);

            people = people.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2p5()
                    .h(px(theme::row_height()))
                    .px_2()
                    .child(kit::avatar(&name, person.me))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .text_size(px(theme::text_sm()))
                            .text_color(rgb(theme::text()))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(name),
                    )
                    .when(seen.hosting && !person.me, |row| {
                        row.child(self.in_the_pane_ring(
                            kit::icon_button(("party-turn-out", at), Icon::Close, TURN_OUT_HINT),
                            move |this, _, cx| this.turn_out_of_the_party(id, cx),
                            cx,
                        ))
                    }),
            );
        }

        card("Here").child(people)
    }

    fn party_requests(&self, seen: &PartySeen, cx: &mut Context<Self>) -> Div {
        let mut asked = div().flex().flex_col().gap_1().p_2();
        for (at, (title, artist)) in seen.asked_for.iter().enumerate() {
            asked = asked.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .child(song_lines(title.clone(), artist.clone()))
                    .child(self.in_the_pane_ring(
                        kit::button(
                            ("party-play-next", at),
                            Some(Icon::QueueNext),
                            "Play next",
                            PLAY_NEXT_HINT,
                            Tone::Outlined,
                        ),
                        move |this, _, cx| this.play_a_request_next(at, cx),
                        cx,
                    ))
                    .child(self.in_the_pane_ring(
                        kit::icon_button(("party-dismiss", at), Icon::Close, DISMISS_HINT),
                        move |this, _, cx| this.dismiss_a_request(at, cx),
                        cx,
                    )),
            );
        }

        card("Asked for").child(asked)
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

    #[test]
    fn a_party_that_can_come_back_is_not_told_as_over() {
        assert!(!reach_is_the_end(PartyReached::Connecting));
        assert!(!reach_is_the_end(PartyReached::Reconnecting));
        assert!(reach_is_the_end(PartyReached::Ended));
        assert!(reach_is_the_end(PartyReached::TurnedAway));
    }
}
