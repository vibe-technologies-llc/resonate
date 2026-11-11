use std::collections::BTreeMap;

use resonate_core::Recording;

use crate::{
    Result, Stamp,
    error::{Error, Malformed},
    shared::place::Place,
    wire::{LONGEST_TEXT, Reader, Writer, reserved},
};

pub const ENTRY_BYTES: usize = 16;
pub const MEMBER_BYTES: usize = 16;
pub const PUBLIC_BYTES: usize = 32;
pub const ROWS_AT_MOST: usize = 10_000;
pub const ENTRIES_KEPT_AT_MOST: usize = 4 * ROWS_AT_MOST;
pub const PEOPLE_KEPT_AT_MOST: usize = 1024;

pub const BUNDLE_OPS_AT_MOST: usize = 256;
pub const KEPT_OPS_AT_MOST: usize = ENTRIES_KEPT_AT_MOST + PEOPLE_KEPT_AT_MOST + 1;
const LONGEST_PLACE: usize = 4096;
const HONEST_OP_AT_MOST: usize = 5 * LONGEST_TEXT + LONGEST_PLACE + 256;
pub const LARGEST_BUNDLE_OPENED: usize = BUNDLE_OPS_AT_MOST * HONEST_OP_AT_MOST;

const LARGEST_OP: usize = 256 * 1024;
const OP_AT_LEAST: usize = 1;

const ADDED: u8 = 0;
const MOVED: u8 = 1;
const REMOVED: u8 = 2;
const RENAMED: u8 = 3;
const MEMBER: u8 = 4;
const LEFT: u8 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntryId(pub [u8; ENTRY_BYTES]);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MemberId(pub [u8; MEMBER_BYTES]);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SharedOp {
    Added {
        entry: EntryId,
        recording: Recording,
        place: Place,
        by: Option<MemberId>,
        stamp: Stamp,
    },
    Moved {
        entry: EntryId,
        place: Place,
        stamp: Stamp,
    },
    Removed {
        entry: EntryId,
        stamp: Stamp,
    },
    Renamed {
        name: String,
        stamp: Stamp,
    },
    Member {
        member: MemberId,
        name: String,
        public: [u8; PUBLIC_BYTES],
        stamp: Stamp,
    },
    Left {
        member: MemberId,
        stamp: Stamp,
    },
}

pub fn encoded_ops(ops: &[SharedOp]) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.length(ops.len());
    for op in ops {
        writer.bytes(&encoded_op(op));
    }

    writer.finish()
}

pub fn decoded_ops(bytes: &[u8]) -> Result<Vec<SharedOp>> {
    decoded_at_most(bytes, BUNDLE_OPS_AT_MOST)
}

pub fn decoded_kept_ops(bytes: &[u8]) -> Result<Vec<SharedOp>> {
    decoded_at_most(bytes, KEPT_OPS_AT_MOST)
}

fn decoded_at_most(bytes: &[u8], most: usize) -> Result<Vec<SharedOp>> {
    let mut reader = Reader::new(bytes);
    let count = reader.count_of(OP_AT_LEAST)?;
    if count > most {
        return Err(Error::Malformed(Malformed::Overlong { length: count }));
    }
    let mut ops = reserved(count);
    for _ in 0..count {
        if let Some(op) = decoded_op(reader.bytes(LARGEST_OP)?)? {
            ops.push(op);
        }
    }
    reader.done()?;

    Ok(ops)
}

fn encoded_op(op: &SharedOp) -> Vec<u8> {
    let mut writer = Writer::new();
    match op {
        SharedOp::Added {
            entry,
            recording,
            place,
            by,
            stamp,
        } => {
            writer.byte(ADDED);
            writer.fixed(&entry.0);
            writer.recording(recording);
            writer.bytes(&place.0);
            writer.flag(by.is_some());
            if let Some(by) = by {
                writer.fixed(&by.0);
            }
            writer.stamp(*stamp);
        }
        SharedOp::Moved {
            entry,
            place,
            stamp,
        } => {
            writer.byte(MOVED);
            writer.fixed(&entry.0);
            writer.bytes(&place.0);
            writer.stamp(*stamp);
        }
        SharedOp::Removed { entry, stamp } => {
            writer.byte(REMOVED);
            writer.fixed(&entry.0);
            writer.stamp(*stamp);
        }
        SharedOp::Renamed { name, stamp } => {
            writer.byte(RENAMED);
            writer.text(name);
            writer.stamp(*stamp);
        }
        SharedOp::Member {
            member,
            name,
            public,
            stamp,
        } => {
            writer.byte(MEMBER);
            writer.fixed(&member.0);
            writer.text(name);
            writer.fixed(public);
            writer.stamp(*stamp);
        }
        SharedOp::Left { member, stamp } => {
            writer.byte(LEFT);
            writer.fixed(&member.0);
            writer.stamp(*stamp);
        }
    }

    writer.finish()
}

fn decoded_op(bytes: &[u8]) -> Result<Option<SharedOp>> {
    let mut reader = Reader::new(bytes);
    let op = match reader.byte()? {
        ADDED => SharedOp::Added {
            entry: EntryId(reader.fixed()?),
            recording: reader.recording()?,
            place: placed(reader.bytes(LONGEST_PLACE)?)?,
            by: if reader.flag()? {
                Some(MemberId(reader.fixed()?))
            } else {
                None
            },
            stamp: reader.stamp()?,
        },
        MOVED => SharedOp::Moved {
            entry: EntryId(reader.fixed()?),
            place: placed(reader.bytes(LONGEST_PLACE)?)?,
            stamp: reader.stamp()?,
        },
        REMOVED => SharedOp::Removed {
            entry: EntryId(reader.fixed()?),
            stamp: reader.stamp()?,
        },
        RENAMED => SharedOp::Renamed {
            name: reader.text()?,
            stamp: reader.stamp()?,
        },
        MEMBER => SharedOp::Member {
            member: MemberId(reader.fixed()?),
            name: reader.text()?,
            public: reader.fixed()?,
            stamp: reader.stamp()?,
        },
        LEFT => SharedOp::Left {
            member: MemberId(reader.fixed()?),
            stamp: reader.stamp()?,
        },
        _ => return Ok(None),
    };
    reader.done()?;

    Ok(Some(op))
}

fn placed(bytes: &[u8]) -> Result<Place> {
    let Some(last) = bytes.last() else {
        return Err(Error::Malformed(Malformed::Short { length: 0 }));
    };
    if *last == 0 {
        return Err(Error::Malformed(Malformed::NotAPlace));
    }

    Ok(Place(bytes.to_vec()))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Row {
    recording: Option<Recording>,
    by: Option<MemberId>,
    placed: Option<(Place, Stamp)>,
    removed: Option<Stamp>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    pub name: String,
    pub public: [u8; PUBLIC_BYTES],
    stamp: Stamp,
    left: Option<Stamp>,
}

impl Person {
    pub fn is_here(&self) -> bool {
        self.left.is_none_or(|left| left < self.stamp)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    pub entry: EntryId,
    pub recording: Recording,
    pub place: Place,
    pub by: Option<MemberId>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SharedList {
    name: Option<(String, Stamp)>,
    rows: BTreeMap<EntryId, Row>,
    people: BTreeMap<MemberId, Person>,
}

impl SharedList {
    pub fn apply(&mut self, op: &SharedOp) -> bool {
        match op {
            SharedOp::Added { entry, .. }
            | SharedOp::Moved { entry, .. }
            | SharedOp::Removed { entry, .. } => {
                let before = self.rows.get(entry).cloned();
                self.applied(op);
                self.rows.get(entry) != before.as_ref()
            }
            SharedOp::Renamed { .. } => {
                let before = self.name.clone();
                self.applied(op);
                self.name != before
            }
            SharedOp::Member { member, .. } | SharedOp::Left { member, .. } => {
                let before = self.people.get(member).cloned();
                self.applied(op);
                self.people.get(member) != before.as_ref()
            }
        }
    }

    fn applied(&mut self, op: &SharedOp) {
        match op {
            SharedOp::Added {
                entry,
                recording,
                place,
                by,
                stamp,
            } => {
                if let Some(row) = self.row_of(*entry)
                    && row.removed.is_none()
                {
                    row.recording.get_or_insert_with(|| recording.clone());
                    row.by = row.by.or(*by);
                    placed_later(row, place, *stamp);
                }
            }
            SharedOp::Moved {
                entry,
                place,
                stamp,
            } => {
                if let Some(row) = self.row_of(*entry)
                    && row.removed.is_none()
                {
                    placed_later(row, place, *stamp);
                }
            }
            SharedOp::Removed { entry, stamp } => {
                if let Some(row) = self.row_of(*entry) {
                    row.recording = None;
                    row.by = None;
                    row.placed = None;
                    row.removed = Some(row.removed.map_or(*stamp, |held| held.max(*stamp)));
                }
            }
            SharedOp::Renamed { name, stamp } => {
                if self.name.as_ref().is_none_or(|(_, held)| *stamp > *held) {
                    self.name = Some((name.clone(), *stamp));
                }
            }
            SharedOp::Member {
                member,
                name,
                public,
                stamp,
            } => {
                let Some(person) = self.person_of(*member, || Person {
                    name: name.clone(),
                    public: *public,
                    stamp: *stamp,
                    left: None,
                }) else {
                    return;
                };
                if *stamp >= person.stamp {
                    person.name.clone_from(name);
                    person.public = *public;
                    person.stamp = *stamp;
                }
            }
            SharedOp::Left { member, stamp } => {
                let Some(person) = self.person_of(*member, || Person {
                    name: String::new(),
                    public: [0; PUBLIC_BYTES],
                    stamp: Stamp {
                        nanos: i64::MIN,
                        device: stamp.device,
                    },
                    left: None,
                }) else {
                    return;
                };
                person.left = Some(person.left.map_or(*stamp, |held| held.max(*stamp)));
            }
        }
    }

    fn row_of(&mut self, entry: EntryId) -> Option<&mut Row> {
        if !self.rows.contains_key(&entry) && self.rows.len() >= ENTRIES_KEPT_AT_MOST {
            let largest = *self.rows.last_key_value()?.0;
            if entry > largest {
                return None;
            }
            self.rows.remove(&largest);
        }

        Some(self.rows.entry(entry).or_default())
    }

    fn person_of(&mut self, member: MemberId, new: impl FnOnce() -> Person) -> Option<&mut Person> {
        if !self.people.contains_key(&member) && self.people.len() >= PEOPLE_KEPT_AT_MOST {
            let largest = *self.people.last_key_value()?.0;
            if member > largest {
                return None;
            }
            self.people.remove(&largest);
        }

        Some(self.people.entry(member).or_insert_with(new))
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_ref().map(|(name, _)| name.as_str())
    }

    pub fn rows(&self) -> Vec<Listed> {
        let mut rows: Vec<Listed> = self
            .rows
            .iter()
            .filter_map(|(entry, row)| {
                let recording = row.recording.as_ref()?;
                let (place, _) = row.placed.as_ref()?;

                Some(Listed {
                    entry: *entry,
                    recording: recording.clone(),
                    place: place.clone(),
                    by: row.by,
                })
            })
            .collect();
        rows.sort_by(|one, other| {
            one.place
                .cmp(&other.place)
                .then_with(|| one.entry.cmp(&other.entry))
        });
        rows.truncate(ROWS_AT_MOST);

        rows
    }

    pub fn people(&self) -> impl Iterator<Item = (MemberId, &Person)> {
        self.people
            .iter()
            .filter(|(_, person)| person.is_here())
            .map(|(member, person)| (*member, person))
    }

    pub fn as_ops(&self) -> Vec<SharedOp> {
        let mut ops = Vec::with_capacity(self.rows.len() + self.people.len() + 1);
        if let Some((name, stamp)) = &self.name {
            ops.push(SharedOp::Renamed {
                name: name.clone(),
                stamp: *stamp,
            });
        }
        for (member, person) in &self.people {
            ops.push(SharedOp::Member {
                member: *member,
                name: person.name.clone(),
                public: person.public,
                stamp: person.stamp,
            });
            if let Some(stamp) = person.left {
                ops.push(SharedOp::Left {
                    member: *member,
                    stamp,
                });
            }
        }
        for (entry, row) in &self.rows {
            if let Some(stamp) = row.removed {
                ops.push(SharedOp::Removed {
                    entry: *entry,
                    stamp,
                });
                continue;
            }
            match (&row.recording, &row.placed) {
                (Some(recording), Some((place, stamp))) => ops.push(SharedOp::Added {
                    entry: *entry,
                    recording: recording.clone(),
                    place: place.clone(),
                    by: row.by,
                    stamp: *stamp,
                }),
                (None, Some((place, stamp))) => ops.push(SharedOp::Moved {
                    entry: *entry,
                    place: place.clone(),
                    stamp: *stamp,
                }),
                (_, None) => {}
            }
        }

        ops
    }
}

fn placed_later(row: &mut Row, place: &Place, stamp: Stamp) {
    if row.placed.as_ref().is_none_or(|(_, held)| stamp > *held) {
        row.placed = Some((place.clone(), stamp));
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::DeviceId;

    fn stamp(nanos: i64, device: u8) -> Stamp {
        Stamp {
            nanos,
            device: DeviceId([device; 16]),
        }
    }

    fn song(title: &str) -> Recording {
        Recording {
            mbid: None,
            title: title.to_owned(),
            artist: "Ada".to_owned(),
            album: "Album".to_owned(),
            length: Some(Duration::from_secs(200)),
        }
    }

    fn entry(of: u8) -> EntryId {
        EntryId([of; ENTRY_BYTES])
    }

    fn added(of: u8, title: &str, place: &[u8], nanos: i64) -> SharedOp {
        SharedOp::Added {
            entry: entry(of),
            recording: song(title),
            place: Place(place.to_vec()),
            by: None,
            stamp: stamp(nanos, 1),
        }
    }

    fn titles(list: &SharedList) -> Vec<String> {
        list.rows()
            .into_iter()
            .map(|row| row.recording.title)
            .collect()
    }

    #[test]
    fn a_list_shows_only_so_many_rows_the_same_ones_whatever_order_they_arrived_in() {
        let hostile: Vec<SharedOp> = (0..ROWS_AT_MOST + 5)
            .map(|at| SharedOp::Added {
                entry: EntryId({
                    let mut id = [0; ENTRY_BYTES];
                    id[..8].copy_from_slice(&u64::try_from(at).expect("a number").to_be_bytes());
                    id
                }),
                recording: song("Row"),
                place: Place(vec![1, 0, 0, 1]),
                by: None,
                stamp: stamp(1, 1),
            })
            .collect();
        let mut forwards = SharedList::default();
        let mut backwards = SharedList::default();

        for op in &hostile {
            forwards.apply(op);
        }
        for op in hostile.iter().rev() {
            backwards.apply(op);
        }

        assert_eq!(forwards.rows().len(), ROWS_AT_MOST);
        assert_eq!(
            forwards
                .rows()
                .into_iter()
                .map(|row| row.entry)
                .collect::<Vec<_>>(),
            backwards
                .rows()
                .into_iter()
                .map(|row| row.entry)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_list_keeps_only_so_many_entries_the_same_ones_whatever_order_the_ops_arrived_in() {
        let numbered = |at: usize| {
            let mut id = [0; ENTRY_BYTES];
            id[..8].copy_from_slice(&u64::try_from(at).expect("a number").to_be_bytes());
            EntryId(id)
        };
        let flood = ENTRIES_KEPT_AT_MOST + 50;
        let mut ops = Vec::new();
        for at in 0..flood {
            ops.push(SharedOp::Added {
                entry: numbered(at),
                recording: song("Row"),
                place: Place(vec![1, 0, 0, 1]),
                by: None,
                stamp: stamp(1, 1),
            });
            if at % 7 == 0 {
                ops.push(SharedOp::Moved {
                    entry: numbered(at),
                    place: Place(vec![1, 0, 0, 2]),
                    stamp: stamp(2, 1),
                });
            }
            if at % 11 == 0 {
                ops.push(SharedOp::Removed {
                    entry: numbered(at),
                    stamp: stamp(3, 1),
                });
            }
        }
        let mut forwards = SharedList::default();
        let mut backwards = SharedList::default();
        let mut interleaved = SharedList::default();

        for op in &ops {
            forwards.apply(op);
        }
        for op in ops.iter().rev() {
            backwards.apply(op);
        }
        let (front, back) = ops.split_at(ops.len() / 2);
        for pair in front.iter().zip(back.iter().rev()) {
            interleaved.apply(pair.1);
            interleaved.apply(pair.0);
        }
        for op in back.iter().rev().skip(front.len()) {
            interleaved.apply(op);
        }

        assert_eq!(forwards.as_ops().len(), ENTRIES_KEPT_AT_MOST);
        assert_eq!(forwards.as_ops(), backwards.as_ops());
        assert_eq!(forwards.as_ops(), interleaved.as_ops());
        assert_eq!(forwards.rows(), backwards.rows());
    }

    #[test]
    fn a_list_keeps_only_so_many_people_the_same_ones_whatever_order_the_ops_arrived_in() {
        let numbered = |at: usize| {
            let mut id = [0; MEMBER_BYTES];
            id[..8].copy_from_slice(&u64::try_from(at).expect("a number").to_be_bytes());
            MemberId(id)
        };
        let flood = PEOPLE_KEPT_AT_MOST + 50;
        let mut ops = Vec::new();
        for at in 0..flood {
            ops.push(SharedOp::Member {
                member: numbered(at),
                name: String::from("Ada"),
                public: [7; PUBLIC_BYTES],
                stamp: stamp(1, 1),
            });
            if at % 7 == 0 {
                ops.push(SharedOp::Member {
                    member: numbered(at),
                    name: String::from("Grace"),
                    public: [8; PUBLIC_BYTES],
                    stamp: stamp(2, 1),
                });
            }
            if at % 11 == 0 {
                ops.push(SharedOp::Left {
                    member: numbered(at),
                    stamp: stamp(3, 1),
                });
            }
        }
        let mut forwards = SharedList::default();
        let mut backwards = SharedList::default();
        let mut interleaved = SharedList::default();

        for op in &ops {
            forwards.apply(op);
        }
        for op in ops.iter().rev() {
            backwards.apply(op);
        }
        let (front, back) = ops.split_at(ops.len() / 2);
        for pair in front.iter().zip(back.iter().rev()) {
            interleaved.apply(pair.1);
            interleaved.apply(pair.0);
        }
        for op in back.iter().rev().skip(front.len()) {
            interleaved.apply(op);
        }

        assert_eq!(forwards.people.len(), PEOPLE_KEPT_AT_MOST);
        assert_eq!(
            forwards.people.keys().next_back(),
            Some(&numbered(PEOPLE_KEPT_AT_MOST - 1))
        );
        assert_eq!(forwards.as_ops(), backwards.as_ops());
        assert_eq!(forwards.as_ops(), interleaved.as_ops());
    }

    #[test]
    fn a_count_of_ops_far_past_what_follows_reserves_no_more_than_a_little() {
        let mut writer = Writer::new();
        writer.length(30_000_000);
        let mut hostile = writer.finish();
        hostile.resize(hostile.len() + 30_000_000, 0);

        assert!(decoded_ops(&hostile).is_err());
    }

    #[test]
    fn a_place_ending_in_a_zero_byte_is_refused_as_no_place() {
        let hostile = encoded_ops(&[added(1, "One", &[0], 1)]);
        let trailing = encoded_ops(&[added(1, "One", &[1, 0], 1)]);
        let fine = encoded_ops(&[added(1, "One", &[1], 1)]);

        assert!(matches!(
            decoded_ops(&hostile),
            Err(Error::Malformed(Malformed::NotAPlace))
        ));
        assert!(matches!(
            decoded_ops(&trailing),
            Err(Error::Malformed(Malformed::NotAPlace))
        ));
        assert!(decoded_ops(&fine).is_ok());
    }

    fn a_busy_history() -> Vec<SharedOp> {
        vec![
            added(1, "One", &[10], 1),
            added(2, "Two", &[20], 2),
            added(3, "Three", &[30], 3),
            SharedOp::Moved {
                entry: entry(3),
                place: Place(vec![5]),
                stamp: stamp(4, 2),
            },
            SharedOp::Moved {
                entry: entry(3),
                place: Place(vec![25]),
                stamp: stamp(5, 3),
            },
            SharedOp::Removed {
                entry: entry(2),
                stamp: stamp(6, 2),
            },
            SharedOp::Renamed {
                name: "First".to_owned(),
                stamp: stamp(7, 1),
            },
            SharedOp::Renamed {
                name: "Second".to_owned(),
                stamp: stamp(8, 2),
            },
            SharedOp::Member {
                member: MemberId([1; MEMBER_BYTES]),
                name: "Ada".to_owned(),
                public: [1; PUBLIC_BYTES],
                stamp: stamp(9, 1),
            },
            SharedOp::Left {
                member: MemberId([1; MEMBER_BYTES]),
                stamp: stamp(10, 2),
            },
        ]
    }

    fn every_order<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
        if items.len() <= 1 {
            return vec![items.to_vec()];
        }

        let mut orders = Vec::new();
        for at in 0..items.len() {
            let mut rest = items.to_vec();
            let first = rest.remove(at);
            for mut order in every_order(&rest) {
                order.insert(0, first.clone());
                orders.push(order);
            }
        }

        orders
    }

    #[test]
    fn however_the_records_cross_every_device_ends_on_the_same_list() {
        let history = a_busy_history();
        let mut merged = SharedList::default();
        for op in &history {
            merged.apply(op);
        }

        for order in every_order(&history[..7]) {
            let mut list = SharedList::default();
            for op in order.iter().chain(&history[7..]) {
                list.apply(op);
            }
            assert_eq!(list, merged);
        }

        assert_eq!(titles(&merged), ["One", "Three"]);
        assert_eq!(merged.name(), Some("Second"));
    }

    #[test]
    fn an_op_arriving_twice_changes_nothing() {
        let history = a_busy_history();
        let mut once = SharedList::default();
        let mut twice = SharedList::default();
        for op in &history {
            once.apply(op);
            twice.apply(op);
            twice.apply(op);
        }

        assert_eq!(once, twice);
    }

    #[test]
    fn two_people_adding_at_once_both_keep_their_song_and_lose_nothing() {
        let mine = added(1, "Mine", &[10], 5);
        let theirs = added(2, "Theirs", &[10], 5);
        let mut here = SharedList::default();
        let mut there = SharedList::default();

        here.apply(&mine);
        here.apply(&theirs);
        there.apply(&theirs);
        there.apply(&mine);

        assert_eq!(here, there);
        assert_eq!(titles(&here), ["Mine", "Theirs"]);
    }

    #[test]
    fn a_song_removed_stays_removed_though_someone_moved_it_afterwards() {
        let mut list = SharedList::default();

        list.apply(&added(1, "One", &[10], 1));
        list.apply(&SharedOp::Removed {
            entry: entry(1),
            stamp: stamp(2, 1),
        });
        list.apply(&SharedOp::Moved {
            entry: entry(1),
            place: Place(vec![50]),
            stamp: stamp(3, 2),
        });
        list.apply(&added(1, "One", &[10], 1));

        assert!(titles(&list).is_empty());
    }

    #[test]
    fn a_song_added_again_after_removing_it_is_a_row_of_its_own() {
        let mut list = SharedList::default();

        list.apply(&added(1, "One", &[10], 1));
        list.apply(&SharedOp::Removed {
            entry: entry(1),
            stamp: stamp(2, 1),
        });
        list.apply(&added(2, "One", &[10], 3));

        assert_eq!(titles(&list), ["One"]);
    }

    #[test]
    fn a_snapshot_is_the_list_whole() {
        let history = a_busy_history();
        let mut list = SharedList::default();
        for op in &history {
            list.apply(op);
        }

        let mut from_a_snapshot = SharedList::default();
        for op in decoded_ops(&encoded_ops(&list.as_ops())).expect("the ops read back") {
            from_a_snapshot.apply(&op);
        }

        assert_eq!(from_a_snapshot, list);
    }

    #[test]
    fn someone_who_left_and_wrote_again_is_back_and_one_gone_is_not_listed() {
        let member = MemberId([1; MEMBER_BYTES]);
        let mut list = SharedList::default();

        list.apply(&SharedOp::Member {
            member,
            name: "Ada".to_owned(),
            public: [1; PUBLIC_BYTES],
            stamp: stamp(1, 1),
        });
        list.apply(&SharedOp::Left {
            member,
            stamp: stamp(2, 1),
        });
        assert_eq!(list.people().count(), 0);

        list.apply(&SharedOp::Member {
            member,
            name: "Ada".to_owned(),
            public: [1; PUBLIC_BYTES],
            stamp: stamp(3, 1),
        });
        assert_eq!(list.people().count(), 1);
    }

    #[test]
    fn an_op_of_a_later_build_is_passed_over_and_the_rest_read() {
        let known = encoded_op(&SharedOp::Renamed {
            name: "Kept".to_owned(),
            stamp: stamp(1, 1),
        });
        let mut writer = Writer::new();
        writer.length(2);
        writer.bytes(&[250, 1, 2, 3]);
        writer.bytes(&known);

        let read = decoded_ops(&writer.finish()).expect("read");

        assert_eq!(read.len(), 1);
    }

    #[test]
    fn every_op_reads_back_as_itself() {
        let history = a_busy_history();

        assert_eq!(decoded_ops(&encoded_ops(&history)).expect("read"), history);
    }

    #[test]
    fn a_record_of_more_ops_than_a_bundle_holds_is_refused_but_a_kept_list_is_not() {
        let ops: Vec<SharedOp> = (0..=BUNDLE_OPS_AT_MOST)
            .map(|at| SharedOp::Removed {
                entry: entry(u8::try_from(at % 200).expect("a byte")),
                stamp: stamp(1, 1),
            })
            .collect();
        let encoded = encoded_ops(&ops);
        let honest = encoded_ops(&ops[..BUNDLE_OPS_AT_MOST]);

        assert!(matches!(
            decoded_ops(&encoded),
            Err(Error::Malformed(Malformed::Overlong { .. }))
        ));
        assert_eq!(
            decoded_ops(&honest).expect("a bundle").len(),
            BUNDLE_OPS_AT_MOST
        );
        assert_eq!(decoded_kept_ops(&encoded).expect("a list").len(), ops.len());
    }

    #[test]
    fn applying_an_op_says_whether_the_list_changed() {
        let mut list = SharedList::default();
        let first = added(1, "One", &[10], 5);
        let rename = SharedOp::Renamed {
            name: "Mix".to_owned(),
            stamp: stamp(6, 1),
        };
        let older_rename = SharedOp::Renamed {
            name: "Old".to_owned(),
            stamp: stamp(2, 1),
        };

        assert!(list.apply(&first));
        assert!(!list.apply(&first));
        assert!(list.apply(&rename));
        assert!(!list.apply(&rename));
        assert!(!list.apply(&older_rename));
        assert!(list.apply(&SharedOp::Removed {
            entry: entry(1),
            stamp: stamp(7, 1),
        }));
        assert!(!list.apply(&first));
    }
}
