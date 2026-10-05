use std::{cell::Cell, rc::Rc};

use gpui::{
    AnyElement, App, Div, ElementId, Font, FontWeight, Pixels, ScrollHandle, SharedString,
    Stateful, canvas, div, point, prelude::*, px, rgb,
};
use resonate_core::{Appearance, StreamSpec};
use resonate_library::Codec;

use crate::{
    format,
    icons::{self, Icon},
    motion, theme,
    views::{hint::Names, listing, pointed::LitUnderThePointer},
};

const BUTTON_GROUP: &str = "button";

const WAY_BACK_GROUP: &str = "way-back";

const GREYED: f32 = 0.5;

pub(crate) const LIT: f32 = 0.82;

const CHOICE_LEADING: f32 = 1.5;

const RADIO: f32 = 14.0;

const RADIO_DOT: f32 = 6.0;

const DETAIL_SEPARATOR: &str = "·";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tone {
    Ghost,
    Outlined,
    Primary,
    Destructive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Press {
    Takes,
    Greyed,
}

impl Press {
    const fn takes(self) -> bool {
        matches!(self, Self::Takes)
    }
}

pub(crate) fn figure(text: impl Into<SharedString>) -> Div {
    div()
        .font(theme::mono(FontWeight::NORMAL))
        .text_size(px(theme::text_xs()))
        .text_color(rgb(theme::muted()))
        .whitespace_nowrap()
        .child(text.into())
}

pub(crate) fn readout(text: impl Into<SharedString>) -> Div {
    div()
        .font(theme::ui(FontWeight::NORMAL))
        .text_size(px(theme::text_xs()))
        .text_color(rgb(theme::muted()))
        .whitespace_nowrap()
        .child(text.into())
}

pub(crate) fn eyebrow(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(theme::text_xs()))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(theme::faint()))
        .whitespace_nowrap()
        .child(text.into())
}

pub(crate) fn title(text: impl IntoElement) -> Div {
    title_face().truncate().ends_in_an_ellipsis().child(text)
}

pub(crate) fn linked_title(link: Stateful<Div>) -> Div {
    title_face()
        .flex()
        .min_w(px(0.0))
        .child(link.keeps_its_width())
}

fn title_face() -> Div {
    div()
        .text_size(px(theme::text_xl()))
        .line_height(px(theme::text_xl() * 1.2))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(theme::text()))
}

pub(crate) fn subtitle(text: impl IntoElement) -> Div {
    subtitle_face().truncate().ends_in_an_ellipsis().child(text)
}

pub(crate) fn linked_subtitle(link: Stateful<Div>) -> Div {
    subtitle_face()
        .flex()
        .min_w(px(0.0))
        .child(link.keeps_its_width())
}

fn subtitle_face() -> Div {
    div()
        .text_size(px(theme::text_sm()))
        .text_color(rgb(theme::muted()))
}

pub(crate) trait KeepsItsWidth: Styled + Sized {
    fn keeps_its_width(self) -> Self {
        self.flex_none().max_w_full().whitespace_nowrap()
    }
}

impl<T: Styled> KeepsItsWidth for T {}

pub(crate) trait EndsInAnEllipsis: Styled + Sized {
    fn ends_in_an_ellipsis(self) -> Self {
        self.whitespace_normal().line_clamp(1)
    }
}

impl<T: Styled> EndsInAnEllipsis for T {}

const ELLIPSIS: &str = "…";

pub(crate) fn cut_to_fit(
    text: SharedString,
    room: Pixels,
    font: Font,
    size: Pixels,
    cx: &App,
) -> SharedString {
    if room <= px(0.0) {
        return text;
    }
    cx.text_system()
        .line_wrapper(font, size)
        .truncate_line(text, room, ELLIPSIS, &mut Vec::new())
}

pub(crate) fn width_of(text: &str, font: &Font, size: Pixels, cx: &App) -> Pixels {
    let system = cx.text_system();
    let id = system.resolve_font(font);
    text.chars()
        .map(|ch| {
            system
                .advance(id, size, ch)
                .map_or(size, |advance| advance.width)
        })
        .fold(px(0.0), |width, advance| width + advance)
}

pub(crate) fn button(
    id: impl Into<ElementId>,
    icon: Option<Icon>,
    label: impl Into<SharedString>,
    saying: impl Into<SharedString>,
    tone: Tone,
) -> Stateful<Div> {
    button_when(Press::Takes, id, icon, label, saying, tone)
}

pub(crate) fn button_when(
    press: Press,
    id: impl Into<ElementId>,
    icon: Option<Icon>,
    label: impl Into<SharedString>,
    saying: impl Into<SharedString>,
    tone: Tone,
) -> Stateful<Div> {
    let (fill, ink, edge) = match tone {
        Tone::Ghost => (theme::UNMARKED, theme::muted(), theme::UNMARKED),
        Tone::Outlined => (theme::raised(), theme::text(), theme::outline()),
        Tone::Primary => (theme::accent(), theme::accent_ink(), theme::UNMARKED),
        Tone::Destructive => (theme::raised(), theme::failure(), theme::failure()),
    };
    let hovered = match tone {
        Tone::Ghost | Tone::Outlined | Tone::Destructive => theme::hover(),
        Tone::Primary => theme::accent(),
    };

    let id = id.into();

    div()
        .id(id.clone())
        .found_as(&id)
        .group(BUTTON_GROUP)
        .flex()
        .flex_none()
        .items_center()
        .gap_1p5()
        .h(px(28.0))
        .px_2p5()
        .rounded_md()
        .text_size(px(theme::text_sm()))
        .font_weight(FontWeight::MEDIUM)
        .whitespace_nowrap()
        .bg(theme::tinted(fill, fill_alpha(tone)))
        .border_1()
        .border_color(theme::tinted(edge, fill_alpha(tone)))
        .text_color(rgb(ink))
        .when_else(
            press.takes(),
            |button| {
                button
                    .cursor_pointer()
                    .hover(move |button| button.bg(theme::tinted(hovered, hover_alpha(tone))))
                    .lit_under_the_pointer(id, |button| {
                        button.text_color(rgb(match tone {
                            Tone::Ghost | Tone::Outlined => theme::text(),
                            Tone::Primary => theme::accent_ink(),
                            Tone::Destructive => theme::failure(),
                        }))
                    })
            },
            |button| button.opacity(GREYED).cursor_default(),
        )
        .names(saying)
        .when_some(icon, |button, icon| {
            let drawn = icons::icon(icon, theme::row_control_icon(), ink);
            button.child(match (tone, press) {
                (Tone::Primary | Tone::Destructive, _) | (_, Press::Greyed) => drawn,
                (Tone::Ghost | Tone::Outlined, Press::Takes) => {
                    icons::lit_on_hover(drawn, BUTTON_GROUP)
                }
            })
        })
        .child(label.into())
}

const fn fill_alpha(tone: Tone) -> u8 {
    match tone {
        Tone::Ghost => 0x00,
        Tone::Outlined | Tone::Primary | Tone::Destructive => 0xff,
    }
}

const fn hover_alpha(tone: Tone) -> u8 {
    match tone {
        Tone::Ghost | Tone::Outlined | Tone::Destructive => 0xff,
        Tone::Primary => 0xe6,
    }
}

pub(crate) fn icon_button(
    id: impl Into<ElementId>,
    icon: Icon,
    saying: impl Into<SharedString>,
) -> Stateful<Div> {
    mark_when(Press::Takes, id, icon, saying, BUTTON_GROUP)
}

pub(crate) fn lit_mark(
    id: impl Into<ElementId>,
    icon: Icon,
    saying: impl Into<SharedString>,
) -> Stateful<Div> {
    marked(
        Press::Takes,
        id,
        icons::icon(icon, theme::row_control_icon(), theme::accent()),
        saying,
    )
}

pub(crate) fn mark_when(
    press: Press,
    id: impl Into<ElementId>,
    icon: Icon,
    saying: impl Into<SharedString>,
    lit_with: &'static str,
) -> Stateful<Div> {
    let drawn = icons::icon(icon, theme::row_control_icon(), theme::muted());
    let drawn = match press {
        Press::Takes => icons::lit_on_hover(drawn, lit_with),
        Press::Greyed => drawn,
    };

    marked(press, id, drawn, saying)
}

pub(crate) fn star(
    id: impl Into<ElementId>,
    favoured: bool,
    which: SharedString,
    saying: impl Into<SharedString>,
) -> Stateful<Div> {
    let drawn = match favoured {
        true => icons::icon(Icon::Favourited, theme::row_control_icon(), theme::accent()),
        false => icons::lit_on_hover(
            icons::icon(Icon::Favourite, theme::row_control_icon(), theme::muted()),
            BUTTON_GROUP,
        ),
    };

    marked(
        Press::Takes,
        id,
        motion::starred(drawn, which, favoured),
        saying,
    )
}

fn marked(
    press: Press,
    id: impl Into<ElementId>,
    drawn: impl IntoElement,
    saying: impl Into<SharedString>,
) -> Stateful<Div> {
    let id = id.into();
    div()
        .id(id.clone())
        .found_as(&id)
        .group(BUTTON_GROUP)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(theme::row_control()))
        .rounded_md()
        .when_else(
            press.takes(),
            |button| {
                button
                    .cursor_pointer()
                    .hover(|button| button.bg(rgb(theme::hover())))
            },
            |button| button.opacity(GREYED).cursor_default(),
        )
        .names(saying)
        .child(drawn)
}

pub(crate) fn chip(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
) -> Stateful<Div> {
    pill(id, theme::accent(), selected).child(label.into())
}

fn pill(id: impl Into<ElementId>, colour: u32, selected: bool) -> Stateful<Div> {
    let id = id.into();

    div()
        .id(id.clone())
        .found_as(&id)
        .flex()
        .flex_none()
        .items_center()
        .gap_1p5()
        .h(px(26.0))
        .px_2p5()
        .rounded_full()
        .text_size(px(theme::text_sm()))
        .cursor_pointer()
        .whitespace_nowrap()
        .border_1()
        .when_else(
            selected,
            |chip| {
                chip.bg(theme::tinted(colour, 0x1f))
                    .border_color(theme::tinted(colour, 0x80))
                    .text_color(rgb(theme::text()))
                    .font_weight(FontWeight::MEDIUM)
            },
            |chip| {
                chip.bg(rgb(theme::raised()))
                    .border_color(rgb(theme::border()))
                    .text_color(rgb(theme::muted()))
                    .hover(|chip| chip.bg(rgb(theme::hover())))
                    .lit_under_the_pointer(id, |chip| chip.text_color(rgb(theme::text())))
            },
        )
}

pub(crate) fn tag(text: impl Into<SharedString>) -> Div {
    div()
        .flex_none()
        .px_2p5()
        .py_0p5()
        .rounded_full()
        .bg(rgb(theme::raised()))
        .border_1()
        .border_color(rgb(theme::border()))
        .text_size(px(theme::text_xs()))
        .text_color(rgb(theme::muted()))
        .whitespace_nowrap()
        .child(text.into())
}

pub(crate) fn badge(text: impl Into<SharedString>, colour: u32) -> Div {
    div()
        .flex_none()
        .px_1p5()
        .py_0p5()
        .rounded_sm()
        .bg(theme::tinted(colour, 0x1c))
        .font_family(theme::mono_face())
        .text_size(px(theme::text_xs()))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(colour))
        .whitespace_nowrap()
        .child(text.into())
}

pub(crate) fn codec_badge(codec: Codec) -> Div {
    let colour = if codec.is_lossless() {
        theme::lossless()
    } else {
        theme::lossy()
    };

    badge(codec.as_str(), colour)
}

pub(crate) fn format_badge(codec: Codec, spec: StreamSpec) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .child(codec_badge(codec))
        .child(figure(format::quality(spec)))
}

pub(crate) fn mode_dot(colour: u32) -> Div {
    div()
        .flex_none()
        .size(px(7.0))
        .rounded_full()
        .bg(rgb(colour))
}

pub(crate) fn section() -> Div {
    div()
        .flex()
        .flex_col()
        .rounded_xl()
        .bg(rgb(theme::surface()))
        .border_1()
        .border_color(rgb(theme::border()))
}

const SECTION_HEADER_PADDING: f32 = 17.0;

pub(crate) fn section_header() -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .px_4()
        .py_2()
        .min_h(px(theme::row_control() + SECTION_HEADER_PADDING))
        .border_b_1()
        .border_color(rgb(theme::border()))
}

pub(crate) fn section_name(text: impl Into<SharedString>) -> Div {
    div()
        .flex_1()
        .min_w(px(0.0))
        .text_size(px(theme::text_xs()))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(theme::faint()))
        .child(text.into())
}

pub(crate) fn section_body() -> Div {
    div().flex().flex_col().gap_4().px_4().py_3()
}

pub(crate) fn field(label: impl Into<SharedString>, control: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_size(px(theme::text_xs()))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(theme::muted()))
                .child(label.into()),
        )
        .child(control)
}

pub(crate) fn segmented() -> Div {
    div()
        .flex()
        .flex_none()
        .flex_wrap()
        .p_0p5()
        .gap_0p5()
        .rounded_lg()
        .bg(rgb(theme::raised()))
        .border_1()
        .border_color(rgb(theme::border()))
}

pub(crate) fn segment(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    chosen: bool,
) -> Stateful<Div> {
    let id = id.into();

    div()
        .id(id.clone())
        .found_as(&id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .px_4()
        .py_1()
        .rounded_md()
        .text_size(px(theme::text_xs()))
        .whitespace_nowrap()
        .cursor_pointer()
        .relative()
        .child(motion::shown_while(
            div()
                .absolute()
                .inset_0()
                .rounded_md()
                .bg(rgb(theme::hover())),
            "chosen",
            chosen,
        ))
        .when_else(
            chosen,
            |option| {
                option
                    .text_color(rgb(theme::text()))
                    .font_weight(FontWeight::MEDIUM)
            },
            |option| {
                option
                    .text_color(rgb(theme::muted()))
                    .lit_under_the_pointer(id, |option| option.text_color(rgb(theme::text())))
            },
        )
        .child(label.into())
}

pub(crate) fn switch(id: impl Into<ElementId>, on: bool) -> Stateful<Div> {
    let id = id.into();

    div()
        .id(id.clone())
        .found_as(&id)
        .relative()
        .flex_none()
        .w(theme::width(theme::switch_track()))
        .h(theme::width(theme::switch_height()))
        .rounded_full()
        .cursor_pointer()
        .child(motion::flips(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .p_0p5()
                .rounded_full(),
            "thrown",
            on,
            |track, from, to, share| {
                let at = motion::turned(from, to, share);

                track
                    .bg(rgb(motion::blended(theme::outline(), theme::accent(), at)))
                    .child(grown(div(), at))
                    .child(
                        div()
                            .size(theme::width(theme::switch_knob()))
                            .flex_none()
                            .rounded_full()
                            .bg(rgb(motion::blended(
                                theme::faint(),
                                theme::accent_ink(),
                                at,
                            ))),
                    )
                    .child(grown(div(), 1.0 - at))
            },
        ))
}

fn grown(spacer: Div, by: f32) -> Div {
    let mut spacer = spacer.flex_basis(px(0.0)).h_full();
    spacer.style().flex_grow = Some(by);
    spacer
}

pub(crate) fn preview(dressed: Appearance) -> Div {
    let sample = theme::sample(dressed);

    div()
        .flex()
        .h(theme::width(theme::theme_swatch_strip()))
        .overflow_hidden()
        .rounded_md()
        .child(
            div()
                .flex_none()
                .h_full()
                .w(theme::width(theme::theme_swatch_sidebar()))
                .bg(rgb(sample.surface)),
        )
        .child(div().flex_1().h_full().bg(rgb(sample.background)))
        .child(
            div()
                .flex_none()
                .h_full()
                .w(theme::width(theme::theme_swatch_raised()))
                .bg(rgb(sample.raised)),
        )
        .child(
            div()
                .flex_none()
                .h_full()
                .w(theme::width(theme::theme_swatch_accent()))
                .bg(rgb(sample.accent)),
        )
}

pub(crate) fn dot_swatch(id: impl Into<ElementId>, colour: u32, chosen: bool) -> Stateful<Div> {
    let id = id.into();
    div()
        .id(id.clone())
        .found_as(&id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(theme::width(theme::accent_swatch()))
        .rounded_full()
        .bg(rgb(colour))
        .cursor_pointer()
        .when_else(
            chosen,
            |swatch| {
                swatch.child(icons::icon(
                    Icon::Check,
                    theme::check_mark(),
                    theme::ink_over(colour),
                ))
            },
            |swatch| swatch.hover(|swatch| swatch.opacity(0.85)),
        )
}

pub(crate) fn choice_row(id: impl Into<ElementId>, chosen: bool) -> Stateful<Div> {
    let id = id.into();
    div()
        .id(id.clone())
        .found_as(&id)
        .flex()
        .items_start()
        .gap_3()
        .px_3()
        .py_2p5()
        .rounded_lg()
        .cursor_pointer()
        .border_1()
        .when_else(
            chosen,
            |row| {
                row.bg(theme::tinted(theme::accent(), 0x14))
                    .border_color(rgb(theme::accent()))
            },
            |row| {
                row.bg(rgb(theme::raised()))
                    .border_color(rgb(theme::border()))
                    .hover(|row| row.bg(rgb(theme::hover())))
            },
        )
}

pub(crate) fn radio(chosen: bool) -> impl IntoElement {
    motion::flips(
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(RADIO))
            .rounded_full()
            .border_1(),
        "radio",
        chosen,
        |mark, from, to, share| {
            let at = motion::turned(from, to, share);

            mark.border_color(rgb(motion::blended(theme::outline(), theme::accent(), at)))
                .when(at > 0.0, |mark| {
                    mark.child(
                        div()
                            .size(px(RADIO_DOT * at))
                            .rounded_full()
                            .bg(rgb(theme::accent())),
                    )
                })
        },
    )
}

pub(crate) fn level_with_the_choice(mark: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(theme::text_sm() * CHOICE_LEADING))
        .child(mark)
}

pub(crate) fn choice_name(text: impl Into<SharedString>, lit: bool) -> Div {
    div()
        .flex_1()
        .min_w(px(0.0))
        .text_size(px(theme::text_sm()))
        .line_height(px(theme::text_sm() * CHOICE_LEADING))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(if lit { theme::text() } else { theme::muted() }))
        .truncate()
        .ends_in_an_ellipsis()
        .child(text.into())
}

pub(crate) fn detail(text: impl Into<SharedString>) -> Div {
    div()
        .min_w(px(0.0))
        .text_size(px(theme::text_xs()))
        .text_color(rgb(theme::faint()))
        .truncate()
        .ends_in_an_ellipsis()
        .child(text.into())
}

pub(crate) fn details(parts: impl IntoIterator<Item = Div>) -> Div {
    parts.into_iter().enumerate().fold(
        div().flex().flex_wrap().items_center().gap_x_1p5(),
        |line, (at, part)| {
            line.when(at > 0, |line| line.child(detail(DETAIL_SEPARATOR)))
                .child(part)
        },
    )
}

pub(crate) fn card() -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_4()
        .rounded_lg()
        .bg(rgb(theme::surface()))
        .border_1()
        .border_color(rgb(theme::border()))
}

pub(crate) fn card_title(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(theme::text_sm()))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(theme::text()))
        .child(text.into())
}

pub(crate) fn heading() -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .px_6()
        .pt_5()
        .pb_4()
        .border_b_1()
        .border_color(rgb(theme::border()))
}

pub(crate) fn way_back(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    saying: impl Into<SharedString>,
) -> Stateful<Div> {
    let id = id.into();

    div()
        .id(id.clone())
        .found_as(&id)
        .group(WAY_BACK_GROUP)
        .flex()
        .flex_none()
        .items_center()
        .gap_1()
        .h(px(26.0))
        .pl_1()
        .pr_2p5()
        .ml(px(-6.0))
        .rounded_md()
        .text_size(px(theme::text_sm()))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(theme::muted()))
        .whitespace_nowrap()
        .cursor_pointer()
        .hover(|back| back.bg(rgb(theme::hover())))
        .lit_under_the_pointer(id, |back| back.text_color(rgb(theme::text())))
        .names(saying)
        .child(icons::lit_on_hover(
            icons::icon(Icon::Back, theme::row_control_icon(), theme::muted()),
            WAY_BACK_GROUP,
        ))
        .child(label.into())
}

pub(crate) fn hero() -> Div {
    div().flex().items_end().gap_6()
}

pub(crate) fn hero_title(text: impl IntoElement, room: Pixels) -> Div {
    div()
        .text_size(px(theme::text_title()))
        .line_height(px(theme::text_title() * 1.15))
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(theme::text()))
        .when_else(
            room > px(0.0),
            |title| title.w(room).whitespace_normal(),
            |title| title.truncate().ends_in_an_ellipsis(),
        )
        .child(text)
}

pub(crate) fn measures_its_width(measured: Rc<Cell<Pixels>>) -> impl IntoElement {
    canvas(
        move |bounds, window, _| {
            if measured.get() != bounds.size.width {
                measured.set(bounds.size.width);
                window.request_animation_frame();
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}

#[derive(Default)]
pub(crate) struct GridWidth {
    narrower_than_the_window: Cell<Option<Pixels>>,
    window: Cell<Pixels>,
}

impl GridWidth {
    pub(crate) fn seen_in(&self, window: Pixels) {
        self.window.set(window);
    }

    pub(crate) fn get(&self) -> Pixels {
        self.narrower_than_the_window
            .get()
            .map_or(px(0.0), |narrower| {
                (self.window.get() - narrower).max(px(0.0))
            })
    }
}

pub(crate) fn measures_the_grid(grid: Rc<GridWidth>) -> impl IntoElement {
    canvas(
        move |bounds, window, _| {
            let across = window.viewport_size().width;
            let narrower = Some(across - bounds.size.width);
            grid.seen_in(across);
            if grid.narrower_than_the_window.get() != narrower {
                grid.narrower_than_the_window.set(narrower);
                window.request_animation_frame();
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}

pub(crate) fn measures_its_height(measured: Rc<Cell<Pixels>>) -> impl IntoElement {
    canvas(
        move |bounds, window, _| {
            if measured.get() != bounds.size.height {
                measured.set(bounds.size.height);
                window.request_animation_frame();
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}

pub(crate) fn brought_into_view(scroll: ScrollHandle, asked: Rc<Cell<bool>>) -> impl IntoElement {
    canvas(
        move |bounds, window, _| {
            if !asked.replace(false) {
                return;
            }
            let seen = scroll.bounds();
            let moved = if bounds.top() < seen.top() {
                seen.top() - bounds.top()
            } else if bounds.bottom() > seen.bottom() {
                (seen.bottom() - bounds.bottom()).max(seen.top() - bounds.top())
            } else {
                return;
            };
            let offset = scroll.offset();
            scroll.set_offset(point(offset.x, offset.y + moved));
            window.refresh();
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}

pub(crate) fn action_row() -> Div {
    div().flex().flex_wrap().items_center().gap_1p5()
}

pub(crate) fn wraps_within(room: Pixels) -> Div {
    div()
        .flex()
        .flex_wrap()
        .when(room > px(0.0), |row| row.w(room))
}

pub(crate) fn heading_row() -> Div {
    div().flex().flex_wrap().items_end().gap_4()
}

pub(crate) fn actions() -> Div {
    div()
        .flex()
        .flex_none()
        .flex_wrap()
        .items_center()
        .justify_end()
        .gap_1p5()
        .max_w_full()
}

pub(crate) fn column_header() -> Div {
    div()
        .flex()
        .w_full()
        .items_center()
        .gap(px(listing::COLUMN_GAP))
        .px(px(listing::ROW_INSET))
        .h(px(28.0))
        .border_b_1()
        .border_color(rgb(theme::border()))
        .text_size(px(theme::text_xs()))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(theme::faint()))
}

pub(crate) fn empty(icon: Icon, message: &'static str, more: Option<&'static str>) -> AnyElement {
    nothing_here(icon, message, more).into_any_element()
}

pub(crate) fn empty_offering(
    icon: Icon,
    message: &'static str,
    more: Option<&'static str>,
    offer: impl IntoElement,
) -> AnyElement {
    nothing_here(icon, message, more)
        .child(offer)
        .into_any_element()
}

fn nothing_here(icon: Icon, message: &'static str, more: Option<&'static str>) -> Div {
    div()
        .flex()
        .flex_1()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_3()
        .px_8()
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .size(px(56.0))
                .rounded_full()
                .bg(rgb(theme::surface()))
                .border_1()
                .border_color(rgb(theme::border()))
                .child(icons::icon(icon, theme::empty_icon(), theme::faint())),
        )
        .child(
            div()
                .text_size(px(theme::text_base()))
                .text_color(rgb(theme::muted()))
                .text_center()
                .child(message),
        )
        .when_some(more, |empty, more| {
            empty.child(
                div()
                    .text_size(px(theme::text_sm()))
                    .text_color(rgb(theme::faint()))
                    .text_center()
                    .child(more),
            )
        })
}

pub(crate) fn avatar(name: &str, lit: bool) -> Div {
    avatar_at(name, lit, theme::avatar(), theme::text_sm())
}

pub(crate) fn avatar_at(name: &str, lit: bool, side: f32, letter: f32) -> Div {
    let initial: String = name
        .chars()
        .find(|letter| letter.is_alphanumeric())
        .map(|letter| letter.to_uppercase().collect())
        .unwrap_or_else(|| "·".to_owned());

    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(side))
        .rounded_full()
        .bg(theme::tinted(
            if lit { theme::accent() } else { theme::muted() },
            0x1c,
        ))
        .text_size(px(letter))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(if lit { theme::accent() } else { theme::muted() }))
        .child(initial)
}

pub(crate) trait Found: InteractiveElement + Sized {
    fn found_as(self, id: &ElementId) -> Self {
        let named = id.clone();
        self.debug_selector(move || named.to_string())
    }
}

impl<E: InteractiveElement> Found for E {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grid_is_as_wide_as_the_window_now_less_what_it_was_narrower_by() {
        let grid = GridWidth::default();
        assert_eq!(grid.get(), px(0.0), "a grid nobody measured has a width");

        grid.seen_in(px(1_400.0));
        grid.narrower_than_the_window.set(Some(px(400.0)));
        assert_eq!(grid.get(), px(1_000.0));

        grid.seen_in(px(1_100.0));
        assert_eq!(
            grid.get(),
            px(700.0),
            "a window made narrower left the grid its old width for a frame"
        );
    }
}
