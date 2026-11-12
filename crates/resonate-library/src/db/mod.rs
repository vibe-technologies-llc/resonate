#[macro_use]
mod statements;
#[macro_use]
mod ordered;
mod corrections;
mod enriching;
mod filing;
mod following;
mod held;
mod listed;
mod listening;
mod lyrics;
mod passes;
mod pictures;
mod playlists;
mod resuming;
mod roots;
mod rows;
mod searched;
mod stamps;
mod studied;
mod syncing;
#[cfg(test)]
mod tests;
mod tracks;
mod vault;
mod versions_chosen;
mod wants;

use std::{
    fs::{self, File, OpenOptions, TryLockError},
    ops::Deref,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};

use ahash::AHashMap;
use parking_lot::{Condvar, Mutex};
use resonate_codec::{Hinting, Sources, StandIn};
use resonate_core::{AlbumId, Frames, Gate, MediaLocation, PlaylistId, Span, ZoneId};
use resonate_vault::{Encoding, Kept as VaultKept, VaultFiles};
use rusqlite::{
    Connection, OptionalExtension as _, Row, Transaction, TransactionBehavior, hooks::Action,
    params, types::Value,
};

pub(crate) use self::{
    rows::{RawTrack, TrackToVault},
    searched::{
        Narrowed, PICTURES_WEIGHED_PER_TILE, clause, cuts_matching, distinct_pictures, measured,
        measured_narrowed, pictured_by, pictures_of_albums, reads_listening, tracks,
    },
    statements::{BESIDE_A_TRACK, TRACK_COLUMNS},
};
use self::{
    rows::{held_cover, rows},
    statements::{FILED_IN_ORDER, NOTE_WHERE_AN_OBJECT_WAS_DELIVERED_FROM},
};
use crate::{
    Cancelling, CoverArt, Error, Measured, MusicFilters, PassKind, Playing, Result, SavedQuery,
    Spellings, StoreOp, Suggestion, Track, TrackQuery, Vault,
    deleted::Trashes,
    filed::{AlbumToFile, DeliveryFolder},
    foresight::Foresight,
    hinted::Hinted,
    playlist::{self},
    renamed::{ADDED, RENAMED_TRIGGERS, RETITLED, Renamed},
    schema, store, suggest,
    synced::{self},
    undo::Step,
    vaulted::Vaulted,
};

const READER_POOL: usize = 8;

enum Visited {
    Held(Option<Box<RawTrack>>, i64),
    Unheld(i64),
}

enum Source {
    File(PathBuf),
    Memory,
}

#[derive(Default)]
struct Pool {
    parked: Vec<Connection>,
    open: usize,
}

pub(crate) struct Inner {
    source: Source,
    writer: Mutex<Connection>,
    readers: Mutex<Pool>,
    freed: Condvar,
    walking: AtomicBool,
    walking_pass: Mutex<Option<Arc<dyn Cancelling>>>,
    walk_ended: Condvar,
    vault: Option<Arc<Vault>>,
    playlists: AtomicU64,
    arrivals: AtomicU64,
    named: Arc<AtomicU64>,
    written: Arc<AtomicU64>,
    heard: Arc<AtomicU64>,
    planned: Arc<AtomicU64>,
    renamed: Arc<Mutex<Renamed>>,
    spellings: Mutex<Option<KeptVocabulary>>,
    suggested: Mutex<Option<KeptSuggestions>>,
    filed: Mutex<Option<KeptFiling>>,
    measured: Mutex<Option<KeptMeasurements>>,
    pictured: Mutex<Option<KeptPictures>>,
    playing: Mutex<Option<Playing>>,
    steps: Mutex<Vec<Step>>,
    walked: Mutex<Vec<Step>>,
    offering: AtomicBool,
    keeping: AtomicBool,
    a_poll_waits_on_a_keep: AtomicBool,
    delivering_into: Mutex<Option<DeliveryFolder>>,
    stalling: Mutex<Gate>,
    trash_home: Mutex<Option<PathBuf>>,
    trashes: Mutex<Option<Arc<dyn Trashes>>>,
    music_filters: Mutex<MusicFilters>,
    listens_settle_after: Mutex<Duration>,
    foresight: Foresight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogStamp {
    pub(crate) named: u64,
    written_elsewhere: Option<i64>,
}

impl CatalogStamp {
    pub fn still_holds_at(self, now: Self) -> bool {
        self.named == now.named && self.written_here_alone_until(now)
    }

    pub(crate) fn written_here_alone_until(self, now: Self) -> bool {
        now.written_elsewhere
            .is_none_or(|elsewhere| self.written_elsewhere == Some(elsewhere))
    }

    #[cfg(test)]
    pub(crate) const fn at(named: u64, written_elsewhere: Option<i64>) -> Self {
        Self {
            named,
            written_elsewhere,
        }
    }
}

struct KeptVocabulary {
    stamp: CatalogStamp,
    spellings: Arc<Spellings>,
}

struct KeptMeasurements {
    stamp: CatalogStamp,
    filters: MusicFilters,
    kept_at: Instant,
    by_query: AHashMap<String, Measured>,
}

impl KeptMeasurements {
    fn holds(&self, stamp: CatalogStamp, filters: MusicFilters) -> bool {
        self.stamp.still_holds_at(stamp)
            && self.filters == filters
            && self.kept_at.elapsed() < MEASUREMENTS_KEPT_FOR
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct PicturesAt {
    rows: CatalogStamp,
    heard: u64,
    playlists: u64,
    filters: MusicFilters,
}

impl PicturesAt {
    fn holds(self, now: Self) -> bool {
        self.rows.still_holds_at(now.rows)
            && self.playlists == now.playlists
            && self.filters == now.filters
    }
}

struct KeptPictures {
    at: PicturesAt,
    by_playlist: AHashMap<(PlaylistId, usize), Pictured>,
}

struct Pictured {
    albums: Vec<AlbumId>,
    heard: Option<u64>,
}

impl Pictured {
    fn holds(&self, heard: u64) -> bool {
        self.heard.is_none_or(|then| then == heard)
    }
}

const MEASUREMENTS_KEPT_FOR: Duration = Duration::from_secs(60);

const MEASUREMENTS_KEPT_AT_MOST: usize = 64;

struct KeptFiling {
    stamp: CatalogStamp,
    filing: Arc<suggest::Filing>,
}

struct KeptSuggestions {
    stamp: CatalogStamp,
    heard: u64,
    filters: MusicFilters,
    today: i64,
    suggestions: Arc<[Suggestion]>,
}

struct Reader<'a> {
    pool: &'a Inner,
    connection: Option<Connection>,
}

pub(crate) struct Walk {
    inner: Arc<Inner>,
    _across_processes: Option<File>,
}

impl Walk {
    pub(crate) fn cancelled_by(&self, pass: Arc<dyn Cancelling>) {
        *self.inner.walking_pass.lock() = Some(pass);
    }
}

impl Drop for Walk {
    fn drop(&mut self) {
        let mut held = self.inner.walking_pass.lock();
        *held = None;
        self.inner.walking.store(false, Ordering::Release);
        drop(held);
        self.inner.walk_ended.notify_all();
    }
}

const WALK_LOCK_SUFFIX: &str = ".walk";

const LANDING_GRACE: Duration = Duration::from_secs(10 * 60);

fn landed_before_it(object: &Path, moment: SystemTime) -> bool {
    fs::metadata(object)
        .and_then(|metadata| metadata.modified())
        .is_ok_and(|landed| landed < moment)
}

fn locked_across_processes(catalog: &Path) -> Result<File> {
    lock_beside(catalog, WALK_LOCK_SUFFIX, || Error::AlreadyWalking)
}

pub(crate) struct Asking {
    _across_processes: Option<File>,
}

fn asking_across_processes(catalog: &Path, pass: PassKind) -> Result<File> {
    let suffix = match pass {
        PassKind::Enrich => ".enrich",
        _ => ".poll",
    };
    lock_beside(catalog, suffix, || Error::AlreadyAsking { pass })
}

fn lock_beside(catalog: &Path, suffix: &str, held: impl FnOnce() -> Error) -> Result<File> {
    let mut named = catalog.as_os_str().to_owned();
    named.push(suffix);
    let path = PathBuf::from(named);
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|source| Error::Io {
            path: path.clone(),
            source,
        })?;
    match lock.try_lock() {
        Ok(()) => Ok(lock),
        Err(TryLockError::WouldBlock) => Err(held()),
        Err(TryLockError::Error(source)) => Err(Error::Io { path, source }),
    }
}

impl Deref for Reader<'_> {
    type Target = Connection;

    fn deref(&self) -> &Connection {
        self.connection
            .as_ref()
            .expect("a checked-out reader holds its connection until it is dropped")
    }
}

impl Drop for Reader<'_> {
    fn drop(&mut self) {
        let Some(connection) = self.connection.take() else {
            return;
        };
        self.pool.readers.lock().parked.push(connection);
        self.pool.freed.notify_one();
    }
}

impl Inner {
    pub(crate) fn vault_root(&self) -> Option<&Path> {
        self.vault.as_deref().map(Vault::root)
    }

    fn opened_vault(&self) -> Result<&Arc<Vault>> {
        self.vault.as_ref().ok_or(Error::NoVault)
    }

    pub(crate) fn cover_art(&self, id: AlbumId) -> Result<Option<CoverArt>> {
        let stored = self.read(|connection| {
            connection
                .query_row(
                    "SELECT cover_art, cover_format, cover_path FROM albums WHERE id = ?1",
                    params![id.get() as i64],
                    |row| {
                        Ok((
                            row.get::<_, Option<Vec<u8>>>(0)?,
                            row.get::<_, Option<i64>>(1)?,
                            row.get::<_, Option<String>>(2)?,
                        ))
                    },
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))
        })?;

        if let Some((_, _, Some(kept))) = &stored {
            let Some(vault) = self.vault.as_ref() else {
                tracing::debug!(
                    album = id.get(),
                    "a cover kept in the vault is drawn by no build that has not opened one"
                );
                return Ok(None);
            };
            let path = self.in_the_vault(kept)?;
            return vault
                .picture(&path)
                .map(Some)
                .map_err(|source| Error::Vault {
                    path,
                    source: Box::new(source),
                });
        }

        let Some((Some(bytes), code, _)) = stored else {
            return Ok(None);
        };
        held_cover(id, bytes, code).map(Some)
    }

    pub(crate) fn in_the_vault(&self, within: &str) -> Result<PathBuf> {
        let vault = self.opened_vault()?;
        vault
            .at(&store::path_of(within))
            .map_err(|source| Error::Vault {
                path: store::path_of(within),
                source: Box::new(source),
            })
    }

    pub(crate) fn within_the_vault(&self, path: &Path) -> Result<String> {
        let vault = self.opened_vault()?;
        let within = vault.within(path).map_err(|source| Error::Vault {
            path: path.to_path_buf(),
            source: Box::new(source),
        })?;
        Ok(store::path_text(&within).into_owned())
    }

    pub(crate) fn walk_the_tree(self: &Arc<Self>) -> Result<Walk> {
        if self
            .walking
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(Error::AlreadyWalking);
        }
        let mut walk = Walk {
            inner: Arc::clone(self),
            _across_processes: None,
        };
        if let Source::File(catalog) = &self.source {
            walk._across_processes = Some(locked_across_processes(catalog)?);
        }
        Ok(walk)
    }

    pub(crate) fn playlists_revision(&self) -> u64 {
        self.playlists.load(Ordering::Acquire)
    }

    pub(crate) fn playlists_changed(&self) {
        self.playlists.fetch_add(1, Ordering::AcqRel);
    }

    pub(crate) fn arrived(&self) {
        self.arrivals.fetch_add(1, Ordering::AcqRel);
    }

    pub(crate) fn arrivals(&self) -> u64 {
        self.arrivals.load(Ordering::Acquire)
    }

    pub(crate) const fn steps(&self) -> &Mutex<Vec<Step>> {
        &self.steps
    }

    pub(crate) const fn walked(&self) -> &Mutex<Vec<Step>> {
        &self.walked
    }

    pub(crate) fn offers(&self) -> bool {
        self.offering.load(Ordering::Acquire)
    }

    pub(crate) fn playing(&self) -> Option<Playing> {
        *self.playing.lock()
    }

    pub(crate) fn set_playing(&self, playing: Option<Playing>) {
        *self.playing.lock() = playing.filter(|playing| self.counted_a_play(playing.playlist));
    }

    fn counted_a_play(&self, playing: PlaylistId) -> bool {
        match playlist::played_now(self, playing) {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(%error, %playing, "a playlist's last play was not recorded");
                false
            }
        }
    }

    pub(crate) fn read<T>(&self, query: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let Source::File(_) = self.source else {
            return query(&self.writer.lock());
        };
        let reader = self.checkout()?;
        query(&reader)
    }

    fn checkout(&self) -> Result<Reader<'_>> {
        let mut pool = self.readers.lock();
        loop {
            if let Some(connection) = pool.parked.pop() {
                return Ok(Reader {
                    pool: self,
                    connection: Some(connection),
                });
            }
            if pool.open < READER_POOL {
                pool.open += 1;
                drop(pool);

                return match connect(&self.source, schema::Role::Reading) {
                    Ok(connection) => Ok(Reader {
                        pool: self,
                        connection: Some(connection),
                    }),
                    Err(error) => {
                        self.readers.lock().open -= 1;
                        Err(error)
                    }
                };
            }
            self.freed.wait(&mut pool);
        }
    }

    #[cfg(test)]
    pub(crate) fn readers_open(&self) -> usize {
        self.readers.lock().open
    }

    pub(crate) fn restate_the_statistics(&self) {
        if let Err(error) = schema::restate_the_statistics(&self.writer.lock()) {
            tracing::warn!(%error, "the query planner is left reading the statistics it already had");
        }
    }

    pub(crate) fn write<T>(
        &self,
        change: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T>,
    ) -> Result<T> {
        let mut connection = self.writer.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| Error::store(StoreOp::Transaction, source))?;

        let value = change(&transaction)?;
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Transaction, source))?;
        Ok(value)
    }

    fn names_stamp(&self) -> CatalogStamp {
        self.stamped_by(&self.named)
    }

    fn rows_stamp(&self) -> CatalogStamp {
        self.stamped_by(&self.written)
    }

    pub(crate) fn filing(&self) -> Result<Arc<suggest::Filing>> {
        let stamp = self.names_stamp();
        if let Some(kept) = self
            .filed
            .lock()
            .as_ref()
            .filter(|kept| kept.stamp.still_holds_at(stamp))
        {
            return Ok(Arc::clone(&kept.filing));
        }

        let filing = Arc::new(suggest::Filing::read(self)?);
        *self.filed.lock() = Some(KeptFiling {
            stamp,
            filing: Arc::clone(&filing),
        });

        Ok(filing)
    }

    pub(crate) fn measured_saved(&self, saved: &SavedQuery) -> Result<Measured> {
        measured(self, &TrackQuery::from(saved))
    }

    fn measurement_kept(
        &self,
        stamp: CatalogStamp,
        filters: MusicFilters,
        key: &str,
    ) -> Option<Measured> {
        self.measured
            .lock()
            .as_ref()
            .filter(|kept| kept.holds(stamp, filters))
            .and_then(|kept| kept.by_query.get(key))
            .copied()
    }

    fn keep_measurement(
        &self,
        stamp: CatalogStamp,
        filters: MusicFilters,
        key: String,
        measured: Measured,
    ) {
        let mut kept = self.measured.lock();
        match kept.as_mut().filter(|kept| kept.holds(stamp, filters)) {
            Some(kept) => {
                if kept.by_query.len() >= MEASUREMENTS_KEPT_AT_MOST {
                    kept.by_query.clear();
                }
                kept.by_query.insert(key, measured);
            }
            None => {
                *kept = Some(KeptMeasurements {
                    stamp,
                    filters,
                    kept_at: Instant::now(),
                    by_query: AHashMap::from_iter([(key, measured)]),
                });
            }
        }
    }

    pub(crate) fn pictures_at(&self) -> PicturesAt {
        PicturesAt {
            rows: self.rows_stamp(),
            heard: self.heard(),
            playlists: self.playlists_revision(),
            filters: *self.music_filters.lock(),
        }
    }

    pub(crate) fn pictures_kept(
        &self,
        at: PicturesAt,
        playlist: PlaylistId,
        at_most: usize,
    ) -> Option<Vec<AlbumId>> {
        self.pictured
            .lock()
            .as_ref()
            .filter(|kept| kept.at.holds(at))
            .and_then(|kept| kept.by_playlist.get(&(playlist, at_most)))
            .filter(|pictured| pictured.holds(at.heard))
            .map(|pictured| pictured.albums.clone())
    }

    pub(crate) fn keep_pictures(
        &self,
        at: PicturesAt,
        playlist: PlaylistId,
        at_most: usize,
        albums: &[AlbumId],
        reads_listening: bool,
    ) {
        let pictured = Pictured {
            albums: albums.to_vec(),
            heard: reads_listening.then_some(at.heard),
        };
        let mut kept = self.pictured.lock();
        match kept.as_mut().filter(|kept| kept.at.holds(at)) {
            Some(kept) => {
                kept.by_playlist.insert((playlist, at_most), pictured);
            }
            None => {
                *kept = Some(KeptPictures {
                    at,
                    by_playlist: AHashMap::from_iter([((playlist, at_most), pictured)]),
                });
            }
        }
    }

    fn heard(&self) -> u64 {
        self.heard.load(Ordering::Acquire)
    }

    fn plans_stamp(&self) -> CatalogStamp {
        self.stamped_by(&self.planned)
    }

    fn stamped_by(&self, counter: &AtomicU64) -> CatalogStamp {
        CatalogStamp {
            named: counter.load(Ordering::Acquire),
            written_elsewhere: self.written_elsewhere(),
        }
    }

    fn written_elsewhere(&self) -> Option<i64> {
        let writer = self.writer.try_lock()?;
        writer
            .query_row("PRAGMA data_version", [], |row| row.get(0))
            .inspect_err(|error| tracing::debug!(%error, "the catalog's data version went unread"))
            .ok()
    }

    fn vocabulary(&self) -> Result<Arc<Spellings>> {
        let stamp = self.names_stamp();
        if let Some(kept) = self
            .spellings
            .lock()
            .as_ref()
            .filter(|kept| kept.stamp.still_holds_at(stamp))
        {
            return Ok(Arc::clone(&kept.spellings));
        }

        let spellings = Arc::new(self.read(store::spellings)?);
        *self.spellings.lock() = Some(KeptVocabulary {
            stamp,
            spellings: Arc::clone(&spellings),
        });
        Ok(spellings)
    }
}

const NAMED_TABLES: [&str; 4] = ["albums", "artists", "artist_genres", "track_credits"];
const NAMES_MOVED: &str = "names_moved";
const PLANS_MOVED: &str = "plans_moved";

const PLANNED_FROM: [(&str, Option<&[&str]>); 8] = [
    (
        "tracks",
        Some(&[
            "root_id",
            "path",
            "span_start",
            "span_frames",
            "title",
            "artist",
            "artist_mbid",
            "album_id",
            "track_number",
            "disc_number",
            "codec",
            "sample_rate",
            "channels",
            "sample_format",
            "file_size",
            "modified",
            "mbid",
            "release_track_mbid",
            "isrc",
            "vault_key",
            "answered",
        ]),
    ),
    (
        "albums",
        Some(&[
            "title",
            "release_title",
            "artist_id",
            "year",
            "cover_art",
            "cover_key",
            "cover_path",
            "mbid",
            "release_group",
            "date",
            "label",
            "catalog_number",
            "barcode",
            "answered",
        ]),
    ),
    ("artists", Some(&["name", "mbid", "answered"])),
    ("roots", Some(&["path"])),
    ("release_tracks", None),
    ("release_media", None),
    ("vault_objects", None),
    ("vault_refused", None),
];
const TEMPORARY: &str = "temp";

const NAMES_MOVED_TRIGGERS: &str = "
CREATE TEMP TABLE names_moved (
    id    INTEGER PRIMARY KEY CHECK (id = 1),
    times INTEGER NOT NULL
);
INSERT INTO temp.names_moved (id, times) VALUES (1, 0);

CREATE TEMP TRIGGER track_named AFTER INSERT ON main.tracks
BEGIN UPDATE temp.names_moved SET times = times + 1; END;
CREATE TEMP TRIGGER track_unnamed AFTER DELETE ON main.tracks
BEGIN UPDATE temp.names_moved SET times = times + 1; END;
CREATE TEMP TRIGGER track_renamed AFTER UPDATE OF title, artist, genre ON main.tracks
WHEN OLD.title IS NOT NEW.title OR OLD.artist IS NOT NEW.artist OR OLD.genre IS NOT NEW.genre
BEGIN UPDATE temp.names_moved SET times = times + 1; END;

CREATE TEMP TRIGGER album_named AFTER INSERT ON main.albums
BEGIN UPDATE temp.names_moved SET times = times + 1; END;
CREATE TEMP TRIGGER album_unnamed AFTER DELETE ON main.albums
BEGIN UPDATE temp.names_moved SET times = times + 1; END;
CREATE TEMP TRIGGER album_renamed AFTER UPDATE OF title ON main.albums
WHEN OLD.title IS NOT NEW.title
BEGIN UPDATE temp.names_moved SET times = times + 1; END;

CREATE TEMP TRIGGER artist_named AFTER INSERT ON main.artists
BEGIN UPDATE temp.names_moved SET times = times + 1; END;
CREATE TEMP TRIGGER artist_unnamed AFTER DELETE ON main.artists
BEGIN UPDATE temp.names_moved SET times = times + 1; END;
CREATE TEMP TRIGGER artist_renamed AFTER UPDATE OF name ON main.artists
WHEN OLD.name IS NOT NEW.name
BEGIN UPDATE temp.names_moved SET times = times + 1; END;

CREATE TEMP TRIGGER genre_named AFTER INSERT ON main.artist_genres
BEGIN UPDATE temp.names_moved SET times = times + 1; END;
CREATE TEMP TRIGGER genre_unnamed AFTER DELETE ON main.artist_genres
BEGIN UPDATE temp.names_moved SET times = times + 1; END;
CREATE TEMP TRIGGER genre_renamed AFTER UPDATE ON main.artist_genres
BEGIN UPDATE temp.names_moved SET times = times + 1; END;
";

const TITLES_WORDED_TRIGGERS: &str = "
CREATE TEMP TRIGGER track_worded AFTER INSERT ON main.tracks
BEGIN UPDATE main.tracks SET title_words = words_of(NEW.title) WHERE id = NEW.id; END;
CREATE TEMP TRIGGER track_reworded AFTER UPDATE OF title ON main.tracks
WHEN OLD.title IS NOT NEW.title OR NEW.title_words IS NULL
BEGIN UPDATE main.tracks SET title_words = words_of(NEW.title) WHERE id = NEW.id; END;
";

fn plans_moved_triggers() -> String {
    let mut sql = format!(
        "CREATE TEMP TABLE {PLANS_MOVED} (
             id    INTEGER PRIMARY KEY CHECK (id = 1),
             times INTEGER NOT NULL
         );
         INSERT INTO temp.{PLANS_MOVED} (id, times) VALUES (1, 0);"
    );
    let bump = format!("BEGIN UPDATE temp.{PLANS_MOVED} SET times = times + 1; END;");
    for (table, columns) in PLANNED_FROM {
        for (event, named) in [("INSERT", "added"), ("DELETE", "taken")] {
            sql.push_str(&format!(
                "CREATE TEMP TRIGGER {table}_{named}_under_a_plan AFTER {event} ON main.{table} {bump}"
            ));
        }
        let moved = match columns {
            Some(columns) => format!(
                "AFTER UPDATE OF {} ON main.{table} WHEN {}",
                columns.join(", "),
                columns
                    .iter()
                    .map(|column| format!("OLD.{column} IS NOT NEW.{column}"))
                    .collect::<Vec<_>>()
                    .join(" OR ")
            ),
            None => format!("AFTER UPDATE ON main.{table}"),
        };
        sql.push_str(&format!(
            "CREATE TEMP TRIGGER {table}_moved_under_a_plan {moved} {bump}"
        ));
    }
    sql
}

struct Counters {
    named: Arc<AtomicU64>,
    written: Arc<AtomicU64>,
    heard: Arc<AtomicU64>,
    planned: Arc<AtomicU64>,
    renamed: Arc<Mutex<Renamed>>,
}

const ROWS_MOVED: &str = "rows_moved";
const HEARD_MOVED: &str = "heard_moved";
const NEVER_LISTED: [&str; 4] = ["seen", "asked", "asks", "refusals"];
const LISTENING: [&str; 3] = ["plays", "played", "favourite"];

fn moved_when(columns: &[String]) -> String {
    format!(
        "AFTER UPDATE OF {} ON main.tracks WHEN {}",
        columns.join(", "),
        columns
            .iter()
            .map(|column| format!("OLD.{column} IS NOT NEW.{column}"))
            .collect::<Vec<_>>()
            .join(" OR ")
    )
}

fn track_rows_moved_triggers(connection: &Connection) -> Result<String> {
    let columns = connection
        .prepare("SELECT name FROM pragma_table_xinfo('tracks') WHERE hidden = 0")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .map_err(|source| Error::store(StoreOp::Open, source))?;
    let (heard, listed): (Vec<String>, Vec<String>) = columns
        .into_iter()
        .filter(|column| !NEVER_LISTED.contains(&column.as_str()))
        .partition(|column| LISTENING.contains(&column.as_str()));

    let mut sql = String::new();
    for table in [ROWS_MOVED, HEARD_MOVED] {
        sql.push_str(&format!(
            "CREATE TEMP TABLE {table} (
                 id    INTEGER PRIMARY KEY CHECK (id = 1),
                 times INTEGER NOT NULL
             );
             INSERT INTO temp.{table} (id, times) VALUES (1, 0);"
        ));
    }
    let rows_moved = format!("BEGIN UPDATE temp.{ROWS_MOVED} SET times = times + 1; END;");
    for (event, named) in [("INSERT", "added"), ("DELETE", "taken")] {
        sql.push_str(&format!(
            "CREATE TEMP TRIGGER track_row_{named} AFTER {event} ON main.tracks {rows_moved}"
        ));
    }
    sql.push_str(&format!(
        "CREATE TEMP TRIGGER track_row_moved {} {rows_moved}",
        moved_when(&listed)
    ));
    sql.push_str(&format!(
        "CREATE TEMP TRIGGER track_heard {} BEGIN UPDATE temp.{HEARD_MOVED} SET times = times + 1; END;",
        moved_when(&heard)
    ));
    Ok(sql)
}

fn watch_the_names(connection: &mut Connection, counters: Counters) -> Result<()> {
    for triggers in [
        NAMES_MOVED_TRIGGERS,
        &plans_moved_triggers(),
        RENAMED_TRIGGERS,
        &track_rows_moved_triggers(connection)?,
    ] {
        connection
            .execute_batch(triggers)
            .map_err(|source| Error::store(StoreOp::Open, source))?;
    }

    let Counters {
        named,
        written,
        heard,
        planned,
        renamed,
    } = counters;
    connection
        .update_hook(Some(
            move |action: Action, database: &str, table: &str, row: i64| {
                if database == TEMPORARY && table == NAMES_MOVED {
                    named.fetch_add(1, Ordering::AcqRel);
                } else if database == TEMPORARY && table == PLANS_MOVED {
                    planned.fetch_add(1, Ordering::AcqRel);
                } else if database == TEMPORARY && table == ROWS_MOVED {
                    written.fetch_add(1, Ordering::AcqRel);
                } else if database == TEMPORARY && table == HEARD_MOVED {
                    heard.fetch_add(1, Ordering::AcqRel);
                } else if database == TEMPORARY
                    && action == Action::SQLITE_INSERT
                    && (table == RETITLED || table == ADDED)
                {
                    let at = named.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
                    renamed.lock().note(at, row, table == ADDED);
                } else if NAMED_TABLES.contains(&table) {
                    written.fetch_add(1, Ordering::AcqRel);
                }
            },
        ))
        .map_err(|source| Error::store(StoreOp::Open, source))
}

fn connect(source: &Source, role: schema::Role) -> Result<Connection> {
    let connection = match source {
        Source::File(path) => Connection::open(path),
        Source::Memory => Connection::open_in_memory(),
    }
    .map_err(|source| Error::store(StoreOp::Open, source))?;

    schema::configure(&connection, role)?;
    connection.set_prepared_statement_cache_capacity(store::STATEMENTS_CACHED);
    Ok(connection)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WrittenElsewhere(i64);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HiddenTracks {
    pub counted: u64,
    pub tracks: Vec<Track>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Applying {
    Yes,
    No,
}

impl Applying {
    fn forgets(
        self,
        vault: &Vault,
        path: &Path,
    ) -> std::result::Result<bool, resonate_vault::Error> {
        match self {
            Self::Yes => vault.forget(path),
            Self::No => Ok(path.exists()),
        }
    }
}

pub struct Library {
    inner: Arc<Inner>,
}

impl Library {
    pub fn open(path: &Path) -> Result<Self> {
        Self::build(Source::File(path.to_path_buf()), None)
    }

    pub fn open_with_vault(path: &Path, vault: Arc<Vault>) -> Result<Self> {
        Self::build(Source::File(path.to_path_buf()), Some(vault))
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::build(Source::Memory, None)
    }

    pub fn open_in_memory_with_vault(vault: Arc<Vault>) -> Result<Self> {
        Self::build(Source::Memory, Some(vault))
    }

    pub fn vault(&self) -> Option<&Arc<Vault>> {
        self.inner.vault.as_ref()
    }

    pub fn trash_into(&self, data_home: Option<PathBuf>) {
        *self.inner.trash_home.lock() = data_home;
    }

    pub fn trash_through(&self, trashes: Option<Arc<dyn Trashes>>) {
        *self.inner.trashes.lock() = trashes;
    }

    pub fn music_filters(&self) -> MusicFilters {
        *self.inner.music_filters.lock()
    }

    pub fn filter_music(&self, filters: MusicFilters) {
        *self.inner.music_filters.lock() = filters;
        *self.inner.suggested.lock() = None;
        *self.inner.filed.lock() = None;
        *self.inner.measured.lock() = None;
        *self.inner.pictured.lock() = None;
    }

    pub fn deliver_into(&self, folder: Option<DeliveryFolder>) {
        *self.inner.delivering_into.lock() = folder;
    }

    pub fn stall_downloads_by(&self, gate: Gate) {
        *self.inner.stalling.lock() = gate;
    }

    pub(crate) fn download_gate(&self) -> Gate {
        self.inner.stalling.lock().clone()
    }

    pub(crate) fn delivering_into(&self) -> Option<DeliveryFolder> {
        self.inner.delivering_into.lock().clone()
    }

    pub(crate) fn album_to_file(&self, album: AlbumId) -> Result<AlbumToFile> {
        self.inner.read(|connection| {
            connection
                .query_row(
                    "SELECT albums.title, artists.name, albums.year,
                            (SELECT count(DISTINCT disc) FROM release_tracks
                              WHERE release_tracks.album_id = albums.id)
                       FROM albums LEFT JOIN artists ON artists.id = albums.artist_id
                      WHERE albums.id = ?1",
                    params![album.get() as i64],
                    |row| {
                        Ok(AlbumToFile {
                            title: row.get(0)?,
                            owner: row.get(1)?,
                            year: row.get(2)?,
                            discs: row.get::<_, i64>(3).map(|discs| discs.max(1) as u32)?,
                        })
                    },
                )
                .map_err(|source| Error::store(StoreOp::Query, source))
        })
    }

    pub(crate) fn root_reaching(&self, path: &Path) -> Result<Option<PathBuf>> {
        let roots = self.inner.read(|connection| {
            rows(connection, "SELECT path FROM roots", Vec::new(), |row| {
                row.get::<_, String>(0)
                    .map(|path| Ok(store::path_of(&path)))
            })
        })?;
        Ok(roots
            .into_iter()
            .filter(|root| path.starts_with(root))
            .max_by_key(|root| root.components().count()))
    }

    pub(crate) fn claim_album_keys(&self, album: AlbumId, keys: &[String]) -> Result<()> {
        self.inner.write(|transaction| {
            for key in keys {
                transaction
                    .execute(
                        "INSERT INTO album_keys (key, album_id) VALUES (?1, ?2)
                         ON CONFLICT(key) DO NOTHING",
                        params![key, album.get() as i64],
                    )
                    .map_err(|source| Error::store(StoreOp::Insert, source))?;
            }
            Ok(())
        })
    }

    pub(crate) fn pair_what_landed(&self) -> Result<u64> {
        let offered = self.inner.read(|connection| {
            rows(
                connection,
                "SELECT release_tracks.id, wants.offered
                   FROM wants JOIN release_tracks ON release_tracks.id = wants.release_track_id
                  WHERE release_tracks.track_id IS NULL AND wants.offered IS NOT NULL",
                Vec::new(),
                |row| Ok(Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))),
            )
        })?;
        let landed: Vec<(i64, String)> = offered
            .into_iter()
            .filter_map(|(release_track, uri)| {
                let location = MediaLocation::from_uri(&uri)?;
                let path = location.as_path()?.to_str()?.to_owned();
                Some((release_track, path))
            })
            .collect();
        if landed.is_empty() {
            return Ok(0);
        }

        self.inner.write(|transaction| {
            let mut paired = 0;
            for (release_track, path) in &landed {
                paired += transaction
                    .execute(
                        "UPDATE release_tracks
                            SET track_id = (SELECT id FROM tracks
                                             WHERE path = ?2 AND span_start = 0
                                               AND root_id IS NOT NULL)
                          WHERE id = ?1 AND track_id IS NULL
                            AND EXISTS (SELECT 1 FROM tracks
                                         WHERE path = ?2 AND span_start = 0
                                           AND root_id IS NOT NULL)",
                        params![release_track, path],
                    )
                    .map_err(|source| Error::store(StoreOp::Update, source))?
                    as u64;
            }
            Ok(paired)
        })
    }

    pub fn sources(&self) -> Sources {
        match self.vault() {
            Some(vault) => Sources::local()
                .and(Arc::new(VaultFiles::over(vault)))
                .standing_in(self.stand_in()),
            None => Sources::local(),
        }
    }

    pub fn stand_in(&self) -> Arc<dyn StandIn> {
        Arc::new(Vaulted::over(Arc::clone(&self.inner)))
    }

    pub fn hinting(&self) -> Arc<dyn Hinting> {
        Arc::new(Hinted::over(Arc::clone(&self.inner)))
    }

    fn build(source: Source, vault: Option<Arc<Vault>>) -> Result<Self> {
        let mut writer = connect(&source, schema::Role::Writing)?;
        schema::lay_out(&writer)?;
        writer
            .execute_batch(TITLES_WORDED_TRIGGERS)
            .map_err(|source| Error::store(StoreOp::Open, source))?;
        store::reconcile_artists(&mut writer)?;
        store::refold_the_index(&mut writer)?;
        store::compose_the_album_keys(&mut writer)?;
        store::word_the_titles(&mut writer)?;
        store::settle_the_credits_if_owed(&mut writer)?;

        let named = Arc::new(AtomicU64::new(0));
        let written = Arc::new(AtomicU64::new(0));
        let heard = Arc::new(AtomicU64::new(0));
        let planned = Arc::new(AtomicU64::new(0));
        let renamed = Arc::new(Mutex::new(Renamed::default()));
        watch_the_names(
            &mut writer,
            Counters {
                named: Arc::clone(&named),
                written: Arc::clone(&written),
                heard: Arc::clone(&heard),
                planned: Arc::clone(&planned),
                renamed: Arc::clone(&renamed),
            },
        )?;

        Ok(Self {
            inner: Arc::new(Inner {
                source,
                writer: Mutex::new(writer),
                readers: Mutex::new(Pool::default()),
                freed: Condvar::new(),
                walking: AtomicBool::new(false),
                walking_pass: Mutex::new(None),
                walk_ended: Condvar::new(),
                vault,
                playlists: AtomicU64::new(0),
                arrivals: AtomicU64::new(0),
                named,
                written,
                heard,
                planned,
                renamed,
                spellings: Mutex::new(None),
                suggested: Mutex::new(None),
                filed: Mutex::new(None),
                measured: Mutex::new(None),
                pictured: Mutex::new(None),
                playing: Mutex::new(None),
                steps: Mutex::new(Vec::new()),
                walked: Mutex::new(Vec::new()),
                offering: AtomicBool::new(false),
                keeping: AtomicBool::new(false),
                a_poll_waits_on_a_keep: AtomicBool::new(false),
                delivering_into: Mutex::new(None),
                stalling: Mutex::new(Gate::open()),
                trash_home: Mutex::new(None),
                trashes: Mutex::new(None),
                music_filters: Mutex::new(MusicFilters::default()),
                listens_settle_after: Mutex::new(synced::LISTEN_SETTLES_AFTER),
                foresight: Foresight::default(),
            }),
        })
    }

    pub(crate) fn inner(&self) -> &Inner {
        &self.inner
    }

    pub(crate) fn foresight(&self) -> &Foresight {
        &self.inner.foresight
    }

    pub(crate) fn shared(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

struct KeptPlace {
    row: usize,
    at: Frames,
    shuffle: bool,
    next: Option<Span>,
}

fn kept_span(first: Option<i64>, last: Option<i64>) -> Option<Span> {
    let first = usize::try_from(first?).ok()?;
    let last = usize::try_from(last?).ok()?;
    Some(Span::between(first, last))
}

fn keep_order(
    transaction: &rusqlite::Transaction<'_>,
    zone: ZoneId,
    order: &[usize],
    kept: KeptPlace,
) -> Result<()> {
    let zoned = i64::from(zone.get());
    transaction
        .execute("DELETE FROM resume_order WHERE zone = ?1", [zoned])
        .map_err(|source| Error::store(StoreOp::Delete, source))?;

    for (position, loaded_at) in order.iter().enumerate() {
        store::cached(
            transaction,
            "INSERT INTO resume_order (zone, position, loaded_at) VALUES (?1, ?2, ?3)",
            params![
                zoned,
                i64::try_from(position).unwrap_or(i64::MAX),
                i64::try_from(*loaded_at).unwrap_or(i64::MAX)
            ],
        )
        .map_err(|source| Error::store(StoreOp::Insert, source))?;
    }

    keep_place(transaction, zone, kept.row, kept.at)?;
    let next = kept.next.map(|next| {
        (
            i64::try_from(next.first()).unwrap_or(i64::MAX),
            i64::try_from(next.last()).unwrap_or(i64::MAX),
        )
    });
    transaction
        .execute(
            "UPDATE resume SET shuffle = ?1, next_first = ?2, next_last = ?3 WHERE zone = ?4",
            params![
                kept.shuffle,
                next.map(|(first, _)| first),
                next.map(|(_, last)| last),
                zoned
            ],
        )
        .map(drop)
        .map_err(|source| Error::store(StoreOp::Update, source))
}

fn keep_place(
    transaction: &rusqlite::Transaction<'_>,
    zone: ZoneId,
    row: usize,
    at: Frames,
) -> Result<()> {
    transaction
        .execute(
            "INSERT INTO resume (zone, row, at, taken)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(zone) DO UPDATE SET
                 row   = excluded.row,
                 at    = excluded.at,
                 taken = excluded.taken",
            params![
                i64::from(zone.get()),
                i64::try_from(row).unwrap_or(i64::MAX),
                i64::try_from(at.get()).unwrap_or(i64::MAX),
                store::to_nanos(SystemTime::now())
            ],
        )
        .map(drop)
        .map_err(|source| Error::store(StoreOp::Insert, source))
}

fn note_the_delivered_object(
    transaction: &Transaction<'_>,
    kept: &VaultKept,
    held: &str,
    offered: &str,
    took: i64,
) -> Result<()> {
    let key = kept.key.to_string();
    transaction
        .execute(
            "INSERT INTO vault_objects (key, form, path, bytes, sample_rate, channels,
                    sample_format, frames, taken_from, took, was_bytes, was_codec,
                    encoding, validated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?4, ?11, ?12, 1)
             ON CONFLICT(key) DO NOTHING",
            params![
                key,
                store::vault_form_code(kept.form),
                held,
                kept.bytes as i64,
                kept.spec.rate.hz(),
                kept.spec.channel_count().get(),
                store::format_code(kept.spec.format),
                kept.frames.get() as i64,
                offered,
                took,
                store::codec_code(kept.codec),
                i64::from(weighed_under(kept, false).get()),
            ],
        )
        .map_err(|source| Error::store(StoreOp::Insert, source))?;
    transaction
        .execute(
            NOTE_WHERE_AN_OBJECT_WAS_DELIVERED_FROM,
            params![key, offered, took],
        )
        .map(drop)
        .map_err(|source| Error::store(StoreOp::Insert, source))
}

const fn weighed_under(kept: &VaultKept, renewing: bool) -> Encoding {
    if kept.deduped && !renewing {
        Encoding::UNRECORDED
    } else {
        Encoding::OF_THIS_BUILD
    }
}

fn rooted(roots: &[PathBuf]) -> Result<Vec<Value>> {
    let mut named = Vec::with_capacity(roots.len());
    for root in roots {
        named.push(Value::Text(store::path_text(root).into_owned()));
    }
    Ok(named)
}

const PATHS_ASKED_AT_ONCE: usize = 512;

fn tracks_among(roots: usize, paths: usize) -> String {
    let among = vec!["?"; paths].join(", ");
    match roots {
        0 => format!(
            "SELECT DISTINCT tracks.path FROM tracks JOIN roots ON roots.id = tracks.root_id
              WHERE tracks.path IN ({among})"
        ),
        held => format!(
            "SELECT DISTINCT tracks.path FROM tracks JOIN roots ON roots.id = tracks.root_id
              WHERE roots.path IN ({}) AND tracks.path IN ({among})",
            vec!["?"; held].join(", ")
        ),
    }
}

fn and_roots(select: &str, roots: usize) -> String {
    match roots {
        0 => format!("{select}{FILED_IN_ORDER}"),
        held => format!(
            "{select} AND roots.path IN ({}){FILED_IN_ORDER}",
            vec!["?"; held].join(", ")
        ),
    }
}

type FilingRow = (String, String, Option<i64>, Option<i64>);

fn read_filing(row: &Row<'_>) -> rusqlite::Result<FilingRow> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
}

pub(crate) struct ReleaseTotals {
    tracks: AHashMap<(i64, i64), u32>,
    discs: AHashMap<i64, u32>,
}

fn paged_under_roots(select: &str, roots: usize, after: bool) -> String {
    let mut clauses = Vec::with_capacity(2);
    if roots > 0 {
        clauses.push(format!("roots.path IN ({})", vec!["?"; roots].join(", ")));
    }
    if after {
        clauses.push("tracks.path > ?".to_owned());
    }
    let filter = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    format!("{select}{filter}{FILED_IN_ORDER} LIMIT ?")
}

fn under_roots(select: &str, roots: usize) -> String {
    match roots {
        0 => format!("{select}{FILED_IN_ORDER}"),
        held => format!(
            "{select} WHERE roots.path IN ({}){FILED_IN_ORDER}",
            vec!["?"; held].join(", ")
        ),
    }
}

fn counted_by_album(connection: &Connection, sql: &str) -> Result<AHashMap<i64, u32>> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let counted = statement
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    Ok(counted
        .into_iter()
        .map(|(album, held)| (album, u32::try_from(held).unwrap_or_default()))
        .collect())
}

fn counted_by_disc(connection: &Connection, sql: &str) -> Result<AHashMap<(i64, i64), u32>> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let counted = statement
        .query_map([], |row| {
            Ok((
                (row.get::<_, i64>(0)?, row.get::<_, i64>(1)?),
                row.get::<_, i64>(2)?,
            ))
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    Ok(counted
        .into_iter()
        .map(|(seat, held)| (seat, u32::try_from(held).unwrap_or_default()))
        .collect())
}
