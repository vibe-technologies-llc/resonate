use std::time::SystemTime;

use gpui::{
    AnyElement, Context, Div, FontWeight, HighlightStyle, SharedString, StyledText, div,
    prelude::*, px, rgb,
};
use resonate_core::{AlbumId, ArtistId, MediaLocation, StreamSpec, TrackId};
use resonate_engine::MediaInfo;
use resonate_library::{Codec, Direction, Lit, Track};

use crate::{
    Notice, Tone, format,
    icons::{self, Icon},
    theme,
    views::{
        browser,
        hint::Names as _,
        kit::{self, EndsInAnEllipsis},
        menu::{self, Menu},
        pointed::LitUnderThePointer,
        root::RootView,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sortable {
    Number,
    Title,
    Artist,
    Heard,
    Length,
}

#[derive(Clone, Copy)]
pub(crate) struct Sorted {
    pub(crate) by: Option<Sortable>,
    pub(crate) reading: Direction,
    pub(crate) offers: &'static [Sortable],
    pub(crate) saying: &'static str,
    pub(crate) press: fn(&mut RootView, Sortable, &mut Context<RootView>),
}

pub(crate) enum Pictured<'a> {
    Album(AlbumId),
    Track {
        album: Option<AlbumId>,
        file: &'a MediaLocation,
    },
}

pub(crate) struct Row {
    pub(crate) track: Option<TrackId>,
    pub(crate) favourite: bool,
    pub(crate) title: SharedString,
    pub(crate) artist: SharedString,
    pub(crate) artist_id: Option<ArtistId>,
    pub(crate) length: SharedString,
    pub(crate) plays: u32,
    pub(crate) played: Option<SystemTime>,
    pub(crate) album: Option<AlbumId>,
    pub(crate) shape: Option<(Codec, StreamSpec)>,
}

pub(crate) fn scanned(track: Track) -> Row {
    Row {
        track: Some(track.id),
        favourite: track.favourite.is_some(),
        title: SharedString::from(track.title),
        artist: SharedString::from(track.artist.unwrap_or_default()),
        artist_id: track.artist_id,
        length: SharedString::from(
            track
                .duration
                .map(|duration| format::clock(duration, track.spec.rate))
                .unwrap_or_default(),
        ),
        plays: track.plays,
        played: track.played,
        album: track.album_id,
        shape: Some((track.codec, track.spec)),
    }
}

pub(crate) fn read(info: &MediaInfo, location: &MediaLocation) -> Row {
    Row {
        track: None,
        favourite: false,
        title: SharedString::from(
            info.tags
                .title
                .clone()
                .unwrap_or_else(|| format::stem(location)),
        ),
        artist: SharedString::from(info.tags.artist.clone().unwrap_or_default()),
        artist_id: None,
        length: SharedString::from(
            info.duration
                .map(|duration| format::clock(duration, info.spec.rate))
                .unwrap_or_default(),
        ),
        plays: 0,
        played: None,
        album: None,
        shape: Some((Codec::from_id(info.codec), info.spec)),
    }
}

pub(crate) fn unread(location: &MediaLocation) -> Row {
    Row {
        track: None,
        favourite: false,
        title: SharedString::from(format::stem(location)),
        artist: SharedString::new_static(""),
        artist_id: None,
        length: SharedString::new_static(""),
        plays: 0,
        played: None,
        album: None,
        shape: None,
    }
}

pub(crate) fn noticed(notice: &Notice) -> Div {
    let colour = match notice.tone() {
        Tone::Trouble => theme::failure(),
        Tone::Done => theme::done(),
        Tone::Noted => theme::muted(),
    };

    div()
        .flex()
        .items_center()
        .gap_2()
        .child(kit::mode_dot(colour))
        .child(
            div()
                .text_size(px(theme::text_sm()))
                .text_color(rgb(colour))
                .child(SharedString::from(notice.text().to_owned())),
        )
}

pub(crate) fn reads(clauses: &[String]) -> Div {
    let mut row = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_1p5()
        .child(kit::eyebrow("READS").pr_1p5());

    for clause in clauses {
        row = row.child(
            div()
                .px_2()
                .py_0p5()
                .rounded_sm()
                .bg(rgb(theme::raised()))
                .border_1()
                .border_color(rgb(theme::border()))
                .font_family(theme::mono_face())
                .text_size(px(theme::text_xs()))
                .text_color(rgb(theme::text()))
                .child(SharedString::from(clause.clone())),
        );
    }

    row
}

pub(crate) fn heard(plays: u32, played: Option<SystemTime>, now: SystemTime) -> Div {
    heard_cell(how_often_and_how_lately(plays, played, now))
}

pub(crate) fn unheard() -> Div {
    heard_cell(SharedString::new_static(""))
}

fn heard_cell(reading: SharedString) -> Div {
    kit::figure(reading)
        .w(px(theme::row_plays()))
        .text_color(rgb(theme::faint()))
        .truncate()
        .ends_in_an_ellipsis()
}

fn how_often_and_how_lately(
    plays: u32,
    played: Option<SystemTime>,
    now: SystemTime,
) -> SharedString {
    let counted = times(plays);
    match played.filter(|_| plays > 0) {
        None => counted,
        Some(when) => SharedString::from(format!("{counted} · {}", format::age(when, now))),
    }
}

pub(crate) fn times(plays: u32) -> SharedString {
    match plays {
        0 => SharedString::new_static(""),
        1 => SharedString::new_static("1 play"),
        plays => SharedString::from(format!("{plays} plays")),
    }
}

fn title_room() -> Div {
    div()
        .flex_grow()
        .flex_shrink()
        .flex_basis(px(theme::row_artist()))
        .min_w(px(0.0))
        .overflow_hidden()
}

fn artist_room() -> Div {
    div()
        .flex_shrink()
        .flex_basis(px(theme::row_artist()))
        .min_w(px(0.0))
        .overflow_hidden()
}

pub(crate) fn title_cell(title: SharedString, lit: Lit, playing: bool) -> Div {
    titled(
        title_room().truncate().ends_in_an_ellipsis(),
        title,
        lit,
        playing,
    )
}

pub(crate) fn tagged_title_cell(
    title: SharedString,
    lit: Lit,
    playing: bool,
    tag: impl IntoElement,
) -> Div {
    title_room()
        .flex()
        .items_center()
        .gap_2()
        .child(titled(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .ends_in_an_ellipsis(),
            title,
            lit,
            playing,
        ))
        .child(tag)
}

fn titled(cell: Div, title: SharedString, lit: Lit, playing: bool) -> Div {
    cell.text_color(rgb(if playing {
        theme::accent()
    } else {
        theme::text()
    }))
    .when(playing, |cell| cell.font_weight(FontWeight::MEDIUM))
    .child(matched(title, lit))
}

pub(crate) fn matched(text: SharedString, lit: Lit) -> AnyElement {
    if lit.is_empty() {
        return text.into_any_element();
    }

    let struck = HighlightStyle {
        color: Some(rgb(theme::accent()).into()),
        font_weight: Some(FontWeight::MEDIUM),
        ..HighlightStyle::default()
    };

    StyledText::new(text)
        .with_highlights(lit.into_iter().map(|run| (run, struck)))
        .into_any_element()
}

pub(crate) fn artist_cell(artist: impl IntoElement) -> Div {
    artist_room()
        .flex()
        .text_color(rgb(theme::muted()))
        .truncate()
        .child(artist)
}

pub(crate) fn format_cell(shape: Option<(Codec, StreamSpec)>) -> Div {
    let cell = div().w(px(theme::row_format())).flex_none();
    match shape {
        Some((codec, spec)) => cell.child(kit::format_badge(codec, spec)),
        None => cell,
    }
}

pub(crate) fn length_cell(length: SharedString) -> Div {
    kit::figure(length)
        .w(px(theme::row_length()))
        .flex_none()
        .text_right()
}

pub(crate) fn number_cell(text: SharedString) -> Div {
    kit::figure(text)
        .w(px(theme::row_number()))
        .flex_none()
        .text_color(rgb(theme::faint()))
}

pub(crate) fn playing_mark() -> Div {
    div()
        .w(px(theme::row_number()))
        .flex_none()
        .child(icons::icon(
            Icon::Play,
            theme::row_marker_icon(),
            theme::accent(),
        ))
}

pub(crate) fn columns(
    numbered: &'static str,
    with_cover: bool,
    sorted: Sorted,
    cx: &mut Context<RootView>,
) -> Div {
    let number = heads(
        Sortable::Number,
        numbered,
        div().w(px(theme::row_number())).flex_none(),
        sorted,
        cx,
    );
    let title = heads(Sortable::Title, "TITLE", title_room(), sorted, cx);
    let artist = heads(Sortable::Artist, "ARTIST", artist_room(), sorted, cx);
    let heard = heads(
        Sortable::Heard,
        "HEARD",
        div().w(px(theme::row_plays())).flex_none(),
        sorted,
        cx,
    );
    let length = heads(
        Sortable::Length,
        "LENGTH",
        div().w(px(theme::row_length())).flex_none().justify_end(),
        sorted,
        cx,
    );

    kit::column_header()
        .child(number)
        .when(with_cover, |header| {
            header.child(div().w(px(theme::row_cover())).flex_none())
        })
        .child(title)
        .child(artist)
        .child(div().w(px(theme::row_format())).flex_none().child("FORMAT"))
        .child(heard)
        .child(length)
        .child(div().w(px(browser::controls_width())).flex_none())
}

fn heads(
    column: Sortable,
    label: &'static str,
    cell: Div,
    sorted: Sorted,
    cx: &mut Context<RootView>,
) -> AnyElement {
    let cell = cell.flex().items_center().gap_1();
    if label.is_empty() || !sorted.offers.contains(&column) {
        return cell.child(label).into_any_element();
    }
    let offers = sorted.offers;
    let press = sorted.press;

    let held = sorted.by == Some(column);
    let mark = match sorted.reading {
        Direction::Ascending => Icon::ChevronUp,
        Direction::Descending => Icon::ChevronDown,
    };

    let head = cell
        .id(label)
        .cursor_pointer()
        .names(sorted.saying)
        .when_else(
            held,
            |head| head.text_color(rgb(theme::accent())),
            |head| head.lit_under_the_pointer(label, |head| head.text_color(rgb(theme::text()))),
        )
        .child(label)
        .when(held, |head| {
            head.child(icons::icon(mark, theme::column_mark(), theme::accent()))
        })
        .on_click(cx.listener(move |this, _, _, cx| {
            press(this, column, cx);
        }));

    menu::opens_a_menu(
        head,
        move |_, at, _| {
            let mut menu = Menu::at(at);
            for offered in offers {
                let by = *offered;
                menu = menu.does(Icon::Sort, sorts_by(by), move |this, _, cx| {
                    press(this, by, cx)
                });
            }

            menu
        },
        cx,
    )
    .into_any_element()
}

const fn sorts_by(column: Sortable) -> &'static str {
    match column {
        Sortable::Number => "Sort by album order",
        Sortable::Title => "Sort by title",
        Sortable::Artist => "Sort by artist",
        Sortable::Heard => "Sort by how often it is heard",
        Sortable::Length => "Sort by length",
    }
}
