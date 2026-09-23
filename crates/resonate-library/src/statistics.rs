use std::time::{Duration, SystemTime, UNIX_EPOCH};

use resonate_core::{AlbumId, ArtistId, TrackId};
use rusqlite::{Connection, params};

use crate::{Error, Result, StoreOp, db::Inner, store};

const SECONDS_PER_DAY: u64 = 86_400;
const DAYS_IN_A_WEEK: u64 = 7;
const DAYS_IN_A_MONTH: u64 = 30;
const DAYS_IN_A_YEAR: u64 = 365;

const A_WEEK: Duration = Duration::from_secs(DAYS_IN_A_WEEK * SECONDS_PER_DAY);
const A_MONTH: Duration = Duration::from_secs(DAYS_IN_A_MONTH * SECONDS_PER_DAY);
const A_YEAR: Duration = Duration::from_secs(DAYS_IN_A_YEAR * SECONDS_PER_DAY);

const NANOS_PER_SECOND: i64 = 1_000_000_000;
const NANOS_PER_DAY: i64 = SECONDS_PER_DAY as i64 * NANOS_PER_SECOND;

const SINCE_THE_BEGINNING: i64 = i64::MIN;

const WHAT_WAS_HEARD: &str = "SELECT count(*), coalesce(sum(l.heard), 0),
            count(DISTINCT l.track_id), count(DISTINCT t.album_id), count(DISTINCT t.artist_id)
       FROM listens l JOIN tracks t ON t.id = l.track_id
      WHERE l.at >= ?1";

const THE_TRACKS_MOST_LISTENED_TO: &str =
    "SELECT t.id, t.title, count(*), coalesce(sum(l.heard), 0) AS listened
       FROM listens l JOIN tracks t ON t.id = l.track_id
      WHERE l.at >= ?1
      GROUP BY t.id
      ORDER BY listened DESC, count(*) DESC, t.title COLLATE NOCASE
      LIMIT ?2";

const THE_ALBUMS_MOST_LISTENED_TO: &str =
    "SELECT a.id, a.title, count(*), coalesce(sum(l.heard), 0) AS listened
       FROM listens l
       JOIN tracks t ON t.id = l.track_id
       JOIN albums a ON a.id = t.album_id
      WHERE l.at >= ?1
      GROUP BY a.id
      ORDER BY listened DESC, count(*) DESC, a.title COLLATE NOCASE
      LIMIT ?2";

const THE_ARTISTS_MOST_LISTENED_TO: &str =
    "SELECT r.id, r.name, count(*), coalesce(sum(l.heard), 0) AS listened
       FROM listens l
       JOIN tracks t ON t.id = l.track_id
       JOIN artists r ON r.id = t.artist_id
      WHERE l.at >= ?1
      GROUP BY r.id
      ORDER BY listened DESC, count(*) DESC, r.name COLLATE NOCASE
      LIMIT ?2";

const WHAT_WAS_HEARD_EACH_DAY: &str = "SELECT l.at / ?1 AS day, count(*),
            coalesce(sum(l.heard), 0)
       FROM listens l
      WHERE l.at >= ?2
      GROUP BY day
      ORDER BY day";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Window {
    Week,
    Month,
    Year,
    #[default]
    Everything,
}

impl Window {
    pub const ALL: [Self; 4] = [Self::Week, Self::Month, Self::Year, Self::Everything];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Week => "week",
            Self::Month => "month",
            Self::Year => "year",
            Self::Everything => "everything",
        }
    }

    pub fn since(&self) -> Option<SystemTime> {
        let ago = match self {
            Self::Week => A_WEEK,
            Self::Month => A_MONTH,
            Self::Year => A_YEAR,
            Self::Everything => return None,
        };

        Some(SystemTime::now().checked_sub(ago).unwrap_or(UNIX_EPOCH))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Statistics {
    pub plays: u64,
    pub listened: Duration,
    pub tracks: u64,
    pub albums: u64,
    pub artists: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listened<Id> {
    pub id: Id,
    pub name: String,
    pub plays: u64,
    pub listened: Duration,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MostListened {
    pub tracks: Vec<Listened<TrackId>>,
    pub albums: Vec<Listened<AlbumId>>,
    pub artists: Vec<Listened<ArtistId>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Day {
    pub at: SystemTime,
    pub plays: u64,
    pub listened: Duration,
}

struct Counted {
    day: i64,
    plays: i64,
    listened: i64,
}

pub(crate) fn statistics(inner: &Inner, window: Window) -> Result<Statistics> {
    let since = from_when(window);

    inner.read(|connection| {
        connection
            .query_row(WHAT_WAS_HEARD, params![since], |row| {
                Ok(Statistics {
                    plays: how_many(row.get(0)?),
                    listened: how_long(row.get(1)?),
                    tracks: how_many(row.get(2)?),
                    albums: how_many(row.get(3)?),
                    artists: how_many(row.get(4)?),
                })
            })
            .map_err(|source| Error::store(StoreOp::Query, source))
    })
}

pub(crate) fn most_listened(inner: &Inner, window: Window, most: usize) -> Result<MostListened> {
    let since = from_when(window);
    let most = most as i64;

    inner.read(|connection| {
        Ok(MostListened {
            tracks: listened(
                connection,
                THE_TRACKS_MOST_LISTENED_TO,
                since,
                most,
                TrackId::new,
            )?,
            albums: listened(
                connection,
                THE_ALBUMS_MOST_LISTENED_TO,
                since,
                most,
                AlbumId::new,
            )?,
            artists: listened(
                connection,
                THE_ARTISTS_MOST_LISTENED_TO,
                since,
                most,
                ArtistId::new,
            )?,
        })
    })
}

pub(crate) fn listening_by_day(inner: &Inner, window: Window) -> Result<Vec<Day>> {
    let since = from_when(window);
    let counted = inner.read(|connection| {
        let mut statement = connection
            .prepare(WHAT_WAS_HEARD_EACH_DAY)
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;

        statement
            .query_map(params![NANOS_PER_DAY, since], |row| {
                Ok(Counted {
                    day: row.get(0)?,
                    plays: row.get(1)?,
                    listened: row.get(2)?,
                })
            })
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))
    })?;

    Ok(filled(
        &counted,
        first_day(window, &counted),
        day_of(SystemTime::now()),
    ))
}

fn listened<Id>(
    connection: &Connection,
    sql: &str,
    since: i64,
    most: i64,
    named: fn(u64) -> resonate_core::Result<Id>,
) -> Result<Vec<Listened<Id>>> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let held = statement
        .query_map(params![since, most], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    held.into_iter()
        .map(|(id, name, plays, listened)| {
            Ok(Listened {
                id: named(id as u64)?,
                name,
                plays: how_many(plays),
                listened: how_long(listened),
            })
        })
        .collect()
}

fn first_day(window: Window, counted: &[Counted]) -> Option<i64> {
    match window.since() {
        Some(since) => Some(day_of(since)),
        None => counted.first().map(|heard| heard.day),
    }
}

fn filled(counted: &[Counted], from: Option<i64>, today: i64) -> Vec<Day> {
    let Some(from) = from else {
        return Vec::new();
    };
    let mut held = counted
        .iter()
        .filter(|heard| (from..=today).contains(&heard.day))
        .peekable();
    let mut days = Vec::new();

    for day in from..=today {
        let heard = held.next_if(|heard| heard.day == day);
        days.push(Day {
            at: midnight_of(day),
            plays: heard.map_or(0, |heard| how_many(heard.plays)),
            listened: heard.map_or(Duration::ZERO, |heard| how_long(heard.listened)),
        });
    }

    days
}

fn from_when(window: Window) -> i64 {
    window.since().map_or(SINCE_THE_BEGINNING, store::to_nanos)
}

fn day_of(at: SystemTime) -> i64 {
    store::to_nanos(at).div_euclid(NANOS_PER_DAY)
}

fn midnight_of(day: i64) -> SystemTime {
    store::from_nanos(day.saturating_mul(NANOS_PER_DAY))
}

fn how_many(counted: i64) -> u64 {
    u64::try_from(counted).unwrap_or_default()
}

fn how_long(nanos: i64) -> Duration {
    Duration::from_nanos(u64::try_from(nanos).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use resonate_core::SampleFormat;

    use super::*;
    use crate::{Codec, Library};

    const A_MINUTE: Duration = Duration::from_secs(60);
    const TWO_DAYS: Duration = Duration::from_secs(2 * SECONDS_PER_DAY);
    const A_FORTNIGHT: Duration = Duration::from_secs(14 * SECONDS_PER_DAY);

    fn counted_at(library: &Library, title: &str, listens: &[(Duration, Duration)]) {
        library
            .inner()
            .write(|transaction| {
                transaction
                    .execute(
                        "INSERT INTO roots (id, path) VALUES (1, '/music')
                         ON CONFLICT(id) DO NOTHING",
                        [],
                    )
                    .map_err(|source| Error::store(StoreOp::Insert, source))?;
                let track: i64 = transaction
                    .query_row(
                        "INSERT INTO tracks (root_id, path, title, sample_rate, channels,
                                             sample_format, codec, file_size, modified, added,
                                             seen, plays)
                         VALUES (1, ?1, ?2, 44100, 2, ?3, ?4, 0, 0, 0, 0, ?5)
                         RETURNING id",
                        params![
                            format!("/music/{title}.wav"),
                            title,
                            store::format_code(SampleFormat::S16),
                            store::codec_code(Codec::Pcm),
                            listens.len() as i64,
                        ],
                        |row| row.get(0),
                    )
                    .map_err(|source| Error::store(StoreOp::Insert, source))?;

                for (ago, heard) in listens {
                    transaction
                        .execute(
                            "INSERT INTO listens (track_id, at, heard) VALUES (?1, ?2, ?3)",
                            params![
                                track,
                                store::to_nanos(
                                    SystemTime::now().checked_sub(*ago).unwrap_or(UNIX_EPOCH)
                                ),
                                i64::try_from(heard.as_nanos()).unwrap_or(i64::MAX),
                            ],
                        )
                        .map_err(|source| Error::store(StoreOp::Insert, source))?;
                }

                Ok(())
            })
            .expect("the catalog takes a track and the listens counted against it");
    }

    #[test]
    fn a_window_reaches_back_as_far_as_it_is_named_for() {
        let now = SystemTime::now();
        for (window, ago) in [
            (Window::Week, A_WEEK),
            (Window::Month, A_MONTH),
            (Window::Year, A_YEAR),
        ] {
            let since = window.since().expect("a bounded window reaches back");
            let reached = now
                .duration_since(since)
                .expect("a bounded window begins before now");
            assert!(
                reached.abs_diff(ago) < A_MINUTE,
                "{} reached back {reached:?} rather than {ago:?}",
                window.name()
            );
        }
        assert_eq!(Window::Everything.since(), None);
    }

    #[test]
    fn what_was_heard_this_week_counts_only_this_week() {
        let library = Library::open_in_memory().expect("an in-memory catalog opens");
        counted_at(&library, "Lately", &[(TWO_DAYS, A_MINUTE)]);
        counted_at(&library, "Once", &[(A_FORTNIGHT, A_MINUTE)]);

        let week = library
            .statistics(Window::Week)
            .expect("the catalog answers for a week");
        assert_eq!(week.plays, 1);
        assert_eq!(week.tracks, 1);
        assert_eq!(week.listened, A_MINUTE);

        let everything = library
            .statistics(Window::Everything)
            .expect("the catalog answers for everything");
        assert_eq!(everything.plays, 2);
        assert_eq!(everything.tracks, 2);
        assert_eq!(everything.listened, A_MINUTE * 2);
    }

    #[test]
    fn the_days_a_chart_draws_have_no_gaps_in_them() {
        let library = Library::open_in_memory().expect("an in-memory catalog opens");
        counted_at(
            &library,
            "Echoes",
            &[(Duration::ZERO, A_MINUTE), (TWO_DAYS, A_MINUTE)],
        );

        let drawn = library
            .listening_by_day(Window::Week)
            .expect("the catalog answers for a week");
        assert_eq!(drawn.len(), 8, "a week is the day it began and seven since");
        assert!(
            drawn.windows(2).all(|pair| {
                pair[1]
                    .at
                    .duration_since(pair[0].at)
                    .is_ok_and(|apart| apart == Duration::from_secs(SECONDS_PER_DAY))
            }),
            "the days a chart draws are not one day apart"
        );
        assert_eq!(
            drawn.iter().map(|day| day.plays).sum::<u64>(),
            2,
            "the plays the chart draws are not the plays that were counted"
        );
        assert_eq!(
            drawn.last().map(|day| day.plays),
            Some(1),
            "today holds the play counted today"
        );
    }

    #[test]
    fn a_day_is_the_midnight_it_began_at() {
        let day = day_of(UNIX_EPOCH + Duration::from_secs(SECONDS_PER_DAY * 3 + 4_000));
        assert_eq!(day, 3);
        assert_eq!(
            midnight_of(day),
            UNIX_EPOCH + Duration::from_secs(SECONDS_PER_DAY * 3)
        );
    }

    #[test]
    fn a_chart_of_nothing_at_all_draws_nothing() {
        assert!(filled(&[], None, 10).is_empty());
        assert!(filled(&[], Some(11), 10).is_empty());
    }
}
