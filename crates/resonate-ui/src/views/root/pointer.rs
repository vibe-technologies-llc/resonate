use std::hash::{Hash as _, Hasher as _};

use ahash::AHasher;
use gpui::{
    App, Canvas, Context, Div, ElementId, MouseDownEvent, MouseExitEvent, MouseMoveEvent,
    SharedString, Stateful, Window, canvas, div, prelude::*, px, rgb,
};
use resonate_library::ArtistFound;

use super::{Pane, Pointer, RootView, UnderThePointer};
use crate::{
    Selection,
    serial::Serial,
    theme,
    views::{
        browser::ArtistShows,
        field::Composing,
        hint::Names,
        kit::EndsInAnEllipsis,
        pointed::{self, LitUnderThePointer},
        sheets::Sheet,
        spoken::{Spelt, Spoken},
    },
};

impl RootView {
    pub(super) fn let_the_control_go(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.disarm();
        let let_go = self.controls.lets_go(window, cx)
            | self.standing_controls.lets_go(window, cx)
            | self.remotely.controls.lets_go(window, cx);
        if let_go {
            cx.notify();
        }
    }

    pub(super) fn laid_out(&self, cx: &App) -> u64 {
        let library = self.library.read(cx);
        let mut stamp = AHasher::default();
        self.pane.hash(&mut stamp);
        self.settings_shown.category.hash(&mut stamp);
        self.settings_shown.page.hash(&mut stamp);
        self.player.read(cx).queued().revision.hash(&mut stamp);
        library.albums().identity().hash(&mut stamp);
        library.artists().identity().hash(&mut stamp);
        library.tracks().identity().hash(&mut stamp);
        library.playlists().len().hash(&mut stamp);
        if self.pane.lands_where_it_was_left() {
            let offset = self
                .scroll_of(self.pane, cx)
                .0
                .borrow()
                .base_handle
                .offset();
            f32::from(offset.y).to_bits().hash(&mut stamp);
        }
        stamp.finish()
    }

    pub(super) fn moved_under_a_still_pointer(&mut self, window: &Window, cx: &App) -> bool {
        let under = UnderThePointer {
            at: window.mouse_position(),
            laid_out: self.laid_out(cx),
        };
        let moved = under.moved_since(self.under_the_pointer);
        self.under_the_pointer = Some(under);
        moved
    }

    pub(crate) fn focused_control_named(&self, window: &Window) -> Option<SharedString> {
        if !window.last_input_was_keyboard() {
            return None;
        }

        self.controls
            .caption_of_the_focused(window)
            .or_else(|| self.standing_controls.caption_of_the_focused(window))
            .or_else(|| self.remotely.controls.caption_of_the_focused(window))
    }

    pub(super) fn hints_are_wanted(&self, cx: &App) -> bool {
        self.pointer_inside
            && self.rails.grabbed.is_none()
            && self.rails.held_band.is_none()
            && !self.something_stands_over_the_pane()
            && !cx.has_active_drag()
    }

    pub(super) fn pointer_watch(&self, cx: &mut Context<Self>) -> Canvas<()> {
        let watching = cx.entity();

        canvas(
            |_, _, _| (),
            move |_, (), window, _| {
                let left = watching.clone();
                window.on_mouse_event(move |_: &MouseExitEvent, _, window, cx| {
                    pointed::let_go(window);
                    left.update(cx, |this, cx| this.pointer_moved_to(Pointer::Gone, cx));
                });

                let came_back = watching.clone();
                window.on_mouse_event(move |_: &MouseMoveEvent, _, _, cx| {
                    came_back.update(cx, |this, cx| {
                        this.pointer_moved_to(Pointer::InTheWindow, cx)
                    });
                });

                let pressed = watching.clone();
                window.on_mouse_event(move |_: &MouseDownEvent, phase, _, cx| {
                    if !phase.bubble() {
                        return;
                    }
                    pressed.update(cx, |this, cx| {
                        this.stop_typing(cx);
                    });
                });
            },
        )
        .absolute()
        .size(px(0.0))
    }

    pub(super) fn composition_watch(&self) -> Canvas<()> {
        let search = self.typed.search.clone();
        let focus = self.focus.clone();
        let welcomed = !self.something_stands_over_the_pane();
        canvas(
            |_, _, _| (),
            move |_, (), window, cx| {
                window.handle_input(
                    &focus,
                    Composing::at_the_window(search.clone(), welcomed),
                    cx,
                );
            },
        )
        .absolute()
        .size(px(0.0))
    }

    pub(super) fn pointer_moved_to(&mut self, pointer: Pointer, cx: &mut Context<Self>) {
        let inside = pointer == Pointer::InTheWindow;
        if self.pointer_inside == inside {
            return;
        }
        self.pointer_inside = inside;
        if !inside {
            self.lyrics.update(cx, |model, _| model.open_out(false));
            self.rails.seek_pointed = None;
        }
        cx.notify();
    }

    pub(crate) fn opens(
        &self,
        id: impl Into<ElementId>,
        label: impl IntoElement + Spelt,
        saying: &'static str,
        selection: Option<Selection>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let press = selection.map(|selection| {
            move |this: &mut Self, cx: &mut Context<Self>| this.opened(selection, cx)
        });

        self.pressed_to(id, label, saying, press, cx)
    }

    pub(crate) fn pressed_to(
        &self,
        id: impl Into<ElementId>,
        label: impl IntoElement + Spelt,
        saying: &'static str,
        press: Option<impl Fn(&mut Self, &mut Context<Self>) + 'static>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = id.into();

        div()
            .id(id.clone())
            .min_w(px(0.0))
            .truncate()
            .ends_in_an_ellipsis()
            .when(press.is_some(), |named| named.linked_as(&label, saying))
            .child(label)
            .when_some(press, |named, press| {
                named
                    .cursor_pointer()
                    .lit_under_the_pointer(id, |named| {
                        named
                            .text_color(rgb(theme::accent()))
                            .text_decoration_1()
                            .text_decoration_color(rgb(theme::accent()))
                    })
                    .names(saying)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        press(this, cx);
                    }))
            })
    }

    pub(crate) fn press_a_lookup(&mut self) -> Serial {
        self.attention.moved()
    }

    pub(crate) fn window_has_moved_on_from(&self, pressed: Serial) -> bool {
        self.attention != pressed
            || self
                .sheet_in_front()
                .is_some_and(|sheet| sheet != Sheet::Menu)
    }

    pub(crate) fn open_artist_found(&mut self, found: ArtistFound, cx: &mut Context<Self>) {
        let pressed = self.press_a_lookup();
        let landed = self
            .library
            .update(cx, |library, cx| library.land_artist_found(found, cx));
        cx.spawn(async move |this, cx| {
            let Some(artist) = landed.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if !this.window_has_moved_on_from(pressed) {
                    this.opened(Selection::Artist(artist), cx);
                }
            });
        })
        .detach();
    }

    pub(crate) fn opened(&mut self, selection: Selection, cx: &mut Context<Self>) {
        let leaving = self.here_now(cx);
        if leaving.selection == selection && leaving.pane == Pane::Tracks {
            return;
        }
        Self::keep_wayback(&mut self.came_from, leaving);
        self.goes_forward.clear();

        if matches!(selection, Selection::Artist(_)) {
            self.shows.artist = ArtistShows::default();
            self.orders.ordering = None;
        }
        self.library
            .update(cx, |library, cx| library.select(selection, cx));
        self.set_pane(Pane::Tracks, cx);
    }
}
