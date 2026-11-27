use std::collections::BTreeSet;

use resonate_core::Recording;
use rusqlite::{Connection, OptionalExtension, params};

use super::{AnswerSlot, Ledger};
use crate::{
    Error, Result, StoreOp,
    catalog::{Catalog, HeldMark, HeldSince, SongsHeldAfter},
    wire::{decoded_answer, decoded_recording},
};

pub(super) const UNHELD_ANSWERS_WAIT_FOR_THEIR_SONGS: &str = "
ALTER TABLE state ADD COLUMN held_mark INTEGER NOT NULL DEFAULT 0;
ALTER TABLE answers ADD COLUMN named TEXT NOT NULL DEFAULT '';
CREATE INDEX answers_not_here_by_name ON answers(named) WHERE here = 0;
CREATE INDEX answers_not_here_by_file ON answers(file) WHERE here = 0;
";

pub(super) const HELD_SONGS_OF_ONE_SECOND_ARE_TOLD_APART: &str = "
ALTER TABLE state ADD COLUMN held_after INTEGER;
";

const HELD_LOOKED_AT_ONCE: usize = 500;
const UNHELD_OFFERED_AT_ONCE: usize = 500;

const UNNAMED: &str = "";

pub(super) fn name_the_answers(connection: &Connection) -> Result<()> {
    let unnamed = connection
        .prepare("SELECT rowid, recording FROM answers WHERE named = ?1")
        .and_then(|mut statement| {
            statement
                .query_map(params![UNNAMED], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(|source| Error::store(StoreOp::Migrate, source))?;

    for (row, recording) in unnamed {
        let Ok(recording) = decoded_recording(&recording) else {
            continue;
        };

        connection
            .execute(
                "UPDATE answers SET named = ?2 WHERE rowid = ?1",
                params![row, recording.named_key().as_str()],
            )
            .map_err(|source| Error::store(StoreOp::Migrate, source))?;
    }
    Ok(())
}

fn keys_naming(song: &HeldSince) -> BTreeSet<String> {
    let albumless = Recording {
        album: String::new(),
        ..song.recording.clone()
    };
    [
        song.recording.key(),
        song.recording.named_key(),
        albumless.named_key(),
    ]
    .iter()
    .map(|key| key.as_str().to_owned())
    .chain(song.received_under.iter().cloned())
    .collect()
}

struct Unheld {
    row: i64,
    recording: Vec<u8>,
    answer: Vec<u8>,
}

impl Ledger {
    pub(super) fn settle_answers(&mut self, catalog: &dyn Catalog) -> Result<usize> {
        if !catalog.takes_answers() {
            return Ok(0);
        }

        let revision = catalog.revision();

        if self.answers_tried_at == Some(revision) {
            return Ok(0);
        }

        let settled = match self.offer_to_songs_held_since(catalog)? {
            Some(settled) => settled,
            None => self.offer_every_unheld_answer(catalog)?,
        };

        self.answers_tried_at = Some(revision);
        Ok(settled)
    }

    fn held_mark(&self) -> Result<HeldMark> {
        self.connection
            .query_row(
                "SELECT held_mark, held_after FROM state WHERE id = 1",
                [],
                |row| {
                    Ok(HeldMark {
                        at: row.get(0)?,
                        after: row.get(1)?,
                    })
                },
            )
            .map_err(|source| Error::store(StoreOp::Read, source))
    }

    fn keep_held_mark(&self, mark: HeldMark) -> Result<()> {
        self.connection
            .execute(
                "UPDATE state SET held_mark = ?1, held_after = ?2 WHERE id = 1",
                params![mark.at, mark.after],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    fn offer_to_songs_held_since(&mut self, catalog: &dyn Catalog) -> Result<Option<usize>> {
        let mut mark = self.held_mark()?;
        let mut settled = 0;

        loop {
            let songs = match catalog
                .songs_held_after(mark, HELD_LOOKED_AT_ONCE)
                .map_err(Error::Catalog)?
            {
                SongsHeldAfter::Untracked => return Ok(None),
                SongsHeldAfter::Listed(songs) => songs,
            };
            let Some(last) = songs.iter().max_by_key(|song| (song.at, song.id)) else {
                break;
            };
            let whole_page = songs.len() >= HELD_LOOKED_AT_ONCE;
            let moved_on = mark.lists(last);

            for song in &songs {
                for row in self.unheld_answers_naming(song)? {
                    if let Some(unheld) = self.unheld_at(row)? {
                        settled += usize::from(self.offered(catalog, &unheld)?);
                    }
                }
            }
            mark = if whole_page {
                HeldMark::past(last)
            } else {
                HeldMark::from_the_second_of(last)
            };
            self.keep_held_mark(mark)?;
            if !whole_page || !moved_on {
                break;
            }
        }
        Ok(Some(settled))
    }

    fn unheld_answers_naming(&self, song: &HeldSince) -> Result<BTreeSet<i64>> {
        let mut rows = BTreeSet::new();

        for key in keys_naming(song) {
            for query in [
                "SELECT rowid FROM answers WHERE key = ?1 AND here = 0",
                "SELECT rowid FROM answers WHERE named = ?1 AND here = 0",
            ] {
                rows.extend(self.rows_where(query, &key)?);
            }
        }
        if let Some(file) = song.file {
            rows.extend(self.rows_where(
                "SELECT rowid FROM answers WHERE file = ?1 AND here = 0",
                &AnswerSlot::slot_of(file),
            )?);
        }
        Ok(rows)
    }

    fn rows_where(&self, query: &str, value: &dyn rusqlite::ToSql) -> Result<Vec<i64>> {
        self.connection
            .prepare_cached(query)
            .and_then(|mut statement| {
                statement
                    .query_map(params![value], |row| row.get(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(|source| Error::store(StoreOp::Read, source))
    }

    fn unheld_at(&self, row: i64) -> Result<Option<Unheld>> {
        self.connection
            .query_row(
                "SELECT recording, answer FROM answers WHERE rowid = ?1 AND here = 0",
                params![row],
                |found| {
                    Ok(Unheld {
                        row,
                        recording: found.get(0)?,
                        answer: found.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(|source| Error::store(StoreOp::Read, source))
    }

    fn offer_every_unheld_answer(&mut self, catalog: &dyn Catalog) -> Result<usize> {
        let mut settled = 0;
        let mut after = 0_i64;

        loop {
            let page = self.unheld_after(after)?;
            let Some(last) = page.last().map(|unheld| unheld.row) else {
                break;
            };

            for unheld in &page {
                settled += usize::from(self.offered(catalog, unheld)?);
            }
            if page.len() < UNHELD_OFFERED_AT_ONCE {
                break;
            }
            after = last;
        }
        Ok(settled)
    }

    fn unheld_after(&self, after: i64) -> Result<Vec<Unheld>> {
        self.connection
            .prepare_cached(
                "SELECT rowid, recording, answer FROM answers
                  WHERE here = 0 AND rowid > ?1 ORDER BY rowid LIMIT ?2",
            )
            .and_then(|mut statement| {
                statement
                    .query_map(
                        params![
                            after,
                            i64::try_from(UNHELD_OFFERED_AT_ONCE).unwrap_or(i64::MAX)
                        ],
                        |found| {
                            Ok(Unheld {
                                row: found.get(0)?,
                                recording: found.get(1)?,
                                answer: found.get(2)?,
                            })
                        },
                    )?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(|source| Error::store(StoreOp::Read, source))
    }

    fn offered(&mut self, catalog: &dyn Catalog, unheld: &Unheld) -> Result<bool> {
        let Some(answer) = decoded_answer(&unheld.answer)? else {
            return Ok(false);
        };
        let recording = decoded_recording(&unheld.recording)?;

        if !catalog
            .take_answer(&recording, &answer)
            .map_err(Error::Catalog)?
        {
            return Ok(false);
        }
        self.connection
            .execute(
                "UPDATE answers SET here = 1 WHERE rowid = ?1",
                params![unheld.row],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use resonate_core::{Answer, Mbid, SharedLyrics};

    use super::*;
    use crate::{
        ledger::MIGRATIONS,
        wire::{encoded_answer, encoded_recording},
    };

    fn identified() -> Recording {
        Recording {
            mbid: Some(Mbid::new("83d91898-7763-47d7-b03b-b92132375c47").expect("an mbid")),
            title: "Slow Water".into(),
            artist: "Example Band".into(),
            album: "Low Tide".into(),
            length: Some(Duration::from_secs(1_400)),
        }
    }

    #[test]
    fn a_ledger_of_before_unheld_answers_waited_names_them_and_finds_one_by_a_song_held_without_its_mbid()
     {
        let connection = Connection::open_in_memory().expect("a connection");
        let before = MIGRATIONS
            .iter()
            .position(|step| *step == UNHELD_ANSWERS_WAIT_FOR_THEIR_SONGS)
            .expect("the step is in the chain");

        for step in &MIGRATIONS[..before] {
            connection
                .execute_batch(step)
                .expect("the ledger of before");
        }
        connection
            .execute(
                "INSERT INTO state (id, device) VALUES (1, ?1)",
                params![[3_u8; 16].as_slice()],
            )
            .expect("a state row");

        let words = Answer::Lyrics(SharedLyrics {
            text: "Sunlight the fireflies".into(),
            synced: false,
        });

        connection
            .execute(
                "INSERT INTO answers (key, kind, file, recording, answer, here, nanos, stamped_by)
                 VALUES (?1, 0, X'', ?2, ?3, 0, 1, X'04040404040404040404040404040404')",
                params![
                    identified().key().as_str(),
                    encoded_recording(&identified()),
                    encoded_answer(&words)
                ],
            )
            .expect("an unheld answer of before");
        connection
            .pragma_update(
                None,
                "user_version",
                u32::try_from(before).expect("a count"),
            )
            .expect("stamped");

        let ledger = Ledger::over(connection).expect("carried forward");
        let named: String = ledger
            .connection
            .query_row("SELECT named FROM answers", [], |row| row.get(0))
            .expect("read");
        let held_without_its_mbid = HeldSince {
            at: 1,
            id: 1,
            recording: Recording {
                mbid: None,
                ..identified()
            },
            received_under: Vec::new(),
            file: None,
        };

        let found = ledger
            .unheld_answers_naming(&held_without_its_mbid)
            .expect("read");
        let mark = ledger.held_mark().expect("a mark");

        assert_eq!(named, identified().named_key().as_str());
        assert_eq!(found.len(), 1);
        assert_eq!(mark, HeldMark { at: 0, after: None });
    }

    #[test]
    fn a_mark_kept_before_songs_of_one_second_were_told_apart_lists_that_second_from_its_first_song()
     {
        let connection = Connection::open_in_memory().expect("a connection");
        let before = MIGRATIONS
            .iter()
            .position(|step| *step == HELD_SONGS_OF_ONE_SECOND_ARE_TOLD_APART)
            .expect("the step is in the chain");

        for step in &MIGRATIONS[..before] {
            connection
                .execute_batch(step)
                .expect("the ledger of before");
        }
        connection
            .execute(
                "INSERT INTO state (id, device, held_mark) VALUES (1, ?1, 1700000000)",
                params![[3_u8; 16].as_slice()],
            )
            .expect("a state row with a mark");
        connection
            .pragma_update(
                None,
                "user_version",
                u32::try_from(before).expect("a count"),
            )
            .expect("stamped");

        let ledger = Ledger::over(connection).expect("carried forward");

        let kept = ledger.held_mark().expect("a mark");
        let first_of_that_second = HeldSince {
            at: 1_700_000_000,
            id: i64::MIN,
            recording: identified(),
            received_under: Vec::new(),
            file: None,
        };
        let a_second_before = HeldSince {
            at: 1_699_999_999,
            ..first_of_that_second.clone()
        };
        let next = HeldMark::past(&first_of_that_second);

        ledger.keep_held_mark(next).expect("kept");

        let kept_again = ledger.held_mark().expect("a mark");

        assert_eq!(
            kept,
            HeldMark {
                at: 1_700_000_000,
                after: None
            }
        );
        assert!(kept.lists(&first_of_that_second));
        assert!(!kept.lists(&a_second_before));
        assert_eq!(kept_again, next);
        assert!(!kept_again.lists(&first_of_that_second));
    }
}
