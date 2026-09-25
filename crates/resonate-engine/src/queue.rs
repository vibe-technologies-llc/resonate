use std::{mem::take, ops::Range, sync::Arc};

use resonate_core::{FrameSpan, MediaLocation, QueueStamp, Resumption, Span, TrackId};

use crate::{RepeatMode, seed};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueItem {
    pub id: TrackId,
    pub location: MediaLocation,
    pub span: Option<FrameSpan>,
}

#[derive(Clone, Debug)]
pub struct Queued {
    pub revision: u64,
    pub rows: Arc<Vec<QueueItem>>,
    pub loaded_at: Arc<[usize]>,
    pub next: Option<Span>,
    pub stamp: QueueStamp,
}

impl Default for Queued {
    fn default() -> Self {
        Self {
            revision: 0,
            rows: Arc::default(),
            loaded_at: Arc::from(Vec::new()),
            next: None,
            stamp: QueueStamp::default(),
        }
    }
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

pub fn unclaimed_id(queue: &[QueueItem]) -> TrackId {
    Unclaimed::beside(queue).mint()
}

pub fn stamp_of(items: &[QueueItem]) -> QueueStamp {
    QueueStamp::of(items.iter().map(|item| (&item.location, item.span)))
}

fn one_id_each(items: Vec<QueueItem>, beside: &[QueueItem]) -> Vec<QueueItem> {
    let mut minting = Unclaimed::beside(beside);

    items
        .into_iter()
        .map(|item| QueueItem {
            id: minting.claim(item.id).unwrap_or_else(|| minting.mint()),
            ..item
        })
        .collect()
}

pub struct Unclaimed {
    taken: Vec<u64>,
    next: u64,
}

impl Unclaimed {
    pub fn beside(queue: &[QueueItem]) -> Self {
        let mut taken: Vec<u64> = queue.iter().map(|item| item.id.get()).collect();
        taken.sort_unstable();

        Self {
            taken,
            next: u64::MAX,
        }
    }

    pub fn mint(&mut self) -> TrackId {
        while self.taken.binary_search(&self.next).is_ok() {
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
        match self.taken.binary_search(&id.get()) {
            Ok(_) => None,
            Err(at) => {
                self.taken.insert(at, id.get());
                Some(id)
            }
        }
    }
}

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
enum Row {
    InOrder(usize),
    Next(usize),
}

type Flagged = Vec<(usize, bool)>;

pub struct Queue {
    items: Vec<QueueItem>,
    order: Vec<usize>,
    unshuffled: Vec<usize>,
    next: Vec<usize>,
    after: usize,
    seat: Seat,
    repeat: RepeatMode,
    shuffle: bool,
    shuffler: Shuffler,
    revision: u64,
    stamp: QueueStamp,
    playing_from: QueueStamp,
}

impl Queue {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            order: Vec::new(),
            unshuffled: Vec::new(),
            next: Vec::new(),
            after: 0,
            seat: Seat::Nowhere,
            repeat: RepeatMode::Off,
            shuffle: false,
            shuffler: Shuffler::new(),
            revision: 0,
            stamp: QueueStamp::default(),
            playing_from: QueueStamp::default(),
        }
    }

    pub fn load(&mut self, items: Vec<QueueItem>, start_at: usize) -> Option<usize> {
        let kept: Vec<QueueItem> = self
            .waiting()
            .iter()
            .filter_map(|index| self.items.get(*index))
            .cloned()
            .collect();
        let loaded = one_id_each(items, &kept);
        let count = loaded.len();

        self.items = loaded;
        self.items.extend(kept);
        self.order = (0..count).collect();
        self.unshuffled.clone_from(&self.order);
        self.next = (count..self.items.len()).collect();
        self.rows_changed();

        let Some(last) = count.checked_sub(1) else {
            self.after = 0;
            self.seat = if self.next.is_empty() {
                Seat::Nowhere
            } else {
                Seat::Next
            };
            return self.heard();
        };
        let start = start_at.min(last);

        if self.shuffle {
            self.reshuffle_keeping(start);
        } else {
            self.after = start + 1;
        }
        self.seat = Seat::InOrder;
        self.heard()
    }

    pub fn restore(&mut self, resumption: Resumption) -> Option<usize> {
        let row = resumption.landing();
        let shuffle = resumption.shuffle;
        let drawn = resumption.in_play_order();
        let next = resumption.next.filter(|next| next.last() < drawn.len());
        let mut minting = Unclaimed::beside(&[]);

        self.items = resumption
            .rows
            .into_iter()
            .map(|row| QueueItem {
                id: minting.mint(),
                location: row.location,
                span: row.span,
            })
            .collect();
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

        self.unshuffled = order.clone();
        if shuffle {
            self.unshuffled.sort_unstable();
        }
        self.order = order;
        self.next = waiting;
        self.after = after;
        self.seat = seat;
        self.rows_changed();
        self.heard()
    }

    pub fn insert(&mut self, items: Vec<QueueItem>, at: Placement) -> Option<usize> {
        if items.is_empty() {
            return None;
        }
        let first = self.items.len();
        self.items.extend(one_id_each(items, &self.items));
        let landed = first..self.items.len();

        let row = match self.landing(at) {
            Row::Next(at) => {
                self.next.splice(at..at, landed);
                Row::Next(at)
            }
            Row::InOrder(at) => {
                self.land_in_order(at, landed);
                Row::InOrder(at)
            }
        };
        self.rows_changed();
        Some(self.drawn_at(row))
    }

    pub fn remove_rows(&mut self, rows: Span) -> Option<Removal> {
        if rows.last() >= self.items.len() {
            return None;
        }

        let heard = self.heard();
        let mut dropping = vec![false; self.items.len()];
        for row in rows.range() {
            if let Some(index) = self.item_at(row) {
                dropping[index] = true;
            }
        }
        let heard_before = self.order[..self.after]
            .iter()
            .filter(|index| dropping[**index])
            .count();
        let mut reseated = Vec::with_capacity(dropping.len());
        let mut kept = 0;
        for dropped in &dropping {
            reseated.push(kept);
            kept += usize::from(!dropped);
        }
        let reseat = |held: Vec<usize>| -> Vec<usize> {
            held.into_iter()
                .filter(|index| !dropping[*index])
                .map(|index| reseated[index])
                .collect()
        };

        self.order = reseat(take(&mut self.order));
        self.unshuffled = reseat(take(&mut self.unshuffled));
        self.next = reseat(take(&mut self.next));
        self.items = take(&mut self.items)
            .into_iter()
            .zip(dropping.iter().copied())
            .filter(|(_, dropped)| !dropped)
            .map(|(item, _)| item)
            .collect();
        self.after -= heard_before;
        self.rows_changed();

        if heard.is_some_and(|index| dropping[index]) {
            self.hand_on();
            return Some(Removal::Playing);
        }
        Some(Removal::Queued)
    }

    pub fn move_rows(&mut self, rows: Span, to: usize) -> Option<()> {
        let last = self.items.len().checked_sub(1)?;
        if rows.last() > last || to > last {
            return None;
        }
        if rows.holds(to) {
            return Some(());
        }

        let heard = self.heard();
        let mut drawn = self.flagged();
        let landing = rows.landing(to);
        let moved: Flagged = drawn.drain(rows.range()).collect();
        let carries_the_heard =
            heard.is_some_and(|heard| moved.iter().any(|(index, _)| *index == heard));
        let moved = if carries_the_heard {
            moved
        } else {
            let waits = landing
                .checked_sub(1)
                .and_then(|before| drawn.get(before))
                .is_some_and(|(index, waits)| *waits || Some(*index) == heard);
            moved.into_iter().map(|(index, _)| (index, waits)).collect()
        };
        drawn.splice(landing..landing, moved);
        self.split(drawn, heard);

        Some(())
    }

    pub fn reorder(&mut self, rows: &[usize]) -> Option<()> {
        if rows.len() != self.items.len() {
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
            .copied()
            .collect();
        self.split(reordered, self.heard());

        Some(())
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub const fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    pub const fn shuffle(&self) -> bool {
        self.shuffle
    }

    pub fn position(&self) -> Option<usize> {
        match self.seat {
            Seat::InOrder => self.last_heard(),
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

    pub const fn stamp(&self) -> QueueStamp {
        self.stamp
    }

    pub const fn playing_from(&self) -> QueueStamp {
        self.playing_from
    }

    pub fn playing_next(&self) -> Option<Span> {
        let last = self.next.len().checked_sub(1)?;
        Some(Span::between(self.after, self.after + last))
    }

    pub fn in_play_order(&self) -> Vec<QueueItem> {
        self.drawn()
            .filter_map(|index| self.items.get(index))
            .cloned()
            .collect()
    }

    pub fn loaded_at(&self) -> Arc<[usize]> {
        self.drawn().collect()
    }

    pub fn current(&self) -> Option<&QueueItem> {
        self.items.get(self.heard()?)
    }

    pub fn set_repeat(&mut self, repeat: RepeatMode) {
        self.repeat = repeat;
    }

    pub fn set_shuffle(&mut self, shuffle: bool) {
        if self.shuffle == shuffle {
            return;
        }
        self.shuffle = shuffle;
        let last_heard = self.last_heard();

        if shuffle {
            match (self.seat, last_heard) {
                (Seat::InOrder | Seat::Next, Some(heard)) => self.reshuffle_keeping(heard),
                _ => self.reshuffle(),
            }
            return;
        }

        self.order.clone_from(&self.unshuffled);
        self.after = match (self.seat, last_heard) {
            (Seat::Nowhere, _) => self.order.len(),
            (_, Some(heard)) => self
                .order
                .iter()
                .position(|index| *index == heard)
                .map_or(0, |at| at + 1),
            (_, None) => 0,
        };
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn wraps_next(&self) -> bool {
        self.repeat == RepeatMode::Queue
            && self.seat != Seat::Nowhere
            && self.waiting().is_empty()
            && self.after >= self.order.len()
    }

    pub fn advance(&mut self, natural: bool) -> Option<usize> {
        if self.seat == Seat::Nowhere {
            return None;
        }
        if natural && self.repeat == RepeatMode::Track {
            return self.heard();
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
            if self.shuffle {
                self.reshuffle();
            }
            self.after = 1;
            self.seat = Seat::InOrder;
        } else {
            self.seat = Seat::Nowhere;
        }
        self.heard()
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
        self.heard()
    }

    pub fn jump_to(&mut self, at: usize) -> Option<usize> {
        match self.row(at)? {
            Row::Next(0) if self.seat == Seat::Next => {}
            Row::Next(waiting) => {
                let wanted = self.next.remove(waiting);
                let wanted = match self.seat {
                    Seat::Next => self.let_go_of_what_was_heard().map_or(wanted, |gone| {
                        if wanted > gone { wanted - 1 } else { wanted }
                    }),
                    Seat::Nowhere | Seat::InOrder => wanted,
                };
                self.next.insert(0, wanted);
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
        self.heard()
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

    fn land_in_order(&mut self, at: usize, landed: Range<usize>) {
        let count = landed.len();
        let unshuffled_at = self.unshuffled_landing(at);
        self.unshuffled
            .splice(unshuffled_at..unshuffled_at, landed.clone());
        self.order.splice(at..at, landed);

        match self.seat {
            Seat::Nowhere => {
                self.after = at + 1;
                self.seat = Seat::InOrder;
            }
            Seat::InOrder if at < self.after => self.after += count,
            Seat::Next if at <= self.after => self.after += count,
            Seat::InOrder | Seat::Next => {}
        }
    }

    fn hand_on(&mut self) {
        if !self.next.is_empty() {
            self.seat = Seat::Next;
        } else if self.after < self.order.len() {
            self.after += 1;
            self.seat = Seat::InOrder;
        } else {
            self.seat = Seat::Nowhere;
        }
    }

    fn split(&mut self, drawn: Flagged, heard: Option<usize>) {
        let mut order = Vec::with_capacity(drawn.len());
        let mut next = Vec::new();
        let mut after = None;

        for (index, waits) in drawn {
            if Some(index) == heard {
                match self.seat {
                    Seat::Next => {
                        after = Some(order.len());
                        next.insert(0, index);
                    }
                    Seat::Nowhere | Seat::InOrder => {
                        order.push(index);
                        after = Some(order.len());
                    }
                }
            } else if waits {
                next.push(index);
            } else {
                order.push(index);
            }
        }

        if self.shuffle {
            self.unshuffle_alongside(&order, &next);
        } else {
            self.unshuffled.clone_from(&order);
        }
        self.after = after.unwrap_or(order.len());
        self.order = order;
        self.next = next;
        self.revision = self.revision.wrapping_add(1);
        self.stamp_what_is_playing_from();
    }

    fn unshuffle_alongside(&mut self, order: &[usize], next: &[usize]) {
        let mut held = vec![false; self.items.len()];
        let mut waits = vec![false; self.items.len()];
        for index in next {
            waits[*index] = true;
        }
        self.unshuffled.retain(|index| !waits[*index]);
        for index in &self.unshuffled {
            held[*index] = true;
        }

        for (at, index) in order.iter().enumerate() {
            if held[*index] {
                continue;
            }
            let landing = match at.checked_sub(1) {
                None => 0,
                Some(_) if at + 1 == order.len() => self.unshuffled.len(),
                Some(before) => self
                    .unshuffled
                    .iter()
                    .position(|held| *held == order[before])
                    .map_or(self.unshuffled.len(), |held| held + 1),
            };
            self.unshuffled.insert(landing, *index);
            held[*index] = true;
        }
    }

    fn let_go_of_what_was_heard(&mut self) -> Option<usize> {
        let heard = *self.next.first()?;
        self.forget(heard);
        Some(heard)
    }

    fn forget(&mut self, gone: usize) {
        if gone >= self.items.len() {
            return;
        }
        self.items.remove(gone);
        for held in [&mut self.order, &mut self.unshuffled, &mut self.next] {
            held.retain(|index| *index != gone);
            for index in held.iter_mut() {
                if *index > gone {
                    *index -= 1;
                }
            }
        }
        self.rows_changed();
    }

    fn seat_after(&mut self, after: usize) {
        if after != self.after && !self.next.is_empty() {
            self.revision = self.revision.wrapping_add(1);
        }
        self.after = after;
    }

    fn waiting(&self) -> &[usize] {
        match self.seat {
            Seat::Next => self.next.get(1..).unwrap_or_default(),
            Seat::Nowhere | Seat::InOrder => &self.next,
        }
    }

    fn heard(&self) -> Option<usize> {
        match self.seat {
            Seat::Nowhere => None,
            Seat::InOrder => self.last_heard(),
            Seat::Next => self.next.first().copied(),
        }
    }

    fn last_heard(&self) -> Option<usize> {
        self.order.get(self.after.checked_sub(1)?).copied()
    }

    fn drawn(&self) -> impl Iterator<Item = usize> + '_ {
        let (heard, rest) = self.order.split_at(self.after.min(self.order.len()));
        heard.iter().chain(&self.next).chain(rest).copied()
    }

    fn flagged(&self) -> Flagged {
        let (heard, rest) = self.order.split_at(self.after.min(self.order.len()));
        heard
            .iter()
            .map(|index| (*index, false))
            .chain(self.next.iter().map(|index| (*index, true)))
            .chain(rest.iter().map(|index| (*index, false)))
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

    fn item_at(&self, drawn: usize) -> Option<usize> {
        match self.row(drawn)? {
            Row::InOrder(at) => self.order.get(at),
            Row::Next(at) => self.next.get(at),
        }
        .copied()
    }

    fn drawn_at(&self, row: Row) -> usize {
        match row {
            Row::InOrder(at) if at < self.after => at,
            Row::InOrder(at) => at + self.next.len(),
            Row::Next(at) => self.after + at,
        }
    }

    fn unshuffled_landing(&self, at: usize) -> usize {
        if !self.shuffle {
            return at;
        }
        let Some(before) = at.checked_sub(1) else {
            return 0;
        };
        if at >= self.order.len() {
            return self.unshuffled.len();
        }
        self.order
            .get(before)
            .and_then(|index| self.unshuffled.iter().position(|held| held == index))
            .map_or(self.unshuffled.len(), |held| held + 1)
    }

    fn rows_changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.stamp = stamp_of(&self.items);
        self.stamp_what_is_playing_from();
    }

    fn stamp_what_is_playing_from(&mut self) {
        let mut waits = vec![false; self.items.len()];
        for index in &self.next {
            waits[*index] = true;
        }
        self.playing_from = QueueStamp::of(
            self.items
                .iter()
                .zip(waits)
                .filter(|(_, waits)| !waits)
                .map(|(item, _)| (&item.location, item.span)),
        );
    }

    fn reshuffle_keeping(&mut self, playing: usize) {
        self.reshuffle();
        if let Some(at) = self.order.iter().position(|index| *index == playing) {
            self.order.swap(0, at);
        }
        self.after = 1;
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

    fn current(queue: &Queue) -> Option<u64> {
        queue.current().map(|item| item.id.get())
    }

    fn kept(number: u64) -> Resumable {
        Resumable {
            location: MediaLocation::local(format!("/music/{number}.flac")),
            span: None,
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
        let loaded_at = queue.stamp();
        assert_eq!(Queue::new().stamp(), QueueStamp::default());

        queue.move_rows(Span::one(0), 3);
        assert_eq!(
            queue.stamp(),
            loaded_at,
            "a move took rows out of the queue"
        );

        queue.set_shuffle(true);
        assert_eq!(
            queue.stamp(),
            loaded_at,
            "shuffling took rows out of the queue"
        );

        queue.insert(vec![item(90)], Placement::Queued);
        let inserted_at = queue.stamp();
        assert_ne!(inserted_at, loaded_at, "an arriving row stamped the same");

        let arrived = numbered(&queue)
            .iter()
            .position(|id| *id == 90)
            .expect("the queued row is drawn");
        queue.remove_rows(Span::one(arrived));
        assert_eq!(
            queue.stamp(),
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
        let loaded_at = queue.stamp();

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
            again.stamp(),
            loaded_at,
            "the same files under other ids stamped as another queue"
        );
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
        assert_eq!(queue.loaded_at().to_vec(), (0..8).collect::<Vec<_>>());

        queue.set_shuffle(true);
        let shown = queue.in_play_order();
        for (row, loaded_at) in queue.loaded_at().iter().copied().enumerate() {
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
        assert_eq!(emptied.stamp(), QueueStamp::default());
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
        let order = queue.loaded_at().to_vec();
        let rows: Vec<Resumable> = queue
            .items
            .iter()
            .map(|item| Resumable {
                location: item.location.clone(),
                span: item.span,
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
        assert_ne!(queue.stamp(), stamp_of(&items(3)));

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
}
