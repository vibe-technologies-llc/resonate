use ahash::{AHashMap, AHashSet};
use rusqlite::{Transaction, params};

use crate::{Error, Result, StoreOp, enriched};

macro_rules! loose_in_the_root {
    ($titled:literal) => {
        concat!(
            "SELECT a.id, a.title, a.year, a.tagged_tracks
               FROM albums a
              WHERE a.id IN (SELECT album_id FROM tracks WHERE root_id = ?1 AND album_id IS NOT NULL)",
            $titled,
            " AND NOT EXISTS (SELECT 1 FROM tracks t
                               WHERE t.album_id = a.id AND t.root_id IS NOT ?1)
              AND NOT EXISTS (SELECT 1 FROM album_keys k
                               WHERE k.album_id = a.id AND substr(k.key, 1, 1) = char(31))
              ORDER BY a.id"
        )
    };
}

const NAMED_BY_A_FOLDER_OR_A_RELEASE: &str = loose_in_the_root!("");

const TITLED_AS_AN_ALBUM_WRITTEN_AT: &str = loose_in_the_root!(
    " AND words_of(a.title) IN (SELECT words_of(b.title) FROM albums b
                                 WHERE b.id IN (SELECT album_id FROM tracks
                                                 WHERE root_id = ?1 AND seen = ?2))"
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Weighed {
    EveryAlbum,
    TitlesWrittenAt(i64),
}

const SEATS: &str = "SELECT disc_number, track_number FROM tracks
      WHERE album_id = ?1 AND alternative_of IS NULL";

const FIRST_DISC: u32 = 1;

struct Loose {
    id: i64,
    year: Option<i64>,
    declared: Option<i64>,
    seats: Vec<Option<(u32, u32)>>,
}

pub(crate) fn gather_the_loose(
    tx: &Transaction<'_>,
    roots: &[i64],
    weighed: Weighed,
) -> Result<u64> {
    let mut gathered = 0;
    for root in roots {
        for run in same_titled(tx, *root, weighed)? {
            if !one_record(&run) {
                tracing::debug!(
                    root,
                    albums = run.len(),
                    "albums loose in a root sharing a title are told apart by their numbering"
                );
                continue;
            }
            let Some((survivor, rest)) = run.split_first() else {
                continue;
            };
            for other in rest {
                enriched::gather(tx, survivor.id, other.id)?;
                gathered += 1;
            }
        }
    }

    Ok(gathered)
}

fn same_titled(tx: &Transaction<'_>, root: i64, weighed: Weighed) -> Result<Vec<Vec<Loose>>> {
    let loose = |row: &rusqlite::Row<'_>| {
        Ok((
            row.get::<_, String>(1)?,
            Loose {
                id: row.get(0)?,
                year: row.get(2)?,
                declared: row.get(3)?,
                seats: Vec::new(),
            },
        ))
    };
    let held = match weighed {
        Weighed::EveryAlbum => {
            tx.prepare_cached(NAMED_BY_A_FOLDER_OR_A_RELEASE)
                .and_then(|mut statement| {
                    statement
                        .query_map(params![root], loose)?
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
        }
        Weighed::TitlesWrittenAt(generation) => tx
            .prepare_cached(TITLED_AS_AN_ALBUM_WRITTEN_AT)
            .and_then(|mut statement| {
                statement
                    .query_map(params![root, generation], loose)?
                    .collect::<rusqlite::Result<Vec<_>>>()
            }),
    }
    .map_err(|source| Error::store(StoreOp::Query, source))?;

    let mut by_title: AHashMap<String, Vec<Loose>> = AHashMap::new();
    let mut order = Vec::new();
    for (title, album) in held {
        let title = title.to_lowercase();
        if !by_title.contains_key(&title) {
            order.push(title.clone());
        }
        by_title.entry(title).or_default().push(album);
    }

    let mut runs = Vec::new();
    for title in order {
        let Some(mut run) = by_title.remove(&title).filter(|albums| albums.len() > 1) else {
            continue;
        };
        for album in &mut run {
            album.seats = seats_of(tx, album.id)?;
        }
        runs.push(run);
    }

    Ok(runs)
}

fn seats_of(tx: &Transaction<'_>, album: i64) -> Result<Vec<Option<(u32, u32)>>> {
    let mut statement = tx
        .prepare_cached(SEATS)
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;

    statement
        .query_map(params![album], |row| {
            let disc = row.get::<_, Option<u32>>(0)?;
            let number = row.get::<_, Option<u32>>(1)?;
            Ok(number.map(|number| (disc.unwrap_or(FIRST_DISC), number)))
        })
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn one_record(run: &[Loose]) -> bool {
    let mut taken = AHashSet::new();
    let every_seat_is_its_own = run
        .iter()
        .flat_map(|album| &album.seats)
        .all(|seat| seat.is_some_and(|seat| taken.insert(seat)));

    every_seat_is_its_own
        && agree(run.iter().map(|album| album.year))
        && agree(run.iter().map(|album| album.declared))
        && run
            .iter()
            .find_map(|album| album.declared)
            .is_none_or(|declared| i64::try_from(taken.len()).is_ok_and(|held| held <= declared))
}

fn agree(values: impl Iterator<Item = Option<i64>>) -> bool {
    let mut stated = values.flatten();
    let Some(first) = stated.next() else {
        return true;
    };

    stated.all(|value| value == first)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loose(year: Option<i64>, declared: Option<i64>, seats: &[(u32, u32)]) -> Loose {
        Loose {
            id: 0,
            year,
            declared,
            seats: seats.iter().copied().map(Some).collect(),
        }
    }

    #[test]
    fn albums_whose_numbers_fill_one_run_of_seats_are_one_record() {
        let run = [
            loose(Some(2004), Some(4), &[(1, 1), (1, 3)]),
            loose(None, Some(4), &[(1, 2)]),
            loose(Some(2004), None, &[(1, 4)]),
        ];

        assert!(one_record(&run));
    }

    #[test]
    fn two_albums_that_each_hold_a_first_track_are_two_records() {
        let run = [
            loose(None, None, &[(1, 1), (1, 2)]),
            loose(None, None, &[(1, 1)]),
        ];

        assert!(!one_record(&run));
    }

    #[test]
    fn a_track_with_no_number_leaves_the_albums_apart() {
        let unnumbered = Loose {
            id: 0,
            year: None,
            declared: None,
            seats: vec![None],
        };

        assert!(!one_record(&[loose(None, None, &[(1, 1)]), unnumbered]));
    }

    #[test]
    fn a_year_or_a_total_the_albums_disagree_about_leaves_them_apart() {
        assert!(!one_record(&[
            loose(Some(1999), None, &[(1, 1)]),
            loose(Some(2004), None, &[(1, 2)]),
        ]));
        assert!(!one_record(&[
            loose(None, Some(10), &[(1, 1)]),
            loose(None, Some(12), &[(1, 2)]),
        ]));
    }

    #[test]
    fn more_tracks_than_the_record_declares_are_not_one_record() {
        assert!(!one_record(&[
            loose(None, Some(2), &[(1, 1), (1, 2)]),
            loose(None, None, &[(1, 3)]),
        ]));
    }

    #[test]
    fn the_same_number_on_two_discs_is_two_seats() {
        assert!(one_record(&[
            loose(None, None, &[(1, 1)]),
            loose(None, None, &[(2, 1)]),
        ]));
    }
}
