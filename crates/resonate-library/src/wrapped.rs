use std::time::Duration;

use resonate_codec::{Codec, LISTED_APART_BY};
use resonate_core::{
    Calendar,
    wrapped::{
        FirstSong, FormatHeard, FormatShares, QuarterHeard, RANKED_AT_MOST, Ranked, Rhythm,
        YearReview, genres_tallied,
    },
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{Error, HistoryKept, Result, StoreOp, db::Inner, store};

const NANOS_PER_QUARTER_HOUR: i64 = 15 * 60 * 1_000_000_000;

const CD_SAMPLE_RATE: i64 = 44_100;

const CD_SAMPLE_DEPTH: u8 = 16;

const GENRE_ROWS_RANKED_AT_MOST: i64 = 10_000;

const THE_FIRST_LISTEN_EVER: &str = "SELECT min(at) FROM (
         SELECT min(at) AS at FROM listens
         UNION ALL SELECT min(at) FROM passes
         UNION ALL SELECT min(at) FROM unheld_listens
     )";

const THE_LAST_LISTEN_EVER: &str = "SELECT max(at) FROM (
         SELECT max(at) AS at FROM listens
         UNION ALL SELECT max(at) FROM passes
         UNION ALL SELECT max(at) FROM unheld_listens
     )";

const ANY_LISTEN_BETWEEN: &str = "SELECT EXISTS (SELECT 1 FROM listens WHERE at >= ?1 AND at < ?2)
      OR EXISTS (SELECT 1 FROM passes WHERE at >= ?1 AND at < ?2)
      OR EXISTS (SELECT 1 FROM unheld_listens WHERE at >= ?1 AND at < ?2)";

const WHAT_WAS_HEARD_IN_THE_YEAR: &str = "SELECT count(*)
         + (SELECT count(*) FROM unheld_listens u WHERE u.at >= ?1 AND u.at < ?2),
       coalesce(sum(l.heard), 0)
         + (SELECT coalesce(sum(p.heard), 0) FROM passes p WHERE p.at >= ?1 AND p.at < ?2)
         + (SELECT coalesce(sum(u.heard), 0) FROM unheld_listens u WHERE u.at >= ?1 AND u.at < ?2),
       count(DISTINCT coalesce(t.alternative_of, t.id)), count(DISTINCT t.artist_id),
       count(DISTINCT t.album_id)
  FROM listens l JOIN tracks t ON t.id = l.track_id
 WHERE l.at >= ?1 AND l.at < ?2";

const THE_SONGS_OF_THE_YEAR: &str =
    "SELECT min(t.title), min(coalesce(t.artist, '')), count(*), coalesce(sum(l.heard), 0) AS listened
       FROM listens l JOIN tracks t ON t.id = l.track_id
      WHERE l.at >= ?1 AND l.at < ?2
      GROUP BY coalesce(t.alternative_of, t.id)
      ORDER BY listened DESC, count(*) DESC, min(t.title) COLLATE NOCASE
      LIMIT ?3";

const THE_ALBUMS_OF_THE_YEAR: &str =
    "SELECT coalesce(a.release_title, a.title), coalesce(owner.name, ''), count(*),
            coalesce(sum(l.heard), 0) AS listened
       FROM listens l
       JOIN tracks t ON t.id = l.track_id
       JOIN albums a ON a.id = t.album_id
       LEFT JOIN artists owner ON owner.id = a.artist_id
      WHERE l.at >= ?1 AND l.at < ?2
      GROUP BY a.id
      ORDER BY listened DESC, count(*) DESC, coalesce(a.release_title, a.title) COLLATE NOCASE
      LIMIT ?3";

const THE_ARTISTS_OF_THE_YEAR: &str =
    "SELECT r.name, '', count(*), coalesce(sum(l.heard), 0) AS listened
       FROM listens l
       JOIN tracks t ON t.id = l.track_id
       JOIN artists r ON r.id = t.artist_id
      WHERE l.at >= ?1 AND l.at < ?2
      GROUP BY r.id
      ORDER BY listened DESC, count(*) DESC, r.name COLLATE NOCASE
      LIMIT ?3";

const THE_GENRES_OF_THE_YEAR: &str =
    "SELECT t.genre, count(*), coalesce(sum(l.heard), 0) AS listened
       FROM listens l JOIN tracks t ON t.id = l.track_id
      WHERE l.at >= ?1 AND l.at < ?2 AND t.genre IS NOT NULL AND t.genre != ''
      GROUP BY t.genre
      ORDER BY listened DESC
      LIMIT ?3";

const THE_QUARTERS_OF_THE_YEAR: &str = "SELECT quarter, sum(listened) FROM (
         SELECT l.at / ?3 AS quarter, coalesce(sum(l.heard), 0) AS listened
           FROM listens l WHERE l.at >= ?1 AND l.at < ?2 GROUP BY quarter
         UNION ALL
         SELECT p.at / ?3 AS quarter, coalesce(sum(p.heard), 0) AS listened
           FROM passes p WHERE p.at >= ?1 AND p.at < ?2 GROUP BY quarter
         UNION ALL
         SELECT u.at / ?3 AS quarter, coalesce(sum(u.heard), 0) AS listened
           FROM unheld_listens u WHERE u.at >= ?1 AND u.at < ?2 GROUP BY quarter
      )
      GROUP BY quarter
      ORDER BY quarter";

const THE_FORMATS_OF_THE_YEAR: &str =
    "SELECT t.codec, t.sample_rate, t.sample_format, coalesce(sum(l.heard), 0)
       FROM listens l JOIN tracks t ON t.id = l.track_id
      WHERE l.at >= ?1 AND l.at < ?2
      GROUP BY t.codec, t.sample_rate, t.sample_format";

const THE_SONGS_FIRST_HEARD_IN_THE_YEAR: &str = "SELECT count(*) FROM (
         SELECT coalesce(t.alternative_of, t.id), min(l.at) AS first
           FROM listens l JOIN tracks t ON t.id = l.track_id
          GROUP BY coalesce(t.alternative_of, t.id)
         HAVING first >= ?1 AND first < ?2
      )";

const THE_SONGS_ADDED_IN_THE_YEAR: &str =
    "SELECT count(*) FROM tracks WHERE added >= ?1 AND added < ?2 AND alternative_of IS NULL";

const THE_FIRST_SONG_OF_THE_YEAR: &str = "SELECT t.title, coalesce(t.artist, ''), l.at
       FROM listens l JOIN tracks t ON t.id = l.track_id
      WHERE l.at >= ?1 AND l.at < ?2
      ORDER BY l.at, l.id
      LIMIT 1";

pub(crate) fn years_heard(inner: &Inner, calendar: &Calendar) -> Result<Vec<i64>> {
    inner.read(|connection| {
        let bound = |sql: &str| -> Result<Option<i64>> {
            connection
                .query_row(sql, [], |row| row.get(0))
                .map_err(|source| Error::store(StoreOp::Query, source))
        };
        let (Some(first), Some(last)) =
            (bound(THE_FIRST_LISTEN_EVER)?, bound(THE_LAST_LISTEN_EVER)?)
        else {
            return Ok(Vec::new());
        };

        let first = calendar.date_at(store::from_nanos(first)).year;
        let last = calendar.date_at(store::from_nanos(last)).year;
        let mut years = Vec::new();
        for year in (first..=last).rev() {
            let (from, to) = bounds_of(calendar, year);
            let held = connection
                .query_row(ANY_LISTEN_BETWEEN, params![from, to], |row| {
                    row.get::<_, bool>(0)
                })
                .map_err(|source| Error::store(StoreOp::Query, source))?;
            if held {
                years.push(year);
            }
        }

        Ok(years)
    })
}

pub(crate) fn year_review(
    inner: &Inner,
    year: i64,
    calendar: &Calendar,
    history: HistoryKept,
) -> Result<YearReview> {
    let (from, to) = bounds_of(calendar, year);

    inner.read(|connection| {
        let (plays, listened, songs, artists, albums) = connection
            .query_row(WHAT_WAS_HEARD_IN_THE_YEAR, params![from, to], |row| {
                Ok((
                    how_many(row.get(0)?),
                    how_long(row.get(1)?),
                    how_many(row.get(2)?),
                    how_many(row.get(3)?),
                    how_many(row.get(4)?),
                ))
            })
            .map_err(|source| Error::store(StoreOp::Query, source))?;

        Ok(YearReview {
            year,
            listened,
            plays,
            songs,
            artists,
            albums,
            songs_new: songs_new(connection, from, to, history)?,
            songs_added: how_many(
                connection
                    .query_row(THE_SONGS_ADDED_IN_THE_YEAR, params![from, to], |row| {
                        row.get(0)
                    })
                    .map_err(|source| Error::store(StoreOp::Query, source))?,
            ),
            top_songs: ranked(connection, THE_SONGS_OF_THE_YEAR, from, to)?,
            top_artists: ranked(connection, THE_ARTISTS_OF_THE_YEAR, from, to)?,
            top_albums: ranked(connection, THE_ALBUMS_OF_THE_YEAR, from, to)?,
            top_genres: genres(connection, from, to)?,
            rhythm: rhythm(connection, from, to, calendar, year)?,
            first_song: first_song(connection, from, to, calendar)?,
            formats: formats(connection, from, to)?,
        })
    })
}

fn bounds_of(calendar: &Calendar, year: i64) -> (i64, i64) {
    let (from, to) = calendar.year_bounds(year);

    (store::to_nanos(from), store::to_nanos(to))
}

fn songs_new(
    connection: &Connection,
    from: i64,
    to: i64,
    history: HistoryKept,
) -> Result<Option<u64>> {
    if history != HistoryKept::Forever {
        return Ok(None);
    }

    let first_ever: Option<i64> = connection
        .query_row(THE_FIRST_LISTEN_EVER, [], |row| row.get(0))
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    if first_ever.is_none_or(|first| first >= from) {
        return Ok(None);
    }

    connection
        .query_row(
            THE_SONGS_FIRST_HEARD_IN_THE_YEAR,
            params![from, to],
            |row| row.get(0),
        )
        .map(|count| Some(how_many(count)))
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn ranked(connection: &Connection, sql: &str, from: i64, to: i64) -> Result<Vec<Ranked>> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;

    statement
        .query_map(params![from, to, RANKED_AT_MOST as i64], |row| {
            Ok(Ranked {
                name: row.get(0)?,
                by: given(row.get(1)?),
                plays: how_many(row.get(2)?),
                listened: how_long(row.get(3)?),
            })
        })
        .and_then(|rows| rows.collect())
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn genres(connection: &Connection, from: i64, to: i64) -> Result<Vec<Ranked>> {
    let mut statement = connection
        .prepare(THE_GENRES_OF_THE_YEAR)
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let listed = statement
        .query_map(params![from, to, GENRE_ROWS_RANKED_AT_MOST], |row| {
            Ok((
                row.get::<_, String>(0)?,
                how_many(row.get(1)?),
                how_long(row.get(2)?),
            ))
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    Ok(genres_tallied(listed, LISTED_APART_BY, RANKED_AT_MOST))
}

fn rhythm(
    connection: &Connection,
    from: i64,
    to: i64,
    calendar: &Calendar,
    year: i64,
) -> Result<Rhythm> {
    let mut statement = connection
        .prepare(THE_QUARTERS_OF_THE_YEAR)
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let quarters = statement
        .query_map(params![from, to, NANOS_PER_QUARTER_HOUR], |row| {
            let quarter: i64 = row.get(0)?;

            Ok(QuarterHeard {
                at: store::from_nanos(quarter.saturating_mul(NANOS_PER_QUARTER_HOUR)),
                listened: how_long(row.get(1)?),
            })
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    Ok(Rhythm::of(quarters, calendar, year))
}

fn first_song(
    connection: &Connection,
    from: i64,
    to: i64,
    calendar: &Calendar,
) -> Result<Option<FirstSong>> {
    connection
        .query_row(THE_FIRST_SONG_OF_THE_YEAR, params![from, to], |row| {
            Ok(FirstSong {
                title: row.get(0)?,
                by: given(row.get(1)?),
                on: calendar.date_at(store::from_nanos(row.get(2)?)),
            })
        })
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn formats(connection: &Connection, from: i64, to: i64) -> Result<Option<FormatShares>> {
    let mut statement = connection
        .prepare(THE_FORMATS_OF_THE_YEAR)
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let heard = statement
        .query_map(params![from, to], |row| {
            let rate: i64 = row.get(1)?;
            let word = store::format_named_by(row.get(2)?);

            Ok(FormatHeard {
                lossless: store::codec_named_by(row.get(0)?).is_some_and(Codec::is_lossless),
                above_cd: rate > CD_SAMPLE_RATE
                    || word.is_some_and(|word| word.valid_bits() > CD_SAMPLE_DEPTH),
                listened: how_long(row.get(3)?),
            })
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    Ok(FormatShares::of(heard))
}

fn given(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}

fn how_many(counted: i64) -> u64 {
    u64::try_from(counted).unwrap_or_default()
}

fn how_long(nanos: i64) -> Duration {
    Duration::from_nanos(u64::try_from(nanos).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use resonate_core::{SampleFormat, wrapped::SlideKind};

    use super::*;
    use crate::Library;

    const THE_FIRST_OF_JANUARY_2025: u64 = 20_089;
    const SECONDS_A_DAY: u64 = 86_400;
    const A_MINUTE: i64 = 60_000_000_000;

    struct Heard<'a> {
        title: &'a str,
        codec: Codec,
        rate: i64,
        format: SampleFormat,
        genre: Option<&'a str>,
        days_into_2025: i64,
        minutes: i64,
    }

    fn nanos_of_day(days_into_2025: i64) -> i64 {
        store::to_nanos(
            UNIX_EPOCH
                + Duration::from_secs(
                    (THE_FIRST_OF_JANUARY_2025 as i64 + days_into_2025) as u64 * SECONDS_A_DAY,
                ),
        )
    }

    fn heard(library: &Library, song: &Heard<'_>) {
        library
            .inner()
            .write(|transaction| {
                transaction
                    .execute(
                        "INSERT INTO roots (id, path) VALUES (1, '/music') ON CONFLICT(id) DO NOTHING",
                        [],
                    )
                    .map_err(|source| Error::store(StoreOp::Insert, source))?;
                let path = format!("/music/{}.flac", song.title);
                let track: i64 = transaction
                    .query_row(
                        "INSERT INTO tracks (root_id, path, title, artist, sample_rate, channels,
                                             sample_format, codec, file_size, modified, added,
                                             seen, genre)
                         VALUES (1, ?1, ?2, 'Ada', ?3, 2, ?4, ?5, 0, 0, ?6, 0, ?7)
                         ON CONFLICT(path, span_start) DO UPDATE SET title = excluded.title
                         RETURNING id",
                        params![
                            path,
                            song.title,
                            song.rate,
                            store::format_code(song.format),
                            store::codec_code(song.codec),
                            nanos_of_day(song.days_into_2025),
                            song.genre,
                        ],
                        |row| row.get(0),
                    )
                    .map_err(|source| Error::store(StoreOp::Insert, source))?;
                transaction
                    .execute(
                        "INSERT INTO listens (track_id, at, heard) VALUES (?1, ?2, ?3)",
                        params![track, nanos_of_day(song.days_into_2025), song.minutes * A_MINUTE],
                    )
                    .map_err(|source| Error::store(StoreOp::Insert, source))?;

                Ok(())
            })
            .expect("the catalog takes a song and a listen");
    }

    fn a_song<'a>(title: &'a str, days_into_2025: i64, minutes: i64) -> Heard<'a> {
        Heard {
            title,
            codec: Codec::Flac,
            rate: 44_100,
            format: SampleFormat::S16,
            genre: None,
            days_into_2025,
            minutes,
        }
    }

    fn review_of_2025(library: &Library, history: HistoryKept) -> YearReview {
        library
            .year_review(2025, &Calendar::utc(), history)
            .expect("the catalog answers for a year")
    }

    #[test]
    fn a_year_counts_what_was_heard_inside_it_and_nothing_either_side() {
        let library = Library::open_in_memory().expect("an in-memory catalog opens");
        heard(&library, &a_song("Before", -1, 30));
        heard(&library, &a_song("Within", 0, 10));
        heard(&library, &a_song("Within", 1, 10));
        heard(&library, &a_song("After", 365, 30));

        let review = review_of_2025(&library, HistoryKept::Forever);

        assert_eq!(review.plays, 2);
        assert_eq!(review.songs, 1);
        assert_eq!(review.listened, Duration::from_secs(20 * 60));
        assert_eq!(review.top_songs[0].name, "Within");
        assert_eq!(review.top_songs[0].by.as_deref(), Some("Ada"));
        assert_eq!(review.rhythm.days_heard, 2);
        assert_eq!(
            review.first_song.map(|first| first.title),
            Some("Within".to_owned())
        );
    }

    #[test]
    fn passes_are_listening_time_but_never_a_play_or_a_rank() {
        let library = Library::open_in_memory().expect("an in-memory catalog opens");
        heard(&library, &a_song("Kept", 3, 10));
        library
            .inner()
            .write(|transaction| {
                transaction
                    .execute(
                        "INSERT INTO passes (at, heard) VALUES (?1, ?2)",
                        params![nanos_of_day(4), 5 * A_MINUTE],
                    )
                    .map_err(|source| Error::store(StoreOp::Insert, source))
                    .map(drop)
            })
            .expect("the catalog keeps a pass");

        let review = review_of_2025(&library, HistoryKept::Forever);

        assert_eq!(review.plays, 1);
        assert_eq!(review.listened, Duration::from_secs(15 * 60));
        assert_eq!(review.top_songs.len(), 1);
        assert_eq!(review.top_songs[0].listened, Duration::from_secs(10 * 60));
        assert_eq!(review.rhythm.days_heard, 2);
    }

    #[test]
    fn new_songs_are_counted_only_where_history_reaches_before_the_year_and_was_kept_whole() {
        let library = Library::open_in_memory().expect("an in-memory catalog opens");
        heard(&library, &a_song("Old", -30, 5));
        heard(&library, &a_song("Old", 2, 5));
        heard(&library, &a_song("New", 3, 5));

        let kept_whole = review_of_2025(&library, HistoryKept::Forever);
        let aged = review_of_2025(
            &library,
            HistoryKept::for_days(365).expect("a year is a span"),
        );

        assert_eq!(kept_whole.songs_new, Some(1));
        assert_eq!(aged.songs_new, None);

        let begun_this_year = Library::open_in_memory().expect("an in-memory catalog opens");
        heard(&begun_this_year, &a_song("New", 3, 5));

        assert_eq!(
            review_of_2025(&begun_this_year, HistoryKept::Forever).songs_new,
            None,
            "every song of a history that began this year would read as new"
        );
    }

    #[test]
    fn the_sound_is_shared_out_by_time_heard_and_genres_listed_apart_are_counted_apart() {
        let library = Library::open_in_memory().expect("an in-memory catalog opens");
        heard(
            &library,
            &Heard {
                genre: Some("Jazz; Fusion"),
                ..a_song("Studio", 0, 30)
            },
        );
        heard(
            &library,
            &Heard {
                rate: 96_000,
                format: SampleFormat::S24,
                genre: Some("Jazz"),
                ..a_song("Master", 1, 10)
            },
        );
        heard(
            &library,
            &Heard {
                codec: Codec::Mp3,
                ..a_song("Rip", 2, 10)
            },
        );

        let review = review_of_2025(&library, HistoryKept::Forever);
        let formats = review.formats.expect("something in the library was heard");

        assert_eq!(formats.lossless_permille, 800);
        assert_eq!(formats.high_resolution_permille, 200);
        assert_eq!(
            review
                .top_genres
                .iter()
                .map(|genre| genre.name.as_str())
                .collect::<Vec<_>>(),
            ["Jazz", "Fusion"]
        );
    }

    #[test]
    fn the_years_heard_are_listed_newest_first_and_a_year_with_nothing_is_left_out() {
        let library = Library::open_in_memory().expect("an in-memory catalog opens");
        assert_eq!(
            library
                .years_heard(&Calendar::utc())
                .expect("the catalog answers"),
            Vec::<i64>::new()
        );

        heard(&library, &a_song("Long ago", -366, 5));
        heard(&library, &a_song("Lately", 400, 5));

        assert_eq!(
            library
                .years_heard(&Calendar::utc())
                .expect("the catalog answers"),
            [2026, 2024]
        );
    }

    #[test]
    fn a_year_holding_a_song_makes_slides_of_it_and_one_holding_nothing_says_so() {
        let library = Library::open_in_memory().expect("an in-memory catalog opens");
        heard(&library, &a_song("Within", 0, 90));

        let kinds: Vec<SlideKind> = review_of_2025(&library, HistoryKept::Forever)
            .slides()
            .iter()
            .map(|slide| slide.kind)
            .collect();
        let empty = library
            .year_review(1999, &Calendar::utc(), HistoryKept::Forever)
            .expect("the catalog answers for a year");

        assert_eq!(kinds.first(), Some(&SlideKind::Whole));
        assert!(kinds.contains(&SlideKind::Songs));
        assert_eq!(kinds.last(), Some(&SlideKind::Farewell));
        assert!(!empty.holds_anything());
    }
}
