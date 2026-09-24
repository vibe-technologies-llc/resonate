use std::{cell::Cell, rc::Rc};

use gpui::{
    Bounds, Context, Div, DragMoveEvent, IntoElement, Render, ScrollHandle, Stateful,
    UniformListScrollHandle, Window, canvas, div, fill, point, prelude::*, px, rgb,
};

use crate::theme;

// GPUI scrolls these regions but does not paint a scrollbar for them.
const TRACK_WIDTH: f32 = 12.0;
const THUMB_WIDTH: f32 = 6.0;
const MIN_THUMB_HEIGHT: f32 = 48.0;
const MIN_THUMB_WIDTH: f32 = 48.0;

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

impl Target {
    fn handle(&self) -> ScrollHandle {
        match self {
            Self::Plain(handle) => handle.clone(),
            Self::Uniform(handle) => handle.0.borrow().base_handle.clone(),
        }
    }

    fn thumb(&self, track_height: f32) -> Option<(f32, f32)> {
        let handle = self.handle();
        let max = f32::from(handle.max_offset().height);
        let viewport = f32::from(handle.bounds().size.height);
        if max <= 0.0 || viewport <= 0.0 || track_height <= 0.0 {
            return None;
        }
        let height = (track_height * viewport / (viewport + max))
            .max(MIN_THUMB_HEIGHT)
            .min(track_height);
        let top = (-f32::from(handle.offset().y) / max).clamp(0.0, 1.0) * (track_height - height);
        Some((top, height))
    }

    fn move_thumb(&self, top: f32, track_height: f32) {
        let Some((_, height)) = self.thumb(track_height) else {
            return;
        };
        let handle = self.handle();
        let max = f32::from(handle.max_offset().height);
        let travel = track_height - height;
        if travel > 0.0 {
            let mut offset = handle.offset();
            offset.y = px(-(top / travel).clamp(0.0, 1.0) * max);
            handle.set_offset(offset);
        }
    }
}

#[derive(Clone)]
struct ThumbDrag {
    id: &'static str,
    grab: Rc<Cell<f32>>,
}

#[derive(Clone)]
struct HorizontalDrag {
    id: &'static str,
    grab: Rc<Cell<f32>>,
}

struct EmptyDrag;

impl Render for EmptyDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_0()
    }
}

pub(crate) fn vertical(id: &'static str, handle: impl Into<Target>) -> Stateful<Div> {
    let target = handle.into();
    let grab = Rc::new(Cell::new(0.0));
    let paint_target = target.clone();
    let down_target = target.clone();
    let move_target = target.clone();
    let down_grab = Rc::clone(&grab);
    let hovered = Rc::new(Cell::new(false));
    let paint_hovered = Rc::clone(&hovered);

    div()
        .id(id)
        .absolute()
        .top_0()
        .bottom_0()
        .right_0()
        .w(px(TRACK_WIDTH))
        .cursor_default()
        .on_hover(move |over, window, _| {
            hovered.set(*over);
            window.refresh();
        })
        .on_mouse_down(gpui::MouseButton::Left, move |event, window, _| {
            let bounds = down_target.handle().bounds();
            let track_height = f32::from(bounds.size.height);
            let y = f32::from(event.position.y - bounds.origin.y);
            if let Some((top, height)) = down_target.thumb(track_height) {
                if (top..=top + height).contains(&y) {
                    down_grab.set(y - top);
                } else {
                    down_grab.set(height / 2.0);
                    down_target.move_thumb(y - height / 2.0, track_height);
                    window.refresh();
                }
            }
        })
        .on_drag(ThumbDrag { id, grab }, |_, _, _, cx| cx.new(|_| EmptyDrag))
        .on_drag_move::<ThumbDrag>(move |event: &DragMoveEvent<ThumbDrag>, window, cx| {
            let drag = event.drag(cx);
            if drag.id != id {
                return;
            }
            let track_height = f32::from(event.bounds.size.height);
            let y = f32::from(event.event.position.y - event.bounds.origin.y);
            move_target.move_thumb(y - drag.grab.get(), track_height);
            window.refresh();
        })
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let Some((top, height)) = paint_target.thumb(f32::from(bounds.size.height))
                    else {
                        return;
                    };
                    let x = bounds.origin.x + px((TRACK_WIDTH - THUMB_WIDTH) / 2.0);
                    let y = bounds.origin.y + px(top);
                    let mut thumb = fill(
                        Bounds::from_corners(
                            point(x, y),
                            point(x + px(THUMB_WIDTH), y + px(height)),
                        ),
                        rgb(if paint_hovered.get() {
                            theme::text()
                        } else {
                            theme::muted()
                        }),
                    );
                    thumb.corner_radii = px(THUMB_WIDTH / 2.0).into();
                    window.paint_quad(thumb);
                },
            )
            .size_full(),
        )
}

pub(crate) fn around(
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
        .child(vertical(id, handle))
}

fn horizontal_thumb(handle: &ScrollHandle, track_width: f32) -> Option<(f32, f32)> {
    let max = f32::from(handle.max_offset().width);
    let viewport = f32::from(handle.bounds().size.width);
    if max <= 0.0 || viewport <= 0.0 || track_width <= 0.0 {
        return None;
    }
    let width = (track_width * viewport / (viewport + max))
        .max(MIN_THUMB_WIDTH)
        .min(track_width);
    let left = (-f32::from(handle.offset().x) / max).clamp(0.0, 1.0) * (track_width - width);
    Some((left, width))
}

fn move_horizontal_thumb(handle: &ScrollHandle, left: f32, track_width: f32) {
    let Some((_, width)) = horizontal_thumb(handle, track_width) else {
        return;
    };
    let travel = track_width - width;
    if travel > 0.0 {
        let mut offset = handle.offset();
        offset.x = px(-(left / travel).clamp(0.0, 1.0) * f32::from(handle.max_offset().width));
        handle.set_offset(offset);
    }
}

pub(crate) fn horizontal(id: &'static str, handle: ScrollHandle) -> Stateful<Div> {
    let grab = Rc::new(Cell::new(0.0));
    let paint_handle = handle.clone();
    let down_handle = handle.clone();
    let move_handle = handle;
    let down_grab = Rc::clone(&grab);
    let hovered = Rc::new(Cell::new(false));
    let paint_hovered = Rc::clone(&hovered);

    div()
        .id(gpui::SharedString::from(format!("{id}-scrollbar")))
        .absolute()
        .left_0()
        .right_0()
        .bottom_0()
        .h(px(TRACK_WIDTH))
        .cursor_default()
        .on_hover(move |over, window, _| {
            hovered.set(*over);
            window.refresh();
        })
        .on_mouse_down(gpui::MouseButton::Left, move |event, window, _| {
            let bounds = down_handle.bounds();
            let track_width = f32::from(bounds.size.width);
            let x = f32::from(event.position.x - bounds.origin.x);
            if let Some((left, width)) = horizontal_thumb(&down_handle, track_width) {
                if (left..=left + width).contains(&x) {
                    down_grab.set(x - left);
                } else {
                    down_grab.set(width / 2.0);
                    move_horizontal_thumb(&down_handle, x - width / 2.0, track_width);
                    window.refresh();
                }
            }
        })
        .on_drag(HorizontalDrag { id, grab }, |_, _, _, cx| {
            cx.new(|_| EmptyDrag)
        })
        .on_drag_move::<HorizontalDrag>(move |event: &DragMoveEvent<HorizontalDrag>, window, cx| {
            let drag = event.drag(cx);
            if drag.id != id {
                return;
            }
            let width = f32::from(event.bounds.size.width);
            let x = f32::from(event.event.position.x - event.bounds.origin.x);
            move_horizontal_thumb(&move_handle, x - drag.grab.get(), width);
            window.refresh();
        })
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let Some((left, width)) =
                        horizontal_thumb(&paint_handle, f32::from(bounds.size.width))
                    else {
                        return;
                    };
                    let x = bounds.origin.x + px(left);
                    let y = bounds.origin.y + px((TRACK_WIDTH - THUMB_WIDTH) / 2.0);
                    let mut thumb = fill(
                        Bounds::from_corners(
                            point(x, y),
                            point(x + px(width), y + px(THUMB_WIDTH)),
                        ),
                        rgb(if paint_hovered.get() {
                            theme::text()
                        } else {
                            theme::muted()
                        }),
                    );
                    thumb.corner_radii = px(THUMB_WIDTH / 2.0).into();
                    window.paint_quad(thumb);
                },
            )
            .size_full(),
        )
}
