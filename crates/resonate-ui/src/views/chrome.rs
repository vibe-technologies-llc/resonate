use gpui::{
    AnyElement, App, Bounds, BoxShadow, Corners, CursorStyle, Decorations, Div, HitboxBehavior,
    MouseButton, MouseDownEvent, Pixels, Point, ResizeEdge, Size, Stateful, Svg, Tiling, Window,
    canvas, div, hsla, point, prelude::*, px, rgb,
};

use crate::{
    ResonateApp, RootView,
    icons::{self, Icon},
    theme,
    views::hint::Names,
};

const SHADOW_ALPHA: f32 = 0.45;
const CONTROL_GROUP: &str = "window-control";

#[derive(Clone, Copy)]
enum Control {
    Minimise,
    Maximise,
    Restore,
    Close,
}

impl Control {
    fn mark(self) -> AnyElement {
        match self {
            Self::Minimise => bar().into_any_element(),
            Self::Close => cross().into_any_element(),
            Self::Maximise => outline().into_any_element(),
            Self::Restore => div()
                .relative()
                .size(theme::width(theme::WINDOW_MARK))
                .child(outline().absolute().top_0().right_0())
                .child(
                    outline()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .bg(rgb(theme::surface())),
                )
                .into_any_element(),
        }
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
            Self::Close => "Close Resonate — ctrl-q",
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

fn bar() -> Div {
    div()
        .w(theme::width(mark_edge()))
        .h(px(1.0))
        .bg(rgb(theme::muted()))
        .group_hover(CONTROL_GROUP, |bar| bar.bg(rgb(theme::text())))
}

fn cross() -> Svg {
    icons::lit_on_hover(
        icons::icon(Icon::WindowClose, mark_edge(), theme::muted()),
        CONTROL_GROUP,
    )
}

fn outline() -> Div {
    div()
        .size(theme::width(mark_edge()))
        .border_1()
        .border_color(rgb(theme::muted()))
        .group_hover(CONTROL_GROUP, |square| {
            square.border_color(rgb(theme::text()))
        })
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
        .when(!tiling.top, |backdrop| backdrop.pt(border))
        .when(!tiling.bottom, |backdrop| backdrop.pb(border))
        .when(!tiling.left, |backdrop| backdrop.pl(border))
        .when(!tiling.right, |backdrop| backdrop.pr(border))
        .on_mouse_move(|_, window, _| window.refresh())
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
                .when(!tiling.is_tiled(), |frame| {
                    frame.shadow(vec![BoxShadow {
                        color: hsla(0.0, 0.0, 0.0, SHADOW_ALPHA),
                        offset: point(px(0.0), px(2.0)),
                        blur_radius: border / 2.0,
                        spread_radius: px(0.0),
                    }])
                })
                .on_mouse_move(|_, _, cx| cx.stop_propagation()),
        )
}

fn held_edges(window: &Window, tiling: Tiling) -> Tiling {
    if window.is_maximized() || window.is_fullscreen() {
        return Tiling::tiled();
    }
    tiling
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
