use std::{
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::{
    Answer, Asked, Error, Result, Store, StoreOp,
    packed::{KEPT_PACKED_AT, packed, unpacked},
    wire::mbid_bytes,
};

pub const KEPT_FRESH_FOR: Duration = Duration::from_secs(30 * 24 * 60 * 60);
pub const KEPT_ANSWERS_TOLD_AT_ONCE: usize = 512;

const V1: &str = "
CREATE TABLE answers (
    key BLOB PRIMARY KEY,
    at INTEGER NOT NULL,
    seq INTEGER NOT NULL UNIQUE,
    came_from TEXT,
    packed BLOB NOT NULL
);
CREATE TABLE entities (
    mbid BLOB NOT NULL,
    kind INTEGER NOT NULL,
    key BLOB NOT NULL REFERENCES answers(key) ON DELETE CASCADE,
    PRIMARY KEY (mbid, kind)
) WITHOUT ROWID;
CREATE INDEX entities_by_key ON entities(key);
CREATE TABLE marks (
    device TEXT PRIMARY KEY,
    mark INTEGER NOT NULL
) WITHOUT ROWID;
";

const AN_ANSWERS_NUMBER_NEVER_GOES_BACK: &str = "
CREATE TABLE numbered (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    last INTEGER NOT NULL
) STRICT;
INSERT INTO numbered (id, last) SELECT 1, coalesce(max(seq), 0) FROM answers;
";

const MIGRATIONS: &[&str] = &[V1, AN_ANSWERS_NUMBER_NEVER_GOES_BACK];

const A_MARK_NO_DEVICE_REACHES: i64 = i64::MAX / 2;

const THE_NEXT_NUMBER: &str = "UPDATE numbered SET last = last + 1 RETURNING last";

const NUMBERED_PAST_A_MARK: &str = "UPDATE numbered SET last = ?1 WHERE last < ?1 AND ?1 <= ?2";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeptAnswer {
    pub seq: u64,
    pub at: u64,
    pub key: Vec<u8>,
    pub packed: Vec<u8>,
}

pub struct Kept {
    connection: Mutex<Connection>,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

fn signed(seconds: u64) -> i64 {
    i64::try_from(seconds).unwrap_or(i64::MAX)
}

fn fresh_since(now: u64) -> i64 {
    signed(now.saturating_sub(KEPT_FRESH_FOR.as_secs()))
}

impl Kept {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(folder) = path.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let connection =
            Connection::open(path).map_err(Error::store(Store::Kept, StoreOp::Open))?;
        Self::over(connection)
    }

    pub fn in_memory() -> Result<Self> {
        let connection =
            Connection::open_in_memory().map_err(Error::store(Store::Kept, StoreOp::Open))?;
        Self::over(connection)
    }

    fn over(mut connection: Connection) -> Result<Self> {
        let configure = Error::store(Store::Kept, StoreOp::Configure);
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(configure)?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(Error::store(Store::Kept, StoreOp::Configure))?;
        connection
            .pragma_update(None, "foreign_keys", true)
            .map_err(Error::store(Store::Kept, StoreOp::Configure))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(Error::store(Store::Kept, StoreOp::LayOut))?;
        let version: u32 = transaction
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(Error::store(Store::Kept, StoreOp::LayOut))?;
        let laid = usize::try_from(version).unwrap_or(usize::MAX);
        if laid > MIGRATIONS.len() {
            return Err(Error::KeptVersion { found: version });
        }
        for step in &MIGRATIONS[laid..] {
            transaction
                .execute_batch(step)
                .map_err(Error::store(Store::Kept, StoreOp::LayOut))?;
        }
        transaction
            .pragma_update(
                None,
                "user_version",
                u32::try_from(MIGRATIONS.len()).unwrap_or(u32::MAX),
            )
            .map_err(Error::store(Store::Kept, StoreOp::LayOut))?;
        transaction
            .execute("DELETE FROM answers WHERE at < ?1", [fresh_since(now())])
            .map_err(Error::store(Store::Kept, StoreOp::Write))?;
        transaction
            .commit()
            .map_err(Error::store(Store::Kept, StoreOp::LayOut))?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn answer(&self, asked: &Asked) -> Result<Option<Answer>> {
        self.answer_at(asked, now())
    }

    pub fn answer_at(&self, asked: &Asked, now: u64) -> Result<Option<Answer>> {
        let connection = self.connection.lock();
        let read = Error::store(Store::Kept, StoreOp::Read);
        let kept: Option<Vec<u8>> = connection
            .query_row(
                "SELECT packed FROM answers WHERE key = ?1 AND at >= ?2",
                params![asked.key(), fresh_since(now)],
                |row| row.get(0),
            )
            .optional()
            .map_err(read)?;
        if let Some(packed) = kept {
            return Ok(Some(answer_of(&packed)?));
        }
        let Some((entity, id)) = asked.names() else {
            return Ok(None);
        };
        let within: Option<Vec<u8>> = connection
            .query_row(
                "SELECT a.packed FROM entities e JOIN answers a ON a.key = e.key \
                 WHERE e.mbid = ?1 AND e.kind = ?2 AND a.at >= ?3",
                params![mbid_bytes(id).to_vec(), entity.code(), fresh_since(now)],
                |row| row.get(0),
            )
            .optional()
            .map_err(Error::store(Store::Kept, StoreOp::Read))?;
        match within {
            Some(packed) => Ok(answer_of(&packed)?.entity(entity, id)),
            None => Ok(None),
        }
    }

    pub fn keep(&self, asked: &Asked, answer: &Answer) -> Result<bool> {
        self.keep_at(asked, answer, now(), None)
    }

    fn keep_at(
        &self,
        asked: &Asked,
        answer: &Answer,
        at: u64,
        came_from: Option<&str>,
    ) -> Result<bool> {
        if answer.is_a_miss() || !asked.is_answered_by(answer) {
            return Ok(false);
        }
        let squeezed = packed(&answer.encoded(), KEPT_PACKED_AT)?;
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(Error::store(Store::Kept, StoreOp::Write))?;
        let kept = written(&transaction, &asked.key(), at, came_from, &squeezed, answer)?;
        transaction
            .commit()
            .map_err(Error::store(Store::Kept, StoreOp::Write))?;
        Ok(kept)
    }

    pub fn after(&self, seq: u64, most: usize, asker: &str) -> Result<Vec<KeptAnswer>> {
        let connection = self.connection.lock();
        connection
            .execute(
                NUMBERED_PAST_A_MARK,
                params![signed(seq), A_MARK_NO_DEVICE_REACHES],
            )
            .map_err(Error::store(Store::Kept, StoreOp::Write))?;
        let mut statement = connection
            .prepare_cached(
                "SELECT seq, at, key, packed FROM answers \
                 WHERE seq > ?1 AND at >= ?2 AND (came_from IS NULL OR came_from <> ?3) \
                 ORDER BY seq LIMIT ?4",
            )
            .map_err(Error::store(Store::Kept, StoreOp::Read))?;
        let rows = statement
            .query_map(
                params![
                    signed(seq),
                    fresh_since(now()),
                    asker,
                    i64::try_from(most).unwrap_or(i64::MAX)
                ],
                |row| {
                    Ok(KeptAnswer {
                        seq: row.get::<_, i64>(0)?.try_into().unwrap_or(0),
                        at: row.get::<_, i64>(1)?.try_into().unwrap_or(0),
                        key: row.get(2)?,
                        packed: row.get(3)?,
                    })
                },
            )
            .map_err(Error::store(Store::Kept, StoreOp::Read))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Error::store(Store::Kept, StoreOp::Read))
    }

    pub fn last_seq(&self) -> Result<u64> {
        let connection = self.connection.lock();
        let last: i64 = connection
            .query_row("SELECT last FROM numbered", [], |row| row.get(0))
            .map_err(Error::store(Store::Kept, StoreOp::Read))?;
        Ok(u64::try_from(last).unwrap_or(0))
    }

    pub fn mark_of(&self, device: &str) -> Result<u64> {
        let connection = self.connection.lock();
        let mark: Option<i64> = connection
            .query_row(
                "SELECT mark FROM marks WHERE device = ?1",
                [device],
                |row| row.get(0),
            )
            .optional()
            .map_err(Error::store(Store::Kept, StoreOp::Read))?;
        Ok(mark.and_then(|mark| u64::try_from(mark).ok()).unwrap_or(0))
    }

    pub fn take(&self, device: &str, answers: &[KeptAnswer], mark: u64) -> Result<usize> {
        let since = fresh_since(now());
        let mut taken = 0;
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(Error::store(Store::Kept, StoreOp::Write))?;
        for told in answers {
            if signed(told.at) < since {
                continue;
            }
            let read =
                Asked::decoded(&told.key).and_then(|asked| Ok((asked, answer_of(&told.packed)?)));
            let (asked, answer) = match read {
                Ok(read) => read,
                Err(error) => {
                    tracing::debug!(%error, "an answer another device kept could not be read");
                    continue;
                }
            };
            if asked.key() != told.key || !asked.is_answered_by(&answer) || answer.is_a_miss() {
                tracing::debug!("an answer another device kept does not answer what it names");
                continue;
            }
            if written(
                &transaction,
                &told.key,
                told.at,
                Some(device),
                &told.packed,
                &answer,
            )? {
                taken += 1;
            }
        }
        transaction
            .execute(
                "INSERT INTO marks (device, mark) VALUES (?1, ?2) \
                 ON CONFLICT (device) DO UPDATE SET mark = max(mark, excluded.mark)",
                params![device, signed(mark)],
            )
            .map_err(Error::store(Store::Kept, StoreOp::Write))?;
        transaction
            .commit()
            .map_err(Error::store(Store::Kept, StoreOp::Write))?;
        Ok(taken)
    }

    pub fn counted(&self) -> Result<u64> {
        let connection = self.connection.lock();
        let counted: i64 = connection
            .query_row("SELECT count(*) FROM answers", [], |row| row.get(0))
            .map_err(Error::store(Store::Kept, StoreOp::Read))?;
        Ok(u64::try_from(counted).unwrap_or(0))
    }
}

fn answer_of(packed: &[u8]) -> Result<Answer> {
    Answer::decoded(&unpacked(packed)?)
}

fn written(
    transaction: &rusqlite::Transaction<'_>,
    key: &[u8],
    at: u64,
    came_from: Option<&str>,
    packed: &[u8],
    answer: &Answer,
) -> Result<bool> {
    let write = || Error::store(Store::Kept, StoreOp::Write);
    let held: Option<i64> = transaction
        .query_row("SELECT at FROM answers WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()
        .map_err(write())?;
    if held.is_some_and(|held| held >= signed(at)) {
        return Ok(false);
    }
    let seq: i64 = transaction
        .query_row(THE_NEXT_NUMBER, [], |row| row.get(0))
        .map_err(write())?;
    transaction
        .execute("DELETE FROM answers WHERE key = ?1", [key])
        .map_err(write())?;
    transaction
        .execute(
            "INSERT INTO answers (key, at, seq, came_from, packed) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![key, signed(at), seq, came_from, packed],
        )
        .map_err(write())?;
    for (entity, id) in answer.entities() {
        transaction
            .execute(
                "INSERT INTO entities (mbid, kind, key) VALUES (?1, ?2, ?3) \
                 ON CONFLICT (mbid, kind) DO UPDATE SET key = excluded.key",
                params![mbid_bytes(&id).to_vec(), entity.code(), key],
            )
            .map_err(write())?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kept_at_the_first_shape(newest: i64) -> Connection {
        let connection = Connection::open_in_memory().expect("a store in memory");
        connection.execute_batch(V1).expect("the first shape");
        connection
            .pragma_update(None, "user_version", 1)
            .expect("the first shape's stamp");
        connection
            .execute(
                "INSERT INTO answers (key, at, seq, came_from, packed) VALUES (?1, ?2, ?3, NULL, ?4)",
                params![vec![1_u8], signed(now()), newest, vec![0_u8]],
            )
            .expect("an answer of the first shape");
        connection
    }

    #[test]
    fn answers_kept_before_they_were_numbered_go_on_from_the_newest_number() {
        let kept = Kept::over(kept_at_the_first_shape(5)).expect("carried forward");

        let last = kept.last_seq().expect("read");
        let next: i64 = kept
            .connection
            .lock()
            .query_row(THE_NEXT_NUMBER, [], |row| row.get(0))
            .expect("numbered");

        assert_eq!(last, 5);
        assert_eq!(next, 6);
        assert_eq!(kept.counted().expect("counted"), 1);
    }
}
