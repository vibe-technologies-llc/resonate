use std::sync::Arc;

use gpui::{Div, FontWeight, ObjectFit, SharedString, div, img, prelude::*, px, rgb};

use crate::{
    icons::{self, Icon},
    models::Picture,
    theme,
    views::kit,
};

pub(crate) const TILES_A_SIDE: usize = 2;

pub(crate) const TILES: usize = TILES_A_SIDE * TILES_A_SIDE;

const NAME_ON_THE_ART: f32 = 0.11;

const MARK_ON_THE_ART: f32 = 0.22;

const LETTERED_INSET: f32 = 0.08;

const LETTERED_LEADING: f32 = 1.1;

const LETTERED_LINES: usize = 3;

pub(crate) const ART_ROUNDING: f32 = 8.0;

pub(crate) struct Mosaic<'a> {
    pub(crate) drawn: &'a [Picture],
    pub(crate) mark: Icon,
    pub(crate) name: &'a str,
}

impl Mosaic<'_> {
    pub(crate) fn drawn(&self, side: f32) -> Div {
        let side = side.round();
        let rounding = px(ART_ROUNDING);

        let frame = div()
            .relative()
            .flex_none()
            .overflow_hidden()
            .size(px(side))
            .rounded(rounding);

        let art = match self.drawn {
            [] => frame
                .bg(rgb(theme::raised()))
                .child(lettered(self.mark, self.name, side)),
            [only] => frame.child({
                img(Arc::clone(only))
                    .size(px(side))
                    .object_fit(ObjectFit::Cover)
                    .rounded(rounding)
            }),
            several => frame.child(tiled(several, side)),
        };

        art.child(hairline())
    }
}

fn hairline() -> Div {
    div()
        .absolute()
        .inset_0()
        .rounded(px(ART_ROUNDING))
        .border_1()
        .border_color(kit::cover_edge())
}

fn lettered(mark: Icon, name: &str, side: f32) -> Div {
    div()
        .absolute()
        .inset_0()
        .flex()
        .flex_col()
        .justify_between()
        .p(px(side * LETTERED_INSET))
        .child(icons::icon(mark, side * MARK_ON_THE_ART, theme::faint()))
        .child(
            div()
                .text_size(px(side * NAME_ON_THE_ART))
                .line_height(px(side * NAME_ON_THE_ART * LETTERED_LEADING))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(theme::muted()))
                .line_clamp(LETTERED_LINES)
                .child(SharedString::from(name.to_owned())),
        )
}

fn tiled(drawn: &[Picture], side: f32) -> Div {
    let mut grid = div().flex().flex_wrap().size(px(side));
    for at in 0..TILES {
        let corner = Corner::of_tile(at);
        let (wide, high) = (
            tile_span(side, at % TILES_A_SIDE),
            tile_span(side, at / TILES_A_SIDE),
        );
        let cell = corner.round(div().w(px(wide)).h(px(high)).overflow_hidden());
        grid = grid.child(match drawn.get(at) {
            Some(art) => cell.child(
                corner.round(
                    img(Arc::clone(art))
                        .w(px(wide))
                        .h(px(high))
                        .object_fit(ObjectFit::Cover),
                ),
            ),
            None => cell.bg(rgb(if at % 2 == 0 {
                theme::raised()
            } else {
                theme::hover()
            })),
        });
    }

    grid
}

fn tile_span(side: f32, place: usize) -> f32 {
    let near = (side / TILES_A_SIDE as f32).floor();
    if place + 1 < TILES_A_SIDE {
        near
    } else {
        side - near * (TILES_A_SIDE - 1) as f32
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    const fn of_tile(at: usize) -> Self {
        let right = at % TILES_A_SIDE == TILES_A_SIDE - 1;
        let bottom = at / TILES_A_SIDE == TILES_A_SIDE - 1;
        match (bottom, right) {
            (false, false) => Self::TopLeft,
            (false, true) => Self::TopRight,
            (true, false) => Self::BottomLeft,
            (true, true) => Self::BottomRight,
        }
    }

    fn round<E: Styled>(self, element: E) -> E {
        let rounding = px(ART_ROUNDING);
        match self {
            Self::TopLeft => element.rounded_tl(rounding),
            Self::TopRight => element.rounded_tr(rounding),
            Self::BottomLeft => element.rounded_bl(rounding),
            Self::BottomRight => element.rounded_br(rounding),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Corner, TILES, TILES_A_SIDE, tile_span};

    #[test]
    fn the_tiles_of_a_side_fill_it_to_the_pixel_whether_it_is_odd_or_even() {
        for side in [120.0_f32, 121.0, 163.0, 164.0] {
            let filled: f32 = (0..TILES_A_SIDE).map(|place| tile_span(side, place)).sum();

            assert_eq!(filled, side, "a side of {side} was left short");
        }
    }

    #[test]
    fn each_tile_rounds_the_one_corner_it_stands_in() {
        let corners: Vec<Corner> = (0..TILES).map(Corner::of_tile).collect();

        assert_eq!(
            corners,
            [
                Corner::TopLeft,
                Corner::TopRight,
                Corner::BottomLeft,
                Corner::BottomRight
            ]
        );
    }
}
