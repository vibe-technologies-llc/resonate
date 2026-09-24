use std::{cell::Cell, rc::Rc};

use gpui::{
    App, Bounds, Context, Div, DragMoveEvent, ElementId, IntoElement, MouseButton, Pixels, Point,
    Render, ScrollHandle, SharedString, Size, Stateful, UniformListScrollHandle, Window, canvas,
    div, fill, point, prelude::*, px, rgb, size,
};

use crate::{app::ResonateApp, theme};

const TRACK_BREADTH: f32 = 12.0;
const THUMB_BREADTH: f32 = 6.0;
const SHORTEST_THUMB: f32 = 48.0;

#[derive(Clone)]
pub(crate) enum Target {
    Plain(ScrollHandle),
    Uniform(UniformListScrollHandle),
}

impl From<ScrollHandle> for Target {
    fn from(handle: ScrollHandle) -> Self {
        Self::Plain(handle)
    }
}

impl From<UniformListScrollHandle> for Target {
    fn from(handle: UniformListScrollHandle) -> Self {
        Self::Uniform(handle)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    Vertical,
    Horizontal,
}

impl Axis {
    fn along(self, at: Point<Pixels>) -> f32 {
        f32::from(match self {
            Self::Vertical => at.y,
            Self::Horizontal => at.x,
        })
    }

    fn length(self, extent: Size<Pixels>) -> f32 {
        f32::from(match self {
            Self::Vertical => extent.height,
            Self::Horizontal => extent.width,
        })
    }
}

#[derive(Clone, Copy)]
struct Thumb {
    start: f32,
    length: f32,
}

impl Thumb {
    fn holds(self, at: f32) -> bool {
        (self.start..=self.start + self.length).contains(&at)
    }
}

impl Target {
    fn handle(&self) -> ScrollHandle {
        match self {
            Self::Plain(handle) => handle.clone(),
            Self::Uniform(handle) => handle.0.borrow().base_handle.clone(),
        }
    }

    fn thumb(&self, axis: Axis, track: f32) -> Option<Thumb> {
        let handle = self.handle();
        let furthest = axis.length(handle.max_offset());
        let viewport = axis.length(handle.bounds().size);
        if furthest <= 0.0 || viewport <= 0.0 || track <= 0.0 {
            return None;
        }
        let length = (track * viewport / (viewport + furthest))
            .max(SHORTEST_THUMB)
            .min(track);
        let start = (-axis.along(handle.offset()) / furthest).clamp(0.0, 1.0) * (track - length);
        Some(Thumb { start, length })
    }

    fn move_thumb(&self, axis: Axis, start: f32, track: f32) {
        let Some(thumb) = self.thumb(axis, track) else {
            return;
        };
        let travel = track - thumb.length;
        if travel <= 0.0 {
            return;
        }
        let handle = self.handle();
        let reached = px(-(start / travel).clamp(0.0, 1.0) * axis.length(handle.max_offset()));
        let mut offset = handle.offset();
        match axis {
            Axis::Vertical => offset.y = reached,
            Axis::Horizontal => offset.x = reached,
        }
        handle.set_offset(offset);
    }
}

#[derive(Clone)]
struct Grip {
    id: &'static str,
    axis: Axis,
    held_at: Rc<Cell<f32>>,
}

struct EmptyDrag;

impl Render for EmptyDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_0()
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Scrollbars {
    shown: bool,
}

impl Scrollbars {
    pub(crate) fn of(cx: &App) -> Self {
        Self {
            shown: cx.global::<ResonateApp>().scrollbars,
        }
    }

    pub(crate) fn vertical(self, id: &'static str, handle: impl Into<Target>) -> Stateful<Div> {
        self.bar(id.into(), id, Axis::Vertical, handle.into())
    }

    pub(crate) fn horizontal(self, id: &'static str, handle: ScrollHandle) -> Stateful<Div> {
        self.bar(
            SharedString::from(format!("{id}-scrollbar")).into(),
            id,
            Axis::Horizontal,
            handle.into(),
        )
    }

    pub(crate) fn around(
        self,
        id: &'static str,
        handle: impl Into<Target>,
        content: impl IntoElement,
    ) -> Div {
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .child(content)
            .child(self.vertical(id, handle))
    }

    fn bar(
        self,
        element: ElementId,
        id: &'static str,
        axis: Axis,
        target: Target,
    ) -> Stateful<Div> {
        if self.shown {
            bar(element, id, axis, target)
        } else {
            div().id(element).absolute()
        }
    }
}

fn bar(element: ElementId, id: &'static str, axis: Axis, target: Target) -> Stateful<Div> {
    let track: Rc<Cell<Bounds<Pixels>>> = Rc::default();
    let placed = div().id(element).absolute().bottom_0().right_0();
    let placed = match axis {
        Axis::Vertical => placed.top_0().w(px(TRACK_BREADTH)),
        Axis::Horizontal => placed.left_0().h(px(TRACK_BREADTH)),
    };

    placed
        .cursor_default()
        .on_hover(|_, window, _| window.refresh())
        .on_mouse_down(MouseButton::Left, {
            let target = target.clone();
            let track = Rc::clone(&track);
            move |event, window, _| {
                let track = track.get();
                let length = axis.length(track.size);
                let at = axis.along(event.position) - axis.along(track.origin);
                if let Some(thumb) = target.thumb(axis, length)
                    && !thumb.holds(at)
                {
                    target.move_thumb(axis, at - thumb.length / 2.0, length);
                    window.refresh();
                }
            }
        })
        .on_drag(
            Grip {
                id,
                axis,
                held_at: Rc::default(),
            },
            {
                let target = target.clone();
                let track = Rc::clone(&track);
                move |grip, cursor, _, cx| {
                    let length = axis.length(track.get().size);
                    let at = axis.along(cursor);
                    let held_at = target
                        .thumb(axis, length)
                        .map_or(0.0, |thumb| (at - thumb.start).clamp(0.0, thumb.length));
                    grip.held_at.set(held_at);
                    cx.new(|_| EmptyDrag)
                }
            },
        )
        .on_drag_move::<Grip>({
            let target = target.clone();
            move |event: &DragMoveEvent<Grip>, window, cx| {
                let grip = event.drag(cx);
                if grip.id != id || grip.axis != axis {
                    return;
                }
                let length = axis.length(event.bounds.size);
                let at = axis.along(event.event.position) - axis.along(event.bounds.origin);
                target.move_thumb(axis, at - grip.held_at.get(), length);
                window.refresh();
            }
        })
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    track.set(bounds);
                    let Some(thumb) = target.thumb(axis, axis.length(bounds.size)) else {
                        return;
                    };
                    let inset = px((TRACK_BREADTH - THUMB_BREADTH) / 2.0);
                    let (origin, extent) = match axis {
                        Axis::Vertical => (
                            point(bounds.origin.x + inset, bounds.origin.y + px(thumb.start)),
                            size(px(THUMB_BREADTH), px(thumb.length)),
                        ),
                        Axis::Horizontal => (
                            point(bounds.origin.x + px(thumb.start), bounds.origin.y + inset),
                            size(px(thumb.length), px(THUMB_BREADTH)),
                        ),
                    };
                    let lit = bounds.contains(&window.mouse_position());
                    let mut quad = fill(
                        Bounds::new(origin, extent),
                        rgb(if lit { theme::text() } else { theme::muted() }),
                    );
                    quad.corner_radii = px(THUMB_BREADTH / 2.0).into();
                    window.paint_quad(quad);
                },
            )
            .size_full(),
        )
}
