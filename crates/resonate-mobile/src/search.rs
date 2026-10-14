use std::{
    cmp::{Ordering, Reverse},
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use parking_lot::Mutex;
use resonate_codec::Codec;
use resonate_core::{folded_letters, is_lettered};
use resonate_library::{Column, Compare, Condition, Grain, Reach, Search, Shape, Spellings, Term};
use rusqlite::Connection;

use crate::{
    Error, MobilePlayer, Result, StoreOp, Track,
    catalog::{Catalog, read},
    features::{TrackFilter, TrackOrder},
};

const LISTED_AT_MOST: u32 = 500;

const COMPLETIONS_AT_MOST: usize = 6;

const LETTERS_A_PIECE_INSIDE_A_WORD_NEEDS: usize = 3;

const BETWEEN_KEY_PARTS: char = '\u{1f}';

const MILLISECONDS_A_SECOND: f64 = 1_000.0;

const CD_SAMPLE_RATE: u32 = 44_100;

const CD_SAMPLE_DEPTH: u32 = 16;

const YEAR_DIGITS: usize = 4;

const FIRST_DISC: u32 = 1;

static NOTHING_SUNG: Name = Name { words: Vec::new() };

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SearchColumn {
    Title,
    Artist,
    Album,
    Genre,
    Lyrics,
}

impl From<SearchColumn> for Column {
    fn from(column: SearchColumn) -> Self {
        match column {
            SearchColumn::Title => Self::Title,
            SearchColumn::Artist => Self::Artist,
            SearchColumn::Album => Self::Album,
            SearchColumn::Genre => Self::Genre,
            SearchColumn::Lyrics => Self::Lyrics,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TextRange {
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Enum)]
pub enum CollectionOrder {
    #[default]
    Alphabetical,
    Artist,
    Year,
    Added,
    Plays,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct AlbumHit {
    pub key: String,
    pub title: String,
    pub artist: String,
    pub year: Option<u32>,
    pub songs: u32,
    pub duration_ms: u64,
    pub artwork_track: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ArtistHit {
    pub name: String,
    pub songs: u32,
    pub albums: u32,
    pub artwork_track: u64,
}

#[derive(Clone, Debug, Default, uniffi::Record)]
pub struct LibrarySearch {
    pub tracks: Vec<Track>,
    pub track_count: u32,
    pub albums: Vec<AlbumHit>,
    pub artists: Vec<ArtistHit>,
    pub reads: Vec<String>,
    pub did_you_mean: Option<String>,
    pub completions: Vec<String>,
}

#[uniffi::export]
pub fn highlights(query: String, text: String, column: SearchColumn) -> Vec<TextRange> {
    Search::read(&query)
        .lit(&text, column.into())
        .into_iter()
        .map(|run| TextRange {
            start: utf16_units_before(&text, run.start),
            end: utf16_units_before(&text, run.end),
        })
        .collect()
}

#[uniffi::export]
impl MobilePlayer {
    pub fn search_library(&self, text: String, limit: u32) -> Result<LibrarySearch> {
        self.services.searching.search(&self.catalog, &text, limit)
    }

    pub fn albums(&self, order: CollectionOrder) -> Result<Vec<AlbumHit>> {
        self.services.searching.albums(&self.catalog, order)
    }

    pub fn artists(&self, order: CollectionOrder) -> Result<Vec<ArtistHit>> {
        self.services.searching.artists(&self.catalog, order)
    }

    pub fn album_tracks(&self, key: String) -> Result<Vec<Track>> {
        self.services.searching.album_tracks(&self.catalog, &key)
    }

    pub fn artist_tracks(&self, name: String) -> Result<Vec<Track>> {
        self.services.searching.artist_tracks(&self.catalog, &name)
    }
}

#[derive(Default)]
pub(crate) struct Searching {
    held: Mutex<Option<Arc<Index>>>,
    sung: Mutex<Option<Arc<Sung>>>,
}

impl Searching {
    pub(crate) fn search(
        &self,
        catalog: &Catalog,
        text: &str,
        limit: u32,
    ) -> Result<LibrarySearch> {
        let search = Search::read(text);
        if search.is_empty() {
            return Ok(LibrarySearch::default());
        }

        let index = self.index(catalog)?;
        let asking = Asking::of(&search, now());
        let sung = self.sung_for(catalog, &asking)?;
        let found = index.ranked(&asking, sung.as_deref());
        let shown = limit.min(LISTED_AT_MOST) as usize;

        let tracks = found
            .iter()
            .take(shown)
            .map(|at| index.rows[*at].track.clone())
            .collect();
        let albums = index.albums_among(&found, &asking, shown);
        let artists = index.artists_among(&found, &asking, shown);
        let spellings = index.spellings();
        let did_you_mean = found
            .is_empty()
            .then(|| spellings.did_you_mean(text))
            .flatten();
        let completions = spellings
            .completing(text)
            .into_iter()
            .take(COMPLETIONS_AT_MOST)
            .collect();

        Ok(LibrarySearch {
            tracks,
            track_count: counted(found.len()),
            albums,
            artists,
            reads: search.reads(),
            did_you_mean,
            completions,
        })
    }

    pub(crate) fn filtered(
        &self,
        catalog: &Catalog,
        filter: &TrackFilter,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<Track>> {
        let asking = (!filter.text.trim().is_empty())
            .then(|| Asking::of(&Search::read(&filter.text), now()));
        if asking.as_ref().is_some_and(Asking::asks_nothing) {
            return Ok(Vec::new());
        }

        let index = self.index(catalog)?;
        let sung = match &asking {
            Some(asking) => self.sung_for(catalog, asking)?,
            None => None,
        };
        let narrowing = Narrowing::of(filter);

        let mut found: Vec<usize> = index
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| narrowing.admits(row))
            .filter(|(_, row)| {
                asking.as_ref().is_none_or(|asking| {
                    asking
                        .standing(row, sung.as_deref().and_then(|sung| sung.of(row)))
                        .is_some()
                })
            })
            .map(|(at, _)| at)
            .collect();
        found.sort_unstable_by(|one, other| {
            let (one, other) = (&index.rows[*one], &index.rows[*other]);
            let ordered = in_track_order(filter.order, one, other);
            let directed = if filter.descending {
                ordered.reverse()
            } else {
                ordered
            };
            directed.then(one.track.id.cmp(&other.track.id))
        });

        Ok(found
            .into_iter()
            .skip(offset as usize)
            .take(limit.min(LISTED_AT_MOST) as usize)
            .map(|at| index.rows[at].track.clone())
            .collect())
    }

    pub(crate) fn albums(
        &self,
        catalog: &Catalog,
        order: CollectionOrder,
    ) -> Result<Vec<AlbumHit>> {
        let index = self.index(catalog)?;
        let mut albums: Vec<&AlbumGroup> = index.albums.iter().collect();
        albums.sort_by(|one, other| album_order(order, one, other));

        Ok(albums.into_iter().map(AlbumGroup::hit).collect())
    }

    pub(crate) fn artists(
        &self,
        catalog: &Catalog,
        order: CollectionOrder,
    ) -> Result<Vec<ArtistHit>> {
        let index = self.index(catalog)?;
        let mut artists: Vec<&ArtistGroup> = index.artists.iter().collect();
        artists.sort_by(|one, other| artist_order(order, one, other));

        Ok(artists.into_iter().map(ArtistGroup::hit).collect())
    }

    pub(crate) fn album_tracks(&self, catalog: &Catalog, key: &str) -> Result<Vec<Track>> {
        let index = self.index(catalog)?;

        Ok(index
            .album_at
            .get(key)
            .map(|group| index.tracks_at(&index.albums[*group].rows))
            .unwrap_or_default())
    }

    pub(crate) fn artist_tracks(&self, catalog: &Catalog, name: &str) -> Result<Vec<Track>> {
        let index = self.index(catalog)?;

        Ok(index
            .artist_at
            .get(&folded_letters(name.trim()))
            .map(|group| index.tracks_at(&index.artists[*group].rows))
            .unwrap_or_default())
    }

    fn index(&self, catalog: &Catalog) -> Result<Arc<Index>> {
        if let Some(index) = self.held.lock().as_ref()
            && index.revision == catalog.revision()
        {
            return Ok(Arc::clone(index));
        }

        let connection = catalog.0.lock();
        let revision = catalog.revision();
        let index = Arc::new(
            Index::read(&connection, revision)
                .map_err(|source| Error::store(StoreOp::Read, source))?,
        );
        *self.held.lock() = Some(Arc::clone(&index));

        Ok(index)
    }

    fn sung_for(&self, catalog: &Catalog, asking: &Asking) -> Result<Option<Arc<Sung>>> {
        if !asking.sings {
            return Ok(None);
        }

        let connection = catalog.0.lock();
        let stamp =
            Sung::stamp_of(&connection).map_err(|source| Error::store(StoreOp::Read, source))?;
        let mut held = self.sung.lock();
        if let Some(sung) = held.as_ref()
            && sung.stamp == stamp
        {
            return Ok(Some(Arc::clone(sung)));
        }

        let sung = Arc::new(
            Sung::read(&connection, stamp).map_err(|source| Error::store(StoreOp::Read, source))?,
        );
        *held = Some(Arc::clone(&sung));

        Ok(Some(sung))
    }
}

#[derive(Debug, Default)]
struct Name {
    words: Vec<Box<str>>,
}

impl Name {
    fn of(text: &str) -> Self {
        Self {
            words: pieces_of(text),
        }
    }

    fn is(&self, pieces: &[Box<str>]) -> bool {
        self.words == pieces
    }

    fn opens_with(&self, pieces: &[Box<str>]) -> bool {
        self.words.starts_with(pieces)
    }

    fn begins_with(&self, pieces: &[Box<str>]) -> bool {
        let Some((last, before)) = pieces.split_last() else {
            return false;
        };

        self.words.starts_with(before)
            && self
                .words
                .get(before.len())
                .is_some_and(|word| word.starts_with(&**last))
    }

    fn holds_the_run(&self, pieces: &[Box<str>]) -> bool {
        !pieces.is_empty() && self.words.windows(pieces.len()).any(|run| run == pieces)
    }

    fn is_in_any_order(&self, sorted: &[Box<str>]) -> bool {
        if self.words.len() != sorted.len() {
            return false;
        }
        let mut words: Vec<&str> = self.words.iter().map(|word| &**word).collect();
        words.sort_unstable();

        words.into_iter().eq(sorted.iter().map(|piece| &**piece))
    }

    fn fit_of(&self, piece: &str, inside: bool) -> Option<Fit> {
        let mut best = None;
        for word in &self.words {
            let fit = if **word == *piece {
                return Some(Fit::WholeWord);
            } else if word.starts_with(piece) {
                Fit::WordPrefix
            } else if inside && word.contains(piece) {
                Fit::Inside
            } else {
                continue;
            };
            best = best.max(Some(fit));
        }

        best
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Fit {
    Inside,
    WordPrefix,
    WholeWord,
    NamePrefix,
    WholeName,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Field {
    Lyrics = 0,
    Genre = 1,
    Album = 2,
    Artist = 3,
    Title = 4,
}

impl Field {
    const fn of(column: Column) -> Self {
        match column {
            Column::Title => Self::Title,
            Column::Artist => Self::Artist,
            Column::Album => Self::Album,
            Column::Genre => Self::Genre,
            Column::Lyrics => Self::Lyrics,
        }
    }

    fn reached_by(self, column: Option<Column>) -> bool {
        column.map_or(self != Self::Lyrics, |column| Self::of(column) == self)
    }

    const fn weight(self) -> u32 {
        self as u32
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Standing {
    fit: Fit,
    weight: u32,
}

const UNRANKED: Standing = Standing {
    fit: Fit::WholeName,
    weight: 0,
};

#[derive(Clone, Debug)]
struct Worded {
    column: Option<Column>,
    reach: Reach,
    pieces: Vec<Box<str>>,
    inside: bool,
}

impl Worded {
    fn of(column: Option<Column>, reach: Reach, text: &str, inside: bool) -> Self {
        let pieces = pieces_of(text);
        let inside = inside
            && reach == Reach::Begins
            && pieces
                .iter()
                .all(|piece| piece.chars().count() >= LETTERS_A_PIECE_INSIDE_A_WORD_NEEDS);

        Self {
            column,
            reach,
            pieces,
            inside,
        }
    }

    fn fit(&self, names: &[(Field, &Name)]) -> Option<(Fit, Field)> {
        let reached = || {
            names
                .iter()
                .filter(|(field, _)| field.reached_by(self.column))
        };
        let pieces = self.pieces.as_slice();

        match self.reach {
            Reach::Whole => reached()
                .filter(|(_, name)| name.is(pieces))
                .map(|(field, _)| (Fit::WholeName, *field))
                .max(),
            Reach::Phrase => reached()
                .filter_map(|(field, name)| {
                    let fit = if name.is(pieces) {
                        Fit::WholeName
                    } else if name.opens_with(pieces) {
                        Fit::NamePrefix
                    } else if name.holds_the_run(pieces) {
                        Fit::WholeWord
                    } else {
                        return None;
                    };
                    Some((fit, *field))
                })
                .max(),
            Reach::Begins => {
                let named = reached()
                    .filter_map(|(field, name)| {
                        if name.is(pieces) {
                            Some((Fit::WholeName, *field))
                        } else if name.begins_with(pieces) {
                            Some((Fit::NamePrefix, *field))
                        } else {
                            None
                        }
                    })
                    .max();
                if named.is_some() {
                    return named;
                }

                let mut worst: Option<(Fit, Field)> = None;
                for piece in pieces {
                    let best = reached()
                        .filter_map(|(field, name)| {
                            name.fit_of(piece, self.inside).map(|fit| (fit, *field))
                        })
                        .max()?;
                    worst = Some(worst.map_or(best, |worst| worst.min(best)));
                }
                worst
            }
        }
    }
}

#[derive(Clone, Debug)]
enum Asks {
    Word(Worded),
    Term(Term),
}

#[derive(Clone, Debug)]
struct Alternative {
    denied: bool,
    all: Vec<Asks>,
}

struct Asking {
    narrowing: Vec<Vec<Alternative>>,
    ranked: Vec<Worded>,
    joined: Option<Joined>,
    sings: bool,
    now: i64,
}

struct Joined {
    ordered: Vec<Box<str>>,
    sorted: Vec<Box<str>>,
}

impl Asking {
    fn of(search: &Search, now: i64) -> Self {
        let mut narrowing = Vec::new();
        let mut ranked = Vec::new();
        let mut sings = false;

        for clause in &search.clauses {
            if let Some(word) = clause.lone_word() {
                sings |= word.column == Some(Column::Lyrics);
                ranked.push(Worded::of(word.column, word.reach, &word.text, true));
                continue;
            }

            let alternatives = clause
                .any
                .iter()
                .map(|asked| Alternative {
                    denied: asked.denied,
                    all: asked
                        .all
                        .iter()
                        .map(|condition| match condition {
                            Condition::Word(word) => {
                                sings |= word.column == Some(Column::Lyrics);
                                Asks::Word(Worded::of(word.column, word.reach, &word.text, false))
                            }
                            Condition::Term(term) => Asks::Term(*term),
                        })
                        .collect(),
                })
                .collect();
            narrowing.push(alternatives);
        }

        let joined = (ranked.len() > 1
            && ranked
                .iter()
                .all(|word| word.column.is_none() && word.reach == Reach::Begins))
        .then(|| {
            let ordered: Vec<Box<str>> = ranked
                .iter()
                .flat_map(|word| word.pieces.iter().cloned())
                .collect();
            let mut sorted = ordered.clone();
            sorted.sort_unstable();
            Joined { ordered, sorted }
        });

        Self {
            narrowing,
            ranked,
            joined,
            sings,
            now,
        }
    }

    fn asks_nothing(&self) -> bool {
        self.narrowing.is_empty() && self.ranked.is_empty()
    }

    fn standing(&self, row: &Row, sung: Option<&Name>) -> Option<Standing> {
        let names = row.names(sung.unwrap_or(&NOTHING_SUNG));

        let narrowed = self.narrowing.iter().all(|clause| {
            clause
                .iter()
                .any(|alternative| alternative.holds(&names, row, self.now))
        });
        if !narrowed {
            return None;
        }

        if self.ranked.is_empty() {
            return Some(UNRANKED);
        }
        let mut fit = Fit::WholeName;
        let mut weight = 0;
        for word in &self.ranked {
            let (fitted, field) = word.fit(&names)?;
            fit = fit.min(fitted);
            weight += field.weight();
        }
        let worded = Standing { fit, weight };

        Some(
            self.joined
                .as_ref()
                .and_then(|joined| joined.standing(&names, self.ranked.len()))
                .map_or(worded, |named| named.max(worded)),
        )
    }

    fn name_fit(&self, names: &[(Field, &Name)]) -> Option<Fit> {
        if self.ranked.is_empty() {
            return None;
        }
        let mut fit = Fit::WholeName;
        for word in &self.ranked {
            fit = fit.min(word.fit(names)?.0);
        }

        Some(fit)
    }
}

impl Joined {
    fn standing(&self, names: &[(Field, &Name)], words: usize) -> Option<Standing> {
        names
            .iter()
            .filter(|(field, _)| *field != Field::Lyrics)
            .filter_map(|(field, name)| {
                let fit = if name.is_in_any_order(&self.sorted) {
                    Fit::WholeName
                } else if name.begins_with(&self.ordered) {
                    Fit::NamePrefix
                } else {
                    return None;
                };
                Some(Standing {
                    fit,
                    weight: field.weight() * counted(words),
                })
            })
            .max()
    }
}

impl Alternative {
    fn holds(&self, names: &[(Field, &Name)], row: &Row, now: i64) -> bool {
        let all = self.all.iter().all(|asks| match asks {
            Asks::Word(word) => word.fit(names).is_some(),
            Asks::Term(term) => term_holds(*term, row, now),
        });

        all != self.denied
    }
}

fn term_holds(term: Term, row: &Row, now: i64) -> bool {
    match term {
        Term::Added { .. } => false,
        Term::Plays {
            compare,
            plays,
            within,
        } => {
            let counted = match within {
                Some(age) => {
                    let since = now.saturating_sub(seconds_of(age));
                    row.heard.iter().filter(|at| **at >= since).count()
                }
                None => row.heard.len(),
            };
            compared(compare, counted as u64, u64::from(plays))
        }
        Term::Played { compare, age } => row
            .played()
            .is_some_and(|at| compared(compare.flipped(), at, now.saturating_sub(seconds_of(age)))),
        Term::Year { compare, year } => row.year.is_some_and(|held| compared(compare, held, year)),
        Term::Length {
            compare,
            length,
            grain,
        } => row.track.duration_ms > 0 && lasts(compare, row.track.duration_ms, length, grain),
        Term::Rate { compare, hertz } => compared(compare, row.track.rate, hertz),
        Term::Depth { compare, bits } => row
            .track
            .bits
            .is_some_and(|held| compared(compare, held, u32::from(bits))),
        Term::Codec(codec) => row.codec == codec,
        Term::Shape(shape) => shaped(shape, row),
    }
}

fn shaped(shape: Shape, row: &Row) -> bool {
    let track = &row.track;

    match shape {
        Shape::Lossless => row.codec.is_lossless(),
        Shape::Lossy => !row.codec.is_lossless() && row.codec != Codec::Unknown,
        Shape::Mono => track.channels == 1,
        Shape::Stereo => track.channels == 2,
        Shape::Multichannel => track.channels > 2,
        Shape::HiRes => {
            row.codec.is_lossless()
                && (track.rate > CD_SAMPLE_RATE
                    || track.bits.is_some_and(|bits| bits > CD_SAMPLE_DEPTH))
        }
        Shape::Favourite => track.favourite,
        Shape::Hidden | Shape::Fake | Shape::Suspect | Shape::Misnamed => false,
    }
}

fn compared<T: PartialOrd>(compare: Compare, held: T, bound: T) -> bool {
    match compare {
        Compare::Below => held < bound,
        Compare::AtMost => held <= bound,
        Compare::Exactly => held == bound,
        Compare::AtLeast => held >= bound,
        Compare::Above => held > bound,
    }
}

fn lasts(compare: Compare, duration_ms: u64, length: Duration, grain: Grain) -> bool {
    let seconds = duration_ms as f64 / MILLISECONDS_A_SECOND;
    let from = length.as_secs_f64();
    let past = from + grain.span().as_secs_f64();

    match compare {
        Compare::Below => seconds < from,
        Compare::AtMost => seconds < past,
        Compare::Exactly => seconds >= from && seconds < past,
        Compare::AtLeast => seconds >= from,
        Compare::Above => seconds >= past,
    }
}

struct Row {
    track: Track,
    title: Name,
    artist: Name,
    album_artist: Name,
    album: Name,
    genre: Name,
    filed_title: String,
    filed_artist: String,
    filed_album_artist: String,
    filed_album: String,
    filed_genre: String,
    year: Option<i32>,
    codec: Codec,
    heard: Vec<i64>,
    album_group: Option<usize>,
}

impl Row {
    fn of(track: Track, heard: Vec<i64>) -> Self {
        Self {
            title: Name::of(&track.title),
            artist: Name::of(&track.artist),
            album_artist: Name::of(&track.album_artist),
            album: Name::of(&track.album),
            genre: Name::of(&track.genre),
            filed_title: folded_letters(track.title.trim()),
            filed_artist: folded_letters(track.artist.trim()),
            filed_album_artist: folded_letters(track.album_artist.trim()),
            filed_album: folded_letters(track.album.trim()),
            filed_genre: folded_letters(track.genre.trim()),
            year: year_of(&track.date),
            codec: codec_named(&track.codec),
            heard,
            album_group: None,
            track,
        }
    }

    fn names<'a>(&'a self, sung: &'a Name) -> [(Field, &'a Name); 6] {
        [
            (Field::Title, &self.title),
            (Field::Artist, &self.artist),
            (Field::Artist, &self.album_artist),
            (Field::Album, &self.album),
            (Field::Genre, &self.genre),
            (Field::Lyrics, sung),
        ]
    }

    fn plays(&self) -> usize {
        self.heard.len()
    }

    fn played(&self) -> Option<i64> {
        self.heard.iter().max().copied()
    }

    fn credited_album_artist(&self) -> Option<&String> {
        (!self.filed_album_artist.is_empty() && self.filed_album_artist != self.filed_artist)
            .then_some(&self.filed_album_artist)
    }

    fn grouping_artist(&self) -> (&str, &str) {
        if self.filed_album_artist.is_empty() {
            (&self.track.artist, &self.filed_artist)
        } else {
            (&self.track.album_artist, &self.filed_album_artist)
        }
    }
}

struct AlbumGroup {
    key: String,
    title: String,
    artist: String,
    title_name: Name,
    artist_name: Name,
    filed_title: String,
    filed_artist: String,
    year: Option<i32>,
    rows: Vec<usize>,
    duration_ms: u64,
    plays: usize,
    added: u64,
    artwork_track: u64,
}

impl AlbumGroup {
    fn first(key: String, row: &Row) -> Self {
        let (artist, filed_artist) = row.grouping_artist();

        Self {
            key,
            title: row.track.album.clone(),
            artist: artist.to_owned(),
            title_name: Name::of(&row.track.album),
            artist_name: Name::of(artist),
            filed_title: row.filed_album.clone(),
            filed_artist: filed_artist.to_owned(),
            year: None,
            rows: Vec::new(),
            duration_ms: 0,
            plays: 0,
            added: 0,
            artwork_track: 0,
        }
    }

    fn settle(&mut self, rows: &[Row]) {
        self.rows
            .sort_by(|one, other| in_album_order(&rows[*one], &rows[*other]));
        let held = || self.rows.iter().map(|at| &rows[*at]);

        self.year = held().filter_map(|row| row.year).min();
        self.duration_ms = held().map(|row| row.track.duration_ms).sum();
        self.plays = held().map(Row::plays).sum();
        self.added = held().map(|row| row.track.id).max().unwrap_or_default();
        self.artwork_track = pictured_among(held());
    }

    fn names(&self) -> [(Field, &Name); 2] {
        [
            (Field::Album, &self.title_name),
            (Field::Artist, &self.artist_name),
        ]
    }

    fn hit(&self) -> AlbumHit {
        AlbumHit {
            key: self.key.clone(),
            title: self.title.clone(),
            artist: self.artist.clone(),
            year: self.year.and_then(|year| u32::try_from(year).ok()),
            songs: counted(self.rows.len()),
            duration_ms: self.duration_ms,
            artwork_track: self.artwork_track,
        }
    }
}

struct ArtistGroup {
    name: String,
    filed: String,
    name_words: Name,
    spellings: HashMap<String, usize>,
    rows: Vec<usize>,
    albums: usize,
    year: Option<i32>,
    plays: usize,
    added: u64,
    artwork_track: u64,
}

impl ArtistGroup {
    fn named(filed: String) -> Self {
        Self {
            name: String::new(),
            filed,
            name_words: Name::default(),
            spellings: HashMap::new(),
            rows: Vec::new(),
            albums: 0,
            year: None,
            plays: 0,
            added: 0,
            artwork_track: 0,
        }
    }

    fn settle(&mut self, rows: &[Row], albums: &[AlbumGroup]) {
        self.name = self
            .spellings
            .drain()
            .max_by(|(one, one_rows), (other, other_rows)| {
                (marks_in(one), one_rows, Reverse(one)).cmp(&(
                    marks_in(other),
                    other_rows,
                    Reverse(other),
                ))
            })
            .map(|(spelling, _)| spelling.trim().to_owned())
            .unwrap_or_default();
        self.name_words = Name::of(&self.name);
        self.rows
            .sort_by(|one, other| in_artist_order(&rows[*one], &rows[*other], albums));
        let held = || self.rows.iter().map(|at| &rows[*at]);

        let mut albums_held: Vec<usize> = held().filter_map(|row| row.album_group).collect();
        albums_held.sort_unstable();
        albums_held.dedup();
        self.albums = albums_held.len();
        self.year = held().filter_map(|row| row.year).max();
        self.plays = held().map(Row::plays).sum();
        self.added = held().map(|row| row.track.id).max().unwrap_or_default();
        self.artwork_track = pictured_among(held());
    }

    fn hit(&self) -> ArtistHit {
        ArtistHit {
            name: self.name.clone(),
            songs: counted(self.rows.len()),
            albums: counted(self.albums),
            artwork_track: self.artwork_track,
        }
    }
}

struct Index {
    revision: u64,
    rows: Vec<Row>,
    albums: Vec<AlbumGroup>,
    album_at: HashMap<String, usize>,
    artists: Vec<ArtistGroup>,
    artist_at: HashMap<String, usize>,
    spellings: OnceLock<Spellings>,
}

impl Index {
    fn read(connection: &Connection, revision: u64) -> rusqlite::Result<Self> {
        let mut heard: HashMap<u64, Vec<i64>> = HashMap::new();
        let mut listens = connection
            .prepare("SELECT track, began FROM listens WHERE counted = 1 AND track IS NOT NULL")?;
        for listen in listens.query_map([], |row| {
            Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)?))
        })? {
            let (track, began) = listen?;
            heard.entry(track).or_default().push(began);
        }

        let mut documents = connection.prepare("SELECT * FROM documents ORDER BY id")?;
        let rows = documents
            .query_map([], read)?
            .map(|track| {
                track.map(|track| {
                    let listened = heard.remove(&track.id).unwrap_or_default();
                    Row::of(track, listened)
                })
            })
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(Self::gathered(rows, revision))
    }

    fn gathered(mut rows: Vec<Row>, revision: u64) -> Self {
        let mut albums: Vec<AlbumGroup> = Vec::new();
        let mut album_at: HashMap<String, usize> = HashMap::new();
        for (at, row) in rows.iter_mut().enumerate() {
            if row.filed_album.is_empty() {
                continue;
            }
            let key = format!(
                "{}{BETWEEN_KEY_PARTS}{}",
                row.filed_album,
                row.grouping_artist().1
            );
            let group = *album_at.entry(key.clone()).or_insert_with(|| {
                albums.push(AlbumGroup::first(key, row));
                albums.len() - 1
            });
            albums[group].rows.push(at);
            row.album_group = Some(group);
        }
        for album in &mut albums {
            album.settle(&rows);
        }

        let mut artists: Vec<ArtistGroup> = Vec::new();
        let mut artist_at: HashMap<String, usize> = HashMap::new();
        for (at, row) in rows.iter().enumerate() {
            let credits = [
                Some((&row.filed_artist, &row.track.artist)),
                row.credited_album_artist()
                    .map(|filed| (filed, &row.track.album_artist)),
            ];
            for (filed, spelling) in credits.into_iter().flatten() {
                let group = *artist_at.entry(filed.clone()).or_insert_with(|| {
                    artists.push(ArtistGroup::named(filed.clone()));
                    artists.len() - 1
                });
                let artist = &mut artists[group];
                artist.rows.push(at);
                *artist.spellings.entry(spelling.clone()).or_default() += 1;
            }
        }
        for artist in &mut artists {
            artist.settle(&rows, &albums);
        }

        Self {
            revision,
            rows,
            albums,
            album_at,
            artists,
            artist_at,
            spellings: OnceLock::new(),
        }
    }

    fn spellings(&self) -> &Spellings {
        self.spellings.get_or_init(|| {
            let mut spellings = Spellings::default();
            for row in &self.rows {
                let track = &row.track;
                spellings.taking(Column::Title, &track.title);
                spellings.taking(Column::Artist, &track.artist);
                spellings.taking(Column::Artist, &track.album_artist);
                spellings.taking(Column::Album, &track.album);
                spellings.taking(Column::Genre, &track.genre);
            }
            spellings
        })
    }

    fn ranked(&self, asking: &Asking, sung: Option<&Sung>) -> Vec<usize> {
        let mut found: Vec<(Standing, usize)> = self
            .rows
            .iter()
            .enumerate()
            .filter_map(|(at, row)| {
                asking
                    .standing(row, sung.and_then(|sung| sung.of(row)))
                    .map(|standing| (standing, at))
            })
            .collect();
        found.sort_unstable_by(|(one_standing, one), (other_standing, other)| {
            let (one, other) = (&self.rows[*one], &self.rows[*other]);
            other_standing
                .cmp(one_standing)
                .then(other.track.favourite.cmp(&one.track.favourite))
                .then(other.plays().cmp(&one.plays()))
                .then_with(|| one.filed_title.cmp(&other.filed_title))
                .then(one.track.id.cmp(&other.track.id))
        });

        found.into_iter().map(|(_, at)| at).collect()
    }

    fn albums_among(&self, found: &[usize], asking: &Asking, most: usize) -> Vec<AlbumHit> {
        let held = first_seen(
            found.iter().filter_map(|row| self.rows[*row].album_group),
            self.albums.len(),
        );

        named_first(held, most, |group| {
            asking.name_fit(&self.albums[group].names())
        })
        .into_iter()
        .map(|group| self.albums[group].hit())
        .collect()
    }

    fn artists_among(&self, found: &[usize], asking: &Asking, most: usize) -> Vec<ArtistHit> {
        let credited = found.iter().flat_map(|row| {
            let row = &self.rows[*row];
            [Some(&row.filed_artist), row.credited_album_artist()]
                .into_iter()
                .flatten()
                .filter_map(|filed| self.artist_at.get(filed).copied())
        });
        let held = first_seen(credited, self.artists.len());

        named_first(held, most, |group| {
            asking.name_fit(&[(Field::Artist, &self.artists[group].name_words)])
        })
        .into_iter()
        .map(|group| self.artists[group].hit())
        .collect()
    }

    fn tracks_at(&self, rows: &[usize]) -> Vec<Track> {
        rows.iter().map(|at| self.rows[*at].track.clone()).collect()
    }
}

struct Sung {
    stamp: SungStamp,
    words: HashMap<u64, Name>,
}

type SungStamp = (i64, i64, i64);

impl Sung {
    fn stamp_of(connection: &Connection) -> rusqlite::Result<SungStamp> {
        connection.query_row(
            "SELECT count(*), coalesce(sum(asked), 0), coalesce(sum(length(text)), 0) FROM lyrics",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
    }

    fn read(connection: &Connection, stamp: SungStamp) -> rusqlite::Result<Self> {
        let mut query =
            connection.prepare("SELECT track, text FROM lyrics WHERE text IS NOT NULL")?;
        let words = query
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)? as u64,
                    Name::of(&without_stamps(&row.get::<_, String>(1)?)),
                ))
            })?
            .collect::<rusqlite::Result<HashMap<_, _>>>()?;

        Ok(Self { stamp, words })
    }

    fn of(&self, row: &Row) -> Option<&Name> {
        self.words.get(&row.track.id)
    }
}

struct Narrowing {
    genre: String,
    artist: String,
    album: String,
    minimum_rate: u32,
    minimum_bits: u32,
    favourites: bool,
    unplayed: bool,
}

impl Narrowing {
    fn of(filter: &TrackFilter) -> Self {
        Self {
            genre: folded_letters(filter.genre.trim()),
            artist: folded_letters(filter.artist.trim()),
            album: folded_letters(filter.album.trim()),
            minimum_rate: filter.minimum_rate,
            minimum_bits: filter.minimum_bits,
            favourites: filter.favourites,
            unplayed: filter.unplayed,
        }
    }

    fn admits(&self, row: &Row) -> bool {
        (self.genre.is_empty() || row.filed_genre == self.genre)
            && (self.artist.is_empty() || row.filed_artist == self.artist)
            && (self.album.is_empty() || row.filed_album == self.album)
            && row.track.rate >= self.minimum_rate
            && row.track.bits.unwrap_or(0) >= self.minimum_bits
            && (!self.favourites || row.track.favourite)
            && (!self.unplayed || row.heard.is_empty())
    }
}

fn in_track_order(order: TrackOrder, one: &Row, other: &Row) -> Ordering {
    match order {
        TrackOrder::Title => one.filed_title.cmp(&other.filed_title),
        TrackOrder::Album => (&one.filed_album, &one.filed_album_artist)
            .cmp(&(&other.filed_album, &other.filed_album_artist))
            .then_with(|| placed(one).cmp(&placed(other))),
        TrackOrder::Artist => (&one.filed_artist, &one.filed_album)
            .cmp(&(&other.filed_artist, &other.filed_album))
            .then_with(|| placed(one).cmp(&placed(other))),
        TrackOrder::Duration => one.track.duration_ms.cmp(&other.track.duration_ms),
        TrackOrder::Added => one.track.id.cmp(&other.track.id),
        TrackOrder::Plays => one.plays().cmp(&other.plays()),
    }
}

fn placed(row: &Row) -> (u32, u32) {
    (
        row.track.disc_number.unwrap_or(FIRST_DISC),
        row.track.track_number.unwrap_or(0),
    )
}

fn in_album_order(one: &Row, other: &Row) -> Ordering {
    let numbered = |row: &Row| row.track.track_number.is_none();

    numbered(one)
        .cmp(&numbered(other))
        .then_with(|| placed(one).cmp(&placed(other)))
        .then_with(|| one.filed_title.cmp(&other.filed_title))
        .then(one.track.id.cmp(&other.track.id))
}

fn in_artist_order(one: &Row, other: &Row, albums: &[AlbumGroup]) -> Ordering {
    let filed = |row: &Row| {
        row.album_group.map(|group| {
            let album = &albums[group];
            (album.year.is_none(), album.year, &album.filed_title, group)
        })
    };

    filed(one)
        .is_none()
        .cmp(&filed(other).is_none())
        .then_with(|| filed(one).cmp(&filed(other)))
        .then_with(|| in_album_order(one, other))
}

fn album_order(order: CollectionOrder, one: &AlbumGroup, other: &AlbumGroup) -> Ordering {
    let by_title = || {
        (&one.filed_title, &one.filed_artist, &one.key).cmp(&(
            &other.filed_title,
            &other.filed_artist,
            &other.key,
        ))
    };

    match order {
        CollectionOrder::Alphabetical => by_title(),
        CollectionOrder::Artist => (&one.filed_artist, one.year.is_none(), one.year)
            .cmp(&(&other.filed_artist, other.year.is_none(), other.year))
            .then_with(by_title),
        CollectionOrder::Year => (one.year.is_none(), Reverse(one.year))
            .cmp(&(other.year.is_none(), Reverse(other.year)))
            .then_with(by_title),
        CollectionOrder::Added => other.added.cmp(&one.added).then_with(by_title),
        CollectionOrder::Plays => other.plays.cmp(&one.plays).then_with(by_title),
    }
}

fn artist_order(order: CollectionOrder, one: &ArtistGroup, other: &ArtistGroup) -> Ordering {
    let by_name =
        || (one.filed.is_empty(), &one.filed).cmp(&(other.filed.is_empty(), &other.filed));

    match order {
        CollectionOrder::Alphabetical | CollectionOrder::Artist => by_name(),
        CollectionOrder::Year => (one.year.is_none(), Reverse(one.year))
            .cmp(&(other.year.is_none(), Reverse(other.year)))
            .then_with(by_name),
        CollectionOrder::Added => other.added.cmp(&one.added).then_with(by_name),
        CollectionOrder::Plays => other.plays.cmp(&one.plays).then_with(by_name),
    }
}

fn first_seen(groups: impl Iterator<Item = usize>, count: usize) -> Vec<usize> {
    let mut seen = vec![false; count];

    groups
        .filter(|group| !std::mem::replace(&mut seen[*group], true))
        .collect()
}

fn named_first(groups: Vec<usize>, most: usize, fit: impl Fn(usize) -> Option<Fit>) -> Vec<usize> {
    let mut weighed: Vec<(Option<Fit>, usize)> = groups
        .into_iter()
        .map(|group| (fit(group), group))
        .collect();
    weighed.sort_by_key(|(fit, _)| Reverse(*fit));

    weighed
        .into_iter()
        .take(most)
        .map(|(_, group)| group)
        .collect()
}

fn pictured_among<'a>(mut rows: impl Iterator<Item = &'a Row> + Clone) -> u64 {
    rows.clone()
        .find(|row| row.track.art_revision > 0)
        .or_else(|| rows.next())
        .map_or(0, |row| row.track.id)
}

fn pieces_of(text: &str) -> Vec<Box<str>> {
    folded_letters(text)
        .split(|letter: char| !is_lettered(letter))
        .filter(|piece| !piece.is_empty())
        .map(Box::from)
        .collect()
}

fn without_stamps(text: &str) -> String {
    let mut words = String::with_capacity(text.len());
    let mut closing = None;
    for letter in text.chars() {
        match (closing, letter) {
            (Some(close), _) if letter == close => closing = None,
            (Some(_), _) => {}
            (None, '[') => closing = Some(']'),
            (None, '<') => closing = Some('>'),
            (None, _) => words.push(letter),
        }
    }

    words
}

fn year_of(date: &str) -> Option<i32> {
    let digits: String = date
        .trim()
        .chars()
        .take_while(char::is_ascii_digit)
        .take(YEAR_DIGITS)
        .collect();

    (digits.len() == YEAR_DIGITS)
        .then(|| digits.parse().ok())
        .flatten()
}

fn codec_named(codec: &str) -> Codec {
    Codec::ALL
        .into_iter()
        .find(|known| known.as_str().eq_ignore_ascii_case(codec))
        .unwrap_or(Codec::Unknown)
}

fn marks_in(spelling: &str) -> usize {
    spelling.chars().filter(|letter| !letter.is_ascii()).count()
}

fn seconds_of(age: Duration) -> i64 {
    i64::try_from(age.as_secs()).unwrap_or(i64::MAX)
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, seconds_of)
}

fn counted(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

fn utf16_units_before(text: &str, byte: usize) -> u32 {
    counted(
        text.get(..byte)
            .map_or(0, |before| before.encode_utf16().count()),
    )
}

#[cfg(test)]
mod tests {
    use std::{path::Path, time::Instant};

    use rusqlite::params;

    use super::*;

    const SYNTHETIC_ROWS: usize = 20_000;

    const TIMED_RUNS: usize = 5;

    const SEARCH_BUDGET: Duration = if cfg!(debug_assertions) {
        Duration::from_millis(150)
    } else {
        Duration::from_millis(30)
    };

    struct Tagged<'a> {
        title: &'a str,
        artist: &'a str,
        album: &'a str,
        album_artist: &'a str,
        genre: &'a str,
        date: &'a str,
        disc: Option<u32>,
        number: Option<u32>,
    }

    const UNTAGGED: Tagged<'static> = Tagged {
        title: "",
        artist: "",
        album: "",
        album_artist: "",
        genre: "",
        date: "",
        disc: None,
        number: None,
    };

    fn catalog() -> Catalog {
        Catalog::open(Path::new(":memory:")).expect("a catalog")
    }

    fn song(catalog: &Catalog, tagged: Tagged<'_>) -> Track {
        let track = catalog
            .upsert(Track {
                uri: format!(
                    "content://test/{}-{}.flac",
                    tagged.title,
                    catalog.revision()
                ),
                name: format!("{}.flac", tagged.title),
                title: tagged.title.into(),
                artist: tagged.artist.into(),
                album: tagged.album.into(),
                duration_ms: 200_000,
                rate: 44_100,
                codec: "FLAC".into(),
                ..Track::default()
            })
            .expect("an indexed song");
        catalog
            .0
            .lock()
            .execute(
                "UPDATE documents SET album_artist = ?2, genre = ?3, date = ?4, disc_number = ?5, track_number = ?6 WHERE id = ?1",
                params![
                    track.id as i64,
                    tagged.album_artist,
                    tagged.genre,
                    tagged.date,
                    tagged.disc,
                    tagged.number
                ],
            )
            .expect("tags");
        catalog.track(track.id).expect("the tagged song")
    }

    fn titles(found: &[Track]) -> Vec<&str> {
        found.iter().map(|track| track.title.as_str()).collect()
    }

    fn library() -> Catalog {
        let catalog = catalog();
        for tagged in [
            Tagged {
                title: "Time",
                artist: "Pink Floyd",
                album: "The Dark Side of the Moon",
                genre: "Progressive Rock",
                date: "1973-03-01",
                ..UNTAGGED
            },
            Tagged {
                title: "Money",
                artist: "Pink Floyd",
                album: "The Dark Side of the Moon",
                genre: "Progressive Rock",
                date: "1973",
                ..UNTAGGED
            },
            Tagged {
                title: "Comfortably Numb (Live)",
                artist: "Pink Floyd",
                album: "Pulse",
                genre: "Progressive Rock",
                date: "1995",
                ..UNTAGGED
            },
            Tagged {
                title: "Jóga",
                artist: "Björk",
                album: "Homogenic",
                genre: "Electronic",
                date: "1997",
                ..UNTAGGED
            },
            Tagged {
                title: "So What",
                artist: "Miles Davis",
                album: "Kind of Blue",
                genre: "Jazz",
                date: "1959",
                ..UNTAGGED
            },
            Tagged {
                title: "Sexy Boy",
                artist: "Air",
                album: "Moon Safari",
                genre: "Electronic",
                date: "1998",
                ..UNTAGGED
            },
            Tagged {
                title: "Sugar Kiss",
                artist: "Airiel",
                album: "Winks & Kisses",
                genre: "Shoegaze",
                date: "2007",
                ..UNTAGGED
            },
            Tagged {
                title: "Money for Nothing",
                artist: "Dire Straits",
                album: "Brothers in Arms",
                genre: "Rock",
                date: "1985",
                ..UNTAGGED
            },
            Tagged {
                title: "Paranoid Android",
                artist: "Radiohead",
                album: "OK Computer",
                genre: "Rock",
                date: "1997",
                ..UNTAGGED
            },
        ] {
            song(&catalog, tagged);
        }
        catalog
    }

    fn searched(catalog: &Catalog, text: &str) -> LibrarySearch {
        Searching::default()
            .search(catalog, text, 100)
            .expect("a search")
    }

    #[test]
    fn words_hold_in_any_order_and_the_whole_name_ranks_first() {
        let catalog = library();

        let found = searched(&catalog, "floyd pink");

        assert_eq!(found.track_count, 3);
        assert_eq!(
            found.artists.first().map(|artist| artist.name.as_str()),
            Some("Pink Floyd")
        );
        assert!(found.reads.is_empty());
    }

    #[test]
    fn an_unaccented_word_finds_the_accented_name() {
        let catalog = library();

        let found = searched(&catalog, "bjork joga");

        assert_eq!(titles(&found.tracks), vec!["Jóga"]);
    }

    #[test]
    fn a_title_named_whole_ranks_above_one_it_begins() {
        let catalog = library();

        let found = searched(&catalog, "money");

        assert_eq!(titles(&found.tracks), vec!["Money", "Money for Nothing"]);
    }

    #[test]
    fn a_scope_a_range_a_denial_and_an_alternative_narrow() {
        let catalog = library();

        let jazz = searched(&catalog, "genre:jazz");
        let seventies = searched(&catalog, "year:1970-1979");
        let studio = searched(&catalog, "floyd -live");
        let either = searched(&catalog, "artist:air or artist:radiohead");

        assert_eq!(titles(&jazz.tracks), vec!["So What"]);
        assert_eq!(jazz.reads, vec!["genre:jazz"]);
        assert_eq!(titles(&seventies.tracks), vec!["Money", "Time"]);
        assert_eq!(titles(&studio.tracks), vec!["Money", "Time"]);
        assert_eq!(either.track_count, 3);
    }

    #[test]
    fn a_whole_name_is_only_that_name() {
        let catalog = library();

        let whole = searched(&catalog, "artist:=Air");
        let begun = searched(&catalog, "artist:air");

        assert_eq!(titles(&whole.tracks), vec!["Sexy Boy"]);
        assert_eq!(begun.track_count, 2);
    }

    #[test]
    fn terms_the_documents_cannot_answer_hold_nothing_and_their_denial_everything() {
        let catalog = library();

        let added = searched(&catalog, "added:<7d");
        let not_added = searched(&catalog, "-added:<7d");
        let measured = searched(&catalog, "is:lossless codec:flac rate:44.1 length:3:20");

        assert_eq!(added.track_count, 0);
        assert_eq!(not_added.track_count, 9);
        assert_eq!(measured.track_count, 9);
    }

    #[test]
    fn a_prefix_is_completed_and_a_typo_corrected_from_the_catalog() {
        let catalog = library();

        let completing = searched(&catalog, "radioh");
        let mistyped = searched(&catalog, "radiohaed");

        assert!(
            completing
                .completions
                .iter()
                .any(|offer| offer == "Radiohead")
        );
        assert_eq!(mistyped.track_count, 0);
        assert_eq!(mistyped.did_you_mean.as_deref(), Some("Radiohead"));
    }

    #[test]
    fn a_fragment_inside_a_word_is_found() {
        let catalog = library();

        let found = searched(&catalog, "roid");

        assert_eq!(titles(&found.tracks), vec!["Paranoid Android"]);
    }

    #[test]
    fn highlights_are_counted_in_utf16_units() {
        let accented = highlights("jog".into(), "Björk — Jóga".into(), SearchColumn::Title);
        let pictured = highlights("love".into(), "🎵 Love".into(), SearchColumn::Title);
        let elsewhere = highlights("artist:love".into(), "🎵 Love".into(), SearchColumn::Title);

        assert_eq!(accented, vec![TextRange { start: 8, end: 12 }]);
        assert_eq!(pictured, vec![TextRange { start: 3, end: 7 }]);
        assert!(elsewhere.is_empty());
    }

    #[test]
    fn albums_gather_by_folded_title_and_owner_in_disc_and_track_order() {
        let catalog = catalog();
        let searching = Searching::default();
        for tagged in [
            Tagged {
                title: "Hidden",
                album: "The Wall",
                artist: "Pink Floyd",
                ..UNTAGGED
            },
            Tagged {
                title: "Hey You",
                album: "The Wall",
                artist: "Pink Floyd",
                disc: Some(2),
                number: Some(1),
                ..UNTAGGED
            },
            Tagged {
                title: "Mother",
                album: "the wall",
                artist: "Pink Floyd",
                disc: Some(1),
                number: Some(5),
                ..UNTAGGED
            },
            Tagged {
                title: "In the Flesh?",
                album: "THE WALL",
                artist: "Roger Waters",
                album_artist: "Pink Floyd",
                disc: Some(1),
                number: Some(1),
                date: "1979",
                ..UNTAGGED
            },
            Tagged {
                title: "Loose",
                artist: "Pink Floyd",
                ..UNTAGGED
            },
        ] {
            song(&catalog, tagged);
        }

        let albums = searching
            .albums(&catalog, CollectionOrder::Alphabetical)
            .expect("albums");
        let wall = albums.first().expect("the wall");
        let tracks = searching
            .album_tracks(&catalog, &wall.key)
            .expect("the album's tracks");
        let floyd = searching
            .artist_tracks(&catalog, "PINK FLOYD")
            .expect("the artist's tracks");
        let artists = searching
            .artists(&catalog, CollectionOrder::Alphabetical)
            .expect("artists");

        assert_eq!(albums.len(), 1);
        assert_eq!(wall.songs, 4);
        assert_eq!(wall.year, Some(1979));
        assert_eq!(
            titles(&tracks),
            vec!["In the Flesh?", "Mother", "Hey You", "Hidden"]
        );
        assert_eq!(floyd.len(), 5);
        assert_eq!(
            artists
                .iter()
                .map(|artist| (artist.name.as_str(), artist.songs, artist.albums))
                .collect::<Vec<_>>(),
            vec![("Pink Floyd", 5, 1), ("Roger Waters", 1, 1)]
        );
    }

    #[test]
    fn the_unknown_artist_holds_only_songs_naming_no_artist() {
        let catalog = library();
        let searching = Searching::default();
        song(
            &catalog,
            Tagged {
                title: "Untitled",
                album: "Demos",
                album_artist: "Somebody",
                ..UNTAGGED
            },
        );
        song(
            &catalog,
            Tagged {
                title: "Nameless",
                ..UNTAGGED
            },
        );

        let unknown = searching
            .artist_tracks(&catalog, "")
            .expect("the unknown artist's tracks");
        let artists = searching
            .artists(&catalog, CollectionOrder::Alphabetical)
            .expect("artists");
        let named = searching
            .search(&catalog, "money", 10)
            .expect("a search")
            .artists;

        assert_eq!(titles(&unknown), vec!["Untitled", "Nameless"]);
        assert!(named.iter().all(|artist| !artist.name.is_empty()));
        assert_eq!(
            artists
                .last()
                .map(|artist| (artist.name.as_str(), artist.songs)),
            Some(("", 2))
        );
    }

    #[test]
    fn kept_lyrics_are_searched_without_their_stamps_once_kept() {
        let catalog = library();
        let searching = Searching::default();
        let money = searching
            .search(&catalog, "money", 1)
            .expect("a search")
            .tracks
            .remove(0);

        let before = searching
            .search(&catalog, "lyrics:\"grab that cash\"", 10)
            .expect("a lyric search");
        catalog
            .keep_lyrics(
                money.id,
                Some("[00:01.00]Money, get away\n[00:05.00]<00:05.00>Grab that cash"),
                None,
                "embedded",
            )
            .expect("kept lyrics");
        let after = searching
            .search(&catalog, "lyrics:\"grab that cash\"", 10)
            .expect("a lyric search");
        let stamp = searching
            .search(&catalog, "lyrics:00", 10)
            .expect("a stamp search");

        assert_eq!(before.track_count, 0);
        assert_eq!(titles(&after.tracks), vec!["Money"]);
        assert_eq!(stamp.track_count, 0);
    }

    #[test]
    fn a_song_indexed_after_a_search_is_found_by_the_next() {
        let catalog = library();
        let searching = Searching::default();

        let before = searching.search(&catalog, "karma", 10).expect("a search");
        song(
            &catalog,
            Tagged {
                title: "Karma Police",
                artist: "Radiohead",
                ..UNTAGGED
            },
        );
        let after = searching.search(&catalog, "karma", 10).expect("a search");

        assert_eq!(before.track_count, 0);
        assert_eq!(titles(&after.tracks), vec!["Karma Police"]);
    }

    #[test]
    fn every_filter_order_and_direction_holds_and_the_album_filter_folds() {
        let catalog = library();
        let searching = Searching::default();

        for order in [
            TrackOrder::Title,
            TrackOrder::Album,
            TrackOrder::Artist,
            TrackOrder::Duration,
            TrackOrder::Added,
            TrackOrder::Plays,
        ] {
            for descending in [true, false] {
                let filter = TrackFilter {
                    text: "FLOYD PINK".into(),
                    album: "the dark side of the moon".into(),
                    order,
                    descending,
                    ..TrackFilter::default()
                };
                assert_eq!(
                    searching
                        .filtered(&catalog, &filter, 0, 100)
                        .expect("a filtered ordering")
                        .len(),
                    2
                );
            }
        }

        let everything = searching
            .filtered(&catalog, &TrackFilter::default(), 0, 100)
            .expect("every song");
        let paged = searching
            .filtered(&catalog, &TrackFilter::default(), 8, 100)
            .expect("the last page");
        let nothing_read = searching
            .filtered(
                &catalog,
                &TrackFilter {
                    text: "!!!".into(),
                    ..TrackFilter::default()
                },
                0,
                100,
            )
            .expect("a search reading nothing");

        assert_eq!(everything.len(), 9);
        assert_eq!(paged.len(), 1);
        assert!(nothing_read.is_empty());
    }

    #[test]
    fn a_two_word_search_over_twenty_thousand_songs_answers_within_its_budget() {
        let catalog = catalog();
        let searching = Searching::default();
        {
            let mut connection = catalog.0.lock();
            let transaction = connection.transaction().expect("a transaction");
            for at in 0..SYNTHETIC_ROWS {
                transaction
                    .execute(
                        "INSERT INTO documents (uri, name, title, artist, album, duration_ms, rate, codec, album_artist, genre, date) VALUES (?1, ?2, ?3, ?4, ?5, 240000, 44100, 'FLAC', ?6, ?7, ?8)",
                        params![
                            format!("content://synthetic/{at}.flac"),
                            format!("{at}.flac"),
                            format!("Song number {at} of the evening"),
                            format!("Performer {}", at % 997),
                            format!("Record {} volume {}", at % 1_499, at % 7),
                            format!("Performer {}", at % 499),
                            ["Rock", "Jazz", "Electronic", "Folk"][at % 4],
                            format!("{}", 1960 + at % 60),
                        ],
                    )
                    .expect("a synthetic song");
            }
            transaction.commit().expect("the synthetic library");
        }
        searching
            .search(&catalog, "warm", 50)
            .expect("the index is built");

        let mut fastest = Duration::MAX;
        let mut found = 0;
        for _ in 0..TIMED_RUNS {
            let began = Instant::now();
            found = searching
                .search(&catalog, "performer 12", 50)
                .expect("a timed search")
                .track_count;
            fastest = fastest.min(began.elapsed());
        }

        assert!(found > 0);
        assert!(
            fastest < SEARCH_BUDGET,
            "a two-word search took {fastest:?}"
        );
    }
}
