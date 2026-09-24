use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    App, Bounds, Context, Div, DragMoveEvent, ElementId, IntoElement, MouseButton, Pixels, Point,
    Render, ScrollHandle, SharedString, Size, Stateful, UniformListScrollHandle, Window, canvas,
    div, fill, point, prelude::*, px, rgb, size,
};
use resonate_core::ScrollbarMode;

use crate::{app::ResonateApp, theme};

const TRACK_BREADTH: f32 = 12.0;
const THUMB_BREADTH: f32 = 6.0;
const SHORTEST_THUMB: f32 = 48.0;
const SCROLLED_LINGERS: Duration = Duration::from_millis(1_200);
const LAST_SCROLLED: &str = "last-scrolled";

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
    mode: ScrollbarMode,
}

impl Scrollbars {
    pub(crate) fn of(cx: &App) -> Self {
        Self {
            mode: cx.global::<ResonateApp>().scrollbars,
        }
    }

    const fn when_drawn(self) -> Shown {
        match self.mode {
            ScrollbarMode::AutoHidden => Shown::WhileScrolled,
            ScrollbarMode::Shown | ScrollbarMode::Hidden => Shown::Always,
        }
    }

    pub(crate) fn vertical(self, id: &'static str, handle: impl Into<Target>) -> Stateful<Div> {
        let shown = self.when_drawn();
        self.bar(id.into(), id, Axis::Vertical, handle.into(), shown)
    }

    pub(crate) fn vertical_while(
        self,
        id: &'static str,
        handle: impl Into<Target>,
        moved: bool,
    ) -> Stateful<Div> {
        self.bar(
            id.into(),
            id,
            Axis::Vertical,
            handle.into(),
            Shown::WhileMoved(moved),
        )
    }

    pub(crate) fn horizontal(self, id: &'static str, handle: ScrollHandle) -> Stateful<Div> {
        self.bar(
            SharedString::from(format!("{id}-scrollbar")).into(),
            id,
            Axis::Horizontal,
            handle.into(),
            self.when_drawn(),
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
        shown: Shown,
    ) -> Stateful<Div> {
        match self.mode {
            ScrollbarMode::Shown | ScrollbarMode::AutoHidden => {
                bar(element, id, axis, target, shown)
            }
            ScrollbarMode::Hidden => div().id(element).absolute(),
        }
    }
}

#[derive(Clone, Copy)]
enum Shown {
    Always,
    WhileMoved(bool),
    WhileScrolled,
}

impl Shown {
    const fn now(self, pointed: bool, held: bool, scrolled: bool) -> bool {
        match self {
            Self::Always => true,
            Self::WhileMoved(moved) => moved || pointed || held,
            Self::WhileScrolled => scrolled || pointed || held,
        }
    }

    const fn follows_the_offset(self) -> bool {
        matches!(self, Self::WhileScrolled)
    }
}

#[derive(Clone, Copy)]
struct Scrolled {
    offset: Point<Pixels>,
    at: Option<Instant>,
}

impl Scrolled {
    fn seen(held: Option<Self>, offset: Point<Pixels>, now: Instant) -> Self {
        match held {
            Some(held) if held.offset == offset => held,
            Some(_) => Self {
                offset,
                at: Some(now),
            },
            None => Self { offset, at: None },
        }
    }

    fn lately(self, now: Instant) -> bool {
        self.at
            .is_some_and(|at| now.saturating_duration_since(at) < SCROLLED_LINGERS)
    }
}

fn scrolled_lately(target: &Target, window: &mut Window, cx: &App) -> bool {
    let offset = target.handle().offset();
    let now = Instant::now();

    let (lately, fresh) = window.with_global_id(LAST_SCROLLED.into(), |global, window| {
        window.with_element_state(global, |held: Option<Scrolled>, _| {
            let scrolled = Scrolled::seen(held, offset, now);
            let fresh = scrolled.at == Some(now);
            ((scrolled.lately(now), fresh), scrolled)
        })
    });
    if fresh {
        hide_once_it_settles(window, cx);
    }

    lately
}

fn hide_once_it_settles(window: &Window, cx: &App) {
    window
        .spawn(cx, async move |cx| {
            cx.background_executor().timer(SCROLLED_LINGERS).await;
            if cx.update(|window, _| window.refresh()).is_err() {
                tracing::debug!("the window a scrollbar was waiting to hide has gone");
            }
        })
        .detach();
}

fn bar(
    element: ElementId,
    id: &'static str,
    axis: Axis,
    target: Target,
    shown: Shown,
) -> Stateful<Div> {
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
                move |bounds, _, window, cx| {
                    track.set(bounds);
                    let lit = bounds.contains(&window.mouse_position());
                    let scrolled =
                        shown.follows_the_offset() && scrolled_lately(&target, window, cx);
                    if !shown.now(lit, cx.has_active_drag(), scrolled) {
                        return;
                    }
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
