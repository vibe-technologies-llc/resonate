use std::time::{Duration, SystemTime};

use resonate_core::ListenId;
use rusqlite::{Connection, OptionalExtension, params};

use crate::{Error, Mbid, Result, StoreOp, db::Inner, store};

pub const SUBMITTED_AT_ONCE: usize = 100;

const REFUSED_AS_MALFORMED: u16 = 400;

const THE_MARK: &str = "SELECT through FROM submissions WHERE service = ?1";

const MARKED_THROUGH: &str = "INSERT INTO submissions (service, through) VALUES (?1, ?2)
     ON CONFLICT (service) DO UPDATE SET through = max(through, excluded.through)";

const THE_LAST_LISTEN: &str = "SELECT coalesce(max(id), 0) FROM listens";

macro_rules! billed_columns {
    () => {
        "t.title, coalesce(t.artist, r.name), a.title, t.mbid, a.mbid,
         coalesce(t.artist_mbid, r.mbid), t.track_number, t.duration, t.sample_rate"
    };
}

const THE_LISTENS_AFTER: &str = concat!(
    "SELECT l.id, coalesce(l.began, l.at), ",
    billed_columns!(),
    " FROM listens l
       JOIN tracks t ON t.id = l.track_id
       LEFT JOIN albums a ON a.id = t.album_id
       LEFT JOIN artists r ON r.id = t.artist_id
      WHERE l.id > ?1
      ORDER BY l.id
      LIMIT ?2"
);

const THE_TRACK_BILLED: &str = concat!(
    "SELECT ",
    billed_columns!(),
    " FROM tracks t
       LEFT JOIN albums a ON a.id = t.album_id
       LEFT JOIN artists r ON r.id = t.artist_id
      WHERE t.path = ?1 AND t.span_start = ?2"
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ListeningService {
    ListenBrainz,
}

impl ListeningService {
    pub const fn name(self) -> &'static str {
        match self {
            Self::ListenBrainz => "listenbrainz",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Billed {
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub recording: Option<Mbid>,
    pub release: Option<Mbid>,
    pub artist_mbid: Option<Mbid>,
    pub number: Option<u32>,
    pub length: Option<Duration>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scrobble {
    pub listen: ListenId,
    pub at: SystemTime,
    pub billed: Billed,
}

pub trait Scrobbler: Send + Sync {
    fn service(&self) -> ListeningService;

    fn submit(&self, listens: &[Scrobble]) -> Result<()>;

    fn playing_now(&self, playing: &Billed) -> Result<()>;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Submitted {
    pub submitted: usize,
    pub refused: usize,
    pub unnamed: usize,
    pub started: bool,
}

struct Pending {
    listen: ListenId,
    scrobble: Option<Scrobble>,
}

pub(crate) fn submit(inner: &Inner, scrobbler: &dyn Scrobbler) -> Result<Submitted> {
    let service = scrobbler.service();
    let Some(mut through) = mark(inner, service)? else {
        let last = inner.read(last_listen)?;
        mark_through(inner, service, last)?;
        return Ok(Submitted {
            started: true,
            ..Submitted::default()
        });
    };

    let mut submitted = Submitted::default();
    loop {
        let pending = inner.read(|connection| listens_after(connection, through))?;
        let Some(last) = pending.last().map(|pending| pending.listen.get()) else {
            return Ok(submitted);
        };
        let named: Vec<Scrobble> = pending
            .iter()
            .filter_map(|pending| pending.scrobble.clone())
            .collect();
        submitted.unnamed += pending.len() - named.len();

        match submitted_whole(scrobbler, &named) {
            Ok(()) => submitted.submitted += named.len(),
            Err(error) if !refused_as_malformed(&error) => return Err(error),
            Err(_) => {
                for one in &named {
                    match scrobbler.submit(std::slice::from_ref(one)) {
                        Ok(()) => submitted.submitted += 1,
                        Err(error) if refused_as_malformed(&error) => {
                            tracing::warn!(
                                listen = one.listen.get(),
                                title = one.billed.title,
                                "a listen was refused as malformed and is passed over"
                            );
                            submitted.refused += 1;
                        }
                        Err(error) => {
                            mark_through(inner, service, one.listen.get() - 1)?;
                            return Err(error);
                        }
                    }
                }
            }
        }

        through = last;
        mark_through(inner, service, through)?;
        if pending.len() < SUBMITTED_AT_ONCE {
            return Ok(submitted);
        }
    }
}

fn submitted_whole(scrobbler: &dyn Scrobbler, named: &[Scrobble]) -> Result<()> {
    if named.is_empty() {
        return Ok(());
    }
    scrobbler.submit(named)
}

const fn refused_as_malformed(error: &Error) -> bool {
    matches!(
        error,
        Error::Refused {
            status: REFUSED_AS_MALFORMED,
            ..
        }
    )
}

fn mark(inner: &Inner, service: ListeningService) -> Result<Option<u64>> {
    inner.read(|connection| {
        connection
            .query_row(THE_MARK, params![service.name()], |row| {
                row.get::<_, i64>(0)
            })
            .optional()
            .map(|through| through.map(|through| through.max(0).cast_unsigned()))
            .map_err(|source| Error::store(StoreOp::Query, source))
    })
}

fn mark_through(inner: &Inner, service: ListeningService, through: u64) -> Result<()> {
    let through = i64::try_from(through).unwrap_or(i64::MAX);
    inner.write(|transaction| {
        transaction
            .execute(MARKED_THROUGH, params![service.name(), through])
            .map(drop)
            .map_err(|source| Error::store(StoreOp::Update, source))
    })
}

fn last_listen(connection: &Connection) -> Result<u64> {
    connection
        .query_row(THE_LAST_LISTEN, [], |row| row.get::<_, i64>(0))
        .map(|last| last.max(0).cast_unsigned())
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn listens_after(connection: &Connection, through: u64) -> Result<Vec<Pending>> {
    let mut statement = connection
        .prepare(THE_LISTENS_AFTER)
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let rows = statement
        .query_map(
            params![
                i64::try_from(through).unwrap_or(i64::MAX),
                SUBMITTED_AT_ONCE as i64
            ],
            RawListen::read,
        )
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    rows.into_iter().map(RawListen::pending).collect()
}

struct RawListen {
    listen: i64,
    at: i64,
    billed: RawBilled,
}

impl RawListen {
    fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            listen: row.get(0)?,
            at: row.get(1)?,
            billed: RawBilled::read(row, 2)?,
        })
    }

    fn pending(self) -> Result<Pending> {
        let listen = ListenId::new(self.listen.cast_unsigned())?;
        let scrobble = self.billed.billed().map(|billed| Scrobble {
            listen,
            at: store::from_nanos(self.at),
            billed,
        });

        Ok(Pending { listen, scrobble })
    }
}

struct RawBilled {
    title: String,
    artist: Option<String>,
    album: Option<String>,
    recording: Option<String>,
    release: Option<String>,
    artist_mbid: Option<String>,
    number: Option<i64>,
    frames: Option<i64>,
    rate: i64,
}

impl RawBilled {
    fn read(row: &rusqlite::Row<'_>, from: usize) -> rusqlite::Result<Self> {
        Ok(Self {
            title: row.get(from)?,
            artist: row.get(from + 1)?,
            album: row.get(from + 2)?,
            recording: row.get(from + 3)?,
            release: row.get(from + 4)?,
            artist_mbid: row.get(from + 5)?,
            number: row.get(from + 6)?,
            frames: row.get(from + 7)?,
            rate: row.get(from + 8)?,
        })
    }

    fn billed(self) -> Option<Billed> {
        let length = self
            .frames
            .zip(u32::try_from(self.rate).ok().filter(|rate| *rate > 0))
            .map(|(frames, rate)| Duration::from_secs_f64(frames.max(0) as f64 / f64::from(rate)));
        named(Some(self.title))
            .zip(named(self.artist))
            .map(|(title, artist)| Billed {
                title,
                artist,
                album: named(self.album),
                recording: store::mbid_in(self.recording.as_deref()),
                release: store::mbid_in(self.release.as_deref()),
                artist_mbid: store::mbid_in(self.artist_mbid.as_deref()),
                number: self.number.and_then(|number| u32::try_from(number).ok()),
                length,
            })
    }
}

pub(crate) fn billed_as(inner: &Inner, path: &str, start: i64) -> Result<Option<Billed>> {
    inner.read(|connection| {
        connection
            .query_row(THE_TRACK_BILLED, params![path, start], |row| {
                RawBilled::read(row, 0)
            })
            .optional()
            .map(|raw| raw.and_then(RawBilled::billed))
            .map_err(|source| Error::store(StoreOp::Query, source))
    })
}

fn named(text: Option<String>) -> Option<String> {
    text.map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}
