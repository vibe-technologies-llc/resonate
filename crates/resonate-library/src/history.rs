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

const COUNT_THE_CREDITED_AND_LET_THEM_GO: &str = "UPDATE tracks
        SET plays = plays + (SELECT count(*) FROM unheld_listens u
                              WHERE u.path = tracks.path AND u.span_start = tracks.span_start),
            played = max(coalesce(played, 0),
                         (SELECT max(u.at) FROM unheld_listens u
                           WHERE u.path = tracks.path AND u.span_start = tracks.span_start))
      WHERE EXISTS (SELECT 1 FROM unheld_listens u
                     WHERE u.path = tracks.path AND u.span_start = tracks.span_start);
     DELETE FROM unheld_listens
      WHERE EXISTS (SELECT 1 FROM tracks t
                     WHERE t.path = unheld_listens.path AND t.span_start = unheld_listens.span_start);";

const SPEND_A_HELD_LISTEN: &str = "UPDATE listens SET heard = ?1 WHERE id = ?2";

const SPEND_WHAT_A_HELD_LISTEN_BECAME: &str =
    "UPDATE unheld_listens SET heard = ?1 WHERE listen_id = ?2";

const SPEND_AN_UNHELD_LISTEN: &str = "UPDATE unheld_listens SET heard = ?1 WHERE id = ?2";

const SPEND_WHAT_AN_UNHELD_LISTEN_BECAME: &str = "UPDATE listens SET heard = ?1
      WHERE id = (SELECT listen FROM unheld_credited WHERE unheld = ?2)";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
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
    for unheld in now_held {
        let listen = transaction
            .query_row(CREDIT_AN_UNHELD, params![unheld], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|source| Error::store(StoreOp::Insert, source))?;
        transaction
            .execute(NOTE_AN_UNHELD_CREDITED, params![unheld, listen])
            .map_err(|source| Error::store(StoreOp::Insert, source))?;
    }

    transaction
        .execute_batch(COUNT_THE_CREDITED_AND_LET_THEM_GO)
        .map_err(|source| Error::store(StoreOp::Update, source))
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
    use super::*;

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
