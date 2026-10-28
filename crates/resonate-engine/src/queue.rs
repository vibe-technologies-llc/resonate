use std::{
    ops::{Bound, Index, Range},
    sync::Arc,
};

use ahash::AHashSet;
use imbl::{OrdMap, Vector};
use resonate_core::{FrameSpan, MediaLocation, QueueStamp, Resumption, RowStamp, Span, TrackId};

use crate::{RepeatMode, seed};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct QueueItem {
    pub id: TrackId,
    pub location: MediaLocation,
    pub span: Option<FrameSpan>,
}

impl QueueItem {
    pub const fn whole(id: TrackId, location: MediaLocation) -> Self {
        Self {
            id,
            location,
            span: None,
        }
    }
}

#[derive(Clone, Debug)]
struct Held {
    place: usize,
    stamp: RowStamp,
    item: Arc<QueueItem>,
}

impl Held {
    fn at(place: usize, item: Arc<QueueItem>) -> Self {
        Self {
            place,
            stamp: row_stamp(&item, place),
            item,
        }
    }
}

fn row_stamp(item: &QueueItem, place: usize) -> RowStamp {
    RowStamp::of((&item.location, item.span), place)
}

#[derive(Clone, Debug, Default)]
pub struct QueueRows(Vector<Held>);

impl QueueRows {
    pub fn placed(rows: impl IntoIterator<Item = (usize, QueueItem)>) -> Self {
        Self(
            rows.into_iter()
                .map(|(place, item)| Held::at(place, Arc::new(item)))
                .collect(),
        )
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, row: usize) -> Option<&Arc<QueueItem>> {
        self.0.get(row).map(|held| &held.item)
    }

    pub fn first(&self) -> Option<&Arc<QueueItem>> {
        self.0.front().map(|held| &held.item)
    }

    pub fn last(&self) -> Option<&Arc<QueueItem>> {
        self.0.back().map(|held| &held.item)
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Arc<QueueItem>> + ExactSizeIterator {
        self.0.iter().map(|held| &held.item)
    }

    pub fn run(&self, rows: Span) -> Option<Self> {
        if rows.last() >= self.len() {
            return None;
        }
        Some(Self(self.0.skip(rows.first()).take(rows.rows())))
    }

    pub fn to_vec(&self) -> Vec<Arc<QueueItem>> {
        self.iter().cloned().collect()
    }

    pub fn is_the_same_as(&self, other: &Self) -> bool {
        self.0.ptr_eq(&other.0)
    }

    pub fn loaded_at(&self) -> impl ExactSizeIterator<Item = usize> + '_ {
        self.0.iter().map(|held| held.place)
    }

    pub fn in_load_order(&self) -> Vec<usize> {
        let mut by_place: Vec<usize> = (0..self.len()).collect();
        by_place.sort_unstable_by_key(|row| self.0.get(*row).map_or(0, |held| held.place));

        let mut loaded = vec![0; self.len()];
        for (at, row) in by_place.into_iter().enumerate() {
            if let Some(slot) = loaded.get_mut(row) {
                *slot = at;
            }
        }
        loaded
    }
}

impl Index<usize> for QueueRows {
    type Output = Arc<QueueItem>;

    fn index(&self, row: usize) -> &Arc<QueueItem> {
        &self.0[row].item
    }
}

impl FromIterator<Arc<QueueItem>> for QueueRows {
    fn from_iter<Rows: IntoIterator<Item = Arc<QueueItem>>>(rows: Rows) -> Self {
        Self(
            rows.into_iter()
                .enumerate()
                .map(|(place, item)| Held::at(place, item))
                .collect(),
        )
    }
}

impl FromIterator<QueueItem> for QueueRows {
    fn from_iter<Rows: IntoIterator<Item = QueueItem>>(rows: Rows) -> Self {
        rows.into_iter().map(Arc::new).collect()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Listing {
    shuffled: bool,
    unshuffled: Standing,
    waiting: Vector<Held>,
    last_heard: Option<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct Queued {
    pub revision: u64,
    pub rows_revision: u64,
    pub rows: QueueRows,
    pub next: Option<Span>,
    pub stamp: QueueStamp,
    pub listing: Listing,
}

impl Queued {
    pub fn in_list_order(&self) -> Vec<Arc<QueueItem>> {
        self.listed()
            .map(|listed| listed.iter().map(|held| Arc::clone(&held.item)).collect())
            .unwrap_or_else(|| self.rows.to_vec())
    }

    fn listed(&self) -> Option<Vec<&Held>> {
        let listing = &self.listing;
        if !listing.shuffled {
            return None;
        }

        let mut unlisted: AHashSet<usize> = self.rows.0.iter().map(|held| held.place).collect();
        let (heard, to_come) = listing.unshuffled.around(listing.last_heard);
        let listed: Vec<&Held> = heard
            .chain(&listing.waiting)
            .chain(to_come)
            .filter(|held| unlisted.remove(&held.place))
            .collect();

        (listed.len() == self.rows.len()).then_some(listed)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Removal {
    Playing,
    Queued,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Placement {
    Next,
    Queued,
    At(usize),
}

pub fn unclaimed_id<'a>(queue: impl IntoIterator<Item = &'a QueueItem>) -> TrackId {
    Unclaimed::beside(queue).mint()
}

pub fn stamp_of<'a>(items: impl IntoIterator<Item = &'a QueueItem>) -> QueueStamp {
    QueueStamp::of(items.into_iter().map(|item| (&item.location, item.span)))
}

fn one_id_each(items: Vec<QueueItem>, minting: &mut Unclaimed) -> Vec<Arc<QueueItem>> {
    items
        .into_iter()
        .map(|item| {
            Arc::new(QueueItem {
                id: minting.claim(item.id).unwrap_or_else(|| minting.mint()),
                ..item
            })
        })
        .collect()
}

#[derive(Clone)]
pub struct Unclaimed {
    taken: AHashSet<u64>,
    next: u64,
}

impl Unclaimed {
    pub fn beside<'a>(queue: impl IntoIterator<Item = &'a QueueItem>) -> Self {
        Self {
            taken: queue.into_iter().map(|item| item.id.get()).collect(),
            next: u64::MAX,
        }
    }

    pub fn mint(&mut self) -> TrackId {
        while self.taken.contains(&self.next) {
            match self.next.checked_sub(1) {
                Some(below) => self.next = below,
                None => return TrackId::MAX,
            }
        }

        let minted = TrackId::new(self.next).unwrap_or(TrackId::MAX);
        self.next = self.next.saturating_sub(1);
        self.claim(minted);
        minted
    }

    pub fn claim(&mut self, id: TrackId) -> Option<TrackId> {
        self.taken.insert(id.get()).then_some(id)
    }

    fn release(&mut self, id: TrackId) {
        self.taken.remove(&id.get());
    }
}

#[derive(Clone)]
struct Shuffler(u64);

impl Shuffler {
    fn new() -> Self {
        Self(seed::from_clock())
    }

    fn next_below(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % bound as u64) as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Seat {
    Nowhere,
    InOrder,
    Next,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Reckoning {
    seat: Seat,
    after: usize,
    let_go: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    InOrder(usize),
    Next(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Waits {
    InOrder,
    Next,
}

type Flagged = Vec<(Held, Waits)>;

struct Lifted {
    passed: Vector<Held>,
    queued: Vector<Held>,
    upcoming: Vector<Held>,
    kept_passed: usize,
}

impl Lifted {
    fn carries(&self, place: usize) -> bool {
        self.passed.back().is_some_and(|held| held.place == place)
            || self.queued.front().is_some_and(|held| held.place == place)
    }
}

fn spliced(into: &mut Vector<Held>, at: usize, run: Vector<Held>) {
    let rest = into.split_off(at.min(into.len()));
    into.append(run);
    into.append(rest);
}

type Label = u64;

const ROOM_BETWEEN_LABELS: Label = 1 << 20;

#[derive(Clone, Copy, Debug)]
enum Anchor {
    Front,
    After(usize),
    Back,
}

#[derive(Clone, Debug, Default)]
struct Standing {
    rows: OrdMap<Label, Held>,
    labels: OrdMap<usize, Label>,
}

impl Standing {
    fn of(rows: impl IntoIterator<Item = Held>) -> Self {
        let rows: Vec<Held> = rows.into_iter().collect();
        let stride = Label::MAX / (rows.len() as Label + 1);
        let mut standing = Self::default();
        for (at, held) in rows.into_iter().enumerate() {
            standing.put(stride * (at as Label + 1), held);
        }
        standing
    }

    fn put(&mut self, label: Label, held: Held) {
        self.labels.insert(held.place, label);
        self.rows.insert(label, held);
    }

    fn iter(&self) -> impl Iterator<Item = &Held> {
        self.rows.values()
    }

    fn contains(&self, place: usize) -> bool {
        self.labels.contains_key(&place)
    }

    fn to_vector(&self) -> Vector<Held> {
        self.iter().cloned().collect()
    }

    fn remove(&mut self, place: usize) {
        if let Some(label) = self.labels.remove(&place) {
            self.rows.remove(&label);
        }
    }

    fn around(
        &self,
        heard: Option<usize>,
    ) -> (impl Iterator<Item = &Held>, impl Iterator<Item = &Held>) {
        let label = heard
            .and_then(|place| self.labels.get(&place))
            .copied()
            .unwrap_or(Label::MIN);
        (
            self.rows.range(..=label).map(|(_, held)| held),
            self.rows
                .range((Bound::Excluded(label), Bound::Unbounded))
                .map(|(_, held)| held),
        )
    }

    fn insert(&mut self, anchor: Anchor, run: &Vector<Held>) {
        if run.is_empty() {
            return;
        }
        let low = match anchor {
            Anchor::Front => Label::MIN,
            Anchor::After(place) => self
                .labels
                .get(&place)
                .copied()
                .unwrap_or_else(|| self.last()),
            Anchor::Back => self.last(),
        };
        let high = self
            .rows
            .range((Bound::Excluded(low), Bound::Unbounded))
            .next()
            .map_or(Label::MAX, |(label, _)| *label);
        let step = (high - low) / (run.len() as Label + 1);

        if step < ROOM_BETWEEN_LABELS {
            self.respace(low, run);
            return;
        }
        for (at, held) in run.iter().enumerate() {
            self.put(low + step * (at as Label + 1), held.clone());
        }
    }

    fn respace(&mut self, low: Label, run: &Vector<Held>) {
        let mut width = 16;
        loop {
            let left: Vec<(Label, Held)> = self
                .rows
                .range(..=low)
                .rev()
                .take(width)
                .map(|(label, held)| (*label, held.clone()))
                .collect();
            let right: Vec<(Label, Held)> = self
                .rows
                .range((Bound::Excluded(low), Bound::Unbounded))
                .take(width)
                .map(|(label, held)| (*label, held.clone()))
                .collect();
            let floor = match left.last() {
                Some((label, _)) if left.len() == width => self
                    .rows
                    .range(..*label)
                    .next_back()
                    .map_or(Label::MIN, |(before, _)| *before),
                _ => Label::MIN,
            };
            let ceiling = match right.last() {
                Some((label, _)) if right.len() == width => self
                    .rows
                    .range((Bound::Excluded(*label), Bound::Unbounded))
                    .next()
                    .map_or(Label::MAX, |(after, _)| *after),
                _ => Label::MAX,
            };
            let step = (ceiling - floor) / ((left.len() + run.len() + right.len()) as Label + 1);
            let everything = left.len() < width && right.len() < width;
            if step < ROOM_BETWEEN_LABELS && !everything {
                width *= 2;
                continue;
            }

            for (label, _) in left.iter().chain(&right) {
                self.rows.remove(label);
            }
            let respaced = left
                .into_iter()
                .rev()
                .map(|(_, held)| held)
                .chain(run.iter().cloned())
                .chain(right.into_iter().map(|(_, held)| held));
            for (at, held) in respaced.enumerate() {
                self.put(floor + step.max(1) * (at as Label + 1), held);
            }
            return;
        }
    }

    fn last(&self) -> Label {
        self.rows.get_max().map_or(Label::MIN, |(label, _)| *label)
    }

    fn relocate(&mut self, moved: &[(MediaLocation, MediaLocation)]) {
        let landed: Vec<(Label, MediaLocation)> = self
            .rows
            .iter()
            .filter_map(|(label, held)| {
                landed_at(&held.item.location, moved).map(|to| (*label, to))
            })
            .collect();
        for (label, to) in landed {
            if let Some(held) = self.rows.get_mut(&label) {
                Arc::make_mut(&mut held.item).location = to;
            }
        }
    }

    #[cfg(test)]
    fn places(&self) -> Vec<usize> {
        self.iter().map(|held| held.place).collect()
    }
}

#[cfg_attr(test, derive(Clone))]
pub struct Queue {
    order: Vector<Held>,
    unshuffled: Standing,
    next: Vector<Held>,
    places: usize,
    ids: Unclaimed,
    after: usize,
    seat: Seat,
    repeat: RepeatMode,
    shuffle: bool,
    shuffler: Shuffler,
    revision: u64,
    rows_revision: u64,
    stamp: QueueStamp,
    playing_from: QueueStamp,
}

impl Queue {
    pub fn new() -> Self {
        Self {
            order: Vector::new(),
            unshuffled: Standing::default(),
            next: Vector::new(),
            places: 0,
            ids: Unclaimed::beside([]),
            after: 0,
            seat: Seat::Nowhere,
            repeat: RepeatMode::Off,
            shuffle: false,
            shuffler: Shuffler::new(),
            revision: 0,
            rows_revision: 0,
            stamp: QueueStamp::default(),
            playing_from: QueueStamp::default(),
        }
    }

    pub fn load(&mut self, items: Vec<QueueItem>, start_at: usize) -> Option<usize> {
        let kept: Vec<Arc<QueueItem>> = self
            .waiting()
            .iter()
            .map(|held| Arc::clone(&held.item))
            .collect();
        let mut minting = Unclaimed::beside(kept.iter().map(Arc::as_ref));
        let loaded = one_id_each(items, &mut minting);
        let count = loaded.len();

        self.places = 0;
        let order: Vector<Held> = loaded.into_iter().map(|item| self.held(item)).collect();
        let next: Vector<Held> = kept.into_iter().map(|item| self.held(item)).collect();
        self.ids = minting;
        self.unshuffled = self.standing_of(&order);
        self.order = order;
        self.next = next;
        self.stamp_afresh();
        self.rows_moved();

        let Some(last) = count.checked_sub(1) else {
            self.after = 0;
            self.seat = if self.next.is_empty() {
                Seat::Nowhere
            } else {
                Seat::Next
            };
            return self.heard_place();
        };
        let start = start_at.min(last);

        if self.shuffle {
            self.reshuffle_keeping(start);
        } else {
            self.after = start + 1;
        }
        self.seat = Seat::InOrder;
        self.heard_place()
    }

    pub fn restore(&mut self, resumption: Resumption) -> Option<usize> {
        let row = resumption.landing();
        let shuffle = resumption.shuffle;
        let drawn = resumption.in_play_order();
        let next = resumption.next.filter(|next| next.last() < drawn.len());
        let mut minting = Unclaimed::beside([]);

        self.places = 0;
        let rows: Vec<Held> = resumption
            .rows
            .into_iter()
            .map(|row| {
                let item = Arc::new(QueueItem {
                    id: row
                        .held
                        .and_then(|held| minting.claim(held))
                        .unwrap_or_else(|| minting.mint()),
                    location: row.location,
                    span: row.span,
                });
                self.held(item)
            })
            .collect();
        self.ids = minting;
        self.shuffle = shuffle;

        let (order, waiting, after, seat) = match next {
            _ if drawn.is_empty() => (Vec::new(), Vec::new(), 0, Seat::Nowhere),
            None => (drawn, Vec::new(), row + 1, Seat::InOrder),
            Some(next) if next.holds(row) => {
                let rest = next.last() + 1;
                let mut order = drawn[..row].to_vec();
                order.extend_from_slice(&drawn[rest..]);
                (order, drawn[row..rest].to_vec(), row, Seat::Next)
            }
            Some(next) => {
                let mut order = drawn[..next.first()].to_vec();
                order.extend_from_slice(&drawn[next.last() + 1..]);
                let heard = if row < next.first() {
                    row
                } else {
                    row - next.rows()
                };
                (
                    order,
                    drawn[next.range()].to_vec(),
                    heard + 1,
                    Seat::InOrder,
                )
            }
        };
        let of = |loaded: &[usize]| -> Vector<Held> {
            loaded
                .iter()
                .filter_map(|index| rows.get(*index))
                .cloned()
                .collect()
        };

        self.unshuffled = if shuffle {
            let mut unshuffled = order.clone();
            unshuffled.sort_unstable();
            Standing::of(of(&unshuffled))
        } else {
            Standing::default()
        };
        self.order = of(&order);
        self.next = of(&waiting);
        self.after = after;
        self.seat = seat;
        self.stamp_afresh();
        self.rows_moved();
        self.heard_place()
    }

    pub fn insert(&mut self, items: Vec<QueueItem>, at: Placement) -> Option<usize> {
        if items.is_empty() {
            return None;
        }
        let arrived = one_id_each(items, &mut self.ids);
        let row = self.landing(at);
        let before_the_queued = self.seat == Seat::Next
            && matches!(at, Placement::At(row) if self.cursor().is_some_and(|playing| row <= playing));
        let landed: Vector<Held> = arrived.into_iter().map(|item| self.held(item)).collect();

        for held in &landed {
            self.joined(held, row_waits(row));
        }
        match row {
            Row::Next(at) => spliced(&mut self.next, at, landed),
            Row::InOrder(at) => self.land_in_order(at, landed, before_the_queued),
        }
        self.rows_moved();
        Some(self.drawn_at(row))
    }

    pub fn insert_to_hear(&mut self, items: Vec<QueueItem>, at: Placement) -> Option<usize> {
        let first = self.places;
        let run = first..first + items.len();
        let row = self.insert(items, at)?;
        let heard = self.jump_to(row);

        if self.seat == Seat::Next
            && let Some(head) = self.next.front().map(|held| held.place)
        {
            let rest: Vector<Held> = self
                .next
                .iter()
                .skip(1)
                .filter(|held| run.contains(&held.place))
                .cloned()
                .collect();
            self.next
                .retain(|held| held.place == head || !run.contains(&held.place));
            spliced(&mut self.next, 1, rest);
            self.revision = self.revision.wrapping_add(1);
        }
        heard
    }

    pub fn remove_rows(&mut self, rows: Span) -> Option<Removal> {
        self.remove_all(&[rows]).ok()
    }

    pub fn remove_all(&mut self, runs: &[Span]) -> Result<Removal, Span> {
        let len = self.len();
        if let Some(outside) = runs.iter().copied().find(|rows| rows.last() >= len) {
            return Err(outside);
        }

        let heard = self.heard_place();
        let mut from_order = Vec::new();
        let mut from_next = Vec::new();
        for row in runs.iter().flat_map(|rows| rows.range()) {
            match self.row(row) {
                Some(Row::InOrder(at)) => from_order.push(at),
                Some(Row::Next(at)) => from_next.push(at),
                None => {}
            }
        }
        for positions in [&mut from_order, &mut from_next] {
            positions.sort_unstable();
            positions.dedup();
        }
        let heard_before = from_order.iter().filter(|at| **at < self.after).count();

        let mut dropped = AHashSet::with_capacity(from_order.len() + from_next.len());
        for at in from_order.into_iter().rev() {
            let held = self.order.remove(at);
            self.left(&held, Waits::InOrder);
            dropped.insert(held.place);
        }
        for at in from_next.into_iter().rev() {
            let held = self.next.remove(at);
            self.left(&held, Waits::Next);
            dropped.insert(held.place);
        }
        for place in &dropped {
            self.unshuffled.remove(*place);
        }
        self.after -= heard_before;
        self.rows_moved();

        if heard.is_some_and(|place| dropped.contains(&place)) {
            self.hand_on();
            return Ok(Removal::Playing);
        }
        Ok(Removal::Queued)
    }

    pub fn move_rows(&mut self, rows: Span, to: usize) -> Option<()> {
        let last = self.len().checked_sub(1)?;
        if rows.last() > last || to > last {
            return None;
        }
        if rows.holds(to) {
            return Some(());
        }

        let heard = self.heard_place();
        let landing = rows.landing(to);
        let lifted = self.lift(rows);
        let carries_the_heard = heard.is_some_and(|heard| lifted.carries(heard));
        let waits = (!carries_the_heard).then(|| self.waits_beside(landing, &lifted, heard));
        let kept_passed = lifted.kept_passed;
        let kept_queued = self.next.len();
        let passed_len = lifted.passed.len();

        let into_order = match waits {
            Some(Waits::Next) => 0,
            Some(Waits::InOrder) => passed_len + lifted.queued.len() + lifted.upcoming.len(),
            None => passed_len + lifted.upcoming.len(),
        };
        let crosses = match waits {
            Some(Waits::Next) => passed_len + lifted.upcoming.len() > 0,
            Some(Waits::InOrder) => !lifted.queued.is_empty(),
            None => false,
        };

        match waits {
            Some(Waits::Next) => {
                for held in lifted.passed.iter().chain(&lifted.upcoming) {
                    self.playing_from = self.playing_from.without(held.stamp);
                }
            }
            Some(Waits::InOrder) => {
                for held in &lifted.queued {
                    self.playing_from = self.playing_from.with(held.stamp);
                }
            }
            None => {}
        }

        let order_at = if landing <= kept_passed {
            landing
        } else if landing <= kept_passed + kept_queued {
            kept_passed
        } else {
            landing - kept_queued
        };
        let next_at = landing.saturating_sub(kept_passed).min(kept_queued);

        let after = match heard {
            None => self.order.len() + into_order,
            Some(_) if carries_the_heard => order_at + passed_len,
            Some(_) if self.seat == Seat::Next => {
                kept_passed
                    + if landing <= kept_passed {
                        into_order
                    } else {
                        0
                    }
            }
            Some(_) => {
                kept_passed
                    + if order_at < kept_passed {
                        into_order
                    } else {
                        0
                    }
            }
        };

        let Lifted {
            mut passed,
            queued,
            upcoming,
            ..
        } = lifted;
        let (order_run, next_run) = match waits {
            None => {
                passed.append(upcoming);
                (passed, queued)
            }
            Some(waits) => {
                passed.append(queued);
                passed.append(upcoming);
                match waits {
                    Waits::InOrder => (passed, Vector::new()),
                    Waits::Next => (Vector::new(), passed),
                }
            }
        };

        let moved = (order_run.clone(), next_run.clone());
        if !order_run.is_empty() {
            spliced(&mut self.order, order_at, order_run);
        }
        if !next_run.is_empty() {
            spliced(&mut self.next, next_at, next_run);
        }
        if carries_the_heard && self.seat == Seat::Next && next_at > 0 {
            let held = self.next.remove(next_at);
            self.next.push_front(held);
        }

        if self.shuffle && crosses {
            let (into_order, into_next) = moved;
            self.unshuffle_moved(order_at, &into_order, &into_next);
        }
        self.after = after;
        self.revision = self.revision.wrapping_add(1);

        Some(())
    }

    pub fn reorder(&mut self, rows: &[usize]) -> Option<()> {
        if rows.len() != self.len() {
            return None;
        }

        let mut named = vec![false; rows.len()];
        for row in rows {
            let once = named.get_mut(*row)?;
            if *once {
                return None;
            }
            *once = true;
        }

        let drawn = self.flagged();
        let reordered = rows
            .iter()
            .filter_map(|row| drawn.get(*row))
            .cloned()
            .collect();
        self.split(reordered, self.heard_place());

        Some(())
    }

    pub fn len(&self) -> usize {
        self.order.len() + self.next.len()
    }

    pub fn relocate(&mut self, moved: &[(MediaLocation, MediaLocation)]) -> bool {
        let mut relocated = relocated_in(&mut self.order, moved);
        relocated |= relocated_in(&mut self.next, moved);
        if !relocated {
            return false;
        }

        self.unshuffled.relocate(moved);
        self.revision = self.revision.wrapping_add(1);
        true
    }

    pub const fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    pub const fn shuffle(&self) -> bool {
        self.shuffle
    }

    pub fn position(&self) -> Option<usize> {
        match self.seat {
            Seat::InOrder => self.last_heard().map(|held| held.place),
            Seat::Nowhere | Seat::Next => None,
        }
    }

    pub const fn cursor(&self) -> Option<usize> {
        match self.seat {
            Seat::Nowhere => None,
            Seat::InOrder => self.after.checked_sub(1),
            Seat::Next => Some(self.after),
        }
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub const fn playing_from(&self) -> QueueStamp {
        self.playing_from
    }

    pub fn playing_next(&self) -> Option<Span> {
        let last = self.next.len().checked_sub(1)?;
        Some(Span::between(self.after, self.after + last))
    }

    pub fn in_play_order(&self) -> QueueRows {
        let mut drawn = self.order.clone();
        let rest = drawn.split_off(self.after.min(drawn.len()));
        drawn.append(self.next.clone());
        drawn.append(rest);
        QueueRows(drawn)
    }

    pub fn published(&self) -> Queued {
        Queued {
            revision: self.revision,
            rows_revision: self.rows_revision,
            rows: self.in_play_order(),
            next: self.playing_next(),
            stamp: self.stamp,
            listing: Listing {
                shuffled: self.shuffle,
                unshuffled: self.unshuffled.clone(),
                waiting: self.next.clone(),
                last_heard: self.last_heard().map(|held| held.place),
            },
        }
    }

    pub fn current(&self) -> Option<&QueueItem> {
        self.heard().map(|held| held.item.as_ref())
    }

    pub fn set_repeat(&mut self, repeat: RepeatMode) {
        self.repeat = repeat;
    }

    pub fn set_shuffle(&mut self, shuffle: bool) {
        if self.shuffle == shuffle {
            return;
        }
        self.shuffle = shuffle;
        let last_heard = self.last_heard().map(|held| held.place);

        if shuffle {
            self.unshuffled = Standing::of(self.order.iter().cloned());
            match (self.seat, last_heard) {
                (Seat::InOrder | Seat::Next, Some(heard)) => self.reshuffle_keeping(heard),
                _ => self.reshuffle(),
            }
            return;
        }

        self.order = self.unshuffled.to_vector();
        self.unshuffled = Standing::default();
        self.after = match (self.seat, last_heard) {
            (Seat::Nowhere, _) => self.order.len(),
            (_, Some(heard)) => self
                .order
                .iter()
                .position(|held| held.place == heard)
                .map_or(0, |at| at + 1),
            (_, None) => 0,
        };
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn wraps_next(&self) -> bool {
        self.wraps_next_after(0)
    }

    pub fn wraps_next_after(&self, rows_ahead: usize) -> bool {
        self.repeat == RepeatMode::Queue && self.is_last_after(rows_ahead)
    }

    pub fn ends_with_this_row(&self) -> bool {
        self.ends_with_the_row_after(0)
    }

    pub fn ends_with_the_row_after(&self, rows_ahead: usize) -> bool {
        self.repeat == RepeatMode::Off && self.is_last_after(rows_ahead)
    }

    fn is_last_after(&self, rows_ahead: usize) -> bool {
        self.reckoned_after(rows_ahead).is_some_and(|reckoning| {
            reckoning.seat != Seat::Nowhere
                && self.waiting_at(reckoning) == 0
                && reckoning.after >= self.order.len()
        })
    }

    fn reckoned_after(&self, rows_ahead: usize) -> Option<Reckoning> {
        let mut reckoning = Reckoning {
            seat: self.seat,
            after: self.after,
            let_go: 0,
        };
        if self.repeat == RepeatMode::Track {
            return Some(reckoning);
        }
        for _ in 0..rows_ahead {
            reckoning = self.advanced_from(reckoning)?;
        }
        Some(reckoning)
    }

    fn advanced_from(&self, reckoning: Reckoning) -> Option<Reckoning> {
        let let_go = match reckoning.seat {
            Seat::Nowhere => return None,
            Seat::Next => reckoning.let_go + 1,
            Seat::InOrder => reckoning.let_go,
        };
        if self.next.len() > let_go {
            Some(Reckoning {
                seat: Seat::Next,
                let_go,
                ..reckoning
            })
        } else if reckoning.after < self.order.len() {
            Some(Reckoning {
                seat: Seat::InOrder,
                after: reckoning.after + 1,
                let_go,
            })
        } else if self.repeat == RepeatMode::Queue && !self.shuffle && !self.order.is_empty() {
            Some(Reckoning {
                seat: Seat::InOrder,
                after: 1,
                let_go,
            })
        } else {
            None
        }
    }

    fn waiting_at(&self, reckoning: Reckoning) -> usize {
        let heard = usize::from(reckoning.seat == Seat::Next);
        self.next
            .len()
            .saturating_sub(reckoning.let_go)
            .saturating_sub(heard)
    }

    pub fn advance(&mut self, natural: bool) -> Option<usize> {
        if self.seat == Seat::Nowhere {
            return None;
        }
        if natural && self.repeat == RepeatMode::Track {
            return self.heard_place();
        }
        if self.seat == Seat::Next {
            self.let_go_of_what_was_heard();
        }

        if !self.next.is_empty() {
            self.seat = Seat::Next;
        } else if self.after < self.order.len() {
            self.after += 1;
            self.seat = Seat::InOrder;
        } else if self.repeat == RepeatMode::Queue && !self.order.is_empty() {
            self.wrap_to_the_start();
        } else {
            self.seat = Seat::Nowhere;
        }
        self.heard_place()
    }

    pub fn follows_naturally_after(&self, rows_ahead: usize) -> Option<&QueueItem> {
        if self.seat == Seat::Nowhere {
            return None;
        }
        if self.repeat == RepeatMode::Track {
            return self.current();
        }
        let reckoning = self.reckoned_after(rows_ahead)?;
        let waiting = match reckoning.seat {
            Seat::Next => self.next.get(reckoning.let_go + 1),
            Seat::Nowhere | Seat::InOrder => self.next.get(reckoning.let_go),
        };
        let held = waiting
            .or_else(|| self.order.get(reckoning.after))
            .or_else(|| {
                (self.repeat == RepeatMode::Queue && !self.shuffle)
                    .then(|| self.order.front())
                    .flatten()
            })?;
        Some(held.item.as_ref())
    }

    pub fn retreat(&mut self) -> Option<usize> {
        match self.seat {
            Seat::Nowhere => return None,
            Seat::Next if self.after > 0 => self.seat = Seat::InOrder,
            Seat::Next if self.repeat == RepeatMode::Queue && !self.order.is_empty() => {
                self.seat_after(self.order.len());
                self.seat = Seat::InOrder;
            }
            Seat::InOrder if self.after > 1 => self.seat_after(self.after - 1),
            Seat::InOrder if self.repeat == RepeatMode::Queue => {
                self.seat_after(self.order.len());
            }
            Seat::InOrder | Seat::Next => {}
        }
        self.heard_place()
    }

    pub fn jump_to(&mut self, at: usize) -> Option<usize> {
        match self.row(at)? {
            Row::Next(0) if self.seat == Seat::Next => {}
            Row::Next(waiting) => {
                let mut wanted = waiting;
                if self.seat == Seat::Next && self.let_go_of_what_was_heard().is_some() {
                    wanted -= 1;
                }
                if wanted >= self.next.len() {
                    return None;
                }
                let held = self.next.remove(wanted);
                self.next.push_front(held);
                self.seat = Seat::Next;
                self.revision = self.revision.wrapping_add(1);
            }
            Row::InOrder(at) => {
                if self.seat == Seat::Next {
                    self.let_go_of_what_was_heard();
                }
                self.seat_after(at + 1);
                self.seat = Seat::InOrder;
            }
        }
        self.heard_place()
    }

    fn held(&mut self, item: Arc<QueueItem>) -> Held {
        let place = self.places;
        self.places += 1;
        Held::at(place, item)
    }

    fn joined(&mut self, held: &Held, waits: Waits) {
        self.stamp = self.stamp.with(held.stamp);
        if waits == Waits::InOrder {
            self.playing_from = self.playing_from.with(held.stamp);
        }
    }

    fn left(&mut self, held: &Held, waited: Waits) {
        self.ids.release(held.item.id);
        self.stamp = self.stamp.without(held.stamp);
        if waited == Waits::InOrder {
            self.playing_from = self.playing_from.without(held.stamp);
        }
    }

    fn landing(&self, at: Placement) -> Row {
        let Some(playing) = self.cursor() else {
            let end = self.order.len();
            return Row::InOrder(match at {
                Placement::At(row) => row.min(end),
                Placement::Next | Placement::Queued => end,
            });
        };
        let heard_next = usize::from(self.seat == Seat::Next);

        match at {
            Placement::Next => Row::Next(heard_next),
            Placement::Queued => Row::Next(self.next.len()),
            Placement::At(row) if row <= playing => Row::InOrder(row),
            Placement::At(row) if row <= self.after + self.next.len() => {
                Row::Next(row - self.after)
            }
            Placement::At(row) => Row::InOrder((row - self.next.len()).min(self.order.len())),
        }
    }

    fn land_in_order(&mut self, at: usize, landed: Vector<Held>, before_the_queued: bool) {
        let count = landed.len();
        if self.shuffle {
            let anchor = self.unshuffled_anchor(at);
            self.unshuffled.insert(anchor, &landed);
        }
        spliced(&mut self.order, at, landed);

        match self.seat {
            Seat::Nowhere => {
                self.after = at + 1;
                self.seat = Seat::InOrder;
            }
            Seat::InOrder if at < self.after => self.after += count,
            Seat::Next if at < self.after || (at == self.after && before_the_queued) => {
                self.after += count;
            }
            Seat::InOrder | Seat::Next => {}
        }
    }

    fn hand_on(&mut self) {
        if !self.next.is_empty() {
            self.seat = Seat::Next;
        } else if self.after < self.order.len() {
            self.after += 1;
            self.seat = Seat::InOrder;
        } else if self.repeat == RepeatMode::Queue && !self.order.is_empty() {
            self.wrap_to_the_start();
        } else {
            self.seat = Seat::Nowhere;
        }
    }

    fn wrap_to_the_start(&mut self) {
        if self.shuffle {
            self.reshuffle_after_a_pass();
        }
        self.after = 1;
        self.seat = Seat::InOrder;
    }

    fn lift(&mut self, rows: Span) -> Lifted {
        let passed = self.after.min(self.order.len());
        let queued = self.next.len();
        let end = self.order.len() + queued;
        let (first, past) = (rows.first(), rows.last() + 1);
        let within = |from: usize, to: usize, offset: usize| {
            first.clamp(from, to) - offset..past.clamp(from, to) - offset
        };

        let upcoming = self.order.slice(within(passed + queued, end, queued));
        let lifted_passed = self.order.slice(within(0, passed, 0));
        let lifted_queued = self.next.slice(within(passed, passed + queued, passed));

        Lifted {
            kept_passed: passed - lifted_passed.len(),
            passed: lifted_passed,
            queued: lifted_queued,
            upcoming,
        }
    }

    fn waits_beside(&self, landing: usize, lifted: &Lifted, heard: Option<usize>) -> Waits {
        let Some(before) = landing.checked_sub(1) else {
            return Waits::InOrder;
        };
        let kept_queued = self.next.len();
        let held = if before < lifted.kept_passed {
            self.order.get(before)
        } else if before < lifted.kept_passed + kept_queued {
            return Waits::Next;
        } else {
            self.order.get(before - kept_queued)
        };

        if held.is_some_and(|held| Some(held.place) == heard) {
            Waits::Next
        } else {
            Waits::InOrder
        }
    }

    fn split(&mut self, drawn: Flagged, heard: Option<usize>) {
        let mut order = Vector::new();
        let mut next = Vector::new();
        let mut after = None;

        for (held, waits) in drawn {
            if Some(held.place) == heard {
                match self.seat {
                    Seat::Next => {
                        after = Some(order.len());
                        next.push_front(held);
                    }
                    Seat::Nowhere | Seat::InOrder => {
                        order.push_back(held);
                        after = Some(order.len());
                    }
                }
            } else if waits == Waits::Next {
                next.push_back(held);
            } else {
                order.push_back(held);
            }
        }

        if self.shuffle {
            self.unshuffle_alongside(&order, &next);
        }
        self.after = after.unwrap_or(order.len());
        self.order = order;
        self.next = next;
        self.revision = self.revision.wrapping_add(1);
        self.playing_from = stamped(&self.order);
    }

    fn unshuffle_alongside(&mut self, order: &Vector<Held>, next: &Vector<Held>) {
        for held in next {
            self.unshuffled.remove(held.place);
        }
        self.unshuffled_in(order, 0..order.len());
    }

    fn unshuffle_moved(
        &mut self,
        order_at: usize,
        into_order: &Vector<Held>,
        into_next: &Vector<Held>,
    ) {
        for held in into_next {
            self.unshuffled.remove(held.place);
        }
        let order = self.order.clone();
        self.unshuffled_in(&order, order_at..order_at + into_order.len());
    }

    fn unshuffled_in(&mut self, order: &Vector<Held>, rows: Range<usize>) {
        let last = order.len().checked_sub(1);
        let mut absent = Vector::new();
        let mut anchor = Anchor::Front;

        for at in rows {
            let Some(held) = order.get(at) else { break };
            if self.unshuffled.contains(held.place) {
                self.unshuffled.insert(anchor, &absent);
                absent.clear();
                continue;
            }
            let after_before = match at.checked_sub(1).and_then(|before| order.get(before)) {
                None => Anchor::Front,
                Some(before) => Anchor::After(before.place),
            };
            if Some(at) == last {
                self.unshuffled.insert(anchor, &absent);
                absent.clear();
                let landing = if at == 0 { Anchor::Front } else { Anchor::Back };
                self.unshuffled.insert(landing, &Vector::unit(held.clone()));
                continue;
            }
            if absent.is_empty() {
                anchor = after_before;
            }
            absent.push_back(held.clone());
        }
        self.unshuffled.insert(anchor, &absent);
    }

    fn let_go_of_what_was_heard(&mut self) -> Option<usize> {
        let heard = self.next.pop_front()?;
        self.left(&heard, Waits::Next);
        self.rows_moved();
        Some(heard.place)
    }

    fn seat_after(&mut self, after: usize) {
        if after != self.after && !self.next.is_empty() {
            self.revision = self.revision.wrapping_add(1);
        }
        self.after = after;
    }

    fn waiting(&self) -> Vector<Held> {
        match self.seat {
            Seat::Next => self.next.skip(1),
            Seat::Nowhere | Seat::InOrder => self.next.clone(),
        }
    }

    fn heard(&self) -> Option<&Held> {
        match self.seat {
            Seat::Nowhere => None,
            Seat::InOrder => self.last_heard(),
            Seat::Next => self.next.front(),
        }
    }

    fn heard_place(&self) -> Option<usize> {
        self.heard().map(|held| held.place)
    }

    fn last_heard(&self) -> Option<&Held> {
        self.order.get(self.after.checked_sub(1)?)
    }

    fn flagged(&self) -> Flagged {
        let after = self.after.min(self.order.len());
        self.order
            .iter()
            .take(after)
            .map(|held| (held.clone(), Waits::InOrder))
            .chain(self.next.iter().map(|held| (held.clone(), Waits::Next)))
            .chain(
                self.order
                    .iter()
                    .skip(after)
                    .map(|held| (held.clone(), Waits::InOrder)),
            )
            .collect()
    }

    fn row(&self, drawn: usize) -> Option<Row> {
        if drawn < self.after {
            return Some(Row::InOrder(drawn));
        }
        let past = drawn - self.after;
        if past < self.next.len() {
            return Some(Row::Next(past));
        }
        let at = drawn - self.next.len();
        (at < self.order.len()).then_some(Row::InOrder(at))
    }

    fn drawn_at(&self, row: Row) -> usize {
        match row {
            Row::InOrder(at) if at < self.after => at,
            Row::InOrder(at) => at + self.next.len(),
            Row::Next(at) => self.after + at,
        }
    }

    fn unshuffled_anchor(&self, at: usize) -> Anchor {
        let Some(before) = at.checked_sub(1) else {
            return Anchor::Front;
        };
        if at >= self.order.len() {
            return Anchor::Back;
        }
        self.order
            .get(before)
            .map_or(Anchor::Back, |before| Anchor::After(before.place))
    }

    fn standing_of(&self, order: &Vector<Held>) -> Standing {
        if self.shuffle {
            Standing::of(order.iter().cloned())
        } else {
            Standing::default()
        }
    }

    fn rows_moved(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.rows_revision = self.rows_revision.wrapping_add(1);
    }

    fn stamp_afresh(&mut self) {
        self.playing_from = stamped(&self.order);
        self.stamp = self
            .next
            .iter()
            .fold(self.playing_from, |stamp, held| stamp.with(held.stamp));
    }

    fn reshuffle_keeping(&mut self, playing: usize) {
        self.reshuffle();
        if let Some(at) = self.order.iter().position(|held| held.place == playing) {
            self.order.swap(0, at);
        }
        self.after = 1;
    }

    fn reshuffle_after_a_pass(&mut self) {
        let last_heard = self.order.back().map(|held| held.place);
        self.reshuffle();

        let later = self.order.len().saturating_sub(1);
        if later == 0 || self.order.front().map(|held| held.place) != last_heard {
            return;
        }
        let elsewhere = 1 + self.shuffler.next_below(later);
        self.order.swap(0, elsewhere);
    }

    fn reshuffle(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        let mut remaining = self.order.len();
        while remaining > 1 {
            let pick = self.shuffler.next_below(remaining);
            remaining -= 1;
            self.order.swap(pick, remaining);
        }
    }
}

impl Default for Queue {
    fn default() -> Self {
        Self::new()
    }
}

const fn row_waits(row: Row) -> Waits {
    match row {
        Row::InOrder(_) => Waits::InOrder,
        Row::Next(_) => Waits::Next,
    }
}

fn stamped(rows: &Vector<Held>) -> QueueStamp {
    rows.iter()
        .fold(QueueStamp::default(), |stamp, held| stamp.with(held.stamp))
}

fn relocated_in(rows: &mut Vector<Held>, moved: &[(MediaLocation, MediaLocation)]) -> bool {
    let mut relocated = false;
    for held in rows.iter_mut() {
        if let Some(to) = landed_at(&held.item.location, moved) {
            Arc::make_mut(&mut held.item).location = to;
            relocated = true;
        }
    }
    relocated
}

pub(crate) fn landed_at(
    location: &MediaLocation,
    moved: &[(MediaLocation, MediaLocation)],
) -> Option<MediaLocation> {
    let mut at = location;
    for (from, to) in moved {
        if from == at {
            at = to;
        }
    }
    (at != location).then(|| at.clone())
}

#[cfg(test)]
mod tests {
    use resonate_core::{Frames, Resumable};

    use super::*;

    fn item(n: u64) -> QueueItem {
        QueueItem {
            id: TrackId::new(n).expect("track ids start at one"),
            location: MediaLocation::local(format!("/music/{n}.flac")),
            span: None,
        }
    }

    fn items(count: u64) -> Vec<QueueItem> {
        (1..=count).map(item).collect()
    }

    fn loaded(count: u64, start_at: usize) -> Queue {
        let mut queue = Queue::new();
        queue.load(items(count), start_at);
        queue
    }

    fn held_rows(queue: &Queue) -> Vec<Arc<QueueItem>> {
        let mut held: Vec<&Held> = queue.order.iter().chain(&queue.next).collect();
        held.sort_unstable_by_key(|held| held.place);
        held.into_iter()
            .map(|held| Arc::clone(&held.item))
            .collect()
    }

    fn loaded_at(queue: &Queue) -> Vec<usize> {
        queue.in_play_order().loaded_at().collect()
    }

    fn current(queue: &Queue) -> Option<u64> {
        queue.current().map(|item| item.id.get())
    }

    #[test]
    fn two_files_trading_names_through_a_parked_one_are_each_followed_to_where_they_landed() {
        let mut queue = loaded(2, 0);
        let first = MediaLocation::local("/music/1.flac");
        let second = MediaLocation::local("/music/2.flac");
        let parked = MediaLocation::local("/music/1.resonate-parked-7.flac");

        let relocated = queue.relocate(&[
            (first.clone(), parked.clone()),
            (second.clone(), first.clone()),
            (parked, second.clone()),
        ]);

        assert!(relocated);
        assert_eq!(held_rows(&queue)[0].location, second);
        assert_eq!(held_rows(&queue)[1].location, first);
    }

    #[test]
    fn a_move_that_comes_back_where_it_started_relocates_nothing() {
        let mut queue = loaded(1, 0);
        let first = MediaLocation::local("/music/1.flac");
        let parked = MediaLocation::local("/music/1.resonate-parked-7.flac");

        assert!(!queue.relocate(&[(first.clone(), parked.clone()), (parked, first)]));
    }

    fn kept(number: u64) -> Resumable {
        Resumable {
            location: MediaLocation::local(format!("/music/{number}.flac")),
            span: None,
            held: None,
        }
    }

    fn drawn(queue: &Queue) -> Vec<String> {
        queue
            .in_play_order()
            .iter()
            .map(|item| item.location.locator().to_string())
            .collect()
    }

    fn resumption(
        rows: Vec<Resumable>,
        order: Vec<usize>,
        row: usize,
        shuffle: bool,
    ) -> Resumption {
        Resumption {
            rows,
            order,
            row,
            at: Frames::ZERO,
            shuffle,
            next: None,
        }
    }

    #[test]
    fn loading_starts_at_the_requested_item() {
        let queue = loaded(5, 2);
        assert_eq!(current(&queue), Some(3));
        assert_eq!(queue.position(), Some(2));
        assert_eq!(queue.len(), 5);
    }

    #[test]
    fn an_empty_queue_has_no_current_item() {
        let queue = loaded(0, 0);
        assert_eq!(queue.len(), 0);
        assert_eq!(current(&queue), None);
        assert_eq!(queue.position(), None);
    }

    #[test]
    fn a_start_index_past_the_end_lands_on_the_last_item() {
        let queue = loaded(3, 99);
        assert_eq!(queue.position(), Some(2));
    }

    #[test]
    fn repeat_off_runs_off_the_end() {
        let mut queue = loaded(3, 0);
        assert_eq!(queue.advance(true), Some(1));
        assert_eq!(queue.advance(true), Some(2));
        assert_eq!(queue.advance(true), None);
        assert_eq!(current(&queue), None);
    }

    #[test]
    fn repeat_queue_wraps_to_the_start() {
        let mut queue = loaded(3, 2);
        queue.set_repeat(RepeatMode::Queue);
        assert_eq!(queue.advance(true), Some(0));
    }

    #[test]
    fn repeat_track_holds_a_track_that_ended_but_yields_to_an_explicit_next() {
        let mut queue = loaded(3, 0);
        queue.set_repeat(RepeatMode::Track);

        assert_eq!(queue.advance(true), Some(0));
        assert_eq!(queue.advance(false), Some(1));
    }

    #[test]
    fn previous_stops_at_the_first_item_unless_the_queue_repeats() {
        let mut queue = loaded(3, 0);
        assert_eq!(queue.retreat(), Some(0));

        queue.set_repeat(RepeatMode::Queue);
        assert_eq!(queue.retreat(), Some(2));
    }

    #[test]
    fn shuffling_keeps_playing_the_track_that_was_playing() {
        let mut queue = loaded(64, 20);
        queue.set_shuffle(true);

        assert_eq!(queue.position(), Some(20));
        assert!(queue.shuffle());
    }

    #[test]
    fn unshuffling_restores_the_load_order_from_wherever_it_stopped() {
        let mut queue = loaded(64, 0);
        queue.set_shuffle(true);
        let playing = queue.advance(true).expect("64 items outlast one advance");

        queue.set_shuffle(false);
        assert_eq!(queue.position(), Some(playing));
        assert_eq!(
            queue.advance(true),
            (playing + 1 < 64).then_some(playing + 1)
        );
    }

    fn numbered(queue: &Queue) -> Vec<u64> {
        queue
            .in_play_order()
            .iter()
            .map(|item| item.id.get())
            .collect()
    }

    #[test]
    fn a_hand_order_made_unshuffled_comes_back_when_shuffling_is_turned_off() {
        let mut queue = loaded(8, 0);
        queue
            .move_rows(Span::one(7), 0)
            .expect("the last row moves to the front");
        queue.reorder(&[1, 0, 2, 3, 4, 5, 6, 7]).expect("a swap");
        let by_hand = numbered(&queue);
        assert_eq!(by_hand, [1, 8, 2, 3, 4, 5, 6, 7]);

        queue.set_shuffle(true);
        queue.set_shuffle(false);

        assert_eq!(numbered(&queue), by_hand);
        assert_eq!(current(&queue), Some(1));
        assert_eq!(queue.cursor(), Some(0));
    }

    #[test]
    fn a_row_played_next_under_shuffle_follows_the_playing_row_once_unshuffled() {
        let mut queue = loaded(32, 0);
        queue.set_shuffle(true);
        queue.advance(true).expect("32 rows outlast one advance");
        let playing = current(&queue).expect("a row is playing");

        queue.insert(vec![item(90)], Placement::Next);
        queue.insert(vec![item(91)], Placement::Queued);
        queue.set_shuffle(false);

        let order = numbered(&queue);
        let at = order
            .iter()
            .position(|id| *id == playing)
            .expect("the playing row is still queued");
        assert_eq!(order.get(at + 1), Some(&90));
        assert_eq!(order.get(at + 2), Some(&91));
        assert_eq!(current(&queue), Some(playing));
    }

    fn listed(queue: &Queue) -> Vec<u64> {
        queue
            .published()
            .in_list_order()
            .iter()
            .map(|item| item.id.get())
            .collect()
    }

    #[test]
    fn the_list_a_shuffle_leaves_is_the_order_the_rows_stand_in_unshuffled() {
        let mut queue = loaded(32, 4);
        let before = listed(&queue);
        let rows_were = queue.published().rows_revision;
        let drawn_was = queue.revision();
        assert_eq!(before, numbered(&queue));

        queue.set_shuffle(true);
        assert_ne!(
            numbered(&queue),
            before,
            "thirty-two rows shuffled into place"
        );
        assert_eq!(listed(&queue), before, "a shuffle moved the list");
        assert_eq!(queue.published().rows_revision, rows_were);
        assert_ne!(queue.revision(), drawn_was);

        queue.advance(true).expect("32 rows outlast one advance");
        let playing = current(&queue).expect("a row is playing");
        queue.insert(vec![item(90)], Placement::Next);
        let list = listed(&queue);
        let at = list
            .iter()
            .position(|id| *id == playing)
            .expect("the playing row is listed");
        assert_eq!(list.get(at + 1), Some(&90), "{list:?}");
        assert_ne!(queue.published().rows_revision, rows_were);
        assert_eq!(list.len(), 33);

        queue.set_shuffle(false);
        assert_eq!(listed(&queue), numbered(&queue));
        assert_eq!(listed(&queue), list, "unshuffling moved the list");
    }

    #[test]
    fn a_row_removed_under_shuffle_stays_gone_once_unshuffled() {
        let mut queue = loaded(16, 0);
        queue.set_shuffle(true);
        let dropped = numbered(&queue)[3];

        queue
            .remove_rows(Span::one(3))
            .expect("a row inside the queue");
        queue.set_shuffle(false);

        let order = numbered(&queue);
        assert_eq!(order.len(), 15);
        assert!(!order.contains(&dropped));
        let mut sorted = order.clone();
        sorted.sort_unstable();
        assert_eq!(
            order, sorted,
            "unshuffling lost the load order around a removal"
        );
    }

    #[test]
    fn a_shuffled_pass_visits_every_item_exactly_once() {
        let mut queue = loaded(32, 0);
        queue.set_shuffle(true);

        let mut seen = vec![queue.position().expect("the queue is not empty")];
        while let Some(position) = queue.advance(true) {
            seen.push(position);
        }

        seen.sort_unstable();
        assert_eq!(seen, (0..32).collect::<Vec<_>>());
    }

    #[test]
    fn a_new_shuffled_pass_never_opens_on_the_row_the_last_one_ended_on() {
        let mut queue = loaded(2, 0);
        queue.set_shuffle(true);
        queue.set_repeat(RepeatMode::Queue);

        let mut heard = vec![queue.position().expect("the queue is not empty")];
        for _ in 0..256 {
            heard.push(
                queue
                    .advance(true)
                    .expect("a repeating queue never runs out"),
            );
        }

        assert!(
            heard.windows(2).all(|pair| pair[0] != pair[1]),
            "a row was heard twice in a row across a pass: {heard:?}"
        );
    }

    #[test]
    fn shuffling_reorders_a_queue_long_enough_for_that_to_be_certain() {
        let mut queue = loaded(256, 0);
        queue.set_shuffle(true);

        let mut order = vec![queue.position().expect("the queue is not empty")];
        while let Some(position) = queue.advance(true) {
            order.push(position);
        }

        assert_ne!(order, (0..256).collect::<Vec<_>>());
    }

    #[test]
    fn jumping_selects_the_row_the_queue_pane_shows_not_the_load_order() {
        let mut queue = loaded(16, 0);
        queue.set_shuffle(true);

        let shown = queue.in_play_order();
        assert_eq!(
            queue.jump_to(9),
            shown.get(9).map(|item| item.id.get() as usize - 1)
        );
        assert_eq!(queue.cursor(), Some(9));
        assert_eq!(current(&queue), shown.get(9).map(|item| item.id.get()));
        assert_eq!(queue.jump_to(16), None, "a row past the end was accepted");
    }

    #[test]
    fn the_play_order_holds_every_item_once_however_it_is_shuffled() {
        let mut queue = loaded(16, 0);
        let loaded_order: Vec<u64> = queue
            .in_play_order()
            .iter()
            .map(|item| item.id.get())
            .collect();
        assert_eq!(loaded_order, (1..=16).collect::<Vec<_>>());

        queue.set_shuffle(true);
        let mut shuffled: Vec<u64> = queue
            .in_play_order()
            .iter()
            .map(|item| item.id.get())
            .collect();
        assert_eq!(shuffled.len(), 16);
        shuffled.sort_unstable();
        assert_eq!(shuffled, (1..=16).collect::<Vec<_>>());
    }

    #[test]
    fn the_revision_moves_only_when_the_order_a_pane_would_draw_changes() {
        let mut queue = loaded(8, 0);
        let loaded_at = queue.revision();

        queue.set_repeat(RepeatMode::Queue);
        assert_eq!(queue.revision(), loaded_at, "repeat reordered nothing");
        queue.advance(true);
        assert_eq!(queue.revision(), loaded_at, "advancing reordered nothing");

        queue.set_shuffle(true);
        assert_ne!(
            queue.revision(),
            loaded_at,
            "shuffling left the order alone"
        );

        let shuffled_at = queue.revision();
        queue.set_shuffle(true);
        assert_eq!(queue.revision(), shuffled_at, "shuffling twice reshuffled");
        queue.set_shuffle(false);
        assert_ne!(
            queue.revision(),
            shuffled_at,
            "unshuffling left the order alone"
        );
    }

    #[test]
    fn inserting_puts_a_row_where_it_is_told_and_keeps_the_one_playing() {
        let mut queue = loaded(4, 1);
        assert_eq!(queue.insert(vec![item(90)], Placement::At(0)), Some(0));

        assert_eq!(queue.len(), 5);
        assert_eq!(queue.cursor(), Some(2), "the playing row did not move down");
        assert_eq!(current(&queue), Some(2));
        assert_eq!(
            queue.in_play_order().first().map(|item| item.id.get()),
            Some(90)
        );
    }

    #[test]
    fn playing_next_lands_the_row_after_the_one_playing() {
        let mut queue = loaded(4, 1);
        assert_eq!(queue.insert(vec![item(90)], Placement::Next), Some(2));

        assert_eq!(queue.cursor(), Some(1), "the playing row moved");
        assert_eq!(current(&queue), Some(2));
        assert_eq!(queue.advance(false), Some(4));
        assert_eq!(current(&queue), Some(90));
    }

    #[test]
    fn playing_next_under_shuffle_lands_after_the_row_the_pane_drew() {
        let mut queue = loaded(16, 0);
        queue.set_shuffle(true);
        let playing = current(&queue);

        assert_eq!(queue.insert(vec![item(90)], Placement::Next), Some(1));
        assert_eq!(current(&queue), playing, "the playing row was displaced");
        assert_eq!(
            queue.in_play_order().get(1).map(|item| item.id.get()),
            Some(90)
        );
    }

    #[test]
    fn playing_next_with_nothing_playing_lands_at_the_end() {
        let mut queue = loaded(3, 2);
        while queue.advance(true).is_some() {}
        assert_eq!(queue.cursor(), None);

        assert_eq!(queue.insert(vec![item(90)], Placement::Next), Some(3));
        assert_eq!(queue.cursor(), Some(3), "the queue did not resume on it");
        assert_eq!(current(&queue), Some(90));
    }

    #[test]
    fn a_row_asked_for_past_the_end_lands_after_the_queued_rows_while_one_of_them_plays() {
        let mut queue = loaded(1, 0);
        queue.insert(vec![item(91), item(92)], Placement::Queued);
        queue.advance(false);
        assert_eq!(current(&queue), Some(91));

        assert_eq!(queue.insert(vec![item(90)], Placement::At(10)), Some(3));

        assert_eq!(numbered(&queue), [1, 91, 92, 90]);
        assert_eq!(current(&queue), Some(91));
    }

    #[test]
    fn a_row_asked_for_just_before_the_queued_row_playing_lands_before_it() {
        let mut queue = loaded(1, 0);
        queue.insert(vec![item(91), item(92)], Placement::Queued);
        queue.advance(false);

        assert_eq!(queue.insert(vec![item(90)], Placement::At(1)), Some(1));

        assert_eq!(numbered(&queue), [1, 90, 91, 92]);
        assert_eq!(current(&queue), Some(91));
    }

    #[test]
    fn a_row_asked_for_past_the_end_lands_at_the_end() {
        let mut queue = loaded(3, 0);
        assert_eq!(queue.insert(vec![item(90)], Placement::At(99)), Some(3));
        assert_eq!(
            queue.in_play_order().last().map(|item| item.id.get()),
            Some(90)
        );
    }

    #[test]
    fn adding_to_the_queue_lands_before_the_rest_of_what_plays_and_leaves_the_cursor() {
        let mut queue = loaded(4, 1);
        assert_eq!(queue.insert(vec![item(90)], Placement::Queued), Some(2));

        assert_eq!(queue.cursor(), Some(1));
        assert_eq!(current(&queue), Some(2));
        assert_eq!(numbered(&queue), [1, 2, 90, 3, 4]);
    }

    #[test]
    fn inserting_into_an_empty_queue_leaves_a_row_for_play_to_start_on() {
        let mut queue = Queue::new();
        assert_eq!(queue.insert(items(2), Placement::Queued), Some(0));

        assert_eq!(queue.cursor(), Some(0));
        assert_eq!(current(&queue), Some(1));
        assert_eq!(
            queue.insert(Vec::new(), Placement::Queued),
            None,
            "nothing was inserted"
        );
    }

    #[test]
    fn removing_a_row_before_the_playing_one_keeps_the_track_playing() {
        let mut queue = loaded(4, 2);
        assert_eq!(queue.remove_rows(Span::one(0)), Some(Removal::Queued));

        assert_eq!(queue.len(), 3);
        assert_eq!(queue.cursor(), Some(1));
        assert_eq!(current(&queue), Some(3));
    }

    #[test]
    fn removing_the_playing_row_hands_the_position_to_the_row_that_slides_in() {
        let mut queue = loaded(4, 1);
        assert_eq!(queue.remove_rows(Span::one(1)), Some(Removal::Playing));

        assert_eq!(queue.cursor(), Some(1));
        assert_eq!(current(&queue), Some(3));
    }

    #[test]
    fn removing_the_playing_row_at_the_end_runs_the_queue_off_it() {
        let mut queue = loaded(3, 2);
        assert_eq!(queue.remove_rows(Span::one(2)), Some(Removal::Playing));

        assert_eq!(queue.cursor(), None);
        assert_eq!(current(&queue), None);
        assert_eq!(queue.len(), 2);
        assert_eq!(
            queue.remove_rows(Span::one(2)),
            None,
            "a row past the end was removed"
        );
    }

    #[test]
    fn removing_the_playing_row_at_the_end_wraps_to_the_start_when_the_queue_repeats() {
        let mut queue = loaded(3, 2);
        queue.set_repeat(RepeatMode::Queue);

        assert_eq!(queue.remove_rows(Span::one(2)), Some(Removal::Playing));

        assert_eq!(queue.cursor(), Some(0));
        assert_eq!(current(&queue), Some(1));
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn removing_the_only_row_under_repeat_queue_leaves_nothing_playing() {
        let mut queue = loaded(1, 0);
        queue.set_repeat(RepeatMode::Queue);

        assert_eq!(queue.remove_rows(Span::one(0)), Some(Removal::Playing));

        assert_eq!(queue.cursor(), None);
        assert_eq!(current(&queue), None);
    }

    #[test]
    fn removing_a_shuffled_row_drops_that_row_and_no_other() {
        let mut queue = loaded(16, 0);
        queue.set_shuffle(true);
        let shown = queue.in_play_order();
        let dropped = shown.get(5).expect("sixteen rows outlast one removal").id;
        let playing = current(&queue);

        queue.remove_rows(Span::one(5));
        let left = queue.in_play_order();

        assert_eq!(left.len(), 15);
        assert!(!left.iter().any(|item| item.id == dropped));
        assert_eq!(current(&queue), playing);
        for (row, item) in left.iter().enumerate() {
            let expected = shown.get(if row < 5 { row } else { row + 1 });
            assert_eq!(Some(item), expected, "row {row} came out reordered");
        }
    }

    #[test]
    fn removing_a_span_drops_every_row_in_it_and_no_other() {
        let mut queue = loaded(6, 0);
        assert_eq!(
            queue.remove_rows(Span::between(1, 3)),
            Some(Removal::Queued)
        );

        let left: Vec<u64> = queue
            .in_play_order()
            .iter()
            .map(|item| item.id.get())
            .collect();
        assert_eq!(left, vec![1, 5, 6]);
        assert_eq!(queue.len(), 3);
        assert_eq!(queue.cursor(), Some(0));
        assert_eq!(current(&queue), Some(1));
    }

    #[test]
    fn removing_a_span_before_the_playing_row_pulls_the_track_back_by_its_length() {
        let mut queue = loaded(6, 4);
        assert_eq!(
            queue.remove_rows(Span::between(0, 2)),
            Some(Removal::Queued)
        );

        assert_eq!(queue.cursor(), Some(1));
        assert_eq!(current(&queue), Some(5));
    }

    #[test]
    fn removing_a_span_holding_the_playing_row_hands_the_position_on() {
        let mut queue = loaded(6, 2);
        assert_eq!(
            queue.remove_rows(Span::between(1, 3)),
            Some(Removal::Playing)
        );

        assert_eq!(queue.cursor(), Some(1));
        assert_eq!(current(&queue), Some(5));
    }

    #[test]
    fn a_span_running_past_the_end_removes_nothing() {
        let mut queue = loaded(3, 0);
        assert_eq!(queue.remove_rows(Span::between(2, 3)), None);
        assert_eq!(queue.len(), 3);
    }

    #[test]
    fn two_runs_removed_together_leave_the_rows_between_them_and_a_stray_run_removes_nothing() {
        let mut queue = loaded(6, 0);
        assert_eq!(
            queue.remove_all(&[Span::one(1), Span::between(3, 4)]),
            Ok(Removal::Queued)
        );

        assert_eq!(queue.len(), 3);
        let left: Vec<u64> = queue
            .in_play_order()
            .iter()
            .map(|item| item.id.get())
            .collect();
        assert_eq!(left, [1, 3, 6]);

        assert_eq!(
            queue.remove_all(&[Span::one(0), Span::one(3)]),
            Err(Span::one(3))
        );
        assert_eq!(queue.len(), 3);
    }

    #[test]
    fn removing_a_shuffled_span_drops_the_rows_the_pane_drew() {
        let mut queue = loaded(16, 0);
        queue.set_shuffle(true);
        let shown = queue.in_play_order();

        queue.remove_rows(Span::between(5, 7));
        let left = queue.in_play_order();

        assert_eq!(left.len(), 13);
        for (row, item) in left.iter().enumerate() {
            let expected = shown.get(if row < 5 { row } else { row + 3 });
            assert_eq!(Some(item), expected, "row {row} came out reordered");
        }
    }

    #[test]
    fn moving_a_span_down_ends_it_on_the_row_it_landed_on() {
        let mut queue = loaded(6, 0);
        assert_eq!(queue.move_rows(Span::between(0, 1), 4), Some(()));

        let shown: Vec<u64> = queue
            .in_play_order()
            .iter()
            .map(|item| item.id.get())
            .collect();
        assert_eq!(shown, vec![3, 4, 5, 1, 2, 6]);
    }

    #[test]
    fn moving_a_span_up_starts_it_on_the_row_it_landed_on() {
        let mut queue = loaded(6, 0);
        assert_eq!(queue.move_rows(Span::between(3, 4), 1), Some(()));

        let shown: Vec<u64> = queue
            .in_play_order()
            .iter()
            .map(|item| item.id.get())
            .collect();
        assert_eq!(shown, vec![1, 4, 5, 2, 3, 6]);
    }

    #[test]
    fn moving_a_span_carries_the_playing_row_inside_it() {
        let mut queue = loaded(6, 1);
        assert_eq!(queue.move_rows(Span::between(0, 2), 5), Some(()));

        assert_eq!(queue.cursor(), Some(4));
        assert_eq!(current(&queue), Some(2));
    }

    #[test]
    fn a_span_dropped_on_itself_stays_where_it_is() {
        let mut queue = loaded(6, 0);
        let held = queue.revision();

        assert_eq!(queue.move_rows(Span::between(1, 3), 2), Some(()));
        assert_eq!(
            queue.revision(),
            held,
            "a span that stayed put redrew the pane"
        );
    }

    #[test]
    fn moving_a_row_lands_it_where_it_was_told_and_leaves_the_rest_in_order() {
        let mut queue = loaded(4, 0);
        assert_eq!(queue.move_rows(Span::one(0), 2), Some(()));

        let shown: Vec<u64> = queue
            .in_play_order()
            .iter()
            .map(|item| item.id.get())
            .collect();
        assert_eq!(shown, vec![2, 3, 1, 4]);

        assert_eq!(queue.move_rows(Span::one(2), 0), Some(()));
        let back: Vec<u64> = queue
            .in_play_order()
            .iter()
            .map(|item| item.id.get())
            .collect();
        assert_eq!(back, vec![1, 2, 3, 4]);
    }

    #[test]
    fn moving_the_playing_row_carries_the_track_with_it() {
        let mut queue = loaded(4, 1);
        assert_eq!(queue.move_rows(Span::one(1), 3), Some(()));

        assert_eq!(queue.cursor(), Some(3));
        assert_eq!(current(&queue), Some(2));
        assert_eq!(queue.advance(true), None, "the moved row is not the last");
    }

    #[test]
    fn moving_a_row_over_the_playing_one_keeps_the_track_playing() {
        let mut queue = loaded(4, 2);
        assert_eq!(queue.move_rows(Span::one(0), 3), Some(()));
        assert_eq!(queue.cursor(), Some(1));
        assert_eq!(current(&queue), Some(3));

        assert_eq!(queue.move_rows(Span::one(3), 0), Some(()));
        assert_eq!(queue.cursor(), Some(2));
        assert_eq!(current(&queue), Some(3));
    }

    #[test]
    fn a_row_that_is_not_there_cannot_be_moved() {
        let mut queue = loaded(3, 0);
        assert_eq!(queue.move_rows(Span::one(0), 3), None);
        assert_eq!(queue.move_rows(Span::one(3), 0), None);
        assert_eq!(Queue::new().move_rows(Span::one(0), 0), None);

        let held = queue.revision();
        assert_eq!(queue.move_rows(Span::one(1), 1), Some(()));
        assert_eq!(
            queue.revision(),
            held,
            "a row that stayed put redrew the pane"
        );
    }

    #[test]
    fn moving_a_shuffled_row_moves_the_row_the_pane_drew_and_no_other() {
        let mut queue = loaded(16, 0);
        queue.set_shuffle(true);
        let shown = queue.in_play_order();

        queue.move_rows(Span::one(5), 11);
        let left = queue.in_play_order();

        assert_eq!(left.len(), 16);
        assert_eq!(left.get(11), shown.get(5), "the row did not land on row 11");
        for row in 0..5 {
            assert_eq!(
                left.get(row),
                shown.get(row),
                "row {row} came out reordered"
            );
        }
        for row in 5..11 {
            assert_eq!(
                left.get(row),
                shown.get(row + 1),
                "row {row} came out reordered"
            );
        }
        for row in 12..16 {
            assert_eq!(
                left.get(row),
                shown.get(row),
                "row {row} came out reordered"
            );
        }
    }

    #[test]
    fn editing_the_queue_moves_the_revision_a_pane_redraws_on() {
        let mut queue = loaded(4, 0);
        let loaded_at = queue.revision();

        queue.insert(vec![item(90)], Placement::Queued);
        let inserted_at = queue.revision();
        assert_ne!(inserted_at, loaded_at, "an insert redrew nothing");

        queue.remove_rows(Span::one(0));
        assert_ne!(queue.revision(), inserted_at, "a removal redrew nothing");
    }

    #[test]
    fn the_stamp_moves_when_the_rows_do_and_a_reordering_leaves_it() {
        let mut queue = loaded(4, 0);
        let loaded_at = queue.published().stamp;
        assert_eq!(Queue::new().published().stamp, QueueStamp::default());

        queue.move_rows(Span::one(0), 3);
        assert_eq!(
            queue.published().stamp,
            loaded_at,
            "a move took rows out of the queue"
        );

        queue.set_shuffle(true);
        assert_eq!(
            queue.published().stamp,
            loaded_at,
            "shuffling took rows out of the queue"
        );

        queue.insert(vec![item(90)], Placement::Queued);
        let inserted_at = queue.published().stamp;
        assert_ne!(inserted_at, loaded_at, "an arriving row stamped the same");

        let arrived = numbered(&queue)
            .iter()
            .position(|id| *id == 90)
            .expect("the queued row is drawn");
        queue.remove_rows(Span::one(arrived));
        assert_eq!(
            queue.published().stamp,
            loaded_at,
            "the rows it was loaded with stamp what they did"
        );
    }

    #[test]
    fn one_file_queued_twice_is_two_rows_under_two_ids() {
        let twice = vec![item(7), item(7)];
        let mut queue = Queue::new();
        queue.load(twice, 0);

        let rows = queue.in_play_order();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].location, rows[1].location);
        assert_ne!(
            rows[0].id, rows[1].id,
            "the same file queued twice answers to one id"
        );
        assert_eq!(rows[0].id, item(7).id, "the row that was there was renamed");
    }

    #[test]
    fn a_row_arriving_under_an_id_the_queue_holds_is_given_one_of_its_own() {
        let mut queue = loaded(4, 0);
        let held: Vec<TrackId> = queue.in_play_order().iter().map(|item| item.id).collect();

        queue.insert(vec![item(2)], Placement::Queued);
        let rows = queue.in_play_order();
        let arrived = rows.get(1).expect("the row arrived after the one playing");

        assert_eq!(arrived.location, item(2).location);
        assert!(
            !held.contains(&arrived.id),
            "an arriving row took an id the queue was already using"
        );
    }

    #[test]
    fn a_queue_stamps_by_the_rows_it_holds_rather_than_the_ids_they_arrived_under() {
        let mut queue = Queue::new();
        queue.load(items(3), 0);
        let loaded_at = queue.published().stamp;

        let renamed: Vec<QueueItem> = items(3)
            .into_iter()
            .enumerate()
            .map(|(place, row)| QueueItem {
                id: TrackId::new(u64::MAX - place as u64).expect("track ids start at one"),
                ..row
            })
            .collect();
        let mut again = Queue::new();
        again.load(renamed, 0);

        assert_eq!(
            again.published().stamp,
            loaded_at,
            "the same files under other ids stamped as another queue"
        );
    }

    #[test]
    fn a_batch_of_rows_all_claiming_one_id_is_given_an_id_each() {
        let mut queue = Queue::new();
        let batch: Vec<QueueItem> = (0..50_000).map(|_| item(1)).collect();

        queue.insert(batch, Placement::Queued);

        let ids: AHashSet<u64> = held_rows(&queue).iter().map(|item| item.id.get()).collect();
        assert_eq!(ids.len(), 50_000);
        assert!(ids.contains(&1));
    }

    #[test]
    fn an_id_for_a_file_outside_the_library_never_collides_with_the_queue() {
        assert_eq!(unclaimed_id(&[]), TrackId::MAX);

        let mut queue = items(4);
        queue.push(QueueItem {
            id: TrackId::MAX,
            location: MediaLocation::local("/music/opened.flac"),
            span: None,
        });

        let minted = unclaimed_id(&queue);
        assert_eq!(minted.get(), u64::MAX - 1);
        assert!(!queue.iter().any(|item| item.id == minted));

        queue.push(QueueItem {
            id: minted,
            location: MediaLocation::local("/music/opened-again.flac"),
            span: None,
        });
        assert_eq!(unclaimed_id(&queue).get(), u64::MAX - 2);
    }

    #[test]
    fn a_run_of_ids_is_minted_from_one_walk_over_what_the_queue_holds() {
        let mut queue = items(4);
        queue.push(QueueItem {
            id: TrackId::MAX,
            location: MediaLocation::local("/music/opened.flac"),
            span: None,
        });

        let mut minting = Unclaimed::beside(&queue);
        let minted: Vec<u64> = (0..3).map(|_| minting.mint().get()).collect();

        assert_eq!(minted, vec![u64::MAX - 1, u64::MAX - 2, u64::MAX - 3]);
        assert!(
            !queue
                .iter()
                .any(|item| minted.contains(&item.id.get()) && item.id != TrackId::MAX)
        );
    }

    #[test]
    fn what_a_drawn_row_says_it_was_loaded_at_names_the_row_the_queue_holds() {
        let mut queue = loaded(8, 0);
        assert_eq!(loaded_at(&queue), (0..8).collect::<Vec<_>>());

        queue.set_shuffle(true);
        let shown = queue.in_play_order();
        for (row, loaded_at) in loaded_at(&queue).into_iter().enumerate() {
            assert_eq!(
                shown[row].id.get() as usize,
                loaded_at + 1,
                "row {row} was loaded somewhere else"
            );
        }
    }

    #[test]
    fn a_restored_queue_plays_the_order_it_was_kept_in_from_the_row_it_was_left_on() {
        let mut queue = Queue::new();
        let rows = vec![kept(1), kept(2), kept(3), kept(4)];

        assert_eq!(
            queue.restore(resumption(rows, vec![2, 0, 3, 1], 2, true)),
            Some(3)
        );
        assert_eq!(
            drawn(&queue),
            vec![
                "/music/3.flac",
                "/music/1.flac",
                "/music/4.flac",
                "/music/2.flac"
            ]
        );
        assert_eq!(queue.cursor(), Some(2));
        assert_eq!(
            queue
                .current()
                .map(|item| item.location.locator().to_string()),
            Some(String::from("/music/4.flac"))
        );
        assert!(
            queue.shuffle(),
            "a queue restored shuffled came back in order"
        );
    }

    #[test]
    fn unshuffling_a_restored_queue_gives_back_the_order_it_was_loaded_in() {
        let mut queue = Queue::new();
        let rows = vec![kept(1), kept(2), kept(3), kept(4)];
        queue.restore(resumption(rows, vec![2, 0, 3, 1], 2, true));

        queue.set_shuffle(false);

        assert_eq!(
            drawn(&queue),
            vec![
                "/music/1.flac",
                "/music/2.flac",
                "/music/3.flac",
                "/music/4.flac"
            ]
        );
        assert_eq!(
            queue
                .current()
                .map(|item| item.location.locator().to_string()),
            Some(String::from("/music/4.flac")),
            "unshuffling took the track that was playing away"
        );
    }

    #[test]
    fn a_restored_row_past_the_end_lands_on_the_last_one_and_nothing_restored_plays_nothing() {
        let mut queue = Queue::new();
        queue.restore(resumption(vec![kept(1), kept(2)], vec![0, 1], 9, false));
        assert_eq!(queue.cursor(), Some(1));

        let mut emptied = loaded(3, 0);
        assert_eq!(
            emptied.restore(resumption(Vec::new(), Vec::new(), 0, false)),
            None
        );
        assert_eq!(emptied.len(), 0);
        assert_eq!(emptied.cursor(), None);
        assert_eq!(emptied.published().stamp, QueueStamp::default());
    }

    #[test]
    fn a_restored_queue_mints_an_id_for_every_row_it_takes() {
        let mut queue = Queue::new();
        queue.restore(resumption(
            vec![kept(1), kept(2), kept(3)],
            vec![0, 1, 2],
            0,
            false,
        ));

        let mut minted: Vec<u64> = queue
            .in_play_order()
            .iter()
            .map(|item| item.id.get())
            .collect();
        minted.sort_unstable();
        minted.dedup();
        assert_eq!(minted.len(), 3, "two restored rows share an id");
    }

    #[test]
    fn a_shuffled_queue_kept_and_restored_draws_the_same_rows_and_unshuffles_the_same_way() {
        let mut queue = loaded(16, 0);
        queue.set_shuffle(true);
        queue.advance(true);
        let shown = drawn(&queue);
        let order = loaded_at(&queue);
        let rows: Vec<Resumable> = held_rows(&queue)
            .iter()
            .map(|item| Resumable {
                location: item.location.clone(),
                span: item.span,
                held: None,
            })
            .collect();
        let cursor = queue.cursor().expect("sixteen rows outlast one advance");

        let mut back = Queue::new();
        back.restore(resumption(rows, order, cursor, queue.shuffle()));

        assert_eq!(drawn(&back), shown, "a kept queue came back reordered");
        assert_eq!(back.cursor(), Some(cursor));
        assert!(back.shuffle());

        back.set_shuffle(false);
        assert_eq!(
            drawn(&back),
            (1..=16)
                .map(|number| format!("/music/{number}.flac"))
                .collect::<Vec<_>>(),
            "the order the queue was loaded in did not survive the run"
        );
    }

    fn heard_in_turn(queue: &mut Queue) -> Vec<u64> {
        let mut heard: Vec<u64> = current(queue).into_iter().collect();
        while queue.advance(true).is_some() {
            heard.extend(current(queue));
        }
        heard
    }

    #[test]
    fn what_is_queued_plays_after_the_row_playing_and_before_the_rest_of_what_plays() {
        let mut queue = loaded(4, 0);
        queue.insert(vec![item(90)], Placement::Queued);
        queue.insert(vec![item(91)], Placement::Queued);
        queue.insert(vec![item(80)], Placement::Next);

        assert_eq!(numbered(&queue), [1, 80, 90, 91, 2, 3, 4]);
        assert_eq!(queue.playing_next(), Some(Span::between(1, 3)));
        assert_eq!(heard_in_turn(&mut queue), [1, 80, 90, 91, 2, 3, 4]);
    }

    #[test]
    fn a_queued_row_leaves_the_queue_once_it_has_played() {
        let mut queue = loaded(3, 0);
        queue.insert(vec![item(90)], Placement::Queued);

        queue.advance(true);
        assert_eq!(current(&queue), Some(90));
        assert_eq!(queue.cursor(), Some(1));
        assert_eq!(
            queue.position(),
            None,
            "a queued row was billed as a playlist row"
        );

        queue.advance(true);
        assert_eq!(current(&queue), Some(2));
        assert_eq!(numbered(&queue), [1, 2, 3]);
        assert_eq!(queue.playing_next(), None);
    }

    #[test]
    fn what_is_playing_from_stamps_the_same_however_much_is_queued() {
        let mut queue = loaded(3, 0);
        let playing_from = queue.playing_from();

        queue.insert(vec![item(90), item(91)], Placement::Queued);
        assert_eq!(queue.playing_from(), playing_from);
        assert_ne!(queue.published().stamp, stamp_of(&items(3)));

        queue.advance(true);
        queue.advance(true);
        queue.advance(true);
        assert_eq!(current(&queue), Some(2));
        assert_eq!(queue.playing_from(), playing_from);

        queue.insert(vec![item(92)], Placement::At(3));
        assert_ne!(
            queue.playing_from(),
            playing_from,
            "a row put into the playlist left it alone"
        );
    }

    #[test]
    fn shuffling_leaves_what_is_queued_in_the_order_it_was_queued() {
        let mut queue = loaded(64, 0);
        queue.insert(vec![item(90), item(91), item(92)], Placement::Queued);

        queue.set_shuffle(true);
        assert_eq!(current(&queue), Some(1));
        assert_eq!(numbered(&queue)[1..4], [90, 91, 92]);

        queue.set_shuffle(false);
        assert_eq!(numbered(&queue)[..4], [1, 90, 91, 92]);
        assert_eq!(numbered(&queue)[4], 2);
    }

    #[test]
    fn loading_something_else_keeps_what_is_queued_to_play_next() {
        let mut queue = loaded(3, 0);
        queue.insert(vec![item(90)], Placement::Queued);
        queue.advance(true);
        queue.insert(vec![item(91)], Placement::Queued);

        queue.load(vec![item(50), item(51)], 1);

        assert_eq!(numbered(&queue), [50, 51, 91]);
        assert_eq!(current(&queue), Some(51));
        assert_eq!(heard_in_turn(&mut queue), [51, 91]);
    }

    #[test]
    fn going_back_from_a_queued_row_keeps_it_queued() {
        let mut queue = loaded(3, 1);
        queue.insert(vec![item(90)], Placement::Queued);
        queue.advance(false);
        assert_eq!(current(&queue), Some(90));

        queue.retreat();
        assert_eq!(current(&queue), Some(2));
        assert_eq!(numbered(&queue), [1, 2, 90, 3]);

        queue.retreat();
        assert_eq!(current(&queue), Some(1));
        assert_eq!(
            numbered(&queue),
            [1, 90, 2, 3],
            "the queue did not follow the row playing"
        );
    }

    #[test]
    fn choosing_a_queued_row_hears_it_and_keeps_the_rest_queued() {
        let mut queue = loaded(3, 0);
        queue.insert(vec![item(90), item(91), item(92)], Placement::Queued);

        queue.jump_to(2);
        assert_eq!(current(&queue), Some(91));
        assert_eq!(numbered(&queue), [1, 91, 90, 92, 2, 3]);

        queue.jump_to(5);
        assert_eq!(current(&queue), Some(3));
        assert_eq!(
            numbered(&queue),
            [1, 2, 3, 90, 92],
            "choosing a row dropped what was queued"
        );
    }

    #[test]
    fn a_row_dragged_among_the_queued_rows_joins_them_and_one_dragged_out_leaves() {
        let mut queue = loaded(4, 0);
        queue.insert(vec![item(90)], Placement::Queued);
        assert_eq!(numbered(&queue), [1, 90, 2, 3, 4]);

        queue
            .move_rows(Span::one(4), 1)
            .expect("the last row moves up");
        assert_eq!(numbered(&queue), [1, 4, 90, 2, 3]);
        assert_eq!(queue.playing_next(), Some(Span::between(1, 2)));

        queue
            .move_rows(Span::one(1), 4)
            .expect("the row moves back down");
        assert_eq!(numbered(&queue), [1, 90, 2, 3, 4]);
        assert_eq!(queue.playing_next(), Some(Span::one(1)));
    }

    #[test]
    fn jumping_from_one_queued_row_to_another_keeps_both_out_of_what_is_playing_from() {
        let mut queue = loaded(3, 0);
        queue.insert(vec![item(90), item(91), item(92)], Placement::Queued);
        let playing_from = queue.playing_from();
        queue.advance(false);

        queue.jump_to(3);
        assert_eq!(current(&queue), Some(92));
        assert_eq!(queue.playing_from(), playing_from);

        queue.jump_to(2);
        assert_eq!(current(&queue), Some(91));
        assert_eq!(queue.playing_from(), playing_from);
    }

    #[test]
    fn a_drag_across_the_queued_rows_moves_what_is_playing_from_and_a_drag_back_restores_it() {
        let mut queue = loaded(4, 0);
        queue.insert(vec![item(90)], Placement::Queued);
        let playing_from = queue.playing_from();

        queue
            .move_rows(Span::one(4), 1)
            .expect("an album row moves up into the queued rows");
        let joined = queue.playing_from();
        assert_ne!(
            joined, playing_from,
            "an album row left and the stamp stood"
        );

        queue
            .move_rows(Span::one(1), 4)
            .expect("the row moves back down into the album");
        assert_eq!(queue.playing_from(), playing_from);

        queue
            .move_rows(Span::one(4), 3)
            .expect("an album row moves within the album");
        assert_eq!(numbered(&queue), [1, 90, 2, 4, 3]);
        assert_eq!(queue.playing_next(), Some(Span::one(1)));
        assert_eq!(queue.playing_from(), playing_from);

        queue
            .move_rows(Span::one(1), 3)
            .expect("the queued row moves down into the album");
        assert_eq!(queue.playing_next(), None);
        assert_ne!(
            queue.playing_from(),
            playing_from,
            "a queued row joined and the stamp stood"
        );
    }

    #[test]
    fn taking_out_the_row_playing_hands_on_to_what_is_queued() {
        let mut queue = loaded(3, 0);
        queue.insert(vec![item(90)], Placement::Queued);

        assert_eq!(queue.remove_rows(Span::one(0)), Some(Removal::Playing));
        assert_eq!(current(&queue), Some(90));
        assert_eq!(queue.remove_rows(Span::one(0)), Some(Removal::Playing));
        assert_eq!(current(&queue), Some(2));
    }

    #[test]
    fn a_queue_played_to_its_end_wraps_only_once_nothing_is_queued() {
        let mut queue = loaded(2, 1);
        queue.set_repeat(RepeatMode::Queue);
        queue.insert(vec![item(90)], Placement::Queued);

        assert!(!queue.wraps_next());
        queue.advance(true);
        assert_eq!(current(&queue), Some(90));
        assert!(queue.wraps_next());
        queue.advance(true);
        assert_eq!(current(&queue), Some(1));
        assert_eq!(numbered(&queue), [1, 2]);
    }

    #[test]
    fn what_follows_rows_ahead_is_what_advancing_that_many_rows_would_find_next() {
        for repeat in [RepeatMode::Off, RepeatMode::Queue, RepeatMode::Track] {
            for queued in 0..3 {
                for start_at in 0..3 {
                    let mut queue = loaded(3, start_at);
                    queue.set_repeat(repeat);
                    queue.insert(
                        (0..queued).map(|n| item(90 + n)).collect(),
                        Placement::Queued,
                    );

                    let mut advanced = queue.clone();
                    for rows_ahead in 0..8 {
                        let said = (
                            queue
                                .follows_naturally_after(rows_ahead)
                                .map(|item| item.id.get()),
                            queue.wraps_next_after(rows_ahead),
                            queue.ends_with_the_row_after(rows_ahead),
                        );
                        let found = (
                            advanced
                                .follows_naturally_after(0)
                                .map(|item| item.id.get()),
                            advanced.wraps_next(),
                            advanced.ends_with_this_row(),
                        );
                        assert_eq!(
                            said, found,
                            "{repeat:?}, {queued} queued, from {start_at}, {rows_ahead} ahead"
                        );
                        if found.0.is_none() {
                            break;
                        }
                        advanced.advance(true);
                    }
                }
            }
        }
    }

    #[test]
    fn what_was_queued_comes_back_queued() {
        let mut queue = Queue::new();
        let rows = vec![kept(1), kept(2), kept(90), kept(3)];
        queue.restore(Resumption {
            next: Some(Span::one(2)),
            ..resumption(rows.clone(), vec![0, 1, 2, 3], 1, false)
        });

        assert_eq!(
            queue.current().map(|item| item.location.clone()),
            Some(kept(2).location)
        );
        assert_eq!(queue.playing_next(), Some(Span::one(2)));
        queue.advance(true);
        queue.advance(true);
        assert_eq!(
            drawn(&queue),
            ["/music/1.flac", "/music/2.flac", "/music/3.flac"]
        );

        let mut heard_then = Queue::new();
        heard_then.restore(Resumption {
            next: Some(Span::one(2)),
            ..resumption(rows, vec![0, 1, 2, 3], 2, false)
        });
        assert_eq!(heard_then.cursor(), Some(2));
        assert_eq!(heard_then.position(), None);
        heard_then.advance(true);
        assert_eq!(
            heard_then.len(),
            3,
            "a queued row heard before the run ended came back to stay"
        );
    }

    fn stamped_afresh(queue: &Queue) -> (QueueStamp, QueueStamp) {
        let afresh = |stamp: QueueStamp, rows: &Vector<Held>| {
            rows.iter().fold(stamp, |stamp, held| {
                stamp.with(row_stamp(&held.item, held.place))
            })
        };
        let playing_from = afresh(QueueStamp::default(), &queue.order);

        (afresh(playing_from, &queue.next), playing_from)
    }

    #[test]
    fn stamps_kept_edit_by_edit_agree_with_the_rows_stamped_afresh() {
        let mut queue = loaded(6, 1);
        let edits: [fn(&mut Queue); 12] = [
            |queue| {
                queue.insert(vec![item(90), item(91)], Placement::Next);
            },
            |queue| {
                queue.insert(vec![item(92)], Placement::Queued);
            },
            |queue| {
                queue.insert(vec![item(93)], Placement::At(5));
            },
            |queue| {
                queue.advance(false);
            },
            |queue| {
                queue.advance(false);
            },
            |queue| {
                queue.jump_to(2);
            },
            |queue| {
                queue.move_rows(Span::one(0), 4);
            },
            |queue| {
                queue.remove_rows(Span::one(1));
            },
            |queue| {
                queue.set_shuffle(true);
            },
            |queue| {
                queue.insert(vec![item(94)], Placement::Next);
            },
            |queue| {
                queue.advance(false);
            },
            |queue| {
                queue.advance(false);
            },
        ];

        for (step, edit) in edits.iter().enumerate() {
            edit(&mut queue);

            assert_eq!(
                (queue.published().stamp, queue.playing_from()),
                stamped_afresh(&queue),
                "edit {step} left a stamp the rows no longer give"
            );
        }
    }

    #[test]
    fn a_row_keeps_the_place_it_was_loaded_at_when_rows_before_it_go() {
        let mut queue = loaded(5, 3);

        queue.remove_rows(Span::one(0));
        let rows = queue.in_play_order();

        assert_eq!(current(&queue), Some(4));
        assert_eq!(queue.position(), Some(3));
        assert_eq!(rows.loaded_at().collect::<Vec<_>>(), [1, 2, 3, 4]);
        assert_eq!(rows.in_load_order(), [0, 1, 2, 3]);
    }

    #[test]
    fn a_row_published_twice_is_the_same_row_and_not_a_copy() {
        let mut queue = loaded(3, 0);
        let before = queue.in_play_order();

        queue.insert(vec![item(90)], Placement::Next);
        let after = queue.in_play_order();

        assert!(Arc::ptr_eq(&before[0], &after[0]));
        assert!(Arc::ptr_eq(&before[1], &after[2]));
        assert!(Arc::ptr_eq(&before[2], &after[3]));
    }

    #[test]
    fn an_id_a_removed_row_let_go_of_is_free_for_the_next_row_claiming_it() {
        let mut queue = loaded(3, 0);

        queue.remove_rows(Span::one(2));
        queue.insert(vec![item(3)], Placement::Queued);
        let ids: AHashSet<u64> = held_rows(&queue).iter().map(|row| row.id.get()).collect();

        assert!(ids.contains(&item(3).id.get()));
        assert_eq!(ids.len(), queue.len());
    }

    #[test]
    fn ids_stay_one_a_row_through_rows_heard_next_and_let_go() {
        let mut queue = loaded(2, 0);

        for _ in 0..4 {
            queue.insert(vec![item(1), item(1)], Placement::Next);
            queue.advance(false);
            queue.advance(false);
        }

        let ids: AHashSet<u64> = held_rows(&queue).iter().map(|row| row.id.get()).collect();
        assert_eq!(ids.len(), queue.len());
    }

    fn moved_by_rebuilding(queue: &mut Queue, rows: Span, to: usize) -> Option<()> {
        let last = queue.len().checked_sub(1)?;
        if rows.last() > last || to > last {
            return None;
        }
        if rows.holds(to) {
            return Some(());
        }

        let heard = queue.heard_place();
        let mut drawn = queue.flagged();
        let landing = rows.landing(to);
        let moved: Flagged = drawn.drain(rows.range()).collect();
        let carries_the_heard =
            heard.is_some_and(|heard| moved.iter().any(|(held, _)| held.place == heard));
        let moved = if carries_the_heard {
            moved
        } else {
            let waits = landing
                .checked_sub(1)
                .and_then(|before| drawn.get(before))
                .is_some_and(|(held, waits)| *waits == Waits::Next || Some(held.place) == heard);
            let waits = if waits { Waits::Next } else { Waits::InOrder };
            moved.into_iter().map(|(held, _)| (held, waits)).collect()
        };
        drawn.splice(landing..landing, moved);
        queue.split(drawn, heard);

        Some(())
    }

    fn places(rows: &Vector<Held>) -> Vec<usize> {
        rows.iter().map(|held| held.place).collect()
    }

    fn held_at(place: usize) -> Held {
        Held::at(place, Arc::new(item(place as u64 + 1)))
    }

    fn run_of(places: std::ops::Range<usize>) -> Vector<Held> {
        places.map(held_at).collect()
    }

    #[test]
    fn rows_inserted_at_one_spot_without_end_stand_where_a_list_would_put_them() {
        let mut standing = Standing::of((0..4).map(held_at));
        let mut model: Vec<usize> = (0..4).collect();
        let mut next = 4;

        for round in 0..300 {
            let count = 1 + round % 3;
            let run = run_of(next..next + count);
            standing.insert(Anchor::After(1), &run);
            model.splice(2..2, next..next + count);
            next += count;
        }

        assert_eq!(standing.places(), model);
    }

    #[test]
    fn rows_inserted_at_the_front_the_back_and_after_a_missing_row_stand_where_a_list_would_put_them()
     {
        let mut standing = Standing::of((0..3).map(held_at));
        let mut model: Vec<usize> = vec![0, 1, 2];

        for place in 3..200 {
            let run = run_of(place..place + 1);
            match place % 3 {
                0 => {
                    standing.insert(Anchor::Front, &run);
                    model.insert(0, place);
                }
                1 => {
                    standing.insert(Anchor::Back, &run);
                    model.push(place);
                }
                _ => {
                    standing.insert(Anchor::After(usize::MAX), &run);
                    model.push(place);
                }
            }
        }

        assert_eq!(standing.places(), model);
    }

    #[test]
    fn rows_removed_leave_the_rest_standing_in_their_order_and_found_by_place() {
        let mut standing = Standing::of((0..50).map(held_at));

        for place in (0..50).filter(|place| place % 3 == 0) {
            standing.remove(place);
        }
        standing.remove(1000);

        let kept: Vec<usize> = (0..50).filter(|place| place % 3 != 0).collect();
        assert_eq!(standing.places(), kept);
        assert!(standing.contains(1));
        assert!(!standing.contains(3));

        let (heard, to_come) = standing.around(Some(25));
        let heard: Vec<usize> = heard.map(|held| held.place).collect();
        let to_come: Vec<usize> = to_come.map(|held| held.place).collect();
        assert_eq!(
            heard,
            kept.iter()
                .copied()
                .filter(|place| *place <= 25)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            to_come,
            kept.iter()
                .copied()
                .filter(|place| *place > 25)
                .collect::<Vec<_>>()
        );
    }

    fn drag_states() -> Vec<Queue> {
        let mut states = Vec::new();

        for shuffle in [false, true] {
            for start in [0, 2, 5] {
                for queued in 0..=2u64 {
                    for heard_next in [false, true] {
                        if heard_next && queued == 0 {
                            continue;
                        }
                        let mut queue = loaded(6, start);
                        if shuffle {
                            queue.set_shuffle(true);
                        }
                        queue.insert(
                            (0..queued).map(|n| item(90 + n)).collect(),
                            Placement::Queued,
                        );
                        if heard_next {
                            queue.advance(false);
                        }
                        states.push(queue);
                    }
                }
            }
        }

        let mut ended = loaded(6, 0);
        while ended.advance(false).is_some() {}
        states.push(ended);

        states
    }

    #[test]
    fn a_drag_spliced_in_the_vectors_leaves_every_queue_the_rebuild_would() {
        for state in drag_states() {
            let len = state.len();
            for first in 0..len {
                for last in first..len {
                    for to in 0..len {
                        let rows = Span::between(first, last);
                        let mut spliced = state.clone();
                        let mut rebuilt = state.clone();

                        let answered = spliced.move_rows(rows, to);
                        let expected = moved_by_rebuilding(&mut rebuilt, rows, to);

                        let case = format!(
                            "{rows} to {to} in a queue of {len} at {:?} after {}, shuffled {}",
                            state.seat, state.after, state.shuffle
                        );
                        assert_eq!(answered, expected, "{case}");
                        assert_eq!(places(&spliced.order), places(&rebuilt.order), "{case}");
                        assert_eq!(places(&spliced.next), places(&rebuilt.next), "{case}");
                        assert_eq!(
                            spliced.unshuffled.places(),
                            rebuilt.unshuffled.places(),
                            "{case}"
                        );
                        assert_eq!(spliced.after, rebuilt.after, "{case}");
                        assert_eq!(spliced.seat, rebuilt.seat, "{case}");
                        assert_eq!(spliced.revision, rebuilt.revision, "{case}");
                        assert_eq!(spliced.stamp, rebuilt.stamp, "{case}");
                        assert_eq!(spliced.playing_from, rebuilt.playing_from, "{case}");
                        assert_eq!(spliced.heard_place(), rebuilt.heard_place(), "{case}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_span_dragged_forward_lands_after_the_rows_it_passed() {
        let mut queue = loaded(8, 0);

        queue.move_rows(Span::between(1, 2), 6);

        assert_eq!(numbered(&queue), [1, 4, 5, 6, 7, 2, 3, 8]);
        assert_eq!(current(&queue), Some(1));
        assert_eq!(stamped_afresh(&queue), (queue.stamp, queue.playing_from));
    }

    #[test]
    fn a_span_dragged_backward_lands_before_the_rows_it_passed() {
        let mut queue = loaded(8, 0);

        queue.move_rows(Span::between(5, 6), 1);

        assert_eq!(numbered(&queue), [1, 6, 7, 2, 3, 4, 5, 8]);
        assert_eq!(current(&queue), Some(1));
        assert_eq!(stamped_afresh(&queue), (queue.stamp, queue.playing_from));
    }

    #[test]
    fn a_span_dragged_across_the_playing_row_leaves_the_song_playing_and_the_cursor_on_it() {
        let mut queue = loaded(8, 3);

        queue.move_rows(Span::between(0, 1), 6);
        assert_eq!(numbered(&queue), [3, 4, 5, 6, 7, 1, 2, 8]);
        assert_eq!(current(&queue), Some(4));
        assert_eq!(queue.cursor(), Some(1));

        queue.move_rows(Span::between(5, 6), 0);
        assert_eq!(numbered(&queue), [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(current(&queue), Some(4));
        assert_eq!(queue.cursor(), Some(3));
        assert_eq!(stamped_afresh(&queue), (queue.stamp, queue.playing_from));
    }

    #[test]
    fn a_span_holding_the_playing_row_takes_the_cursor_with_it() {
        let mut queue = loaded(8, 3);

        queue.move_rows(Span::between(2, 4), 7);

        assert_eq!(numbered(&queue), [1, 2, 6, 7, 8, 3, 4, 5]);
        assert_eq!(current(&queue), Some(4));
        assert_eq!(queue.cursor(), Some(6));
    }

    #[test]
    fn a_span_dragged_among_the_rows_played_next_joins_them_and_leaves_with_a_drag_out() {
        let mut queue = loaded(6, 1);
        queue.insert(vec![item(90), item(91)], Placement::Queued);
        let playing_from = queue.playing_from();
        let stamp = queue.stamp;

        queue.move_rows(Span::between(5, 6), 3);
        assert_eq!(numbered(&queue), [1, 2, 90, 4, 5, 91, 3, 6]);
        assert_eq!(queue.playing_next(), Some(Span::between(2, 5)));
        assert_ne!(queue.playing_from(), playing_from);
        assert_eq!(queue.stamp, stamp);
        assert_eq!(stamped_afresh(&queue), (queue.stamp, queue.playing_from));

        queue.move_rows(Span::between(3, 4), 7);
        assert_eq!(numbered(&queue), [1, 2, 90, 91, 3, 6, 4, 5]);
        assert_eq!(queue.playing_next(), Some(Span::between(2, 3)));
        assert_eq!(queue.playing_from(), playing_from);
        assert_eq!(stamped_afresh(&queue), (queue.stamp, queue.playing_from));
    }

    #[test]
    fn the_row_playing_next_is_dragged_past_its_neighbours_and_stays_the_one_heard() {
        let mut queue = loaded(4, 0);
        queue.insert(vec![item(90), item(91)], Placement::Queued);
        queue.advance(false);
        assert_eq!(current(&queue), Some(90));

        queue.move_rows(Span::one(1), 4);

        assert_eq!(current(&queue), Some(90));
        assert_eq!(queue.cursor(), Some(3));
        assert_eq!(numbered(&queue), [1, 2, 3, 90, 91, 4]);
        assert_eq!(stamped_afresh(&queue), (queue.stamp, queue.playing_from));
    }

    #[test]
    fn a_shuffled_drag_inside_the_play_order_leaves_the_list_order_alone() {
        let mut queue = loaded(12, 0);
        queue.set_shuffle(true);
        let listed = queue.unshuffled.places();
        let played = places(&queue.order);

        queue.move_rows(Span::between(3, 5), 9);

        assert_eq!(queue.unshuffled.places(), listed);
        assert_ne!(places(&queue.order), played);
        let mut drawn = places(&queue.order);
        drawn.sort_unstable();
        assert_eq!(drawn, (0..12).collect::<Vec<_>>());
    }

    #[test]
    fn a_shuffled_drag_into_the_rows_played_next_takes_them_out_of_the_list_order() {
        let mut queue = loaded(8, 0);
        queue.set_shuffle(true);
        queue.insert(vec![item(90)], Placement::Queued);
        let moved = queue.order.get(4).map(|held| held.place).expect("a row");

        queue.move_rows(Span::one(5), 1);

        assert_eq!(queue.next.front().map(|held| held.place), Some(moved));
        assert!(!queue.unshuffled.places().contains(&moved));

        queue.move_rows(Span::one(1), 5);
        assert!(queue.unshuffled.places().contains(&moved));
        assert_eq!(queue.next.len(), 1);
    }
}
