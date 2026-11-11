use std::{
    fs,
    hint::black_box,
    path::{Path, PathBuf},
    process,
    time::{Duration, Instant},
};

use resonate_library::{Direction, Library, SortOrder, TrackQuery};
use rusqlite::{Connection, params};

const TRACKS: usize = 500_000;
const ALBUMS: usize = 60_000;
const ARTISTS: usize = 50_000;
const PAGE: usize = 2_000;
const SEED: u64 = 0x5265_736F_6E61_7465;
const ORDERS: [(SortOrder, Direction); 6] = [
    (SortOrder::Relevance, Direction::Ascending),
    (SortOrder::DateAdded, Direction::Descending),
    (SortOrder::Title, Direction::Ascending),
    (SortOrder::Artist, Direction::Descending),
    (SortOrder::Plays, Direction::Descending),
    (SortOrder::Played, Direction::Descending),
];

struct Noise(u64);

impl Noise {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> i64 {
        (self.next() % bound as u64) as i64
    }
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn filled(catalog: &Path) {
    let mut connection = Connection::open(catalog).expect("the catalog opens beside the library");
    let filling = connection.transaction().expect("a transaction begins");
    let mut noise = Noise(SEED);
    {
        let mut artist = filling
            .prepare("INSERT INTO artists (id, key, name) VALUES (?1, ?2, ?3)")
            .expect("artists are written");
        for id in 1..=ARTISTS as i64 {
            artist
                .execute(params![id, format!("artist {id}"), format!("Artist {id}")])
                .expect("an artist is written");
        }
        let mut album = filling
            .prepare("INSERT INTO albums (id, title, artist_id) VALUES (?1, ?2, ?3)")
            .expect("albums are written");
        for id in 1..=ALBUMS as i64 {
            album
                .execute(params![
                    id,
                    format!("Album {}", noise.below(ALBUMS)),
                    1 + noise.below(ARTISTS)
                ])
                .expect("an album is written");
        }
        let mut track = filling
            .prepare(
                "INSERT INTO tracks (path, title, artist, artist_id, album_id, track_number,
                                     disc_number, duration, sample_rate, channels, sample_format,
                                     codec, file_size, modified, added, seen, plays, played)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, 44100, 2, 1, 1, 1, 0, ?8, 0, ?9, ?10)",
            )
            .expect("tracks are written");
        for at in 0..TRACKS {
            let artist = 1 + noise.below(ARTISTS);
            let plays = match noise.below(5) {
                0 => noise.below(40),
                _ => 0,
            };
            track
                .execute(params![
                    format!("/music/{at}.flac"),
                    format!("Song {}", noise.below(TRACKS)),
                    format!("Artist {artist}"),
                    artist,
                    1 + noise.below(ALBUMS),
                    1 + noise.below(14),
                    44_100 * (60 + noise.below(400)),
                    noise.below(1 << 30),
                    plays,
                    (plays > 0).then(|| noise.below(1 << 30)),
                ])
                .expect("a track is written");
        }
    }
    filling.commit().expect("the catalog is filled");
    connection
        .execute_batch("ANALYZE")
        .expect("the planner is told");
}

fn asked(sort: SortOrder, reading: Direction) -> TrackQuery {
    TrackQuery {
        sort,
        reading,
        ..TrackQuery::default()
    }
}

fn by_offset(library: &Library, sort: SortOrder, reading: Direction) -> usize {
    let mut held: usize = 0;
    loop {
        let overlapping = held.saturating_sub(1);
        let page = library
            .tracks(&TrackQuery {
                limit: Some(PAGE + 1),
                offset: overlapping,
                ..asked(sort, reading)
            })
            .expect("a page is read");
        let grown = page.len().saturating_sub(usize::from(held > 0));
        held += grown;
        if page.len() < PAGE + 1 {
            return held;
        }
    }
}

fn by_key(library: &Library, sort: SortOrder, reading: Direction) -> usize {
    let mut held: usize = 0;
    let mut last = None;
    loop {
        let query = TrackQuery {
            limit: Some(PAGE + 1),
            offset: held.saturating_sub(1),
            ..asked(sort, reading)
        };
        let page = match last {
            Some(last) => library.tracks_from(&query, last),
            None => library.tracks(&query),
        }
        .expect("a page is read");
        let grown = page.len().saturating_sub(usize::from(last.is_some()));
        held += grown;
        last = page.last().map(|track| track.id);
        if page.len() < PAGE + 1 {
            return held;
        }
    }
}

fn timed(what: &str, run: impl FnOnce() -> usize) {
    let started = Instant::now();
    let rows = black_box(run());
    let took: Duration = started.elapsed();
    println!(
        "{what:<48} {rows:>8} rows {:>10.1} ms",
        took.as_secs_f64() * 1e3
    );
}

fn main() {
    let scratch = Scratch(std::env::temp_dir().join(format!("resonate-paging-{}", process::id())));
    fs::create_dir_all(&scratch.0).expect("a scratch folder is made");
    let catalog = scratch.0.join("library.db");

    let started = Instant::now();
    drop(Library::open(&catalog).expect("the library opens"));
    filled(&catalog);
    println!(
        "{:<48} {:>10.1} ms",
        "building a catalog of 500k tracks",
        started.elapsed().as_secs_f64() * 1e3
    );

    let library = Library::open(&catalog).expect("the library opens again");
    for (sort, reading) in ORDERS {
        let order = format!("{sort:?} {reading:?}");
        timed(&format!("to the end by key: {order}"), || {
            by_key(&library, sort, reading)
        });
        if std::env::var_os("RESONATE_BENCH_OFFSET").is_some() {
            timed(&format!("to the end by offset: {order}"), || {
                by_offset(&library, sort, reading)
            });
        }
    }
}
