use std::{sync::Arc, time::Duration};

use gpui::{Context, Div, SharedString, Stateful, div, prelude::*, px, relative, rgb};
use resonate_core::PlaylistId;

use crate::{
    Notice, ResonateApp, clipboard,
    icons::Icon,
    keys, motion, theme,
    together::{SharedPerson, SharedReach, SharedSeen, Together},
    views::{
        kit::{self, EndsInAnEllipsis as _, Tone},
        root::RootView,
        settings::sync::scanned,
        sheets,
        spoken::Spoken,
    },
    words,
};

const SCANNED_SIDE: f32 = 132.0;

const SHEET_REDRAWN_EVERY: Duration = Duration::from_millis(1_000);

const UNHELD_NAMED_AT_MOST: usize = 5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Armed {
    Nothing,
    Reset,
    Leave,
    Person([u8; 16]),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Sharing {
    pub(crate) playlist: PlaylistId,
    pub(crate) armed: Armed,
}

fn reach_said(reach: SharedReach) -> Option<&'static str> {
    match reach {
        SharedReach::Waiting => Some(words::sharing::reaching_the_server()),
        SharedReach::Reached => None,
        SharedReach::Unreached => Some(words::sharing::unreached()),
        SharedReach::ServerTooOld => Some(words::sharing::server_too_old()),
        SharedReach::TurnedAway => Some(words::sharing::turned_away()),
        SharedReach::NoServer => Some(words::sharing::no_server()),
    }
}

fn person_said(person: &SharedPerson) -> SharedString {
    if person.me {
        return SharedString::new_static(words::sharing::you());
    }
    if person.name.is_empty() {
        return SharedString::new_static(words::sharing::a_nameless_person());
    }

    SharedString::from(person.name.clone())
}

impl RootView {
    pub(crate) fn open_sharing(&mut self, playlist: PlaylistId, cx: &mut Context<Self>) {
        self.sharing = Some(Sharing {
            playlist,
            armed: Armed::Nothing,
        });
        if let Some(together) = together_of(cx) {
            together.watch(Some(playlist));
        }
        self.sharing_redraws = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(SHEET_REDRAWN_EVERY).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }
        }));
        cx.notify();
    }

    pub(crate) fn close_sharing(&mut self, cx: &mut Context<Self>) {
        self.sharing = None;
        self.sharing_redraws = None;
        if let Some(together) = together_of(cx) {
            together.watch(None);
        }
        cx.notify();
    }

    pub(crate) fn sharing_sheet(&self, cx: &mut Context<Self>) -> Div {
        let Some(sharing) = self.sharing.clone() else {
            return div();
        };
        let together = together_of(cx);
        let seen = together
            .as_ref()
            .and_then(|together| together.seen(sharing.playlist));

        let mut card = kit::sheet()
            .id("sharing-sheet")
            .dialog(words::sharing::share_label())
            .track_focus(&self.sheet_panels.holding(sheets::SHARING_PANEL, cx))
            .w(px(theme::wrapped_width()))
            .max_h(relative(0.94))
            .overflow_y_scroll()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_sharing(cx)));

        card = match seen {
            Some(seen) => card.child(self.shared_body(&sharing, &seen, cx)),
            None => card.child(self.unshared_body(&sharing, together.is_some(), cx)),
        };

        kit::over_a_scrim(motion::lifted_in(card, "sharing-sheet-arrives"))
    }

    fn unshared_body(&self, sharing: &Sharing, can_share: bool, cx: &mut Context<Self>) -> Div {
        let playlist = sharing.playlist;

        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(heading(words::sharing::share_label()))
            .child(said(if can_share {
                words::sharing::not_shared()
            } else {
                words::sharing::not_seen_yet()
            }))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(self.in_the_pane_ring(
                        close_button(),
                        |this, _, cx| this.close_sharing(cx),
                        cx,
                    ))
                    .when(can_share, |row| {
                        row.child(self.in_the_pane_ring(
                            kit::button(
                                "sharing-start",
                                None,
                                words::sharing::share(),
                                words::sharing::share_hint(),
                                Tone::Primary,
                            ),
                            move |this, _, cx| this.begin_sharing(playlist, cx),
                            cx,
                        ))
                    }),
            )
    }

    fn shared_body(&self, sharing: &Sharing, seen: &SharedSeen, cx: &mut Context<Self>) -> Div {
        let playlist = sharing.playlist;
        let mut body = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(heading(seen.name.clone()));

        if let Some(reach) = reach_said(seen.reach) {
            body = body.child(said(reach));
        }

        if seen.may_edit {
            if let Some(link) = seen.edit_link.clone() {
                body = body.child(self.link_row(
                    "sharing-copy-edit",
                    words::sharing::edit_link(),
                    link,
                    words::sharing::copy_edit_hint(),
                    cx,
                ));
            }
            body = body.child(self.link_row(
                "sharing-copy-follow",
                words::sharing::follow_link(),
                seen.view_link.clone(),
                words::sharing::copy_follow_hint(),
                cx,
            ));
        } else {
            body = body.child(said(words::sharing::following()));
        }

        body = body
            .child(said(words::sharing::scan_to_follow()))
            .child(div().size(px(SCANNED_SIDE)).child(scanned(&seen.view_link)));

        if !seen.people.is_empty() {
            let mut people = div()
                .flex()
                .flex_col()
                .gap_1()
                .child(kit::eyebrow(words::sharing::people()));
            for (at, person) in seen.people.iter().enumerate() {
                people = people.child(self.person_row(at, person, sharing, seen.may_edit, cx));
            }
            body = body.child(people);
        }

        if !seen.unheld.is_empty() {
            let named = seen
                .unheld
                .iter()
                .take(UNHELD_NAMED_AT_MOST)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            let more = seen.unheld.len().saturating_sub(UNHELD_NAMED_AT_MOST);
            let line = if more > 0 {
                words::sharing::unheld_and_more(more, named)
            } else {
                words::sharing::unheld(named)
            };
            body = body.child(said(line));
        }

        let reset_armed = sharing.armed == Armed::Reset;
        let leave_armed = sharing.armed == Armed::Leave;

        body.child(
            div()
                .flex()
                .flex_wrap()
                .justify_end()
                .gap_2()
                .when(seen.may_edit, |row| {
                    row.child(self.in_the_pane_ring(
                        kit::button(
                            "sharing-reset",
                            None,
                            if reset_armed {
                                words::sharing::press_again()
                            } else {
                                words::sharing::make_new_links()
                            },
                            words::sharing::reset_hint(),
                            Tone::Ghost,
                        ),
                        move |this, _, cx| this.press_reset(playlist, cx),
                        cx,
                    ))
                })
                .child(self.in_the_pane_ring(
                    kit::button(
                        "sharing-leave",
                        None,
                        if leave_armed {
                            words::sharing::press_again()
                        } else {
                            words::sharing::leave()
                        },
                        words::sharing::leave_hint(),
                        Tone::Destructive,
                    ),
                    move |this, _, cx| this.press_leave(playlist, cx),
                    cx,
                ))
                .child(self.in_the_pane_ring(
                    close_button(),
                    |this, _, cx| this.close_sharing(cx),
                    cx,
                )),
        )
    }

    fn link_row(
        &self,
        id: &'static str,
        label: &'static str,
        link: String,
        hint: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        let copied = link.clone();

        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.0))
                    .child(kit::eyebrow(label))
                    .child(kit::figure(link).truncate().ends_in_an_ellipsis()),
            )
            .child(self.in_the_pane_ring(
                kit::button(id, None, words::sharing::copy(), hint, Tone::Outlined),
                move |this, _, cx| {
                    clipboard::copy(copied.clone(), cx);
                    this.report(Notice::Done(words::sharing::link_copied().to_owned()), cx);
                },
                cx,
            ))
    }

    fn person_row(
        &self,
        at: usize,
        person: &SharedPerson,
        sharing: &Sharing,
        may_edit: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let playlist = sharing.playlist;
        let id = person.id;
        let armed = sharing.armed == Armed::Person(id);
        let removable = may_edit && !person.me;

        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .text_size(px(theme::text_base()))
                    .text_color(rgb(theme::text()))
                    .truncate()
                    .ends_in_an_ellipsis()
                    .child(person_said(person)),
            )
            .when(removable, |row| {
                row.child(self.in_the_pane_ring(
                    kit::button(
                        ("sharing-remove", at),
                        None,
                        if armed {
                            words::sharing::press_again()
                        } else {
                            words::sharing::remove()
                        },
                        words::sharing::remove_hint(),
                        Tone::Ghost,
                    ),
                    move |this, _, cx| this.press_remove(playlist, id, cx),
                    cx,
                ))
            })
    }

    fn begin_sharing(&mut self, playlist: PlaylistId, cx: &mut Context<Self>) {
        let Some(together) = together_of(cx) else {
            return;
        };

        run_it(
            cx,
            move || together.share(playlist),
            words::sharing::could_not_share(),
            None,
        );
    }

    fn press_reset(&mut self, playlist: PlaylistId, cx: &mut Context<Self>) {
        if !self.arming(Armed::Reset, cx) {
            return;
        }
        let Some(together) = together_of(cx) else {
            return;
        };

        run_it(
            cx,
            move || together.reset(playlist, None),
            words::sharing::could_not_reset(),
            Some(words::sharing::reset()),
        );
    }

    fn press_leave(&mut self, playlist: PlaylistId, cx: &mut Context<Self>) {
        if !self.arming(Armed::Leave, cx) {
            return;
        }
        let Some(together) = together_of(cx) else {
            return;
        };

        self.close_sharing(cx);
        run_it(
            cx,
            move || together.leave(playlist),
            words::sharing::could_not_leave(),
            Some(words::sharing::left()),
        );
    }

    fn press_remove(&mut self, playlist: PlaylistId, person: [u8; 16], cx: &mut Context<Self>) {
        if !self.arming(Armed::Person(person), cx) {
            return;
        }
        let Some(together) = together_of(cx) else {
            return;
        };

        run_it(
            cx,
            move || together.reset(playlist, Some(person)),
            words::sharing::could_not_remove(),
            Some(words::sharing::removed()),
        );
    }

    fn arming(&mut self, wanted: Armed, cx: &mut Context<Self>) -> bool {
        let Some(sharing) = self.sharing.as_mut() else {
            return false;
        };
        if sharing.armed == wanted {
            sharing.armed = Armed::Nothing;
            return true;
        }

        sharing.armed = wanted;
        cx.notify();

        false
    }

    pub(crate) fn accept_an_invitation(&mut self, link: &str, cx: &mut Context<Self>) {
        let Some(together) = together_of(cx) else {
            self.report(
                Notice::Trouble(words::sharing::shares_nothing().to_owned()),
                cx,
            );
            return;
        };
        let link = link.to_owned();

        run_it(
            cx,
            move || together.accept(&link),
            words::sharing::could_not_accept(),
            Some(words::sharing::accepted()),
        );
    }
}

fn together_of(cx: &Context<RootView>) -> Option<Arc<dyn Together>> {
    cx.global::<ResonateApp>().together.clone()
}

fn run_it(
    cx: &mut Context<RootView>,
    asked: impl FnOnce() -> bool + Send + 'static,
    refused: &'static str,
    done: Option<&'static str>,
) {
    let running = cx.background_spawn(async move { asked() });

    cx.spawn(async move |this, cx| {
        let worked = running.await;
        let told = this.update(cx, |this, cx| {
            if !worked {
                this.report(Notice::Trouble(refused.to_owned()), cx);
            } else if let Some(done) = done {
                this.report(Notice::Done(done.to_owned()), cx);
            }
            cx.notify();
        });
        let _ = told;
    })
    .detach();
}

fn close_button() -> Stateful<Div> {
    kit::button(
        "sharing-close",
        None,
        words::sharing::close(),
        keys::keyed(words::sharing::close(), &[key!(leave)]),
        Tone::Ghost,
    )
}

fn heading(text: impl Into<SharedString>) -> impl IntoElement {
    kit::sheet_heading("sharing-heading", Icon::Share, theme::accent(), text)
}

fn said(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(theme::text_xs()))
        .text_color(rgb(theme::muted()))
        .child(text.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn someone_with_no_name_is_someone_and_you_are_you() {
        let person = |name: &str, me| SharedPerson {
            id: [0; 16],
            name: name.to_owned(),
            me,
        };

        assert_eq!(person_said(&person("", false)), "Someone");
        assert_eq!(person_said(&person("Ada", false)), "Ada");
        assert_eq!(person_said(&person("Ada", true)), "You");
    }

    #[test]
    fn being_up_to_date_says_nothing_and_everything_else_says_why() {
        assert_eq!(reach_said(SharedReach::Reached), None);
        for reach in [
            SharedReach::Waiting,
            SharedReach::Unreached,
            SharedReach::ServerTooOld,
            SharedReach::TurnedAway,
            SharedReach::NoServer,
        ] {
            assert!(reach_said(reach).is_some());
        }
    }
}
