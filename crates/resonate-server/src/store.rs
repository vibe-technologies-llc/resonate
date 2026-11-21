use std::{collections::HashSet, path::Path, time::Duration};

use resonate_sync::{
    DeviceId, SpaceId,
    wire::{BLOBS_A_FRAME_AT_MOST, LARGEST_FRAME, SpaceAsked, SpaceHead},
};
use ring::digest::{Digest, SHA256, digest};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::{Error, Result, StoreOp};

#[cfg(test)]
mod released;

const V1: &str = "
CREATE TABLE spaces (
    id BLOB PRIMARY KEY,
    token BLOB NOT NULL,
    created INTEGER NOT NULL,
    touched INTEGER NOT NULL,
    bytes INTEGER NOT NULL DEFAULT 0,
    head INTEGER NOT NULL DEFAULT 0,
    snapshot_through INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE records (
    space BLOB NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    seq INTEGER NOT NULL,
    device BLOB NOT NULL,
    blob BLOB NOT NULL,
    PRIMARY KEY (space, seq)
) STRICT, WITHOUT ROWID;
CREATE TABLE snapshots (
    space BLOB NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    part INTEGER NOT NULL,
    blob BLOB NOT NULL,
    PRIMARY KEY (space, part)
) STRICT, WITHOUT ROWID;
";

const RECORDS_ARE_KNOWN_BY_THEIR_DIGEST: &str = "
ALTER TABLE records ADD COLUMN digest BLOB;
CREATE INDEX records_by_digest ON records(space, device, digest) WHERE digest IS NOT NULL;
";

const A_SPACE_MAY_BE_SHARED_AND_CLOSED: &str = "
ALTER TABLE spaces ADD COLUMN viewer BLOB;
ALTER TABLE spaces ADD COLUMN closed BLOB;
";

const A_CLOSING_WORD_IS_COUNTED_IN_ITS_SPACE: &str = "
UPDATE spaces SET bytes = bytes + length(closed) WHERE closed IS NOT NULL;
";

const A_COMPACTION_MAY_BE_STAGED_IN_PARTS: &str = "
CREATE TABLE stagings (
    space BLOB PRIMARY KEY REFERENCES spaces(id) ON DELETE CASCADE,
    id INTEGER NOT NULL,
    touched INTEGER NOT NULL,
    bytes INTEGER NOT NULL DEFAULT 0,
    parts INTEGER NOT NULL DEFAULT 0
) STRICT, WITHOUT ROWID;
CREATE TABLE staged (
    space BLOB NOT NULL REFERENCES stagings(space) ON DELETE CASCADE,
    part INTEGER NOT NULL,
    blob BLOB NOT NULL,
    PRIMARY KEY (space, part)
) STRICT, WITHOUT ROWID;
";

const MIGRATIONS: &[&str] = &[
    V1,
    RECORDS_ARE_KNOWN_BY_THEIR_DIGEST,
    A_SPACE_MAY_BE_SHARED_AND_CLOSED,
    A_CLOSING_WORD_IS_COUNTED_IN_ITS_SPACE,
    A_COMPACTION_MAY_BE_STAGED_IN_PARTS,
];

pub const SNAPSHOT_BYTES_A_FRAME: usize = LARGEST_FRAME / 2;

pub struct Store {
    connection: Connection,
    capacity: Capacity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capacity {
    pub spaces: u64,
    pub bytes: u64,
}

impl Capacity {
    pub const UNLIMITED: Self = Self {
        spaces: u64::MAX,
        bytes: u64::MAX,
    };

    fn cannot_hold(self, held: u64, stored: u64, adding: u64, space_bytes: u64) -> bool {
        held.saturating_add(adding) > space_bytes || stored.saturating_add(adding) > self.bytes
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Claim {
    Made,
    Held,
    Refused,
    Throttled,
    Full,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Closing {
    Now(Vec<u8>),
    Before(Vec<u8>),
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appended {
    Through(u64),
    Full,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compacted {
    Through(u64),
    Stale,
    Full,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Staged {
    Taken,
    Unstaged,
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StagingId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reading {
    pub through: u64,
    pub next: u64,
}

pub struct Stored {
    pub seq: u64,
    pub device: DeviceId,
    pub blob: Vec<u8>,
}

pub struct Snapshot {
    pub through: u64,
    pub parts: Vec<Vec<u8>>,
    pub next: Option<u64>,
}

pub enum CaughtUp {
    Snapshot(Snapshot),
    Records(Vec<Stored>),
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let connection =
            Connection::open(path).map_err(|source| Error::store(StoreOp::Open, source))?;
        Self::over(connection)
    }

    pub fn in_memory() -> Result<Self> {
        let connection =
            Connection::open_in_memory().map_err(|source| Error::store(StoreOp::Open, source))?;
        Self::over(connection)
    }

    fn over(mut connection: Connection) -> Result<Self> {
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|source| Error::store(StoreOp::Open, source))?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(|source| Error::store(StoreOp::Open, source))?;
        connection
            .pragma_update(None, "foreign_keys", true)
            .map_err(|source| Error::store(StoreOp::Open, source))?;

        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| Error::store(StoreOp::Migrate, source))?;
        let version: u32 = transaction
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|source| Error::store(StoreOp::Migrate, source))?;
        let laid = usize::try_from(version).unwrap_or(usize::MAX);

        if laid > MIGRATIONS.len() {
            return Err(Error::StoreVersion { found: version });
        }
        for step in &MIGRATIONS[laid..] {
            transaction
                .execute_batch(step)
                .map_err(|source| Error::store(StoreOp::Migrate, source))?;
        }
        transaction
            .pragma_update(
                None,
                "user_version",
                u32::try_from(MIGRATIONS.len()).unwrap_or(u32::MAX),
            )
            .map_err(|source| Error::store(StoreOp::Migrate, source))?;
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Migrate, source))?;
        Ok(Self {
            connection,
            capacity: Capacity::UNLIMITED,
        })
    }

    pub fn limited(mut self, capacity: Capacity) -> Self {
        self.capacity = capacity;
        self
    }

    pub fn claim(
        &mut self,
        space: SpaceId,
        token: &[u8; 32],
        now: i64,
        may_make: impl FnOnce() -> bool,
    ) -> Result<Claim> {
        let held: Option<Vec<u8>> = self
            .connection
            .query_row(
                "SELECT token FROM spaces WHERE id = ?1",
                params![space.0.as_slice()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        match held {
            Some(held) if held == token => {
                self.connection
                    .execute(
                        "UPDATE spaces SET touched = ?2 WHERE id = ?1",
                        params![space.0.as_slice(), now],
                    )
                    .map_err(|source| Error::store(StoreOp::Write, source))?;
                Ok(Claim::Held)
            }
            Some(_) => Ok(Claim::Refused),
            None if !space.may_be_made_by(token) => Ok(Claim::Refused),
            None => {
                let (spaces, bytes) = self.totals()?;

                if spaces >= self.capacity.spaces || bytes >= self.capacity.bytes {
                    return Ok(Claim::Full);
                }
                if !may_make() {
                    return Ok(Claim::Throttled);
                }
                self.connection
                    .execute(
                        "INSERT INTO spaces (id, token, created, touched) VALUES (?1, ?2, ?3, ?3)",
                        params![space.0.as_slice(), token.as_slice(), now],
                    )
                    .map_err(|source| Error::store(StoreOp::Write, source))?;
                Ok(Claim::Made)
            }
        }
    }

    pub fn allow_viewers(&mut self, space: SpaceId, viewer: &[u8; 32]) -> Result<()> {
        self.connection
            .execute(
                "UPDATE spaces SET viewer = coalesce(viewer, ?2) WHERE id = ?1",
                params![space.0.as_slice(), viewer.as_slice()],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;

        Ok(())
    }

    pub fn is_shared(&self, space: SpaceId) -> Result<bool> {
        self.connection
            .query_row(
                "SELECT viewer IS NOT NULL FROM spaces WHERE id = ?1",
                params![space.0.as_slice()],
                |row| row.get(0),
            )
            .optional()
            .map(Option::unwrap_or_default)
            .map_err(|source| Error::store(StoreOp::Read, source))
    }

    pub fn may_view(&mut self, space: SpaceId, viewer: &[u8; 32], now: i64) -> Result<bool> {
        let touched = self
            .connection
            .execute(
                "UPDATE spaces SET touched = ?3 WHERE id = ?1 AND viewer = ?2",
                params![space.0.as_slice(), viewer.as_slice(), now],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;

        Ok(touched > 0)
    }

    pub fn heads(&mut self, asked: &[SpaceAsked], now: i64) -> Result<Vec<SpaceHead>> {
        let mut heads = Vec::with_capacity(asked.len());

        for space in asked {
            let digest = space.token.digest();
            let found: Option<(i64, bool)> = self
                .connection
                .query_row(
                    "UPDATE spaces SET touched = ?3
                      WHERE id = ?1 AND (token = ?2 OR viewer = ?2)
                  RETURNING head, closed IS NOT NULL",
                    params![space.space.0.as_slice(), digest.as_slice(), now],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Write, source))?;
            if let Some((head, closed)) = found {
                heads.push(SpaceHead {
                    space: space.space,
                    head: head.cast_unsigned(),
                    closed,
                });
            }
        }

        Ok(heads)
    }

    pub fn close(&mut self, space: SpaceId, sealed: &[u8], space_bytes: u64) -> Result<Closing> {
        let capacity = self.capacity;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        let (bytes, closed): (i64, Option<Vec<u8>>) = transaction
            .query_row(
                "SELECT bytes, closed FROM spaces WHERE id = ?1",
                params![space.0.as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|source| Error::store(StoreOp::Read, source))?;

        if let Some(first) = closed {
            return Ok(Closing::Before(first));
        }

        let adding = u64::try_from(sealed.len()).unwrap_or(u64::MAX);
        let (_, stored) = totals_in(&transaction)?;

        if capacity.cannot_hold(bytes.cast_unsigned(), stored, adding, space_bytes) {
            return Ok(Closing::Full);
        }
        transaction
            .execute(
                "UPDATE spaces SET closed = ?2, bytes = bytes + ?3 WHERE id = ?1",
                params![
                    space.0.as_slice(),
                    sealed,
                    i64::try_from(adding).unwrap_or(i64::MAX)
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(Closing::Now(sealed.to_vec()))
    }

    pub fn closed(&self, space: SpaceId) -> Result<Option<Vec<u8>>> {
        self.connection
            .query_row(
                "SELECT closed FROM spaces WHERE id = ?1",
                params![space.0.as_slice()],
                |row| row.get(0),
            )
            .optional()
            .map(Option::flatten)
            .map_err(|source| Error::store(StoreOp::Read, source))
    }

    fn totals(&self) -> Result<(u64, u64)> {
        totals_in(&self.connection)
    }

    pub fn head(&self, space: SpaceId) -> Result<(u64, u64)> {
        self.connection
            .query_row(
                "SELECT head, snapshot_through FROM spaces WHERE id = ?1",
                params![space.0.as_slice()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?.cast_unsigned(),
                        row.get::<_, i64>(1)?.cast_unsigned(),
                    ))
                },
            )
            .optional()
            .map(Option::unwrap_or_default)
            .map_err(|source| Error::store(StoreOp::Read, source))
    }

    pub fn append(
        &mut self,
        space: SpaceId,
        device: DeviceId,
        blobs: &[Vec<u8>],
        space_bytes: u64,
    ) -> Result<Appended> {
        let capacity = self.capacity;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        let (head, bytes, closed): (i64, i64, bool) = transaction
            .query_row(
                "SELECT head, bytes, closed IS NOT NULL FROM spaces WHERE id = ?1",
                params![space.0.as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|source| Error::store(StoreOp::Read, source))?;

        if closed {
            return Ok(Appended::Closed);
        }

        let bytes = bytes.cast_unsigned();
        let mut seen = HashSet::with_capacity(blobs.len());

        let distinct: Vec<(Digest, &Vec<u8>)> = blobs
            .iter()
            .map(|blob| (digest(&SHA256, blob), blob))
            .filter(|(known, _)| seen.insert(known.as_ref().to_vec()))
            .collect();
        let distinct_bytes = summed(distinct.iter().map(|(_, blob)| blob.len()));
        let (_, stored) = totals_in(&transaction)?;

        if capacity.cannot_hold(
            bytes,
            stored,
            distinct_bytes.saturating_sub(bytes),
            space_bytes,
        ) {
            return Ok(Appended::Full);
        }

        let mut statement = transaction
            .prepare_cached(
                "SELECT 1 FROM records WHERE space = ?1 AND device = ?2 AND digest = ?3",
            )
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let mut fresh = Vec::with_capacity(distinct.len());

        for (known, blob) in distinct {
            let held = statement
                .query_row(
                    params![space.0.as_slice(), device.0.as_slice(), known.as_ref()],
                    |_| Ok(()),
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Read, source))?
                .is_some();
            if !held {
                fresh.push((known, blob));
            }
        }
        drop(statement);

        let adding = summed(fresh.iter().map(|(_, blob)| blob.len()));

        if capacity.cannot_hold(bytes, stored, adding, space_bytes) {
            return Ok(Appended::Full);
        }

        let adding = i64::try_from(adding).unwrap_or(i64::MAX);
        let mut seq = head;

        for (known, blob) in fresh {
            seq += 1;
            transaction
                .execute(
                    "INSERT INTO records (space, seq, device, blob, digest) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![space.0.as_slice(), seq, device.0.as_slice(), blob, known.as_ref()],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        transaction
            .execute(
                "UPDATE spaces SET head = ?2, bytes = bytes + ?3 WHERE id = ?1",
                params![space.0.as_slice(), seq, adding],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(Appended::Through(seq.cast_unsigned()))
    }

    pub fn records_after(
        &self,
        space: SpaceId,
        after: u64,
        most: usize,
        bytes: usize,
    ) -> Result<Vec<Stored>> {
        let mut statement = self
            .connection
            .prepare_cached(
                "SELECT seq, device, blob FROM records WHERE space = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3",
            )
            .map_err(|source| Error::store(StoreOp::Read, source))?;

        let rows = statement
            .query_map(
                params![
                    space.0.as_slice(),
                    after.cast_signed(),
                    i64::try_from(most).unwrap_or(i64::MAX)
                ],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                    ))
                },
            )
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let mut stored = Vec::new();
        let mut total = 0;

        for row in rows {
            let (seq, device, blob) = row.map_err(|source| Error::store(StoreOp::Read, source))?;

            total += blob.len();
            stored.push(Stored {
                seq: seq.cast_unsigned(),
                device: device_from(&device),
                blob,
            });
            if total >= bytes {
                break;
            }
        }
        Ok(stored)
    }

    pub fn catch_up(
        &self,
        space: SpaceId,
        after: u64,
        reading: Option<Reading>,
        most: usize,
        bytes: usize,
    ) -> Result<CaughtUp> {
        let (_, snapshot_through) = self.head(space)?;

        if after < snapshot_through {
            let from = reading
                .filter(|reading| reading.through == snapshot_through)
                .map_or(0, |reading| reading.next);

            return self
                .snapshot_from(space, snapshot_through, from, SNAPSHOT_BYTES_A_FRAME)
                .map(CaughtUp::Snapshot);
        }
        self.records_after(space, after, most, bytes)
            .map(CaughtUp::Records)
    }

    pub fn snapshot(&self, space: SpaceId) -> Result<Option<Snapshot>> {
        let (_, through) = self.head(space)?;

        if through == 0 {
            return Ok(None);
        }
        self.snapshot_from(space, through, 0, usize::MAX).map(Some)
    }

    fn snapshot_from(
        &self,
        space: SpaceId,
        through: u64,
        from: u64,
        bytes: usize,
    ) -> Result<Snapshot> {
        let mut statement = self
            .connection
            .prepare_cached(
                "SELECT part, blob FROM snapshots WHERE space = ?1 AND part >= ?2 ORDER BY part",
            )
            .map_err(|source| Error::store(StoreOp::Read, source))?;

        let rows = statement
            .query_map(params![space.0.as_slice(), from.cast_signed()], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let mut parts = Vec::new();
        let mut total = 0_usize;
        let mut next = None;

        for row in rows {
            let (part, blob) = row.map_err(|source| Error::store(StoreOp::Read, source))?;
            let fits =
                parts.len() < BLOBS_A_FRAME_AT_MOST && total.saturating_add(blob.len()) <= bytes;

            if !parts.is_empty() && !fits {
                next = Some(part.cast_unsigned());
                break;
            }
            total = total.saturating_add(blob.len());
            parts.push(blob);
        }
        Ok(Snapshot {
            through,
            parts,
            next,
        })
    }

    pub fn compact(
        &mut self,
        space: SpaceId,
        through: u64,
        parts: &[Vec<u8>],
        space_bytes: u64,
    ) -> Result<Compacted> {
        self.replaced(space, through, None, parts, space_bytes)
    }

    pub fn stage(&mut self, space: SpaceId, id: StagingId, now: i64) -> Result<()> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| Error::store(StoreOp::Write, source))?;

        transaction
            .execute(
                "DELETE FROM stagings WHERE space = ?1",
                params![space.0.as_slice()],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        transaction
            .execute(
                "INSERT INTO stagings (space, id, touched) VALUES (?1, ?2, ?3)",
                params![space.0.as_slice(), id.0.cast_signed(), now],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Write, source))
    }

    pub fn staged(
        &mut self,
        space: SpaceId,
        id: StagingId,
        parts: &[Vec<u8>],
        space_bytes: u64,
        now: i64,
    ) -> Result<Staged> {
        let capacity = self.capacity;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        let Some((staged_bytes, staged_parts)) = staging_in(&transaction, space, id)? else {
            return Ok(Staged::Unstaged);
        };
        let adding = summed(parts.iter().map(Vec::len));
        let (_, stored) = totals_in(&transaction)?;

        if capacity.cannot_hold(staged_bytes, stored, adding, space_bytes) {
            transaction
                .execute(
                    "DELETE FROM stagings WHERE space = ?1",
                    params![space.0.as_slice()],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
            transaction
                .commit()
                .map_err(|source| Error::store(StoreOp::Write, source))?;
            return Ok(Staged::Full);
        }
        for (part, blob) in (staged_parts..).zip(parts) {
            transaction
                .execute(
                    "INSERT INTO staged (space, part, blob) VALUES (?1, ?2, ?3)",
                    params![space.0.as_slice(), part.cast_signed(), blob],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }

        let part = staged_parts.saturating_add(u64::try_from(parts.len()).unwrap_or(u64::MAX));

        transaction
            .execute(
                "UPDATE stagings SET bytes = bytes + ?2, parts = ?3, touched = ?4 WHERE space = ?1",
                params![
                    space.0.as_slice(),
                    adding.cast_signed(),
                    part.cast_signed(),
                    now
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(Staged::Taken)
    }

    pub fn compact_staged(
        &mut self,
        space: SpaceId,
        id: StagingId,
        through: u64,
        parts: &[Vec<u8>],
        space_bytes: u64,
    ) -> Result<Compacted> {
        let compacted = self.replaced(space, through, Some(id), parts, space_bytes)?;

        if compacted != Compacted::Through(through) {
            self.unstage(space, id)?;
        }
        Ok(compacted)
    }

    pub fn unstage(&mut self, space: SpaceId, id: StagingId) -> Result<()> {
        self.connection
            .execute(
                "DELETE FROM stagings WHERE space = ?1 AND id = ?2",
                params![space.0.as_slice(), id.0.cast_signed()],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    pub fn sweep_staged(&mut self, untouched_since: i64) -> Result<usize> {
        self.connection
            .execute(
                "DELETE FROM stagings WHERE touched < ?1",
                params![untouched_since],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))
    }

    fn replaced(
        &mut self,
        space: SpaceId,
        through: u64,
        staged: Option<StagingId>,
        parts: &[Vec<u8>],
        space_bytes: u64,
    ) -> Result<Compacted> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        let (head, snapshot_through, held, closed): (i64, i64, i64, bool) = transaction
            .query_row(
                "SELECT head, snapshot_through, bytes, closed IS NOT NULL FROM spaces WHERE id = ?1",
                params![space.0.as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(|source| Error::store(StoreOp::Read, source))?;

        if closed {
            return Ok(Compacted::Closed);
        }

        let staged_parts = match staged {
            None => 0,
            Some(id) => match staging_in(&transaction, space, id)? {
                Some((_, parts)) => parts,
                None => return Ok(Compacted::Stale),
            },
        };
        let through = through.cast_signed();

        if through <= snapshot_through || through > head {
            return Ok(Compacted::Stale);
        }
        transaction
            .execute(
                "DELETE FROM records WHERE space = ?1 AND seq <= ?2",
                params![space.0.as_slice(), through],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        transaction
            .execute(
                "DELETE FROM snapshots WHERE space = ?1",
                params![space.0.as_slice()],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        if staged.is_some() {
            transaction
                .execute(
                    "INSERT INTO snapshots (space, part, blob) SELECT space, part, blob FROM staged WHERE space = ?1",
                    params![space.0.as_slice()],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
            transaction
                .execute(
                    "DELETE FROM stagings WHERE space = ?1",
                    params![space.0.as_slice()],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        for (part, blob) in (staged_parts..).zip(parts) {
            transaction
                .execute(
                    "INSERT INTO snapshots (space, part, blob) VALUES (?1, ?2, ?3)",
                    params![space.0.as_slice(), part.cast_signed(), blob],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }

        let bytes: i64 = transaction
            .query_row(
                "SELECT coalesce((SELECT sum(length(blob)) FROM records WHERE space = ?1), 0)
                      + coalesce((SELECT sum(length(blob)) FROM snapshots WHERE space = ?1), 0)",
                params![space.0.as_slice()],
                |row| row.get(0),
            )
            .map_err(|source| Error::store(StoreOp::Read, source))?;

        if bytes.cast_unsigned() > space_bytes {
            return Ok(Compacted::Full);
        }

        let growth = bytes.saturating_sub(held).max(0).cast_unsigned();
        let (_, stored) = totals_in(&transaction)?;

        if stored.saturating_add(growth) > self.capacity.bytes {
            return Ok(Compacted::Full);
        }
        transaction
            .execute(
                "UPDATE spaces SET snapshot_through = ?2, bytes = ?3 WHERE id = ?1",
                params![space.0.as_slice(), through, bytes],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(Compacted::Through(through.cast_unsigned()))
    }

    pub fn sweep_unused(&mut self, made_before: i64) -> Result<usize> {
        self.connection
            .execute(
                "DELETE FROM spaces WHERE head = 0 AND snapshot_through = 0 AND created < ?1",
                params![made_before],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))
    }

    pub fn sweep(&mut self, untouched_since: i64) -> Result<usize> {
        self.connection
            .execute(
                "DELETE FROM spaces WHERE touched < ?1",
                params![untouched_since],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))
    }
}

fn summed(lengths: impl Iterator<Item = usize>) -> u64 {
    lengths.fold(0, |sum, length| {
        sum.saturating_add(u64::try_from(length).unwrap_or(u64::MAX))
    })
}

fn staging_in(
    connection: &rusqlite::Connection,
    space: SpaceId,
    id: StagingId,
) -> Result<Option<(u64, u64)>> {
    connection
        .query_row(
            "SELECT bytes, parts FROM stagings WHERE space = ?1 AND id = ?2",
            params![space.0.as_slice(), id.0.cast_signed()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?.cast_unsigned(),
                    row.get::<_, i64>(1)?.cast_unsigned(),
                ))
            },
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Read, source))
}

fn totals_in(connection: &rusqlite::Connection) -> Result<(u64, u64)> {
    connection
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(bytes), 0) + COALESCE((SELECT SUM(bytes) FROM stagings), 0) FROM spaces",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?.cast_unsigned(),
                    row.get::<_, i64>(1)?.cast_unsigned(),
                ))
            },
        )
        .map_err(|source| Error::store(StoreOp::Read, source))
}

fn device_from(bytes: &[u8]) -> DeviceId {
    let mut device = [0; 16];

    let length = bytes.len().min(device.len());

    device[..length].copy_from_slice(&bytes[..length]);
    DeviceId(device)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPACE: SpaceId = SpaceId([1; 16]);
    const DESK: DeviceId = DeviceId([2; 16]);

    fn claimed() -> Store {
        let mut store = Store::in_memory().expect("a store");

        assert_eq!(
            store.claim(SPACE, &[3; 32], 1, || true).expect("claimed"),
            Claim::Made
        );
        store
    }

    #[test]
    fn a_space_is_claimed_by_its_first_token_and_refuses_any_other() {
        let mut store = claimed();

        assert_eq!(
            store.claim(SPACE, &[3; 32], 2, || true).expect("held"),
            Claim::Held
        );
        assert_eq!(
            store.claim(SPACE, &[4; 32], 3, || true).expect("refused"),
            Claim::Refused
        );
    }

    #[test]
    fn a_space_committed_to_a_write_token_is_made_by_that_token_alone_and_held_by_it_after() {
        let mut store = Store::in_memory().expect("a store");
        let owner = resonate_sync::Token([5; 32]).digest();
        let committed = SpaceId::committed_to(&owner);

        let first_viewer = store
            .claim(committed, &[6; 32], 1, || true)
            .expect("refused");
        let owner_after = store.claim(committed, &owner, 2, || true).expect("made");
        let owner_again = store.claim(committed, &owner, 3, || true).expect("held");
        let unmarked_first = store.claim(SPACE, &[6; 32], 4, || true).expect("made");

        assert_eq!(first_viewer, Claim::Refused);
        assert_eq!(owner_after, Claim::Made);
        assert_eq!(owner_again, Claim::Held);
        assert_eq!(unmarked_first, Claim::Made);
    }

    #[test]
    fn a_space_closed_twice_is_told_to_have_been_closed_by_the_first_word_alone() {
        let mut store = claimed();

        let first = store.close(SPACE, &[7; 32], 1 << 20).expect("closed");
        let second = store.close(SPACE, &[8; 32], 1 << 20).expect("closed again");

        assert_eq!(first, Closing::Now(vec![7; 32]));
        assert_eq!(second, Closing::Before(vec![7; 32]));
        assert_eq!(store.closed(SPACE).expect("read"), Some(vec![7; 32]));
    }

    #[test]
    fn a_closing_word_is_weighed_against_the_space_and_total_caps_and_counted_once_kept() {
        let mut store = claimed().limited(Capacity {
            spaces: u64::MAX,
            bytes: 100,
        });
        let other = SpaceId([9; 16]);

        store.claim(other, &[3; 32], 1, || true).expect("made");
        store
            .append(SPACE, DESK, &[vec![0; 40]], 1 << 20)
            .expect("appended");

        let past_the_space = store.close(SPACE, &[7; 32], 64).expect("weighed");
        let kept = store.close(SPACE, &[7; 32], 1 << 20).expect("closed");
        let (_, stored) = store.totals().expect("totals");
        let past_the_total = store.close(other, &[8; 32], 1 << 20).expect("weighed");

        assert_eq!(past_the_space, Closing::Full);
        assert_eq!(kept, Closing::Now(vec![7; 32]));
        assert_eq!(stored, 72);
        assert_eq!(past_the_total, Closing::Full);
        assert_eq!(store.closed(other).expect("read"), None);
    }

    #[test]
    fn a_closing_word_kept_before_it_was_counted_is_counted_once_the_store_is_opened() {
        let connection = Connection::open_in_memory().expect("a connection");

        for step in &MIGRATIONS[..3] {
            connection.execute_batch(step).expect("an earlier store");
        }
        connection
            .execute(
                "INSERT INTO spaces (id, token, created, touched, bytes, closed) VALUES (?1, ?2, 1, 1, 40, ?3)",
                params![
                    SPACE.0.as_slice(),
                    [3_u8; 32].as_slice(),
                    [7_u8; 32].as_slice()
                ],
            )
            .expect("a closed space");
        connection
            .pragma_update(None, "user_version", 3)
            .expect("stamped");

        let store = Store::over(connection).expect("carried forward");

        assert_eq!(store.totals().expect("totals"), (1, 72));
        assert_eq!(store.closed(SPACE).expect("read"), Some(vec![7; 32]));
    }

    #[test]
    fn many_repeats_inside_one_append_are_kept_once_each() {
        let mut store = claimed();
        let repeated: Vec<Vec<u8>> = (0..resonate_sync::wire::BLOBS_A_FRAME_AT_MOST)
            .map(|at| vec![u8::try_from(at % 7).expect("a byte"); 40])
            .collect();

        let appended = store
            .append(SPACE, DESK, &repeated, 1 << 20)
            .expect("appended");
        let kept = store.records_after(SPACE, 0, 100, 1 << 20).expect("read");

        assert_eq!(appended, Appended::Through(7));
        assert_eq!(
            kept.iter().map(|record| record.blob[0]).collect::<Vec<_>>(),
            [0, 1, 2, 3, 4, 5, 6]
        );
    }

    #[test]
    fn an_append_no_lookup_could_make_fit_is_refused_and_a_resend_within_the_cap_is_not() {
        let mut store = claimed();

        store
            .append(SPACE, DESK, &[vec![1; 40]], 100)
            .expect("appended");

        let past = store
            .append(SPACE, DESK, &[vec![2; 40], vec![3; 40], vec![4; 40]], 100)
            .expect("weighed");
        let resent = store
            .append(SPACE, DESK, &[vec![1; 40]], 100)
            .expect("resent");

        assert_eq!(past, Appended::Full);
        assert_eq!(resent, Appended::Through(1));
    }

    #[test]
    fn records_are_numbered_in_order_and_read_after_a_cursor() {
        let mut store = claimed();

        assert_eq!(
            store
                .append(SPACE, DESK, &[vec![1], vec![2], vec![3]], 1 << 20)
                .expect("appended"),
            Appended::Through(3)
        );

        let after = store.records_after(SPACE, 1, 10, 1 << 20).expect("read");

        assert_eq!(
            after.iter().map(|record| record.seq).collect::<Vec<_>>(),
            [2, 3]
        );
        assert_eq!(after[0].device, DESK);
        assert_eq!(store.head(SPACE).expect("head"), (3, 0));
    }

    #[test]
    fn a_full_space_takes_nothing_more() {
        let mut store = claimed();

        assert_eq!(
            store
                .append(SPACE, DESK, &[vec![0; 10]], 15)
                .expect("appended"),
            Appended::Through(1)
        );
        assert_eq!(
            store
                .append(SPACE, DESK, &[vec![1; 10]], 15)
                .expect("refused"),
            Appended::Full
        );
    }

    #[test]
    fn a_record_sent_again_by_the_device_that_sent_it_is_kept_once() {
        let mut store = claimed();
        let phone = DeviceId([9; 16]);

        let first = store
            .append(SPACE, DESK, &[vec![1; 40], vec![2; 40]], 1 << 20)
            .expect("appended");
        let again = store
            .append(
                SPACE,
                DESK,
                &[vec![1; 40], vec![2; 40], vec![3; 40]],
                1 << 20,
            )
            .expect("appended again");
        let elsewhere = store
            .append(SPACE, phone, &[vec![1; 40]], 1 << 20)
            .expect("appended elsewhere");
        let kept = store.records_after(SPACE, 0, 10, 1 << 20).expect("read");

        assert_eq!(first, Appended::Through(2));
        assert_eq!(again, Appended::Through(3));
        assert_eq!(elsewhere, Appended::Through(4));
        assert_eq!(
            kept.iter().map(|record| record.blob[0]).collect::<Vec<_>>(),
            [1, 2, 3, 1]
        );
    }

    #[test]
    fn a_store_of_the_first_release_is_carried_forward_with_its_records() {
        let connection = Connection::open_in_memory().expect("a connection");

        connection
            .execute_batch(V1)
            .expect("the first release's store");
        connection
            .execute(
                "INSERT INTO spaces (id, token, created, touched, head, bytes) VALUES (?1, ?2, 1, 1, 1, 3)",
                params![SPACE.0.as_slice(), [3_u8; 32].as_slice()],
            )
            .expect("a space");
        connection
            .execute(
                "INSERT INTO records (space, seq, device, blob) VALUES (?1, 1, ?2, ?3)",
                params![SPACE.0.as_slice(), DESK.0.as_slice(), [7_u8; 3].as_slice()],
            )
            .expect("a record");
        connection
            .pragma_update(None, "user_version", 1)
            .expect("stamped");

        let mut store = Store::over(connection).expect("carried forward");

        let appended = store
            .append(SPACE, DESK, &[vec![7; 3]], 1 << 20)
            .expect("appended");

        assert_eq!(appended, Appended::Through(2));
        assert_eq!(store.head(SPACE).expect("head"), (2, 0));
    }

    #[test]
    fn a_compaction_replaces_what_it_covers_and_refuses_a_stale_one() {
        let mut store = claimed();

        store
            .append(
                SPACE,
                DESK,
                &[vec![1; 100], vec![2; 100], vec![3; 100]],
                1 << 20,
            )
            .expect("appended");

        assert_eq!(
            store
                .compact(SPACE, 2, &[vec![9; 10]], 1 << 20)
                .expect("compacted"),
            Compacted::Through(2)
        );
        assert_eq!(
            store
                .compact(SPACE, 2, &[vec![9; 10]], 1 << 20)
                .expect("stale"),
            Compacted::Stale
        );
        assert_eq!(
            store
                .compact(SPACE, 4, &[vec![9; 10]], 1 << 20)
                .expect("ahead"),
            Compacted::Stale
        );

        let snapshot = store.snapshot(SPACE).expect("read").expect("a snapshot");

        assert_eq!(snapshot.through, 2);
        assert_eq!(snapshot.parts, [vec![9; 10]]);

        let left = store.records_after(SPACE, 0, 10, 1 << 20).expect("read");

        assert_eq!(
            left.iter().map(|record| record.seq).collect::<Vec<_>>(),
            [3]
        );
    }

    const STAGED: StagingId = StagingId(41);
    const STAGED_LATER: StagingId = StagingId(42);

    fn appended_three() -> Store {
        let mut store = claimed();

        store
            .append(
                SPACE,
                DESK,
                &[vec![1; 100], vec![2; 100], vec![3; 100]],
                1 << 20,
            )
            .expect("appended");
        store
    }

    fn seqs_after(store: &Store, after: u64) -> Vec<u64> {
        store
            .records_after(SPACE, after, 10, 1 << 20)
            .expect("read")
            .iter()
            .map(|record| record.seq)
            .collect()
    }

    #[test]
    fn the_compaction_through_nothing_a_staging_is_asked_by_is_refused_as_stale_by_the_one_frame_form()
     {
        let mut store = appended_three();

        let asked = store.compact(SPACE, 0, &[], 1 << 20).expect("refused");

        assert_eq!(asked, Compacted::Stale);
        assert_eq!(store.head(SPACE).expect("head"), (3, 0));
    }

    #[test]
    fn a_compaction_staged_in_parts_replaces_the_log_only_once_its_last_part_is_taken() {
        let mut store = appended_three();

        store.stage(SPACE, STAGED, 1).expect("staged");

        let first = store
            .staged(SPACE, STAGED, &[vec![7; 10], vec![8; 10]], 1 << 20, 1)
            .expect("taken");
        let before = store.catch_up(SPACE, 0, None, 10, 1 << 20).expect("read");
        let (_, held_while_staging) = store.totals().expect("totals");
        let compacted = store
            .compact_staged(SPACE, STAGED, 2, &[vec![9; 10]], 1 << 20)
            .expect("compacted");
        let snapshot = store.snapshot(SPACE).expect("read").expect("a snapshot");
        let (_, held_after) = store.totals().expect("totals");
        let again = store
            .compact_staged(SPACE, STAGED, 3, &[vec![9; 10]], 1 << 20)
            .expect("refused");

        assert_eq!(first, Staged::Taken);

        let CaughtUp::Records(whole) = before else {
            panic!("the old log, whole");
        };

        assert_eq!(whole.len(), 3);
        assert_eq!(held_while_staging, 320);
        assert_eq!(compacted, Compacted::Through(2));
        assert_eq!(snapshot.through, 2);
        assert_eq!(snapshot.parts, [vec![7; 10], vec![8; 10], vec![9; 10]]);
        assert_eq!(seqs_after(&store, 0), [3]);
        assert_eq!(held_after, 130);
        assert_eq!(again, Compacted::Stale);
    }

    #[test]
    fn a_staging_left_unfinished_leaves_the_log_whole_and_holds_nothing_once_unstaged_or_swept() {
        let mut store = appended_three();

        store.stage(SPACE, STAGED, 1).expect("staged");
        store
            .staged(SPACE, STAGED, &[vec![7; 50]], 1 << 20, 1)
            .expect("taken");

        store.unstage(SPACE, STAGED).expect("unstaged");

        let (_, held) = store.totals().expect("totals");
        let after_unstaging = store
            .staged(SPACE, STAGED, &[vec![7; 50]], 1 << 20, 2)
            .expect("unstaged");
        let finished_after = store
            .compact_staged(SPACE, STAGED, 2, &[], 1 << 20)
            .expect("refused");

        store.stage(SPACE, STAGED_LATER, 5).expect("staged");
        store
            .staged(SPACE, STAGED_LATER, &[vec![7; 50]], 1 << 20, 6)
            .expect("taken");

        let kept_while_touched = store.sweep_staged(6).expect("swept");
        let swept_once_untouched = store.sweep_staged(7).expect("swept");
        let (_, held_after_the_sweep) = store.totals().expect("totals");

        assert_eq!(held, 300);
        assert_eq!(after_unstaging, Staged::Unstaged);
        assert_eq!(finished_after, Compacted::Stale);
        assert_eq!(kept_while_touched, 0);
        assert_eq!(swept_once_untouched, 1);
        assert_eq!(held_after_the_sweep, 300);
        assert_eq!(store.head(SPACE).expect("head"), (3, 0));
        assert_eq!(seqs_after(&store, 0), [1, 2, 3]);
    }

    #[test]
    fn a_later_staging_of_the_space_puts_the_earlier_aside() {
        let mut store = appended_three();

        store.stage(SPACE, STAGED, 1).expect("staged");
        store
            .staged(SPACE, STAGED, &[vec![7; 10]], 1 << 20, 1)
            .expect("taken");

        store.stage(SPACE, STAGED_LATER, 2).expect("staged");

        let earlier = store
            .staged(SPACE, STAGED, &[vec![7; 10]], 1 << 20, 2)
            .expect("unstaged");
        let earlier_finished = store
            .compact_staged(SPACE, STAGED, 2, &[], 1 << 20)
            .expect("refused");
        let later_finished = store
            .compact_staged(SPACE, STAGED_LATER, 2, &[vec![9; 10]], 1 << 20)
            .expect("compacted");

        assert_eq!(earlier, Staged::Unstaged);
        assert_eq!(earlier_finished, Compacted::Stale);
        assert_eq!(later_finished, Compacted::Through(2));
        assert_eq!(
            store.snapshot(SPACE).expect("read").expect("held").parts,
            [vec![9; 10]]
        );
    }

    #[test]
    fn what_is_staged_is_weighed_against_the_space_and_the_total_cap_and_put_aside_past_either() {
        let mut store = appended_three().limited(Capacity {
            spaces: u64::MAX,
            bytes: 500,
        });
        let other = SpaceId([9; 16]);

        store.claim(other, &[3; 32], 1, || true).expect("made");

        store.stage(SPACE, STAGED, 1).expect("staged");

        let past_the_space = store
            .staged(SPACE, STAGED, &[vec![7; 101]], 100, 1)
            .expect("weighed");
        let gone = store
            .staged(SPACE, STAGED, &[vec![7; 10]], 100, 1)
            .expect("unstaged");

        store.stage(SPACE, STAGED_LATER, 2).expect("staged");

        let within = store
            .staged(SPACE, STAGED_LATER, &[vec![7; 150]], 1 << 20, 2)
            .expect("taken");
        let crowding_another = store
            .append(other, DESK, &[vec![1; 60]], 1 << 20)
            .expect("weighed");
        let past_the_total = store
            .staged(SPACE, STAGED_LATER, &[vec![7; 60]], 1 << 20, 3)
            .expect("weighed");
        let (_, held) = store.totals().expect("totals");

        assert_eq!(past_the_space, Staged::Full);
        assert_eq!(gone, Staged::Unstaged);
        assert_eq!(within, Staged::Taken);
        assert_eq!(crowding_another, Appended::Full);
        assert_eq!(past_the_total, Staged::Full);
        assert_eq!(held, 300);
    }

    #[test]
    fn a_snapshot_past_a_frame_is_read_in_frames_and_begun_again_when_another_replaces_it_between()
    {
        let mut store = appended_three();
        let parts = vec![vec![5; 30]; BLOBS_A_FRAME_AT_MOST + 1];

        store.compact(SPACE, 2, &parts, 1 << 30).expect("compacted");

        let CaughtUp::Snapshot(first) = store.catch_up(SPACE, 0, None, 10, 1 << 20).expect("read")
        else {
            panic!("the snapshot's first frame");
        };
        let reading = Reading {
            through: first.through,
            next: first.next.expect("more to read"),
        };
        let CaughtUp::Snapshot(rest) = store
            .catch_up(SPACE, 0, Some(reading), 10, 1 << 20)
            .expect("read")
        else {
            panic!("the snapshot's last frame");
        };

        store.compact(SPACE, 3, &parts, 1 << 30).expect("compacted");

        let CaughtUp::Snapshot(replaced) = store
            .catch_up(SPACE, 0, Some(reading), 10, 1 << 20)
            .expect("read")
        else {
            panic!("the newer snapshot from its start");
        };

        assert_eq!(first.parts.len(), BLOBS_A_FRAME_AT_MOST);
        assert_eq!(reading.next, 4096);
        assert_eq!(rest.parts.len(), 1);
        assert_eq!(rest.next, None);
        assert_eq!(replaced.through, 3);
        assert_eq!(replaced.parts.len(), BLOBS_A_FRAME_AT_MOST);
    }

    #[test]
    fn a_reader_behind_a_compaction_is_handed_the_snapshot_rather_than_what_is_left() {
        let mut store = claimed();

        store
            .append(
                SPACE,
                DESK,
                &(1..=10).map(|seq| vec![seq; 10]).collect::<Vec<_>>(),
                1 << 20,
            )
            .expect("appended");
        store
            .compact(SPACE, 8, &[vec![9; 10]], 1 << 20)
            .expect("compacted");

        let behind = store.catch_up(SPACE, 3, None, 10, 1 << 20).expect("read");
        let level = store.catch_up(SPACE, 8, None, 10, 1 << 20).expect("read");

        assert!(matches!(
            behind,
            CaughtUp::Snapshot(Snapshot { through: 8, .. })
        ));

        let CaughtUp::Records(left) = level else {
            panic!("records past the snapshot");
        };

        assert_eq!(
            left.iter().map(|record| record.seq).collect::<Vec<_>>(),
            [9, 10]
        );
    }

    #[test]
    fn a_space_untouched_too_long_is_swept_away() {
        let mut store = claimed();

        store
            .append(SPACE, DESK, &[vec![1]], 1 << 20)
            .expect("appended");

        assert_eq!(store.sweep(2).expect("swept"), 1);
        assert_eq!(
            store
                .claim(SPACE, &[4; 32], 3, || true)
                .expect("claimed anew"),
            Claim::Made
        );
    }

    #[test]
    fn a_store_at_its_space_cap_makes_no_more_but_still_serves_the_ones_it_has() {
        let mut store = Store::in_memory().expect("a store").limited(Capacity {
            spaces: 1,
            bytes: u64::MAX,
        });

        assert_eq!(
            store.claim(SPACE, &[3; 32], 1, || true).expect("made"),
            Claim::Made
        );
        assert_eq!(
            store
                .claim(SpaceId([9; 16]), &[3; 32], 2, || true)
                .expect("full"),
            Claim::Full
        );
        assert_eq!(
            store.claim(SPACE, &[3; 32], 3, || true).expect("held"),
            Claim::Held
        );
    }

    #[test]
    fn a_throttled_claim_makes_no_space_and_a_held_one_is_never_throttled() {
        let mut store = Store::in_memory().expect("a store");

        assert_eq!(
            store
                .claim(SPACE, &[3; 32], 1, || false)
                .expect("throttled"),
            Claim::Throttled
        );
        assert_eq!(
            store.claim(SPACE, &[3; 32], 2, || true).expect("made"),
            Claim::Made
        );
        assert_eq!(
            store.claim(SPACE, &[3; 32], 3, || false).expect("held"),
            Claim::Held
        );
    }

    #[test]
    fn all_the_spaces_together_hold_no_more_than_the_total_cap() {
        let mut store = Store::in_memory().expect("a store").limited(Capacity {
            spaces: u64::MAX,
            bytes: 15,
        });

        store.claim(SPACE, &[3; 32], 1, || true).expect("made");
        store
            .claim(SpaceId([9; 16]), &[3; 32], 1, || true)
            .expect("made");

        assert_eq!(
            store
                .append(SPACE, DESK, &[vec![0; 10]], 1 << 20)
                .expect("appended"),
            Appended::Through(1)
        );
        assert_eq!(
            store
                .append(SpaceId([9; 16]), DESK, &[vec![0; 10]], 1 << 20)
                .expect("refused"),
            Appended::Full
        );
    }

    #[test]
    fn a_compaction_that_grows_a_space_is_weighed_against_the_total_cap_as_an_append_is() {
        let mut store = Store::in_memory().expect("a store").limited(Capacity {
            spaces: u64::MAX,
            bytes: 30,
        });

        store.claim(SPACE, &[3; 32], 1, || true).expect("made");
        store
            .append(SPACE, DESK, &[vec![1; 10], vec![2; 10]], 1 << 20)
            .expect("appended");

        let refused = store
            .compact(SPACE, 1, &[vec![9; 25]], 1 << 20)
            .expect("refused");
        let kept = store.head(SPACE).expect("head");
        let fits = store
            .compact(SPACE, 1, &[vec![9; 15]], 1 << 20)
            .expect("compacted");

        assert_eq!(refused, Compacted::Full);
        assert_eq!(kept, (2, 0));
        assert_eq!(fits, Compacted::Through(1));
    }

    #[test]
    fn a_compaction_that_shrinks_a_space_is_never_refused_by_a_total_already_full() {
        let mut store = Store::in_memory().expect("a store").limited(Capacity {
            spaces: u64::MAX,
            bytes: 20,
        });

        store.claim(SPACE, &[3; 32], 1, || true).expect("made");
        store
            .append(SPACE, DESK, &[vec![1; 10], vec![2; 10]], 1 << 20)
            .expect("appended");

        let shrunk = store
            .compact(SPACE, 2, &[vec![9; 5]], 1 << 20)
            .expect("compacted");

        assert_eq!(shrunk, Compacted::Through(2));
    }

    #[test]
    fn a_space_is_shared_once_a_viewer_is_allowed_and_not_before() {
        let mut store = claimed();

        let before = store.is_shared(SPACE).expect("read");

        store.allow_viewers(SPACE, &[6; 32]).expect("allowed");

        let after = store.is_shared(SPACE).expect("read");

        assert!(!before);
        assert!(after);
        assert!(!store.is_shared(SpaceId([8; 16])).expect("read"));
    }

    #[test]
    fn a_space_never_written_to_is_swept_once_old_enough_and_a_used_one_stays() {
        let mut store = claimed();

        store
            .claim(SpaceId([9; 16]), &[3; 32], 1, || true)
            .expect("made");
        store
            .append(SpaceId([9; 16]), DESK, &[vec![1]], 1 << 20)
            .expect("appended");

        assert_eq!(store.sweep_unused(0).expect("nothing yet"), 0);
        assert_eq!(store.sweep_unused(10).expect("swept"), 1);
        assert_eq!(store.head(SpaceId([9; 16])).expect("head"), (1, 0));
        assert_eq!(store.head(SPACE).expect("gone"), (0, 0));
    }
}
