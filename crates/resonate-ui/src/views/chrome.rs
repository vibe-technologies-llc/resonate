use std::cell::Cell;

use gpui::{
    AnyElement, App, Bounds, BoxShadow, Corners, CursorStyle, Decorations, Div, HitboxBehavior,
    MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Point, ResizeEdge, Size, Stateful, Svg,
    Tiling, Window, canvas, div, hsla, point, prelude::*, px, rgb,
};

use crate::{
    ResonateApp, RootView,
    icons::{self, Icon},
    theme,
    views::hint::Names,
};

const SHADOW_ALPHA: f32 = 0.45;
const SHADOW_DROP: f32 = 2.0;
const SHADOW_BLUR_OF_THE_BORDER: f32 = 0.5;
const SHADOW_REACH_IN_BLURS: f32 = 3.0;
const RIM_BEYOND_THE_REACH: f32 = 1.0;

thread_local! {
    static EDGE_UNDER_THE_POINTER: Cell<Option<ResizeEdge>> = const { Cell::new(None) };
}
const CONTROL_GROUP: &str = "window-control";

#[derive(Clone, Copy)]
enum Control {
    Minimise,
    Maximise,
    Restore,
    Close,
}

impl Control {
    fn mark(self) -> Svg {
        let icon = match self {
            Self::Minimise => Icon::WindowMinimise,
            Self::Maximise => Icon::WindowMaximise,
            Self::Restore => Icon::WindowRestore,
            Self::Close => Icon::WindowClose,
        };
        icons::lit_on_hover(
            icons::icon(icon, mark_edge(), theme::muted()),
            CONTROL_GROUP,
        )
    }

    const fn id(self) -> &'static str {
        match self {
            Self::Minimise => "minimise-window",
            Self::Maximise => "maximise-window",
            Self::Restore => "restore-window",
            Self::Close => "close-window",
        }
    }

    const fn saying(self) -> &'static str {
        match self {
            Self::Minimise => "Minimise the window",
            Self::Maximise => "Fill the screen",
            Self::Restore => "Back to the window's own size",
            Self::Close => keyed!("Close Resonate", key!(quit)),
        }
    }

    fn hover(self) -> u32 {
        match self {
            Self::Minimise | Self::Maximise | Self::Restore => theme::hover(),
            Self::Close => theme::close_hover(),
        }
    }

    fn act(self, window: &mut Window) {
        match self {
            Self::Minimise => window.minimize_window(),
            Self::Maximise | Self::Restore => window.zoom_window(),
            Self::Close => window.remove_window(),
        }
    }
}

impl RootView {
    pub(crate) fn window_controls(&self, window: &Window, cx: &App) -> Div {
        let offered = window.window_controls();
        let shown = cx.global::<ResonateApp>().window_buttons;
        let zoom = if window.is_maximized() {
            Control::Restore
        } else {
            Control::Maximise
        };

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_1()
            .mr(-theme::width(mark_inset()))
            .when(offered.minimize && shown.minimise, |row| {
                row.child(window_control(Control::Minimise))
            })
            .when(offered.maximize && shown.maximise, |row| {
                row.child(window_control(zoom))
            })
            .child(window_control(Control::Close))
    }
}

fn window_control(control: Control) -> Stateful<Div> {
    div()
        .id(control.id())
        .group(CONTROL_GROUP)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(theme::width(theme::WINDOW_CONTROL))
        .rounded_md()
        .cursor_pointer()
        .hover(|button| button.bg(rgb(control.hover())))
        .names(control.saying())
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, window, _| control.act(window))
        .child(control.mark())
}

const fn mark_edge() -> f32 {
    theme::WINDOW_MARK - theme::WINDOW_MARK_OVERLAP
}

const fn mark_inset() -> f32 {
    (theme::WINDOW_CONTROL - mark_edge()) / 2.0
}

pub(crate) fn client_side(window: &Window) -> bool {
    matches!(window.window_decorations(), Decorations::Client { .. })
}

pub(crate) fn titlebar(bar: Div) -> Div {
    bar.on_mouse_down(
        MouseButton::Left,
        |event: &MouseDownEvent, window, _| match event.click_count {
            1 => window.start_window_move(),
            _ => window.zoom_window(),
        },
    )
    .on_mouse_down(MouseButton::Right, |event: &MouseDownEvent, window, _| {
        window.show_window_menu(event.position);
    })
}

pub(crate) fn rounded(window: &Window) -> Corners<Pixels> {
    let Decorations::Client { tiling } = window.window_decorations() else {
        return Corners::default();
    };
    let rounding = theme::width(theme::CORNER_RADIUS);
    let square = px(0.0);
    let round_unless = |held: bool| if held { square } else { rounding };

    Corners {
        top_left: round_unless(tiling.top || tiling.left),
        top_right: round_unless(tiling.top || tiling.right),
        bottom_left: round_unless(tiling.bottom || tiling.left),
        bottom_right: round_unless(tiling.bottom || tiling.right),
    }
}

pub(crate) fn rounded_within_the_frame(window: &Window) -> Corners<Pixels> {
    let inside = |outer: Pixels| (outer - theme::width(theme::FRAME_BORDER)).max(px(0.0));
    let outer = rounded(window);

    Corners {
        top_left: inside(outer.top_left),
        top_right: inside(outer.top_right),
        bottom_left: inside(outer.bottom_left),
        bottom_right: inside(outer.bottom_right),
    }
}

pub(crate) fn frame(window: &mut Window, content: Div) -> Div {
    let Decorations::Client { tiling } = window.window_decorations() else {
        return content.size_full();
    };

    let border = theme::width(theme::RESIZE_BORDER);
    let corners = rounded(window);
    window.set_client_inset(border);
    let held = held_edges(window, tiling);

    div()
        .size_full()
        .child(resize_cursor(border, held))
        .when(!tiling.is_tiled(), |backdrop| {
            backdrop.children(shadow_rim(border, corners))
        })
        .when(!tiling.top, |backdrop| backdrop.pt(border))
        .when(!tiling.bottom, |backdrop| backdrop.pb(border))
        .when(!tiling.left, |backdrop| backdrop.pl(border))
        .when(!tiling.right, |backdrop| backdrop.pr(border))
        .on_mouse_move(move |event: &MouseMoveEvent, window, _| {
            let size = window.window_bounds().get_bounds().size;
            let edge = grabbed_edge(event.position, border, size, held);
            if EDGE_UNDER_THE_POINTER.replace(edge) != edge {
                window.refresh();
            }
        })
        .on_mouse_down(
            MouseButton::Left,
            move |event: &MouseDownEvent, window, _| {
                let size = window.window_bounds().get_bounds().size;
                if let Some(edge) = grabbed_edge(event.position, border, size, held) {
                    window.start_window_resize(edge);
                }
            },
        )
        .child(
            content
                .size_full()
                .overflow_hidden()
                .border(theme::width(theme::FRAME_BORDER))
                .border_color(rgb(theme::border()))
                .rounded_tl(corners.top_left)
                .rounded_tr(corners.top_right)
                .rounded_bl(corners.bottom_left)
                .rounded_br(corners.bottom_right)
                .on_mouse_move(|_, _, cx| cx.stop_propagation()),
        )
}

fn held_edges(window: &Window, tiling: Tiling) -> Tiling {
    if window.is_maximized() || window.is_fullscreen() {
        return Tiling::tiled();
    }
    tiling
}

fn shadow_rim(border: Pixels, corners: Corners<Pixels>) -> [Div; 4] {
    let blur = border * SHADOW_BLUR_OF_THE_BORDER;
    let rim = (blur * SHADOW_REACH_IN_BLURS + px(SHADOW_DROP + RIM_BEYOND_THE_REACH))
        .max(corners.top_left.max(corners.bottom_left))
        .max(corners.top_right.max(corners.bottom_right));
    let cast = || {
        div().absolute().shadow(vec![BoxShadow {
            color: hsla(0.0, 0.0, 0.0, SHADOW_ALPHA),
            offset: point(px(0.0), px(SHADOW_DROP)),
            blur_radius: blur,
            spread_radius: px(0.0),
        }])
    };
    let across = || cast().left(border).right(border).h(rim);
    let down = || cast().top(border + rim).bottom(border + rim).w(rim);

    [
        across()
            .top(border)
            .rounded_tl(corners.top_left)
            .rounded_tr(corners.top_right),
        across()
            .bottom(border)
            .rounded_bl(corners.bottom_left)
            .rounded_br(corners.bottom_right),
        down().left(border),
        down().right(border),
    ]
}

fn resize_cursor(border: Pixels, held: Tiling) -> AnyElement {
    canvas(
        |_, window, _| {
            window.insert_hitbox(
                Bounds::new(
                    point(px(0.0), px(0.0)),
                    window.window_bounds().get_bounds().size,
                ),
                HitboxBehavior::Normal,
            )
        },
        move |_, hitbox, window, _| {
            let size = window.window_bounds().get_bounds().size;
            let Some(edge) = grabbed_edge(window.mouse_position(), border, size, held) else {
                return;
            };
            window.set_cursor_style(cursor_over(edge), &hitbox);
        },
    )
    .absolute()
    .size_full()
    .into_any_element()
}

fn grabbed_edge(
    at: Point<Pixels>,
    border: Pixels,
    size: Size<Pixels>,
    held: Tiling,
) -> Option<ResizeEdge> {
    let top = !held.top && at.y < border;
    let bottom = !held.bottom && at.y > size.height - border;
    let left = !held.left && at.x < border;
    let right = !held.right && at.x > size.width - border;

    match (top, bottom, left, right) {
        (true, _, true, _) => Some(ResizeEdge::TopLeft),
        (true, _, _, true) => Some(ResizeEdge::TopRight),
        (_, true, true, _) => Some(ResizeEdge::BottomLeft),
        (_, true, _, true) => Some(ResizeEdge::BottomRight),
        (true, _, _, _) => Some(ResizeEdge::Top),
        (_, true, _, _) => Some(ResizeEdge::Bottom),
        (_, _, true, _) => Some(ResizeEdge::Left),
        (_, _, _, true) => Some(ResizeEdge::Right),
        (false, false, false, false) => None,
    }
}

const fn cursor_over(edge: ResizeEdge) -> CursorStyle {
    match edge {
        ResizeEdge::Top | ResizeEdge::Bottom => CursorStyle::ResizeUpDown,
        ResizeEdge::Left | ResizeEdge::Right => CursorStyle::ResizeLeftRight,
        ResizeEdge::TopLeft | ResizeEdge::BottomRight => CursorStyle::ResizeUpLeftDownRight,
        ResizeEdge::TopRight | ResizeEdge::BottomLeft => CursorStyle::ResizeUpRightDownLeft,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BORDER: f32 = 10.0;

    fn grabbed(x: f32, y: f32, held: Tiling) -> Option<ResizeEdge> {
        grabbed_edge(
            point(px(x), px(y)),
            px(BORDER),
            Size {
                width: px(1_920.0),
                height: px(1_080.0),
            },
            held,
        )
    }

    #[test]
    fn a_press_along_a_free_edge_resizes_the_window() {
        let free = Tiling::default();

        assert_eq!(grabbed(960.0, 1_075.0, free), Some(ResizeEdge::Bottom));
        assert_eq!(grabbed(2.0, 2.0, free), Some(ResizeEdge::TopLeft));
        assert_eq!(grabbed(960.0, 540.0, free), None);
    }

    #[test]
    fn a_press_along_an_edge_held_to_the_screen_reaches_what_is_drawn_there() {
        assert_eq!(
            grabbed(960.0, 1_075.0, Tiling::tiled()),
            None,
            "a press on the bottom of a maximised window started a resize"
        );
        assert_eq!(grabbed(1_915.0, 1_075.0, Tiling::tiled()), None);

        let left_half = Tiling {
            top: true,
            bottom: true,
            left: true,
            right: false,
        };
        assert_eq!(grabbed(960.0, 1_075.0, left_half), None);
        assert_eq!(grabbed(1_915.0, 540.0, left_half), Some(ResizeEdge::Right));
    }
}
