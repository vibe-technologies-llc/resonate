use std::{cell::Cell, rc::Rc};

use gpui::{
    AnyElement, App, BoxShadow, Div, ElementId, Font, FontWeight, MouseButton, Pixels, Rgba, Role,
    ScrollHandle, SharedString, Stateful, TruncateFrom, canvas, div, point, prelude::*, px,
    relative, rgb, rgba,
};
use resonate_core::{Appearance, StreamSpec};
use resonate_library::Codec;

use crate::{
    format,
    icons::{self, Icon},
    motion, theme,
    views::{
        focus,
        hint::Names,
        listing,
        pointed::LitUnderThePointer,
        spoken::{Spelt, Spoken},
    },
};

const BUTTON_GROUP: &str = "button";

const WAY_BACK_GROUP: &str = "way-back";

const NAV_ROW_GROUP: &str = "nav-row";

const MARK_WASH: u8 = 0x1c;

const FIELD_LIT: u8 = 0x99;

const COVER_EDGE: u8 = 0x0c;

const COVER_VEIL: u8 = 0xb8;

const UNCOVERED_MARK_SHARE: f32 = 0.42;

const PRIMARY_PRESSED: u8 = 0xcc;

const ROW_CONTROL_SHARE: f32 = 0.6;

const CHOSEN_WASH: u8 = 0x1a;

pub(crate) const GREYED: f32 = 0.5;

pub(crate) const LIT: f32 = 0.82;

const CHOICE_LEADING: f32 = 1.5;

const RADIO: f32 = 14.0;

const RADIO_DOT: f32 = 6.0;

const TICK: f32 = 10.0;

const STEPPER_READING: f32 = 56.0;

const TAB_MARK: f32 = 2.0;

const DETAIL_SEPARATOR: &str = "·";

const SHEET_MARK: f32 = 18.0;

const SHEET_SHADOW_DROP: f32 = 16.0;

const SHEET_SHADOW_BLUR: f32 = 48.0;

const FLOATING_SHADOW_DROP: f32 = 6.0;

const FLOATING_SHADOW_BLUR: f32 = 24.0;

const SLIGHT_SHADOW_DROP: f32 = 4.0;

const SLIGHT_SHADOW_BLUR: f32 = 16.0;

const SHADOW_STRENGTH: f32 = 0.5;

const RULE: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cast {
    Long,
    Short,
    Slight,
}

impl Cast {
    const fn drop_and_blur(self) -> (f32, f32) {
        match self {
            Self::Long => (SHEET_SHADOW_DROP, SHEET_SHADOW_BLUR),
            Self::Short => (FLOATING_SHADOW_DROP, FLOATING_SHADOW_BLUR),
            Self::Slight => (SLIGHT_SHADOW_DROP, SLIGHT_SHADOW_BLUR),
        }
    }

    fn shadows(self) -> Vec<BoxShadow> {
        let (drop, blur) = self.drop_and_blur();

        vec![BoxShadow {
            color: theme::shadow(SHADOW_STRENGTH),
            offset: point(px(0.0), px(drop)),
            blur_radius: px(blur),
            spread_radius: px(0.0),
            inset: false,
        }]
    }
}

pub(crate) trait Casts: Styled + Sized {
    fn casting(self, cast: Cast) -> Self {
        self.shadow(cast.shadows())
    }
}

impl<E: Styled> Casts for E {}

pub(crate) trait GivesUnderAPress: StatefulInteractiveElement + Sized {
    fn gives_under_a_press(self) -> Self {
        self.active(|pressed| pressed.bg(rgb(theme::pressed())))
    }
}

impl<E: StatefulInteractiveElement> GivesUnderAPress for E {}

pub(crate) fn chosen_wash() -> Rgba {
    theme::tinted(theme::accent(), CHOSEN_WASH)
}

pub(crate) fn mark_wash(shade: u32) -> Rgba {
    theme::tinted(shade, MARK_WASH)
}

pub(crate) fn field_lit() -> Rgba {
    theme::tinted(theme::accent(), FIELD_LIT)
}

pub(crate) fn cover_edge() -> Rgba {
    theme::tinted(theme::text(), COVER_EDGE)
}

pub(crate) fn uncovered_mark(side: f32) -> impl IntoElement {
    icons::icon(Icon::Disc, side * UNCOVERED_MARK_SHARE, theme::faint())
}

const PLAY_ON_THE_ART: f32 = 36.0;

const PLAY_MARK_ON_THE_ART: f32 = 0.45;

pub(crate) fn play_on_the_art(id: impl Into<ElementId>, hint: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .size(px(PLAY_ON_THE_ART))
        .rounded_full()
        .bg(rgb(theme::accent()))
        .casting(Cast::Slight)
        .said_as(Role::Button, hint)
        .names(hint)
        .child(icons::icon(
            Icon::Play,
            PLAY_ON_THE_ART * PLAY_MARK_ON_THE_ART,
            theme::ink_over(theme::accent()),
        ))
}

pub(crate) fn cover_veil() -> Rgba {
    theme::tinted(theme::background(), COVER_VEIL)
}

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
        .font(theme::ui(FontWeight::NORMAL))
        .text_size(px(theme::text_xs()))
        .text_color(rgb(theme::muted()))
        .whitespace_nowrap()
        .child(text.into())
}

pub(crate) fn code(text: impl Into<SharedString>) -> Div {
    div()
        .font(theme::mono(FontWeight::NORMAL))
        .text_size(px(theme::text_xs()))
        .text_color(rgb(theme::muted()))
        .whitespace_nowrap()
        .child(text.into())
}

pub(crate) fn eyebrow(text: impl Into<SharedString>) -> Stateful<Div> {
    let text = text.into();

    div()
        .id(text.clone())
        .read_plainly(&text)
        .text_size(px(theme::text_xs()))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(theme::faint()))
        .whitespace_nowrap()
        .child(text)
}

pub(crate) fn title(text: impl IntoElement + Spelt) -> Stateful<Div> {
    let spelt = text.spelt();

    title_face()
        .id(spelt.clone())
        .headed(1, &spelt)
        .truncate()
        .ends_in_an_ellipsis()
        .child(text)
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
        self.whitespace_nowrap().line_clamp(1)
    }
}

impl<T: Styled> EndsInAnEllipsis for T {}

const ELLIPSIS: &str = "…";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Cut {
    whole: SharedString,
    drawn: SharedString,
}

impl Cut {
    pub(crate) fn of(whole: SharedString, drawn: SharedString) -> Self {
        Self { whole, drawn }
    }
}

impl IntoElement for Cut {
    type Element = SharedString;

    fn into_element(self) -> SharedString {
        self.drawn
    }
}

impl Spelt for Cut {
    fn spelt(&self) -> SharedString {
        self.whole.clone()
    }
}

pub(crate) fn cut_to_fit(
    text: SharedString,
    room: Pixels,
    font: Font,
    size: Pixels,
    cx: &App,
) -> Cut {
    if room <= px(0.0) {
        return Cut::of(text.clone(), text);
    }
    let drawn = cx
        .text_system()
        .line_wrapper(font, size)
        .truncate_line(text.clone(), room, ELLIPSIS, &[], TruncateFrom::End)
        .0;

    Cut::of(text, drawn)
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
        Tone::Outlined => (theme::raised(), theme::text(), theme::border()),
        Tone::Primary => (theme::accent(), theme::accent_ink(), theme::UNMARKED),
        Tone::Destructive => (theme::raised(), theme::failure(), theme::failure()),
    };
    let hovered = match tone {
        Tone::Ghost | Tone::Outlined | Tone::Destructive => theme::hover(),
        Tone::Primary => theme::accent(),
    };
    let pressed = match tone {
        Tone::Ghost | Tone::Outlined | Tone::Destructive => theme::pressed(),
        Tone::Primary => theme::accent(),
    };

    let id = id.into();
    let label = label.into();
    let saying = saying.into();

    div()
        .id(id.clone())
        .found_as(&id)
        .named_as(Role::Button, &label, &saying)
        .when(!press.takes(), Spoken::greyed_for_a_reader)
        .group(BUTTON_GROUP)
        .flex()
        .flex_none()
        .items_center()
        .gap_1p5()
        .h(px(theme::button_height()))
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
                    .when(tone == Tone::Primary, |button| {
                        button.focus_visible(focus::lit_over(theme::accent()))
                    })
                    .hover(move |button| button.bg(theme::tinted(hovered, hover_alpha(tone))))
                    .active(move |button| button.bg(theme::tinted(pressed, pressed_alpha(tone))))
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
        .child(label)
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

const fn pressed_alpha(tone: Tone) -> u8 {
    match tone {
        Tone::Ghost | Tone::Outlined | Tone::Destructive => 0xff,
        Tone::Primary => PRIMARY_PRESSED,
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
    .thrown(favoured)
}

fn marked(
    press: Press,
    id: impl Into<ElementId>,
    drawn: impl IntoElement,
    saying: impl Into<SharedString>,
) -> Stateful<Div> {
    let id = id.into();
    let saying = saying.into();
    div()
        .id(id.clone())
        .found_as(&id)
        .said_as(Role::Button, &saying)
        .when(!press.takes(), Spoken::greyed_for_a_reader)
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
                    .gives_under_a_press()
            },
            |button| button.opacity(GREYED).cursor_default(),
        )
        .names(saying)
        .child(drawn)
}

pub(crate) fn pick(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    saying: impl Into<SharedString>,
) -> Stateful<Div> {
    let id = id.into();
    let label = label.into();
    let saying = saying.into();

    div()
        .id(id.clone())
        .found_as(&id)
        .named_as(Role::ComboBox, &label, &saying)
        .flex()
        .flex_none()
        .items_center()
        .gap_1p5()
        .max_w_full()
        .h(px(theme::pill_height()))
        .pl_2p5()
        .pr_1p5()
        .rounded_md()
        .bg(rgb(theme::raised()))
        .border_1()
        .border_color(rgb(theme::border()))
        .text_size(px(theme::text_sm()))
        .text_color(rgb(theme::text()))
        .whitespace_nowrap()
        .cursor_pointer()
        .hover(|pick| pick.bg(rgb(theme::hover())))
        .gives_under_a_press()
        .names(saying)
        .child(
            div()
                .min_w(px(0.0))
                .truncate()
                .ends_in_an_ellipsis()
                .child(label),
        )
        .child(icons::icon(
            Icon::ChevronDown,
            theme::row_control_icon(),
            theme::muted(),
        ))
}

pub(crate) fn radio_row(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    chosen: bool,
) -> Stateful<Div> {
    let id = id.into();
    let label = label.into();

    div()
        .id(id.clone())
        .found_as(&id)
        .chosen_among(&label, String::new, chosen)
        .flex()
        .flex_none()
        .items_center()
        .gap_2p5()
        .px_2p5()
        .py_1p5()
        .rounded_md()
        .cursor_pointer()
        .when_else(
            chosen,
            |row| row.bg(chosen_wash()),
            |row| row.hover(|row| row.bg(rgb(theme::hover()))),
        )
        .child(radio(chosen))
        .child(choice_name(label, chosen))
}

pub(crate) fn checked_row(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    held: bool,
) -> Stateful<Div> {
    let id = id.into();
    let label = label.into();

    div()
        .id(id.clone())
        .found_as(&id)
        .chosen_among(&label, String::new, held)
        .focused_while(held)
        .flex()
        .flex_none()
        .items_center()
        .gap_2p5()
        .h(px(theme::menu_row()))
        .px_2()
        .rounded_md()
        .cursor_pointer()
        .text_size(px(theme::text_sm()))
        .hover(|row| row.bg(rgb(theme::hover())))
        .when_else(
            held,
            |row| {
                row.text_color(rgb(theme::text()))
                    .font_weight(FontWeight::MEDIUM)
            },
            |row| {
                row.text_color(rgb(theme::muted()))
                    .lit_under_the_pointer(id, |row| row.text_color(rgb(theme::text())))
            },
        )
        .child(motion::shown_while(
            icons::icon(Icon::Check, theme::menu_icon(), theme::accent()),
            "held",
            held,
        ))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .ends_in_an_ellipsis()
                .child(label),
        )
}

pub(crate) fn checkbox(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    ticked: bool,
) -> Stateful<Div> {
    let id = id.into();
    let label = label.into();
    let square = div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(RADIO))
        .rounded_sm()
        .border_1();
    let square = match ticked {
        true => square
            .bg(rgb(theme::accent()))
            .border_color(rgb(theme::accent()))
            .child(icons::icon(
                Icon::Check,
                TICK,
                theme::ink_over(theme::accent()),
            )),
        false => square.border_color(rgb(theme::outline())),
    };

    div()
        .id(id.clone())
        .found_as(&id)
        .named_as(Role::CheckBox, &label, "")
        .thrown(ticked)
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .h(px(theme::pill_height()))
        .px_1p5()
        .rounded_md()
        .text_size(px(theme::text_sm()))
        .text_color(rgb(if ticked {
            theme::text()
        } else {
            theme::muted()
        }))
        .whitespace_nowrap()
        .cursor_pointer()
        .hover(|row| row.bg(rgb(theme::hover())))
        .child(square)
        .child(label)
}

pub(crate) fn stepper() -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_0p5()
        .p_0p5()
        .rounded_full()
        .bg(rgb(theme::raised()))
        .border_1()
        .border_color(rgb(theme::border()))
}

pub(crate) fn step(
    id: impl Into<ElementId>,
    icon: Icon,
    saying: impl Into<SharedString>,
) -> Stateful<Div> {
    icon_button(id, icon, saying).rounded_full()
}

pub(crate) fn stepper_reading(text: impl Into<SharedString>) -> Div {
    figure(text)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .min_w(px(STEPPER_READING))
        .h(px(theme::row_control()))
}

pub(crate) fn stepper_reading_put_back(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    saying: impl Into<SharedString>,
) -> Stateful<Div> {
    let id = id.into();
    let saying = saying.into();

    stepper_reading(text)
        .id(id.clone())
        .found_as(&id)
        .said_as(Role::Button, &saying)
        .rounded_full()
        .text_color(rgb(theme::accent()))
        .cursor_pointer()
        .hover(|reading| reading.bg(rgb(theme::hover())))
        .gives_under_a_press()
        .names(saying)
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
        .font(theme::ui(FontWeight::MEDIUM))
        .text_size(px(theme::text_xs()))
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
        .gap_1p5()
        .child(codec_badge(codec))
        .child(figure(format::quality(spec)).text_color(rgb(theme::faint())))
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

pub(crate) fn section_name(text: impl Into<SharedString>) -> Stateful<Div> {
    let text = text.into();

    div()
        .id(text.clone())
        .headed(2, &text)
        .flex_1()
        .min_w(px(0.0))
        .text_size(px(theme::text_xs()))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(theme::faint()))
        .child(text)
}

pub(crate) fn section_body() -> Div {
    div().flex().flex_col().gap_4().px_4().py_3()
}

pub(crate) fn field(label: impl Into<SharedString>, control: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(row_name(label))
        .child(control)
}

pub(crate) fn row_name(label: impl Into<SharedString>) -> Div {
    div()
        .min_w(px(0.0))
        .text_size(px(theme::text_sm()))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(theme::text()))
        .child(label.into())
}

pub(crate) fn row_under(text: impl Into<SharedString>) -> Div {
    div()
        .min_w(px(0.0))
        .text_size(px(theme::text_xs()))
        .text_color(rgb(theme::muted()))
        .child(text.into())
}

pub(crate) fn named(name: impl Into<SharedString>, under: impl Into<SharedString>) -> Div {
    let under = under.into();

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.0))
        .gap_0p5()
        .child(row_name(name))
        .when(!under.is_empty(), |named| named.child(row_under(under)))
}

pub(crate) fn note(text: impl Into<SharedString>) -> Stateful<Div> {
    let text = text.into();

    div()
        .id(text.clone())
        .read_plainly(&text)
        .text_size(px(theme::text_sm()))
        .text_color(rgb(theme::faint()))
        .child(text)
}

pub(crate) struct Rows {
    card: Div,
    notes: Vec<Stateful<Div>>,
    held: usize,
}

pub(crate) fn rows() -> Rows {
    Rows {
        card: div()
            .flex()
            .flex_col()
            .p_1()
            .rounded_xl()
            .bg(rgb(theme::surface()))
            .border_1()
            .border_color(rgb(theme::soft_edge())),
        notes: Vec::new(),
        held: 0,
    }
}

impl Rows {
    pub(crate) fn note(mut self, text: impl Into<SharedString>) -> Self {
        self.notes.push(note(text));
        self
    }
}

impl ParentElement for Rows {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        for element in elements {
            if self.held > 0 {
                self.card.extend([row_rule().into_any_element()]);
            }
            self.card.extend([cell().child(element).into_any_element()]);
            self.held += 1;
        }
    }
}

impl IntoElement for Rows {
    type Element = Div;

    fn into_element(self) -> Div {
        let Self { card, notes, held } = self;

        div()
            .flex()
            .flex_col()
            .gap_2()
            .when(held > 0, |boxed| boxed.child(card))
            .children(notes.into_iter().map(|note| note.px_4()))
    }
}

fn cell() -> Div {
    div().flex().flex_col().gap_2().px_3().py_2()
}

fn row_rule() -> Div {
    div()
        .flex_none()
        .h(px(RULE))
        .my_1()
        .bg(rgb(theme::soft_edge()))
}

pub(crate) fn group(heading: impl IntoElement, rows: impl IntoElement) -> Div {
    div().flex().flex_col().gap_2().child(heading).child(rows)
}

pub(crate) fn group_heading() -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .px_4()
        .min_h(px(theme::row_control()))
}

pub(crate) fn group_title(text: impl Into<SharedString>) -> Stateful<Div> {
    let text = text.into();

    div()
        .id(text.clone())
        .headed(2, &text)
        .flex_1()
        .min_w(px(0.0))
        .text_size(px(theme::text_sm()))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(theme::text()))
        .child(text)
}

pub(crate) fn row(
    name: impl Into<SharedString>,
    under: impl Into<SharedString>,
    control: impl IntoElement,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap_4()
        .child(told(name, under))
        .child(
            div()
                .flex()
                .flex_none()
                .justify_end()
                .max_w(relative(ROW_CONTROL_SHARE))
                .child(control),
        )
}

pub(crate) fn stacked_row(
    name: impl Into<SharedString>,
    under: impl Into<SharedString>,
    control: impl IntoElement,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(told(name, under))
        .child(control)
}

fn told(name: impl Into<SharedString>, under: impl Into<SharedString>) -> Div {
    let under = under.into();

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.0))
        .gap_0p5()
        .child(row_name(name))
        .when(!under.is_empty(), |named| {
            named.child(
                note(under)
                    .text_size(px(theme::text_xs()))
                    .text_color(rgb(theme::muted())),
            )
        })
}

pub(crate) trait Bleeds: Styled + Sized {
    fn bleeds(self) -> Self {
        self.mx_neg_3().my_neg_2().px_3().py_2().rounded_lg()
    }
}

impl<E: Styled> Bleeds for E {}

pub(crate) fn switch_row(
    label: impl Into<SharedString>,
    under: impl Into<SharedString>,
    on: bool,
    id: impl Into<SharedString>,
) -> Stateful<Div> {
    let id = id.into();
    let label = label.into();
    let under = under.into();

    div()
        .id(id.clone())
        .named_as(Role::Switch, &label, &under)
        .thrown(on)
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .bleeds()
        .cursor_pointer()
        .hover(|row| row.bg(rgb(theme::hover())))
        .child(named(label, under))
        .child(switch(SharedString::from(format!("{id}-track")), on))
}

pub(crate) fn nav_row(
    id: impl Into<ElementId>,
    icon: Icon,
    name: impl Into<SharedString>,
    under: impl Into<SharedString>,
) -> Stateful<Div> {
    let id = id.into();
    let name = name.into();
    let under = under.into();

    div()
        .id(id.clone())
        .found_as(&id)
        .named_as(Role::Link, &name, &under)
        .group(NAV_ROW_GROUP)
        .flex()
        .items_center()
        .gap_3()
        .bleeds()
        .cursor_pointer()
        .hover(|row| row.bg(rgb(theme::hover())))
        .gives_under_a_press()
        .names(under.clone())
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(theme::row_control()))
                .child(icons::icon(icon, theme::row_control_icon(), theme::muted())),
        )
        .child(named(name, under))
        .child(icons::lit_on_hover(
            icons::icon(
                Icon::ChevronRight,
                theme::row_control_icon(),
                theme::faint(),
            ),
            NAV_ROW_GROUP,
        ))
}

pub(crate) fn place_link(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    saying: impl Into<SharedString>,
) -> Stateful<Div> {
    let id = id.into();
    let label = label.into();
    let saying = saying.into();

    div()
        .id(id.clone())
        .found_as(&id)
        .named_as(Role::Link, &label, &saying)
        .group(NAV_ROW_GROUP)
        .flex()
        .flex_none()
        .items_center()
        .gap_1()
        .min_h(px(theme::hint_control()))
        .px_1()
        .rounded_md()
        .text_size(px(theme::text_xs()))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(theme::faint()))
        .whitespace_nowrap()
        .cursor_pointer()
        .hover(|link| link.bg(rgb(theme::hover())))
        .gives_under_a_press()
        .lit_under_the_pointer(id, |link| link.text_color(rgb(theme::text())))
        .names(saying)
        .child(label)
        .child(icons::lit_on_hover(
            icons::icon(Icon::ChevronRight, theme::hint_icon(), theme::faint()),
            NAV_ROW_GROUP,
        ))
}

pub(crate) fn framed_field(id: impl Into<ElementId>, lit: bool) -> Stateful<Div> {
    let id = id.into();

    div()
        .id(id.clone())
        .found_as(&id)
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .w_full()
        .h(theme::width(theme::search_height()))
        .px_3()
        .rounded_lg()
        .bg(rgb(theme::background()))
        .border_1()
        .border_color(if lit {
            field_lit()
        } else {
            rgb(theme::soft_edge())
        })
        .text_size(px(theme::text_sm()))
        .cursor_text()
        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
        .hover(|field| field.border_color(field_lit()))
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
    let label = label.into();

    div()
        .id(id.clone())
        .found_as(&id)
        .named_as(Role::RadioButton, &label, "")
        .thrown(chosen)
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
            focus::beneath_the_ring(div())
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
        .child(label)
}

pub(crate) fn tab_strip() -> Div {
    div()
        .flex()
        .flex_none()
        .flex_wrap()
        .items_end()
        .gap_x_6()
        .gap_y_2()
}

pub(crate) fn tab(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    chosen: bool,
) -> Stateful<Div> {
    let id = id.into();
    let label = label.into();

    div()
        .id(id.clone())
        .found_as(&id)
        .named_as(Role::RadioButton, &label, "")
        .thrown(chosen)
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .pt_1()
        .pb_2()
        .text_size(px(theme::text_base()))
        .whitespace_nowrap()
        .cursor_pointer()
        .child(motion::shown_while(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .h(px(TAB_MARK))
                .rounded_full()
                .bg(rgb(theme::accent())),
            "chosen",
            chosen,
        ))
        .when_else(
            chosen,
            |tab| {
                tab.text_color(rgb(theme::text()))
                    .font_weight(FontWeight::SEMIBOLD)
            },
            |tab| {
                tab.text_color(rgb(theme::muted()))
                    .font_weight(FontWeight::MEDIUM)
                    .lit_under_the_pointer(id, |tab| tab.text_color(rgb(theme::text())))
            },
        )
        .child(label)
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
                                theme::switch_knob_off(),
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

pub(crate) fn dot_swatch(
    id: impl Into<ElementId>,
    label: &str,
    colour: u32,
    chosen: bool,
) -> Stateful<Div> {
    let id = id.into();
    div()
        .id(id.clone())
        .found_as(&id)
        .chosen_among(label, String::new, chosen)
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
            |swatch| swatch.hover(|swatch| swatch.opacity(LIT)),
        )
}

pub(crate) fn choice_row(
    id: impl Into<ElementId>,
    label: &str,
    saying: impl FnOnce() -> String,
    chosen: bool,
) -> Stateful<Div> {
    let id = id.into();
    div()
        .id(id.clone())
        .found_as(&id)
        .chosen_among(label, saying, chosen)
        .flex()
        .items_start()
        .gap_3()
        .bleeds()
        .cursor_pointer()
        .when_else(
            chosen,
            |row| row.bg(chosen_wash()),
            |row| row.hover(|row| row.bg(rgb(theme::hover()))),
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

pub(crate) fn ledger(across: usize, cells: Vec<AnyElement>) -> Div {
    let across = across.max(1);
    let mut cells = cells.into_iter().peekable();
    let mut ledger = section();
    let mut below_the_first = false;
    while cells.peek().is_some() {
        let mut line = div().flex().w_full().when(below_the_first, |line| {
            line.border_t_1().border_color(rgb(theme::border()))
        });
        for place in 0..across {
            line = line.child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.0))
                    .p_4()
                    .when(place > 0, |cell| {
                        cell.border_l_1().border_color(rgb(theme::border()))
                    })
                    .children(cells.next()),
            );
        }
        ledger = ledger.child(line);
        below_the_first = true;
    }

    ledger
}

pub(crate) fn card_title(text: impl Into<SharedString>) -> Stateful<Div> {
    let text = text.into();

    div()
        .id(text.clone())
        .headed(3, &text)
        .text_size(px(theme::text_sm()))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(theme::text()))
        .child(text)
}

pub(crate) fn sheet() -> Div {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .p_5()
        .rounded_xl()
        .bg(rgb(theme::surface()))
        .border_1()
        .border_color(rgb(theme::border()))
        .casting(Cast::Long)
}

pub(crate) fn over_a_scrim(card: impl IntoElement) -> Div {
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgba(theme::scrim()))
        .occlude()
        .child(card)
}

pub(crate) fn sheet_heading(
    id: &'static str,
    mark: Icon,
    shade: u32,
    title: impl Into<SharedString>,
) -> Div {
    let title = title.into();

    div()
        .flex()
        .items_center()
        .gap_2()
        .child(icons::icon(mark, SHEET_MARK, shade))
        .child(
            div()
                .id(id)
                .headed(2, &title)
                .flex_1()
                .min_w(px(0.0))
                .text_size(px(theme::text_lg()))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(theme::text()))
                .truncate()
                .ends_in_an_ellipsis()
                .child(title),
        )
}

pub(crate) fn sheet_actions() -> Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_end()
        .gap_2()
}

pub(crate) fn floating() -> Div {
    div()
        .flex()
        .flex_col()
        .p_1()
        .gap_0p5()
        .rounded_lg()
        .bg(rgb(theme::raised()))
        .border_1()
        .border_color(rgb(theme::soft_edge()))
        .casting(Cast::Short)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
}

pub(crate) fn rule() -> Div {
    div().flex_none().h(px(RULE)).bg(rgb(theme::soft_edge()))
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
        .max_w_full()
        .h(px(theme::way_back_height()))
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
        .gives_under_a_press()
        .lit_under_the_pointer(id, |back| back.text_color(rgb(theme::text())))
        .names(saying)
        .child(icons::lit_on_hover(
            icons::icon(Icon::Back, theme::row_control_icon(), theme::muted()),
            WAY_BACK_GROUP,
        ))
        .child(
            div()
                .min_w(px(0.0))
                .truncate()
                .ends_in_an_ellipsis()
                .child(label.into()),
        )
}

pub(crate) fn hero() -> Div {
    div().flex().items_end().gap_6()
}

pub(crate) fn hero_title(text: impl IntoElement + Spelt, room: Pixels) -> Stateful<Div> {
    let spelt = text.spelt();

    div()
        .id(spelt.clone())
        .headed(1, &spelt)
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
    brought_into_view_within(Some(scroll), None, asked)
}

pub(crate) fn brought_into_view_within(
    down: Option<ScrollHandle>,
    across: Option<ScrollHandle>,
    asked: Rc<Cell<bool>>,
) -> impl IntoElement {
    canvas(
        move |bounds, window, _| {
            if !asked.replace(false) {
                return;
            }
            let lowered = down.as_ref().and_then(|down| {
                let seen = down.bounds();
                moved_into(bounds.top(), bounds.bottom(), seen.top(), seen.bottom())
                    .map(|moved| (down, moved))
            });
            let slid = across.as_ref().and_then(|across| {
                let seen = across.bounds();
                moved_into(bounds.left(), bounds.right(), seen.left(), seen.right())
                    .map(|moved| (across, moved))
            });
            if let Some((down, moved)) = lowered {
                let offset = down.offset();
                down.set_offset(point(offset.x, offset.y + moved));
            }
            if let Some((across, moved)) = slid {
                let offset = across.offset();
                across.set_offset(point(offset.x + moved, offset.y));
            }
            if lowered.is_some() || slid.is_some() {
                window.refresh();
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}

pub(crate) fn moved_into(
    start: Pixels,
    end: Pixels,
    seen_start: Pixels,
    seen_end: Pixels,
) -> Option<Pixels> {
    if start < seen_start {
        Some(seen_start - start)
    } else if end > seen_end {
        Some((seen_end - end).max(seen_start - start))
    } else {
        None
    }
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
        .h(px(theme::column_header_height()))
        .border_b_1()
        .border_color(rgb(theme::border()))
        .text_size(px(theme::text_xs()))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(theme::faint()))
}

pub(crate) fn pane() -> Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.0))
        .min_h(px(0.0))
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

pub(crate) fn trouble(icon: Icon, message: &'static str, more: Option<&'static str>) -> AnyElement {
    stands_empty(icon, theme::failure(), message, more).into_any_element()
}

pub(crate) fn trouble_offering(
    icon: Icon,
    message: &'static str,
    more: Option<&'static str>,
    offer: impl IntoElement,
) -> AnyElement {
    stands_empty(icon, theme::failure(), message, more)
        .child(offer)
        .into_any_element()
}

const EMPTY_WORDS_AT_MOST: f32 = 420.0;

pub(crate) fn empty_mark(icon: Icon, shade: u32) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .child(icons::icon(icon, theme::empty_icon(), shade))
}

fn nothing_here(icon: Icon, message: &'static str, more: Option<&'static str>) -> Div {
    stands_empty(icon, theme::faint(), message, more)
}

fn stands_empty(icon: Icon, shade: u32, message: &'static str, more: Option<&'static str>) -> Div {
    div()
        .flex()
        .flex_1()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_1p5()
        .px_8()
        .py_8()
        .child(empty_mark(icon, shade))
        .child(
            div()
                .id(message)
                .headed(2, message)
                .pt_3()
                .max_w(px(EMPTY_WORDS_AT_MOST))
                .text_size(px(theme::text_lg()))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(theme::text()))
                .text_center()
                .child(message),
        )
        .when_some(more, |empty, more| {
            empty.child(
                div()
                    .id(more)
                    .read_plainly(more)
                    .max_w(px(EMPTY_WORDS_AT_MOST))
                    .pb_3()
                    .text_size(px(theme::text_sm()))
                    .text_color(rgb(theme::muted()))
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
        .bg(mark_wash(if lit {
            theme::accent()
        } else {
            theme::muted()
        }))
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
    use crate::views::spoken::{self, node_of};

    #[cfg(not(target_os = "macos"))]
    struct Names {
        layouts: Vec<gpui::TextLayout>,
    }

    #[cfg(not(target_os = "macos"))]
    impl gpui::Render for Names {
        fn render(&mut self, _: &mut gpui::Window, _: &mut Context<Self>) -> impl IntoElement {
            self.layouts.clear();
            let mut row = div()
                .flex()
                .w(px(600.0))
                .font_family("sans-serif")
                .text_size(px(14.0));
            for name in [
                "AVATAR",
                "Taylor Swift",
                "A very long song title ".repeat(30).as_str(),
            ] {
                let text = gpui::StyledText::new(name.to_owned());
                self.layouts.push(text.layout().clone());
                row = row.child(
                    div().flex().w(px(180.0)).flex_none().child(
                        div()
                            .min_w(px(0.0))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(text),
                    ),
                );
            }
            row
        }
    }

    #[cfg(not(target_os = "macos"))]
    #[gpui::test]
    fn names_that_fit_stay_whole_and_long_names_end_in_an_ellipsis(cx: &mut gpui::TestAppContext) {
        let platform = gpui_platform::current_platform(true);
        let mut real = gpui::TestAppContext::build_with_text_system(
            cx.dispatcher.clone(),
            None,
            platform.text_system(),
        );
        let (names, cx) = real.add_window_view(|_, _| Names {
            layouts: Vec::new(),
        });
        cx.run_until_parked();
        names.read_with(cx, |names, _| {
            assert_eq!(names.layouts[0].text(), "AVATAR");
            assert_eq!(names.layouts[1].text(), "Taylor Swift");
            assert!(names.layouts[2].text().ends_with('…'));
        });
    }

    #[test]
    fn every_kit_control_says_what_it_is_and_what_state_it_stands_in() {
        spoken::listened_to(true);

        let labelled = node_of(&button(
            "play-all",
            Some(Icon::Play),
            "Play",
            "Play every track listed, in order",
            Tone::Primary,
        ))
        .expect("a button is a node");
        let marked = node_of(&icon_button("more", Icon::More, "More — shift-f10"))
            .expect("a mark is a node");
        let starred =
            node_of(&star("starred", true, "a".into(), "Unfavourite")).expect("a star is a node");
        let chosen = node_of(&segment("grid", "Grid", true)).expect("a segment is a node");
        let picked =
            node_of(&pick("order", "Newest", "What the list is put in order by")).expect("a pick");
        let row = node_of(&radio_row("newest", "Newest", true)).expect("a radio row is a node");
        let ticked = node_of(&checkbox("flac", ".flac", false)).expect("a checkbox is a node");

        assert_eq!(labelled.role(), Role::Button);
        assert_eq!(labelled.label(), Some("Play"));
        assert_eq!(
            labelled.description(),
            Some("Play every track listed, in order")
        );
        assert_eq!(marked.label(), Some("More"));
        assert_eq!(starred.toggled(), Some(gpui::Toggled::True));
        assert_eq!(chosen.role(), Role::RadioButton);
        assert_eq!(chosen.toggled(), Some(gpui::Toggled::True));
        assert_eq!(picked.role(), Role::ComboBox);
        assert_eq!(picked.label(), Some("Newest"));
        assert_eq!(row.role(), Role::RadioButton);
        assert_eq!(row.toggled(), Some(gpui::Toggled::True));
        assert_eq!(ticked.role(), Role::CheckBox);
        assert_eq!(ticked.label(), Some(".flac"));
        assert_eq!(ticked.toggled(), Some(gpui::Toggled::False));
    }

    #[test]
    fn the_text_the_kit_draws_is_headings_and_plain_text_a_reader_can_reach() {
        spoken::listened_to(true);

        let eyebrow = node_of(&eyebrow("Search")).expect("an eyebrow is a node");
        let title = node_of(&title(SharedString::new_static("Albums"))).expect("a title is a node");
        let hero = node_of(&hero_title(SharedString::new_static("Echoes"), px(300.0)))
            .expect("a hero title is a node");
        let section = node_of(&section_name("Output")).expect("a section name is a node");
        let card = node_of(&card_title("Signal")).expect("a card title is a node");
        let empty = node_of(&nothing_here(
            Icon::Search,
            "No albums yet",
            Some("Scan a folder."),
        ))
        .map(|node| node.role());

        assert_eq!(eyebrow.role(), Role::Label);
        assert_eq!(eyebrow.label(), Some("Search"));
        assert_eq!(title.role(), Role::Heading);
        assert_eq!(title.level(), Some(1));
        assert_eq!(title.label(), Some("Albums"));
        assert_eq!(hero.role(), Role::Heading);
        assert_eq!(hero.label(), Some("Echoes"));
        assert_eq!(section.role(), Role::Heading);
        assert_eq!(section.level(), Some(2));
        assert_eq!(card.level(), Some(3));
        assert_eq!(empty, None, "the empty state names its parts, not itself");
    }

    #[test]
    fn the_controls_a_pointer_alone_once_reached_are_named_buttons() {
        spoken::listened_to(true);

        let sort = div()
            .id("sort-by-title")
            .named_as(Role::Button, "TITLE", "Sort by title");
        let enriching =
            div()
                .id("enriching")
                .named_as(Role::Button, "Enriching…", "Looking things up");
        let fixed = [
            node_of(&sort),
            node_of(&enriching),
            node_of(&icon_button(
                "clear-the-filter",
                Icon::Close,
                "Clear what is typed",
            )),
            node_of(&button(
                "visualiser-hold",
                Some(Icon::Pause),
                "Pause",
                "Hold the picture still — ctrl-p",
                Tone::Ghost,
            )),
        ];

        for node in fixed {
            let node = node.expect("a fixed control is a node");

            assert_eq!(node.role(), Role::Button);
            assert!(
                node.label().is_some_and(|name| !name.is_empty()),
                "a control with no name is not reachable"
            );
        }
    }

    #[test]
    fn a_choice_row_and_a_swatch_are_radio_buttons_naming_what_they_choose() {
        spoken::listened_to(true);

        let device = node_of(&choice_row(
            "speakers",
            "Built-in Audio",
            || "Whatever PipeWire routes to. Takes 16-bit".to_owned(),
            true,
        ))
        .expect("a choice row is a node");
        let swatch =
            node_of(&dot_swatch("mauve", "Mauve", 0xcba6f7, false)).expect("a swatch is a node");

        assert_eq!(device.role(), Role::RadioButton);
        assert_eq!(device.label(), Some("Built-in Audio"));
        assert_eq!(
            device.description(),
            Some("Whatever PipeWire routes to. Takes 16-bit")
        );
        assert_eq!(device.toggled(), Some(gpui::Toggled::True));
        assert_eq!(swatch.role(), Role::RadioButton);
        assert_eq!(swatch.label(), Some("Mauve"));
        assert_eq!(swatch.toggled(), Some(gpui::Toggled::False));
    }

    #[test]
    fn a_link_cut_to_fit_is_drawn_cut_and_named_whole() {
        spoken::listened_to(true);

        let cut = Cut::of(
            SharedString::new_static("Echoes (2011 Remaster)"),
            SharedString::new_static("Echoes…"),
        );
        let uncut = Cut::of(
            SharedString::new_static("Meddle"),
            SharedString::new_static("Meddle"),
        );

        let link = node_of(&div().id("playing-title").linked_as(&cut, "Open this album"))
            .expect("a heard link is a node");
        let whole = node_of(
            &div()
                .id("playing-album")
                .linked_as(&uncut, "Open this album"),
        )
        .expect("a heard link is a node");

        assert_eq!(cut.clone().into_element(), "Echoes…");
        assert_eq!(link.role(), Role::Link);
        assert_eq!(link.label(), Some("Echoes (2011 Remaster)"));
        assert_eq!(whole.label(), Some("Meddle"));
    }

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
