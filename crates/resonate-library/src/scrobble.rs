use std::{
    collections::BTreeSet,
    fmt,
    sync::Arc,
    time::{Duration, SystemTime},
};

use resonate_core::ListenId;
use rusqlite::{Connection, OptionalExtension, params};

use crate::{Error, Isrc, Mbid, Result, StoreOp, db::Inner, store};

pub const SUBMITTED_AT_ONCE: usize = 100;

pub const LOVES_TOLD_AT_ONCE: usize = 25;

const REFUSED_AS_MALFORMED: u16 = 400;

const REFUSED_FOR_THE_TOKEN: [u16; 2] = [401, 403];

const REFUSED_FOR_NOW: [u16; 2] = [408, 429];

const THE_MARK: &str = "SELECT through FROM submissions WHERE service = ?1";

const MARKED_THROUGH: &str = "INSERT INTO submissions (service, through, began) VALUES (?1, ?2, ?2)
     ON CONFLICT (service) DO UPDATE SET through = max(through, excluded.through)";

const THE_FIRST_MARK: &str = "SELECT coalesce(began, through) FROM submissions WHERE service = ?1";

const THE_LAST_LISTEN: &str = "SELECT coalesce(max(id), 0) FROM listens";

const THE_EARLIER_MARK: &str = "SELECT through, until FROM earlier_submissions WHERE service = ?1";

const EARLIER_ASKED_FOR: &str = "INSERT INTO earlier_submissions (service, through, until)
     VALUES (?1, 0, ?2)
     ON CONFLICT (service) DO NOTHING";

const EARLIER_MARKED_THROUGH: &str = "UPDATE earlier_submissions SET through = max(through, ?2)
      WHERE service = ?1";

const EARLIER_ALL_TOLD: &str = "DELETE FROM earlier_submissions WHERE service = ?1";

const STILL_MARKED: &str = "SELECT EXISTS (SELECT 1 FROM submissions WHERE service = ?1)
             OR EXISTS (SELECT 1 FROM earlier_submissions WHERE service = ?1)";

const NO_LONGER_TOLD: &str = "DELETE FROM submissions WHERE service = ?1;
     DELETE FROM earlier_submissions WHERE service = ?1";

const THE_LOVED_RECORDINGS: &str = "SELECT DISTINCT mbid FROM tracks
      WHERE favourite IS NOT NULL AND mbid IS NOT NULL";

const THE_LOVES_TOLD: &str = "SELECT recording FROM loves_told WHERE service = ?1";

const A_LOVE_TOLD: &str = "INSERT INTO loves_told (service, recording) VALUES (?1, ?2)
     ON CONFLICT DO NOTHING";

const A_LOVE_TAKEN_BACK: &str = "DELETE FROM loves_told WHERE service = ?1 AND recording = ?2";

const THE_NAMES_OF_A_RECORDING: &str = "SELECT t.title, coalesce(t.artist, r.name) FROM tracks t
       LEFT JOIN artists r ON r.id = t.artist_id
      WHERE t.mbid = ?1 AND coalesce(t.artist, r.name) IS NOT NULL
      ORDER BY t.favourite IS NULL, t.id
      LIMIT 1";

macro_rules! billed_columns {
    () => {
        "t.title, coalesce(t.artist, r.name), a.title, t.mbid, a.mbid,
         coalesce(t.artist_mbid, r.mbid), t.track_number, t.duration, t.sample_rate,
         t.isrc, a.release_group"
    };
}

const THE_LISTENS_AFTER: &str = concat!(
    "SELECT l.id, coalesce(l.began, l.at), ",
    billed_columns!(),
    " FROM listens l
       JOIN tracks t ON t.id = l.track_id
       LEFT JOIN albums a ON a.id = t.album_id
       LEFT JOIN artists r ON r.id = t.artist_id
      WHERE l.id > ?1 AND l.id <= ?3
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
    LastFm,
}

impl ListeningService {
    pub const fn name(self) -> &'static str {
        match self {
            Self::ListenBrainz => "listenbrainz",
            Self::LastFm => "lastfm",
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
    pub release_group: Option<Mbid>,
    pub artist_mbid: Option<Mbid>,
    pub isrc: Option<Isrc>,
    pub number: Option<u32>,
    pub length: Option<Duration>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scrobble {
    pub listen: ListenId,
    pub at: SystemTime,
    pub billed: Billed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Love {
    Loved,
    TakenBack,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loved {
    pub recording: Mbid,
    pub named: Option<LovedNames>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LovedNames {
    pub title: String,
    pub artist: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenHeld {
    By(String),
    Unknown,
}

pub trait Scrobbler: Send + Sync {
    fn service(&self) -> ListeningService;

    fn submit(&self, listens: &[Scrobble]) -> Result<()>;

    fn playing_now(&self, playing: &Billed) -> Result<()>;

    fn love(&self, loved: &Loved, love: Love) -> Result<()>;

    fn token_held(&self) -> Result<TokenHeld>;
}

pub trait Scrobblers: Send + Sync {
    fn under(&self, token: String) -> Arc<dyn Scrobbler>;

    fn signed_in_to_lastfm(&self, asked: &LastfmSignIn) -> Result<LastfmSession>;
}

#[derive(Clone, PartialEq, Eq)]
pub struct LastfmSignIn {
    pub key: String,
    pub secret: String,
    pub user: String,
    pub password: String,
}

impl fmt::Debug for LastfmSignIn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LastfmSignIn")
            .field("key", &self.key)
            .field("user", &self.user)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct LastfmSession {
    pub name: String,
    pub key: String,
}

impl fmt::Debug for LastfmSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LastfmSession")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LovesTold {
    pub loved: usize,
    pub taken_back: usize,
    pub refused: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Submitted {
    pub submitted: usize,
    pub refused: usize,
    pub unnamed: usize,
    pub started: bool,
    pub earlier: usize,
}

#[derive(Clone, Copy)]
enum Cursor {
    Since,
    Earlier { until: u64 },
}

impl Cursor {
    const fn until(self) -> u64 {
        match self {
            Self::Since => i64::MAX.cast_unsigned(),
            Self::Earlier { until } => until,
        }
    }
}

struct Pending {
    listen: ListenId,
    scrobble: Option<Scrobble>,
}

pub(crate) fn submit(inner: &Inner, scrobbler: &dyn Scrobbler) -> Result<Submitted> {
    let service = scrobbler.service();
    let Some(through) = mark(inner, service)? else {
        let last = inner.read(last_listen)?;
        mark_through(inner, service, last)?;
        return Ok(Submitted {
            started: true,
            ..Submitted::default()
        });
    };

    let mut submitted = Submitted::default();
    told_in_batches(inner, scrobbler, through, Cursor::Since, &mut submitted)?;
    if let Some((through, until)) = earlier_mark(inner, service)? {
        let since = submitted.submitted;
        told_in_batches(
            inner,
            scrobbler,
            through,
            Cursor::Earlier { until },
            &mut submitted,
        )?;
        submitted.earlier = submitted.submitted - since;
        earlier_all_told(inner, service)?;
    }
    Ok(submitted)
}

pub(crate) fn tell_earlier(inner: &Inner, service: ListeningService) -> Result<()> {
    let until = match first_mark(inner, service)? {
        Some(began) => began,
        None => {
            let last = inner.read(last_listen)?;
            mark_through(inner, service, last)?;
            last
        }
    };
    let until = i64::try_from(until).unwrap_or(i64::MAX);
    inner.write(|transaction| {
        transaction
            .execute(EARLIER_ASKED_FOR, params![service.name(), until])
            .map(drop)
            .map_err(|source| Error::store(StoreOp::Update, source))
    })
}

pub(crate) fn earlier_owed(inner: &Inner, service: ListeningService) -> Result<bool> {
    Ok(earlier_mark(inner, service)?.is_some())
}

fn told_in_batches(
    inner: &Inner,
    scrobbler: &dyn Scrobbler,
    mut through: u64,
    cursor: Cursor,
    submitted: &mut Submitted,
) -> Result<()> {
    let service = scrobbler.service();
    let marked = |through: u64| match cursor {
        Cursor::Since => mark_through(inner, service, through),
        Cursor::Earlier { .. } => earlier_mark_through(inner, service, through),
    };
    loop {
        let pending =
            inner.read(|connection| listens_after(connection, through, cursor.until()))?;
        let Some(last) = pending.last().map(|pending| pending.listen.get()) else {
            return Ok(());
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
                            marked(one.listen.get() - 1)?;
                            return Err(error);
                        }
                    }
                }
            }
        }

        through = last;
        marked(through)?;
        if pending.len() < SUBMITTED_AT_ONCE {
            return Ok(());
        }
    }
}

pub(crate) fn tell_loves(inner: &Inner, scrobbler: &dyn Scrobbler) -> Result<LovesTold> {
    let service = scrobbler.service();
    let (loved, told) = inner.read(|connection| {
        Ok((
            recordings(connection, THE_LOVED_RECORDINGS, &[])?,
            recordings(connection, THE_LOVES_TOLD, &[service.name()])?,
        ))
    })?;
    let owed: Vec<(&Mbid, Love)> = loved
        .difference(&told)
        .map(|recording| (recording, Love::Loved))
        .chain(
            told.difference(&loved)
                .map(|recording| (recording, Love::TakenBack)),
        )
        .take(LOVES_TOLD_AT_ONCE)
        .collect();
    let owed: Vec<(Loved, Love)> = inner.read(|connection| {
        owed.iter()
            .map(|(recording, love)| {
                Ok((
                    Loved {
                        recording: (*recording).clone(),
                        named: names_of(connection, recording)?,
                    },
                    *love,
                ))
            })
            .collect()
    })?;

    let mut said = LovesTold::default();
    for (loved, love) in &owed {
        let (recording, love) = (&loved.recording, *love);
        match scrobbler.love(loved, love) {
            Ok(()) => match love {
                Love::Loved => said.loved += 1,
                Love::TakenBack => said.taken_back += 1,
            },
            Err(error) if refused_for_good(&error) => {
                tracing::warn!(
                    %recording,
                    ?love,
                    %error,
                    "a love was refused outright and is not told again"
                );
                said.refused += 1;
            }
            Err(error) => return Err(error),
        }
        note_told(inner, service, recording, love)?;
    }
    Ok(said)
}

fn names_of(connection: &Connection, recording: &Mbid) -> Result<Option<LovedNames>> {
    connection
        .query_row(THE_NAMES_OF_A_RECORDING, [recording.as_str()], |row| {
            Ok(LovedNames {
                title: row.get(0)?,
                artist: row.get(1)?,
            })
        })
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn recordings(connection: &Connection, sql: &str, binds: &[&str]) -> Result<BTreeSet<Mbid>> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let held = statement
        .query_map(rusqlite::params_from_iter(binds), |row| {
            row.get::<_, String>(0)
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    Ok(held
        .iter()
        .filter_map(|text| store::mbid_in(Some(text)))
        .collect())
}

fn note_told(inner: &Inner, service: ListeningService, recording: &Mbid, love: Love) -> Result<()> {
    let sql = match love {
        Love::Loved => A_LOVE_TOLD,
        Love::TakenBack => A_LOVE_TAKEN_BACK,
    };
    inner.write(|transaction| {
        transaction
            .execute(sql, params![service.name(), recording.as_str()])
            .map(drop)
            .map_err(|source| Error::store(StoreOp::Update, source))
    })
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

fn refused_for_good(error: &Error) -> bool {
    match error {
        Error::Refused { status, .. } => {
            *status >= 400
                && *status < 500
                && !REFUSED_FOR_THE_TOKEN.contains(status)
                && !REFUSED_FOR_NOW.contains(status)
        }
        _ => false,
    }
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

fn first_mark(inner: &Inner, service: ListeningService) -> Result<Option<u64>> {
    inner.read(|connection| {
        connection
            .query_row(THE_FIRST_MARK, params![service.name()], |row| {
                row.get::<_, i64>(0)
            })
            .optional()
            .map(|began| began.map(|began| began.max(0).cast_unsigned()))
            .map_err(|source| Error::store(StoreOp::Query, source))
    })
}

fn earlier_mark(inner: &Inner, service: ListeningService) -> Result<Option<(u64, u64)>> {
    inner.read(|connection| {
        connection
            .query_row(THE_EARLIER_MARK, params![service.name()], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })
            .optional()
            .map(|marked| {
                marked.map(|(through, until)| {
                    (through.max(0).cast_unsigned(), until.max(0).cast_unsigned())
                })
            })
            .map_err(|source| Error::store(StoreOp::Query, source))
    })
}

fn earlier_mark_through(inner: &Inner, service: ListeningService, through: u64) -> Result<()> {
    let through = i64::try_from(through).unwrap_or(i64::MAX);
    inner.write(|transaction| {
        transaction
            .execute(EARLIER_MARKED_THROUGH, params![service.name(), through])
            .map(drop)
            .map_err(|source| Error::store(StoreOp::Update, source))
    })
}

pub(crate) fn stop_telling(inner: &Inner, service: ListeningService) -> Result<bool> {
    let marked: bool = inner.read(|connection| {
        connection
            .query_row(STILL_MARKED, params![service.name()], |row| row.get(0))
            .map_err(|source| Error::store(StoreOp::Query, source))
    })?;
    if !marked {
        return Ok(false);
    }
    inner.write(|transaction| {
        for forgetting in NO_LONGER_TOLD.split(';') {
            transaction
                .execute(forgetting.trim(), params![service.name()])
                .map_err(|source| Error::store(StoreOp::Delete, source))?;
        }
        Ok(true)
    })
}

fn earlier_all_told(inner: &Inner, service: ListeningService) -> Result<()> {
    inner.write(|transaction| {
        transaction
            .execute(EARLIER_ALL_TOLD, params![service.name()])
            .map(drop)
            .map_err(|source| Error::store(StoreOp::Delete, source))
    })
}

fn last_listen(connection: &Connection) -> Result<u64> {
    connection
        .query_row(THE_LAST_LISTEN, [], |row| row.get::<_, i64>(0))
        .map(|last| last.max(0).cast_unsigned())
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn listens_after(connection: &Connection, through: u64, until: u64) -> Result<Vec<Pending>> {
    let mut statement = connection
        .prepare(THE_LISTENS_AFTER)
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let rows = statement
        .query_map(
            params![
                i64::try_from(through).unwrap_or(i64::MAX),
                SUBMITTED_AT_ONCE as i64,
                i64::try_from(until).unwrap_or(i64::MAX)
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
    isrc: Option<String>,
    release_group: Option<String>,
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
            isrc: row.get(from + 9)?,
            release_group: row.get(from + 10)?,
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
                release_group: store::mbid_in(self.release_group.as_deref()),
                artist_mbid: store::mbid_in(self.artist_mbid.as_deref()),
                isrc: store::isrc_in(self.isrc.as_deref()),
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
