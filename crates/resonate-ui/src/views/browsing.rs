use std::{cell::RefCell, mem};

use ahash::{AHashMap, AHashSet};
use gpui::{
    App, Context, Entity, Pixels, Point, ScrollHandle, Subscription, Task, UniformListScrollHandle,
};
use resonate_core::{AlbumId, PlaylistId, Span, TrackId};
use resonate_engine::{QueueRows, Queued};
use resonate_library::{Direction, Kept, RowOrder, SortOrder};

use crate::{
    LibraryModel, Pressings,
    models::AroundTheRows,
    views::{
        RootView,
        menu::AtTheReach,
        reorder::{Chosen, Reach, Shift},
        sorting::Ordered,
        typing::TypeAhead,
    },
};

pub(crate) struct Scrolls {
    pub(crate) artist_records: ScrollHandle,
    pub(crate) ordering: ScrollHandle,
    pub(crate) search: ScrollHandle,
    pub(crate) playlist_rows: UniformListScrollHandle,
    pub(crate) track_rows: UniformListScrollHandle,
    pub(crate) artist_rows: UniformListScrollHandle,
    pub(crate) album_rows: UniformListScrollHandle,
    pub(crate) all_playlist_rows: UniformListScrollHandle,
    pub(crate) favourite_rows: UniformListScrollHandle,
    pub(crate) shelves: RefCell<AHashMap<&'static str, ScrollHandle>>,
    pub(crate) left_at: AHashMap<PlaylistId, UniformListScrollHandle>,
}

pub(crate) struct Orders {
    pub(crate) row_order: RowOrder,
    pub(crate) row_reading: Direction,
    pub(crate) keeping: bool,
    pub(crate) query_sort: SortOrder,
    pub(crate) query_reading: Direction,
    pub(crate) query_cap: Option<usize>,
    pub(crate) ordering: Option<Ordered>,
    pub(crate) ordering_at: Point<Pixels>,
    pub(crate) ordering_typed: TypeAhead,
}

struct Pressed {
    album: AlbumId,
    pressings: Pressings,
    _asking: Task<()>,
}

enum QueueReachOver {
    Drawn { revision: u64, rows: QueueRows },
    TheMoveSentAt(u64),
}

impl QueueReachOver {
    fn drawn(queued: &Queued) -> Self {
        Self::Drawn {
            revision: queued.revision,
            rows: queued.rows.clone(),
        }
    }
}

#[derive(Default)]
struct QueueReach {
    over: Option<QueueReachOver>,
    moved_at: Option<u64>,
}

pub(crate) struct BrowsePanes {
    pub(crate) scrolls: Scrolls,
    pub(crate) orders: Orders,
    pub(crate) reach: Option<Reach>,
    queue_reach: QueueReach,
    around: AroundTheRows,
    pressed: Option<Pressed>,
    _landing: Subscription,
}

impl BrowsePanes {
    pub(crate) fn new(library: &Entity<LibraryModel>, cx: &mut Context<Self>) -> Self {
        let mut panes = Self {
            scrolls: Scrolls {
                artist_records: ScrollHandle::new(),
                ordering: ScrollHandle::new(),
                search: ScrollHandle::new(),
                playlist_rows: UniformListScrollHandle::default(),
                track_rows: UniformListScrollHandle::default(),
                artist_rows: UniformListScrollHandle::default(),
                album_rows: UniformListScrollHandle::default(),
                all_playlist_rows: UniformListScrollHandle::default(),
                favourite_rows: UniformListScrollHandle::default(),
                shelves: RefCell::new(AHashMap::new()),
                left_at: AHashMap::new(),
            },
            orders: Orders {
                row_order: RowOrder::default(),
                row_reading: Direction::default(),
                keeping: false,
                query_sort: SortOrder::default(),
                query_reading: SortOrder::default().reads(),
                query_cap: None,
                ordering: None,
                ordering_at: Point::default(),
                ordering_typed: TypeAhead::default(),
            },
            reach: None,
            queue_reach: QueueReach::default(),
            around: AroundTheRows::default(),
            pressed: None,
            _landing: cx.observe(library, |panes, library, cx| {
                panes.take_what_landed(&library, cx);
            }),
        };
        panes.take_what_landed(library, cx);

        panes
    }

    fn take_what_landed(&mut self, library: &Entity<LibraryModel>, cx: &mut Context<Self>) {
        let Some(landed) =
            library.update(cx, |library, _| library.take_what_goes_around_the_rows())
        else {
            return;
        };
        let records = match *self.around.records == *landed.records {
            true => mem::take(&mut self.around.records),
            false => landed.records,
        };
        self.around = AroundTheRows { records, ..landed };
        cx.notify();
    }

    pub(crate) const fn around(&self) -> &AroundTheRows {
        &self.around
    }

    pub(crate) fn pressings_of(&self, album: AlbumId) -> Option<&Pressings> {
        self.pressed
            .as_ref()
            .filter(|pressed| pressed.album == album)
            .map(|pressed| &pressed.pressings)
    }

    fn ask_for_pressings(
        &mut self,
        album: AlbumId,
        asked: Task<Pressings>,
        cx: &mut Context<Self>,
    ) {
        let asking = cx.spawn(async move |panes, cx| {
            let answered = asked.await;
            let landed = panes.update(cx, |panes, cx| {
                if let Some(pressed) = panes
                    .pressed
                    .as_mut()
                    .filter(|pressed| pressed.album == album)
                {
                    pressed.pressings = answered;
                    cx.notify();
                }
            });
            let _ = landed;
        });
        self.pressed = Some(Pressed {
            album,
            pressings: Pressings::Asking,
            _asking: asking,
        });
        cx.notify();
    }

    pub(crate) fn shelf(&self, id: &'static str) -> ScrollHandle {
        self.scrolls
            .shelves
            .borrow_mut()
            .entry(id)
            .or_default()
            .clone()
    }

    pub(crate) fn open_the_rows_of(&mut self, opened: Option<PlaylistId>) {
        self.scrolls.playlist_rows = match opened {
            Some(id) => self.scrolls.left_at.entry(id).or_default().clone(),
            None => UniformListScrollHandle::default(),
        };
    }

    pub(crate) fn read_from_the_top(&mut self, opened: Option<PlaylistId>) {
        self.scrolls.playlist_rows = UniformListScrollHandle::default();
        if let Some(opened) = opened {
            self.scrolls
                .left_at
                .insert(opened, self.scrolls.playlist_rows.clone());
        }
    }

    pub(crate) fn forget_playlists_gone(&mut self, held: &AHashSet<PlaylistId>) {
        self.scrolls.left_at.retain(|id, _| held.contains(id));
    }

    pub(crate) fn read_the_kept_order(&mut self, kept: Option<Kept>) {
        self.orders.keeping = kept.is_some();
        let Kept { order, reading } = kept.unwrap_or(Kept {
            order: RowOrder::default(),
            reading: Direction::default(),
        });
        self.orders.row_order = order;
        self.orders.row_reading = reading;
    }

    pub(crate) fn reach_in(&self, shift: Shift, held: usize) -> Option<Reach> {
        self.reach
            .as_ref()
            .filter(|reach| reach.shift == shift)
            .and_then(|reach| reach.within(held))
    }

    pub(crate) fn reaches(&self, shift: Shift, row: usize) -> bool {
        self.reach
            .as_ref()
            .is_some_and(|reach| reach.shift == shift && reach.reaches(row))
    }

    pub(crate) fn at_the_reach(&self, shift: Shift, row: usize) -> Option<AtTheReach> {
        self.reach
            .as_ref()
            .filter(|reach| reach.shift == shift && reach.row == row)
            .map(|_| AtTheReach { shift, row })
    }

    pub(crate) fn stands_unchosen_on(&self, shift: Shift, row: usize) -> bool {
        self.reach
            .as_ref()
            .is_some_and(|reach| reach.shift == shift && reach.row == row && !reach.is_chosen())
    }

    pub(crate) fn acting_on_chosen(&self, shift: Shift, row: usize) -> Chosen {
        match self.reach.as_ref().filter(|reach| reach.shift == shift) {
            Some(reach) if reach.reaches(row) => reach.selected(),
            _ => Chosen::of(Span::one(row)),
        }
    }

    pub(crate) fn reached_chosen(&self, shift: Shift) -> Option<Chosen> {
        self.reach
            .as_ref()
            .filter(|reach| reach.shift == shift)
            .map(Reach::selected)
    }

    pub(crate) fn reaching(&self, shift: Shift) -> Option<Span> {
        self.reach
            .as_ref()
            .filter(|reach| reach.shift == shift)
            .map(Reach::rows)
    }

    fn reach_over(&mut self, reach: Option<Reach>, queued: &Queued) {
        self.queue_reach.over = match reach.as_ref().map(|reach| reach.shift) {
            Some(Shift::Queue) => Some(match self.queue_reach.moved_at {
                Some(sent_at) if sent_at == queued.revision => {
                    QueueReachOver::TheMoveSentAt(sent_at)
                }
                _ => QueueReachOver::drawn(queued),
            }),
            _ => None,
        };
        if reach.is_none() {
            self.queue_reach.moved_at = None;
        }
        self.reach = reach;
    }

    pub(crate) const fn queue_moved_at(&mut self, revision: u64) {
        self.queue_reach.moved_at = Some(revision);
    }

    pub(crate) fn follow_the_queue(&mut self, now: &Queued) {
        let Some(reach) = self
            .reach
            .as_ref()
            .filter(|reach| reach.shift == Shift::Queue)
        else {
            return;
        };
        let followed = match &self.queue_reach.over {
            Some(QueueReachOver::Drawn { revision, .. }) if *revision == now.revision => return,
            Some(QueueReachOver::TheMoveSentAt(sent_at)) if *sent_at == now.revision => return,
            Some(QueueReachOver::Drawn { rows: then, .. }) => {
                let placed: AHashMap<TrackId, usize> = now
                    .rows
                    .iter()
                    .enumerate()
                    .map(|(row, item)| (item.id, row))
                    .collect();
                let now_at =
                    |row: usize| then.get(row).and_then(|item| placed.get(&item.id).copied());
                reach
                    .followed(now_at)
                    .or_else(|| reach.within(now.rows.len()))
            }
            Some(QueueReachOver::TheMoveSentAt(_)) | None => reach.within(now.rows.len()),
        };

        self.reach = followed;
        self.queue_reach.over = Some(QueueReachOver::drawn(now));
    }

    pub(crate) fn let_go_of_the_reach_where(&mut self, held: impl FnOnce(Shift) -> bool) {
        if self.reach.as_ref().is_some_and(|reach| held(reach.shift)) {
            self.reach = None;
        }
    }
}

impl RootView {
    pub(crate) fn scrolls<'a>(&self, cx: &'a App) -> &'a Scrolls {
        &self.browsing.read(cx).scrolls
    }

    pub(crate) fn orders<'a>(&self, cx: &'a App) -> &'a Orders {
        &self.browsing.read(cx).orders
    }

    pub(crate) fn change_the_orders<R>(
        &self,
        cx: &mut App,
        change: impl FnOnce(&mut Orders) -> R,
    ) -> R {
        self.browsing
            .update(cx, |browsing, _| change(&mut browsing.orders))
    }

    pub(crate) fn reach<'a>(&self, cx: &'a App) -> Option<&'a Reach> {
        self.browsing.read(cx).reach.as_ref()
    }

    pub(crate) fn set_reach(&self, reach: Option<Reach>, cx: &mut App) {
        let queued = self.player.read(cx).queued();
        self.browsing
            .update(cx, |browsing, _| browsing.reach_over(reach, &queued));
    }

    pub(crate) fn follow_the_queue_reach(&self, cx: &mut App) {
        let queued = self.player.read(cx).queued();
        self.browsing
            .update(cx, |browsing, _| browsing.follow_the_queue(&queued));
    }

    pub(crate) fn around_the_rows<'a>(&self, cx: &'a App) -> &'a AroundTheRows {
        self.browsing.read(cx).around()
    }

    pub(crate) fn pressings_of<'a>(&self, album: AlbumId, cx: &'a App) -> Option<&'a Pressings> {
        self.browsing.read(cx).pressings_of(album)
    }

    pub(crate) fn ask_for_pressings(
        &self,
        album: AlbumId,
        asked: Option<Task<Pressings>>,
        cx: &mut App,
    ) {
        let Some(asked) = asked else {
            return;
        };
        self.browsing.update(cx, |browsing, cx| {
            browsing.ask_for_pressings(album, asked, cx)
        });
    }

    pub(crate) fn let_go_of_the_pressings(&self, cx: &mut App) {
        self.browsing.update(cx, |browsing, cx| {
            browsing.pressed = None;
            cx.notify();
        });
    }
}
