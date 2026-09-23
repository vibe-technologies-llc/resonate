use gpui::{
    Context, Div, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
    div,
};
use resonate_library::{AlbumOrder, ArtistOrder, Direction, PlaylistOrder, RowOrder, SortOrder};

use crate::{
    theme,
    views::{
        hint::Names as _,
        kit,
        listing::{Sortable, Sorted},
        root::RootView,
    },
};

const ORDER_HINT: &str = "What the list is put in order by";
const READING_HINT: &str = "Which way round that order runs";

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

pub(crate) fn order_row<O: Ordering>(
    orders_named: &'static str,
    readings_named: &'static str,
    held: O,
    reading: Direction,
    order_by: impl Fn(&mut RootView, O, &mut Context<RootView>) + Clone + 'static,
    read_as: impl Fn(&mut RootView, Direction, &mut Context<RootView>) + Clone + 'static,
    cx: &mut Context<RootView>,
) -> Div {
    let mut orders = shape("Order");
    for (index, order) in O::EVERY.iter().copied().enumerate() {
        let chosen = order_by.clone();
        orders = orders.child(
            kit::chip(
                (orders_named, index),
                SharedString::new_static(order.named()),
                order == held,
            )
            .names(ORDER_HINT)
            .on_click(cx.listener(move |this, _, _, cx| chosen(this, order, cx))),
        );
    }

    let mut readings = shape("Reading");
    for (index, direction) in Direction::ALL.into_iter().enumerate() {
        let taken = read_as.clone();
        readings = readings.child(
            kit::chip(
                (readings_named, index),
                SharedString::new_static(held.read(direction)),
                direction == reading,
            )
            .names(READING_HINT)
            .on_click(cx.listener(move |this, _, _, cx| taken(this, direction, cx))),
        );
    }

    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(orders)
        .child(readings)
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
        }
    }

    fn read(self, reading: Direction) -> &'static str {
        match (self, reading) {
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
            Self::Plays => "Most played",
            Self::Played => "Last played",
            Self::Favourited => "Favourited",
        }
    }

    fn read(self, reading: Direction) -> &'static str {
        match (self, reading) {
            (Self::Relevance, Direction::Ascending) => "Best first",
            (Self::Relevance, Direction::Descending) => "Worst first",
            (Self::AlbumThenTrack, Direction::Ascending) => "First album first",
            (Self::AlbumThenTrack, Direction::Descending) => "Last album first",
            (Self::Title | Self::Artist, Direction::Ascending) => "A to Z",
            (Self::Title | Self::Artist, Direction::Descending) => "Z to A",
            (Self::DateAdded, Direction::Descending) => "Newest first",
            (Self::DateAdded, Direction::Ascending) => "Oldest first",
            (Self::Duration, Direction::Ascending) => "Shortest first",
            (Self::Duration, Direction::Descending) => "Longest first",
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
        Sortable::Title => SortOrder::Title,
        Sortable::Artist => SortOrder::Artist,
        Sortable::Heard => SortOrder::Plays,
        Sortable::Length => SortOrder::Duration,
    }
}

const fn row_order(column: Sortable) -> RowOrder {
    match column {
        Sortable::Number | Sortable::Heard => RowOrder::Album,
        Sortable::Title => RowOrder::Title,
        Sortable::Artist => RowOrder::Artist,
        Sortable::Length => RowOrder::Length,
    }
}

const fn track_column(sort: SortOrder) -> Option<Sortable> {
    match sort {
        SortOrder::Relevance | SortOrder::DateAdded | SortOrder::Played | SortOrder::Favourited => {
            None
        }
        SortOrder::AlbumThenTrack => Some(Sortable::Number),
        SortOrder::Title => Some(Sortable::Title),
        SortOrder::Artist => Some(Sortable::Artist),
        SortOrder::Plays => Some(Sortable::Heard),
        SortOrder::Duration => Some(Sortable::Length),
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

pub(crate) fn playlist_sorted(this: &RootView) -> Sorted {
    Sorted {
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
        let mut named: Vec<(&'static str, &'static str)> = Vec::new();

        for order in O::EVERY {
            for reading in Direction::ALL {
                let reads = order.read(reading);
                assert!(
                    !reads.is_empty(),
                    "{kind} names {} read {reading:?} nothing",
                    order.named()
                );
                named.push((order.named(), reads));
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
