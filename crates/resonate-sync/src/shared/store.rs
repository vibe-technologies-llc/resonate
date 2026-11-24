use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::{
    Clock, DeviceId, Error, LocalPlaylistId, Result, SpaceId, Stamp, StoreOp, Welcomed,
    error::Malformed,
    key::{DEVICE_BYTES, random_bytes},
    shared::{
        Access,
        moved::Personal,
        op::{
            ENTRY_BYTES, EntryId, MEMBER_BYTES, MemberId, SharedList, SharedOp, decoded_kept_ops,
            encoded_ops,
        },
    },
    wall_nanos,
    wire::Incarnation,
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
    member        BLOB NOT NULL,
    personal      BLOB NOT NULL,
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

const A_LIST_KNOWS_ITS_SPACE_BY_ITS_INCARNATION: &str = "
ALTER TABLE lists ADD COLUMN incarnation INTEGER;
";

const MIGRATIONS: &[&str] = &[V1, A_LIST_KNOWS_ITS_SPACE_BY_ITS_INCARNATION];

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
    pub me: Me,
    pub at: Option<LocalPlaylistId>,
    pub alias: Option<String>,
    pub seen: Option<Vec<u8>>,
    pub cursor: u64,
    pub incarnation: Option<Incarnation>,
    pub list: SharedList,
    pub written: Vec<EntryId>,
    pub records_since: u64,
    pub bytes_since: u64,
    pub leaving: bool,
    pub owed: Vec<(i64, Vec<SharedOp>)>,
    pub clock: Clock,
}

impl Kept {
    pub fn stamped(&mut self) -> Stamp {
        self.clock.now(self.device)
    }

    pub fn owe(&mut self, ops: Vec<SharedOp>) {
        let seq = self.owed.last().map_or(1, |(seq, _)| seq + 1);

        self.owed.push((seq, ops));
    }

    pub fn welcomed(&mut self, head: u64, told: Option<Incarnation>) -> Welcomed {
        let another_incarnation =
            matches!((self.incarnation, told), (Some(kept), Some(told)) if kept != told);
        let welcomed = if head < self.cursor || another_incarnation {
            Welcomed::MadeAnew
        } else {
            Welcomed::Continued
        };

        match welcomed {
            Welcomed::MadeAnew if self.access.may_edit() => self.owe_everything(),
            Welcomed::MadeAnew => self.read_from_the_start(),
            Welcomed::Continued => {}
        }
        if told.is_some() {
            self.incarnation = told;
        }

        welcomed
    }

    pub fn owe_everything(&mut self) {
        let whole = self.list.as_ops();

        self.read_from_the_start();
        self.owe(whole);
    }

    fn read_from_the_start(&mut self) {
        self.cursor = 0;
        self.records_since = 0;
        self.bytes_since = 0;
        self.owed.clear();
    }

    pub fn restamped_past_the_bound(&mut self, wall: i64) -> bool {
        let past: BTreeSet<Stamp> = self
            .list
            .stamps_mut()
            .map(|stamp| *stamp)
            .chain(stamps_owed(&mut self.owed).map(|stamp| *stamp))
            .filter(|stamp| stamp.too_far_ahead_of(wall))
            .collect();

        if past.is_empty() {
            return false;
        }

        let brought_back: BTreeMap<Stamp, Stamp> = past
            .into_iter()
            .zip(wall..)
            .map(|(stamp, nanos)| (stamp, Stamp { nanos, ..stamp }))
            .collect();
        let before = self.list.as_ops();

        for stamp in self.list.stamps_mut().chain(stamps_owed(&mut self.owed)) {
            if let Some(back) = brought_back.get(stamp) {
                *stamp = *back;
            }
        }

        let restamped: Vec<SharedOp> = self
            .list
            .as_ops()
            .into_iter()
            .zip(before)
            .filter_map(|(after, before)| (after != before).then_some(after))
            .collect();

        if let Some(latest) = brought_back.values().max() {
            self.clock.saw(*latest);
        }
        if self.access.may_edit() && !restamped.is_empty() {
            self.owe(restamped);
        }
        tracing::info!(
            stamps = brought_back.len(),
            "a shared playlist's stamps past the wall clock were brought back to it"
        );

        true
    }
}

fn stamps_owed(owed: &mut [(i64, Vec<SharedOp>)]) -> impl Iterator<Item = &mut Stamp> {
    owed.iter_mut()
        .flat_map(|(_, ops)| ops.iter_mut().map(SharedOp::stamp_mut))
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

    pub fn lists(&mut self) -> Result<Vec<Kept>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT space, code, server, device, local_id, local_created, alias, seen, cursor,
                        state, written, records_since, bytes_since, leaving, member, personal,
                        incarnation
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
                    member: row.get(14)?,
                    personal: row.get(15)?,
                    incarnation: row.get(16)?,
                })
            })
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Read, source))?;

        drop(statement);

        let mut lists = rows
            .into_iter()
            .filter_map(|raw| match self.read(raw) {
                Ok(kept) => Some(Ok(kept)),
                Err(Error::Code(_)) => {
                    tracing::warn!("a shared playlist's code no longer reads and was passed over");
                    None
                }
                Err(error) => Some(Err(error)),
            })
            .collect::<Result<Vec<_>>>()?;

        let wall = wall_nanos();

        for kept in &mut lists {
            if kept.restamped_past_the_bound(wall) {
                self.keep(kept)?;
            }
        }

        Ok(lists)
    }

    fn read(&self, raw: Raw) -> Result<Kept> {
        let space: [u8; 16] = raw.space.try_into().map_err(|_| malformed())?;
        let device: [u8; DEVICE_BYTES] = raw.device.try_into().map_err(|_| malformed())?;
        let member: [u8; MEMBER_BYTES] = raw.member.try_into().map_err(|_| malformed())?;
        let personal: [u8; 32] = raw.personal.try_into().map_err(|_| malformed())?;
        let mut list = SharedList::default();

        for op in decoded_kept_ops(&raw.state)? {
            list.apply(&op);
        }

        let written = raw
            .written
            .as_chunks::<ENTRY_BYTES>()
            .0
            .iter()
            .copied()
            .map(EntryId)
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
            .map(|(seq, ops)| decoded_kept_ops(&ops).map(|ops| (seq, ops)))
            .collect::<Result<Vec<_>>>()?;

        Ok(Kept {
            space: SpaceId(space),
            access: Access::from_code(&raw.code)?,
            server: raw.server,
            device: DeviceId(device),
            me: Me {
                member: MemberId(member),
                personal: Personal::from_bytes(personal),
            },
            at: raw
                .local_id
                .zip(raw.local_created)
                .map(|(id, created)| LocalPlaylistId { id, created }),
            alias: raw.alias,
            seen: raw.seen,
            cursor: raw.cursor.cast_unsigned(),
            incarnation: raw
                .incarnation
                .map(|incarnation| Incarnation(incarnation.cast_unsigned())),
            list,
            written,
            records_since: raw.records_since.cast_unsigned(),
            bytes_since: raw.bytes_since.cast_unsigned(),
            leaving: raw.leaving != 0,
            owed,
            clock: Clock::from_last(0),
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
                                    seen, cursor, state, written, records_since, bytes_since, leaving,
                                    member, personal, incarnation)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
                 ON CONFLICT(space) DO UPDATE SET
                    code = excluded.code, server = excluded.server, device = excluded.device,
                    local_id = excluded.local_id, local_created = excluded.local_created,
                    alias = excluded.alias, seen = excluded.seen, cursor = excluded.cursor,
                    state = excluded.state, written = excluded.written,
                    records_since = excluded.records_since, bytes_since = excluded.bytes_since,
                    leaving = excluded.leaving, member = excluded.member, personal = excluded.personal,
                    incarnation = excluded.incarnation",
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
                    kept.me.member.0.as_slice(),
                    kept.me.personal.to_bytes().as_slice(),
                    kept.incarnation.map(|incarnation| incarnation.0.cast_signed()),
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
    member: Vec<u8>,
    personal: Vec<u8>,
    incarnation: Option<i64>,
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
            me: Me {
                member: MemberId([6; MEMBER_BYTES]),
                personal: Personal::minted().expect("a secret"),
            },
            at: Some(LocalPlaylistId { id: 4, created: 5 }),
            alias: Some("Mix (2)".to_owned()),
            seen: Some(vec![7; 32]),
            cursor: 9,
            incarnation: Some(Incarnation(11)),
            list,
            written: vec![EntryId([2; ENTRY_BYTES])],
            records_since: 2,
            bytes_since: 3,
            leaving: false,
            owed: vec![(1, vec![added()])],
            clock: Clock::from_last(0),
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
        assert_eq!(read[0].incarnation, kept.incarnation);
    }

    const CURSOR_OF_BEFORE: u64 = 40;

    #[test]
    fn a_store_of_before_incarnations_keeps_its_lists_and_learns_the_first_incarnation_each_is_told()
     {
        let connection = Connection::open_in_memory().expect("a connection");
        let before = MIGRATIONS
            .iter()
            .position(|step| *step == A_LIST_KNOWS_ITS_SPACE_BY_ITS_INCARNATION)
            .expect("the step is in the chain");
        let listed = a_list();

        for step in &MIGRATIONS[..before] {
            connection.execute_batch(step).expect("the store of before");
        }
        connection
            .execute(
                "INSERT INTO lists (space, code, server, device, member, personal, cursor, state, written)
                 VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    listed.space.0.as_slice(),
                    listed.access.code(),
                    listed.device.0.as_slice(),
                    listed.me.member.0.as_slice(),
                    listed.me.personal.to_bytes().as_slice(),
                    CURSOR_OF_BEFORE.cast_signed(),
                    encoded_ops(&listed.list.as_ops()),
                    [2_u8; ENTRY_BYTES].as_slice(),
                ],
            )
            .expect("a list");
        connection
            .execute(
                "INSERT INTO owed (space, seq, ops) VALUES (?1, 1, ?2)",
                params![listed.space.0.as_slice(), encoded_ops(&[added()])],
            )
            .expect("an op owed");
        connection
            .pragma_update(
                None,
                "user_version",
                u32::try_from(before).expect("a count"),
            )
            .expect("stamped");

        let mut store = Store::laid_out(connection).expect("carried forward");

        let mut carried = store.lists().expect("read").remove(0);

        let kept_before = carried.incarnation;
        let first = carried.welcomed(CURSOR_OF_BEFORE, Some(Incarnation(9)));

        store.keep(&carried).expect("kept");

        let read_again = store.lists().expect("read again").remove(0);

        assert_eq!(carried.space, listed.space);
        assert_eq!(carried.list, listed.list);
        assert_eq!(carried.owed, [(1, vec![added()])]);
        assert_eq!(kept_before, None);
        assert_eq!(first, Welcomed::Continued);
        assert_eq!(carried.cursor, CURSOR_OF_BEFORE);
        assert_eq!(read_again.incarnation, Some(Incarnation(9)));
        assert_eq!(read_again.cursor, CURSOR_OF_BEFORE);
        assert_eq!(read_again.owed, [(1, vec![added()])]);
    }

    fn sent_through_the_cursor() -> Kept {
        let mut kept = a_list();

        kept.owed.clear();
        kept.records_since = 4;
        kept.bytes_since = 400;

        kept
    }

    #[test]
    fn a_space_of_another_incarnation_is_made_anew_though_its_head_is_past_the_cursor() {
        let mut kept = sent_through_the_cursor();

        let told_another = kept.welcomed(kept.cursor + 4, Some(Incarnation(12)));
        let owed: Vec<SharedOp> = kept.owed.iter().flat_map(|(_, ops)| ops.clone()).collect();
        let told_again = kept.welcomed(0, Some(Incarnation(12)));

        assert_eq!(told_another, Welcomed::MadeAnew);
        assert_eq!(kept.cursor, 0);
        assert_eq!(kept.records_since, 0);
        assert_eq!(kept.bytes_since, 0);
        assert_eq!(owed, kept.list.as_ops());
        assert_eq!(kept.incarnation, Some(Incarnation(12)));
        assert_eq!(told_again, Welcomed::Continued);
    }

    #[test]
    fn a_server_that_tells_no_incarnation_is_judged_by_its_head_alone_and_the_one_kept_stays() {
        let mut kept = sent_through_the_cursor();

        let cursor = kept.cursor;

        let at_the_cursor = kept.welcomed(cursor, None);
        let owed_at_the_cursor = kept.owed.len();
        let below_the_cursor = kept.welcomed(cursor - 1, None);

        assert_eq!(at_the_cursor, Welcomed::Continued);
        assert_eq!(owed_at_the_cursor, 0);
        assert_eq!(below_the_cursor, Welcomed::MadeAnew);
        assert_eq!(kept.cursor, 0);
        assert_eq!(kept.incarnation, Some(Incarnation(11)));
    }

    #[test]
    fn a_space_of_the_same_incarnation_whose_head_fell_below_the_cursor_is_made_anew() {
        let mut kept = sent_through_the_cursor();

        let fallen = kept.welcomed(kept.cursor - 1, Some(Incarnation(11)));

        assert_eq!(fallen, Welcomed::MadeAnew);
        assert!(!kept.owed.is_empty());
    }

    #[test]
    fn a_view_of_a_space_of_another_incarnation_reads_it_from_its_start_and_owes_nothing() {
        let mut kept = a_list();

        kept.access = kept.access.viewing().expect("a view key");

        let told_another = kept.welcomed(kept.cursor + 4, Some(Incarnation(12)));

        assert_eq!(told_another, Welcomed::MadeAnew);
        assert_eq!(kept.cursor, 0);
        assert_eq!(kept.records_since, 0);
        assert_eq!(kept.bytes_since, 0);
        assert!(kept.owed.is_empty());
        assert_eq!(kept.incarnation, Some(Incarnation(12)));
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

    fn at_the_end_of_time(less: i64) -> Stamp {
        Stamp {
            nanos: i64::MAX - less,
            device: DeviceId([8; DEVICE_BYTES]),
        }
    }

    #[test]
    fn a_list_kept_with_stamps_past_the_wall_clock_is_restamped_as_it_loads_and_takes_later_honest_edits()
     {
        let mut store = Store::in_memory().expect("a store");
        let mut kept = a_list();
        let entry = EntryId([2; ENTRY_BYTES]);

        kept.owed.clear();
        kept.list.apply(&SharedOp::Moved {
            entry,
            place: Place(vec![200]),
            stamp: at_the_end_of_time(0),
        });
        kept.list.apply(&SharedOp::Renamed {
            name: "Theirs".to_owned(),
            stamp: at_the_end_of_time(0),
        });
        store.keep(&kept).expect("kept as an older build kept it");

        let wall = wall_nanos();
        let mut loaded = store.lists().expect("read").remove(0);

        let restamped = loaded.list.clone();
        let owed: Vec<SharedOp> = loaded
            .owed
            .iter()
            .flat_map(|(_, ops)| ops.clone())
            .collect();
        let read_again = store.lists().expect("read again").remove(0);
        let moved = SharedOp::Moved {
            entry,
            place: Place(vec![100]),
            stamp: loaded.stamped(),
        };
        let renamed = SharedOp::Renamed {
            name: "Ours".to_owned(),
            stamp: loaded.stamped(),
        };

        assert_eq!(restamped.rows()[0].place, Place(vec![200]));
        assert_eq!(restamped.name(), Some("Theirs"));
        assert_eq!(owed.len(), 2);
        assert!(owed.iter().map(SharedOp::stamp).all(|stamp| {
            stamp.device == DeviceId([8; DEVICE_BYTES])
                && stamp.nanos >= wall
                && !stamp.too_far_ahead_of(wall)
        }));
        assert_eq!(read_again.list, restamped);
        assert_eq!(read_again.owed, loaded.owed);

        assert!(loaded.list.apply(&moved));
        assert!(loaded.list.apply(&renamed));
        assert_eq!(loaded.list.rows()[0].place, Place(vec![100]));
        assert_eq!(loaded.list.name(), Some("Ours"));
    }

    #[test]
    fn restamping_keeps_the_order_of_the_stamps_it_brings_back_and_a_followed_list_owes_nothing() {
        let wall = wall_nanos();
        let mut kept = a_list();
        let gone = MemberId([4; MEMBER_BYTES]);
        let back = MemberId([5; MEMBER_BYTES]);

        kept.access = kept.access.viewing().expect("a view key");
        kept.owed.clear();
        for op in [
            SharedOp::Member {
                member: gone,
                name: "Gone".to_owned(),
                public: [1; 32],
                stamp: at_the_end_of_time(1),
            },
            SharedOp::Left {
                member: gone,
                stamp: at_the_end_of_time(0),
            },
            SharedOp::Member {
                member: back,
                name: "Back".to_owned(),
                public: [2; 32],
                stamp: at_the_end_of_time(0),
            },
            SharedOp::Left {
                member: back,
                stamp: at_the_end_of_time(1),
            },
        ] {
            kept.list.apply(&op);
        }

        let restamped = kept.restamped_past_the_bound(wall);
        let here: Vec<MemberId> = kept.list.people().map(|(member, _)| member).collect();
        let again = kept.restamped_past_the_bound(wall);

        assert!(restamped);
        assert_eq!(here, [back]);
        assert!(kept.owed.is_empty());
        assert!(!again);
        assert!(kept.stamped().nanos > wall + 1);
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
