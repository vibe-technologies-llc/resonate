use std::{cell::Cell, rc::Rc};

use gpui::{
    AccessibleAction, AnyElement, Bounds, Context, Div, KeyContext, Length, Modifiers, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, SharedString, canvas, div,
    prelude::*, px, relative, rgb,
};
use resonate_core::TrackId;

use crate::{
    RootView,
    app::{CONTROL_CONTEXT, RAIL_CONTEXT, RailBack, RailOn, seek_step},
    theme,
    views::{
        focus, kit,
        root::VOLUME_STEP,
        spoken::{self, Reading, Spoken},
    },
};

const RAIL_GROUP: &str = "rail";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Handle {
    Seek,
    Volume,
}

impl Handle {
    const fn id(self) -> &'static str {
        match self {
            Self::Seek => "seek",
            Self::Volume => "volume",
        }
    }

    fn track_width(self) -> Length {
        match self {
            Self::Seek => relative(1.0).into(),
            Self::Volume => theme::width(halves_on_whole_pixels(theme::volume_width())).into(),
        }
    }

    const fn spoken(self) -> &'static str {
        match self {
            Self::Seek => "Position",
            Self::Volume => "Volume",
        }
    }

    const fn fills_its_row(self) -> bool {
        matches!(self, Self::Seek)
    }
}

fn snapped_by(handle: Handle, modifiers: Modifiers, fraction: f32) -> f32 {
    match handle {
        Handle::Volume if modifiers.shift => snapped_to_steps(fraction, VOLUME_STEP),
        Handle::Volume | Handle::Seek => fraction,
    }
}

fn snapped_to_steps(fraction: f32, step: f32) -> f32 {
    ((fraction / step).round() * step).clamp(0.0, 1.0)
}

fn halves_on_whole_pixels(wide: f32) -> f32 {
    (wide / 2.0).round() * 2.0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Toward {
    On,
    Back,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Pointed {
    pub(crate) fraction: f32,
    pub(crate) reading: SharedString,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Grab {
    handle: Handle,
    fraction: f32,
    track: Option<TrackId>,
}

impl Grab {
    fn seeks_still(self, playing: Option<TrackId>) -> bool {
        self.handle == Handle::Seek && self.track == playing
    }
}

#[derive(Clone, Default)]
pub(crate) struct Rail {
    painted: Rc<Cell<Bounds<Pixels>>>,
}

impl Rail {
    pub(crate) fn width(&self) -> Pixels {
        self.painted.get().size.width
    }

    pub(crate) fn painted_as(&self) -> Rc<Cell<Bounds<Pixels>>> {
        Rc::clone(&self.painted)
    }

    pub(crate) fn fraction_at(&self, at: Point<Pixels>) -> Option<f32> {
        let painted = self.painted.get();
        let width = f32::from(painted.size.width);
        (width > 0.0).then(|| (f32::from(at.x - painted.origin.x) / width).clamp(0.0, 1.0))
    }
}

impl RootView {
    const fn rail_of(&self, handle: Handle) -> &Rail {
        match handle {
            Handle::Seek => &self.seek_rail,
            Handle::Volume => &self.volume_rail,
        }
    }

    pub(crate) fn grabbed_fraction(&self, handle: Handle) -> Option<f32> {
        self.grabbed
            .filter(|grab| grab.handle == handle)
            .map(|grab| grab.fraction)
    }

    fn grab(
        &mut self,
        handle: Handle,
        at: Point<Pixels>,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        let Some(fraction) = self
            .rail_of(handle)
            .fraction_at(at)
            .map(|fraction| snapped_by(handle, modifiers, fraction))
        else {
            return;
        };
        let track = self.player.read(cx).state().current.map(|track| track.id);
        self.grabbed = Some(Grab {
            handle,
            fraction,
            track,
        });
        self.dragged(handle, fraction, cx);
    }

    fn drag(&mut self, at: Point<Pixels>, modifiers: Modifiers, cx: &mut Context<Self>) {
        let Some(grab) = self.grabbed else {
            return;
        };
        let Some(fraction) = self
            .rail_of(grab.handle)
            .fraction_at(at)
            .map(|fraction| snapped_by(grab.handle, modifiers, fraction))
        else {
            return;
        };
        if fraction == grab.fraction {
            return;
        }

        self.grabbed = Some(Grab { fraction, ..grab });
        self.dragged(grab.handle, fraction, cx);
    }

    fn release(&mut self, cx: &mut Context<Self>) {
        let Some(grab) = self.grabbed.take() else {
            return;
        };
        let playing = self.player.read(cx).state().current.map(|track| track.id);
        if grab.seeks_still(playing) {
            self.seek_to(grab.fraction, cx);
        }
        cx.notify();
    }

    fn let_everything_go(&mut self, cx: &mut Context<Self>) {
        self.release(cx);
        self.let_the_band_go(cx);
    }

    fn dragged(&mut self, handle: Handle, fraction: f32, cx: &mut Context<Self>) {
        match handle {
            Handle::Seek => {}
            Handle::Volume => self.set_volume(fraction, cx),
        }
        cx.notify();
    }

    pub(crate) fn drag_surface(&self, cx: &mut Context<Self>) -> Div {
        let held = self.grabbed.is_some() || self.held_band.is_some();

        div()
            .absolute()
            .inset_0()
            .when(held, |surface| surface.occlude().cursor_pointer())
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                match event.pressed_button {
                    Some(MouseButton::Left) => {
                        this.drag(event.position, event.modifiers, cx);
                        this.drag_a_band(event.position, cx);
                    }
                    _ => this.let_everything_go(cx),
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| this.let_everything_go(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| this.let_everything_go(cx)),
            )
    }

    fn pointed_along(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let fraction = self.seek_rail.fraction_at(at);
        if self.seek_pointed != fraction {
            self.seek_pointed = fraction;
            cx.notify();
        }
    }

    fn set_by_a_reader(&mut self, handle: Handle, share: f32, cx: &mut Context<Self>) {
        match handle {
            Handle::Seek => self.seek_to(share, cx),
            Handle::Volume => self.set_volume(share, cx),
        }
        cx.notify();
    }

    fn step_the_rail(&mut self, handle: Handle, toward: Toward, cx: &mut Context<Self>) {
        let sign = match toward {
            Toward::On => 1,
            Toward::Back => -1,
        };
        match handle {
            Handle::Seek => self.seek_by(sign * seek_step(), cx),
            Handle::Volume => self.volume_by(sign as f32 * VOLUME_STEP, cx),
        }
    }

    pub(crate) fn pointer_left_the_seek_rail(&mut self, cx: &mut Context<Self>) {
        if self.seek_pointed.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn rail(
        &self,
        handle: Handle,
        filled: f32,
        pointed: Option<Pointed>,
        reading: Option<Reading>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let painted = self.rail_of(handle).painted.clone();
        let held = self.grabbed_fraction(handle).is_some();
        let focus = self.standing_controls.at(handle.id(), cx);

        div()
            .id(handle.id())
            .track_focus(&focus)
            .key_context(
                KeyContext::parse(&format!("{CONTROL_CONTEXT} {RAIL_CONTEXT}")).unwrap_or_default(),
            )
            .on_action(cx.listener(move |this, _: &RailOn, _, cx| {
                this.step_the_rail(handle, Toward::On, cx);
            }))
            .on_action(cx.listener(move |this, _: &RailBack, _, cx| {
                this.step_the_rail(handle, Toward::Back, cx);
            }))
            .when_some(reading, |rail, reading| {
                let on = cx.entity().downgrade();
                let back = on.clone();
                let set = on.clone();
                rail.read_as(handle.spoken(), reading)
                    .on_a11y_action(AccessibleAction::SetValue, move |asked, _, cx| {
                        let Some(share) = spoken::share_asked(asked) else {
                            return;
                        };
                        set.update(cx, |this, cx| this.set_by_a_reader(handle, share, cx))
                            .ok();
                    })
                    .on_a11y_action(AccessibleAction::Increment, move |_, _, cx| {
                        on.update(cx, |this, cx| this.step_the_rail(handle, Toward::On, cx))
                            .ok();
                    })
                    .on_a11y_action(AccessibleAction::Decrement, move |_, _, cx| {
                        back.update(cx, |this, cx| this.step_the_rail(handle, Toward::Back, cx))
                            .ok();
                    })
            })
            .rounded_md()
            .focus_visible(focus::lit)
            .group(RAIL_GROUP)
            .flex()
            .when_else(
                handle.fills_its_row(),
                |rail| rail.flex_1().min_w(px(0.0)),
                |rail| rail.flex_none(),
            )
            .items_center()
            .h(theme::width(theme::RAIL_HEIGHT))
            .px(theme::width(theme::RAIL_THUMB / 2.0))
            .cursor_pointer()
            .when(handle == Handle::Seek, |rail| {
                rail.on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                    this.pointed_along(event.position, cx);
                }))
                .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                    if !hovered {
                        this.pointer_left_the_seek_rail(cx);
                    }
                }))
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    window.prevent_default();
                    this.grab(handle, event.position, event.modifiers, cx);
                }),
            )
            .child(
                div()
                    .relative()
                    .h(theme::width(theme::RAIL_TRACK))
                    .w(handle.track_width())
                    .rounded_full()
                    .bg(rgb(theme::soft_edge()))
                    .child(
                        div()
                            .h_full()
                            .w(relative(filled))
                            .rounded_full()
                            .bg(rgb(theme::text()))
                            .group_hover(RAIL_GROUP, |filled| filled.bg(rgb(theme::accent())))
                            .when(held, |filled| filled.bg(rgb(theme::accent()))),
                    )
                    .child(thumb(filled, held))
                    .children(pointed.map(|pointed| bubble(&pointed)))
                    .child(canvas(
                        move |bounds, _, _| painted.set(bounds),
                        |_, _, _, _| {},
                    )),
            )
            .into_any_element()
    }
}

fn bubble(pointed: &Pointed) -> Div {
    div()
        .absolute()
        .left(relative(pointed.fraction))
        .ml(theme::width(-theme::RAIL_BUBBLE_WIDTH / 2.0))
        .bottom(theme::width(theme::RAIL_TRACK + theme::RAIL_BUBBLE_GAP))
        .w(theme::width(theme::RAIL_BUBBLE_WIDTH))
        .flex()
        .justify_center()
        .child(
            div()
                .flex_none()
                .px_2()
                .py_1()
                .rounded_md()
                .bg(rgb(theme::raised()))
                .border_1()
                .border_color(rgb(theme::soft_edge()))
                .child(kit::figure(pointed.reading.clone()).text_color(rgb(theme::text()))),
        )
}

fn thumb(filled: f32, held: bool) -> Div {
    div()
        .absolute()
        .left(relative(filled))
        .ml(theme::width(-theme::RAIL_THUMB / 2.0))
        .top(theme::width((theme::RAIL_TRACK - theme::RAIL_THUMB) / 2.0))
        .size(theme::width(theme::RAIL_THUMB))
        .rounded_full()
        .bg(rgb(theme::text()))
        .border_2()
        .border_color(rgb(theme::surface()))
        .when(!held, |thumb| {
            thumb
                .opacity(0.0)
                .group_hover(RAIL_GROUP, |thumb| thumb.opacity(1.0))
        })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui::TestAppContext;
    use resonate_core::Frames;
    use resonate_engine::Command;
    use resonate_library::Library;

    use super::*;
    use crate::driven::{Driven, Folder};

    #[gpui::test]
    fn releasing_a_seek_keeps_its_preview_until_the_engine_answers(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let file = folder.tone("seek.wav", 10);
        let mut driven = Driven::opened_in(
            cx,
            Arc::new(Library::open_in_memory().expect("a catalog")),
            &folder,
        );
        driven.play(&[file]);
        let root = driven.root.clone();
        let (wanted, engine) = driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| {
                let current = root
                    .player
                    .read(cx)
                    .state()
                    .current
                    .expect("a paused track");
                let wanted = Frames(current.duration.expect("a length").get() * 3 / 4);
                root.grabbed = Some(Grab {
                    handle: Handle::Seek,
                    fraction: 0.75,
                    track: Some(current.id),
                });
                root.release(cx);
                let model = root.player.read(cx);
                assert_eq!(
                    model.state().current.expect("a track").position,
                    current.position
                );
                assert_eq!(model.shown_position(), Some(wanted));
                assert!(root.grabbed.is_none());
                (wanted, model.engine())
            })
        });
        driven.until(|root, cx| {
            root.player
                .read(cx)
                .state()
                .current
                .is_some_and(|track| track.position == wanted)
        });
        let newest = driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| {
                let current = root
                    .player
                    .read(cx)
                    .state()
                    .current
                    .expect("a paused track");
                for fraction in [0.5, 0.25] {
                    root.grabbed = Some(Grab {
                        handle: Handle::Seek,
                        fraction,
                        track: Some(current.id),
                    });
                    root.release(cx);
                }
                let newest = Frames(current.duration.expect("a length").get() / 4);
                assert_eq!(root.player.read(cx).shown_position(), Some(newest));
                newest
            })
        });
        driven.until(|root, cx| {
            root.player
                .read(cx)
                .state()
                .current
                .is_some_and(|track| track.position == newest)
        });
        let later = Frames::ZERO;
        engine.send(Command::Seek(later)).expect("a later seek");
        driven.until(|root, cx| {
            root.player
                .read(cx)
                .state()
                .current
                .is_some_and(|track| track.position == later)
        });
        assert_eq!(
            driven.read(|root, cx| root.player.read(cx).shown_position()),
            Some(later)
        );
    }

    #[gpui::test]
    fn a_refused_seek_releases_the_preview_even_when_the_clock_did_not_move(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let file = folder.tone("seek.wav", 10);
        let mut driven = Driven::opened_in(
            cx,
            Arc::new(Library::open_in_memory().expect("a catalog")),
            &folder,
        );
        driven.play(&[file]);
        let model = driven.read(|root, _| root.player.clone());
        let before = driven.cx.update(|_, cx| {
            model.update(cx, |model, cx| {
                let current = model.state().current.expect("a paused track");
                let past_end = Frames(current.duration.expect("a length").get() + 1);
                model.seek(past_end, cx);
                assert_eq!(model.shown_position(), Some(past_end));
                current.position
            })
        });
        driven.until(|root, cx| root.player.read(cx).shown_position() == Some(before));
        assert_eq!(
            driven.read(|root, cx| root
                .player
                .read(cx)
                .state()
                .current
                .expect("a track")
                .position),
            before,
        );
        assert!(driven.read(|_, cx| crate::toast::is_showing(cx)));
    }

    fn track(id: u64) -> Option<TrackId> {
        Some(TrackId::new(id).expect("an id"))
    }

    #[test]
    fn a_seek_rail_released_after_the_track_changed_seeks_nothing() {
        let held = Grab {
            handle: Handle::Seek,
            fraction: 0.5,
            track: track(3),
        };

        assert!(held.seeks_still(track(3)));
        assert!(!held.seeks_still(track(4)));
        assert!(!held.seeks_still(None));
    }

    #[test]
    fn a_rail_halves_on_a_whole_pixel_at_every_text_size() {
        for wide in [79.2, 88.0, 96.8, 99.0, 117.3, 176.0] {
            let halved = halves_on_whole_pixels(wide);

            assert_eq!(halved % 2.0, 0.0, "{wide} is drawn at {halved}");
            assert!((halved - wide).abs() <= 1.0);
        }
    }

    #[test]
    fn a_volume_dragged_with_shift_lands_on_a_step_of_five() {
        let shift = Modifiers::shift();

        for (at, lands) in [
            (0.0, 0.0),
            (0.02, 0.0),
            (0.03, 0.05),
            (0.52, 0.5),
            (0.98, 1.0),
        ] {
            let snapped = snapped_by(Handle::Volume, shift, at);

            assert!((snapped - lands).abs() < 1e-6, "{at} landed on {snapped}");
        }
    }

    #[test]
    fn a_volume_dragged_without_shift_and_a_seek_with_it_stay_where_they_are() {
        assert_eq!(snapped_by(Handle::Volume, Modifiers::none(), 0.337), 0.337);
        assert_eq!(snapped_by(Handle::Seek, Modifiers::shift(), 0.337), 0.337);
    }

    #[test]
    fn a_volume_rail_seeks_nothing_whatever_plays() {
        let held = Grab {
            handle: Handle::Volume,
            fraction: 0.5,
            track: track(3),
        };

        assert!(!held.seeks_still(track(3)));
    }
}
