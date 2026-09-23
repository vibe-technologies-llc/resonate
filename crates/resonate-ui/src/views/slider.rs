use std::{cell::Cell, rc::Rc};

use gpui::{
    AnyElement, Bounds, Context, Div, Length, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, canvas, div, prelude::*, px, relative, rgb,
};

use crate::{RootView, theme};

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
            Self::Volume => theme::width(theme::volume_width()).into(),
        }
    }

    const fn fills_its_row(self) -> bool {
        matches!(self, Self::Seek)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Grab {
    handle: Handle,
    fraction: f32,
}

#[derive(Clone, Default)]
pub(crate) struct Rail {
    painted: Rc<Cell<Bounds<Pixels>>>,
}

impl Rail {
    pub(crate) fn width(&self) -> Pixels {
        self.painted.get().size.width
    }

    fn fraction_at(&self, at: Point<Pixels>) -> Option<f32> {
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

    fn grab(&mut self, handle: Handle, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(fraction) = self.rail_of(handle).fraction_at(at) else {
            return;
        };
        self.grabbed = Some(Grab { handle, fraction });
        self.dragged(handle, fraction, cx);
    }

    fn drag(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(grab) = self.grabbed else {
            return;
        };
        let Some(fraction) = self.rail_of(grab.handle).fraction_at(at) else {
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
        match grab.handle {
            Handle::Seek => self.seek_to(grab.fraction, cx),
            Handle::Volume => {}
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
                        this.drag(event.position, cx);
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

    pub(crate) fn rail(&self, handle: Handle, filled: f32, cx: &mut Context<Self>) -> AnyElement {
        let painted = self.rail_of(handle).painted.clone();
        let held = self.grabbed_fraction(handle).is_some();

        div()
            .id(handle.id())
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
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.grab(handle, event.position, cx);
                }),
            )
            .child(
                div()
                    .relative()
                    .h(theme::width(theme::RAIL_TRACK))
                    .w(handle.track_width())
                    .rounded_full()
                    .bg(rgb(theme::outline()))
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
                    .child(canvas(
                        move |bounds, _, _| painted.set(bounds),
                        |_, _, _, _| {},
                    )),
            )
            .into_any_element()
    }
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
