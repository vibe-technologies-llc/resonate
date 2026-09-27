use std::sync::Arc;

use gpui::{
    Background, Div, FontWeight, Image, ObjectFit, SharedString, div, img, linear_color_stop,
    linear_gradient, prelude::*, px, rgb,
};
use resonate_core::Accent;

use crate::{
    icons::{self, Icon},
    theme,
};

pub(crate) const TILES_A_SIDE: usize = 2;

pub(crate) const TILES: usize = TILES_A_SIDE * TILES_A_SIDE;

const NAME_ON_THE_ART: f32 = 0.11;

const MARK_ON_THE_ART: f32 = 0.22;

const LETTERED_INSET: f32 = 0.08;

const LETTERED_LEADING: f32 = 1.1;

const LETTERED_LINES: usize = 3;

pub(crate) const ART_ROUNDING: f32 = 8.0;

const MARK_ALPHA: u8 = 0x9c;

const HAIRLINE_ALPHA: u8 = 0x0c;

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

const FNV_PRIME: u64 = 0x0100_0000_01b3;

pub(crate) struct Mosaic<'a> {
    pub(crate) drawn: &'a [Arc<Image>],
    pub(crate) ground: (Accent, Accent),
    pub(crate) mark: Icon,
    pub(crate) name: &'a str,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Framed {
    Alone,
    OnACard,
}

impl Mosaic<'_> {
    pub(crate) fn drawn(&self, side: f32, framed: Framed) -> Div {
        let (from, to) = (theme::hue(self.ground.0), theme::hue(self.ground.1));
        let side = side.round();
        let rounding = px(ART_ROUNDING);

        let frame = div().relative().flex_none().overflow_hidden();
        let frame = match framed {
            Framed::OnACard => frame
                .w_full()
                .h(px(side))
                .rounded_tl(rounding)
                .rounded_tr(rounding),
            Framed::Alone => frame.size(px(side)).rounded(rounding),
        };

        let art = match self.drawn {
            [] => frame
                .bg(ground(from, to))
                .child(lettered(self.mark, self.name, side, from)),
            [only] => frame.child({
                let image = img(Arc::clone(only))
                    .size(px(side))
                    .object_fit(ObjectFit::Cover);
                match framed {
                    Framed::OnACard => image.rounded_tl(rounding).rounded_tr(rounding),
                    Framed::Alone => image.rounded(rounding),
                }
            }),
            several => frame.child(tiled(several, side, (from, to), framed)),
        };

        match framed {
            Framed::OnACard => art,
            Framed::Alone => art.child(hairline()),
        }
    }
}

fn hairline() -> Div {
    div()
        .absolute()
        .inset_0()
        .rounded(px(ART_ROUNDING))
        .border_1()
        .border_color(theme::tinted(theme::text(), HAIRLINE_ALPHA))
}

pub(crate) fn named_accents(name: &str) -> (Accent, Accent) {
    let hashed = name.bytes().fold(FNV_OFFSET_BASIS, |hashed, byte| {
        (hashed ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
    });
    let accents = Accent::ALL.len() as u64;
    let first = (hashed % accents) as usize;
    let apart = 1 + ((hashed / accents) % (accents - 1)) as usize;

    (
        Accent::ALL[first],
        Accent::ALL[(first + apart) % Accent::ALL.len()],
    )
}

fn lettered(mark: Icon, name: &str, side: f32, ground: u32) -> Div {
    let ink = theme::ink_over(ground);

    div()
        .absolute()
        .inset_0()
        .flex()
        .flex_col()
        .justify_between()
        .p(px(side * LETTERED_INSET))
        .child(icons::icon(mark, side * MARK_ON_THE_ART, ink))
        .child(
            div()
                .text_size(px(side * NAME_ON_THE_ART))
                .line_height(px(side * NAME_ON_THE_ART * LETTERED_LEADING))
                .font_weight(FontWeight::BOLD)
                .text_color(theme::tinted(ink, MARK_ALPHA))
                .line_clamp(LETTERED_LINES)
                .child(SharedString::from(name.to_owned())),
        )
}

fn ground(from: u32, to: u32) -> Background {
    linear_gradient(
        135.0,
        linear_color_stop(rgb(from), 0.0),
        linear_color_stop(rgb(to), 1.0),
    )
}

fn tiled(drawn: &[Arc<Image>], side: f32, (from, to): (u32, u32), framed: Framed) -> Div {
    let mut grid = div().flex().flex_wrap().size(px(side));
    for at in 0..TILES {
        let corner = Corner::of_tile(at, framed);
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
            None => cell.bg(rgb(if at % 2 == 0 { from } else { to })),
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
    Square,
}

impl Corner {
    const fn of_tile(at: usize, framed: Framed) -> Self {
        let right = at % TILES_A_SIDE == TILES_A_SIDE - 1;
        let bottom = at / TILES_A_SIDE == TILES_A_SIDE - 1;
        match (bottom, right, framed) {
            (false, false, _) => Self::TopLeft,
            (false, true, _) => Self::TopRight,
            (true, _, Framed::OnACard) => Self::Square,
            (true, false, Framed::Alone) => Self::BottomLeft,
            (true, true, Framed::Alone) => Self::BottomRight,
        }
    }

    fn round<E: Styled>(self, element: E) -> E {
        let rounding = px(ART_ROUNDING);
        match self {
            Self::TopLeft => element.rounded_tl(rounding),
            Self::TopRight => element.rounded_tr(rounding),
            Self::BottomLeft => element.rounded_bl(rounding),
            Self::BottomRight => element.rounded_br(rounding),
            Self::Square => element,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Corner, Framed, TILES, TILES_A_SIDE, named_accents, tile_span};

    #[test]
    fn the_tiles_of_a_side_fill_it_to_the_pixel_whether_it_is_odd_or_even() {
        for side in [120.0_f32, 121.0, 163.0, 164.0] {
            let filled: f32 = (0..TILES_A_SIDE).map(|place| tile_span(side, place)).sum();

            assert_eq!(filled, side, "a side of {side} was left short");
        }
    }

    #[test]
    fn a_name_is_grounded_in_two_different_accents_and_the_same_two_every_time() {
        for name in ["Progressive Rock", "The Orbiters", "", "Trip Hop", "Jazz"] {
            let (from, to) = named_accents(name);

            assert_ne!(from, to, "{name} was grounded in one accent");
            assert_eq!(
                named_accents(name),
                (from, to),
                "{name} moved between reads"
            );
        }
    }

    #[test]
    fn each_tile_rounds_the_one_corner_it_stands_in_and_a_card_keeps_its_foot_square() {
        let alone: Vec<Corner> = (0..TILES)
            .map(|at| Corner::of_tile(at, Framed::Alone))
            .collect();
        let carded: Vec<Corner> = (0..TILES)
            .map(|at| Corner::of_tile(at, Framed::OnACard))
            .collect();

        assert_eq!(
            alone,
            [
                Corner::TopLeft,
                Corner::TopRight,
                Corner::BottomLeft,
                Corner::BottomRight
            ]
        );
        assert_eq!(
            carded,
            [
                Corner::TopLeft,
                Corner::TopRight,
                Corner::Square,
                Corner::Square
            ]
        );
    }
}
