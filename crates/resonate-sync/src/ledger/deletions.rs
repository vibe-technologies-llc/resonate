use resonate_core::Recording;
use rusqlite::{Connection, TransactionBehavior, params};

use super::{Ledger, device_from};
use crate::{
    Error, Result, StoreOp,
    catalog::{Catalog, Refused},
    change::{DeletionNote, Stamp, wall_nanos},
    wire::{decoded_recording, encoded_recording},
};

pub(super) const SONGS_DELETED_EVERYWHERE: &str = "
ALTER TABLE state ADD COLUMN joined INTEGER NOT NULL DEFAULT 0;
CREATE TABLE deletions (
    key TEXT PRIMARY KEY,
    named TEXT NOT NULL,
    recording BLOB NOT NULL,
    deleted INTEGER NOT NULL,
    nanos INTEGER NOT NULL,
    stamped_by BLOB NOT NULL,
    heard INTEGER NOT NULL,
    owed INTEGER NOT NULL DEFAULT 0,
    settled INTEGER NOT NULL DEFAULT 1
) STRICT;
CREATE INDEX deletions_by_name ON deletions(named);
CREATE INDEX deletions_unsettled ON deletions(settled) WHERE settled = 0;
";

const NOTED: &str =
    "INSERT INTO deletions (key, named, recording, deleted, nanos, stamped_by, heard, owed, settled)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
     ON CONFLICT (key) DO UPDATE SET named = ?2, recording = ?3, deleted = ?4, nanos = ?5,
         stamped_by = ?6, heard = ?7, owed = ?8, settled = ?9
     WHERE (?5, ?6) > (deletions.nanos, deletions.stamped_by)";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Owed {
    ToTell,
    Heard,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Settled {
    Already,
    Waiting,
}

fn noted(connection: &Connection, note: &DeletionNote, owed: Owed, settled: Settled) -> Result<()> {
    connection
        .execute(
            NOTED,
            params![
                note.recording.key().as_str(),
                note.recording.named_key().as_str(),
                encoded_recording(&note.recording),
                note.deleted,
                note.stamp.nanos,
                note.stamp.device.0.as_slice(),
                wall_nanos(),
                owed == Owed::ToTell,
                settled == Settled::Already,
            ],
        )
        .map(drop)
        .map_err(|source| Error::store(StoreOp::Write, source))
}

impl Ledger {
    pub(super) fn look_at_deletions(&mut self, catalog: &dyn Catalog) -> Result<()> {
        let told = catalog.deleted_everywhere().map_err(Error::Catalog)?;

        if !told.is_empty() {
            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|source| Error::store(StoreOp::Write, source))?;

            for deleted in &told {
                let note = DeletionNote {
                    recording: deleted.recording.clone(),
                    deleted: true,
                    stamp: self.clock.now(self.device),
                };

                noted(&transaction, &note, Owed::ToTell, Settled::Already)?;
            }
            transaction
                .commit()
                .map_err(|source| Error::store(StoreOp::Write, source))?;

            let ids: Vec<i64> = told.iter().map(|deleted| deleted.id).collect();

            catalog.deletions_told(&ids).map_err(Error::Catalog)?;
        }
        self.look_for_songs_added_again(catalog)
    }

    fn look_for_songs_added_again(&mut self, catalog: &dyn Catalog) -> Result<()> {
        let revision = catalog.revision();

        if self.deletions_tried_at == Some(revision) {
            return Ok(());
        }
        for (standing, heard) in self.heard_where("deleted = 1")? {
            if !catalog
                .added_by_hand_since(&standing.recording, heard)
                .map_err(Error::Catalog)?
            {
                continue;
            }

            let answered = DeletionNote {
                recording: standing.recording,
                deleted: false,
                stamp: self.clock.now(self.device),
            };

            noted(&self.connection, &answered, Owed::ToTell, Settled::Already)?;
        }
        self.deletions_tried_at = Some(revision);
        Ok(())
    }

    pub(super) fn deletion_arrived(&mut self, note: &DeletionNote) -> Result<()> {
        noted(&self.connection, note, Owed::Heard, Settled::Waiting)
    }

    pub(super) fn settle_deletions(&mut self, catalog: &dyn Catalog) -> Result<usize> {
        let waiting = self.heard_where("settled = 0")?;

        if waiting.is_empty() {
            return Ok(0);
        }

        let joined = self.joined()?;
        let mut removed = 0;

        for (note, heard) in waiting {
            let taken = if note.deleted {
                self.taken_here(catalog, &note, heard, joined)
            } else {
                catalog.forgive_deletion(&note.recording).map(|()| 0)
            };

            match taken {
                Ok(copies) => removed += usize::try_from(copies).unwrap_or(usize::MAX),
                Err(Refused::Unavailable) => continue,
                Err(refused) => {
                    tracing::debug!(
                        ?refused,
                        "a deletion another device told was not taken here"
                    );
                }
            }
            self.connection
                .execute(
                    "UPDATE deletions SET settled = 1 WHERE key = ?1 AND nanos = ?2 AND stamped_by = ?3",
                    params![
                        note.recording.key().as_str(),
                        note.stamp.nanos,
                        note.stamp.device.0.as_slice()
                    ],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        Ok(removed)
    }

    fn taken_here(
        &self,
        catalog: &dyn Catalog,
        note: &DeletionNote,
        heard: i64,
        joined: i64,
    ) -> std::result::Result<u64, Refused> {
        let told_before_this_device_joined = note.stamp.nanos <= joined;
        let answered_since = self
            .newest_word_on(&note.recording)
            .map_err(|_| Refused::Unavailable)?
            .is_some_and(|newest| newest.stamp > note.stamp);

        if told_before_this_device_joined || answered_since || !catalog.takes_deletions() {
            return Ok(0);
        }
        catalog.take_deletion(&note.recording, note.stamp.nanos.min(heard))
    }

    pub fn deleted_everywhere(&self, recording: &Recording) -> Result<bool> {
        Ok(self
            .newest_word_on(recording)?
            .is_some_and(|newest| newest.deleted))
    }

    pub fn asked_back(&mut self, recording: &Recording) -> Result<bool> {
        let standing: Vec<DeletionNote> = self
            .alike(recording)?
            .into_iter()
            .filter(|note| note.deleted)
            .collect();

        for note in &standing {
            let answered = DeletionNote {
                recording: note.recording.clone(),
                deleted: false,
                stamp: self.clock.now(self.device),
            };

            noted(&self.connection, &answered, Owed::ToTell, Settled::Already)?;
        }
        if !standing.is_empty() {
            self.keep_the_clock()?;
        }
        Ok(!standing.is_empty())
    }

    fn newest_word_on(&self, recording: &Recording) -> Result<Option<DeletionNote>> {
        Ok(self
            .alike(recording)?
            .into_iter()
            .max_by_key(|note| note.stamp))
    }

    fn alike(&self, recording: &Recording) -> Result<Vec<DeletionNote>> {
        let mut statement = self
            .connection
            .prepare_cached(
                "SELECT recording, deleted, nanos, stamped_by, heard FROM deletions
                  WHERE key IN (?1, ?2) OR named = ?2",
            )
            .map_err(|source| Error::store(StoreOp::Read, source))?;

        let rows = statement
            .query_map(
                params![recording.key().as_str(), recording.named_key().as_str()],
                deletion_row,
            )
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let mut alike = Vec::with_capacity(rows.len());

        for row in rows {
            let note = row.into_note()?;
            if note.recording.names_the_same_song_as(recording) {
                alike.push(note);
            }
        }
        Ok(alike)
    }

    pub(super) fn deletions_where(&self, condition: &str) -> Result<Vec<DeletionNote>> {
        Ok(self
            .heard_where(condition)?
            .into_iter()
            .map(|(note, _)| note)
            .collect())
    }

    fn heard_where(&self, condition: &str) -> Result<Vec<(DeletionNote, i64)>> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT recording, deleted, nanos, stamped_by, heard FROM deletions WHERE {condition} ORDER BY nanos, key"
            ))
            .map_err(|source| Error::store(StoreOp::Read, source))?;

        let rows = statement
            .query_map([], deletion_row)
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        rows.into_iter()
            .map(|row| {
                let heard = row.heard;
                row.into_note().map(|note| (note, heard))
            })
            .collect()
    }

    pub(super) fn joined(&self) -> Result<i64> {
        Ok(self.state_number("joined")?.cast_signed())
    }

    pub(super) fn joined_now(connection: &Connection) -> Result<()> {
        connection
            .execute(
                "UPDATE state SET joined = ?1 WHERE id = 1",
                params![wall_nanos()],
            )
            .map(drop)
            .map_err(|source| Error::store(StoreOp::Write, source))
    }
}

struct DeletionRow {
    recording: Vec<u8>,
    deleted: bool,
    nanos: i64,
    stamped_by: Vec<u8>,
    heard: i64,
}

fn deletion_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DeletionRow> {
    Ok(DeletionRow {
        recording: row.get(0)?,
        deleted: row.get(1)?,
        nanos: row.get(2)?,
        stamped_by: row.get(3)?,
        heard: row.get(4)?,
    })
}

impl DeletionRow {
    fn into_note(self) -> Result<DeletionNote> {
        Ok(DeletionNote {
            recording: decoded_recording(&self.recording)?,
            deleted: self.deleted,
            stamp: Stamp {
                nanos: self.nanos,
                device: device_from(&self.stamped_by),
            },
        })
    }
}
