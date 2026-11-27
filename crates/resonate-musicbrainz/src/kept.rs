use std::{
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use crate::{
    Answer, Asked, Error, Result, Store, StoreOp,
    packed::{KEPT_PACKED_AT, packed, unpacked},
};

#[cfg(test)]
mod released;

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

const AN_ANSWERS_AGE_IS_READ_OFF_AN_INDEX: &str = "
CREATE INDEX answers_by_age ON answers(at);
";

const A_LOOKUP_BY_ID_IS_ANSWERED_BY_ITS_OWN_ANSWER_ALONE: &str = "
DROP TABLE entities;
";

const MIGRATIONS: &[&str] = &[
    V1,
    AN_ANSWERS_NUMBER_NEVER_GOES_BACK,
    AN_ANSWERS_AGE_IS_READ_OFF_AN_INDEX,
    A_LOOKUP_BY_ID_IS_ANSWERED_BY_ITS_OWN_ANSWER_ALONE,
];

const FORGET_THE_STALE: &str = "DELETE FROM answers WHERE at < ?1";

const BRING_THE_FUTURE_BACK_TO_NOW: &str = "UPDATE answers SET at = ?1 WHERE at > ?1";

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dated {
    pub answer: Answer,
    pub at: u64,
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

fn unsigned(stored: i64) -> u64 {
    u64::try_from(stored).unwrap_or(0)
}

fn fresh_since(now: u64) -> i64 {
    signed(now.saturating_sub(KEPT_FRESH_FOR.as_secs()))
}

fn no_later_than(at: u64, now: u64) -> u64 {
    at.min(now)
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

        let now = now();

        transaction
            .execute(FORGET_THE_STALE, [fresh_since(now)])
            .map_err(Error::store(Store::Kept, StoreOp::Write))?;
        transaction
            .execute(BRING_THE_FUTURE_BACK_TO_NOW, [signed(now)])
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
        Ok(self.dated_at(asked, now)?.map(|dated| dated.answer))
    }

    pub fn answer_dated(&self, asked: &Asked) -> Result<Option<Dated>> {
        self.dated_at(asked, now())
    }

    fn dated_at(&self, asked: &Asked, now: u64) -> Result<Option<Dated>> {
        let connection = self.connection.lock();
        let read = Error::store(Store::Kept, StoreOp::Read);

        let kept: Option<(Vec<u8>, i64)> = connection
            .query_row(
                "SELECT packed, at FROM answers WHERE key = ?1 AND at >= ?2",
                params![asked.key(), fresh_since(now)],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(read)?;
        kept.map(|(packed, at)| {
            Ok(Dated {
                answer: answer_of(&packed)?,
                at: unsigned(at),
            })
        })
        .transpose()
    }

    pub fn keep(&self, asked: &Asked, answer: &Answer) -> Result<bool> {
        self.keep_at(asked, answer, now(), None)
    }

    pub fn keep_told(&self, asked: &Asked, answer: &Answer, at: u64, device: &str) -> Result<bool> {
        let now = now();
        let at = no_later_than(at, now);

        if signed(at) < fresh_since(now) {
            return Ok(false);
        }
        self.keep_at(asked, answer, at, Some(device))
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
        let kept = written(&transaction, &asked.key(), at, came_from, &squeezed)?;

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
                        seq: unsigned(row.get(0)?),
                        at: unsigned(row.get(1)?),
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
        let now = now();
        let since = fresh_since(now);
        let mut taken = 0;
        let mut connection = self.connection.lock();

        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(Error::store(Store::Kept, StoreOp::Write))?;

        for told in answers {
            let at = no_later_than(told.at, now);

            if signed(at) < since {
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
            if written(&transaction, &told.key, at, Some(device), &told.packed)? {
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
    Ok(true)
}

#[cfg(test)]
mod tests {
    use resonate_core::{Isrc, Mbid};
    use resonate_library::Recording;

    use super::*;
    use crate::wire::mbid_bytes;

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

    fn plan_of(connection: &Connection, sql: &str) -> String {
        connection
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .and_then(|mut statement| {
                statement
                    .query_map([0], |row| row.get::<_, String>(3))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .expect("a plan")
            .join("; ")
    }

    #[test]
    fn answers_kept_before_their_age_was_indexed_stay_and_are_aged_off_the_index() {
        let numbered = kept_at_the_first_shape(5);

        numbered
            .execute_batch(AN_ANSWERS_NUMBER_NEVER_GOES_BACK)
            .expect("the second shape");
        numbered
            .pragma_update(None, "user_version", 2)
            .expect("the second shape's stamp");
        numbered
            .execute(
                "INSERT INTO answers (key, at, seq, came_from, packed) VALUES (?1, 1, 6, NULL, ?2)",
                params![vec![2_u8], vec![0_u8]],
            )
            .expect("an answer long stale");

        let kept = Kept::over(numbered).expect("carried forward");
        let connection = kept.connection.lock();

        let version: u32 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("the stamp");
        let held: i64 = connection
            .query_row("SELECT count(*) FROM answers", [], |row| row.get(0))
            .expect("counted");

        assert_eq!(usize::try_from(version).expect("a count"), MIGRATIONS.len());
        assert_eq!(held, 1, "the fresh answer was lost or the stale one kept");
        for statement in [FORGET_THE_STALE, BRING_THE_FUTURE_BACK_TO_NOW] {
            let plan = plan_of(&connection, statement);

            assert!(
                plan.contains("answers_by_age"),
                "{statement} reads every answer: {plan}"
            );
        }
    }

    #[test]
    fn a_recording_indexed_inside_an_isrc_answer_before_this_build_no_longer_answers_its_id() {
        let slow_water = Mbid::new("83d91898-7763-47d7-b03b-b92132375c47").expect("an mbid");
        let code = Isrc::new("GBN9Y7100006").expect("a code");
        let coded = Asked::Isrc(code.clone());

        let thin = Answer::Recordings(vec![Recording {
            id: slow_water.clone(),
            title: "Slow Water".to_owned(),
            credit: Vec::new(),
            length: None,
            isrcs: vec![code],
            releases: Vec::new(),
        }]);
        let squeezed = packed(&thin.encoded(), KEPT_PACKED_AT).expect("packed");

        let indexed = Connection::open_in_memory().expect("a store in memory");

        for step in &MIGRATIONS[..3] {
            indexed.execute_batch(step).expect("a released step");
        }
        indexed
            .pragma_update(None, "user_version", 3)
            .expect("the third shape's stamp");
        indexed
            .execute(
                "INSERT INTO answers (key, at, seq, came_from, packed) VALUES (?1, ?2, 1, NULL, ?3)",
                params![coded.key(), signed(now()), squeezed],
            )
            .expect("an isrc answer of the third shape");
        indexed
            .execute(
                "INSERT INTO entities (mbid, kind, key) VALUES (?1, ?2, ?3)",
                params![
                    mbid_bytes(&slow_water).to_vec(),
                    crate::Entity::Recording.code(),
                    coded.key()
                ],
            )
            .expect("its recording indexed by id");

        let kept = Kept::over(indexed).expect("carried forward");

        let indexes: i64 = kept
            .connection
            .lock()
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name = 'entities'",
                [],
                |row| row.get(0),
            )
            .expect("the schema read");

        assert_eq!(
            kept.answer(&Asked::Recording(slow_water)).expect("read"),
            None
        );
        assert_eq!(kept.answer(&coded).expect("read"), Some(thin));
        assert_eq!(indexes, 0);
    }
}
