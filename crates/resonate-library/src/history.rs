use std::{
    fmt,
    num::NonZeroU16,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rusqlite::{Transaction, params};

use crate::{Error, Result, StoreOp, db::Inner, store};

const SECONDS_PER_DAY: u64 = 86_400;

const FOREVER: &str = "forever";

const FORGET_THE_LISTENS: &str = "DELETE FROM listens
      WHERE at < ?1
        AND id <= coalesce((SELECT min(through) FROM submissions), id)";

const FORGET_THE_PASSES: &str = "DELETE FROM passes WHERE at < ?1";

const FORGET_THE_UNHELD: &str = "DELETE FROM unheld_listens WHERE at < ?1";

const CREDIT_THE_UNHELD: &str = "INSERT INTO listens (track_id, at)
     SELECT t.id, u.at FROM unheld_listens u
       JOIN tracks t ON t.path = u.path AND t.span_start = u.span_start
      ORDER BY u.at;
     UPDATE tracks
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
    transaction
        .execute(FORGET_THE_UNHELD, params![before])
        .map_err(|source| Error::store(StoreOp::Delete, source))?;
    Ok(Aged {
        listens: listens as u64,
        passes: passes as u64,
    })
}

pub(crate) fn credit_the_unheld(transaction: &Transaction<'_>) -> Result<()> {
    transaction
        .execute_batch(CREDIT_THE_UNHELD)
        .map_err(|source| Error::store(StoreOp::Update, source))
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
