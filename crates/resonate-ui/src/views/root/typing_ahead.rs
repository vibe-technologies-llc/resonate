use std::time::Instant;

use gpui::{
    App, Context, Div, Entity, KeyDownEvent, SharedString, Task, Window, div, prelude::*, px, rgb,
};
use resonate_engine::Command;

use super::RootView;
use crate::{
    motion, theme,
    toast::{self},
    views::{
        field::Field,
        kit::{self, Casts as _},
        queue::Named,
        reorder::Shift,
        typing::{self, jumped},
    },
    words,
};

impl RootView {
    pub(super) fn typed(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.control || keystroke.modifiers.platform || keystroke.modifiers.alt {
            return;
        }
        if self.editing(window, cx) {
            return;
        }

        match keystroke.key.as_str() {
            "escape" => self.escaped(window, cx),
            _ if self.something_stands_over_the_pane() => {}
            "backspace" => {
                if !self.drop_typed(cx)
                    && !self.stands_where_a_jump_landed(cx)
                    && !self.drop_reached(cx)
                {
                    self.search.update(cx, |search, cx| search.drop_last(cx));
                }
            }
            _ => {
                let Some(typed) = keystroke
                    .key_char
                    .as_deref()
                    .filter(|key| *key != " " && !key.contains(char::is_control))
                else {
                    return;
                };
                self.type_where_typing_goes(typed, window, cx);
            }
        }
        cx.notify();
    }

    pub(super) fn jumping(&mut self, cx: &mut Context<Self>) -> Option<(Shift, usize)> {
        let (shift, held) = self.reachable(cx)?;
        match shift {
            Shift::Queue => {}
            Shift::Playlist(_) | Shift::Listing(_) => return None,
        }
        let from = self
            .reach_in(shift, held)
            .map_or_else(|| self.opening_row(shift, held, cx), |reach| reach.row);

        Some((shift, from))
    }

    pub(crate) fn jump_where_typed(&mut self, cx: &mut Context<Self>) {
        if !self.type_ahead.is_live() {
            return;
        }
        let Some((shift, from)) = self.jumping(cx) else {
            return;
        };
        if let Some(names) = self.names_in_the_queue(cx) {
            self.jumped_to(shift, from, &names, cx);
        }
    }

    pub(super) fn type_where_typing_goes(
        &mut self,
        typed: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.typed_ahead(typed, cx) {
            self.search.update(cx, |search, cx| {
                search.take_focus(window, cx);
                search.append(typed, cx);
            });
        }
        self.last_typed = Some(Instant::now());
    }

    pub(super) fn is_typing(&self) -> bool {
        self.type_ahead.is_live()
            || self
                .last_typed
                .is_some_and(|typed| typed.elapsed() < typing::HELD_FOR)
    }

    pub(super) fn play_pause_or_type_a_space(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_typing() {
            self.type_where_typing_goes(" ", window, cx);
            cx.notify();
        } else {
            self.send(Command::TogglePlayPause, cx);
        }
    }

    pub(super) fn typed_ahead(&mut self, letter: &str, cx: &mut Context<Self>) -> bool {
        if self.jumping(cx).is_none() {
            return false;
        }

        self.type_ahead.took(letter, Instant::now());
        self.jump_where_typed(cx);
        self.stops_typing_soon(cx);
        true
    }

    pub(super) fn drop_typed(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.type_ahead.is_live() {
            return false;
        }

        self.type_ahead.dropped_a_letter(Instant::now());
        self.jump_where_typed(cx);
        self.stops_typing_soon(cx);
        true
    }

    pub(super) fn jumped_to(
        &mut self,
        shift: Shift,
        from: usize,
        names: &[Named],
        cx: &mut Context<Self>,
    ) {
        let Some(typed) = self.type_ahead.typed() else {
            return;
        };
        let landing = jumped(names, from, typed);

        self.type_ahead.landed(landing.is_some());
        let Some(row) = landing else {
            return;
        };
        self.reach_at(shift, row, false, cx);
        self.landed_by_a_jump.clone_from(&self.reach);
        self.show_row(shift, row, cx);
    }

    pub(super) fn stops_typing_soon(&mut self, cx: &mut Context<Self>) {
        self.typing_stops = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(typing::HELD_FOR).await;
            let stopped = this.update(cx, |this, cx| {
                if this.type_ahead.ran_out_by(Instant::now()) {
                    this.stop_typing(cx);
                }
            });
            let _ = stopped;
        });
    }

    pub(super) fn escaped(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.close_what_stands_over_the_pane(window, cx) {
            return;
        }
        if self.downloads_open {
            self.close_the_downloads(cx);
        } else if Self::noticed(cx) {
            toast::dismiss(cx);
        } else if !self.stop_typing(cx) {
            self.dismiss_search(window, cx);
        }
    }

    pub(super) fn close_what_stands_over_the_pane(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.deleting.is_some() {
            self.keep_the_track(cx);
        } else if self.matching.is_some() {
            self.leave_the_matching(window, cx);
        } else if self.party_offered.is_some() {
            self.not_joining_the_party(cx);
        } else if self.list_offered.is_some() {
            self.not_joining_the_list(cx);
        } else if self.listening_open {
            self.close_the_listener(cx);
        } else if self.magnified.is_some() {
            self.shrink_cover(cx);
        } else if self.adding.is_some() {
            self.stop_naming(window, cx);
        } else if self.record.is_some() {
            self.record = None;
            cx.notify();
        } else if self.wrapped.is_some() {
            self.close_wrapped(cx);
        } else if self.sharing.is_some() {
            self.close_sharing(cx);
        } else if self.ordering.is_some() {
            self.close_the_order(cx);
        } else if self.menu.is_some() {
            if !self.back_out_of_the_menu(cx) {
                self.close_the_menu(cx);
            }
        } else {
            return false;
        }
        true
    }

    pub(crate) const fn something_stands_over_the_pane(&self) -> bool {
        self.menu.is_some()
            || self.adding.is_some()
            || self.magnified.is_some()
            || self.record.is_some()
            || self.wrapped.is_some()
            || self.sharing.is_some()
            || self.ordering.is_some()
            || self.listening_open
            || self.deleting.is_some()
            || self.matching.is_some()
            || self.party_offered.is_some()
            || self.list_offered.is_some()
    }

    pub(super) fn reach_by_hand(&mut self, cx: &mut Context<Self>) {
        self.stop_typing(cx);
        self.landed_by_a_jump = None;
    }

    pub(super) fn stands_where_a_jump_landed(&self, cx: &App) -> bool {
        let Some(landed) = self.landed_by_a_jump.as_ref() else {
            return false;
        };

        self.reach.as_ref() == Some(landed)
            && self
                .reachable(cx)
                .is_some_and(|(shift, _)| shift == landed.shift)
    }

    pub(super) fn stop_typing(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.type_ahead.cleared() {
            return false;
        }
        self.typing_stops = Task::ready(());
        cx.notify();
        true
    }

    pub(super) fn type_ahead_pill(&self) -> Option<Div> {
        let typed = SharedString::from(self.type_ahead.typed()?.to_owned());
        let says = if self.type_ahead.found() {
            words::root::jumped_to()
        } else {
            words::root::jumped_nowhere()
        };

        Some(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(px(theme::transport_height() + theme::type_ahead_lift()))
                .flex()
                .justify_center()
                .child(motion::lifted_in(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .py_1p5()
                        .rounded_full()
                        .bg(rgb(theme::raised()))
                        .border_1()
                        .border_color(rgb(theme::soft_edge()))
                        .casting(kit::Cast::Slight)
                        .child(kit::eyebrow(says))
                        .child(
                            div()
                                .text_size(px(theme::text_sm()))
                                .text_color(rgb(theme::text()))
                                .whitespace_nowrap()
                                .child(typed),
                        ),
                    "type-ahead-arrives",
                )),
        )
    }

    pub(super) fn text_fields(&self) -> impl Iterator<Item = &Entity<Field>> {
        [
            &self.search,
            &self.zone_name,
            &self.name,
            &self.typed_words,
            &self.typed_isrc,
            &self.contact,
            &self.acoustid,
            &self.audd,
            &self.listenbrainz,
            &self.discord_app,
            &self.discord_icon,
            &self.sync_server,
            &self.sync_device,
            &self.shared_name,
            &self.sync_code,
            &self.organising,
            &self.minimum_length,
            &self.typed_root,
            &self.finding,
            &self.server_search,
            &self.figure,
            &self.looking,
            &self.spotify_client,
        ]
        .into_iter()
        .chain(&self.subsonic)
        .chain(&self.jellyfin)
        .chain(&self.tidal)
        .chain(&self.lastfm)
        .chain(&self.youtube_music)
    }

    pub(super) fn editing(&self, window: &Window, cx: &App) -> bool {
        self.text_fields()
            .any(|field| field.read(cx).is_focused(window))
    }
}
