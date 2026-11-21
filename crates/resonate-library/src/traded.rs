use std::{
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use resonate_core::{MediaLocation, Recording};
use rusqlite::{OptionalExtension as _, Transaction, params};

use crate::{
    Billed, Error, ListeningService, Result, StoreOp,
    db::Inner,
    scrobble::{RawBilled, billed_columns},
    store, synced,
};

pub const SAME_LISTEN_WITHIN: Duration = Duration::from_secs(2);

const THE_HELD_AFTER: &str = concat!(
    "SELECT l.id, coalesce(l.began, l.at), l.heard, l.elsewhere, t.path, t.span_start, t.span_frames, ",
    billed_columns!(),
    " FROM listens l
       JOIN tracks t ON t.id = l.track_id
       LEFT JOIN albums a ON a.id = t.album_id
       LEFT JOIN artists r ON r.id = t.artist_id
      WHERE l.id > ?1
      ORDER BY l.id
      LIMIT ?2"
);

const THE_UNHELD_AFTER: &str = "SELECT id, coalesce(began, at), heard, elsewhere, path, span_start
       FROM unheld_listens
      WHERE id > ?1
      ORDER BY id
      LIMIT ?2";

const THE_TRACK_CUT: &str = "SELECT id, duration, sample_rate FROM tracks
      WHERE path = ?1 AND span_start = ?2";

const HELD_AT_THAT_MOMENT: &str = "SELECT EXISTS (
         SELECT 1 FROM listens
          WHERE track_id = ?1 AND coalesce(began, at) BETWEEN ?2 - ?3 AND ?2 + ?3
     )";

const CREDITED: &str = "INSERT OR IGNORE INTO listens
         (track_id, at, began, heard, elsewhere, origin_device, origin_id)
     VALUES (?1, ?2, ?2, ?3, 1, ?4, ?5)";

const COUNTED: &str =
    "UPDATE tracks SET plays = plays + 1, played = max(coalesce(played, 0), ?2) WHERE id = ?1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryFrom {
    Held(u64),
    Unheld(u64),
}

impl HistoryFrom {
    pub const START: Self = Self::Held(0);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeardOnce {
    pub began: SystemTime,
    pub heard: Duration,
    pub elsewhere: bool,
    pub billed: Option<Billed>,
    pub location: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryRead {
    pub heard: Vec<HeardOnce>,
    pub next: Option<HistoryFrom>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeardElsewhere {
    pub at: SystemTime,
    pub recording: Recording,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeardPage {
    pub heard: Vec<HeardElsewhere>,
    pub next: Option<u64>,
}

pub trait History: Send + Sync {
    fn service(&self) -> ListeningService;

    fn page(&self, from: Option<u64>) -> Result<HeardPage>;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BroughtIn {
    pub credited: u64,
    pub held_already: u64,
    pub unmatched: u64,
}

impl BroughtIn {
    pub const fn and(self, other: Self) -> Self {
        Self {
            credited: self.credited + other.credited,
            held_already: self.held_already + other.held_already,
            unmatched: self.unmatched + other.unmatched,
        }
    }
}

pub(crate) fn history_from(inner: &Inner, from: HistoryFrom, most: usize) -> Result<HistoryRead> {
    let most = i64::try_from(most.max(1)).unwrap_or(i64::MAX);
    inner.read(|connection| {
        let (sql, after) = match from {
            HistoryFrom::Held(after) => (THE_HELD_AFTER, after),
            HistoryFrom::Unheld(after) => (THE_UNHELD_AFTER, after),
        };
        let mut statement = connection
            .prepare_cached(sql)
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;
        let held = matches!(from, HistoryFrom::Held(_));

        let rows = statement
            .query_map(params![after.cast_signed(), most], |row| {
                let path: String = row.get(4)?;
                let start: i64 = row.get(5)?;
                let (frames, billed) = if held {
                    (
                        row.get::<_, Option<i64>>(6)?,
                        RawBilled::read(row, 7)?.billed(),
                    )
                } else {
                    (None, None)
                };
                Ok((
                    row.get::<_, i64>(0)?,
                    HeardOnce {
                        began: store::from_nanos(row.get(1)?),
                        heard: Duration::from_nanos(row.get::<_, i64>(2)?.max(0).cast_unsigned()),
                        elsewhere: row.get::<_, i64>(3)? != 0,
                        billed,
                        location: located(&path, start, frames),
                    },
                ))
            })
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))?;

        let last = rows.last().map(|(id, _)| id.max(&0).cast_unsigned());
        let full = rows.len() == usize::try_from(most).unwrap_or(usize::MAX);
        let next = match (from, last, full) {
            (HistoryFrom::Held(_), Some(last), true) => Some(HistoryFrom::Held(last)),
            (HistoryFrom::Held(_), _, false) => Some(HistoryFrom::Unheld(0)),
            (HistoryFrom::Unheld(_), Some(last), true) => Some(HistoryFrom::Unheld(last)),
            (HistoryFrom::Held(_) | HistoryFrom::Unheld(_), _, _) => None,
        };
        Ok(HistoryRead {
            heard: rows.into_iter().map(|(_, heard)| heard).collect(),
            next,
        })
    })
}

fn located(text: &str, start: i64, frames: Option<i64>) -> String {
    if Path::new(text).is_absolute() {
        return MediaLocation::local(store::path_of(text))
            .to_uri_within(store::span(start, frames));
    }
    text.to_owned()
}

pub(crate) fn bring_in(
    inner: &Inner,
    service: ListeningService,
    heard: &[HeardElsewhere],
) -> Result<BroughtIn> {
    let brought = inner.write(|transaction| {
        let mut brought = BroughtIn::default();

        for listen in heard {
            match credited(transaction, service, listen)? {
                Landed::Credited => brought.credited += 1,
                Landed::HeldAlready => brought.held_already += 1,
                Landed::Unmatched => brought.unmatched += 1,
            }
        }
        Ok(brought)
    })?;

    if brought.credited > 0 {
        inner.arrived();
    }
    Ok(brought)
}

enum Landed {
    Credited,
    HeldAlready,
    Unmatched,
}

fn credited(
    transaction: &Transaction<'_>,
    service: ListeningService,
    listen: &HeardElsewhere,
) -> Result<Landed> {
    let Some(cut) = synced::resolved(transaction, &listen.recording)? else {
        return Ok(Landed::Unmatched);
    };
    let Some(path) = cut.location.as_path() else {
        return Ok(Landed::Unmatched);
    };
    let (start, _) = store::span_columns(cut.span);

    let track = transaction
        .query_row(
            THE_TRACK_CUT,
            params![store::path_text(path), start],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    let Some((track, frames, rate)) = track else {
        return Ok(Landed::Unmatched);
    };

    let at = store::to_nanos(listen.at);
    let within = i64::try_from(SAME_LISTEN_WITHIN.as_nanos()).unwrap_or(i64::MAX);

    let held = transaction
        .query_row(HELD_AT_THAT_MOMENT, params![track, at, within], |row| {
            row.get::<_, bool>(0)
        })
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    if held {
        return Ok(Landed::HeldAlready);
    }

    let heard = listen
        .recording
        .length
        .or_else(|| lasting(frames, rate))
        .map_or(0, |length| {
            i64::try_from(length.as_nanos()).unwrap_or(i64::MAX)
        });
    let moment = listen
        .at
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs().cast_signed());

    let inserted = transaction
        .execute(
            CREDITED,
            params![track, at, heard, service.name().as_bytes(), moment],
        )
        .map_err(|source| Error::store(StoreOp::Insert, source))?;

    if inserted == 0 {
        return Ok(Landed::HeldAlready);
    }
    transaction
        .execute(COUNTED, params![track, at])
        .map_err(|source| Error::store(StoreOp::Update, source))?;
    Ok(Landed::Credited)
}

fn lasting(frames: Option<i64>, rate: i64) -> Option<Duration> {
    let frames = u64::try_from(frames?).ok()?;
    let rate = u64::try_from(rate).ok().filter(|rate| *rate > 0)?;
    Some(Duration::from_nanos(
        u64::try_from(u128::from(frames) * 1_000_000_000 / u128::from(rate)).unwrap_or(u64::MAX),
    ))
}
