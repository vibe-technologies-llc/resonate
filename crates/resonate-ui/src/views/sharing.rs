use std::{sync::Arc, time::Duration};

use gpui::{
    BoxShadow, Context, Div, FontWeight, SharedString, div, point, prelude::*, px, relative, rgb,
    rgba,
};
use resonate_core::PlaylistId;

use crate::{
    Notice, ResonateApp, clipboard, motion, theme,
    together::{SharedPerson, SharedReach, SharedSeen, Together},
    views::{
        kit::{self, EndsInAnEllipsis as _, Tone},
        root::RootView,
        settings::sync::scanned,
        sheets,
        spoken::Spoken,
    },
};

const SHARE_LABEL: &str = "Share this playlist";

const CLOSE_HINT: &str = keyed!("Close", key!(leave));

const SHARE_HINT: &str = "Make this playlist one other people can follow or edit with you";

const COPY_FOLLOW_HINT: &str = "Copy the link someone can follow this playlist by";

const COPY_EDIT_HINT: &str = "Copy the link someone can edit this playlist with";

const RESET_HINT: &str = "Make new links, so the ones given out reach nothing";

const LEAVE_HINT: &str = "Stop sharing this playlist here; its songs stay as your own list";

const REMOVE_HINT: &str = "Take this person off the list; everyone else moves to new links";

const PRESS_AGAIN: &str = "Press again";

const NOT_SHARED: &str = "Anyone with a link can follow it or edit it with you, with no account. \
                          The server keeps it sealed and cannot read it, and everyone plays their \
                          own copy of each song.";

const FOLLOW_LINK: &str = "Anyone with this link can follow";

const EDIT_LINK: &str = "Anyone with this link can edit";

const FOLLOWING: &str = "You follow this playlist. Whoever shared it decides what is on it.";

const NOT_SEEN_YET: &str = "This playlist could not be shared. Is Online on?";

const SCAN_TO_FOLLOW: &str = "Scan to follow";

const SCANNED_SIDE: f32 = 132.0;

const A_NAMELESS_PERSON: &str = "Someone";

const YOU: &str = "You";

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
        SharedReach::Waiting => Some("Reaching the server…"),
        SharedReach::Reached => None,
        SharedReach::Unreached => {
            Some("The server cannot be reached. Edits are kept and go when it can.")
        }
        SharedReach::ServerTooOld => Some("The server is too old to keep shared playlists."),
        SharedReach::TurnedAway => Some("The server turned this playlist away."),
        SharedReach::NoServer => {
            Some("This device is set to use no server, so nothing can be shared.")
        }
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

        let mut card = div()
            .id("sharing-sheet")
            .dialog(SHARE_LABEL)
            .track_focus(&self.sheet_panels.holding(sheets::SHARING_PANEL, cx))
            .flex()
            .flex_col()
            .gap_4()
            .w(px(theme::wrapped_width()))
            .max_h(relative(0.94))
            .overflow_y_scroll()
            .p_6()
            .rounded_xl()
            .bg(rgb(theme::surface()))
            .border_1()
            .border_color(rgb(theme::border()))
            .shadow(vec![BoxShadow {
                color: theme::shadow(0.5),
                offset: point(px(0.0), px(16.0)),
                blur_radius: px(48.0),
                spread_radius: px(0.0),
                inset: false,
            }])
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_sharing(cx)));

        card = match seen {
            Some(seen) => card.child(self.shared_body(&sharing, &seen, cx)),
            None => card.child(self.unshared_body(&sharing, together.is_some(), cx)),
        };

        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(theme::scrim()))
            .occlude()
            .child(motion::lifted_in(card, "sharing-sheet-arrives"))
    }

    fn unshared_body(&self, sharing: &Sharing, can_share: bool, cx: &mut Context<Self>) -> Div {
        let playlist = sharing.playlist;

        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(heading(SHARE_LABEL))
            .child(said(if can_share { NOT_SHARED } else { NOT_SEEN_YET }))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(self.in_the_pane_ring(
                        kit::button("sharing-close", None, "Close", CLOSE_HINT, Tone::Ghost),
                        |this, _, cx| this.close_sharing(cx),
                        cx,
                    ))
                    .when(can_share, |row| {
                        row.child(self.in_the_pane_ring(
                            kit::button("sharing-start", None, "Share", SHARE_HINT, Tone::Primary),
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
                    EDIT_LINK,
                    link,
                    COPY_EDIT_HINT,
                    cx,
                ));
            }
            body = body.child(self.link_row(
                "sharing-copy-follow",
                FOLLOW_LINK,
                seen.view_link.clone(),
                COPY_FOLLOW_HINT,
                cx,
            ));
        } else {
            body = body.child(said(FOLLOWING));
        }

        body = body
            .child(said(SCAN_TO_FOLLOW))
            .child(div().size(px(SCANNED_SIDE)).child(scanned(&seen.view_link)));

        if !seen.people.is_empty() {
            let mut people = div()
                .flex()
                .flex_col()
                .gap_1()
                .child(kit::eyebrow("PEOPLE"));
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
                format!("Not in your library: {named} and {more} more")
            } else {
                format!("Not in your library: {named}")
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
                                PRESS_AGAIN
                            } else {
                                "Make new links"
                            },
                            RESET_HINT,
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
                        if leave_armed { PRESS_AGAIN } else { "Leave" },
                        LEAVE_HINT,
                        Tone::Destructive,
                    ),
                    move |this, _, cx| this.press_leave(playlist, cx),
                    cx,
                ))
                .child(self.in_the_pane_ring(
                    kit::button("sharing-close", None, "Close", CLOSE_HINT, Tone::Ghost),
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
                kit::button(id, None, "Copy", hint, Tone::Outlined),
                move |this, _, cx| {
                    clipboard::copy(copied.clone(), cx);
                    this.report(Notice::Done("The link is on the clipboard".to_owned()), cx);
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
                        if armed { PRESS_AGAIN } else { "Remove" },
                        REMOVE_HINT,
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
            "Couldn't share this playlist",
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
            "Couldn't reach the server to make new links",
            Some("The old links reach nothing now"),
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
            "Couldn't leave this playlist",
            Some("You left; its songs stay here as your own playlist"),
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
            "Couldn't reach the server to take them off",
            Some("They no longer reach this playlist"),
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
                Notice::Trouble("This build shares nothing with other people".to_owned()),
                cx,
            );
            return;
        };
        let link = link.to_owned();

        run_it(
            cx,
            move || together.accept(&link),
            "That link could not be read",
            Some("Joined; it appears here once it arrives"),
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

fn heading(text: impl Into<SharedString>) -> impl IntoElement {
    let text = text.into();

    div()
        .id("sharing-heading")
        .headed(2, &text)
        .text_size(px(theme::text_lg()))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(theme::text()))
        .child(text)
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

        assert_eq!(person_said(&person("", false)), A_NAMELESS_PERSON);
        assert_eq!(person_said(&person("Ada", false)), "Ada");
        assert_eq!(person_said(&person("Ada", true)), YOU);
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
