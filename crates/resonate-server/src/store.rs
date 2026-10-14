use std::{path::Path, time::Duration};

use resonate_sync::{DeviceId, SpaceId};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::{Error, Result, StoreOp};

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

const MIGRATIONS: &[&str] = &[V1];

pub struct Store {
    connection: Connection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Claim {
    Made,
    Held,
    Refused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appended {
    Through(u64),
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compacted {
    Through(u64),
    Stale,
    Full,
}

pub struct Stored {
    pub seq: u64,
    pub device: DeviceId,
    pub blob: Vec<u8>,
}

pub struct Snapshot {
    pub through: u64,
    pub parts: Vec<Vec<u8>>,
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
        Ok(Self { connection })
    }

    pub fn claim(&mut self, space: SpaceId, token: &[u8; 32], now: i64) -> Result<Claim> {
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
            None => {
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
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        let (head, bytes): (i64, i64) = transaction
            .query_row(
                "SELECT head, bytes FROM spaces WHERE id = ?1",
                params![space.0.as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let adding = blobs.iter().map(Vec::len).sum::<usize>();
        let adding = i64::try_from(adding).unwrap_or(i64::MAX);
        if bytes.saturating_add(adding).cast_unsigned() > space_bytes {
            return Ok(Appended::Full);
        }
        let mut seq = head;
        for blob in blobs {
            seq += 1;
            transaction
                .execute(
                    "INSERT INTO records (space, seq, device, blob) VALUES (?1, ?2, ?3, ?4)",
                    params![space.0.as_slice(), seq, device.0.as_slice(), blob],
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

    pub fn snapshot(&self, space: SpaceId) -> Result<Option<Snapshot>> {
        let (_, through) = self.head(space)?;
        if through == 0 {
            return Ok(None);
        }
        let mut statement = self
            .connection
            .prepare_cached("SELECT blob FROM snapshots WHERE space = ?1 ORDER BY part")
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let parts = statement
            .query_map(params![space.0.as_slice()], |row| row.get::<_, Vec<u8>>(0))
            .map_err(|source| Error::store(StoreOp::Read, source))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        Ok(Some(Snapshot { through, parts }))
    }

    pub fn compact(
        &mut self,
        space: SpaceId,
        through: u64,
        parts: &[Vec<u8>],
        space_bytes: u64,
    ) -> Result<Compacted> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        let (head, snapshot_through): (i64, i64) = transaction
            .query_row(
                "SELECT head, snapshot_through FROM spaces WHERE id = ?1",
                params![space.0.as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|source| Error::store(StoreOp::Read, source))?;
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
        for (part, blob) in parts.iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO snapshots (space, part, blob) VALUES (?1, ?2, ?3)",
                    params![
                        space.0.as_slice(),
                        i64::try_from(part).unwrap_or(i64::MAX),
                        blob
                    ],
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

    pub fn sweep(&mut self, untouched_since: i64) -> Result<usize> {
        self.connection
            .execute(
                "DELETE FROM spaces WHERE touched < ?1",
                params![untouched_since],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))
    }
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
            store.claim(SPACE, &[3; 32], 1).expect("claimed"),
            Claim::Made
        );
        store
    }

    #[test]
    fn a_space_is_claimed_by_its_first_token_and_refuses_any_other() {
        let mut store = claimed();

        assert_eq!(store.claim(SPACE, &[3; 32], 2).expect("held"), Claim::Held);
        assert_eq!(
            store.claim(SPACE, &[4; 32], 3).expect("refused"),
            Claim::Refused
        );
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
                .append(SPACE, DESK, &[vec![0; 10]], 15)
                .expect("refused"),
            Appended::Full
        );
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

    #[test]
    fn a_space_untouched_too_long_is_swept_away() {
        let mut store = claimed();
        store
            .append(SPACE, DESK, &[vec![1]], 1 << 20)
            .expect("appended");

        assert_eq!(store.sweep(2).expect("swept"), 1);
        assert_eq!(
            store.claim(SPACE, &[4; 32], 3).expect("claimed anew"),
            Claim::Made
        );
    }
}
