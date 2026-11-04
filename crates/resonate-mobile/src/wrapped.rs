use std::time::{Duration, SystemTime, UNIX_EPOCH};

use resonate_codec::LISTED_APART_BY;
use resonate_core::{
    Calendar, seconds_since_the_epoch,
    wrapped::{
        FirstSong, FormatHeard, FormatShares, QuarterHeard, RANKED_AT_MOST, Ranked, Rhythm, Slide,
        YearReview, genres_tallied, year_worth_showing,
    },
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{Error, Result, StoreOp, catalog::Catalog, search::codec_named};

const SECONDS_A_QUARTER_HOUR: i64 = 900;

const CD_SAMPLE_RATE: i64 = 44_100;

const CD_SAMPLE_DEPTH: i64 = 16;

const GENRE_ROWS_RANKED_AT_MOST: i64 = 10_000;

const THE_FIRST_LISTEN_EVER: &str = "SELECT min(began) FROM listens";

const THE_LAST_LISTEN_EVER: &str = "SELECT max(began) FROM listens";

const ANY_LISTEN_BETWEEN: &str =
    "SELECT EXISTS (SELECT 1 FROM listens WHERE began >= ?1 AND began < ?2)";

const WHAT_WAS_HEARD_IN_THE_YEAR: &str = "SELECT
       coalesce(sum(l.counted), 0),
       coalesce(sum(l.heard_ms), 0),
       count(DISTINCT CASE WHEN l.counted = 1 THEN l.track END)
         + count(DISTINCT CASE WHEN l.counted = 1 THEN l.served END),
       count(DISTINCT CASE WHEN l.counted = 1 AND d.artist != '' THEN folded(d.artist) END),
       count(DISTINCT CASE WHEN l.counted = 1 AND d.album_key != '' THEN d.album_key END)
  FROM listens l LEFT JOIN documents d ON d.id = l.track
 WHERE l.began >= ?1 AND l.began < ?2";

const THE_SONGS_OF_THE_YEAR: &str =
    "SELECT min(d.title), min(d.artist), count(*), coalesce(sum(l.heard_ms), 0) AS listened
  FROM listens l JOIN documents d ON d.id = l.track
 WHERE d.missing = 0 AND l.counted = 1 AND l.began >= ?1 AND l.began < ?2
 GROUP BY d.id
 ORDER BY listened DESC, count(*) DESC, min(d.title) COLLATE NOCASE
 LIMIT ?3";

const THE_ALBUMS_OF_THE_YEAR: &str = "SELECT min(d.album), min(coalesce(nullif(d.album_artist, ''), d.artist)), count(*), coalesce(sum(l.heard_ms), 0) AS listened
  FROM listens l JOIN documents d ON d.id = l.track
 WHERE d.missing = 0 AND d.album_key != '' AND l.counted = 1 AND l.began >= ?1 AND l.began < ?2
 GROUP BY d.album_key
 ORDER BY listened DESC, count(*) DESC, min(d.album) COLLATE NOCASE
 LIMIT ?3";

const THE_ARTISTS_OF_THE_YEAR: &str =
    "SELECT min(d.artist), '', count(*), coalesce(sum(l.heard_ms), 0) AS listened
  FROM listens l JOIN documents d ON d.id = l.track
 WHERE d.missing = 0 AND d.artist != '' AND l.counted = 1 AND l.began >= ?1 AND l.began < ?2
 GROUP BY folded(d.artist)
 ORDER BY listened DESC, count(*) DESC, min(d.artist) COLLATE NOCASE
 LIMIT ?3";

const THE_GENRES_OF_THE_YEAR: &str =
    "SELECT d.genre, count(*), coalesce(sum(l.heard_ms), 0) AS listened
  FROM listens l JOIN documents d ON d.id = l.track
 WHERE d.missing = 0 AND d.genre != '' AND l.counted = 1 AND l.began >= ?1 AND l.began < ?2
 GROUP BY d.genre
 ORDER BY listened DESC
 LIMIT ?3";

const THE_QUARTERS_OF_THE_YEAR: &str = "SELECT began / ?3 AS quarter, coalesce(sum(heard_ms), 0)
  FROM listens
 WHERE began >= ?1 AND began < ?2
 GROUP BY quarter
 ORDER BY quarter";

const THE_FORMATS_OF_THE_YEAR: &str = "SELECT d.codec, d.rate, d.bits, coalesce(sum(l.heard_ms), 0)
  FROM listens l JOIN documents d ON d.id = l.track
 WHERE d.missing = 0 AND l.counted = 1 AND l.began >= ?1 AND l.began < ?2
 GROUP BY d.codec, d.rate, d.bits";

const THE_SONGS_FIRST_HEARD_IN_THE_YEAR: &str = "SELECT count(*) FROM (
    SELECT l.track, min(l.began) AS first
      FROM listens l JOIN documents d ON d.id = l.track
     WHERE d.missing = 0 AND l.counted = 1
     GROUP BY l.track
    HAVING first >= ?1 AND first < ?2
 )";

const THE_SONGS_ADDED_IN_THE_YEAR: &str =
    "SELECT count(*) FROM documents WHERE missing = 0 AND added >= ?1 AND added < ?2";

const THE_FIRST_SONG_OF_THE_YEAR: &str = "SELECT d.title, d.artist, l.began
  FROM listens l JOIN documents d ON d.id = l.track
 WHERE d.missing = 0 AND l.counted = 1 AND l.began >= ?1 AND l.began < ?2
 ORDER BY l.began, l.id
 LIMIT 1";

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct WrappedRow {
    pub name: String,
    pub by: Option<String>,
    pub figure: String,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct WrappedBar {
    pub label: String,
    pub said: String,
    pub share_permille: u16,
    pub peak: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct WrappedSlide {
    pub eyebrow: String,
    pub headline: String,
    pub detail: Option<String>,
    pub rows: Vec<WrappedRow>,
    pub bars: Vec<WrappedBar>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct WrappedYear {
    pub year: i64,
    pub years: Vec<i64>,
    pub slides: Vec<WrappedSlide>,
    pub summary: String,
}

impl From<Slide> for WrappedSlide {
    fn from(slide: Slide) -> Self {
        Self {
            eyebrow: slide.eyebrow,
            headline: slide.headline,
            detail: slide.detail,
            rows: slide
                .rows
                .into_iter()
                .map(|row| WrappedRow {
                    name: row.name,
                    by: row.by,
                    figure: row.figure,
                })
                .collect(),
            bars: slide
                .bars
                .into_iter()
                .map(|bar| WrappedBar {
                    label: bar.label,
                    said: bar.said,
                    share_permille: bar.share_permille,
                    peak: bar.peak,
                })
                .collect(),
        }
    }
}

impl Catalog {
    pub(crate) fn wrapped(
        &self,
        year: Option<i64>,
        calendar: &Calendar,
    ) -> Result<Option<WrappedYear>> {
        let connection = self.0.lock();
        let years = years_heard(&connection, calendar)?;
        let today = calendar.date_at(SystemTime::now());
        let Some(year) = year.or_else(|| year_worth_showing(&years, today)) else {
            return Ok(None);
        };
        let review = year_review(&connection, year, calendar)?;
        drop(connection);

        Ok(Some(WrappedYear {
            year,
            years,
            slides: review.slides().into_iter().map(Into::into).collect(),
            summary: review.summary(),
        }))
    }
}

fn read_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Read, source)
}

fn years_heard(connection: &Connection, calendar: &Calendar) -> Result<Vec<i64>> {
    let bound = |sql: &str| -> Result<Option<i64>> {
        connection
            .query_row(sql, [], |row| row.get(0))
            .map_err(read_error)
    };
    let (Some(first), Some(last)) = (bound(THE_FIRST_LISTEN_EVER)?, bound(THE_LAST_LISTEN_EVER)?)
    else {
        return Ok(Vec::new());
    };

    let first = calendar.date_at(moment(first)).year;
    let last = calendar.date_at(moment(last)).year;
    let mut years = Vec::new();
    for year in (first..=last).rev() {
        let (from, to) = bounds_of(calendar, year);
        let held = connection
            .query_row(ANY_LISTEN_BETWEEN, params![from, to], |row| {
                row.get::<_, bool>(0)
            })
            .map_err(read_error)?;
        if held {
            years.push(year);
        }
    }

    Ok(years)
}

fn year_review(connection: &Connection, year: i64, calendar: &Calendar) -> Result<YearReview> {
    let (from, to) = bounds_of(calendar, year);
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
        .map_err(read_error)?;

    Ok(YearReview {
        year,
        listened,
        plays,
        songs,
        artists,
        albums,
        songs_new: songs_new(connection, from, to)?,
        songs_added: how_many(
            connection
                .query_row(THE_SONGS_ADDED_IN_THE_YEAR, params![from, to], |row| {
                    row.get(0)
                })
                .map_err(read_error)?,
        ),
        top_songs: ranked(connection, THE_SONGS_OF_THE_YEAR, from, to)?,
        top_artists: ranked(connection, THE_ARTISTS_OF_THE_YEAR, from, to)?,
        top_albums: ranked(connection, THE_ALBUMS_OF_THE_YEAR, from, to)?,
        top_genres: genres(connection, from, to)?,
        rhythm: rhythm(connection, from, to, calendar, year)?,
        first_song: first_song(connection, from, to, calendar)?,
        formats: formats(connection, from, to)?,
    })
}

fn bounds_of(calendar: &Calendar, year: i64) -> (i64, i64) {
    let (from, to) = calendar.year_bounds(year);

    (seconds_since_the_epoch(from), seconds_since_the_epoch(to))
}

fn songs_new(connection: &Connection, from: i64, to: i64) -> Result<Option<u64>> {
    let first_ever: Option<i64> = connection
        .query_row(THE_FIRST_LISTEN_EVER, [], |row| row.get(0))
        .map_err(read_error)?;
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
        .map_err(read_error)
}

fn ranked(connection: &Connection, sql: &str, from: i64, to: i64) -> Result<Vec<Ranked>> {
    let mut statement = connection.prepare(sql).map_err(read_error)?;

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
        .map_err(read_error)
}

fn genres(connection: &Connection, from: i64, to: i64) -> Result<Vec<Ranked>> {
    let mut statement = connection
        .prepare(THE_GENRES_OF_THE_YEAR)
        .map_err(read_error)?;
    let listed = statement
        .query_map(params![from, to, GENRE_ROWS_RANKED_AT_MOST], |row| {
            Ok((
                row.get::<_, String>(0)?,
                how_many(row.get(1)?),
                how_long(row.get(2)?),
            ))
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(read_error)?;

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
        .map_err(read_error)?;
    let quarters = statement
        .query_map(params![from, to, SECONDS_A_QUARTER_HOUR], |row| {
            let quarter: i64 = row.get(0)?;

            Ok(QuarterHeard {
                at: moment(quarter.saturating_mul(SECONDS_A_QUARTER_HOUR)),
                listened: how_long(row.get(1)?),
            })
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(read_error)?;

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
                on: calendar.date_at(moment(row.get(2)?)),
            })
        })
        .optional()
        .map_err(read_error)
}

fn formats(connection: &Connection, from: i64, to: i64) -> Result<Option<FormatShares>> {
    let mut statement = connection
        .prepare(THE_FORMATS_OF_THE_YEAR)
        .map_err(read_error)?;
    let heard = statement
        .query_map(params![from, to], |row| {
            let codec: String = row.get(0)?;
            let rate: i64 = row.get(1)?;
            let bits: Option<i64> = row.get(2)?;

            Ok(FormatHeard {
                lossless: codec_named(&codec).is_lossless(),
                above_cd: rate > CD_SAMPLE_RATE || bits.is_some_and(|bits| bits > CD_SAMPLE_DEPTH),
                listened: how_long(row.get(3)?),
            })
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(read_error)?;

    Ok(FormatShares::of(heard))
}

fn moment(seconds: i64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds.max(0).unsigned_abs())
}

fn given(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}

fn how_many(counted: i64) -> u64 {
    u64::try_from(counted).unwrap_or_default()
}

fn how_long(milliseconds: i64) -> Duration {
    Duration::from_millis(u64::try_from(milliseconds).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use resonate_core::CivilDate;

    use super::*;
    use crate::Track;

    const THE_FIRST_OF_JANUARY_2025: i64 = 20_089 * 86_400;
    const A_DAY: i64 = 86_400;
    const A_MINUTE_MS: i64 = 60_000;

    struct Song<'a> {
        name: &'a str,
        artist: &'a str,
        album: &'a str,
        genre: &'a str,
        codec: &'a str,
        rate: u32,
        bits: Option<u32>,
    }

    const PLAIN: Song<'static> = Song {
        name: "song",
        artist: "Air",
        album: "Moon Safari",
        genre: "",
        codec: "FLAC",
        rate: 44_100,
        bits: Some(16),
    };

    fn held(catalog: &Catalog, song: &Song<'_>) -> i64 {
        let id = catalog
            .upsert(Track {
                uri: format!("content://test/{}.flac", song.name),
                name: format!("{}.flac", song.name),
                title: song.name.into(),
                artist: song.artist.into(),
                album: song.album.into(),
                codec: song.codec.into(),
                rate: song.rate,
                duration_ms: 200_000,
                ..Track::default()
            })
            .expect("a song")
            .id as i64;

        catalog
            .0
            .lock()
            .execute(
                "UPDATE documents SET genre = ?2, bits = ?3 WHERE id = ?1",
                params![id, song.genre, song.bits],
            )
            .expect("what a scan would have read");

        id
    }

    fn heard(catalog: &Catalog, track: i64, day: i64, minutes: i64, counted: i64) {
        catalog
            .0
            .lock()
            .execute(
                "INSERT INTO listens (track, began, heard_ms, counted) VALUES (?1, ?2, ?3, ?4)",
                params![
                    track,
                    THE_FIRST_OF_JANUARY_2025 + day * A_DAY,
                    minutes * A_MINUTE_MS,
                    counted
                ],
            )
            .expect("a listen");
    }

    fn year_of(catalog: &Catalog, year: i64) -> YearReview {
        year_review(&catalog.0.lock(), year, &Calendar::utc()).expect("the catalog answers")
    }

    #[test]
    fn a_year_counts_what_was_heard_inside_it_and_a_visit_that_earned_no_play_is_time_alone() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let song = held(&catalog, &PLAIN);
        heard(&catalog, song, -1, 30, 1);
        heard(&catalog, song, 0, 10, 1);
        heard(&catalog, song, 1, 5, 0);
        heard(&catalog, song, 365, 30, 1);

        let review = year_of(&catalog, 2025);

        assert_eq!(review.plays, 1);
        assert_eq!(review.songs, 1);
        assert_eq!(review.listened, Duration::from_secs(15 * 60));
        assert_eq!(review.top_songs[0].listened, Duration::from_secs(10 * 60));
        assert_eq!(review.rhythm.days_heard, 2);
        assert_eq!(
            review.first_song.map(|first| first.on),
            Some(CivilDate {
                year: 2025,
                month: 1,
                day: 1
            })
        );
    }

    #[test]
    fn songs_new_are_counted_only_where_history_reaches_before_the_year() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let old = held(
            &catalog,
            &Song {
                name: "old",
                ..PLAIN
            },
        );
        let new = held(
            &catalog,
            &Song {
                name: "new",
                ..PLAIN
            },
        );
        heard(&catalog, old, 2, 5, 1);
        heard(&catalog, new, 3, 5, 1);

        assert_eq!(
            year_of(&catalog, 2025).songs_new,
            None,
            "every song of a history that began this year would read as new"
        );

        heard(&catalog, old, -30, 5, 1);

        assert_eq!(year_of(&catalog, 2025).songs_new, Some(1));
    }

    #[test]
    fn genres_are_split_in_rust_and_the_sound_shared_by_time_heard() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let studio = held(
            &catalog,
            &Song {
                name: "studio",
                genre: "Jazz; Fusion",
                ..PLAIN
            },
        );
        let master = held(
            &catalog,
            &Song {
                name: "master",
                genre: "Jazz",
                rate: 96_000,
                bits: Some(24),
                ..PLAIN
            },
        );
        let rip = held(
            &catalog,
            &Song {
                name: "rip",
                codec: "MP3",
                bits: None,
                ..PLAIN
            },
        );
        heard(&catalog, studio, 0, 30, 1);
        heard(&catalog, master, 1, 10, 1);
        heard(&catalog, rip, 2, 10, 1);

        let review = year_of(&catalog, 2025);
        let formats = review.formats.expect("a song was heard");

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
    fn a_catalog_that_heard_nothing_has_no_year_and_one_asked_for_any_year_answers_it() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");

        assert_eq!(
            catalog
                .wrapped(None, &Calendar::utc())
                .expect("the catalog answers"),
            None
        );

        let song = held(&catalog, &PLAIN);
        heard(&catalog, song, 0, 90, 1);

        let shown = catalog
            .wrapped(None, &Calendar::utc())
            .expect("the catalog answers")
            .expect("a year was heard");
        let empty = catalog
            .wrapped(Some(1999), &Calendar::utc())
            .expect("the catalog answers")
            .expect("a year asked for is answered");

        assert_eq!(shown.year, 2025);
        assert_eq!(shown.years, [2025]);
        assert_eq!(shown.slides[0].headline, "1 hour of music");
        assert!(shown.summary.starts_with("My 2025 on Resonate"));
        assert_eq!(empty.slides.len(), 1);
    }
}
