use std::time::{Duration, SystemTime, UNIX_EPOCH};

use resonate_core::{Frames, SampleRate, TrackId};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::{
    Error, KeptLyrics, LyricText, LyricsAsked, REFUSED_AGAIN_AFTER, Result, StoreOp, enriched,
    store,
};

pub const MISSED_AGAIN_AFTER: Duration = Duration::from_secs(7 * 24 * 60 * 60);

pub const BETTERED_AFTER: Duration = Duration::from_secs(30 * 24 * 60 * 60);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LyricsAhead(i32);

impl LyricsAhead {
    pub const ZERO: Self = Self(0);
    pub const STEP_MS: i32 = 100;
    pub const MOST_MS: i32 = 30_000;

    pub const fn of_millis(millis: i32) -> Option<Self> {
        if millis.abs() > Self::MOST_MS {
            return None;
        }
        Some(Self(millis))
    }

    pub const fn millis(self) -> i32 {
        self.0
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    #[must_use]
    pub const fn earlier(self) -> Self {
        Self(clamped(self.0 + Self::STEP_MS))
    }

    #[must_use]
    pub const fn later(self) -> Self {
        Self(clamped(self.0 - Self::STEP_MS))
    }

    pub fn read(self, heard: Duration) -> Duration {
        let by = Duration::from_millis(u64::from(self.0.unsigned_abs()));
        if self.0 >= 0 {
            heard.saturating_add(by)
        } else {
            heard.saturating_sub(by)
        }
    }

    pub fn back(self, sung: Duration) -> Duration {
        Self(-self.0).read(sung)
    }
}

const fn clamped(millis: i32) -> i32 {
    if millis > LyricsAhead::MOST_MS {
        LyricsAhead::MOST_MS
    } else if millis < -LyricsAhead::MOST_MS {
        -LyricsAhead::MOST_MS
    } else {
        millis
    }
}

impl KeptLyrics {
    pub fn is_due(&self, now: SystemTime) -> bool {
        let waited = |wait: Duration| now.duration_since(self.taken).is_ok_and(|age| age >= wait);
        match &self.sung {
            None => waited(MISSED_AGAIN_AFTER),
            Some(sung) if sung.lyricsfile.is_none() => waited(BETTERED_AFTER),
            Some(_) => false,
        }
    }

    pub fn richer_of(self, told: Option<LyricText>) -> Option<LyricText> {
        richer(self.sung, told)
    }
}

fn richer(held: Option<LyricText>, told: Option<LyricText>) -> Option<LyricText> {
    match (held, told) {
        (Some(held), Some(told)) if held.detail() > told.detail() => Some(held),
        (held, None) => held,
        (_, told) => told,
    }
}

fn before(now: SystemTime, wait: Duration) -> i64 {
    store::to_nanos(now.checked_sub(wait).unwrap_or(UNIX_EPOCH))
}

fn refused_lately() -> String {
    format!(
        "EXISTS (SELECT 1 FROM lyrics_refused r
                  WHERE r.path = t.path AND r.span_start = t.span_start
                    AND r.refused + {} > ?5)",
        enriched::doubled("?4", "r.refusals")
    )
}

pub(crate) fn to_ask(
    connection: &Connection,
    refresh: bool,
    now: SystemTime,
) -> Result<Vec<TrackId>> {
    let mut statement = connection
        .prepare(&format!(
            "SELECT t.id FROM tracks t
               LEFT JOIN lyrics_kept k ON k.path = t.path AND k.span_start = t.span_start
              WHERE ?1
                 OR ((k.path IS NULL
                      OR (k.text IS NULL AND k.taken <= ?2)
                      OR (k.text IS NOT NULL AND k.lyricsfile IS NULL AND k.taken <= ?3))
                     AND NOT {})
              ORDER BY t.id",
            refused_lately()
        ))
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let found = statement
        .query_map(
            params![
                refresh,
                before(now, MISSED_AGAIN_AFTER),
                before(now, BETTERED_AFTER),
                i64::try_from(REFUSED_AGAIN_AFTER.as_nanos()).unwrap_or(i64::MAX),
                store::to_nanos(now)
            ],
            |row| row.get::<_, i64>(0),
        )
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    found
        .into_iter()
        .map(|id| TrackId::new(id as u64).map_err(Error::from))
        .collect()
}

pub(crate) struct Asking {
    pub(crate) path: String,
    pub(crate) span_start: i64,
    pub(crate) asked: LyricsAsked,
}

pub(crate) fn asking(connection: &Connection, id: TrackId) -> Result<Option<Asking>> {
    let found = connection
        .query_row(
            "SELECT t.path, t.span_start, t.title, coalesce(t.artist, oa.name),
                    coalesce(a.release_title, a.title), t.duration, t.sample_rate
               FROM tracks t
                    LEFT JOIN albums a ON a.id = t.album_id
                    LEFT JOIN artists oa ON oa.id = a.artist_id
              WHERE t.id = ?1",
            [id.get() as i64],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, u32>(6)?,
                ))
            },
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    let Some((path, span_start, title, artist, album, duration, rate)) = found else {
        return Ok(None);
    };
    let Some(artist) = artist.filter(|artist| !artist.trim().is_empty()) else {
        return Ok(None);
    };
    if title.trim().is_empty() {
        return Ok(None);
    }
    let rate = SampleRate::new(rate)?;

    Ok(Some(Asking {
        path,
        span_start,
        asked: LyricsAsked {
            title,
            artist,
            album: album.filter(|album| !album.trim().is_empty()),
            length: duration.map(|frames| Frames(frames.max(0) as u64).to_duration(rate)),
        },
    }))
}

pub(crate) fn ahead(connection: &Connection, path: &str, span_start: i64) -> Result<LyricsAhead> {
    let held = connection
        .query_row(
            "SELECT ahead_ms FROM lyrics_ahead WHERE path = ?1 AND span_start = ?2",
            params![path, span_start],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    Ok(held
        .and_then(|millis| i32::try_from(millis).ok())
        .and_then(LyricsAhead::of_millis)
        .unwrap_or_default())
}

pub(crate) fn hold_ahead(
    transaction: &Transaction<'_>,
    path: &str,
    span_start: i64,
    ahead: LyricsAhead,
) -> Result<()> {
    let written = if ahead.is_zero() {
        transaction.execute(
            "DELETE FROM lyrics_ahead WHERE path = ?1 AND span_start = ?2",
            params![path, span_start],
        )
    } else {
        transaction.execute(
            "INSERT INTO lyrics_ahead (path, span_start, ahead_ms) VALUES (?1, ?2, ?3)
             ON CONFLICT (path, span_start) DO UPDATE SET ahead_ms = excluded.ahead_ms",
            params![path, span_start, i64::from(ahead.millis())],
        )
    };
    written
        .map(drop)
        .map_err(|source| Error::store(StoreOp::Update, source))
}

pub(crate) fn kept(
    connection: &Connection,
    path: &str,
    span_start: i64,
) -> Result<Option<KeptLyrics>> {
    connection
        .query_row(
            "SELECT text, synced, lyricsfile, taken FROM lyrics_kept
              WHERE path = ?1 AND span_start = ?2",
            params![path, span_start],
            |row| {
                let text: Option<String> = row.get(0)?;
                let synced: bool = row.get(1)?;
                let lyricsfile: Option<String> = row.get(2)?;
                Ok(KeptLyrics {
                    sung: text.map(|text| LyricText {
                        text,
                        synced,
                        lyricsfile,
                    }),
                    taken: store::from_nanos(row.get(3)?),
                })
            },
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))
}

pub(crate) fn keep(
    tx: &Transaction<'_>,
    path: &str,
    span_start: i64,
    told: Option<&LyricText>,
    now: SystemTime,
) -> Result<bool> {
    let held = kept(tx, path, span_start)?.and_then(|kept| kept.sung);
    let bettered = match (&held, told) {
        (_, None) => false,
        (None, Some(_)) => true,
        (Some(held), Some(told)) => told.detail() >= held.detail() && held != told,
    };
    let kept = richer(held, told.cloned());
    tx.execute(
        "INSERT INTO lyrics_kept (path, span_start, text, synced, lyricsfile, taken)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(path, span_start) DO UPDATE SET
             text       = excluded.text,
             synced     = excluded.synced,
             lyricsfile = excluded.lyricsfile,
             taken      = excluded.taken",
        params![
            path,
            span_start,
            kept.as_ref().map(|kept| kept.text.as_str()),
            kept.as_ref().is_some_and(|kept| kept.synced),
            kept.as_ref().and_then(|kept| kept.lyricsfile.as_deref()),
            store::to_nanos(now)
        ],
    )
    .map_err(|source| Error::store(StoreOp::Insert, source))?;
    tx.execute(
        "DELETE FROM lyrics_refused WHERE path = ?1 AND span_start = ?2",
        params![path, span_start],
    )
    .map_err(|source| Error::store(StoreOp::Delete, source))?;
    store::index_what_is_sung(
        tx,
        path,
        span_start,
        kept.as_ref().map(|kept| kept.text.as_str()),
    )?;

    Ok(bettered)
}

pub(crate) fn note_refused(
    tx: &Transaction<'_>,
    path: &str,
    span_start: i64,
    now: SystemTime,
) -> Result<()> {
    tx.execute(
        "INSERT INTO lyrics_refused (path, span_start, refused, refusals) VALUES (?1, ?2, ?3, 1)
         ON CONFLICT(path, span_start) DO UPDATE SET
             refused  = excluded.refused,
             refusals = lyrics_refused.refusals + 1",
        params![path, span_start, store::to_nanos(now)],
    )
    .map(drop)
    .map_err(|source| Error::store(StoreOp::Insert, source))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_offset_reads_the_words_ahead_or_behind_and_is_held_within_its_reach() {
        let heard = Duration::from_secs(10);
        let ahead = LyricsAhead::of_millis(500).expect("within reach");
        let behind = LyricsAhead::of_millis(-500).expect("within reach");

        assert_eq!(ahead.read(heard), Duration::from_millis(10_500));
        assert_eq!(behind.read(heard), Duration::from_millis(9_500));
        assert_eq!(behind.read(Duration::from_millis(200)), Duration::ZERO);
        assert_eq!(ahead.back(ahead.read(heard)), heard);
        assert_eq!(LyricsAhead::ZERO.earlier().millis(), LyricsAhead::STEP_MS);
        assert_eq!(LyricsAhead::ZERO.later().millis(), -LyricsAhead::STEP_MS);
        assert_eq!(LyricsAhead::of_millis(LyricsAhead::MOST_MS + 1), None);
        let furthest = LyricsAhead::of_millis(LyricsAhead::MOST_MS).expect("at the edge");
        assert_eq!(furthest.earlier(), furthest);
        let least = LyricsAhead::of_millis(-LyricsAhead::MOST_MS).expect("at the edge");
        assert_eq!(least.later(), least);
    }

    fn plain(text: &str) -> LyricText {
        LyricText {
            text: text.to_owned(),
            synced: false,
            lyricsfile: None,
        }
    }

    fn lined(text: &str) -> LyricText {
        LyricText {
            synced: true,
            ..plain(text)
        }
    }

    fn documented(text: &str) -> LyricText {
        LyricText {
            lyricsfile: Some("version: '1.0'".to_owned()),
            ..lined(text)
        }
    }

    fn kept(sung: Option<LyricText>, age: Duration, now: SystemTime) -> KeptLyrics {
        KeptLyrics {
            sung,
            taken: now - age,
        }
    }

    const A_DAY: Duration = Duration::from_secs(24 * 60 * 60);

    #[test]
    fn a_miss_is_asked_again_after_a_week_and_a_set_short_of_a_lyricsfile_after_a_month() {
        let now = SystemTime::now();

        assert!(!kept(None, A_DAY, now).is_due(now));
        assert!(kept(None, MISSED_AGAIN_AFTER, now).is_due(now));
        assert!(!kept(Some(plain("la")), MISSED_AGAIN_AFTER, now).is_due(now));
        assert!(kept(Some(plain("la")), BETTERED_AFTER, now).is_due(now));
        assert!(kept(Some(lined("[00:01.00]la")), BETTERED_AFTER, now).is_due(now));
        assert!(!kept(Some(documented("[00:01.00]la")), BETTERED_AFTER * 12, now).is_due(now));
        assert!(
            !kept(None, Duration::ZERO, now + A_DAY).is_due(now),
            "a miss stamped in the future was asked again"
        );
    }

    #[test]
    fn what_is_kept_is_never_traded_for_something_plainer() {
        assert_eq!(richer(Some(lined("a")), Some(plain("b"))), Some(lined("a")));
        assert_eq!(richer(Some(lined("a")), None), Some(lined("a")));
        assert_eq!(richer(Some(plain("a")), Some(lined("b"))), Some(lined("b")));
        assert_eq!(richer(Some(lined("a")), Some(lined("b"))), Some(lined("b")));
        assert_eq!(
            richer(Some(lined("a")), Some(documented("b"))),
            Some(documented("b"))
        );
        assert_eq!(richer(None, None), None);
    }
}
