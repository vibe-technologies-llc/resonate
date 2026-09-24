use std::time::Duration;

use ahash::AHashMap;
use resonate_core::AlbumId;
use rusqlite::{Connection, Row, params};
use smallvec::smallvec;

use crate::{
    Asked, Clause, Column, Compare, Condition, Direction, Error, Result, SavedQuery, Search, Shape,
    SortOrder, StoreOp, Term, TrackQuery, Word,
    db::{self, Inner},
    search::Conditions,
    store,
};

const ENOUGH_FOR_A_DECADE: u32 = 20;
const ENOUGH_TO_OFFER: u32 = 10;

pub const PICTURED_BY_AT_MOST: usize = 4;

const MOST_DECADES: usize = 3;
const MOST_GENRES: usize = 4;
const MOST_ARTIST_MIXES: usize = 3;

const YEARS_IN_A_DECADE: i32 = 10;

const SECONDS_PER_DAY: u64 = 86_400;
const RECENTLY_ADDED_WITHIN: Duration = Duration::from_secs(30 * SECONDS_PER_DAY);
const NOT_HEARD_FOR: Duration = Duration::from_secs(365 * SECONDS_PER_DAY);
const A_LONG_PLAYER: Duration = Duration::from_secs(10 * 60);

const NEVER_PLAYED: u32 = 0;
const PLAYED_MORE_THAN: u32 = 5;

const DECADES_WITH_A_NAME: [(i32, &str); 8] = [
    (1920, "the twenties"),
    (1930, "the thirties"),
    (1940, "the forties"),
    (1950, "the fifties"),
    (1960, "the sixties"),
    (1970, "the seventies"),
    (1980, "the eighties"),
    (1990, "the nineties"),
];

const THE_DECADES_THE_CATALOG_HOLDS: &str = "SELECT (a.year / ?1) * ?1 AS decade,
            count(*) AS held
       FROM tracks t JOIN albums a ON a.id = t.album_id
      WHERE a.year IS NOT NULL
      GROUP BY decade
     HAVING held >= ?2
      ORDER BY held DESC, decade DESC
      LIMIT ?3";

const THE_GENRES_THE_FILES_NAME: &str = "SELECT genre, count(*) FROM tracks
      WHERE genre IS NOT NULL AND trim(genre) <> ''
      GROUP BY genre";

const THE_GENRES_THE_ARTISTS_WERE_GIVEN: &str = "SELECT g.name, count(*)
       FROM artist_genres g JOIN tracks t ON t.artist_id = g.artist_id
      WHERE trim(g.name) <> ''
      GROUP BY g.name";

const THE_ARTISTS_MOST_PLAYED: &str = "SELECT r.name, sum(t.plays) AS plays, count(*) AS held
       FROM tracks t JOIN artists r ON r.id = t.artist_id
      WHERE trim(r.name) <> ''
      GROUP BY r.id
      ORDER BY plays DESC, held DESC, r.name COLLATE NOCASE
      LIMIT ?1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Reason {
    Decade(i32),
    Genre,
    Artist,
    NeverHeard,
    MostPlayed,
    RecentlyAdded,
    BackCatalogue,
    HiRes,
    LongPlayers,
}

impl Reason {
    pub fn says(&self) -> String {
        match self {
            Self::Decade(decade) => format!("Everything from {}", spelled(*decade)),
            Self::Genre => "Everything filed under it".to_owned(),
            Self::Artist => "Everything they made".to_owned(),
            Self::NeverHeard => "You have never heard these".to_owned(),
            Self::MostPlayed => "The ones you come back to".to_owned(),
            Self::RecentlyAdded => "The newest in the library".to_owned(),
            Self::BackCatalogue => "Not heard for over a year".to_owned(),
            Self::HiRes => "Better than a CD".to_owned(),
            Self::LongPlayers => "Ten minutes and over".to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Listening,
    Eras,
    Genres,
    Artists,
    Sound,
}

impl Kind {
    pub const ALL: [Self; 5] = [
        Self::Listening,
        Self::Eras,
        Self::Genres,
        Self::Artists,
        Self::Sound,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Listening => "From your listening",
            Self::Eras => "Eras",
            Self::Genres => "Genres",
            Self::Artists => "Artists",
            Self::Sound => "Sound",
        }
    }
}

impl Reason {
    pub const fn kind(&self) -> Kind {
        match self {
            Self::NeverHeard | Self::MostPlayed | Self::RecentlyAdded | Self::BackCatalogue => {
                Kind::Listening
            }
            Self::Decade(_) => Kind::Eras,
            Self::Genre => Kind::Genres,
            Self::Artist => Kind::Artists,
            Self::HiRes | Self::LongPlayers => Kind::Sound,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub name: String,
    pub reason: Reason,
    pub query: SavedQuery,
    pub rows: u64,
    pub length: Option<Duration>,
    pub pictured_by: Vec<AlbumId>,
}

struct Candidate {
    name: String,
    reason: Reason,
    query: SavedQuery,
}

#[derive(Default)]
struct Spelt {
    rows: u64,
    spellings: AHashMap<String, u64>,
}

impl Spelt {
    fn taking(&mut self, spelling: &str, rows: u64) {
        self.rows = self.rows.saturating_add(rows);
        let held = self.spellings.entry(spelling.to_owned()).or_default();
        *held = held.saturating_add(rows);
    }

    fn the_spelling_most_rows_hold(&self) -> Option<&str> {
        let mut billed: Option<(u64, usize, &str)> = None;
        for (spelling, rows) in &self.spellings {
            let weighed = (*rows, store::marks_in(spelling), spelling.as_str());
            if billed.is_none_or(|held| weighed > held) {
                billed = Some(weighed);
            }
        }

        billed.map(|(_, _, spelling)| spelling)
    }
}

pub(crate) fn suggestions(inner: &Inner) -> Result<Vec<Suggestion>> {
    let mut candidates = decades(inner)?;
    candidates.extend(genres(inner)?);
    candidates.extend(artist_mixes(inner)?);
    candidates.extend(the_shape_of_the_catalog());

    let mut offered = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let asked = TrackQuery::from(&candidate.query);
        let measured = db::measured(inner, &asked)?;
        if measured.rows < ENOUGH_TO_OFFER {
            continue;
        }
        offered.push(Suggestion {
            pictured_by: db::pictured_by(inner, &asked, PICTURED_BY_AT_MOST)?,
            name: candidate.name,
            reason: candidate.reason,
            query: candidate.query,
            rows: u64::from(measured.rows),
            length: measured.length,
        });
    }

    Ok(offered)
}

fn decades(inner: &Inner) -> Result<Vec<Candidate>> {
    let held: Vec<i64> = inner.read(|connection| {
        let mut statement = connection
            .prepare(THE_DECADES_THE_CATALOG_HOLDS)
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;

        statement
            .query_map(
                params![
                    i64::from(YEARS_IN_A_DECADE),
                    i64::from(ENOUGH_FOR_A_DECADE),
                    MOST_DECADES as i64
                ],
                |row| row.get::<_, i64>(0),
            )
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))
    })?;

    Ok(held
        .into_iter()
        .filter_map(|decade| i32::try_from(decade).ok())
        .map(a_decade)
        .collect())
}

fn a_decade(decade: i32) -> Candidate {
    let text = asking(smallvec![
        Condition::Term(Term::Year {
            compare: Compare::AtLeast,
            year: decade,
        }),
        Condition::Term(Term::Year {
            compare: Compare::AtMost,
            year: decade + YEARS_IN_A_DECADE - 1,
        }),
    ]);

    Candidate {
        name: beginning_in_capitals(&spelled(decade)),
        reason: Reason::Decade(decade),
        query: written(
            text,
            SortOrder::AlbumThenTrack,
            SortOrder::AlbumThenTrack.reads(),
        ),
    }
}

fn genres(inner: &Inner) -> Result<Vec<Candidate>> {
    let billed = inner.read(|connection| {
        let mut billed: AHashMap<String, Spelt> = AHashMap::new();
        for sql in [THE_GENRES_THE_FILES_NAME, THE_GENRES_THE_ARTISTS_WERE_GIVEN] {
            for (name, rows) in named_and_counted(connection, sql, None)? {
                let name = name.trim();
                if name.is_empty() || !fits_in_a_phrase(name) || names_a_decade(name) {
                    continue;
                }
                billed
                    .entry(store::folded_letters(name))
                    .or_default()
                    .taking(name, u64::try_from(rows).unwrap_or_default());
            }
        }

        Ok(billed)
    })?;

    let mut held: Vec<(String, u64)> = billed
        .into_values()
        .filter_map(|spelt| Some((spelt.the_spelling_most_rows_hold()?.to_owned(), spelt.rows)))
        .collect();
    held.sort_by(|(name, rows), (other, held)| held.cmp(rows).then_with(|| name.cmp(other)));
    held.truncate(MOST_GENRES);

    Ok(held.iter().map(|(name, _)| a_genre(name)).collect())
}

fn names_a_decade(name: &str) -> bool {
    let figures = name.strip_suffix('s').unwrap_or(name);

    !figures.is_empty() && figures.chars().all(|letter| letter.is_ascii_digit())
}

fn billed_as(name: &str) -> String {
    if name.chars().any(char::is_uppercase) {
        return name.to_owned();
    }

    let mut billed = String::with_capacity(name.len());
    let mut leads = true;
    for letter in name.chars() {
        if leads {
            billed.extend(letter.to_uppercase());
        } else {
            billed.push(letter);
        }
        leads = !letter.is_alphanumeric();
    }

    billed
}

fn a_genre(name: &str) -> Candidate {
    let text = asking(smallvec![Condition::Word(one_word_or_a_phrase(
        Column::Genre,
        name,
    ))]);

    Candidate {
        name: billed_as(name),
        reason: Reason::Genre,
        query: written(text, SortOrder::Plays, SortOrder::Plays.reads()),
    }
}

fn artist_mixes(inner: &Inner) -> Result<Vec<Candidate>> {
    let held = inner.read(|connection| {
        named_and_counted(
            connection,
            THE_ARTISTS_MOST_PLAYED,
            Some(MOST_ARTIST_MIXES as i64),
        )
    })?;

    Ok(held
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| !name.trim().is_empty() && fits_in_a_phrase(name))
        .map(|name| an_artist(name.trim()))
        .collect())
}

fn an_artist(name: &str) -> Candidate {
    let text = asking(smallvec![Condition::Word(always_a_phrase(
        Column::Artist,
        name
    ))]);

    Candidate {
        name: name.to_owned(),
        reason: Reason::Artist,
        query: written(
            text,
            SortOrder::AlbumThenTrack,
            SortOrder::AlbumThenTrack.reads(),
        ),
    }
}

fn the_shape_of_the_catalog() -> Vec<Candidate> {
    vec![
        of_one_term(
            "Never heard",
            Reason::NeverHeard,
            Term::Plays {
                compare: Compare::Exactly,
                plays: NEVER_PLAYED,
                within: None,
            },
            SortOrder::DateAdded,
            SortOrder::DateAdded.reads(),
        ),
        of_one_term(
            "Most played",
            Reason::MostPlayed,
            Term::Plays {
                compare: Compare::Above,
                plays: PLAYED_MORE_THAN,
                within: None,
            },
            SortOrder::Plays,
            SortOrder::Plays.reads(),
        ),
        of_one_term(
            "Recently added",
            Reason::RecentlyAdded,
            Term::Added {
                compare: Compare::Below,
                age: RECENTLY_ADDED_WITHIN,
            },
            SortOrder::DateAdded,
            SortOrder::DateAdded.reads(),
        ),
        of_one_term(
            "Back catalogue",
            Reason::BackCatalogue,
            Term::Played {
                compare: Compare::Above,
                age: NOT_HEARD_FOR,
            },
            SortOrder::Played,
            SortOrder::Played.reads(),
        ),
        of_one_term(
            "Hi-res",
            Reason::HiRes,
            Term::Shape(Shape::HiRes),
            SortOrder::AlbumThenTrack,
            SortOrder::AlbumThenTrack.reads(),
        ),
        of_one_term(
            "Long players",
            Reason::LongPlayers,
            Term::Length {
                compare: Compare::Above,
                length: A_LONG_PLAYER,
            },
            SortOrder::Duration,
            Direction::Descending,
        ),
    ]
}

fn of_one_term(
    name: &str,
    reason: Reason,
    term: Term,
    sort: SortOrder,
    reading: Direction,
) -> Candidate {
    Candidate {
        name: name.to_owned(),
        reason,
        query: written(asking(smallvec![Condition::Term(term)]), sort, reading),
    }
}

fn written(text: Search, sort: SortOrder, reading: Direction) -> SavedQuery {
    SavedQuery {
        text: Some(text.to_string()),
        sort,
        reading,
        limit: None,
    }
}

fn asking(all: Conditions) -> Search {
    Search {
        clauses: vec![Clause {
            any: smallvec![Asked { denied: false, all }],
        }],
    }
}

fn one_word_or_a_phrase(column: Column, text: &str) -> Word {
    Word {
        column: Some(column),
        text: text.to_owned(),
        phrase: text.chars().any(char::is_whitespace),
    }
}

fn always_a_phrase(column: Column, text: &str) -> Word {
    Word {
        column: Some(column),
        text: text.to_owned(),
        phrase: true,
    }
}

fn fits_in_a_phrase(text: &str) -> bool {
    !text.contains('"')
}

fn named_and_counted(
    connection: &Connection,
    sql: &str,
    most: Option<i64>,
) -> Result<Vec<(String, i64)>> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let held = match most {
        Some(most) => statement.query_map(params![most], name_and_count),
        None => statement.query_map([], name_and_count),
    };

    held.and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn name_and_count(row: &Row<'_>) -> rusqlite::Result<(String, i64)> {
    Ok((row.get(0)?, row.get(1)?))
}

fn spelled(decade: i32) -> String {
    DECADES_WITH_A_NAME
        .into_iter()
        .find(|(from, _)| *from == decade)
        .map_or_else(|| format!("the {decade}s"), |(_, named)| named.to_owned())
}

fn beginning_in_capitals(text: &str) -> String {
    let mut letters = text.chars();

    match letters.next() {
        Some(first) => first.to_uppercase().chain(letters).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_decade_is_named_where_it_has_a_name_and_numbered_where_it_has_none() {
        assert_eq!(spelled(1990), "the nineties");
        assert_eq!(spelled(2000), "the 2000s");
        assert_eq!(beginning_in_capitals(&spelled(1970)), "The seventies");
        assert_eq!(beginning_in_capitals(""), "");
    }

    #[test]
    fn a_genre_that_only_names_a_decade_is_left_to_the_decades() {
        assert!(names_a_decade("2010s"));
        assert!(names_a_decade("90s"));
        assert!(names_a_decade("1990"));
        assert!(!names_a_decade("rock"));
        assert!(!names_a_decade("trip hop"));
        assert!(!names_a_decade("s"));
    }

    #[test]
    fn a_genre_written_in_lower_case_is_billed_with_its_words_capitalised() {
        assert_eq!(billed_as("alternative"), "Alternative");
        assert_eq!(billed_as("singer-songwriter"), "Singer-Songwriter");
        assert_eq!(billed_as("trip hop"), "Trip Hop");
        assert_eq!(billed_as("r&b"), "R&B");
        assert_eq!(billed_as("K-Pop"), "K-Pop");
    }

    #[test]
    fn a_genre_is_billed_under_the_spelling_most_rows_hold() {
        let mut spelt = Spelt::default();
        spelt.taking("rock", 3);
        spelt.taking("Rock", 9);
        spelt.taking("ROCK", 1);

        assert_eq!(spelt.the_spelling_most_rows_hold(), Some("Rock"));
        assert_eq!(spelt.rows, 13);
        assert_eq!(Spelt::default().the_spelling_most_rows_hold(), None);
    }

    #[test]
    fn a_marked_spelling_wins_a_tie_against_a_plain_one() {
        let mut spelt = Spelt::default();
        spelt.taking("brutal death metal", 4);
        spelt.taking("brütal death metal", 4);

        assert_eq!(
            spelt.the_spelling_most_rows_hold(),
            Some("brütal death metal")
        );
    }

    #[test]
    fn a_decade_is_written_as_the_range_the_grammar_reads_back() {
        let decade = a_decade(1990);
        let text = decade.query.text.expect("a decade asks for its years");

        assert_eq!(text, "year:1990-1999");
        assert_eq!(Search::read(&text).to_string(), text);
    }

    #[test]
    fn every_shape_the_catalog_is_offered_in_reads_back_as_what_it_was_written_as() {
        let written: Vec<String> = the_shape_of_the_catalog()
            .into_iter()
            .map(|candidate| {
                candidate
                    .query
                    .text
                    .expect("a shape asks the catalog something")
            })
            .collect();

        assert_eq!(
            written,
            vec![
                "plays:0",
                "plays:>5",
                "added:<1mo",
                "played:>1y",
                "is:hires",
                "length:>10m",
            ]
        );
        for text in &written {
            let read = Search::read(text);

            assert!(!read.is_empty(), "{text} reads as no search at all");
            assert_eq!(read.to_string(), *text);
        }
    }

    #[test]
    fn a_name_with_a_space_in_it_is_asked_for_as_a_phrase() {
        assert!(one_word_or_a_phrase(Column::Genre, "progressive rock").phrase);
        assert!(!one_word_or_a_phrase(Column::Genre, "rock").phrase);
        assert!(always_a_phrase(Column::Artist, "Nirvana").phrase);
        assert!(!fits_in_a_phrase("a \"name\""));
    }
}
