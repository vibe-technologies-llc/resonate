use std::time::Duration;

use gpui::{
    AnyElement, Context, Div, FontWeight, SharedString, Stateful, div, prelude::*, px, rgb,
};

use crate::{
    Notice, ResonateApp, clipboard,
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
    words,
};

const PANE_REDRAWN_EVERY: Duration = Duration::from_millis(500);

const QR_SIDE: f32 = 168.0;

const COVER_SIDE: f32 = 132.0;

const MARK_ICON: f32 = 34.0;

const WELCOME_WIDTH: f32 = 680.0;

const STEP_AT_LEAST: f32 = 170.0;

const COLUMN_AT_LEAST: f32 = 320.0;

fn reach_said(reach: PartyReached) -> Option<&'static str> {
    match reach {
        PartyReached::Here => None,
        PartyReached::Connecting => Some(words::party::reaching_the_server()),
        PartyReached::Reconnecting => Some(words::party::reconnecting()),
        PartyReached::Ended => Some(words::party::ended()),
        PartyReached::NoSuchParty => Some(words::party::no_such_party()),
        PartyReached::TurnedAway => Some(words::party::turned_away()),
        PartyReached::ServerTooOld => Some(words::party::server_too_old()),
    }
}

fn standing_said(standing: PartyStanding) -> &'static str {
    match standing {
        PartyStanding::WaitingForTheHost => words::party::waiting_for_the_host(),
        PartyStanding::Finding => words::party::finding(),
        PartyStanding::NotHeld => words::party::not_held(),
        PartyStanding::CannotStartMidSong => words::party::cannot_start_mid_song(),
        PartyStanding::SatOut => words::party::sat_out(),
        PartyStanding::HostPaused => words::party::host_paused(),
        PartyStanding::InStep => words::party::in_step(),
    }
}

fn what_happens() -> [(Icon, &'static str, &'static str); 3] {
    [
        (
            Icon::Play,
            words::party::you_play(),
            words::party::you_play_more(),
        ),
        (
            Icon::Share,
            words::party::they_join(),
            words::party::they_join_more(),
        ),
        (
            Icon::Party,
            words::party::everyone_hears(),
            words::party::everyone_hears_more(),
        ),
    ]
}

fn person_said(person: &SharedPerson) -> SharedString {
    if person.me {
        return SharedString::new_static(words::party::you());
    }
    if person.name.is_empty() {
        return SharedString::new_static(words::party::a_nameless_person());
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
            (false, _) => kit::empty(
                Icon::Party,
                words::party::no_service(),
                Some(words::party::no_service_more()),
            ),
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
            None => words::party::about().to_owned(),
            Some(seen) => words::party::people_here(seen.people.len()),
        };
        let leaving = seen.map(|seen| {
            self.in_the_pane_ring(
                kit::button(
                    "party-leave",
                    None,
                    if seen.hosting {
                        words::party::end_the_party()
                    } else {
                        words::party::leave()
                    },
                    if seen.hosting {
                        words::party::end_hint()
                    } else {
                        words::party::leave_hint()
                    },
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
                        .child(kit::title(words::party::listening_party()))
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
                    .headed(2, words::party::welcome_title())
                    .pt_2()
                    .text_size(px(theme::text_title()))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(theme::text()))
                    .child(words::party::welcome_title()),
            )
            .child(
                said(words::party::welcome())
                    .text_size(px(theme::text_base()))
                    .text_center(),
            );
        let begin = self.in_the_pane_ring(
            kit::button(
                "party-begin",
                Some(Icon::Party),
                words::party::begin(),
                words::party::begin_hint(),
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
                    .child(words::party::joining()),
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
                            .children(what_happens().map(step_card)),
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
                words::party::nothing_yet().to_owned(),
                if seen.hosting {
                    words::party::host_plays().to_owned()
                } else {
                    standing_said(PartyStanding::WaitingForTheHost).to_owned()
                },
            )
        });
        let (shade, standing) = if seen.hosting {
            (theme::done(), words::party::hosting())
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
                    .child(kit::eyebrow(words::party::now_playing()))
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
                                    kit::figure(SharedString::from(words::party::round_trip(
                                        round_trip.as_millis().max(1).to_string(),
                                    )))
                                    .text_color(rgb(theme::faint())),
                                )
                            }),
                    )
                    .when(!seen.hosting, |column| {
                        column.child(
                            said(words::party::suggesting())
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

        card(words::party::invite_people()).child(
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
                        .child(said(words::party::inviting()))
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
                            kit::button(
                                "party-copy",
                                None,
                                words::party::copy_link(),
                                words::party::copy_hint(),
                                Tone::Outlined,
                            ),
                            move |this, _, cx| {
                                clipboard::copy(copied.clone(), cx);
                                this.report(
                                    Notice::Done(words::party::link_copied().to_owned()),
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
                            kit::icon_button(
                                ("party-turn-out", at),
                                Icon::Close,
                                words::party::turn_out_hint(),
                            ),
                            move |this, _, cx| this.turn_out_of_the_party(id, cx),
                            cx,
                        ))
                    }),
            );
        }

        card(words::party::here()).child(people)
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
                            words::party::play_next(),
                            words::party::play_next_hint(),
                            Tone::Outlined,
                        ),
                        move |this, _, cx| this.play_a_request_next(at, cx),
                        cx,
                    ))
                    .child(self.in_the_pane_ring(
                        kit::icon_button(
                            ("party-dismiss", at),
                            Icon::Close,
                            words::party::dismiss_hint(),
                        ),
                        move |this, _, cx| this.dismiss_a_request(at, cx),
                        cx,
                    )),
            );
        }

        card(words::party::asked_for()).child(asked)
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
                    this.report(
                        Notice::Trouble(words::party::could_not_begin().to_owned()),
                        cx,
                    );
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
                        Notice::Trouble(words::party::not_held_to_play().to_owned()),
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
