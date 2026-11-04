use std::path::Path;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::{
    DeviceId, Error, LocalPlaylistId, Result, SpaceId, StoreOp,
    error::Malformed,
    key::{DEVICE_BYTES, random_bytes},
    shared::{
        Access,
        moved::Personal,
        op::{
            ENTRY_BYTES, EntryId, MEMBER_BYTES, MemberId, SharedList, SharedOp, decoded_ops,
            encoded_ops,
        },
    },
};

const V1: &str = "
CREATE TABLE me (
    only     INTEGER PRIMARY KEY CHECK (only = 1),
    member   BLOB NOT NULL,
    personal BLOB NOT NULL
) STRICT;
CREATE TABLE lists (
    space         BLOB PRIMARY KEY,
    code          TEXT NOT NULL,
    server        TEXT,
    device        BLOB NOT NULL,
    local_id      INTEGER,
    local_created INTEGER,
    alias         TEXT,
    seen          BLOB,
    cursor        INTEGER NOT NULL DEFAULT 0,
    state         BLOB NOT NULL,
    written       BLOB NOT NULL,
    records_since INTEGER NOT NULL DEFAULT 0,
    bytes_since   INTEGER NOT NULL DEFAULT 0,
    leaving       INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE owed (
    space BLOB NOT NULL REFERENCES lists(space) ON DELETE CASCADE,
    seq   INTEGER NOT NULL,
    ops   BLOB NOT NULL,
    PRIMARY KEY (space, seq)
) STRICT, WITHOUT ROWID;
";

const MIGRATIONS: &[&str] = &[V1];

pub struct Store {
    connection: Connection,
}

#[derive(Clone)]
pub struct Me {
    pub member: MemberId,
    pub personal: Personal,
}

#[derive(Clone)]
pub struct Kept {
    pub space: SpaceId,
    pub access: Access,
    pub server: Option<String>,
    pub device: DeviceId,
    pub at: Option<LocalPlaylistId>,
    pub alias: Option<String>,
    pub seen: Option<Vec<u8>>,
    pub cursor: u64,
    pub list: SharedList,
    pub written: Vec<EntryId>,
    pub records_since: u64,
    pub bytes_since: u64,
    pub leaving: bool,
    pub owed: Vec<(i64, Vec<SharedOp>)>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        Self::laid_out(
            Connection::open(path).map_err(|source| Error::store(StoreOp::Open, source))?,
        )
    }

    pub fn in_memory() -> Result<Self> {
        Self::laid_out(
            Connection::open_in_memory().map_err(|source| Error::store(StoreOp::Open, source))?,
        )
    }

    fn laid_out(mut connection: Connection) -> Result<Self> {
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
            return Err(Error::LedgerVersion { found: version });
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

    pub fn me(&self) -> Result<Me> {
        let held: Option<(Vec<u8>, Vec<u8>)> = self
            .connection
            .query_row("SELECT member, personal FROM me", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        if let Some((member, personal)) = held {
            let member: [u8; MEMBER_BYTES] = member.try_into().map_err(|_| malformed())?;
            let personal: [u8; 32] = personal.try_into().map_err(|_| malformed())?;

            return Ok(Me {
                member: MemberId(member),
                personal: Personal::from_bytes(personal),
            });
        }

        let me = Me {
            member: MemberId(random_bytes()?),
            personal: Personal::minted()?,
        };
        self.keep_me(&me)?;

        Ok(me)
    }

    pub fn keep_me(&self, me: &Me) -> Result<()> {
        self.connection
            .execute(
                "INSERT INTO me (only, member, personal) VALUES (1, ?1, ?2)
                 ON CONFLICT(only) DO UPDATE SET member = excluded.member, personal = excluded.personal",
                params![me.member.0.as_slice(), me.personal.to_bytes().as_slice()],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;

        Ok(())
    }

    pub fn lists(&self) -> Result<Vec<Kept>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT space, code, server, device, local_id, local_created, alias, seen, cursor,
                        state, written, records_since, bytes_since, leaving
                   FROM lists",
            )
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let rows = statement
            .query_map([], |row| {
                Ok(Raw {
                    space: row.get(0)?,
                    code: row.get(1)?,
                    server: row.get(2)?,
                    device: row.get(3)?,
                    local_id: row.get(4)?,
                    local_created: row.get(5)?,
                    alias: row.get(6)?,
                    seen: row.get(7)?,
                    cursor: row.get(8)?,
                    state: row.get(9)?,
                    written: row.get(10)?,
                    records_since: row.get(11)?,
                    bytes_since: row.get(12)?,
                    leaving: row.get(13)?,
                })
            })
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Read, source))?;

        rows.into_iter()
            .filter_map(|raw| match self.read(raw) {
                Ok(kept) => Some(Ok(kept)),
                Err(Error::Code(_)) => {
                    tracing::warn!("a shared playlist's code no longer reads and was passed over");
                    None
                }
                Err(error) => Some(Err(error)),
            })
            .collect()
    }

    fn read(&self, raw: Raw) -> Result<Kept> {
        let space: [u8; 16] = raw.space.try_into().map_err(|_| malformed())?;
        let device: [u8; DEVICE_BYTES] = raw.device.try_into().map_err(|_| malformed())?;
        let mut list = SharedList::default();
        for op in decoded_ops(&raw.state)? {
            list.apply(&op);
        }
        let written = raw
            .written
            .chunks_exact(ENTRY_BYTES)
            .filter_map(|bytes| bytes.try_into().ok().map(EntryId))
            .collect();

        let mut statement = self
            .connection
            .prepare("SELECT seq, ops FROM owed WHERE space = ?1 ORDER BY seq")
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let owed = statement
            .query_map(params![space.as_slice()], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Read, source))?
            .into_iter()
            .map(|(seq, ops)| decoded_ops(&ops).map(|ops| (seq, ops)))
            .collect::<Result<Vec<_>>>()?;

        Ok(Kept {
            space: SpaceId(space),
            access: Access::from_code(&raw.code)?,
            server: raw.server,
            device: DeviceId(device),
            at: raw
                .local_id
                .zip(raw.local_created)
                .map(|(id, created)| LocalPlaylistId { id, created }),
            alias: raw.alias,
            seen: raw.seen,
            cursor: raw.cursor.cast_unsigned(),
            list,
            written,
            records_since: raw.records_since.cast_unsigned(),
            bytes_since: raw.bytes_since.cast_unsigned(),
            leaving: raw.leaving != 0,
            owed,
        })
    }

    pub fn keep(&mut self, kept: &Kept) -> Result<()> {
        let written: Vec<u8> = kept.written.iter().flat_map(|entry| entry.0).collect();
        let state = encoded_ops(&kept.list.as_ops());
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        transaction
            .execute(
                "INSERT INTO lists (space, code, server, device, local_id, local_created, alias,
                                    seen, cursor, state, written, records_since, bytes_since, leaving)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                 ON CONFLICT(space) DO UPDATE SET
                    code = excluded.code, server = excluded.server, device = excluded.device,
                    local_id = excluded.local_id, local_created = excluded.local_created,
                    alias = excluded.alias, seen = excluded.seen, cursor = excluded.cursor,
                    state = excluded.state, written = excluded.written,
                    records_since = excluded.records_since, bytes_since = excluded.bytes_since,
                    leaving = excluded.leaving",
                params![
                    kept.space.0.as_slice(),
                    kept.access.code(),
                    kept.server,
                    kept.device.0.as_slice(),
                    kept.at.map(|at| at.id),
                    kept.at.map(|at| at.created),
                    kept.alias,
                    kept.seen,
                    kept.cursor.cast_signed(),
                    state,
                    written,
                    kept.records_since.cast_signed(),
                    kept.bytes_since.cast_signed(),
                    i64::from(kept.leaving),
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        transaction
            .execute(
                "DELETE FROM owed WHERE space = ?1",
                params![kept.space.0.as_slice()],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        for (seq, ops) in &kept.owed {
            transaction
                .execute(
                    "INSERT INTO owed (space, seq, ops) VALUES (?1, ?2, ?3)",
                    params![kept.space.0.as_slice(), seq, encoded_ops(ops)],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Write, source))
    }

    pub fn forget(&self, space: SpaceId) -> Result<()> {
        self.connection
            .execute(
                "DELETE FROM lists WHERE space = ?1",
                params![space.0.as_slice()],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;

        Ok(())
    }
}

struct Raw {
    space: Vec<u8>,
    code: String,
    server: Option<String>,
    device: Vec<u8>,
    local_id: Option<i64>,
    local_created: Option<i64>,
    alias: Option<String>,
    seen: Option<Vec<u8>>,
    cursor: i64,
    state: Vec<u8>,
    written: Vec<u8>,
    records_since: i64,
    bytes_since: i64,
    leaving: i64,
}

const fn malformed() -> Error {
    Error::Malformed(Malformed::Number)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use resonate_core::Recording;

    use super::*;
    use crate::{Stamp, shared::place::Place};

    fn stamp() -> Stamp {
        Stamp {
            nanos: 5,
            device: DeviceId([1; DEVICE_BYTES]),
        }
    }

    fn added() -> SharedOp {
        SharedOp::Added {
            entry: EntryId([2; ENTRY_BYTES]),
            recording: Recording {
                mbid: None,
                title: "Echoes".to_owned(),
                artist: "Pink Floyd".to_owned(),
                album: "Meddle".to_owned(),
                length: Some(Duration::from_secs(1_400)),
            },
            place: Place::first(),
            by: None,
            stamp: stamp(),
        }
    }

    fn a_list() -> Kept {
        let access = Access::minted().expect("a key");
        let space = access.keys().expect("derived").space;
        let mut list = SharedList::default();
        list.apply(&added());

        Kept {
            space,
            access,
            server: Some("wss://elsewhere.example/".to_owned()),
            device: DeviceId([3; DEVICE_BYTES]),
            at: Some(LocalPlaylistId { id: 4, created: 5 }),
            alias: Some("Mix (2)".to_owned()),
            seen: Some(vec![7; 32]),
            cursor: 9,
            list,
            written: vec![EntryId([2; ENTRY_BYTES])],
            records_since: 2,
            bytes_since: 3,
            leaving: false,
            owed: vec![(1, vec![added()])],
        }
    }

    #[test]
    fn a_list_kept_reads_back_with_what_it_owes() {
        let mut store = Store::in_memory().expect("a store");
        let kept = a_list();

        store.keep(&kept).expect("kept");
        let read = store.lists().expect("read");

        assert_eq!(read.len(), 1);
        assert_eq!(read[0].space, kept.space);
        assert_eq!(read[0].list, kept.list);
        assert_eq!(read[0].written, kept.written);
        assert_eq!(read[0].owed, kept.owed);
        assert_eq!(read[0].at, kept.at);
        assert_eq!(read[0].alias, kept.alias);
        assert_eq!(read[0].server, kept.server);
    }

    #[test]
    fn a_list_kept_again_replaces_itself_and_a_list_forgotten_takes_what_it_owed() {
        let mut store = Store::in_memory().expect("a store");
        let mut kept = a_list();

        store.keep(&kept).expect("kept");
        kept.owed.clear();
        kept.cursor = 10;
        store.keep(&kept).expect("kept again");
        let read = store.lists().expect("read");

        assert_eq!(read.len(), 1);
        assert_eq!(read[0].cursor, 10);
        assert!(read[0].owed.is_empty());

        store.forget(kept.space).expect("forgotten");
        assert!(store.lists().expect("read").is_empty());
    }

    #[test]
    fn who_i_am_is_minted_once_and_kept() {
        let store = Store::in_memory().expect("a store");

        let first = store.me().expect("minted");
        let again = store.me().expect("read");

        assert_eq!(first.member, again.member);
        assert_eq!(first.personal.public(), again.personal.public());
    }
}
