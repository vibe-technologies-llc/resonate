use std::{
    cell::Cell,
    hash::{DefaultHasher, Hash, Hasher},
    rc::Rc,
    time::SystemTime,
};

use gpui::{
    AnyElement, Context, Div, ElementId, FontWeight, HighlightStyle, Pixels, SharedString,
    StyledText, div, prelude::*, px, rgb,
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

pub(crate) fn keyed_by(name: &'static str, held: &impl Hash) -> ElementId {
    let mut hasher = DefaultHasher::new();
    held.hash(&mut hasher);

    ElementId::from((name, hasher.finish()))
}

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

pub(crate) fn scanned(track: Track, favourite: bool) -> Row {
    Row {
        track: Some(track.id),
        favourite,
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

pub(crate) const COLUMN_GAP: f32 = 12.0;

pub(crate) const ROW_INSET: f32 = 24.0;

const NAMES_AT_LEAST: f32 = 220.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Shown {
    pub(crate) format: bool,
    pub(crate) heard: bool,
}

impl Shown {
    const EVERY_COLUMN: Self = Self {
        format: true,
        heard: true,
    };

    const WITHOUT_HEARD: Self = Self {
        format: true,
        heard: false,
    };

    const NAMES_ALONE: Self = Self {
        format: false,
        heard: false,
    };

    fn within(room: Pixels, with_cover: bool, controls: usize) -> Self {
        if room <= px(0.0) {
            return Self::EVERY_COLUMN;
        }

        [Self::EVERY_COLUMN, Self::WITHOUT_HEARD]
            .into_iter()
            .find(|shown| shown.width(with_cover, controls) <= f32::from(room))
            .unwrap_or(Self::NAMES_ALONE)
    }

    fn width(self, with_cover: bool, controls: usize) -> f32 {
        let cells: Vec<f32> = [
            Some(theme::row_number()),
            with_cover.then(theme::row_cover),
            Some(NAMES_AT_LEAST),
            Some(COLUMN_GAP),
            self.format.then(theme::row_format),
            self.heard.then(theme::row_plays),
            Some(theme::row_length()),
            Some(browser::controls_width(controls)),
        ]
        .into_iter()
        .flatten()
        .collect();
        let gaps = cells.len().saturating_sub(1) as f32;

        cells.iter().sum::<f32>() + COLUMN_GAP * gaps + ROW_INSET * 2.0
    }
}

pub(crate) struct Fitting {
    room: Rc<Cell<Pixels>>,
    shown: Cell<Shown>,
}

impl Default for Fitting {
    fn default() -> Self {
        Self {
            room: Rc::new(Cell::new(px(0.0))),
            shown: Cell::new(Shown::EVERY_COLUMN),
        }
    }
}

impl Fitting {
    pub(crate) fn shown(&self) -> Shown {
        self.shown.get()
    }
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
        .ends_in_an_ellipsis()
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
    row_mark(Icon::Play)
}

pub(crate) fn playing_next_mark() -> Div {
    row_mark(Icon::QueueNext)
}

fn row_mark(icon: Icon) -> Div {
    div()
        .w(px(theme::row_number()))
        .flex_none()
        .child(icons::icon(icon, theme::row_marker_icon(), theme::accent()))
}

pub(crate) fn columns(
    numbered: &'static str,
    with_cover: bool,
    trailing_controls: usize,
    sorted: Sorted,
    fitting: &Fitting,
    cx: &mut Context<RootView>,
) -> Div {
    let shown = Shown::within(fitting.room.get(), with_cover, trailing_controls);
    fitting.shown.set(shown);

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
        .relative()
        .child(kit::measures_its_width(Rc::clone(&fitting.room)))
        .child(number)
        .when(with_cover, |header| {
            header.child(div().w(px(theme::row_cover())).flex_none())
        })
        .child(title)
        .child(artist)
        .when(shown.format, |header| {
            header.child(div().w(px(theme::row_format())).flex_none().child("FORMAT"))
        })
        .when(shown.heard, |header| header.child(heard))
        .child(length)
        .child(
            div()
                .w(px(browser::controls_width(trailing_controls)))
                .flex_none(),
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fitted_in(room: f32) -> Shown {
        Shown::within(px(room), true, browser::TRACK_CONTROLS)
    }

    #[test]
    fn a_narrowing_listing_gives_up_what_was_heard_then_the_format_before_the_names() {
        let every = Shown::EVERY_COLUMN.width(true, browser::TRACK_CONTROLS);
        let without_heard = Shown::WITHOUT_HEARD.width(true, browser::TRACK_CONTROLS);

        assert_eq!(fitted_in(every), Shown::EVERY_COLUMN);
        assert_eq!(fitted_in(every - 1.0), Shown::WITHOUT_HEARD);
        assert_eq!(fitted_in(without_heard), Shown::WITHOUT_HEARD);
        assert_eq!(fitted_in(without_heard - 1.0), Shown::NAMES_ALONE);
        assert_eq!(fitted_in(1.0), Shown::NAMES_ALONE);
    }

    #[test]
    fn a_listing_not_yet_measured_draws_every_column() {
        assert_eq!(fitted_in(0.0), Shown::EVERY_COLUMN);
    }

    #[test]
    fn the_narrowest_window_draws_the_names_alone() {
        let pane = theme::WINDOW_MIN_WIDTH - theme::sidebar_width();

        assert_eq!(fitted_in(pane), Shown::NAMES_ALONE);
    }
}
