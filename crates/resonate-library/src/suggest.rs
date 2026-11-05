use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ahash::{AHashMap, AHashSet};
use resonate_codec::LISTED_APART_BY;
use resonate_core::{Accent, AlbumId};
use rusqlite::{Connection, Row, params};
use smallvec::smallvec;

use crate::{
    Asked, Clause, Column, Compare, Condition, Direction, Error, Grain, Reach, Result, SavedQuery,
    Search, Shape, SortOrder, StoreOp, Term, TrackQuery, Word,
    db::{self, Inner},
    search::Conditions,
    store,
};

const ENOUGH_FOR_A_DECADE: u32 = 20;
const ENOUGH_TO_OFFER: u32 = 10;
const ENOUGH_OF_ONE_ARTIST: u32 = 5;

pub const PICTURED_BY_AT_MOST: usize = 4;

const MOST_DECADES: usize = 5;
const GENRES_ALWAYS_OFFERED: usize = 4;
const GENRES_OFFERED_IN_TURN: usize = 2;
const ARTISTS_ALWAYS_OFFERED: usize = 3;
const ARTISTS_OFFERED_IN_TURN: usize = 2;
const GENRES_OF_A_DECADE_IN_TURN: usize = 3;
const KINDRED_MIXES_IN_TURN: usize = 2;
const DEEP_CUTS_IN_TURN: usize = 2;
const ARTISTS_TO_DISCOVER_IN_TURN: usize = 2;
const ARTISTS_TO_GO_BACK_TO_IN_TURN: usize = 2;
const UNHEARD_GENRES: usize = 2;
const KINDRED_ARTISTS_AT_MOST: usize = 6;

const ARTISTS_WEIGHED_AT_MOST: usize = 24;
pub const ARTISTS_OF_LATE_AT_MOST: i64 = 6;
const DEEP_CUTS_BEGIN_DAYS_AFTER_THE_KINDRED: i64 = 3;

const YEARS_IN_A_DECADE: i32 = 10;

const SECONDS_PER_DAY: u64 = 86_400;
const RECENTLY_ADDED_WITHIN: Duration = Duration::from_secs(30 * SECONDS_PER_DAY);
const NOT_HEARD_FOR: Duration = Duration::from_secs(365 * SECONDS_PER_DAY);
const A_LONG_PLAYER: Duration = Duration::from_secs(10 * 60);
const A_SLOW_BURNER: Duration = Duration::from_secs(6 * 60);
const SHORT_AND_SHARP: Duration = Duration::from_secs(2 * 60);
const A_MONTH: Duration = Duration::from_secs(30 * SECONDS_PER_DAY);
const HALF_A_YEAR: Duration = Duration::from_secs(180 * SECONDS_PER_DAY);
pub const OF_LATE: Duration = Duration::from_secs(90 * SECONDS_PER_DAY);

const NEVER_PLAYED: u32 = 0;
const PLAYED_ONCE: u32 = 1;
const PLAYED_MORE_THAN: u32 = 5;
const ON_REPEAT_AT_LEAST: u32 = 3;
const AN_OLD_FLAME_AT_LEAST: u32 = 5;
const LEFT_BEHIND_AT_LEAST: i64 = 10;

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
      WHERE a.year IS NOT NULL AND a.year >= ?1
      GROUP BY decade";

const THE_GENRES_THE_FILES_NAME: &str = "SELECT genre, count(*) FROM tracks
      WHERE genre IS NOT NULL AND trim(genre) <> ''
      GROUP BY genre";

const THE_GENRES_THE_ARTISTS_WERE_GIVEN: &str = "SELECT g.name, count(*)
       FROM artist_genres g JOIN tracks t ON t.artist_id = g.artist_id
      WHERE trim(g.name) <> ''
      GROUP BY g.name";

const THE_ARTISTS_AND_WHAT_WAS_PLAYED_OF_THEM: &str =
    "SELECT r.name, sum(t.plays) AS plays, count(*) AS held, max(t.played)
       FROM tracks t JOIN artists r ON r.id = t.artist_id
      WHERE trim(r.name) <> ''
      GROUP BY r.id
      ORDER BY plays DESC, held DESC, r.name COLLATE NOCASE";

const THE_ARTISTS_HEARD_LATELY: &str = "SELECT r.name, sum(l.heard) AS heard
       FROM listens l
       JOIN tracks t ON t.id = l.track_id
       JOIN artists r ON r.id = t.artist_id
      WHERE l.at >= ?1 AND trim(r.name) <> ''
      GROUP BY r.id
      ORDER BY heard DESC, r.name COLLATE NOCASE
      LIMIT ?2";

const THE_GENRES_OF_THE_ARTISTS: &str = "SELECT DISTINCT r.name, t.genre
       FROM tracks t JOIN artists r ON r.id = t.artist_id
      WHERE t.genre IS NOT NULL AND trim(t.genre) <> '' AND trim(r.name) <> ''
     UNION
     SELECT r.name, g.name
       FROM artist_genres g JOIN artists r ON r.id = g.artist_id
      WHERE trim(g.name) <> '' AND trim(r.name) <> ''
        AND EXISTS (SELECT 1 FROM tracks t WHERE t.artist_id = r.id)";

const THE_GENRES_PLAYED: &str = "SELECT genre, sum(plays) FROM tracks
      WHERE genre IS NOT NULL AND trim(genre) <> '' AND plays > 0
      GROUP BY genre";

const THE_GENRES_OF_THE_ARTISTS_PLAYED: &str = "SELECT g.name, sum(t.plays)
       FROM artist_genres g JOIN tracks t ON t.artist_id = g.artist_id
      WHERE trim(g.name) <> '' AND t.plays > 0
      GROUP BY g.name";

const THE_GENRES_OF_EACH_DECADE: &str = "SELECT (a.year / ?1) * ?1 AS decade, t.genre, count(*)
       FROM tracks t JOIN albums a ON a.id = t.album_id
      WHERE a.year IS NOT NULL AND a.year >= ?1
        AND t.genre IS NOT NULL AND trim(t.genre) <> ''
      GROUP BY decade, t.genre";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Reason {
    Decade(i32),
    GenreOfADecade(i32),
    Genre,
    Artist,
    NeverHeard,
    MostPlayed,
    RecentlyAdded,
    FreshArrivals,
    BackCatalogue,
    OnRepeat,
    KindredArtists,
    DeepCuts,
    ArtistToDiscover,
    UnheardGenre,
    ForgottenFavourites,
    OldFlames,
    SecondChances,
    StarredUnheard,
    ArtistLeftBehind,
    HiRes,
    LongPlayers,
    SlowBurners,
    ShortAndSharp,
    Surround,
    Mono,
}

impl Reason {
    pub fn says(&self) -> String {
        match self {
            Self::Decade(decade) => format!("Everything from {}", spelled(*decade)),
            Self::GenreOfADecade(decade) => format!("One genre, from {}", spelled(*decade)),
            Self::Genre => "Everything filed under it".to_owned(),
            Self::Artist => "Everything they made".to_owned(),
            Self::NeverHeard => "You have never heard these".to_owned(),
            Self::MostPlayed => "The ones you come back to".to_owned(),
            Self::RecentlyAdded => "The newest in the library".to_owned(),
            Self::FreshArrivals => "New this month and not played yet".to_owned(),
            Self::BackCatalogue => "Not heard for over a year".to_owned(),
            Self::OnRepeat => "What you keep going back to this month".to_owned(),
            Self::KindredArtists => "Artists sharing its genres, that you hear less".to_owned(),
            Self::DeepCuts => "What you pass over by an artist you play".to_owned(),
            Self::ArtistToDiscover => "An artist you hold and have never played".to_owned(),
            Self::UnheardGenre => "A genre you play, in songs you have not".to_owned(),
            Self::ForgottenFavourites => "Starred, then left alone for half a year".to_owned(),
            Self::OldFlames => "Played often once, quiet for half a year".to_owned(),
            Self::SecondChances => "Played once, over a month ago".to_owned(),
            Self::StarredUnheard => "You marked these before hearing them".to_owned(),
            Self::ArtistLeftBehind => "You played them often, then stopped".to_owned(),
            Self::HiRes => "Better than a CD".to_owned(),
            Self::LongPlayers => "Ten minutes and over".to_owned(),
            Self::SlowBurners => "Six to ten minutes".to_owned(),
            Self::ShortAndSharp => "Two minutes or less".to_owned(),
            Self::Surround => "More than two channels".to_owned(),
            Self::Mono => "One channel".to_owned(),
        }
    }
}

impl Reason {
    pub fn accents(&self, name: &str) -> (Accent, Accent) {
        match self {
            Self::Decade(_) | Self::GenreOfADecade(_) => (Accent::Amber, Accent::Peach),
            Self::NeverHeard | Self::UnheardGenre => (Accent::Teal, Accent::Blue),
            Self::MostPlayed | Self::OnRepeat => (Accent::Red, Accent::Peach),
            Self::RecentlyAdded | Self::FreshArrivals => (Accent::Green, Accent::Teal),
            Self::BackCatalogue | Self::OldFlames | Self::SecondChances => {
                (Accent::Amber, Accent::Mauve)
            }
            Self::ForgottenFavourites | Self::StarredUnheard => (Accent::Mauve, Accent::Red),
            Self::HiRes | Self::Surround | Self::Mono => (Accent::Green, Accent::Blue),
            Self::LongPlayers | Self::SlowBurners | Self::ShortAndSharp => {
                (Accent::Blue, Accent::Mauve)
            }
            Self::Genre
            | Self::Artist
            | Self::KindredArtists
            | Self::DeepCuts
            | Self::ArtistToDiscover
            | Self::ArtistLeftBehind => Accent::named_pair(name),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    ForYou,
    Rediscover,
    Listening,
    Eras,
    Genres,
    Artists,
    Sound,
}

impl Kind {
    pub const ALL: [Self; 7] = [
        Self::ForYou,
        Self::Rediscover,
        Self::Listening,
        Self::Eras,
        Self::Genres,
        Self::Artists,
        Self::Sound,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::ForYou => "Made for you",
            Self::Rediscover => "Rediscover",
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
            Self::OnRepeat
            | Self::KindredArtists
            | Self::DeepCuts
            | Self::ArtistToDiscover
            | Self::UnheardGenre => Kind::ForYou,
            Self::BackCatalogue
            | Self::ForgottenFavourites
            | Self::OldFlames
            | Self::SecondChances
            | Self::StarredUnheard
            | Self::ArtistLeftBehind => Kind::Rediscover,
            Self::NeverHeard | Self::MostPlayed | Self::RecentlyAdded | Self::FreshArrivals => {
                Kind::Listening
            }
            Self::Decade(_) | Self::GenreOfADecade(_) => Kind::Eras,
            Self::Genre => Kind::Genres,
            Self::Artist => Kind::Artists,
            Self::HiRes
            | Self::LongPlayers
            | Self::SlowBurners
            | Self::ShortAndSharp
            | Self::Surround
            | Self::Mono => Kind::Sound,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Standing {
    AlwaysOffered,
    GivesWayToTheSameRows,
}

#[derive(Clone, Debug)]
pub struct Candidate {
    pub name: String,
    pub reason: Reason,
    pub query: SavedQuery,
    enough: u32,
    standing: Standing,
}

impl Candidate {
    fn new(name: impl Into<String>, reason: Reason, query: SavedQuery) -> Self {
        Self {
            name: name.into(),
            reason,
            query,
            enough: ENOUGH_TO_OFFER,
            standing: Standing::AlwaysOffered,
        }
    }

    const fn only_if_it_adds_something(mut self) -> Self {
        self.standing = Standing::GivesWayToTheSameRows;
        self
    }

    const fn of_one_artist(mut self) -> Self {
        self.enough = ENOUGH_OF_ONE_ARTIST;
        self
    }
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

#[derive(Debug, Default)]
pub struct Kinship {
    artists: ArtistsFiled,
    holders: AHashMap<String, u32>,
}

impl Kinship {
    pub fn of(shelved: &[(String, String)]) -> Self {
        let (artists, holders) = artists_and_who_shares_their_genres(shelved);

        Self { artists, holders }
    }
}

#[derive(Debug, Default)]
pub struct Filing {
    decades: Vec<i32>,
    genres: Vec<String>,
    genres_of_decades: Vec<(i32, String)>,
    kin: Kinship,
}

impl Filing {
    pub fn of(
        years: impl IntoIterator<Item = (i32, u64)>,
        genres: impl IntoIterator<Item = (String, u64)>,
        genres_of_years: impl IntoIterator<Item = (i32, String, u64)>,
        kin: Kinship,
    ) -> Self {
        Self {
            decades: decades_ranked_by(years),
            genres: genres_ranked_by(genres),
            genres_of_decades: genres_of_decades_ranked_by(genres_of_years),
            kin,
        }
    }

    pub(crate) fn read(inner: &Inner) -> Result<Self> {
        let kin = Kinship::of(&inner.read(named_and_filed)?);

        Ok(Self {
            decades: decades_ranked_by(decades_held(inner)?),
            genres: genres_ranked(
                inner,
                [THE_GENRES_THE_FILES_NAME, THE_GENRES_THE_ARTISTS_WERE_GIVEN],
            )?,
            genres_of_decades: genres_of_decades_ranked_by(genres_of_decades_held(inner)?),
            kin,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtistHeard {
    pub name: String,
    pub plays: i64,
    pub held: i64,
    pub last_played: Option<SystemTime>,
}

pub fn artists_as_heard(mut heard: Vec<ArtistHeard>) -> Vec<ArtistHeard> {
    heard.retain(|artist| {
        let name = artist.name.trim();

        fits_in_a_phrase(name)
    });
    heard.sort_by(|one, other| {
        other
            .plays
            .cmp(&one.plays)
            .then(other.held.cmp(&one.held))
            .then_with(|| one.name.to_lowercase().cmp(&other.name.to_lowercase()))
    });

    heard
}

pub fn artists_heard_lately(heard: impl IntoIterator<Item = (String, u64)>) -> Vec<String> {
    let mut merged: AHashMap<String, (String, u64)> = AHashMap::new();
    for (name, time) in heard {
        let held = merged
            .entry(store::folded_letters(name.trim()))
            .or_insert_with(|| (name.trim().to_owned(), 0));
        held.1 = held.1.saturating_add(time);
    }

    let mut ranked: Vec<(String, u64)> = merged.into_values().collect();
    ranked.sort_by(|one, other| {
        other
            .1
            .cmp(&one.1)
            .then_with(|| one.0.to_lowercase().cmp(&other.0.to_lowercase()))
    });

    names_of(
        ranked
            .into_iter()
            .take(ARTISTS_OF_LATE_AT_MOST as usize)
            .map(|(name, time)| (name, i64::try_from(time).unwrap_or(i64::MAX)))
            .collect(),
    )
}

fn the_artists_as_heard(inner: &Inner) -> Result<Vec<ArtistHeard>> {
    inner
        .read(|connection| {
            let mut statement = connection
                .prepare(THE_ARTISTS_AND_WHAT_WAS_PLAYED_OF_THEM)
                .map_err(|source| Error::store(StoreOp::Prepare, source))?;

            statement
                .query_map([], |row| {
                    Ok(ArtistHeard {
                        name: row.get::<_, String>(0)?.trim().to_owned(),
                        plays: row.get(1)?,
                        held: row.get(2)?,
                        last_played: row.get::<_, Option<i64>>(3)?.map(store::from_nanos),
                    })
                })
                .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
                .map_err(|source| Error::store(StoreOp::Query, source))
        })
        .map(|mut heard| {
            heard.retain(|artist| fits_in_a_phrase(&artist.name));
            heard
        })
}

pub(crate) fn suggestions(
    inner: &Inner,
    filing: &Filing,
    today: i64,
    now: SystemTime,
) -> Result<Vec<Suggestion>> {
    let heard = the_artists_as_heard(inner)?;
    let (lately, played_most) = what_you_play(inner, now)?;
    let candidates = lists_to_offer(filing, &heard, &lately, &played_most, today, now);

    worth_offering(candidates, |query| -> Result<Weighed> {
        let measured = db::measured(inner, &TrackQuery::from(query))?;

        Ok(Weighed {
            rows: measured.rows,
            length: measured.length,
        })
    })?
    .into_iter()
    .map(|(candidate, weighed)| -> Result<Suggestion> {
        Ok(Suggestion {
            pictured_by: db::pictured_by(
                inner,
                &TrackQuery::from(&candidate.query),
                PICTURED_BY_AT_MOST,
            )?,
            name: candidate.name,
            reason: candidate.reason,
            query: candidate.query,
            rows: u64::from(weighed.rows),
            length: weighed.length,
        })
    })
    .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Weighed {
    pub rows: u32,
    pub length: Option<Duration>,
}

pub fn worth_offering<E>(
    candidates: Vec<Candidate>,
    mut weigh: impl FnMut(&SavedQuery) -> std::result::Result<Weighed, E>,
) -> std::result::Result<Vec<(Candidate, Weighed)>, E> {
    let mut offered = Vec::with_capacity(candidates.len());
    let mut filled: AHashSet<(u32, Option<Duration>)> = AHashSet::new();
    for candidate in candidates {
        let weighed = weigh(&candidate.query)?;
        if weighed.rows < candidate.enough {
            continue;
        }
        let what_it_holds = (weighed.rows, weighed.length);
        if candidate.standing == Standing::GivesWayToTheSameRows && filled.contains(&what_it_holds)
        {
            continue;
        }
        filled.insert(what_it_holds);
        offered.push((candidate, weighed));
    }

    Ok(offered)
}

fn taken_in_turn<T: Clone>(held: &[T], today: i64, most: usize) -> Vec<T> {
    if held.is_empty() {
        return Vec::new();
    }
    let from = usize::try_from(today.rem_euclid(held.len() as i64)).unwrap_or_default();

    held.iter()
        .cycle()
        .skip(from)
        .take(most.min(held.len()))
        .cloned()
        .collect()
}

fn decades_held(inner: &Inner) -> Result<Vec<(i32, u64)>> {
    let held: Vec<(i64, i64)> = inner.read(|connection| {
        let mut statement = connection
            .prepare(THE_DECADES_THE_CATALOG_HOLDS)
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;

        statement
            .query_map(params![i64::from(YEARS_IN_A_DECADE)], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))
    })?;

    Ok(held
        .into_iter()
        .filter_map(|(decade, held)| Some((i32::try_from(decade).ok()?, u64::try_from(held).ok()?)))
        .collect())
}

fn decades_ranked_by(counted: impl IntoIterator<Item = (i32, u64)>) -> Vec<i32> {
    let mut held: AHashMap<i32, u64> = AHashMap::new();
    for (year, rows) in counted {
        if year >= YEARS_IN_A_DECADE {
            let decade = (year / YEARS_IN_A_DECADE) * YEARS_IN_A_DECADE;
            let total = held.entry(decade).or_default();
            *total = total.saturating_add(rows);
        }
    }

    let mut ranked: Vec<(i32, u64)> = held
        .into_iter()
        .filter(|(_, rows)| *rows >= u64::from(ENOUGH_FOR_A_DECADE))
        .collect();
    ranked.sort_by(|(decade, rows), (other, held)| held.cmp(rows).then(other.cmp(decade)));

    ranked
        .into_iter()
        .take(MOST_DECADES)
        .map(|(decade, _)| decade)
        .collect()
}

fn a_decade(decade: i32) -> Candidate {
    let text = asking(years_of(decade));

    Candidate::new(
        beginning_in_capitals(&spelled(decade)),
        Reason::Decade(decade),
        written(
            text,
            SortOrder::AlbumThenTrack,
            SortOrder::AlbumThenTrack.reads(),
        ),
    )
}

fn years_of(decade: i32) -> Conditions {
    smallvec![
        Condition::Term(Term::Year {
            compare: Compare::AtLeast,
            year: decade,
        }),
        Condition::Term(Term::Year {
            compare: Compare::AtMost,
            year: decade + YEARS_IN_A_DECADE - 1,
        }),
    ]
}

fn genres_of_decades_held(inner: &Inner) -> Result<Vec<(i32, String, u64)>> {
    let counted = inner.read(|connection| {
        let mut statement = connection
            .prepare(THE_GENRES_OF_EACH_DECADE)
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;

        statement
            .query_map(params![i64::from(YEARS_IN_A_DECADE)], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))
    })?;

    Ok(counted
        .into_iter()
        .filter_map(|(decade, listed, rows)| {
            Some((
                i32::try_from(decade).ok()?,
                listed,
                u64::try_from(rows).unwrap_or_default(),
            ))
        })
        .collect())
}

fn genres_of_decades_ranked_by(
    counted: impl IntoIterator<Item = (i32, String, u64)>,
) -> Vec<(i32, String)> {
    let mut billed: AHashMap<(i32, String), Spelt> = AHashMap::new();
    for (year, listed, rows) in counted {
        if year < YEARS_IN_A_DECADE {
            continue;
        }
        let decade = (year / YEARS_IN_A_DECADE) * YEARS_IN_A_DECADE;
        for name in offerable_genres(&listed) {
            billed
                .entry((decade, store::folded_letters(name)))
                .or_default()
                .taking(name, rows);
        }
    }

    let mut held: Vec<(u64, i32, String)> = billed
        .into_iter()
        .filter(|(_, spelt)| spelt.rows >= u64::from(ENOUGH_FOR_A_DECADE))
        .filter_map(|((decade, _), spelt)| {
            Some((
                spelt.rows,
                decade,
                spelt.the_spelling_most_rows_hold()?.to_owned(),
            ))
        })
        .collect();
    held.sort_by(|one, other| {
        other
            .0
            .cmp(&one.0)
            .then(other.1.cmp(&one.1))
            .then_with(|| one.2.cmp(&other.2))
    });

    held.into_iter()
        .map(|(_, decade, name)| (decade, name))
        .collect()
}

fn genres_of_a_decade(filing: &Filing, today: i64) -> Vec<Candidate> {
    taken_in_turn(&filing.genres_of_decades, today, GENRES_OF_A_DECADE_IN_TURN)
        .iter()
        .map(|(decade, name)| a_genre_of_a_decade(*decade, name))
        .collect()
}

fn a_genre_of_a_decade(decade: i32, name: &str) -> Candidate {
    let text = asking_each([
        years_of(decade),
        smallvec![Condition::Word(the_whole_name(Column::Genre, name))],
    ]);

    Candidate::new(
        format!("{} of {}", billed_as(name), spelled(decade)),
        Reason::GenreOfADecade(decade),
        written(text, SortOrder::Plays, SortOrder::Plays.reads()),
    )
    .only_if_it_adds_something()
}

fn genres(filing: &Filing, today: i64) -> Vec<Candidate> {
    let (always, rest) = filing
        .genres
        .split_at(GENRES_ALWAYS_OFFERED.min(filing.genres.len()));

    always
        .iter()
        .chain(taken_in_turn(rest, today, GENRES_OFFERED_IN_TURN).iter())
        .map(|name| a_genre(name))
        .collect()
}

fn genres_ranked(inner: &Inner, asked: [&str; 2]) -> Result<Vec<String>> {
    let counted = inner.read(|connection| {
        let mut counted = Vec::new();
        for sql in asked {
            counted.extend(named_and_counted(connection, sql, None)?);
        }

        Ok(counted)
    })?;

    Ok(genres_ranked_by(counted.into_iter().map(
        |(listed, rows)| (listed, u64::try_from(rows).unwrap_or_default()),
    )))
}

pub fn genres_ranked_by(counted: impl IntoIterator<Item = (String, u64)>) -> Vec<String> {
    let mut billed: AHashMap<String, Spelt> = AHashMap::new();
    for (listed, rows) in counted {
        for name in offerable_genres(&listed) {
            billed
                .entry(store::folded_letters(name))
                .or_default()
                .taking(name, rows);
        }
    }

    let mut held: Vec<(String, u64)> = billed
        .into_values()
        .filter_map(|spelt| Some((spelt.the_spelling_most_rows_hold()?.to_owned(), spelt.rows)))
        .collect();
    held.sort_by(|(name, rows), (other, held)| held.cmp(rows).then_with(|| name.cmp(other)));

    held.into_iter().map(|(name, _)| name).collect()
}

fn offerable_genres(listed: &str) -> impl Iterator<Item = &str> {
    listed
        .split(LISTED_APART_BY)
        .map(str::trim)
        .filter(|name| !name.is_empty() && fits_in_a_phrase(name) && !names_a_decade(name))
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
    let text = asking(smallvec![Condition::Word(the_whole_name(
        Column::Genre,
        name,
    ))]);

    Candidate::new(
        billed_as(name),
        Reason::Genre,
        written(text, SortOrder::Plays, SortOrder::Plays.reads()),
    )
}

fn artist_mixes(heard: &[ArtistHeard], today: i64) -> Vec<Candidate> {
    let held: Vec<&str> = heard
        .iter()
        .take(ARTISTS_WEIGHED_AT_MOST)
        .map(|artist| artist.name.as_str())
        .collect();
    let (always, rest) = held.split_at(ARTISTS_ALWAYS_OFFERED.min(held.len()));

    always
        .iter()
        .chain(taken_in_turn(rest, today, ARTISTS_OFFERED_IN_TURN).iter())
        .map(|name| an_artist(name))
        .collect()
}

fn names_of(counted: Vec<(String, i64)>) -> Vec<String> {
    counted
        .into_iter()
        .map(|(name, _)| name.trim().to_owned())
        .filter(|name| !name.is_empty() && fits_in_a_phrase(name))
        .collect()
}

fn an_artist(name: &str) -> Candidate {
    let text = asking(smallvec![Condition::Word(the_whole_name(
        Column::Artist,
        name
    ))]);

    Candidate::new(
        name,
        Reason::Artist,
        written(
            text,
            SortOrder::AlbumThenTrack,
            SortOrder::AlbumThenTrack.reads(),
        ),
    )
}

fn never_played_of(heard: &[ArtistHeard]) -> Vec<&str> {
    heard
        .iter()
        .filter(|artist| artist.plays == 0 && artist.held >= i64::from(ENOUGH_OF_ONE_ARTIST))
        .take(ARTISTS_WEIGHED_AT_MOST)
        .map(|artist| artist.name.as_str())
        .collect()
}

fn left_behind_of(heard: &[ArtistHeard], before: SystemTime) -> Vec<&str> {
    heard
        .iter()
        .filter(|artist| {
            artist.plays >= LEFT_BEHIND_AT_LEAST
                && artist.last_played.is_some_and(|played| played < before)
        })
        .take(ARTISTS_WEIGHED_AT_MOST)
        .map(|artist| artist.name.as_str())
        .collect()
}

fn what_you_play(inner: &Inner, now: SystemTime) -> Result<(Vec<String>, Vec<String>)> {
    let of_late = store::to_nanos(now.checked_sub(OF_LATE).unwrap_or(UNIX_EPOCH));
    let lately = inner.read(|connection| {
        names_read(
            connection,
            THE_ARTISTS_HEARD_LATELY,
            params![of_late, ARTISTS_OF_LATE_AT_MOST],
        )
    })?;
    let played_most = genres_ranked(inner, [THE_GENRES_PLAYED, THE_GENRES_OF_THE_ARTISTS_PLAYED])?;

    Ok((lately, played_most))
}

pub fn lists_to_offer(
    filing: &Filing,
    heard: &[ArtistHeard],
    lately: &[String],
    played_most: &[String],
    today: i64,
    now: SystemTime,
) -> Vec<Candidate> {
    let mut candidates: Vec<Candidate> = filing.decades.iter().copied().map(a_decade).collect();
    candidates.extend(genres(filing, today));
    candidates.extend(artist_mixes(heard, today));
    candidates.extend(the_shape_of_the_catalog());
    candidates.extend(genres_of_a_decade(filing, today));
    candidates.extend(listening_lists(
        &filing.kin,
        heard,
        lately,
        played_most,
        today,
        now,
    ));
    candidates.extend(the_sounds_the_catalog_holds());

    candidates
}

fn listening_lists(
    kin: &Kinship,
    heard: &[ArtistHeard],
    lately: &[String],
    played_most: &[String],
    today: i64,
    now: SystemTime,
) -> Vec<Candidate> {
    let half_a_year_ago = now.checked_sub(HALF_A_YEAR).unwrap_or(UNIX_EPOCH);
    let never_played = never_played_of(heard);
    let left_behind = left_behind_of(heard, half_a_year_ago);

    let mut candidates = vec![
        a_list_of(
            "On repeat",
            Reason::OnRepeat,
            smallvec![Condition::Term(Term::Plays {
                compare: Compare::AtLeast,
                plays: ON_REPEAT_AT_LEAST,
                within: Some(A_MONTH),
            })],
            SortOrder::PlaysThisMonth,
        ),
        a_list_of(
            "Forgotten favourites",
            Reason::ForgottenFavourites,
            smallvec![
                Condition::Term(Term::Shape(Shape::Favourite)),
                Condition::Term(not_heard_for(HALF_A_YEAR)),
            ],
            SortOrder::Played,
        ),
        a_list_of(
            "Old flames",
            Reason::OldFlames,
            smallvec![
                Condition::Term(Term::Plays {
                    compare: Compare::AtLeast,
                    plays: AN_OLD_FLAME_AT_LEAST,
                    within: None,
                }),
                Condition::Term(not_heard_for(HALF_A_YEAR)),
            ],
            SortOrder::Plays,
        ),
        a_list_of(
            "Second chances",
            Reason::SecondChances,
            smallvec![
                Condition::Term(Term::Plays {
                    compare: Compare::Exactly,
                    plays: PLAYED_ONCE,
                    within: None,
                }),
                Condition::Term(not_heard_for(A_MONTH)),
            ],
            SortOrder::Played,
        ),
        a_list_of(
            "Starred, never played",
            Reason::StarredUnheard,
            smallvec![
                Condition::Term(Term::Shape(Shape::Favourite)),
                Condition::Term(never_played_yet()),
            ],
            SortOrder::Favourited,
        ),
        a_list_of(
            "Fresh arrivals",
            Reason::FreshArrivals,
            smallvec![
                Condition::Term(Term::Added {
                    compare: Compare::Below,
                    age: RECENTLY_ADDED_WITHIN,
                }),
                Condition::Term(never_played_yet()),
            ],
            SortOrder::DateAdded,
        ),
    ];

    let known: AHashSet<String> = lately
        .iter()
        .map(|name| store::folded_letters(name))
        .collect();
    for seed in taken_in_turn(lately, today, KINDRED_MIXES_IN_TURN) {
        let kindred = kindred_to(kin, &seed, &known);
        if !kindred.is_empty() {
            candidates.push(more_like(&seed, &kindred));
        }
    }
    for artist in taken_in_turn(
        lately,
        today + DEEP_CUTS_BEGIN_DAYS_AFTER_THE_KINDRED,
        DEEP_CUTS_IN_TURN,
    ) {
        candidates.push(deep_cuts_of(&artist));
    }
    for artist in taken_in_turn(&never_played, today, ARTISTS_TO_DISCOVER_IN_TURN) {
        candidates.push(an_artist_to_discover(artist));
    }
    for artist in taken_in_turn(&left_behind, today, ARTISTS_TO_GO_BACK_TO_IN_TURN) {
        candidates.push(an_artist_left_behind(artist));
    }
    for genre in played_most.iter().take(UNHEARD_GENRES) {
        candidates.push(a_genre_unheard(genre));
    }

    candidates
}

fn names_read(
    connection: &Connection,
    sql: &str,
    asked: impl rusqlite::Params,
) -> Result<Vec<String>> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let counted = statement
        .query_map(asked, name_and_count)
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    Ok(names_of(counted))
}

fn named_and_filed(connection: &Connection) -> Result<Vec<(String, String)>> {
    let mut statement = connection
        .prepare(THE_GENRES_OF_THE_ARTISTS)
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;

    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))
}

type ArtistsFiled = AHashMap<String, (String, AHashSet<String>)>;

fn artists_and_who_shares_their_genres(
    shelved: &[(String, String)],
) -> (ArtistsFiled, AHashMap<String, u32>) {
    let mut filed: ArtistsFiled = AHashMap::new();
    let mut holders: AHashMap<String, u32> = AHashMap::new();
    for (artist, listed) in shelved {
        let name = artist.trim();
        if !fits_in_a_phrase(name) {
            continue;
        }
        let held = filed
            .entry(store::folded_letters(name))
            .or_insert_with(|| (name.to_owned(), AHashSet::new()));
        for genre in offerable_genres(listed).map(store::folded_letters) {
            if held.1.insert(genre.clone()) {
                *holders.entry(genre).or_default() += 1;
            }
        }
    }

    (filed, holders)
}

fn kindred_to(kin: &Kinship, seed: &str, known: &AHashSet<String>) -> Vec<String> {
    let Kinship {
        artists: filed,
        holders,
    } = kin;
    let seed = store::folded_letters(seed);
    let Some((_, sought)) = filed.get(&seed) else {
        return Vec::new();
    };
    let weighed = |genre: &String| {
        let held_by = holders.get(genre).copied().unwrap_or(1);

        1.0 / f64::from(held_by + 1).ln_1p()
    };
    let mut scored: Vec<(f64, &str)> = filed
        .iter()
        .filter(|(folded, _)| **folded != seed && !known.contains(*folded))
        .filter_map(|(_, (name, genres))| {
            let shared: f64 = genres.intersection(sought).map(weighed).sum();

            (shared > 0.0).then_some((shared, name.as_str()))
        })
        .collect();
    scored.sort_by(|one, other| other.0.total_cmp(&one.0).then_with(|| one.1.cmp(other.1)));

    scored
        .into_iter()
        .take(KINDRED_ARTISTS_AT_MOST)
        .map(|(_, name)| name.to_owned())
        .collect()
}

fn more_like(seed: &str, kindred: &[String]) -> Candidate {
    let text = Search {
        clauses: vec![Clause {
            any: kindred
                .iter()
                .map(|artist| Asked {
                    denied: false,
                    all: smallvec![Condition::Word(the_whole_name(Column::Artist, artist))],
                })
                .collect(),
        }],
    };

    Candidate::new(
        format!("More like {seed}"),
        Reason::KindredArtists,
        written(text, SortOrder::Plays, Direction::Ascending),
    )
    .of_one_artist()
    .only_if_it_adds_something()
}

fn deep_cuts_of(artist: &str) -> Candidate {
    a_list_of(
        &format!("{artist}, deep cuts"),
        Reason::DeepCuts,
        smallvec![
            Condition::Word(the_whole_name(Column::Artist, artist)),
            Condition::Term(Term::Plays {
                compare: Compare::AtMost,
                plays: PLAYED_ONCE,
                within: None,
            }),
        ],
        SortOrder::AlbumThenTrack,
    )
    .of_one_artist()
}

fn an_artist_to_discover(artist: &str) -> Candidate {
    a_list_of(
        &format!("Discover {artist}"),
        Reason::ArtistToDiscover,
        smallvec![Condition::Word(the_whole_name(Column::Artist, artist))],
        SortOrder::AlbumThenTrack,
    )
    .of_one_artist()
}

fn an_artist_left_behind(artist: &str) -> Candidate {
    a_list_of(
        &format!("Back to {artist}"),
        Reason::ArtistLeftBehind,
        smallvec![Condition::Word(the_whole_name(Column::Artist, artist))],
        SortOrder::Plays,
    )
    .of_one_artist()
}

fn a_genre_unheard(genre: &str) -> Candidate {
    a_list_of(
        &format!("Unheard {}", billed_as(genre)),
        Reason::UnheardGenre,
        smallvec![
            Condition::Word(the_whole_name(Column::Genre, genre)),
            Condition::Term(never_played_yet()),
        ],
        SortOrder::DateAdded,
    )
}

fn a_list_of(name: &str, reason: Reason, all: Conditions, sort: SortOrder) -> Candidate {
    Candidate::new(name, reason, written(asking(all), sort, sort.reads()))
        .only_if_it_adds_something()
}

const fn not_heard_for(age: Duration) -> Term {
    Term::Played {
        compare: Compare::Above,
        age,
    }
}

const fn never_played_yet() -> Term {
    Term::Plays {
        compare: Compare::Exactly,
        plays: NEVER_PLAYED,
        within: None,
    }
}

fn the_sounds_the_catalog_holds() -> Vec<Candidate> {
    vec![
        of_one_term(
            "Short and sharp",
            Reason::ShortAndSharp,
            Term::Length {
                compare: Compare::AtMost,
                length: SHORT_AND_SHARP,
                grain: Grain::MINUTE,
            },
            SortOrder::Plays,
            SortOrder::Plays.reads(),
        )
        .only_if_it_adds_something(),
        of_one_term(
            "Surround",
            Reason::Surround,
            Term::Shape(Shape::Multichannel),
            SortOrder::AlbumThenTrack,
            SortOrder::AlbumThenTrack.reads(),
        )
        .only_if_it_adds_something(),
        of_one_term(
            "In mono",
            Reason::Mono,
            Term::Shape(Shape::Mono),
            SortOrder::AlbumThenTrack,
            SortOrder::AlbumThenTrack.reads(),
        )
        .only_if_it_adds_something(),
        a_list_of(
            "Slow burners",
            Reason::SlowBurners,
            smallvec![
                Condition::Term(Term::Length {
                    compare: Compare::AtLeast,
                    length: A_SLOW_BURNER,
                    grain: Grain::MINUTE,
                }),
                Condition::Term(Term::Length {
                    compare: Compare::AtMost,
                    length: A_LONG_PLAYER,
                    grain: Grain::MINUTE,
                }),
            ],
            SortOrder::Plays,
        ),
    ]
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
                compare: Compare::AtLeast,
                length: A_LONG_PLAYER,
                grain: Grain::MINUTE,
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
    Candidate::new(
        name,
        reason,
        written(asking(smallvec![Condition::Term(term)]), sort, reading),
    )
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

fn asking_each(each: [Conditions; 2]) -> Search {
    Search {
        clauses: each
            .into_iter()
            .map(|all| Clause {
                any: smallvec![Asked { denied: false, all }],
            })
            .collect(),
    }
}

fn the_whole_name(column: Column, text: &str) -> Word {
    Word {
        column: Some(column),
        text: text.to_owned(),
        reach: Reach::Whole,
    }
}

fn fits_in_a_phrase(text: &str) -> bool {
    !text.contains('"') && text.chars().any(char::is_alphanumeric)
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
    use resonate_core::SampleFormat;

    use super::*;
    use crate::{Codec, Library};

    #[test]
    fn a_decade_is_named_where_it_has_a_name_and_numbered_where_it_has_none() {
        assert_eq!(spelled(1990), "the nineties");
        assert_eq!(spelled(2000), "the 2000s");
        assert_eq!(beginning_in_capitals(&spelled(1970)), "The seventies");
        assert_eq!(beginning_in_capitals(""), "");
    }

    #[test]
    fn a_name_with_no_letter_or_digit_is_no_phrase_a_search_can_hold() {
        assert!(!fits_in_a_phrase("!!!"));
        assert!(!fits_in_a_phrase("?"));
        assert!(!fits_in_a_phrase("say \"hi\""));
        assert!(fits_in_a_phrase("!!! (Chk Chk Chk)"));
        assert!(fits_in_a_phrase("Sigur Rós"));
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
                "length:>=10m",
            ]
        );
        for text in &written {
            let read = Search::read(text);

            assert!(!read.is_empty(), "{text} reads as no search at all");
            assert_eq!(read.to_string(), *text);
        }
    }

    fn filed(shelf: &[(&str, &str)]) -> Filing {
        let shelved: Vec<(String, String)> = shelf
            .iter()
            .map(|(artist, genre)| ((*artist).to_owned(), (*genre).to_owned()))
            .collect();

        Filing {
            kin: Kinship::of(&shelved),
            ..Filing::default()
        }
    }

    #[test]
    fn what_is_offered_in_turn_moves_on_a_day_at_a_time_and_comes_round_again() {
        let held = ["a", "b", "c", "d"];

        assert_eq!(taken_in_turn(&held, 0, 2), ["a", "b"]);
        assert_eq!(taken_in_turn(&held, 1, 2), ["b", "c"]);
        assert_eq!(taken_in_turn(&held, 3, 2), ["d", "a"]);
        assert_eq!(taken_in_turn(&held, 4, 2), ["a", "b"]);
        assert_eq!(taken_in_turn(&held, -1, 1), ["d"]);
        assert_eq!(taken_in_turn(&held, 2, 9).len(), held.len());
        assert!(taken_in_turn::<&str>(&[], 2, 2).is_empty());
    }

    #[test]
    fn a_kindred_artist_shares_a_genre_the_rarer_the_nearer_and_is_not_one_already_heard() {
        let shelf = filed(&[
            ("Seed", "Rock; Krautrock"),
            ("Everyone", "Rock"),
            ("Others", "Rock"),
            ("Anyone", "Rock"),
            ("Neighbour", "Krautrock"),
            ("Heard Already", "Krautrock"),
            ("Stranger", "Polka"),
        ]);
        let known: AHashSet<String> = ["heard already".to_owned()].into_iter().collect();

        assert_eq!(
            kindred_to(&shelf.kin, "Seed", &known),
            ["Neighbour", "Anyone", "Everyone", "Others"]
        );
        assert!(kindred_to(&shelf.kin, "Nobody", &known).is_empty());
    }

    #[test]
    fn a_kindred_artist_is_one_artist_however_spelled() {
        let shelf = filed(&[
            ("Seed", "Rock"),
            ("Sigur Rós", "Rock"),
            ("SIGUR ROS", "Rock"),
        ]);

        assert_eq!(kindred_to(&shelf.kin, "seed", &AHashSet::new()).len(), 1);
    }

    #[test]
    fn everything_read_from_listening_reads_back_as_what_it_was_written_as() {
        let written: Vec<String> = [
            more_like("Seed", &["One".to_owned(), "Two".to_owned()]),
            deep_cuts_of("Seed"),
            an_artist_to_discover("Seed"),
            an_artist_left_behind("Seed"),
            a_genre_unheard("Rock"),
            a_genre_of_a_decade(1980, "Rock"),
        ]
        .into_iter()
        .chain(the_sounds_the_catalog_holds())
        .map(|candidate| {
            candidate
                .query
                .text
                .expect("each asks the catalog something")
        })
        .collect();

        assert_eq!(
            written,
            vec![
                "artist:=\"One\" or artist:=\"Two\"",
                "artist:=\"Seed\" plays:<=1",
                "artist:=\"Seed\"",
                "artist:=\"Seed\"",
                "genre:=\"Rock\" plays:0",
                "year:1980-1989 genre:=\"Rock\"",
                "length:<=2m",
                "is:multichannel",
                "is:mono",
                "length:6m-10m",
            ]
        );
        for text in &written {
            assert_eq!(Search::read(text).to_string(), *text);
        }
    }

    #[test]
    fn the_lists_read_from_listening_are_made_from_plain_readings_and_no_catalog() {
        let now = SystemTime::now();
        let heard = artists_as_heard(vec![
            ArtistHeard {
                name: "Quiet".to_owned(),
                plays: 0,
                held: 7,
                last_played: None,
            },
            ArtistHeard {
                name: "Long Gone".to_owned(),
                plays: 40,
                held: 12,
                last_played: now.checked_sub(Duration::from_secs(300 * SECONDS_PER_DAY)),
            },
            ArtistHeard {
                name: "!!!".to_owned(),
                plays: 90,
                held: 12,
                last_played: None,
            },
        ]);
        let kin = Kinship::of(&[
            ("Played Lately".to_owned(), "Dub".to_owned()),
            ("Neighbour".to_owned(), "Dub".to_owned()),
        ]);

        let made = listening_lists(
            &kin,
            &heard,
            &["Played Lately".to_owned()],
            &["Dub".to_owned()],
            0,
            now,
        );
        let named: Vec<&str> = made
            .iter()
            .map(|candidate| candidate.name.as_str())
            .collect();

        assert_eq!(heard[0].name, "Long Gone");
        assert!(named.contains(&"On repeat"));
        assert!(named.contains(&"More like Played Lately"));
        assert!(named.contains(&"Played Lately, deep cuts"));
        assert!(named.contains(&"Discover Quiet"));
        assert!(named.contains(&"Back to Long Gone"));
        assert!(named.contains(&"Unheard Dub"));
        assert!(!named.iter().any(|name| name.contains("!!!")));
    }

    #[test]
    fn artists_heard_lately_are_one_artist_however_spelled_most_heard_first_and_few() {
        let lately = artists_heard_lately([
            ("Sigur Rós".to_owned(), 100),
            ("Air".to_owned(), 500),
            ("SIGUR ROS".to_owned(), 450),
            ("a".to_owned(), 1),
            ("b".to_owned(), 1),
            ("c".to_owned(), 1),
            ("d".to_owned(), 1),
            ("e".to_owned(), 1),
        ]);

        assert_eq!(lately.len(), ARTISTS_OF_LATE_AT_MOST as usize);
        assert_eq!(lately[0], "Sigur Rós");
        assert_eq!(lately[1], "Air");
    }

    #[test]
    fn a_list_holding_the_rows_of_one_before_it_gives_way_to_it() {
        let mut candidates = the_shape_of_the_catalog();
        candidates.truncate(1);
        candidates.extend(listening_lists(
            &Kinship::default(),
            &[],
            &[],
            &[],
            0,
            SystemTime::now(),
        ));
        let every_list_alike = |_: &SavedQuery| -> std::result::Result<Weighed, ()> {
            Ok(Weighed {
                rows: ENOUGH_TO_OFFER,
                length: None,
            })
        };

        let offered = worth_offering(candidates, every_list_alike).expect("weighing cannot fail");

        assert_eq!(offered.len(), 1);
        assert_eq!(offered[0].0.name, "Never heard");
    }

    #[test]
    fn a_list_too_short_for_its_floor_is_not_offered() {
        let candidates = vec![deep_cuts_of("Seed")];
        let four_songs = |_: &SavedQuery| -> std::result::Result<Weighed, ()> {
            Ok(Weighed {
                rows: ENOUGH_OF_ONE_ARTIST - 1,
                length: None,
            })
        };

        assert!(
            worth_offering(candidates, four_songs)
                .expect("weighing cannot fail")
                .is_empty()
        );
    }

    #[test]
    fn a_genre_of_a_decade_says_both() {
        let found = a_genre_of_a_decade(1980, "rock");

        assert_eq!(found.name, "Rock of the eighties");
        assert_eq!(found.reason.says(), "One genre, from the eighties");
        assert_eq!(found.reason.kind(), Kind::Eras);
    }

    struct Shelved<'a> {
        artist: &'a str,
        genre: &'a str,
        songs: u32,
        plays: u32,
        played_ago: Option<Duration>,
        favourite: bool,
    }

    const LONG_AGO: Duration = Duration::from_secs(400 * SECONDS_PER_DAY);
    const A_WEEK_AGO: Duration = Duration::from_secs(7 * SECONDS_PER_DAY);
    const SOME_DAY: i64 = 20_000;
    const RATE: i64 = 44_100;

    fn stacked(library: &Library, shelves: &[Shelved<'_>], now: SystemTime) {
        library
            .inner()
            .write(|transaction| {
                let store_failed = |source| Error::store(StoreOp::Insert, source);

                transaction
                    .execute("INSERT INTO roots (id, path) VALUES (1, '/music')", [])
                    .map_err(store_failed)?;
                for (place, shelf) in shelves.iter().enumerate() {
                    let artist: i64 = transaction
                        .query_row(
                            "INSERT INTO artists (key, name) VALUES (?1, ?1)
                             ON CONFLICT(key) DO UPDATE SET name = name
                             RETURNING id",
                            params![shelf.artist],
                            |row| row.get(0),
                        )
                        .map_err(store_failed)?;
                    for song in 0..shelf.songs {
                        let played = shelf.played_ago.and_then(|ago| now.checked_sub(ago));
                        let track: i64 = transaction
                            .query_row(
                                "INSERT INTO tracks (root_id, path, title, sample_rate, channels,
                                                     sample_format, codec, file_size, modified,
                                                     added, seen, plays, played, artist, artist_id,
                                                     genre, favourite, duration)
                                 VALUES (1, ?1, ?2, 44100, 2, ?3, ?4, 0, 0, 0, 0, ?5, ?6, ?7, ?8,
                                         ?9, ?10, ?11)
                                 RETURNING id",
                                params![
                                    format!("/music/{place}/{song}.wav"),
                                    format!("{} {song}", shelf.artist),
                                    store::format_code(SampleFormat::S16),
                                    store::codec_code(Codec::Pcm),
                                    shelf.plays,
                                    played.map(store::to_nanos),
                                    shelf.artist,
                                    artist,
                                    shelf.genre,
                                    shelf.favourite.then_some(1_i64),
                                    RATE * (100 + place as i64),
                                ],
                                |row| row.get(0),
                            )
                            .map_err(store_failed)?;
                        transaction
                            .execute(
                                "INSERT INTO tracks_fts (rowid, title, artist, album, genre, lyrics)
                                 VALUES (?1, ?2, ?3, '', ?4, '')",
                                params![
                                    track,
                                    store::folded_letters(&format!("{} {song}", shelf.artist)),
                                    store::folded_letters(shelf.artist),
                                    store::folded_letters(shelf.genre),
                                ],
                            )
                            .map_err(store_failed)?;
                        for _ in 0..shelf.plays {
                            let Some(played) = played else { continue };
                            transaction
                                .execute(
                                    "INSERT INTO listens (track_id, at, heard) VALUES (?1, ?2, ?3)",
                                    params![track, store::to_nanos(played), 180_000_000_000_i64],
                                )
                                .map_err(store_failed)?;
                        }
                    }
                }

                Ok(())
            })
            .expect("the catalog takes the shelves");
    }

    const fn shelved<'a>(artist: &'a str, genre: &'a str, songs: u32, plays: u32) -> Shelved<'a> {
        Shelved {
            artist,
            genre,
            songs,
            plays,
            played_ago: None,
            favourite: false,
        }
    }

    fn a_listeners_shelves() -> Vec<Shelved<'static>> {
        vec![
            Shelved {
                played_ago: Some(A_WEEK_AGO),
                ..shelved("Played Lately", "Krautrock", 12, 4)
            },
            shelved("Played Lately", "Krautrock", 7, 0),
            shelved("A Neighbour", "Krautrock", 11, 0),
            shelved("Another Neighbour", "Krautrock", 5, 0),
            Shelved {
                played_ago: Some(LONG_AGO),
                favourite: true,
                ..shelved("Long Gone", "Polka", 13, 5)
            },
            Shelved {
                played_ago: Some(LONG_AGO),
                ..shelved("Long Gone", "Polka", 5, 5)
            },
            shelved("Long Gone", "Polka", 8, 0),
            Shelved {
                played_ago: Some(LONG_AGO),
                ..shelved("Tried Once", "Polka", 14, 1)
            },
            shelved("Tried Once", "Polka", 6, 0),
            Shelved {
                favourite: true,
                ..shelved("Starred Unplayed", "Polka", 15, 0)
            },
            shelved("Starred Unplayed", "Polka", 3, 0),
            Shelved {
                played_ago: Some(A_WEEK_AGO),
                ..shelved("Filler", "Polka", 5, 1)
            },
        ]
    }

    fn offered_on(day: i64) -> Vec<Suggestion> {
        let now = SystemTime::now();
        let library = Library::open_in_memory().expect("an in-memory catalog opens");
        stacked(&library, &a_listeners_shelves(), now);

        let filing = Filing::read(library.inner()).expect("the catalog is filed");

        suggestions(library.inner(), &filing, day, now).expect("the catalog suggests")
    }

    fn names_of_those(offered: &[Suggestion]) -> Vec<&str> {
        offered.iter().map(|held| held.name.as_str()).collect()
    }

    fn holding(offered: &[Suggestion], name: &str) -> u64 {
        offered
            .iter()
            .find(|suggestion| suggestion.name == name)
            .map_or_else(
                || panic!("{name} was not offered among {:?}", names_of_those(offered)),
                |suggestion| suggestion.rows,
            )
    }

    #[test]
    fn what_a_listener_played_and_when_decides_what_they_are_offered() {
        let offered = offered_on(SOME_DAY);

        assert_eq!(holding(&offered, "On repeat"), 12);
        assert_eq!(holding(&offered, "Forgotten favourites"), 13);
        assert_eq!(holding(&offered, "Old flames"), 18);
        assert_eq!(holding(&offered, "Second chances"), 14);
        assert_eq!(holding(&offered, "Starred, never played"), 15);
        assert_eq!(holding(&offered, "More like Played Lately"), 16);
        assert_eq!(holding(&offered, "Played Lately, deep cuts"), 7);
        assert_eq!(holding(&offered, "Unheard Polka"), 32);
        assert_eq!(holding(&offered, "Unheard Krautrock"), 23);
    }

    #[test]
    fn a_list_read_from_listening_gives_way_to_one_holding_the_same_rows() {
        let offered = offered_on(SOME_DAY);
        let names = names_of_those(&offered);

        assert!(names.contains(&"Long Gone"), "{names:?}");
        assert!(
            !names.contains(&"Back to Long Gone"),
            "two cards hold every song by one artist: {names:?}"
        );
    }

    #[test]
    fn an_artist_offered_to_discover_is_someone_else_another_day() {
        const NEVER_PLAYED_ARTISTS: i64 = 3;
        const ARTISTS_BEYOND_THE_MOST_PLAYED: i64 = 4;
        const A_DAY_BOTH_TURNS_COME_ROUND_ON: i64 =
            NEVER_PLAYED_ARTISTS * ARTISTS_BEYOND_THE_MOST_PLAYED * 1_667;
        const DISCOVERED: &str = "Discover Another Neighbour";

        let day = A_DAY_BOTH_TURNS_COME_ROUND_ON + 2;
        let that_day = offered_on(day);
        let the_next = offered_on(day + 1);

        assert_eq!(holding(&that_day, DISCOVERED), 5);
        assert!(!names_of_those(&the_next).contains(&DISCOVERED));
    }

    #[test]
    fn no_two_reasons_say_the_same() {
        let reasons = [
            Reason::Decade(1990),
            Reason::GenreOfADecade(1990),
            Reason::Genre,
            Reason::Artist,
            Reason::NeverHeard,
            Reason::MostPlayed,
            Reason::RecentlyAdded,
            Reason::FreshArrivals,
            Reason::BackCatalogue,
            Reason::OnRepeat,
            Reason::KindredArtists,
            Reason::DeepCuts,
            Reason::ArtistToDiscover,
            Reason::UnheardGenre,
            Reason::ForgottenFavourites,
            Reason::OldFlames,
            Reason::SecondChances,
            Reason::StarredUnheard,
            Reason::ArtistLeftBehind,
            Reason::HiRes,
            Reason::LongPlayers,
            Reason::SlowBurners,
            Reason::ShortAndSharp,
            Reason::Surround,
            Reason::Mono,
        ];
        let said: AHashSet<String> = reasons.iter().map(Reason::says).collect();

        assert_eq!(said.len(), reasons.len());
    }

    #[test]
    fn a_genre_is_asked_for_as_the_whole_name_it_is() {
        assert_eq!(
            the_whole_name(Column::Genre, "progressive rock").reach,
            Reach::Whole
        );
        assert_eq!(the_whole_name(Column::Genre, "rock").reach, Reach::Whole);
        assert_eq!(
            the_whole_name(Column::Artist, "Nirvana").reach,
            Reach::Whole
        );
        assert!(!fits_in_a_phrase("a \"name\""));
    }
}
