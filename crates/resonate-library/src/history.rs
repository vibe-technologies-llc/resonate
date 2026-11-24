use std::{
    fmt,
    num::NonZeroU16,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use resonate_core::ListenId;
use rusqlite::{Transaction, params};

use crate::{Error, Listen, Result, StoreOp, db::Inner, store};

const SECONDS_PER_DAY: u64 = 86_400;

const FOREVER: &str = "forever";

const FORGET_THE_LISTENS: &str = "DELETE FROM listens
      WHERE at < ?1
        AND id <= coalesce((SELECT min(through) FROM submissions), id)
        AND id <= coalesce((SELECT min(through) FROM earlier_submissions), id)";

const FORGET_THE_PASSES: &str = "DELETE FROM passes WHERE at < ?1";

const FORGET_THE_UNHELD: &str = "DELETE FROM unheld_listens WHERE at < ?1";

const FORGET_THE_PLAYLIST_PLAYS: &str = "DELETE FROM playlist_plays WHERE at < ?1";

const NOTE_THE_UNHELD_CREDITED_ELSEWHERE: &str = "INSERT INTO unheld_credited (unheld, listen)
     SELECT u.id, l.id
       FROM unheld_listens u
       JOIN listens l ON l.origin_device = u.origin_device AND l.origin_id = u.origin_id
      WHERE u.origin_device IS NOT NULL";

const FORGET_THE_UNHELD_CREDITED_ELSEWHERE: &str = "DELETE FROM unheld_listens
      WHERE origin_device IS NOT NULL
        AND EXISTS (SELECT 1 FROM listens l
                     WHERE l.origin_device = unheld_listens.origin_device
                       AND l.origin_id = unheld_listens.origin_id)";

const THE_UNHELD_NOW_HELD: &str = "SELECT u.id
       FROM unheld_listens u
       JOIN tracks t ON t.path = u.path AND t.span_start = u.span_start
      ORDER BY u.at, u.id";

const CREDIT_AN_UNHELD: &str = "INSERT INTO listens
            (id, track_id, at, began, heard, elsewhere, origin_device, origin_id)
     SELECT CASE WHEN NOT EXISTS (SELECT 1 FROM listens l WHERE l.id = u.listen_id)
                 THEN u.listen_id END,
            t.id, u.at, u.began, u.heard, u.elsewhere, u.origin_device, u.origin_id
       FROM unheld_listens u
       JOIN tracks t ON t.path = u.path AND t.span_start = u.span_start
      WHERE u.id = ?1
     RETURNING id";

const NOTE_AN_UNHELD_CREDITED: &str =
    "INSERT INTO unheld_credited (unheld, listen) VALUES (?1, ?2)";

const COUNT_THE_CREDITED: &str = "UPDATE tracks
        SET plays = plays + (SELECT count(*) FROM unheld_listens u
                              WHERE u.path = tracks.path AND u.span_start = tracks.span_start),
            played = max(coalesce(played, 0),
                         (SELECT max(u.at) FROM unheld_listens u
                           WHERE u.path = tracks.path AND u.span_start = tracks.span_start))
      WHERE id IN (SELECT t.id FROM unheld_listens u
                     JOIN tracks t ON t.path = u.path AND t.span_start = u.span_start)";

const LET_THE_CREDITED_GO: &str = "DELETE FROM unheld_listens
      WHERE EXISTS (SELECT 1 FROM tracks t
                     WHERE t.path = unheld_listens.path AND t.span_start = unheld_listens.span_start)";

const SPEND_A_HELD_LISTEN: &str = "UPDATE listens SET heard = ?1 WHERE id = ?2";

const SPEND_WHAT_A_HELD_LISTEN_BECAME: &str =
    "UPDATE unheld_listens SET heard = ?1 WHERE listen_id = ?2";

const SPEND_AN_UNHELD_LISTEN: &str = "UPDATE unheld_listens SET heard = ?1 WHERE id = ?2";

const SPEND_WHAT_AN_UNHELD_LISTEN_BECAME: &str = "UPDATE listens SET heard = ?1
      WHERE id = (SELECT listen FROM unheld_credited WHERE unheld = ?2)";

#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub enum HistoryKept {
    #[default]
    Forever,
    Days(NonZeroU16),
}

impl HistoryKept {
    pub const OFFERED: [Self; 5] = [
        Self::Forever,
        Self::days(1_826),
        Self::days(730),
        Self::days(365),
        Self::days(182),
    ];

    const fn days(count: u16) -> Self {
        match NonZeroU16::new(count) {
            Some(count) => Self::Days(count),
            None => Self::Forever,
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();

        if text.eq_ignore_ascii_case(FOREVER) {
            return Some(Self::Forever);
        }
        text.parse().ok().map(Self::Days)
    }

    pub fn for_days(days: i64) -> Option<Self> {
        u16::try_from(days)
            .ok()
            .and_then(NonZeroU16::new)
            .map(Self::Days)
    }

    pub fn span(self) -> Option<Duration> {
        match self {
            Self::Forever => None,
            Self::Days(days) => Some(Duration::from_secs(u64::from(days.get()) * SECONDS_PER_DAY)),
        }
    }

    pub fn before(self, now: SystemTime) -> Option<SystemTime> {
        self.span()
            .map(|span| now.checked_sub(span).unwrap_or(UNIX_EPOCH))
    }
}

impl fmt::Display for HistoryKept {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Forever => f.write_str(FOREVER),
            Self::Days(days) => write!(f, "{days}"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Aged {
    pub listens: u64,
    pub passes: u64,
}

pub(crate) fn age(inner: &Inner, kept: HistoryKept, now: SystemTime) -> Result<Aged> {
    let Some(before) = kept.before(now) else {
        return Ok(Aged::default());
    };
    let before = store::to_nanos(before);
    inner.write(|transaction| forgotten_before(transaction, before))
}

fn forgotten_before(transaction: &Transaction<'_>, before: i64) -> Result<Aged> {
    let listens = transaction
        .execute(FORGET_THE_LISTENS, params![before])
        .map_err(|source| Error::store(StoreOp::Delete, source))?;
    let passes = transaction
        .execute(FORGET_THE_PASSES, params![before])
        .map_err(|source| Error::store(StoreOp::Delete, source))?;

    for forgetting in [FORGET_THE_UNHELD, FORGET_THE_PLAYLIST_PLAYS] {
        transaction
            .execute(forgetting, params![before])
            .map_err(|source| Error::store(StoreOp::Delete, source))?;
    }
    Ok(Aged {
        listens: listens as u64,
        passes: passes as u64,
    })
}

pub(crate) fn credit_the_unheld(transaction: &Transaction<'_>) -> Result<()> {
    for elsewhere in [
        NOTE_THE_UNHELD_CREDITED_ELSEWHERE,
        FORGET_THE_UNHELD_CREDITED_ELSEWHERE,
    ] {
        transaction
            .execute(elsewhere, [])
            .map_err(|source| Error::store(StoreOp::Update, source))?;
    }

    let now_held = transaction
        .prepare(THE_UNHELD_NOW_HELD)
        .and_then(|mut query| {
            query
                .query_map([], |row| row.get::<_, i64>(0))
                .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        })
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    if now_held.is_empty() {
        return Ok(());
    }
    for unheld in now_held {
        let listen = store::queried(transaction, CREDIT_AN_UNHELD, params![unheld], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|source| Error::store(StoreOp::Insert, source))?;

        store::cached(
            transaction,
            NOTE_AN_UNHELD_CREDITED,
            params![unheld, listen],
        )
        .map_err(|source| Error::store(StoreOp::Insert, source))?;
    }

    for counting in [COUNT_THE_CREDITED, LET_THE_CREDITED_GO] {
        transaction
            .execute(counting, [])
            .map_err(|source| Error::store(StoreOp::Update, source))?;
    }
    Ok(())
}

pub(crate) fn spent(transaction: &Transaction<'_>, listen: Listen, heard: i64) -> Result<()> {
    let spend = |statement: &str, id: ListenId| {
        transaction
            .execute(statement, params![heard, id.get() as i64])
            .map_err(|source| Error::store(StoreOp::Update, source))
    };

    match listen {
        Listen::Held(id) => match spend(SPEND_A_HELD_LISTEN, id)? {
            0 => spend(SPEND_WHAT_A_HELD_LISTEN_BECAME, id).map(drop),
            _ => Ok(()),
        },
        Listen::Unheld(id) => match spend(SPEND_AN_UNHELD_LISTEN, id)? {
            0 => spend(SPEND_WHAT_AN_UNHELD_LISTEN_BECAME, id).map(drop),
            _ => Ok(()),
        },
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::*;
    use crate::{Library, schema};

    const THIRTY_DAYS: u16 = 30;

    fn the_clock() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_000 * SECONDS_PER_DAY)
    }

    fn the_cutoff() -> i64 {
        store::to_nanos(
            HistoryKept::days(THIRTY_DAYS)
                .before(the_clock())
                .expect("a span reaches back"),
        )
    }

    fn lay_down(library: &Library, statements: &str) {
        library
            .inner()
            .write(|transaction| {
                transaction
                    .execute_batch(statements)
                    .map_err(|source| Error::store(StoreOp::Insert, source))
            })
            .expect("the rows are laid down");
    }

    fn one_track(library: &Library) {
        lay_down(
            library,
            "INSERT INTO roots (id, path) VALUES (1, '/music');
             INSERT INTO tracks (id, root_id, path, title, sample_rate, channels, sample_format,
                                 codec, file_size, modified, added, seen)
             VALUES (1, 1, '/music/cold.flac', 'Cold', 44100, 2, 16, 1, 0, 0, 0, 0);",
        );
    }

    fn listen_ids(library: &Library) -> Vec<i64> {
        library
            .inner()
            .read(|connection| {
                connection
                    .prepare("SELECT id FROM listens ORDER BY id")
                    .and_then(|mut query| {
                        query
                            .query_map([], |row| row.get(0))
                            .and_then(Iterator::collect)
                    })
                    .map_err(|source| Error::store(StoreOp::Query, source))
            })
            .expect("the listens")
    }

    fn unheld_times(library: &Library) -> Vec<i64> {
        library
            .inner()
            .read(|connection| {
                connection
                    .prepare("SELECT at FROM unheld_listens ORDER BY at")
                    .and_then(|mut query| {
                        query
                            .query_map([], |row| row.get(0))
                            .and_then(Iterator::collect)
                    })
                    .map_err(|source| Error::store(StoreOp::Query, source))
            })
            .expect("the unheld listens")
    }

    #[test]
    fn a_listen_is_forgotten_only_before_the_cutoff_and_only_once_the_slower_service_was_told() {
        let library = Library::open_in_memory().expect("an in-memory catalog opens");
        let cutoff = the_cutoff();
        let a_day = SECONDS_PER_DAY as i64 * 1_000_000_000;
        let a_second = 1_000_000_000;

        one_track(&library);
        lay_down(
            &library,
            &format!(
                "INSERT INTO listens (id, track_id, at) VALUES
                    (1, 1, {early}), (2, 1, {recent}), (3, 1, {second}), (4, 1, {nano}),
                    (5, 1, {cutoff}), (6, 1, {after});
                 INSERT INTO submissions (service, through, began)
                    VALUES ('listenbrainz', 4, 0), ('lastfm', 2, 0);",
                early = cutoff - 3 * a_day,
                recent = cutoff - 2 * a_day,
                second = cutoff - a_second,
                nano = cutoff - 1,
                after = cutoff + 1,
            ),
        );

        let aged = age(library.inner(), HistoryKept::days(THIRTY_DAYS), the_clock())
            .expect("the history ages");

        assert_eq!(aged.listens, 2);
        assert_eq!(
            listen_ids(&library),
            vec![3, 4, 5, 6],
            "a listen the slower service was yet to be told of was forgotten, or one before the cutoff was kept"
        );

        lay_down(
            &library,
            "UPDATE submissions SET through = 4 WHERE service = 'lastfm';",
        );

        let aged = age(library.inner(), HistoryKept::days(THIRTY_DAYS), the_clock())
            .expect("the history ages");

        assert_eq!(aged.listens, 2);
        assert_eq!(
            listen_ids(&library),
            vec![5, 6],
            "the listen at the cutoff itself, or one after it, was forgotten"
        );

        let aged = age(library.inner(), HistoryKept::days(THIRTY_DAYS), the_clock())
            .expect("the history ages");

        assert_eq!(aged, Aged::default());
    }

    #[test]
    fn an_unheld_listen_or_a_pass_is_forgotten_by_age_whatever_a_service_was_told() {
        let library = Library::open_in_memory().expect("an in-memory catalog opens");
        let cutoff = the_cutoff();

        lay_down(
            &library,
            &format!(
                "INSERT INTO unheld_listens (path, span_start, at) VALUES
                    ('/away/a.flac', 0, {before}), ('/away/b.flac', 0, {cutoff}),
                    ('/away/c.flac', 0, {after});
                 INSERT INTO passes (at, heard) VALUES ({before}, 1), ({cutoff}, 1), ({after}, 1);
                 INSERT INTO submissions (service, through, began)
                    VALUES ('listenbrainz', 0, 0), ('lastfm', 0, 0);",
                before = cutoff - 1,
                after = cutoff + 1,
            ),
        );

        let aged = age(library.inner(), HistoryKept::days(THIRTY_DAYS), the_clock())
            .expect("the history ages");

        assert_eq!(
            aged,
            Aged {
                listens: 0,
                passes: 1
            }
        );
        assert_eq!(unheld_times(&library), vec![cutoff, cutoff + 1]);
    }

    fn plan_of(connection: &Connection, sql: &str) -> String {
        connection
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| row.get::<_, String>(3))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .expect("a plan")
            .join("; ")
    }

    #[test]
    fn the_credit_counts_only_the_tracks_an_unheld_listen_names() {
        let connection = Connection::open_in_memory().expect("a catalog");

        schema::configure(&connection, schema::Role::Writing).expect("the pragmas");
        schema::lay_out(&connection).expect("the schema applies");

        let plan = plan_of(&connection, COUNT_THE_CREDITED);

        assert!(
            !plan.contains("SCAN tracks"),
            "the credit reads every track: {plan}"
        );
    }

    #[test]
    fn an_unheld_listen_whose_track_arrived_is_counted_on_it_and_the_others_wait() {
        let mut connection = Connection::open_in_memory().expect("a catalog");

        schema::configure(&connection, schema::Role::Writing).expect("the pragmas");
        schema::lay_out(&connection).expect("the schema applies");
        connection
            .execute_batch(
                "INSERT INTO tracks (id, path, title, sample_rate, channels, sample_format, codec,
                                     file_size, modified, added, seen, plays, played)
                 VALUES (1, '/music/arrived.flac', 'Arrived', 44100, 2, 16, 1, 1, 1, 1, 1, 2, 5),
                        (2, '/music/other.flac', 'Other', 44100, 2, 16, 1, 1, 1, 1, 1, 3, 7);
                 INSERT INTO unheld_listens (path, span_start, at, heard)
                 VALUES ('/music/arrived.flac', 0, 10, 100),
                        ('/music/arrived.flac', 0, 20, 100),
                        ('/music/away.flac', 0, 30, 100);",
            )
            .expect("a catalog with listens kept against paths");

        let transaction = connection.transaction().expect("a write");

        credit_the_unheld(&transaction).expect("the credit");
        transaction.commit().expect("the credit is kept");

        let counted = |id: i64| -> (i64, i64) {
            connection
                .query_row(
                    "SELECT plays, played FROM tracks WHERE id = ?1",
                    [id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .expect("a track")
        };
        let waiting: Vec<String> = connection
            .prepare("SELECT path FROM unheld_listens")
            .and_then(|mut query| {
                query
                    .query_map([], |row| row.get(0))
                    .and_then(Iterator::collect)
            })
            .expect("the unheld listens");
        let credited: i64 = connection
            .query_row(
                "SELECT count(*) FROM listens WHERE track_id = 1",
                [],
                |row| row.get(0),
            )
            .expect("the listens");

        assert_eq!(counted(1), (4, 20));
        assert_eq!(counted(2), (3, 7));
        assert_eq!(waiting, vec!["/music/away.flac".to_owned()]);
        assert_eq!(credited, 2);
    }

    #[test]
    fn a_kept_history_reads_back_as_it_was_written() {
        for kept in HistoryKept::OFFERED {
            assert_eq!(HistoryKept::parse(&kept.to_string()), Some(kept));
        }

        assert_eq!(HistoryKept::parse(" Forever "), Some(HistoryKept::Forever));
        assert_eq!(HistoryKept::parse("0"), None);
        assert_eq!(HistoryKept::parse("a while"), None);
        assert_eq!(HistoryKept::for_days(0), None);
        assert_eq!(HistoryKept::for_days(-3), None);
        assert_eq!(HistoryKept::for_days(365), Some(HistoryKept::days(365)));
    }

    #[test]
    fn forever_forgets_nothing_and_a_span_reaches_back_that_far() {
        let now = UNIX_EPOCH + Duration::from_secs(1_000 * SECONDS_PER_DAY);

        assert_eq!(HistoryKept::Forever.before(now), None);
        assert_eq!(
            HistoryKept::days(365).before(now),
            Some(UNIX_EPOCH + Duration::from_secs(635 * SECONDS_PER_DAY))
        );
    }
}
