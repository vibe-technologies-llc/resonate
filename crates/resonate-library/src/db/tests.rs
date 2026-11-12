use std::{
    env, fs,
    num::NonZeroUsize,
    process,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::UNIX_EPOCH,
};

use ahash::AHashSet;
use resonate_core::{ArtistId, SampleFormat, TrackId};
use rusqlite::params_from_iter;

use super::{
    ordered::{album_order_by, artist_order_by, order_by},
    searched::{listing, matching_tracks, scoped, sought},
    statements::{ALBUMS_NOT_HELD_BY_AN_ARTIST, MISSING_FROM_AN_ARTISTS_ALBUMS},
    *,
};
use crate::{
    AlbumOrder, AlbumQuery, ArtistOrder, ArtistQuery, Codec, Direction, OrderedColumn,
    OrganiseOptions, ScanOptions, SchemaFingerprint, SortOrder, keyed,
};

const A_TEMPORARY_SORT: &str = "TEMP B-TREE";

const READERS_AT_ONCE: usize = READER_POOL * 4;

const A_MINUTE_AGO: Duration = Duration::from_secs(60);

const A_YEAR_AGO: Duration = Duration::from_secs(365 * 24 * 60 * 60);

#[test]
fn a_catalog_another_schema_wrote_is_refused_at_the_door_rather_than_on_a_query() {
    let scratch = Scratch::new("another-schema");
    let database = scratch.path.join("library.db");
    drop(Library::open(&database).expect("a catalog opens on disk"));

    let elsewhere = SchemaFingerprint(schema::SCHEMA_FINGERPRINT.0.wrapping_add(1));
    let connection = Connection::open(&database).expect("the catalog opens");
    connection
        .pragma_update(None, "user_version", elsewhere.0.cast_signed())
        .expect("another build's stamp is written");
    drop(connection);

    assert!(
        matches!(
            Library::open(&database),
            Err(Error::SchemaMismatch { found, expected })
                if found == elsewhere && expected == schema::SCHEMA_FINGERPRINT
        ),
        "a catalog written to another schema opened rather than being refused"
    );
}

#[test]
fn a_saved_query_measured_twice_is_counted_once_until_the_catalog_moves() {
    let library = Library::open_in_memory().expect("an in-memory catalog opens");
    let saved = SavedQuery::default();

    library
        .inner
        .measured_saved(&saved)
        .expect("an empty catalog is measured");
    let first = library
        .inner
        .measured
        .lock()
        .as_ref()
        .map(|kept| kept.kept_at);
    library
        .inner
        .measured_saved(&saved)
        .expect("the same query is measured again");
    let second = library
        .inner
        .measured
        .lock()
        .as_ref()
        .map(|kept| kept.kept_at);

    assert!(first.is_some());
    assert_eq!(
        first, second,
        "the second measure counted the catalog again"
    );

    library.filter_music(MusicFilters::default());
    assert!(
        library.inner.measured.lock().is_none(),
        "a change of the music filters kept the counts made under the old ones"
    );
}

#[test]
fn a_listing_measured_twice_is_counted_once_until_a_row_of_the_catalog_moves() -> Result<()> {
    let library = Library::open_in_memory()?;
    titled(&library, "First");
    let query = TrackQuery::default();

    assert_eq!(library.measured(&query)?.rows, 1);
    let first = library
        .inner
        .measured
        .lock()
        .as_ref()
        .map(|kept| kept.kept_at);
    assert_eq!(library.measured(&query)?.rows, 1);
    assert_eq!(
        library
            .inner
            .measured
            .lock()
            .as_ref()
            .map(|kept| kept.kept_at),
        first,
        "the second measure counted the catalog again"
    );

    titled(&library, "Second");
    assert_eq!(
        library.measured(&query)?.rows,
        2,
        "a track written under a kept count was not counted"
    );
    Ok(())
}

#[test]
fn a_playlists_pictures_are_read_once_until_the_catalog_or_the_playlists_move() -> Result<()> {
    let library = Library::open_in_memory()?;
    let playlist = library.create_playlist("Mine")?;

    assert!(library.playlist_pictures(playlist, 4)?.is_empty());
    assert!(
        library
            .inner
            .pictures_kept(library.inner.pictures_at(), playlist, 4)
            .is_some(),
        "the pictures read were not kept"
    );

    library.inner.playlists_changed();
    assert!(
        library
            .inner
            .pictures_kept(library.inner.pictures_at(), playlist, 4)
            .is_none(),
        "a change of the playlists kept the pictures read before it"
    );

    assert!(library.playlist_pictures(playlist, 4)?.is_empty());
    titled(&library, "First");
    assert!(
        library
            .inner
            .pictures_kept(library.inner.pictures_at(), playlist, 4)
            .is_none(),
        "a write to the tracks kept the pictures read before it"
    );

    library.filter_music(MusicFilters::default());
    assert!(library.inner.pictured.lock().is_none());
    Ok(())
}

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(named: &str) -> Self {
        let path = env::temp_dir().join(format!("resonate-db-{}-{named}", process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("a writable temporary directory");
        Self { path }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn plan(library: &Library, query: &TrackQuery) -> Vec<String> {
    let (sql, binds) = listing(query, None, MusicFilters::default())
        .expect("an unnarrowed listing is always scoped");

    library
        .inner
        .read(|connection| {
            let mut statement = connection
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .map_err(|source| Error::store(StoreOp::Prepare, source))?;
            statement
                .query_map(params_from_iter(binds), |row| row.get::<_, String>(3))
                .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
                .map_err(|source| Error::store(StoreOp::Query, source))
        })
        .expect("the planner answers for a listing it can prepare")
}

#[test]
fn a_tracks_title_words_are_kept_as_it_is_named_and_a_held_song_is_found_by_them() {
    let library = Library::open_in_memory().expect("an in-memory catalog opens");
    let words = |library: &Library| -> Option<String> {
        library
            .inner
            .read(|connection| {
                connection
                    .query_row("SELECT title_words FROM tracks", [], |row| row.get(0))
                    .map_err(|source| Error::store(StoreOp::Query, source))
            })
            .expect("the track reads back")
    };

    library
        .inner
        .write(|transaction| {
            transaction
                .execute_batch(
                    "INSERT INTO artists (id, key, name) VALUES (1, 'ada', 'Ada');
                     INSERT INTO tracks (root_id, path, title, artist, artist_id, sample_rate,
                                         channels, sample_format, codec, file_size, modified,
                                         added, seen)
                     VALUES (NULL, '/a.flac', 'Wish You Were Here!', 'Ada', 1, 44100, 2, 1, 1,
                             1, 1, 1, 1);",
                )
                .map_err(|source| Error::store(StoreOp::Insert, source))
        })
        .expect("a track is written");
    assert_eq!(words(&library).as_deref(), Some("wish you were here"));

    library
        .inner
        .write(|transaction| {
            transaction
                .execute("UPDATE tracks SET title = 'Hey You'", [])
                .map(drop)
                .map_err(|source| Error::store(StoreOp::Update, source))
        })
        .expect("the track is renamed");
    assert_eq!(words(&library).as_deref(), Some("hey you"));

    for sql in [MISSING_FROM_AN_ARTISTS_ALBUMS, ALBUMS_NOT_HELD_BY_AN_ARTIST] {
        let steps: Vec<String> = library
            .inner
            .read(|connection| {
                let mut statement = connection
                    .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                    .map_err(|source| Error::store(StoreOp::Prepare, source))?;
                statement
                    .query_map([1_i64], |row| row.get::<_, String>(3))
                    .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
                    .map_err(|source| Error::store(StoreOp::Query, source))
            })
            .expect("the planner answers");
        assert!(
            steps
                .iter()
                .any(|step| step.contains("tracks_by_artist_and_words")),
            "a held song is looked for title by title: {steps:?}"
        );
    }
}

#[test]
fn every_order_the_panes_offer_is_read_off_an_index_either_way_round() {
    let library = Library::open_in_memory().expect("an in-memory catalog opens");

    for sort in SortOrder::HELD_BY_AN_INDEX {
        for reading in Direction::ALL {
            let steps = plan(
                &library,
                &TrackQuery {
                    sort,
                    reading,
                    ..TrackQuery::default()
                },
            );
            assert!(
                !steps.iter().any(|step| step.contains(A_TEMPORARY_SORT)),
                "{sort:?} read {reading:?} sorts the whole table before the limit lands: \
                 {steps:?}"
            );
        }
    }
}

fn explained(library: &Library, sql: &str, binds: Vec<Value>) -> Vec<String> {
    library
        .inner
        .read(|connection| {
            let mut statement = connection
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .map_err(|source| Error::store(StoreOp::Prepare, source))?;
            statement
                .query_map(params_from_iter(binds), |row| row.get::<_, String>(3))
                .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
                .map_err(|source| Error::store(StoreOp::Query, source))
        })
        .expect("the planner answers for a seek it can prepare")
}

fn keys_held(library: &Library, terms: &[keyed::Term<'_>]) -> Vec<Vec<Value>> {
    library
        .inner
        .read(|connection| {
            let mut statement = connection
                .prepare(&keyed::key_read(terms).replace("WHERE tracks.id = ?", ""))
                .map_err(|source| Error::store(StoreOp::Prepare, source))?;
            statement
                .query_map([], |row| {
                    (0..terms.len())
                        .map(|column| row.get::<_, Value>(column))
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
                .map_err(|source| Error::store(StoreOp::Query, source))
        })
        .expect("every row's key is read")
}

fn never_null(library: &Library) -> Vec<String> {
    library
        .inner
        .read(|connection| {
            let mut statement = connection
                .prepare("SELECT name FROM pragma_table_xinfo('tracks') WHERE \"notnull\"")
                .map_err(|source| Error::store(StoreOp::Prepare, source))?;
            statement
                .query_map([], |row| row.get::<_, String>(0))
                .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
                .map_err(|source| Error::store(StoreOp::Query, source))
        })
        .expect("the columns are read")
        .into_iter()
        .map(|column| format!("(tracks.{column}) IS NULL"))
        .collect()
}

#[test]
fn every_seek_past_a_key_is_read_off_the_orders_index_either_way_round() {
    let library = Library::open_in_memory().expect("an in-memory catalog opens");
    tied_and_unheld(&library);
    let answered_before_reading = never_null(&library);

    for sort in SortOrder::HELD_BY_AN_INDEX {
        for reading in Direction::ALL {
            let query = TrackQuery {
                sort,
                reading,
                ..TrackQuery::default()
            };
            let matching = matching_tracks(&query, None, MusicFilters::default())
                .expect("an unnarrowed listing is always matched");
            let order = order_by(sort, reading, false);
            let terms = keyed::terms(order);
            for key in keys_held(&library, &terms) {
                for seek in keyed::seeks(&terms, &key) {
                    if seek
                        .conditions
                        .iter()
                        .any(|condition| answered_before_reading.contains(condition))
                    {
                        continue;
                    }
                    let said = format!("{sort:?} read {reading:?} past {:?}", seek.conditions);
                    let (sql, binds) = sought(&matching, seek, Some(PAGED_BY));
                    let steps = explained(&library, &sql, binds);
                    assert!(
                        !steps.iter().any(|step| step.contains(A_TEMPORARY_SORT)),
                        "{said} sorts before the limit lands: {steps:?}"
                    );
                    assert!(
                        steps.iter().any(|step| step.contains("USING INDEX")
                            || step.contains("USING INTEGER PRIMARY KEY")
                            || step.contains("USING ROWID")),
                        "{said} walks the table rather than seeking: {steps:?}"
                    );
                }
            }
        }
    }
}

const PAGED_BY: usize = 3;

fn tied_and_unheld(library: &Library) {
    run(
        library,
        "INSERT INTO roots (id, path) VALUES (1, '/music');
         INSERT INTO albums (id, title) VALUES (1, 'One'), (2, 'Two');
         INSERT INTO tracks (root_id, path, title, artist, album_id, disc_number, track_number,
                             duration, sample_rate, channels, sample_format, codec, file_size,
                             modified, added, seen, plays, played, favourite, hidden)
         VALUES
             (1, '/music/01', 'Song', 'Band', 1, 1, 1, 100, 44100, 2, 1, 1, 0, 0, 5, 0, 2, 9, NULL, 0),
             (1, '/music/02', 'song', 'band', 1, 1, 1, 100, 44100, 2, 1, 1, 0, 0, 5, 0, 2, 9, 3, 0),
             (1, '/music/03', 'Song', NULL, 1, 1, NULL, NULL, 96000, 2, 2, 1, 0, 0, 5, 0, 0, NULL, NULL, 0),
             (1, '/music/04', 'Other', 'Band', 2, NULL, 2, 300, 44100, 2, 1, 7, 0, 0, 7, 0, 2, 4, 3, 0),
             (1, '/music/05', 'Other', 'Band', NULL, NULL, NULL, 300, 44100, 2, 1, 7, 0, 0, 7, 0, 0, NULL, NULL, 0),
             (1, '/music/06', 'Alone', 'Solo', NULL, NULL, NULL, NULL, 48000, 2, 1, 1, 0, 0, 1, 0, 5, 1, NULL, 0),
             (1, '/music/07', 'Hidden', 'Solo', 2, 1, 3, 200, 44100, 2, 1, 1, 0, 0, 1, 0, 0, NULL, NULL, 1),
             (1, '/music/08', 'Song', 'Band', 2, 1, 1, 100, 44100, 2, 1, 1, 0, 0, 5, 0, 2, 9, NULL, 0),
             (1, '/music/09', 'Late', NULL, 2, 2, 1, 250, 44100, 2, 1, 1, 0, 0, 9, 0, 1, 2, 1, 0),
             (1, '/music/10', 'late', NULL, 2, 2, 1, 250, 44100, 2, 1, 1, 0, 0, 9, 0, 1, 2, 1, 0),
             (1, '/music/11', 'Copy', 'Band', 1, 1, 1, 100, 44100, 2, 1, 1, 0, 0, 5, 0, 2, 9, NULL, 0);
         UPDATE tracks SET alternative_of = 1 WHERE path = '/music/11';",
    );
}

fn ids(tracks: &[Track]) -> Vec<TrackId> {
    tracks.iter().map(|track| track.id).collect()
}

fn paged_by_key(library: &Library, query: &TrackQuery) -> Result<Vec<TrackId>> {
    let mut held = ids(&library.tracks(&TrackQuery {
        limit: Some(PAGED_BY),
        ..query.clone()
    })?);
    while let Some(last) = held.last().copied() {
        let page = library.tracks_from(
            &TrackQuery {
                limit: Some(PAGED_BY + 1),
                offset: held.len() - 1,
                ..query.clone()
            },
            last,
        )?;
        let mut page = ids(&page).into_iter();
        assert_eq!(
            page.next(),
            Some(last),
            "a page begins on the row it was read from"
        );
        let after: Vec<TrackId> = page.collect();
        if after.is_empty() {
            break;
        }
        held.extend(after);
    }
    Ok(held)
}

#[test]
fn paging_by_the_last_key_reads_every_row_once_in_the_order_read_whole() -> Result<()> {
    let library = Library::open_in_memory()?;
    tied_and_unheld(&library);

    for sort in SortOrder::ALL {
        for reading in Direction::ALL {
            for text in [None, Some("plays:>0")] {
                let query = TrackQuery {
                    sort,
                    reading,
                    text: text.map(str::to_owned),
                    ..TrackQuery::default()
                };
                let whole = ids(&library.tracks(&query)?);

                assert!(whole.len() > PAGED_BY, "{sort:?} reads more than a page");
                assert_eq!(
                    paged_by_key(&library, &query)?,
                    whole,
                    "{sort:?} read {reading:?} for {text:?} paged by its last key"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn a_page_from_a_row_the_catalog_no_longer_holds_is_empty() -> Result<()> {
    let library = Library::open_in_memory()?;
    tied_and_unheld(&library);
    let gone = TrackId::new(999).expect("an id");

    assert!(
        library
            .tracks_from(
                &TrackQuery {
                    sort: SortOrder::Title,
                    limit: Some(PAGED_BY),
                    ..TrackQuery::default()
                },
                gone,
            )?
            .is_empty()
    );
    Ok(())
}

#[test]
fn every_order_ends_on_the_rows_own_id_so_a_page_never_splits_a_tie() {
    for ranked in [false, true] {
        for reading in Direction::ALL {
            for sort in SortOrder::ALL {
                let order = order_by(sort, reading, ranked);
                assert!(
                    order.ends_with(", tracks.id") || order.ends_with(", tracks.id DESC"),
                    "{sort:?} read {reading:?} ends on a tie: {order}"
                );
            }
            for sort in AlbumOrder::ALL {
                let order = album_order_by(sort, reading, ranked);
                assert!(
                    order.ends_with(", a.id") || order.ends_with(", a.id DESC"),
                    "{sort:?} read {reading:?} ends on a tie: {order}"
                );
            }
            for sort in ArtistOrder::ALL {
                let order = artist_order_by(sort, reading, ranked);
                assert!(
                    order.ends_with(", r.id") || order.ends_with(", r.id DESC"),
                    "{sort:?} read {reading:?} ends on a tie: {order}"
                );
            }
        }
    }
}

#[test]
fn a_saved_query_remembers_which_way_round_it_was_read() {
    let playlist = PlaylistId::new(1).expect("one names a playlist");
    for sort in SortOrder::ALL {
        assert_eq!(
            store::sort_of(playlist, store::sort_code(sort))
                .expect("a code this build wrote reads back"),
            sort
        );
    }
    for reading in Direction::ALL {
        assert_eq!(
            store::direction_of(
                playlist,
                OrderedColumn::Reading,
                store::direction_code(reading)
            )
            .expect("a code this build wrote reads back"),
            reading
        );
    }
}

fn titled(library: &Library, title: &str) {
    library
        .inner
        .write(|transaction| {
            transaction
                .execute(
                    "INSERT INTO roots (id, path) VALUES (1, '/music')
                     ON CONFLICT(id) DO NOTHING",
                    [],
                )
                .map_err(|source| Error::store(StoreOp::Insert, source))?;
            transaction
                .execute(
                    "INSERT INTO tracks (root_id, path, title, sample_rate, channels,
                                         sample_format, codec, file_size, modified, added, seen)
                     VALUES (1, ?1, ?2, 44100, 2, ?3, ?4, 0, 0, 0, 0)",
                    params![
                        format!("/music/{title}.wav"),
                        title,
                        store::format_code(SampleFormat::S16),
                        store::codec_code(Codec::Pcm),
                    ],
                )
                .map_err(|source| Error::store(StoreOp::Insert, source))?;
            Ok(())
        })
        .expect("the catalog takes a track");
}

#[test]
fn the_codecs_a_format_order_puts_first_are_the_lossless_ones() {
    let mut lossless: Vec<i64> = Codec::ALL
        .into_iter()
        .filter(|codec| codec.is_lossless())
        .map(store::codec_code)
        .collect();
    lossless.sort_unstable();
    let listed = lossless
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(", ");

    assert_eq!(lossless_codecs!(), format!("tracks.codec IN ({listed})"));
}

#[test]
fn a_format_order_puts_lossless_and_deeper_files_first_and_turns_round() -> Result<()> {
    let library = Library::open_in_memory()?;
    for title in ["Lossy", "Sixteen", "TwentyFour", "HighRate"] {
        titled(&library, title);
    }
    library.inner.write(|transaction| {
        transaction
            .execute(
                "UPDATE tracks SET codec = ?1 WHERE title = 'Lossy'",
                params![store::codec_code(Codec::Mp3)],
            )
            .and_then(|_| {
                transaction.execute(
                    "UPDATE tracks SET codec = ?1 WHERE title != 'Lossy'",
                    params![store::codec_code(Codec::Flac)],
                )
            })
            .and_then(|_| {
                transaction.execute(
                    "UPDATE tracks SET sample_format = ?1 WHERE title IN ('TwentyFour', 'HighRate')",
                    params![store::format_code(SampleFormat::S24)],
                )
            })
            .and_then(|_| {
                transaction.execute(
                    "UPDATE tracks SET sample_rate = 96000 WHERE title = 'HighRate'",
                    [],
                )
            })
            .map_err(|source| Error::store(StoreOp::Update, source))?;
        Ok(())
    })?;
    let titles = |reading| -> Result<Vec<String>> {
        Ok(library
            .tracks(&TrackQuery {
                sort: SortOrder::Format,
                reading,
                ..TrackQuery::default()
            })?
            .into_iter()
            .map(|track| track.title)
            .collect())
    };

    assert_eq!(
        titles(Direction::Descending)?,
        ["HighRate", "TwentyFour", "Sixteen", "Lossy"]
    );
    assert_eq!(
        titles(Direction::Ascending)?,
        ["Lossy", "Sixteen", "TwentyFour", "HighRate"]
    );
    assert_eq!(
        SortOrder::coded(SortOrder::Format.code()),
        Some(SortOrder::Format)
    );
    Ok(())
}

#[test]
fn music_filters_apply_before_paging_and_keep_excluded_tracks() -> Result<()> {
    let library = Library::open_in_memory()?;
    for title in ["Below", "Boundary", "Long", "Unknown"] {
        titled(&library, title);
    }
    library.inner.write(|transaction| {
        transaction.execute_batch(
            "INSERT INTO artists (id, key, name) VALUES (1, 'one', 'One'), (2, 'two', 'Two'), (3, 'guest', 'Guest');
             INSERT INTO albums (id, title, artist_id) VALUES (1, 'First', 1), (2, 'Second', 2);
             UPDATE tracks SET artist_id = 1, album_id = 1, artist = 'One';
             UPDATE tracks SET duration = 59 * sample_rate WHERE title = 'Below';
             UPDATE tracks SET duration = 60 * sample_rate, path = '/music/boundary.FLAC' WHERE title = 'Boundary';
             UPDATE tracks SET duration = 600 * sample_rate, path = '/music/long.mp3', artist_id = 2, album_id = 2, artist = 'Two' WHERE title = 'Long';
             INSERT INTO track_credits (track_id, artist_id) SELECT id, 3 FROM tracks WHERE title = 'Boundary';"
        ).map_err(|source| Error::store(StoreOp::Insert, source))?;
        Ok(())
    })?;
    let query = TrackQuery {
        sort: SortOrder::Title,
        ..TrackQuery::default()
    };
    let all = library.tracks(&query)?;
    assert_eq!(all.len(), 4);
    let boundary = all
        .iter()
        .find(|track| track.title == "Boundary")
        .expect("the boundary track")
        .id;
    library.filter_music(MusicFilters {
        extensions: crate::MusicExtensions::parse(&[".flac", ".mp3"])
            .expect("supported extensions"),
        minimum_length: crate::MinimumLength::new(60).expect("a minute"),
    });
    assert_eq!(
        library
            .tracks(&query)?
            .iter()
            .map(|track| track.title.as_str())
            .collect::<Vec<_>>(),
        ["Boundary", "Long"]
    );
    assert_eq!(library.measured(&query)?.rows, 2);
    assert_eq!(
        library.tracks(&TrackQuery {
            limit: Some(1),
            offset: 1,
            ..query.clone()
        })?[0]
            .title,
        "Long"
    );
    assert_eq!(library.albums(&AlbumQuery::default())?.len(), 2);
    assert_eq!(library.albums_counted(&AlbumQuery::default())?, 2);
    assert_eq!(library.artists_counted(&ArtistQuery::default())?, 3);
    assert_eq!(library.artist_totals(ArtistId::new(1)?)?.tracks, 1);
    library.filter_music(MusicFilters {
        extensions: crate::MusicExtensions::parse(&["flac"]).expect("FLAC"),
        ..library.music_filters()
    });
    assert_eq!(library.tracks(&query)?.len(), 1);
    let albums = library.albums(&AlbumQuery::default())?;
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].track_count, 1);
    assert_eq!(library.albums_counted(&AlbumQuery::default())?, 1);
    let artists = library.artists(&ArtistQuery::default())?;
    assert_eq!(artists.len(), 2);
    assert_eq!(artists[0].track_count, 1);
    assert_eq!(library.artists_counted(&ArtistQuery::default())?, 2);
    assert!(library.search("Below", 10)?.tracks.is_empty());
    assert!(library.search("Below", 10)?.albums.is_empty());
    assert!(library.track(boundary)?.is_some());
    library.filter_music(MusicFilters {
        extensions: crate::MusicExtensions::parse(&[]).expect("none"),
        ..MusicFilters::default()
    });
    assert_eq!(library.measured(&query)?.rows, 0);
    assert!(library.albums(&AlbumQuery::default())?.is_empty());
    assert!(library.artists(&ArtistQuery::default())?.is_empty());
    library.filter_music(MusicFilters::default());
    assert_eq!(library.tracks(&query)?, all);
    Ok(())
}

fn titles_of_albums(library: &Library, query: &AlbumQuery) -> Vec<String> {
    library
        .albums(query)
        .expect("the albums are listed")
        .into_iter()
        .map(|album| album.title)
        .collect()
}

fn run(library: &Library, sql: &str) {
    library
        .inner
        .write(|transaction| {
            transaction
                .execute_batch(sql)
                .map_err(|source| Error::store(StoreOp::Insert, source))?;
            Ok(())
        })
        .expect("the catalog takes the statements");
}

#[test]
fn an_album_counted_order_reads_the_tracks_the_music_filters_leave() {
    let library = Library::open_in_memory().expect("an in-memory catalog opens");
    for title in [
        "Short one",
        "Short two",
        "Long one",
        "Long two",
        "Long three",
    ] {
        titled(&library, title);
    }
    run(
        &library,
        "INSERT INTO albums (id, title) VALUES (1, 'Alpha'), (2, 'Beta');
         UPDATE tracks SET duration = 59 * sample_rate WHERE title LIKE 'Short%';
         UPDATE tracks SET duration = 600 * sample_rate WHERE title LIKE 'Long%';
         UPDATE tracks SET album_id = 1 WHERE title IN ('Short one', 'Short two', 'Long one');
         UPDATE tracks SET album_id = 2 WHERE title IN ('Long two', 'Long three');",
    );
    let by_tracks = AlbumQuery {
        sort: AlbumOrder::Tracks,
        ..AlbumQuery::default()
    };
    assert_eq!(titles_of_albums(&library, &by_tracks), ["Beta", "Alpha"]);

    library.filter_music(MusicFilters {
        minimum_length: crate::MinimumLength::new(60).expect("a minute"),
        ..MusicFilters::default()
    });

    let albums = library.albums(&by_tracks).expect("the albums are listed");
    assert_eq!(
        albums
            .iter()
            .map(|album| (album.title.as_str(), album.track_count))
            .collect::<Vec<_>>(),
        [("Alpha", 1), ("Beta", 2)]
    );
}

#[test]
fn albums_with_no_year_come_last_whichever_way_the_year_is_read() {
    let library = Library::open_in_memory().expect("an in-memory catalog opens");
    for title in ["One", "Two", "Three"] {
        titled(&library, title);
    }
    run(
        &library,
        "INSERT INTO albums (id, title, year) VALUES (1, 'Old', 1990), (2, 'New', 2000), (3, 'Undated', NULL);
         UPDATE tracks SET album_id = (SELECT id FROM albums WHERE title = CASE tracks.title WHEN 'One' THEN 'Old' WHEN 'Two' THEN 'New' ELSE 'Undated' END);",
    );

    let read = |reading| {
        titles_of_albums(
            &library,
            &AlbumQuery {
                sort: AlbumOrder::Year,
                reading,
                ..AlbumQuery::default()
            },
        )
    };

    assert_eq!(read(Direction::Ascending), ["Old", "New", "Undated"]);
    assert_eq!(read(Direction::Descending), ["New", "Old", "Undated"]);
}

#[test]
fn the_albums_and_artists_panes_match_a_search_only_over_shown_best_copies() {
    let library = Library::open_in_memory().expect("an in-memory catalog opens");
    for title in [
        "Whisper hidden",
        "Whisper copy",
        "Original",
        "Plain",
        "Seen",
    ] {
        titled(&library, title);
    }
    run(
        &library,
        "INSERT INTO artists (id, key, name) VALUES (1, 'a', 'Hidden Maker'), (2, 'b', 'Copy Maker'), (3, 'c', 'Seen Maker');
         INSERT INTO albums (id, title, artist_id) VALUES (1, 'Veiled', 1), (2, 'Doubled', 2), (3, 'Open', 3);
         UPDATE tracks SET album_id = 1, artist_id = 1, hidden = 1 WHERE id = 1;
         UPDATE tracks SET album_id = 1, artist_id = 1 WHERE id = 4;
         UPDATE tracks SET album_id = 2, artist_id = 2, alternative_of = 3 WHERE id = 2;
         UPDATE tracks SET album_id = 2, artist_id = 2 WHERE id = 3;
         UPDATE tracks SET album_id = 3, artist_id = 3 WHERE id = 5;
         INSERT INTO tracks_fts (rowid, title, artist, album, genre, lyrics)
              SELECT id, title, '', '', '', '' FROM tracks;",
    );
    let asking = |text: &str| AlbumQuery {
        text: Some(text.to_owned()),
        ..AlbumQuery::default()
    };

    assert!(titles_of_albums(&library, &asking("whisper")).is_empty());
    assert_eq!(titles_of_albums(&library, &asking("plain")), ["Veiled"]);
    assert_eq!(titles_of_albums(&library, &asking("seen")), ["Open"]);
    assert!(
        library
            .artists(&ArtistQuery {
                text: Some("whisper".to_owned()),
                ..ArtistQuery::default()
            })
            .expect("the artists are listed")
            .is_empty()
    );
}

fn suggested(library: &Library, text: &str) -> Option<String> {
    library.did_you_mean(text).expect("the catalog answers")
}

#[test]
fn the_vocabulary_is_kept_until_a_name_in_the_catalog_could_have_moved() {
    let library = Library::open_in_memory().expect("an in-memory catalog");
    titled(&library, "Echoes");
    assert_eq!(suggested(&library, "ekhoes"), Some("Echoes".to_owned()));

    library
        .inner
        .writer
        .lock()
        .execute("UPDATE resume_rows SET uri = uri", [])
        .expect("a table holding no name is written");
    assert_eq!(
        suggested(&library, "ekhoes"),
        Some("Echoes".to_owned()),
        "the vocabulary was read again for a write that could not move a name"
    );

    let named_before = library.inner.named.load(Ordering::Acquire);
    library
        .inner
        .writer
        .lock()
        .execute("UPDATE tracks SET plays = plays + 1, title = title", [])
        .expect("a play is counted and the title written as it stood");
    assert_eq!(
        library.inner.named.load(Ordering::Acquire),
        named_before,
        "a counted play dropped a vocabulary no name moved in"
    );

    library
        .inner
        .writer
        .lock()
        .execute("UPDATE tracks SET title = 'Fearless'", [])
        .expect("the title is rewritten");
    assert_eq!(
        suggested(&library, "ekhoes"),
        None,
        "a write that moved a name left the vocabulary that was read before it standing"
    );
    assert_eq!(suggested(&library, "fearliss"), Some("Fearless".to_owned()));
}

#[test]
fn a_counted_play_or_a_row_seen_again_moves_only_what_reads_the_listening() -> Result<()> {
    let library = Library::open_in_memory()?;
    titled(&library, "Echoes");
    titled(&library, "Time");
    let written = |sql: &str| {
        library
            .inner
            .writer
            .lock()
            .execute(sql, [])
            .expect("the catalog takes the write");
    };
    let whole = TrackQuery::default();
    let most_played = TrackQuery {
        sort: SortOrder::Plays,
        reading: Direction::Descending,
        limit: Some(1),
        ..TrackQuery::default()
    };
    let kept = |library: &Library| {
        library
            .inner
            .measured
            .lock()
            .as_ref()
            .map_or(0, |kept| kept.by_query.len())
    };
    let rows = library.rows_stamp();
    let suggested = library.suggestions()?;

    library.measured(&whole)?;
    library.measured(&most_played)?;
    let measured = kept(&library);

    written("UPDATE tracks SET seen = 7, asked = 1, asks = 1");
    written(
        "UPDATE tracks SET plays = plays + 1, played = 1, favourite = 1, title = title
         WHERE title = 'Time'",
    );
    assert!(
        rows.still_holds_at(library.rows_stamp()),
        "a counted play or a row seen again by a scan moved the rows stamp"
    );
    assert_eq!(library.measured(&whole)?.rows, 2);
    assert_eq!(
        kept(&library),
        measured,
        "the whole listing was counted again"
    );
    library.measured(&most_played)?;
    assert_eq!(
        kept(&library),
        measured + 1,
        "the most played was kept across a play that may have reordered it"
    );
    assert!(
        !Arc::ptr_eq(&suggested, &library.suggestions()?),
        "the lists read from listening were kept across a play"
    );

    written("UPDATE tracks SET duration = 44100 WHERE title = 'Time'");
    assert!(
        !rows.still_holds_at(library.rows_stamp()),
        "a length written left the rows stamp standing"
    );
    Ok(())
}

#[test]
fn a_plan_is_stamped_stale_by_what_it_reads_and_not_by_a_play_or_a_favourite() {
    let library = Library::open_in_memory().expect("an in-memory catalog");
    titled(&library, "Echoes");
    let planned = library.plans_stamp();
    let written = |sql: &str| {
        library
            .inner
            .writer
            .lock()
            .execute(sql, [])
            .expect("the catalog takes the write");
    };

    written("UPDATE tracks SET plays = plays + 1, played = 1, favourite = 1, title = title");
    assert!(
        planned.still_holds_at(library.plans_stamp()),
        "a counted play or a favourite took down a plan nothing it reads moved under"
    );

    written("UPDATE tracks SET track_number = 3");
    assert!(
        !planned.still_holds_at(library.plans_stamp()),
        "a track number written under a plan left it standing"
    );

    let renumbered = library.plans_stamp();
    written("INSERT INTO albums (title) VALUES ('Meddle')");
    let albumed = library.plans_stamp();
    assert!(
        !renumbered.still_holds_at(albumed),
        "an album landed under a plan left it standing"
    );
    written("UPDATE albums SET favourite = 2");
    assert!(
        albumed.still_holds_at(library.plans_stamp()),
        "an album favoured took down a plan"
    );
    written("UPDATE albums SET mbid = 'b1f3'");
    assert!(
        !albumed.still_holds_at(library.plans_stamp()),
        "a release matched under a plan left it standing"
    );
}

#[test]
fn the_vocabulary_is_read_again_once_another_connection_has_written_the_catalog() {
    let scratch = Scratch::new("vocabulary-elsewhere");
    let path = scratch.path.join("catalog.db");
    let window = Library::open(&path).expect("a catalog on disc");
    titled(&window, "Echoes");
    assert_eq!(suggested(&window, "ekhoes"), Some("Echoes".to_owned()));

    let scan = Library::open(&path).expect("the same catalog opened again");
    titled(&scan, "Fearless");

    assert_eq!(
        suggested(&window, "fearliss"),
        Some("Fearless".to_owned()),
        "a name another connection wrote was never offered to this one"
    );
}

#[test]
fn what_another_connection_writes_moves_the_stamp_and_what_this_one_writes_does_not() {
    let scratch = Scratch::new("written-elsewhere");
    let path = scratch.path.join("catalog.db");
    let window = Library::open(&path).expect("a catalog on disc");
    let before = window.written_elsewhere();
    assert!(before.is_some(), "the stamp went unread");

    titled(&window, "Echoes");
    assert_eq!(
        window.written_elsewhere(),
        before,
        "a write of this connection's own was read as another's"
    );

    let model = Library::open(&path).expect("the same catalog opened again");
    titled(&model, "Fearless");
    assert_ne!(
        window.written_elsewhere(),
        before,
        "a write another connection committed left the stamp where it stood"
    );
}

fn counted_at(library: &Library, title: &str, listens: &[Duration]) {
    library
        .inner
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
                                         seen, plays, played)
                     VALUES (1, ?1, ?2, 44100, 2, ?3, ?4, 0, 0, 0, 0, ?5, ?6)
                     RETURNING id",
                    params![
                        format!("/music/{title}.wav"),
                        title,
                        store::format_code(SampleFormat::S16),
                        store::codec_code(Codec::Pcm),
                        listens.len() as i64,
                        store::to_nanos(SystemTime::now()),
                    ],
                    |row| row.get(0),
                )
                .map_err(|source| Error::store(StoreOp::Insert, source))?;

            for ago in listens {
                transaction
                    .execute(
                        "INSERT INTO listens (track_id, at) VALUES (?1, ?2)",
                        params![
                            track,
                            store::to_nanos(
                                SystemTime::now().checked_sub(*ago).unwrap_or(UNIX_EPOCH)
                            ),
                        ],
                    )
                    .map_err(|source| Error::store(StoreOp::Insert, source))?;
            }

            Ok(())
        })
        .expect("the catalog takes a track and the listens counted against it");
}

fn narrowed_to(library: &Library, text: &str) -> Vec<String> {
    library
        .tracks(&TrackQuery {
            text: Some(text.to_owned()),
            sort: SortOrder::Title,
            ..TrackQuery::default()
        })
        .expect("a search the grammar reads")
        .into_iter()
        .map(|track| track.title)
        .collect()
}

#[test]
fn a_play_older_than_the_window_is_counted_by_the_lifetime_term_alone() {
    let library = Library::open_in_memory().expect("an in-memory catalog opens");
    counted_at(&library, "Lately", &[A_MINUTE_AGO]);
    counted_at(&library, "Once", &[A_YEAR_AGO]);
    counted_at(&library, "Both", &[A_MINUTE_AGO, A_YEAR_AGO]);

    assert_eq!(
        narrowed_to(&library, "plays:>0"),
        vec!["Both", "Lately", "Once"]
    );
    assert_eq!(
        narrowed_to(&library, "plays:>0@30d"),
        vec!["Both", "Lately"]
    );
    assert_eq!(narrowed_to(&library, "plays:2"), vec!["Both"]);
    assert_eq!(narrowed_to(&library, "plays:1@30d"), vec!["Both", "Lately"]);
    assert_eq!(narrowed_to(&library, "plays:0@30d"), vec!["Once"]);
    assert_eq!(
        narrowed_to(&library, "plays:>0@2y"),
        vec!["Both", "Lately", "Once"]
    );
}

#[test]
fn a_narrowing_with_nothing_a_search_reads_matches_nothing_rather_than_everything() {
    assert!(matches!(cuts_matching("???", "e"), Narrowed::Nothing));
    assert!(matches!(cuts_matching("!!!", "e"), Narrowed::Nothing));
    assert!(matches!(cuts_matching("", "e"), Narrowed::Unasked));
    assert!(matches!(cuts_matching("echoes", "e"), Narrowed::To(_)));
    assert!(scoped(&TrackQuery::default(), Some("???"), MusicFilters::default()).is_none());
    assert!(scoped(&TrackQuery::default(), None, MusicFilters::default()).is_some());
}

#[test]
fn no_more_readers_are_open_at_once_than_the_pool_parks() {
    let scratch = Scratch::new("readers");
    let library = Library::open(&scratch.path.join("library.db")).expect("a catalog opens on disk");
    let inner = Arc::clone(&library.inner);

    thread::scope(|scope| {
        for _ in 0..READERS_AT_ONCE {
            scope.spawn(|| {
                inner
                    .read(|connection| {
                        connection
                            .query_row("SELECT count(*) FROM tracks", [], |row| {
                                row.get::<_, i64>(0)
                            })
                            .map_err(|source| Error::store(StoreOp::Query, source))
                    })
                    .expect("a reader answers a count");
            });
        }
    });

    assert!(
        inner.readers_open() <= READER_POOL,
        "{} readers were opened for {READERS_AT_ONCE} callers",
        inner.readers_open()
    );
}

fn nothing_to_walk() -> ScanOptions {
    ScanOptions {
        roots: Vec::new(),
        incremental: true,
        follow_symlinks: false,
        extract_cover_art: false,
        workers: NonZeroUsize::MIN,
    }
}

struct Stall {
    reached: Sender<()>,
    lifted: Receiver<()>,
}

fn holding_the_writer(library: &Library, stall: Stall) {
    library
        .inner
        .write(|_| {
            stall.reached.send(()).expect("the test is still waiting");
            stall.lifted.recv().expect("the test lifts the stall");
            Ok(())
        })
        .expect("a transaction that writes nothing commits");
}

#[test]
fn a_scan_refuses_to_start_while_an_organise_is_running() {
    let library = Library::open_in_memory().expect("a catalog opens in memory");
    let (reached, reaches) = mpsc::channel();
    let (lift, lifted) = mpsc::channel();

    thread::scope(|scope| {
        scope.spawn(|| holding_the_writer(&library, Stall { reached, lifted }));
        reaches.recv().expect("the writer is held");

        let organising = library
            .organise(OrganiseOptions::default())
            .expect("nothing else is walking the tree");
        assert!(
            matches!(library.scan(nothing_to_walk()), Err(Error::AlreadyWalking)),
            "a scan started while an organise was in flight"
        );

        lift.send(()).expect("the stalled writer is still there");
        organising.join().expect("the organise finished");
    });

    library
        .scan(nothing_to_walk())
        .expect("the tree is walkable once the organise has gone")
        .join()
        .expect("the scan finished");
}

#[test]
fn an_organise_refuses_to_start_while_a_scan_is_running() {
    let library = Library::open_in_memory().expect("a catalog opens in memory");
    let (reached, reaches) = mpsc::channel();
    let (lift, lifted) = mpsc::channel();

    thread::scope(|scope| {
        scope.spawn(|| holding_the_writer(&library, Stall { reached, lifted }));
        reaches.recv().expect("the writer is held");

        let scanning = library
            .scan(nothing_to_walk())
            .expect("nothing else is walking the tree");
        assert!(
            matches!(
                library.organise(OrganiseOptions::default()),
                Err(Error::AlreadyWalking)
            ),
            "an organise started while a scan was in flight"
        );

        lift.send(()).expect("the stalled writer is still there");
        scanning.join().expect("the scan finished");
    });

    library
        .organise(OrganiseOptions::default())
        .expect("the tree is walkable once the scan has gone")
        .join()
        .expect("the organise finished");
}

#[test]
fn a_lookup_or_a_poll_in_one_process_refuses_the_same_in_another_on_the_same_catalog() {
    let scratch = Scratch::new("asked-across-processes");
    let catalog = scratch.path.join("library.db");
    let here = Library::open(&catalog).expect("a catalog opens on disc");
    let elsewhere = Library::open(&catalog).expect("a catalog opens twice on disc");

    let looking = here
        .asking_alone(PassKind::Enrich)
        .expect("nothing else is looking things up");
    let polling = here
        .asking_alone(PassKind::Poll)
        .expect("a poll is not a lookup");
    assert!(matches!(
        elsewhere.asking_alone(PassKind::Enrich),
        Err(Error::AlreadyAsking {
            pass: PassKind::Enrich
        })
    ));
    assert!(matches!(
        elsewhere.asking_alone(PassKind::Poll),
        Err(Error::AlreadyAsking {
            pass: PassKind::Poll
        })
    ));

    drop(looking);
    elsewhere
        .asking_alone(PassKind::Enrich)
        .expect("a lookup may start once the other process let it go");
    drop(polling);

    let memory = Library::open_in_memory().expect("a catalog opens in memory");
    let (first, second) = (
        memory.asking_alone(PassKind::Enrich),
        memory.asking_alone(PassKind::Enrich),
    );
    assert!(
        first.is_ok() && second.is_ok(),
        "a catalog in memory has no second process"
    );
}

#[test]
fn a_catalog_with_nothing_to_settle_is_opened_again_without_a_write() {
    let scratch = Scratch::new("opened-quietly");
    let catalog = scratch.path.join("library.db");
    drop(Library::open(&catalog).expect("a catalog opens on disc"));

    let watching = Connection::open(&catalog).expect("a second connection");
    let data_version = || -> i64 {
        watching
            .query_row("PRAGMA data_version", [], |row| row.get(0))
            .expect("a data version")
    };
    let owed: bool = watching
        .query_row("SELECT owed FROM settle_owed", [], |row| row.get(0))
        .expect("the flag is there");
    assert!(!owed, "the first open left what it settled owed");

    let before = data_version();
    drop(Library::open(&catalog).expect("a catalog opens again"));
    assert_eq!(
        data_version(),
        before,
        "opening a catalog nothing had changed wrote to it"
    );

    watching
        .execute("INSERT INTO artists (key, name) VALUES ('x', 'X')", [])
        .expect("an artist is added");
    let owed: bool = watching
        .query_row("SELECT owed FROM settle_owed", [], |row| row.get(0))
        .expect("the flag is there");
    assert!(owed, "an artist added did not owe a settle");
    drop(Library::open(&catalog).expect("a catalog opens once more"));
    let owed: bool = watching
        .query_row("SELECT owed FROM settle_owed", [], |row| row.get(0))
        .expect("the flag is there");
    assert!(!owed, "an open did not settle what was owed");
}

#[test]
fn a_walk_in_one_process_refuses_a_walk_in_another_on_the_same_catalog() {
    let scratch = Scratch::new("walked-across-processes");
    let catalog = scratch.path.join("library.db");
    let here = Library::open(&catalog).expect("a catalog opens on disc");
    let elsewhere = Library::open(&catalog).expect("a catalog opens twice on disc");

    let walking = here
        .walk_the_tree()
        .expect("nothing else is walking the tree");
    assert!(
        matches!(
            elsewhere.scan(nothing_to_walk()),
            Err(Error::AlreadyWalking)
        ),
        "a second process walked a catalog the first was walking"
    );

    drop(walking);
    elsewhere
        .scan(nothing_to_walk())
        .expect("the tree is walkable once the other process let it go")
        .join()
        .expect("the scan finished");
}

#[test]
fn an_edit_that_reads_before_it_writes_is_not_torn_by_another_process_committing() {
    let scratch = Scratch::new("read-then-written");
    let catalog = scratch.path.join("library.db");
    let here = Library::open(&catalog).expect("a catalog opens on disc");
    let elsewhere = Library::open(&catalog).expect("a catalog opens twice on disc");
    let first = scratch.path.join("first");
    let second = scratch.path.join("second");

    let (go, going) = mpsc::channel();
    let committing = thread::spawn(move || {
        going.recv().expect("the edit under way says when");
        elsewhere
            .inner
            .write(|transaction| store::register_root(transaction, &second))
    });
    let written = here.inner.write(|transaction| {
        transaction
            .query_row("SELECT count(*) FROM roots", [], |row| row.get::<_, i64>(0))
            .map_err(|source| Error::store(StoreOp::Query, source))?;
        go.send(()).expect("the other process is waiting");
        thread::sleep(Duration::from_millis(200));
        store::register_root(transaction, &first)
    });

    assert!(
        written.is_ok(),
        "an edit reading before it wrote was torn: {written:?}"
    );
    assert!(
        committing
            .join()
            .expect("the other process did not panic")
            .is_ok(),
        "the other process's edit failed rather than waiting its turn"
    );
    assert_eq!(here.roots().expect("the roots read back").len(), 2);
}

#[test]
fn the_tracks_among_some_paths_are_those_the_named_roots_hold_however_many_are_asked() -> Result<()>
{
    let library = Library::open_in_memory()?;
    titled(&library, "Echoes");
    titled(&library, "Days");

    let echoes = PathBuf::from("/music/Echoes.wav");
    let days = PathBuf::from("/music/Days.wav");
    let absent = PathBuf::from("/music/Absent.wav");
    let asked = [echoes.as_path(), days.as_path(), absent.as_path()];

    let everywhere = library.tracks_among(&[], &asked)?;
    let under_music = library.tracks_among(&[PathBuf::from("/music")], &asked)?;
    let under_nothing = library.tracks_among(&[PathBuf::from("/nowhere")], &asked)?;

    assert_eq!(
        everywhere,
        AHashSet::from_iter([echoes.clone(), days.clone()])
    );
    assert_eq!(under_music, everywhere);
    assert!(under_nothing.is_empty());

    let many: Vec<PathBuf> = (0..PATHS_ASKED_AT_ONCE * 2 + 1)
        .map(|at| PathBuf::from(format!("/music/{at}.wav")))
        .chain([echoes.clone()])
        .collect();
    let many: Vec<&Path> = many.iter().map(PathBuf::as_path).collect();
    assert_eq!(
        library.tracks_among(&[], &many)?,
        AHashSet::from_iter([echoes])
    );
    Ok(())
}

#[test]
fn a_pass_that_panicked_does_not_hold_the_library_shut() {
    let library = Library::open_in_memory().expect("a catalog opens in memory");
    let inner = Arc::clone(&library.inner);

    let lost = thread::spawn(move || {
        let _walking = inner
            .walk_the_tree()
            .expect("nothing else is walking the tree");
        panic!("a pass fell over on its way through the tree");
    });

    assert!(
        lost.join().is_err(),
        "the pass this test breaks did not panic"
    );
    library
        .scan(nothing_to_walk())
        .expect("the tree is walkable after a pass panicked")
        .join()
        .expect("the scan finished");
}

#[test]
fn a_root_is_neither_added_nor_dropped_under_a_pass_walking_the_tree() {
    let library = Library::open_in_memory().expect("a catalog opens in memory");
    let root = std::env::temp_dir();

    let walking = library.walk_the_tree().expect("nothing else is walking");
    let added = library.add_root(&root);
    let dropped = library.remove_root(&root);
    drop(walking);

    assert!(matches!(added, Err(Error::AlreadyWalking)), "{added:?}");
    assert!(matches!(dropped, Err(Error::AlreadyWalking)), "{dropped:?}");
    library
        .add_root(&root)
        .expect("a root added once the walk ended");
    assert!(library.remove_root(&root).expect("a root dropped"));
}
