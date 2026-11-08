use std::time::Instant;

use gpui::{
    Action, App, Context, Div, ElementId, KeyDownEvent, MouseButton, MouseDownEvent, Pixels, Point,
    Role, Size, Window, anchored, deferred, div, point, prelude::*, px, rgb,
};
use resonate_core::PlaylistId;
use resonate_library::{
    AlbumOrder, ArtistOrder, Direction, PlaylistOrder, RowOrder, SortOrder, folded_letters,
};

use crate::{
    app::{
        ORDER_CONTEXT, OrderAbove, OrderBelow, OrderFirst, OrderLast, ReadTheFirstWay,
        ReadTheOtherWay, TakeTheOrder,
    },
    motion, theme,
    views::{
        kit,
        listing::{Sortable, Sorted},
        root::RootView,
        sheets,
        spoken::Spoken,
        typing::jumped,
    },
};

const BELOW_ITS_CONTROL: f32 = 4.0;

const KEPT_FROM_THE_WINDOW_EDGE: f32 = 8.0;

const ORDERS_LABEL: &str = "Order";

const ORDERS_HINT: &str = keyed!(
    "Choose an order",
    key!(order_above),
    key!(order_below),
    "its first letters"
);

const READINGS_LABEL: &str = "Direction";

const READINGS_HINT: &str = keyed!(
    "Choose which way round it reads",
    key!(read_the_first_way),
    key!(read_the_other_way)
);

pub(crate) trait Ordering: Copy + PartialEq + 'static {
    const EVERY: &'static [Self];

    fn named(self) -> &'static str;

    fn read(self, reading: Direction) -> &'static str;
}

pub(crate) fn shape(label: &'static str) -> Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_1p5()
        .child(kit::eyebrow(label).w(theme::width(theme::field_label())))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ordered {
    Tracks,
    Albums,
    Artists,
    Playlists,
    Rows(PlaylistId),
    Queue,
    SavedSearch,
}

impl Ordered {
    const fn title(self) -> &'static str {
        match self {
            Self::Tracks => "Sort songs",
            Self::Albums => "Sort albums",
            Self::Artists => "Sort artists",
            Self::Playlists => "Sort playlists",
            Self::Rows(_) => "Sort this playlist",
            Self::Queue => "Sort the queue",
            Self::SavedSearch => "Sort this search",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stepped {
    Above,
    Below,
    First,
    Last,
}

const fn stepped_to(held: usize, every: usize, step: Stepped) -> usize {
    let last = every.saturating_sub(1);

    match step {
        Stepped::Above if held == 0 => last,
        Stepped::Above => held - 1,
        Stepped::Below if held >= last => 0,
        Stepped::Below => held + 1,
        Stepped::First => 0,
        Stepped::Last => last,
    }
}

fn typed_onto(named: &[&str], held: usize, typed: &str) -> Option<usize> {
    let folded: Vec<String> = named.iter().map(|name| folded_letters(name)).collect();
    let wanted = folded_letters(typed);
    let mut letters = wanted.chars();
    let first = letters.next()?;

    match letters.all(|letter| letter == first) {
        true => jumped(&folded, held + 1, first.encode_utf8(&mut [0; 4])),
        false => jumped(&folded, held, &wanted),
    }
}

fn typed_letter(event: &KeyDownEvent) -> Option<&str> {
    let modifiers = &event.keystroke.modifiers;
    if modifiers.control || modifiers.platform || modifiers.alt {
        return None;
    }

    event
        .keystroke
        .key_char
        .as_deref()
        .filter(|key| !key.trim().is_empty() && !key.contains(char::is_control))
}

fn order_row_id(at: usize) -> ElementId {
    ("order-choice", at).into()
}

fn reading_id(at: usize) -> ElementId {
    ("order-reading", at).into()
}

fn order_stepped<A: Action, O: Ordering>(
    step: Stepped,
    held_at: usize,
    order_by: impl Fn(&mut RootView, O, &mut Context<RootView>) + 'static,
    cx: &mut Context<RootView>,
) -> impl Fn(&A, &mut Window, &mut App) + 'static {
    cx.listener(move |this, _: &A, window, cx| {
        let at = stepped_to(held_at, O::EVERY.len(), step);
        this.hold_the_order_at(at, window, cx);
        order_by(this, O::EVERY[at], cx);
    })
}

fn read_by_key<A: Action>(
    direction: Direction,
    read_as: impl Fn(&mut RootView, Direction, &mut Context<RootView>) + 'static,
    cx: &mut Context<RootView>,
) -> impl Fn(&A, &mut Window, &mut App) + 'static {
    cx.listener(move |this, _: &A, window, cx| {
        this.carry_the_ring_onto(reading_id(usize::from(direction.code())), window, cx);
        read_as(this, direction, cx);
    })
}

fn order_typed<O: Ordering>(
    held_at: usize,
    order_by: impl Fn(&mut RootView, O, &mut Context<RootView>) + 'static,
    cx: &mut Context<RootView>,
) -> impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static {
    cx.listener(move |this, event: &KeyDownEvent, window, cx| {
        let Some(letter) = typed_letter(event) else {
            return;
        };
        cx.stop_propagation();

        let now = Instant::now();
        if this.ordering_typed.ran_out_by(now) {
            this.ordering_typed.cleared();
        }
        this.ordering_typed.took(letter, now);

        let named: Vec<&str> = O::EVERY.iter().map(|order| order.named()).collect();
        let Some(at) = this
            .ordering_typed
            .typed()
            .and_then(|typed| typed_onto(&named, held_at, typed))
        else {
            return;
        };
        this.hold_the_order_at(at, window, cx);
        order_by(this, O::EVERY[at], cx);
    })
}

impl RootView {
    pub(crate) fn order_a_listing(
        &mut self,
        ordered: Ordered,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.ordering = Some(ordered);
        self.ordering_at = at + point(px(0.0), px(BELOW_ITS_CONTROL));
        self.ordering_typed.cleared();
        cx.notify();
    }

    pub(crate) fn close_the_order(&mut self, cx: &mut Context<Self>) {
        self.ordering = None;
        cx.notify();
    }

    pub(crate) fn order_sheet(
        &self,
        ordered: Ordered,
        room: Size<Pixels>,
        cx: &mut Context<Self>,
    ) -> Div {
        let choices = match ordered {
            Ordered::Tracks => self.tracks_in_order(cx),
            Ordered::Albums => self.albums_in_order(cx),
            Ordered::Artists => self.artists_in_order(cx),
            Ordered::Playlists => self.playlists_order(cx),
            Ordered::Rows(opened) => self.rows_in_order(opened, cx),
            Ordered::Queue => self.queue_in_order(cx),
            Ordered::SavedSearch => self.search_in_order(cx),
        };
        let title = ordered.title();

        let heading = div()
            .id("order-heading")
            .headed(2, title)
            .flex_none()
            .px_2()
            .pt_1p5()
            .pb_1()
            .text_size(px(theme::text_xs()))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(rgb(theme::faint()))
            .child(title);
        let card = kit::floating()
            .id("order-sheet")
            .debug_selector(|| "order-sheet".into())
            .dialog(title)
            .w(theme::width(theme::order_width()))
            .max_h(room.height - px(KEPT_FROM_THE_WINDOW_EDGE * 2.0))
            .child(heading)
            .child(choices);

        div()
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| this.close_the_order(cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _: &MouseDownEvent, _, cx| this.close_the_order(cx)),
            )
            .child(
                deferred(
                    anchored()
                        .position(self.ordering_at)
                        .snap_to_window_with_margin(px(KEPT_FROM_THE_WINDOW_EDGE))
                        .child(motion::lifted_in(card, "order-sheet-arrives")),
                )
                .with_priority(1),
            )
    }

    pub(crate) fn order_choices<O: Ordering>(
        &self,
        held: O,
        reading: Direction,
        order_by: impl Fn(&mut RootView, O, &mut Context<RootView>) + Clone + 'static,
        read_as: impl Fn(&mut RootView, Direction, &mut Context<RootView>) + Clone + 'static,
        cx: &mut Context<RootView>,
    ) -> Div {
        let held_at = O::EVERY
            .iter()
            .position(|order| *order == held)
            .unwrap_or_default();

        let orders = O::EVERY.iter().copied().enumerate().fold(
            div()
                .id("order-choices")
                .named_as(Role::RadioGroup, ORDERS_LABEL, ORDERS_HINT)
                .flex()
                .flex_col()
                .gap_0p5()
                .min_h(px(0.0))
                .overflow_y_scroll()
                .track_scroll(&self.ordering_scroll),
            |orders, (at, order)| {
                let chosen = order_by.clone();
                orders.child(self.in_the_pane_ring(
                    kit::checked_row(order_row_id(at), order.named(), order == held),
                    move |this, _, cx| chosen(this, order, cx),
                    cx,
                ))
            },
        );
        let readings = Direction::ALL.into_iter().fold(
            kit::segmented()
                .id("order-readings")
                .named_as(Role::RadioGroup, READINGS_LABEL, READINGS_HINT)
                .w_full(),
            |readings, direction| {
                let taken = read_as.clone();
                readings.child(
                    self.in_the_pane_ring(
                        kit::segment(
                            reading_id(usize::from(direction.code())),
                            held.read(direction),
                            direction == reading,
                        )
                        .flex_1(),
                        move |this, _, cx| taken(this, direction, cx),
                        cx,
                    ),
                )
            },
        );

        div()
            .track_focus(&self.sheet_panels.holding(sheets::ORDER_PANEL, cx))
            .key_context(ORDER_CONTEXT)
            .on_action(order_stepped::<OrderAbove, O>(
                Stepped::Above,
                held_at,
                order_by.clone(),
                cx,
            ))
            .on_action(order_stepped::<OrderBelow, O>(
                Stepped::Below,
                held_at,
                order_by.clone(),
                cx,
            ))
            .on_action(order_stepped::<OrderFirst, O>(
                Stepped::First,
                held_at,
                order_by.clone(),
                cx,
            ))
            .on_action(order_stepped::<OrderLast, O>(
                Stepped::Last,
                held_at,
                order_by.clone(),
                cx,
            ))
            .on_action(read_by_key::<ReadTheFirstWay>(
                Direction::Ascending,
                read_as.clone(),
                cx,
            ))
            .on_action(read_by_key::<ReadTheOtherWay>(
                Direction::Descending,
                read_as,
                cx,
            ))
            .on_action(cx.listener(|this, _: &TakeTheOrder, _, cx| this.close_the_order(cx)))
            .on_key_down(order_typed(held_at, order_by, cx))
            .flex()
            .flex_col()
            .gap_1()
            .min_h(px(0.0))
            .child(orders)
            .child(kit::rule().mx_1())
            .child(div().flex_none().px_1().pb_1().child(readings))
    }

    fn hold_the_order_at(&self, at: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.ordering_scroll.scroll_to_item(at);
        self.carry_the_ring_onto(order_row_id(at), window, cx);
    }

    fn carry_the_ring_onto(&self, onto: ElementId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = self.sheet_panels.handle(sheets::ORDER_PANEL) else {
            return;
        };
        if panel.is_focused(window) {
            return;
        }
        if let Some(control) = self.controls.handle(&onto.to_string()) {
            window.focus(&control, cx);
        }
    }
}

impl Ordering for PlaylistOrder {
    const EVERY: &'static [Self] = &Self::ALL;

    fn named(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Created => "Newest",
            Self::Modified => "Last changed",
            Self::Played => "Last played",
            Self::Plays => "Most played",
            Self::PlaysThisMonth => "Most played this month",
        }
    }

    fn read(self, reading: Direction) -> &'static str {
        match (self, reading) {
            (Self::PlaysThisMonth, Direction::Descending) => "Most played first",
            (Self::PlaysThisMonth, Direction::Ascending) => "Least played first",
            (Self::Name, Direction::Ascending) => "A to Z",
            (Self::Name, Direction::Descending) => "Z to A",
            (Self::Created | Self::Modified, Direction::Descending) => "Newest first",
            (Self::Created | Self::Modified, Direction::Ascending) => "Oldest first",
            (Self::Played, Direction::Descending) => "Most recent first",
            (Self::Played, Direction::Ascending) => "Longest ago first",
            (Self::Plays, Direction::Descending) => "Most played first",
            (Self::Plays, Direction::Ascending) => "Least played first",
        }
    }
}

impl Ordering for RowOrder {
    const EVERY: &'static [Self] = &Self::ALL;

    fn named(self) -> &'static str {
        match self {
            Self::Album => "Album order",
            Self::Artist => "Artist",
            Self::Title => "Title",
            Self::Length => "Length",
            Self::File => "File name",
        }
    }

    fn read(self, reading: Direction) -> &'static str {
        match (self, reading) {
            (Self::Album, Direction::Ascending) => "First album first",
            (Self::Album, Direction::Descending) => "Last album first",
            (Self::Artist | Self::Title | Self::File, Direction::Ascending) => "A to Z",
            (Self::Artist | Self::Title | Self::File, Direction::Descending) => "Z to A",
            (Self::Length, Direction::Ascending) => "Shortest first",
            (Self::Length, Direction::Descending) => "Longest first",
        }
    }
}

impl Ordering for SortOrder {
    const EVERY: &'static [Self] = &Self::ALL;

    fn named(self) -> &'static str {
        match self {
            Self::Relevance => "Best match",
            Self::AlbumThenTrack => "Album order",
            Self::Title => "Title",
            Self::Artist => "Artist",
            Self::DateAdded => "Added",
            Self::Duration => "Length",
            Self::Format => "Format",
            Self::Plays => "Most played",
            Self::Played => "Last played",
            Self::Favourited => "Favourited",
            Self::PlaysThisMonth => "Most played this month",
        }
    }

    fn read(self, reading: Direction) -> &'static str {
        match (self, reading) {
            (Self::Relevance, Direction::Ascending) => "Best first",
            (Self::Relevance, Direction::Descending) => "Worst first",
            (Self::PlaysThisMonth, Direction::Descending) => "Most played first",
            (Self::PlaysThisMonth, Direction::Ascending) => "Least played first",
            (Self::AlbumThenTrack, Direction::Ascending) => "First album first",
            (Self::AlbumThenTrack, Direction::Descending) => "Last album first",
            (Self::Title | Self::Artist, Direction::Ascending) => "A to Z",
            (Self::Title | Self::Artist, Direction::Descending) => "Z to A",
            (Self::DateAdded, Direction::Descending) => "Newest first",
            (Self::DateAdded, Direction::Ascending) => "Oldest first",
            (Self::Duration, Direction::Ascending) => "Shortest first",
            (Self::Duration, Direction::Descending) => "Longest first",
            (Self::Format, Direction::Descending) => "Best quality first",
            (Self::Format, Direction::Ascending) => "Lossy first",
            (Self::Plays, Direction::Descending) => "Most played first",
            (Self::Plays, Direction::Ascending) => "Least played first",
            (Self::Played, Direction::Descending) => "Most recent first",
            (Self::Played, Direction::Ascending) => "Longest ago first",
            (Self::Favourited, Direction::Descending) => "Just favourited first",
            (Self::Favourited, Direction::Ascending) => "Favourited longest ago first",
        }
    }
}

impl Ordering for AlbumOrder {
    const EVERY: &'static [Self] = &Self::ALL;

    fn named(self) -> &'static str {
        match self {
            Self::Relevance => "Best match",
            Self::Title => "Title",
            Self::Artist => "Artist",
            Self::Year => "Year",
            Self::Tracks => "Tracks",
            Self::Added => "Added",
            Self::Favourited => "Favourited",
        }
    }

    fn read(self, reading: Direction) -> &'static str {
        match (self, reading) {
            (Self::Relevance, Direction::Ascending) => "Best first",
            (Self::Relevance, Direction::Descending) => "Worst first",
            (Self::Title | Self::Artist, Direction::Ascending) => "A to Z",
            (Self::Title | Self::Artist, Direction::Descending) => "Z to A",
            (Self::Year, Direction::Descending) => "Newest first",
            (Self::Year, Direction::Ascending) => "Oldest first",
            (Self::Tracks, Direction::Descending) => "Longest first",
            (Self::Tracks, Direction::Ascending) => "Shortest first",
            (Self::Added, Direction::Descending) => "Newest first",
            (Self::Added, Direction::Ascending) => "Oldest first",
            (Self::Favourited, Direction::Descending) => "Just favourited first",
            (Self::Favourited, Direction::Ascending) => "Favourited longest ago first",
        }
    }
}

impl Ordering for ArtistOrder {
    const EVERY: &'static [Self] = &Self::ALL;

    fn named(self) -> &'static str {
        match self {
            Self::Relevance => "Best match",
            Self::Name => "Name",
            Self::Albums => "Albums",
            Self::Tracks => "Tracks",
            Self::Favourited => "Favourited",
        }
    }

    fn read(self, reading: Direction) -> &'static str {
        match (self, reading) {
            (Self::Relevance, Direction::Ascending) => "Best first",
            (Self::Relevance, Direction::Descending) => "Worst first",
            (Self::Name, Direction::Ascending) => "A to Z",
            (Self::Name, Direction::Descending) => "Z to A",
            (Self::Albums | Self::Tracks, Direction::Descending) => "Most first",
            (Self::Albums | Self::Tracks, Direction::Ascending) => "Fewest first",
            (Self::Favourited, Direction::Descending) => "Just favourited first",
            (Self::Favourited, Direction::Ascending) => "Favourited longest ago first",
        }
    }
}

const TRACK_COLUMNS: &[Sortable] = &[
    Sortable::Number,
    Sortable::Title,
    Sortable::Artist,
    Sortable::Format,
    Sortable::Heard,
    Sortable::Length,
];

const FAVOURITE_COLUMNS: &[Sortable] = &[
    Sortable::Marked,
    Sortable::Title,
    Sortable::Artist,
    Sortable::Format,
    Sortable::Heard,
    Sortable::Length,
];

const ROW_COLUMNS: &[Sortable] = &[
    Sortable::Number,
    Sortable::Title,
    Sortable::Artist,
    Sortable::Length,
];

const COLUMN_HINT: &str = "Put the listing in this order, or press again to turn it round";

const fn track_order(column: Sortable) -> SortOrder {
    match column {
        Sortable::Number => SortOrder::AlbumThenTrack,
        Sortable::Marked => SortOrder::Favourited,
        Sortable::Title => SortOrder::Title,
        Sortable::Artist => SortOrder::Artist,
        Sortable::Format => SortOrder::Format,
        Sortable::Heard => SortOrder::Plays,
        Sortable::Length => SortOrder::Duration,
    }
}

const fn row_order(column: Sortable) -> RowOrder {
    match column {
        Sortable::Number | Sortable::Marked | Sortable::Format | Sortable::Heard => RowOrder::Album,
        Sortable::Title => RowOrder::Title,
        Sortable::Artist => RowOrder::Artist,
        Sortable::Length => RowOrder::Length,
    }
}

const fn track_column(sort: SortOrder) -> Option<Sortable> {
    match sort {
        SortOrder::Relevance
        | SortOrder::DateAdded
        | SortOrder::Played
        | SortOrder::Favourited
        | SortOrder::PlaysThisMonth => None,
        SortOrder::AlbumThenTrack => Some(Sortable::Number),
        SortOrder::Title => Some(Sortable::Title),
        SortOrder::Artist => Some(Sortable::Artist),
        SortOrder::Plays => Some(Sortable::Heard),
        SortOrder::Duration => Some(Sortable::Length),
        SortOrder::Format => Some(Sortable::Format),
    }
}

const fn favourite_column(sort: SortOrder) -> Option<Sortable> {
    match sort {
        SortOrder::Favourited => Some(Sortable::Marked),
        SortOrder::Title => Some(Sortable::Title),
        SortOrder::Artist => Some(Sortable::Artist),
        SortOrder::Plays => Some(Sortable::Heard),
        SortOrder::Duration => Some(Sortable::Length),
        SortOrder::Format => Some(Sortable::Format),
        SortOrder::Relevance
        | SortOrder::AlbumThenTrack
        | SortOrder::DateAdded
        | SortOrder::Played
        | SortOrder::PlaysThisMonth => None,
    }
}

const fn row_column(order: RowOrder) -> Option<Sortable> {
    match order {
        RowOrder::Album => Some(Sortable::Number),
        RowOrder::Artist => Some(Sortable::Artist),
        RowOrder::Title => Some(Sortable::Title),
        RowOrder::Length => Some(Sortable::Length),
        RowOrder::File => None,
    }
}

pub(crate) const fn unsorted() -> Sorted {
    Sorted {
        number: Sortable::Number,
        by: None,
        reading: Direction::Ascending,
        offers: &[],
        saying: COLUMN_HINT,
        press: |_, _, _| {},
    }
}

pub(crate) fn tracks_sorted(this: &RootView, cx: &Context<RootView>) -> Sorted {
    let sorting = this.library.read(cx).sorting();

    Sorted {
        number: Sortable::Number,
        by: track_column(sorting.tracks),
        reading: sorting.tracks_read,
        offers: TRACK_COLUMNS,
        saying: COLUMN_HINT,
        press: |this, column, cx| {
            let wanted = track_order(column);
            this.library.update(cx, |library, cx| {
                if library.sorting().tracks == wanted {
                    library.read_tracks(library.sorting().tracks_read.flipped(), cx);
                } else {
                    library.order_tracks(wanted, cx);
                }
            });
        },
    }
}

pub(crate) fn favourites_sorted(this: &RootView, cx: &Context<RootView>) -> Sorted {
    let sorting = this.library.read(cx).sorting();

    Sorted {
        number: Sortable::Marked,
        by: favourite_column(sorting.favourites),
        reading: sorting.favourites_read,
        offers: FAVOURITE_COLUMNS,
        saying: COLUMN_HINT,
        press: |this, column, cx| {
            let wanted = track_order(column);
            this.library.update(cx, |library, cx| {
                if library.sorting().favourites == wanted {
                    library.read_favourites(library.sorting().favourites_read.flipped(), cx);
                } else {
                    library.order_favourites(wanted, cx);
                }
            });
        },
    }
}

pub(crate) fn playlist_sorted(this: &RootView) -> Sorted {
    Sorted {
        number: Sortable::Number,
        by: row_column(this.row_order),
        reading: this.row_reading,
        offers: ROW_COLUMNS,
        saying: COLUMN_HINT,
        press: |this, column, cx| {
            let Some(opened) = this.library.read(cx).opened() else {
                return;
            };
            let wanted = row_order(column);
            let reading = if this.row_order == wanted {
                this.row_reading.flipped()
            } else {
                Direction::Ascending
            };

            this.put_in_order(opened, wanted, reading, cx);
        },
    }
}

pub(crate) fn queue_sorted(this: &RootView) -> Sorted {
    Sorted {
        number: Sortable::Number,
        by: row_column(this.queue_order),
        reading: this.queue_reading,
        offers: ROW_COLUMNS,
        saying: COLUMN_HINT,
        press: |this, column, cx| {
            let wanted = row_order(column);
            let reading = if this.queue_order == wanted {
                this.queue_reading.flipped()
            } else {
                Direction::Ascending
            };

            this.put_the_queue_in_order(wanted, reading, cx);
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_reading_is_named<O: Ordering>(kind: &str) {
        for order in O::EVERY {
            for reading in Direction::ALL {
                assert!(
                    !order.read(reading).is_empty(),
                    "{kind} names {} read {reading:?} nothing",
                    order.named()
                );
            }
        }

        for order in O::EVERY {
            let up = order.read(Direction::Ascending);
            let down = order.read(Direction::Descending);
            assert_ne!(
                up,
                down,
                "{kind} reads {} the same way round either way",
                order.named()
            );
        }
    }

    const ORDERS: [&str; 5] = ["Best match", "Title", "Artist", "Added", "Album order"];

    #[test]
    fn the_arrows_walk_the_orders_round_and_home_and_end_land_on_either_end() {
        assert_eq!(stepped_to(2, 5, Stepped::Above), 1);
        assert_eq!(stepped_to(2, 5, Stepped::Below), 3);
        assert_eq!(stepped_to(0, 5, Stepped::Above), 4);
        assert_eq!(stepped_to(4, 5, Stepped::Below), 0);
        assert_eq!(stepped_to(3, 5, Stepped::First), 0);
        assert_eq!(stepped_to(1, 5, Stepped::Last), 4);
    }

    #[test]
    fn a_letter_typed_again_walks_on_through_the_orders_it_begins() {
        assert_eq!(typed_onto(&ORDERS, 0, "a"), Some(2));
        assert_eq!(typed_onto(&ORDERS, 2, "aa"), Some(3));
        assert_eq!(typed_onto(&ORDERS, 3, "aaa"), Some(4));
        assert_eq!(typed_onto(&ORDERS, 4, "aaaa"), Some(2));
    }

    #[test]
    fn letters_typed_together_hold_the_order_they_already_name() {
        assert_eq!(typed_onto(&ORDERS, 2, "ar"), Some(2));
        assert_eq!(typed_onto(&ORDERS, 0, "AD"), Some(3));
        assert_eq!(typed_onto(&ORDERS, 0, "al"), Some(4));
    }

    #[test]
    fn letters_no_order_holds_leave_the_order_where_it_is() {
        assert_eq!(typed_onto(&ORDERS, 1, "zq"), None);
        assert_eq!(typed_onto(&ORDERS, 1, ""), None);
    }

    #[test]
    fn every_column_the_favourites_offer_is_held_by_the_order_it_puts_the_rows_in() {
        for column in FAVOURITE_COLUMNS {
            assert_eq!(favourite_column(track_order(*column)), Some(*column));
        }
    }

    #[test]
    fn favourites_open_in_the_order_they_were_marked_newest_first() {
        let sorting = crate::models::Sorting::default();

        assert_eq!(sorting.favourites, SortOrder::Favourited);
        assert_eq!(sorting.favourites_read, Direction::Descending);
        assert_eq!(favourite_column(sorting.favourites), Some(Sortable::Marked));
    }

    #[test]
    fn every_order_a_pane_offers_reads_differently_each_way_round() {
        every_reading_is_named::<PlaylistOrder>("a playlist listing");
        every_reading_is_named::<RowOrder>("a playlist's rows");
        every_reading_is_named::<SortOrder>("the tracks pane");
        every_reading_is_named::<AlbumOrder>("the albums pane");
        every_reading_is_named::<ArtistOrder>("the artists pane");
    }

    #[test]
    fn no_two_orders_in_one_pane_are_named_alike() {
        fn all_apart<O: Ordering>(kind: &str) {
            let mut named: Vec<&'static str> = O::EVERY.iter().map(|order| order.named()).collect();
            let offered = named.len();
            named.sort_unstable();
            named.dedup();

            assert_eq!(named.len(), offered, "{kind} names two orders alike");
        }

        all_apart::<PlaylistOrder>("a playlist listing");
        all_apart::<RowOrder>("a playlist's rows");
        all_apart::<SortOrder>("the tracks pane");
        all_apart::<AlbumOrder>("the albums pane");
        all_apart::<ArtistOrder>("the artists pane");
    }
}
