use std::{
    sync::LazyLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ahash::{AHashMap, AHashSet};
use resonate_codec::Likeness;
use resonate_core::{AlbumId, SampleFormat, TrackId};
use rusqlite::{OptionalExtension as _, params_from_iter, types::Value};

use super::{
    Inner,
    ordered::order_by,
    rows::{RawTrack, limit, rows},
    statements::{
        A_FAVOURITE_TRACK, A_HIDDEN_TRACK, A_SHOWN_TRACK, CD_SAMPLE_DEPTH, CD_SAMPLE_RATE,
        IDS_READ_AT_ONCE, INDEX_JOIN, INDEX_LOOKUP, LISTENS_SINCE, NOTHING_MATCHES, THE_BEST_COPY,
        TRACK_COLUMNS, artist_tracks, holds_a_best_copy, studied_as,
    },
};
use crate::{
    AlbumQuery, ArtistQuery, Asked, Clause, Codec, Column, Compare, Condition, Error, Measured,
    MusicFilters, Result, Search, Shape, SortOrder, StoreOp, Term, Track, TrackQuery, Verdict,
    Word, keyed, likeness,
    playlist::{self},
    search, store,
    studies::Agreement,
};

pub(super) struct Scoped {
    pub(super) from: String,
    pub(super) binds: Vec<Value>,
    pub(super) ranked: bool,
}

pub(crate) struct Narrowing {
    pub(crate) sql: String,
    pub(crate) binds: Vec<Value>,
}

#[derive(Default)]
pub(super) struct Matching {
    pub(super) ranked: bool,
    pub(super) insists_on_hidden: bool,
    pub(super) join: &'static str,
    pub(super) filters: Vec<String>,
    pub(super) binds: Vec<Value>,
}

impl Matching {
    fn narrows(&self) -> bool {
        !self.filters.is_empty()
    }

    pub(super) fn grouped(&self, column: &str, onto: &str) -> String {
        let mut filters = self.filters.clone();
        filters.push(format!("tracks.{column} IS NOT NULL"));

        format!(
            " JOIN (SELECT tracks.{column} AS id, {} AS score FROM tracks{}{} \
             GROUP BY tracks.{column}) matched ON matched.id = {onto}",
            if self.ranked {
                "min(tracks_fts.rank)"
            } else {
                NOTHING_MATCHES
            },
            self.join,
            clause(&filters)
        )
    }
}

pub(crate) fn tracks(
    inner: &Inner,
    query: &TrackQuery,
    narrowing: Option<&str>,
) -> Result<Vec<Track>> {
    let Some((sql, binds)) = listing(query, narrowing, *inner.music_filters.lock()) else {
        return Ok(Vec::new());
    };

    collect(inner, &sql, binds)
}

pub(crate) fn tracks_from(inner: &Inner, query: &TrackQuery, at: TrackId) -> Result<Vec<Track>> {
    let filters = *inner.music_filters.lock();
    let Some(matching) = matching_tracks(query, None, filters) else {
        return Ok(Vec::new());
    };
    if matching.ranked || !SortOrder::HELD_BY_AN_INDEX.contains(&query.sort) {
        return tracks(inner, query, None);
    }
    let order = order_by(query.sort, query.reading, false);
    let terms = keyed::terms(order);

    let raw = inner.read(|connection| {
        let reading = connection
            .unchecked_transaction()
            .map_err(|source| Error::store(StoreOp::Transaction, source))?;
        let key = reading
            .prepare_cached(&keyed::key_read(&terms))
            .and_then(|mut statement| {
                statement
                    .query_row([at.get() as i64], |row| {
                        (0..terms.len())
                            .map(|column| row.get::<_, Value>(column))
                            .collect::<rusqlite::Result<Vec<_>>>()
                    })
                    .optional()
            })
            .map_err(|source| Error::store(StoreOp::Query, source))?;
        let Some(key) = key else {
            return Ok(Vec::new());
        };

        let mut read = Vec::new();
        for seek in keyed::seeks(&terms, &key) {
            let wanted = match query.limit {
                Some(limit) => match limit.checked_sub(read.len()) {
                    Some(0) | None => break,
                    Some(left) => Some(left),
                },
                None => None,
            };
            let (sql, binds) = sought(&matching, seek, wanted);
            let mut statement = reading
                .prepare(&sql)
                .map_err(|source| Error::store(StoreOp::Prepare, source))?;
            let rows = statement
                .query_map(params_from_iter(binds), RawTrack::read)
                .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
                .map_err(|source| Error::store(StoreOp::Query, source))?;
            read.extend(rows);
        }
        Ok(read)
    })?;

    raw.into_iter().map(RawTrack::into_track).collect()
}

pub(crate) fn reads_listening(query: &TrackQuery) -> bool {
    let ordered_by_listening = matches!(
        query.sort,
        SortOrder::Plays | SortOrder::Played | SortOrder::Favourited | SortOrder::PlaysThisMonth
    );
    query.text.is_some() || (ordered_by_listening && (query.limit.is_some() || query.offset > 0))
}

pub(super) fn sought(
    matching: &Matching,
    seek: keyed::Seek,
    wanted: Option<usize>,
) -> (String, Vec<Value>) {
    let mut conditions = matching.filters.clone();
    conditions.extend(seek.conditions);
    let mut binds = matching.binds.clone();
    binds.extend(seek.binds);
    binds.push(Value::Integer(limit(wanted)));

    (
        format!(
            "SELECT {TRACK_COLUMNS} FROM tracks{}{} ORDER BY {} LIMIT ?",
            matching.join,
            clause(&conditions),
            seek.order
        ),
        binds,
    )
}

pub(super) fn listing(
    query: &TrackQuery,
    narrowing: Option<&str>,
    filters: MusicFilters,
) -> Option<(String, Vec<Value>)> {
    let scoped = scoped(query, narrowing, filters)?;
    let sql = format!(
        "SELECT {TRACK_COLUMNS}{} ORDER BY {} LIMIT ? OFFSET ?",
        scoped.from,
        order_by(query.sort, query.reading, scoped.ranked)
    );

    Some((sql, paged(scoped.binds, query)))
}

pub(crate) fn measured(inner: &Inner, query: &TrackQuery) -> Result<Measured> {
    measured_narrowed(inner, query, None)
}

pub(crate) fn measured_narrowed(
    inner: &Inner,
    query: &TrackQuery,
    narrowing: Option<&str>,
) -> Result<Measured> {
    let stamp = inner.rows_stamp();
    let filters = *inner.music_filters.lock();
    let Some(scoped) = scoped(query, narrowing, filters) else {
        return Ok(Measured::default());
    };
    let whole = query.limit.is_none() && query.offset == 0;
    let (sql, binds) = match whole {
        true => (
            format!(
                "SELECT count(*), sum(tracks.duration * 1.0 / tracks.sample_rate),
                        sum(tracks.codec IN ({})){}",
                lossless_codes(),
                scoped.from,
            ),
            scoped.binds,
        ),
        false => (
            format!(
                "SELECT count(*), sum(seconds), sum(lossless) FROM
                 (SELECT tracks.duration * 1.0 / tracks.sample_rate AS seconds,
                         tracks.codec IN ({}) AS lossless{}
                  ORDER BY {} LIMIT ? OFFSET ?)",
                lossless_codes(),
                scoped.from,
                order_by(query.sort, query.reading, scoped.ranked)
            ),
            paged(scoped.binds, query),
        ),
    };
    let keepable = narrowing.is_none();
    let key = match reads_listening(query) {
        true => format!("{sql}\u{0}{binds:?}\u{0}{}", inner.heard()),
        false => format!("{sql}\u{0}{binds:?}"),
    };
    if keepable && let Some(kept) = inner.measurement_kept(stamp, filters, &key) {
        return Ok(kept);
    }

    let measured = inner.read(|connection| {
        connection
            .query_row(&sql, params_from_iter(binds), |row| {
                Ok(Measured {
                    rows: row.get::<_, i64>(0)? as u32,
                    length: played(row.get::<_, Option<f64>>(1)?),
                    lossless: row.get::<_, Option<i64>>(2)?.unwrap_or_default() as u32,
                })
            })
            .map_err(|source| Error::store(StoreOp::Query, source))
    })?;
    if keepable {
        inner.keep_measurement(stamp, filters, key, measured);
    }
    Ok(measured)
}

pub(super) const COVERED_ALBUM: &str = "(tracks.album_id IN (SELECT id FROM albums
       WHERE cover_print IS NOT NULL OR cover_path IS NOT NULL))";

pub(super) const PICTURE_OF_THE_ALBUM: &str = concat!(
    "(SELECT ",
    store::the_picture_of!("a"),
    " FROM albums a WHERE a.id = tracks.album_id)"
);

pub(crate) const PICTURES_WEIGHED_PER_TILE: usize = 4;

pub(crate) fn pictured_by(
    inner: &Inner,
    query: &TrackQuery,
    at_most: usize,
) -> Result<Vec<AlbumId>> {
    let Some(scoped) = scoped(query, None, *inner.music_filters.lock()) else {
        return Ok(Vec::new());
    };
    let covered = if scoped.from.contains(" WHERE ") {
        format!("{} AND {COVERED_ALBUM}", scoped.from)
    } else {
        format!("{} WHERE {COVERED_ALBUM}", scoped.from)
    };
    let sql = format!(
        "SELECT tracks.album_id, {PICTURE_OF_THE_ALBUM}{covered}
          GROUP BY tracks.album_id
          ORDER BY count(*) DESC, min(tracks.id)
          LIMIT ?"
    );
    let mut binds = scoped.binds;
    binds.push(Value::Integer(
        at_most.saturating_mul(PICTURES_WEIGHED_PER_TILE) as i64,
    ));

    let weighed = inner.read(|connection| {
        rows(connection, &sql, binds, |row| {
            Ok(Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
            )))
        })
    })?;
    distinct_pictures(inner, weighed, at_most)
}

pub(crate) fn pictures_of_albums(
    inner: &Inner,
    albums: &[AlbumId],
) -> Result<Vec<(i64, Option<String>)>> {
    if albums.is_empty() {
        return Ok(Vec::new());
    }
    let held: AHashMap<i64, Option<String>> = inner.read(|connection| {
        let mut held = AHashMap::with_capacity(albums.len());
        for batch in albums.chunks(IDS_READ_AT_ONCE) {
            let sql = format!(
                "SELECT a.id, {} FROM albums a
                  WHERE (a.cover_print IS NOT NULL OR a.cover_path IS NOT NULL) AND a.id IN ({})",
                store::the_picture_of!("a"),
                vec!["?"; batch.len()].join(", ")
            );
            let binds: Vec<Value> = batch
                .iter()
                .map(|album| Value::Integer(album.get() as i64))
                .collect();
            held.extend(rows(connection, &sql, binds, |row| {
                Ok(Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<String>>(1)?,
                )))
            })?);
        }
        Ok(held)
    })?;

    Ok(albums
        .iter()
        .filter_map(|album| {
            let id = album.get() as i64;
            held.get(&id).map(|picture| (id, picture.clone()))
        })
        .collect())
}

pub(crate) fn distinct_pictures(
    inner: &Inner,
    weighed: Vec<(i64, Option<String>)>,
    at_most: usize,
) -> Result<Vec<AlbumId>> {
    let mut seen = AHashSet::new();
    let mut looked_like: Vec<Likeness> = Vec::with_capacity(at_most);
    let mut pictured = Vec::with_capacity(at_most);
    for (album, picture) in weighed {
        let album = AlbumId::new(album as u64)?;
        if let Some(picture) = picture {
            if !seen.insert(picture.clone()) {
                continue;
            }
            if let Some(likeness) = likeness::of(inner, album, &picture)? {
                if looked_like.iter().any(|held| held.resembles(&likeness)) {
                    continue;
                }
                looked_like.push(likeness);
            }
        }
        pictured.push(album);
        if pictured.len() == at_most {
            break;
        }
    }

    Ok(pictured)
}

fn lossless_codes() -> String {
    Codec::ALL
        .iter()
        .filter(|codec| codec.is_lossless())
        .map(|codec| store::codec_code(*codec).to_string())
        .collect::<Vec<String>>()
        .join(",")
}

pub(super) fn scoped_missing(narrowing: Option<&str>) -> Option<Scoped> {
    narrowed_onto(narrowing, "album_id", "a.id", MusicFilters::default())
}

pub(super) struct Infix {
    pub(super) column: &'static str,
    pub(super) owner: &'static str,
    pub(super) index: &'static str,
    pub(super) indexed_column: &'static str,
}

impl Infix {
    pub(super) const FEWEST_LETTERS_INDEXED: usize = 3;

    pub(super) const OF_A_MISSING_TRACK: Infix = Infix {
        column: "rt.folded",
        owner: "rt.id",
        index: "release_tracks_fts",
        indexed_column: "folded",
    };

    pub(super) const OF_A_RELEASE: Infix = Infix {
        column: "r.folded",
        owner: "r.rowid",
        index: "artist_releases_fts",
        indexed_column: "folded",
    };

    pub(super) const OF_AN_ARTIST: Infix = Infix {
        column: "ar.key",
        owner: "r.artist_id",
        index: "artists_key_fts",
        indexed_column: "key",
    };

    pub(super) fn holding(&self, folded: &str, binds: &mut Vec<Value>) -> String {
        binds.push(Value::Text(playlist::anywhere(folded)));
        if folded.chars().count() < Self::FEWEST_LETTERS_INDEXED {
            return format!("{} LIKE ? ESCAPE '\\'", self.column);
        }

        format!(
            "{} IN (SELECT rowid FROM {} WHERE {} LIKE ? ESCAPE '\\')",
            self.owner, self.index, self.indexed_column
        )
    }

    pub(super) fn held_by_a_release_or_its_artist(folded: &str, binds: &mut Vec<Value>) -> String {
        let release = Self::OF_A_RELEASE.holding(folded, binds);
        let artist = Self::OF_AN_ARTIST.holding(folded, binds);
        format!("({release} OR {artist})")
    }
}

pub(super) fn unheld_holding(narrowing: Option<&str>) -> (String, Vec<Value>) {
    let mut held = String::new();
    let mut binds = Vec::new();
    for word in playlist::words_of(narrowing) {
        held.push_str(" AND ");
        held.push_str(&Infix::held_by_a_release_or_its_artist(
            &store::folded_letters(&word),
            &mut binds,
        ));
    }

    (held, binds)
}

pub(super) fn scoped_albums(
    query: &AlbumQuery,
    only: Option<&str>,
    filters: MusicFilters,
) -> Option<Scoped> {
    let mut scoped = narrowed_onto(query.text.as_deref(), "album_id", "a.id", filters)?;
    let mut conditions = vec![holds_a_best_copy(filters)];

    if let Some(artist) = query.artist {
        let owned_or_played_on = artist.get() as i64;
        conditions.push(BY_OR_HOLDING_THE_ARTIST.to_owned());
        scoped.binds.push(Value::Integer(owned_or_played_on));
        scoped.binds.push(Value::Integer(owned_or_played_on));
        scoped.binds.push(Value::Integer(owned_or_played_on));
    }
    conditions.extend(only.map(str::to_owned));
    scoped.from.push_str(&clause(&conditions));

    Some(scoped)
}

pub(super) const BY_OR_HOLDING_THE_ARTIST: &str = "(a.artist_id = ?
      OR EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = a.id AND t.artist_id = ?)
      OR EXISTS (SELECT 1 FROM tracks t JOIN track_credits c ON c.track_id = t.id
                  WHERE t.album_id = a.id AND c.artist_id = ?))";

pub(super) fn scoped_artists(
    query: &ArtistQuery,
    only: Option<&str>,
    filters: MusicFilters,
) -> Option<Scoped> {
    let mut scoped = narrowed_onto(query.text.as_deref(), "artist_id", "r.id", filters)?;
    let mut conditions: Vec<String> = only.map(str::to_owned).into_iter().collect();
    if filters != MusicFilters::default() {
        conditions.push(format!("{} > 0", artist_tracks(filters)));
    }
    scoped.from.push_str(&clause(&conditions));

    Some(scoped)
}

fn narrowed_onto(
    text: Option<&str>,
    column: &str,
    onto: &str,
    filters: MusicFilters,
) -> Option<Scoped> {
    let mut matching = matching(&[text])?;
    let ranked = matching.ranked;
    if !matching.narrows() {
        return Some(Scoped {
            from: String::new(),
            binds: Vec::new(),
            ranked,
        });
    }

    matching.filters.extend(filters.predicate("tracks"));
    matching.filters.push(THE_BEST_COPY.to_owned());
    if !matching.insists_on_hidden {
        matching.filters.push(A_SHOWN_TRACK.to_owned());
    }

    Some(Scoped {
        from: matching.grouped(column, onto),
        binds: matching.binds,
        ranked,
    })
}

pub(super) fn scoped(
    query: &TrackQuery,
    narrowing: Option<&str>,
    filters: MusicFilters,
) -> Option<Scoped> {
    let matching = matching_tracks(query, narrowing, filters)?;

    Some(Scoped {
        from: format!(" FROM tracks{}{}", matching.join, clause(&matching.filters)),
        binds: matching.binds,
        ranked: matching.ranked,
    })
}

pub(super) fn matching_tracks(
    query: &TrackQuery,
    narrowing: Option<&str>,
    filters: MusicFilters,
) -> Option<Matching> {
    if narrowing.is_some_and(asks_for_nothing_it_can_read) {
        return None;
    }
    let mut matching = matching(&[query.text.as_deref(), narrowing])?;
    matching.filters.extend(filters.predicate("tracks"));
    matching.filters.push(THE_BEST_COPY.to_owned());
    if !matching.insists_on_hidden {
        matching.filters.push(A_SHOWN_TRACK.to_owned());
    }

    if let Some(album) = query.album {
        matching.filters.push("tracks.album_id = ?".to_owned());
        matching.binds.push(Value::Integer(album.get() as i64));
    }
    if let Some(artist) = query.artist {
        matching.filters.push(
            "(tracks.artist_id = ?
              OR tracks.id IN (SELECT track_id FROM track_credits WHERE artist_id = ?))"
                .to_owned(),
        );
        matching.binds.push(Value::Integer(artist.get() as i64));
        matching.binds.push(Value::Integer(artist.get() as i64));
    }

    Some(matching)
}

pub(super) fn matching(texts: &[Option<&str>]) -> Option<Matching> {
    let asking: Vec<Search> = texts
        .iter()
        .flatten()
        .map(|text| Search::read(text))
        .filter(|search| !search.is_empty())
        .collect();
    if asking.is_empty() {
        return Some(Matching::default());
    }

    let mut ranked: Vec<&Word> = Vec::new();
    let mut narrowing: Vec<Narrowing> = Vec::new();
    for clause in asking.iter().flat_map(|search| &search.clauses) {
        match clause.lone_word() {
            Some(word) if word.reach != search::Reach::Whole => ranked.push(word),
            _ => narrowing.extend(any_of(clause)),
        }
    }

    let indexed = indexed(&ranked);
    if indexed.is_none() && narrowing.is_empty() {
        return None;
    }

    let mut matching = Matching {
        insists_on_hidden: asking
            .iter()
            .flat_map(|search| &search.clauses)
            .any(|clause| clause.insists_on(Shape::Hidden)),
        ..Matching::default()
    };
    if let Some(query) = indexed {
        matching.ranked = true;
        matching.join = INDEX_JOIN;
        matching.filters.push("tracks_fts MATCH ?".to_owned());
        matching.binds.push(Value::Text(query));
    }
    for narrowed in narrowing {
        matching.filters.push(narrowed.sql);
        matching.binds.extend(narrowed.binds);
    }

    Some(matching)
}

pub(crate) enum Narrowed {
    Unasked,
    To(Narrowing),
    Nothing,
}

pub(crate) fn asks_for_nothing_it_can_read(text: &str) -> bool {
    !text.trim().is_empty() && Search::read(text).is_empty()
}

pub(crate) fn cuts_matching(text: &str, row: &str) -> Narrowed {
    if asks_for_nothing_it_can_read(text) {
        return Narrowed::Nothing;
    }
    let Some(matching) = matching(&[Some(text)]) else {
        return Narrowed::Nothing;
    };
    if !matching.narrows() {
        return Narrowed::Unasked;
    }

    Narrowed::To(Narrowing {
        sql: format!(
            "({row}.path, {row}.span_start) IN \
             (SELECT tracks.path, tracks.span_start FROM tracks{}{})",
            matching.join,
            clause(&matching.filters)
        ),
        binds: matching.binds,
    })
}

fn any_of(clause: &Clause) -> Option<Narrowing> {
    let mut held = Vec::with_capacity(clause.any.len());
    let mut binds = Vec::new();
    for asked in &clause.any {
        let Some(narrowed) = all_of(asked) else {
            continue;
        };
        held.push(narrowed.sql);
        binds.extend(narrowed.binds);
    }

    joined(held, " OR ").map(|sql| Narrowing { sql, binds })
}

fn all_of(asked: &Asked) -> Option<Narrowing> {
    let mut held = Vec::with_capacity(asked.all.len());
    let mut binds = Vec::new();
    for condition in &asked.all {
        let narrowed = narrowed(condition)?;
        held.push(narrowed.sql);
        binds.extend(narrowed.binds);
    }

    let held = joined(held, " AND ")?;
    let sql = if asked.denied {
        format!("NOT coalesce({held}, 0)")
    } else {
        held
    };

    Some(Narrowing { sql, binds })
}

pub(super) fn narrowed(condition: &Condition) -> Option<Narrowing> {
    match condition {
        Condition::Word(word) if word.reach == search::Reach::Whole => named(word),
        Condition::Word(word) => Some(Narrowing {
            sql: INDEX_LOOKUP.to_owned(),
            binds: vec![Value::Text(indexed(&[word])?)],
        }),
        Condition::Term(term) => {
            let mut binds = Vec::new();
            let sql = filtered(*term, &mut binds);
            Some(Narrowing { sql, binds })
        }
    }
}

pub(super) fn named(word: &Word) -> Option<Narrowing> {
    let looked_up = Value::Text(indexed(&[word])?);
    let words = Value::Text(store::words_of(&word.text));

    let (sql, binds) = match word.column {
        Some(Column::Title) => (
            format!("({INDEX_LOOKUP} AND {TITLED_AS_A_WHOLE})"),
            vec![looked_up, words],
        ),
        Some(Column::Genre) => (
            format!("({INDEX_LOOKUP} AND {FILED_UNDER_A_WHOLE_GENRE})"),
            vec![looked_up, words.clone(), words],
        ),
        Some(Column::Artist) => (
            BY_AN_ARTIST_NAMED_AS_A_WHOLE.to_owned(),
            vec![words.clone(), words],
        ),
        Some(Column::Album) => (
            ON_AN_ALBUM_NAMED_AS_A_WHOLE.to_owned(),
            vec![words.clone(), words],
        ),
        Some(Column::Lyrics) | None => (INDEX_LOOKUP.to_owned(), vec![looked_up]),
    };

    Some(Narrowing { sql, binds })
}

pub(super) const TITLED_AS_A_WHOLE: &str = "tracks.title_words = ?";

pub(super) const FILED_UNDER_A_WHOLE_GENRE: &str = "(lists_words(tracks.genre, ?)
      OR tracks.artist_id IN (SELECT artist_id FROM artist_genres WHERE words_of(name) = ?))";

pub(super) const BY_AN_ARTIST_NAMED_AS_A_WHOLE: &str = "(tracks.artist_id IN
         (SELECT id FROM artists WHERE words_of(name) = ?)
      OR tracks.id IN
         (SELECT c.track_id FROM track_credits c JOIN artists r ON r.id = c.artist_id
           WHERE words_of(r.name) = ?))";

pub(super) const ON_AN_ALBUM_NAMED_AS_A_WHOLE: &str = concat!(
    "tracks.album_id IN (SELECT a.id FROM albums a WHERE words_of(",
    album_title!(),
    ") = ? OR words_of(a.title) = ?)"
);

pub(super) fn joined(mut held: Vec<String>, by: &str) -> Option<String> {
    match held.len() {
        0 => None,
        1 => Some(held.remove(0)),
        _ => Some(format!("({})", held.join(by))),
    }
}

pub(super) fn indexed(words: &[&Word]) -> Option<String> {
    let mut matched = Vec::with_capacity(words.len());

    for word in words {
        let pieces = search::pieces_of(word);
        if pieces.is_empty() {
            continue;
        }

        let scoped = match word.column {
            Some(column) => format!("{} : ", column.name()),
            None => format!("{{{}}} : ", *WHAT_A_BARE_WORD_REACHES),
        };
        if word.reach.is_phrased() {
            matched.push(format!("{scoped}\"{}\"", pieces.join(" ")));
            continue;
        }
        for piece in pieces {
            matched.push(format!("{scoped}\"{piece}\"*"));
        }
    }

    (!matched.is_empty()).then(|| matched.join(" "))
}

pub(super) static WHAT_A_BARE_WORD_REACHES: LazyLock<String> = LazyLock::new(|| {
    Column::NAMES
        .iter()
        .map(|column| column.name())
        .collect::<Vec<_>>()
        .join(" ")
});

fn filtered(term: Term, binds: &mut Vec<Value>) -> String {
    match term {
        Term::Added { compare, age } => {
            binds.push(Value::Integer(store::to_nanos(
                SystemTime::now().checked_sub(age).unwrap_or(UNIX_EPOCH),
            )));
            format!("tracks.added {} ?", compare.flipped().operator())
        }
        Term::Plays {
            compare,
            plays,
            within,
        } => {
            let counted = match within {
                Some(age) => {
                    binds.push(Value::Integer(store::to_nanos(
                        SystemTime::now().checked_sub(age).unwrap_or(UNIX_EPOCH),
                    )));
                    LISTENS_SINCE
                }
                None => "tracks.plays",
            };
            binds.push(Value::Integer(i64::from(plays)));
            format!("{counted} {} ?", compare.operator())
        }
        Term::Played { compare, age } => {
            binds.push(Value::Integer(store::to_nanos(
                SystemTime::now().checked_sub(age).unwrap_or(UNIX_EPOCH),
            )));
            format!(
                "(tracks.played IS NOT NULL AND tracks.played {} ?)",
                compare.flipped().operator()
            )
        }
        Term::Year { compare, year } => {
            binds.push(Value::Integer(i64::from(year)));
            format!(
                "tracks.album_id IN \
                 (SELECT id FROM albums WHERE year IS NOT NULL AND year {} ?)",
                compare.operator()
            )
        }
        Term::Length {
            compare,
            length,
            grain,
        } => lengths(compare, length, grain, binds),
        Term::Rate { compare, hertz } => {
            binds.push(Value::Integer(i64::from(hertz)));
            format!("tracks.sample_rate {} ?", compare.operator())
        }
        Term::Depth { compare, bits } => depths(compare, bits),
        Term::Codec(codec) => {
            binds.push(Value::Integer(store::codec_code(codec)));
            "tracks.codec = ?".to_owned()
        }
        Term::Shape(shape) => shaped(shape),
    }
}

fn lengths(
    compare: Compare,
    length: Duration,
    grain: search::Grain,
    binds: &mut Vec<Value>,
) -> String {
    let from = length.as_secs_f64();
    let past = from + grain.span().as_secs_f64();
    let bounds: &[(&str, f64)] = match compare {
        Compare::Below => &[("<", from)],
        Compare::AtMost => &[("<", past)],
        Compare::Exactly => &[(">=", from), ("<", past)],
        Compare::AtLeast => &[(">=", from)],
        Compare::Above => &[(">=", past)],
    };

    let mut sql = "(tracks.duration IS NOT NULL".to_owned();
    for (operator, bound) in bounds {
        sql.push_str(&format!(" AND {SECONDS_LONG} {operator} ?"));
        binds.push(Value::Real(*bound));
    }
    sql.push(')');
    sql
}

pub(super) const SECONDS_LONG: &str = "tracks.duration * 1.0 / tracks.sample_rate";

fn shaped(shape: Shape) -> String {
    match shape {
        Shape::Lossless => codecs(Codec::is_lossless),
        Shape::Lossy => codecs(|codec| !codec.is_lossless() && codec != Codec::Unknown),
        Shape::Mono => "tracks.channels = 1".to_owned(),
        Shape::Stereo => "tracks.channels = 2".to_owned(),
        Shape::Multichannel => "tracks.channels > 2".to_owned(),
        Shape::HiRes => format!(
            "({} AND (tracks.sample_rate > {CD_SAMPLE_RATE} OR {}))",
            codecs(Codec::is_lossless),
            depths(Compare::Above, CD_SAMPLE_DEPTH)
        ),
        Shape::Favourite => A_FAVOURITE_TRACK.to_owned(),
        Shape::Hidden => A_HIDDEN_TRACK.to_owned(),
        Shape::Fake => studied_as("verdict", Verdict::Fake.as_str()),
        Shape::Suspect => studied_as("verdict", Verdict::Suspect.as_str()),
        Shape::Misnamed => studied_as("agreement", Agreement::Disagrees.as_str()),
    }
}

fn codecs(wanted: impl Fn(Codec) -> bool) -> String {
    coded(
        "tracks.codec",
        Codec::ALL
            .into_iter()
            .filter(|codec| wanted(*codec))
            .map(store::codec_code),
    )
}

fn depths(compare: Compare, bits: u8) -> String {
    coded(
        "tracks.sample_format",
        SampleFormat::ALL
            .into_iter()
            .filter(|format| holds(compare, format.valid_bits(), bits))
            .map(store::format_code),
    )
}

pub(super) const fn holds(compare: Compare, held: u8, wanted: u8) -> bool {
    match compare {
        Compare::Below => held < wanted,
        Compare::AtMost => held <= wanted,
        Compare::Exactly => held == wanted,
        Compare::AtLeast => held >= wanted,
        Compare::Above => held > wanted,
    }
}

pub(super) fn coded(column: &str, codes: impl Iterator<Item = i64>) -> String {
    let listed: Vec<String> = codes.map(|code| code.to_string()).collect();
    if listed.is_empty() {
        return NOTHING_MATCHES.to_owned();
    }

    format!("{column} IN ({})", listed.join(", "))
}

pub(crate) fn clause(filters: &[String]) -> String {
    if filters.is_empty() {
        return String::new();
    }

    format!(" WHERE {}", filters.join(" AND "))
}

pub(super) fn paged(mut binds: Vec<Value>, query: &TrackQuery) -> Vec<Value> {
    binds.push(Value::Integer(limit(query.limit)));
    binds.push(Value::Integer(query.offset as i64));
    binds
}

pub(super) fn played(seconds: Option<f64>) -> Option<Duration> {
    seconds
        .filter(|seconds| *seconds > 0.0)
        .map(|seconds| Duration::try_from_secs_f64(seconds).unwrap_or_default())
}

pub(super) fn collect(inner: &Inner, sql: &str, binds: Vec<Value>) -> Result<Vec<Track>> {
    let raw = inner.read(|connection| {
        let mut statement = connection
            .prepare(sql)
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;
        statement
            .query_map(params_from_iter(binds), RawTrack::read)
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))
    })?;

    raw.into_iter().map(RawTrack::into_track).collect()
}
