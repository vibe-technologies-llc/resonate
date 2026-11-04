use std::collections::BTreeSet;

use resonate_core::Recording;

use crate::{
    Result, Stamp,
    shared::{
        op::{EntryId, Listed, MemberId, SharedList, SharedOp},
        place::Place,
    },
};

pub struct Looking<'a> {
    pub list: &'a SharedList,
    pub written: &'a [EntryId],
    pub listed: &'a [Option<Recording>],
    pub by: Option<MemberId>,
}

pub trait Resolving {
    fn held_as(&mut self, recording: &Recording) -> Result<Option<Recording>>;
    fn minted(&mut self) -> Result<EntryId>;
    fn stamped(&mut self) -> Stamp;
}

pub fn edits_seen(looking: &Looking<'_>, doing: &mut impl Resolving) -> Result<Vec<SharedOp>> {
    let rows = looking.list.rows();
    let base: Vec<&Listed> = looking
        .written
        .iter()
        .filter_map(|entry| rows.iter().find(|row| row.entry == *entry))
        .collect();
    let named: Vec<(usize, &Recording)> = looking
        .listed
        .iter()
        .enumerate()
        .filter_map(|(at, row)| row.as_ref().map(|recording| (at, recording)))
        .collect();
    let some_row_is_unnamed = named.len() < looking.listed.len();

    let pairs = paired_with(&base, &named, doing)?;

    let mut ops = Vec::new();
    if !some_row_is_unnamed {
        for (row, pair) in base.iter().zip(&pairs) {
            if pair.is_none() {
                ops.push(SharedOp::Removed {
                    entry: row.entry,
                    stamp: doing.stamped(),
                });
            }
        }
    }

    let mut base_of_named: Vec<Option<usize>> = vec![None; named.len()];
    for (at, pair) in pairs.iter().enumerate() {
        if let Some(pair) = pair {
            base_of_named[*pair] = Some(at);
        }
    }
    let kept_in_order = longest_rising(&base_of_named);

    let moving: BTreeSet<EntryId> = base_of_named
        .iter()
        .enumerate()
        .filter_map(|(at, base_at)| {
            let base_at = (*base_at)?;
            (!kept_in_order.contains(&at)).then(|| base[base_at].entry)
        })
        .collect();
    let removed: BTreeSet<EntryId> = base
        .iter()
        .zip(&pairs)
        .filter(|(_, pair)| !some_row_is_unnamed && pair.is_none())
        .map(|(row, _)| row.entry)
        .collect();
    let mut standing: Vec<Place> = rows
        .iter()
        .filter(|row| !moving.contains(&row.entry) && !removed.contains(&row.entry))
        .map(|row| row.place.clone())
        .collect();
    standing.sort();

    let mut last: Option<Place> = None;
    for (at, (_, recording)) in named.iter().enumerate() {
        let base_at = base_of_named[at];
        if let Some(base_at) = base_at
            && kept_in_order.contains(&at)
        {
            last = Some(base[base_at].place.clone());
            continue;
        }

        let place = placed_after(last.as_ref(), &standing);
        standing.push(place.clone());
        standing.sort();
        last = Some(place.clone());
        ops.push(match base_at {
            Some(base_at) => SharedOp::Moved {
                entry: base[base_at].entry,
                place,
                stamp: doing.stamped(),
            },
            None => SharedOp::Added {
                entry: doing.minted()?,
                recording: (*recording).clone(),
                place,
                by: looking.by,
                stamp: doing.stamped(),
            },
        });
    }

    Ok(ops)
}

fn placed_after(last: Option<&Place>, standing: &[Place]) -> Place {
    match last {
        Some(last) => match standing.iter().find(|place| *place > last) {
            Some(next) => Place::between(Some(last), Some(next)),
            None => Place::after(last),
        },
        None => match standing.first() {
            Some(first) => Place::before(first),
            None => Place::first(),
        },
    }
}

fn paired_with(
    base: &[&Listed],
    named: &[(usize, &Recording)],
    doing: &mut impl Resolving,
) -> Result<Vec<Option<usize>>> {
    let mut taken = vec![false; named.len()];
    let mut pairs = vec![None; base.len()];

    for (at, row) in base.iter().enumerate() {
        let found = (0..named.len())
            .find(|at| !taken[*at] && named[*at].1.names_the_same_song_as(&row.recording));
        if let Some(found) = found {
            taken[found] = true;
            pairs[at] = Some(found);
        }
    }
    for (at, row) in base.iter().enumerate() {
        if pairs[at].is_some() || taken.iter().all(|taken| *taken) {
            continue;
        }
        let Some(own) = doing.held_as(&row.recording)? else {
            continue;
        };
        let found =
            (0..named.len()).find(|at| !taken[*at] && named[*at].1.names_the_same_song_as(&own));
        if let Some(found) = found {
            taken[found] = true;
            pairs[at] = Some(found);
        }
    }

    Ok(pairs)
}

fn longest_rising(sequence: &[Option<usize>]) -> BTreeSet<usize> {
    let mut tails: Vec<usize> = Vec::new();
    let mut before: Vec<Option<usize>> = vec![None; sequence.len()];

    for (at, value) in sequence.iter().enumerate() {
        let Some(value) = *value else {
            continue;
        };
        let spot = tails.partition_point(|tail| sequence[*tail].is_some_and(|held| held < value));
        before[at] = spot.checked_sub(1).map(|earlier| tails[earlier]);
        if spot == tails.len() {
            tails.push(at);
        } else {
            tails[spot] = at;
        }
    }

    let mut kept = BTreeSet::new();
    let mut walking = tails.last().copied();
    while let Some(at) = walking {
        kept.insert(at);
        walking = before[at];
    }

    kept
}

pub fn written_order(
    rows: &[Listed],
    listed: &[Option<Recording>],
    doing: &mut impl Resolving,
) -> Result<Vec<EntryId>> {
    let named: Vec<(usize, &Recording)> = listed
        .iter()
        .enumerate()
        .filter_map(|(at, row)| row.as_ref().map(|recording| (at, recording)))
        .collect();
    let base: Vec<&Listed> = rows.iter().collect();
    let pairs = paired_with(&base, &named, doing)?;

    let mut by_position: Vec<(usize, EntryId)> = pairs
        .iter()
        .zip(rows)
        .filter_map(|(pair, row)| pair.map(|pair| (pair, row.entry)))
        .collect();
    by_position.sort_by_key(|(at, _)| *at);

    Ok(by_position.into_iter().map(|(_, entry)| entry).collect())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use proptest::prelude::*;

    use super::*;
    use crate::{DeviceId, shared::op::ENTRY_BYTES};

    struct Doing {
        minted: u8,
        nanos: i64,
        spelled: Vec<(Recording, Recording)>,
    }

    impl Doing {
        fn new() -> Self {
            Self {
                minted: 100,
                nanos: 1_000,
                spelled: Vec::new(),
            }
        }
    }

    impl Resolving for Doing {
        fn held_as(&mut self, recording: &Recording) -> Result<Option<Recording>> {
            Ok(self
                .spelled
                .iter()
                .find(|(asked, _)| asked == recording)
                .map(|(_, own)| own.clone()))
        }

        fn minted(&mut self) -> Result<EntryId> {
            self.minted += 1;
            Ok(EntryId([self.minted; ENTRY_BYTES]))
        }

        fn stamped(&mut self) -> Stamp {
            self.nanos += 1;
            Stamp {
                nanos: self.nanos,
                device: DeviceId([1; 16]),
            }
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

    fn a_list(titles: &[&str]) -> (SharedList, Vec<EntryId>) {
        let mut list = SharedList::default();
        let mut written = Vec::new();
        let mut place = Place::first();
        for (at, title) in titles.iter().enumerate() {
            let entry = EntryId([u8::try_from(at).expect("few") + 1; ENTRY_BYTES]);
            list.apply(&SharedOp::Added {
                entry,
                recording: song(title),
                place: place.clone(),
                by: None,
                stamp: Stamp {
                    nanos: 1,
                    device: DeviceId([9; 16]),
                },
            });
            written.push(entry);
            place = Place::after(&place);
        }

        (list, written)
    }

    fn looked_at(list: &mut SharedList, written: &[EntryId], listed: &[Option<Recording>]) -> Vec<String> {
        let mut doing = Doing::new();
        let ops = edits_seen(
            &Looking {
                list,
                written,
                listed,
                by: None,
            },
            &mut doing,
        )
        .expect("looked");
        for op in &ops {
            list.apply(op);
        }

        list.rows()
            .into_iter()
            .map(|row| row.recording.title)
            .collect()
    }

    fn listed(titles: &[&str]) -> Vec<Option<Recording>> {
        titles.iter().map(|title| Some(song(title))).collect()
    }

    #[test]
    fn nothing_edited_writes_nothing() {
        let (list, written) = a_list(&["A", "B", "C"]);
        let mut doing = Doing::new();

        let ops = edits_seen(
            &Looking {
                list: &list,
                written: &written,
                listed: &listed(&["A", "B", "C"]),
                by: None,
            },
            &mut doing,
        )
        .expect("looked");

        assert!(ops.is_empty());
    }

    #[test]
    fn a_song_added_removed_or_moved_is_that_one_edit_and_no_more() {
        let (mut added, written) = a_list(&["A", "B", "C"]);
        assert_eq!(
            looked_at(&mut added, &written, &listed(&["A", "New", "B", "C"])),
            ["A", "New", "B", "C"]
        );

        let (mut removed, written) = a_list(&["A", "B", "C"]);
        assert_eq!(
            looked_at(&mut removed, &written, &listed(&["A", "C"])),
            ["A", "C"]
        );

        let (mut moved, written) = a_list(&["A", "B", "C", "D"]);
        let mut doing = Doing::new();
        let ops = edits_seen(
            &Looking {
                list: &moved,
                written: &written,
                listed: &listed(&["D", "A", "B", "C"]),
                by: None,
            },
            &mut doing,
        )
        .expect("looked");
        assert_eq!(ops.len(), 1, "one drag is one move");
        for op in &ops {
            moved.apply(op);
        }
        assert_eq!(
            moved
                .rows()
                .into_iter()
                .map(|row| row.recording.title)
                .collect::<Vec<_>>(),
            ["D", "A", "B", "C"]
        );
    }

    #[test]
    fn the_same_song_twice_stays_twice_and_only_the_one_taken_out_goes() {
        let (mut list, written) = a_list(&["A", "B", "A"]);

        assert_eq!(
            looked_at(&mut list, &written, &listed(&["A", "B"])),
            ["A", "B"]
        );
    }

    #[test]
    fn a_row_a_rescan_left_unnamed_removes_nothing() {
        let (mut list, written) = a_list(&["A", "B", "C"]);
        let rows = vec![Some(song("A")), None, Some(song("C"))];

        assert_eq!(
            looked_at(&mut list, &written, &rows),
            ["A", "B", "C"],
            "a song the catalog cannot name now was told to everyone as removed"
        );
    }

    #[test]
    fn a_song_someone_else_added_and_this_device_lacks_keeps_its_place_through_an_edit() {
        let (mut list, written) = a_list(&["A", "Unheld", "B"]);
        let held_here = vec![written[0], written[2]];

        assert_eq!(
            looked_at(&mut list, &held_here, &listed(&["A", "New", "B"])),
            ["A", "New", "Unheld", "B"]
        );
    }

    #[test]
    fn a_song_the_catalog_holds_under_other_names_is_no_edit() {
        let (list, written) = a_list(&["Their spelling"]);
        let own = song("My spelling");
        let mut doing = Doing::new();
        doing.spelled.push((song("Their spelling"), own.clone()));

        let ops = edits_seen(
            &Looking {
                list: &list,
                written: &written,
                listed: &[Some(own)],
                by: None,
            },
            &mut doing,
        )
        .expect("looked");

        assert!(ops.is_empty());
    }

    #[test]
    fn written_order_follows_the_rows_the_catalog_kept() {
        let (list, written) = a_list(&["A", "Unheld", "B"]);
        let mut doing = Doing::new();

        let kept = written_order(&list.rows(), &listed(&["A", "B"]), &mut doing).expect("paired");

        assert_eq!(kept, [written[0], written[2]]);
    }

    proptest! {
        #[test]
        fn whatever_the_listener_did_the_list_comes_to_read_as_the_catalog_does(
            from in proptest::collection::vec(0_u8..12, 0..12),
            to in proptest::collection::vec(0_u8..12, 0..12),
        ) {
            let from: Vec<String> = from.iter().map(u8::to_string).collect();
            let to: Vec<String> = to.iter().map(u8::to_string).collect();
            let from: Vec<&str> = from.iter().map(String::as_str).collect();
            let to: Vec<&str> = to.iter().map(String::as_str).collect();
            let (mut list, written) = a_list(&from);

            let rows = looked_at(&mut list, &written, &listed(&to));

            prop_assert_eq!(rows, to);
        }
    }
}
