use std::{
    num::NonZeroU32,
    ops::Deref,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ahash::{AHashMap, AHashSet};
use parking_lot::{Condvar, Mutex};
use resonate_codec::{Hinting, Sources, StandIn};
use resonate_core::{
    AlbumId, ArtistId, ChannelCount, ChannelLayout, Chromaprint, FrameSpan, Frames, ListenId,
    MediaLocation, PlaylistId, QueueStamp, ReleaseTrackId, Reordered, Resumable, Resumption,
    SampleFormat, SampleRate, Span, StreamSpec, TrackId, WantId,
};
use resonate_providers::Providers;
use resonate_vault::{Encoding, Kept as VaultKept, VaultFiles};
use rusqlite::{
    Connection, OptionalExtension as _, Row, hooks::Action, params, params_from_iter, types::Value,
};

use crate::{
    Album, AlbumOrder, AlbumQuery, AlbumToAsk, Artist, ArtistDetail, ArtistOrder, ArtistProfile,
    ArtistQuery, ArtistRelease, ArtistToAsk, ArtistTotals, Asked, Certainty, Clause, Codec, Column,
    Compare, Condition, Counted, CoverArt, Cut, Day, Direction, EnrichHandle, EnrichOptions, Error,
    Exported, Favoured, Fingerprinters, Found, Fruitless, Genre, HeldMedium, HeldReleaseTrack,
    Holdings, ImageFormat, ImportHandle, ImportOptions, Imported, Isrc, Kept, KeptCorrection,
    KeptCover, KeptIndex, KeptLyrics, LifeSpan, Link, Mbid, Measured, Missing, MissingTrack,
    MostListened, Move, NamedPlaylist, OrganiseHandle, OrganiseOptions, Playing, Playlist,
    PlaylistEntry, PlaylistOrder, PollHandle, PollOptions, PortraitWanted, Pruned, Recording,
    RecordingRelease, Reference, Release, ReleaseDetail, ReleaseGroup, Released, Result,
    RetagHandle, RetagOptions, RowOrder, SavedQuery, ScanHandle, ScanOptions, Search,
    SearchResults, Shape, Shared, SortOrder, Spellings, Statistics, StoreOp, Study, Suggestion,
    Sung, TagSink, Term, Track, TrackQuery, TrackToAsk, Undoable, Unfinished, UnheldRelease, Vault,
    VaultKey, VaultObject, Verdict, Waits, Want, Window, Word, elsewhere, enrich, enriched,
    hinted::Hinted,
    import,
    model::CoverWanted,
    organise::{self, TrackToFile},
    playlist,
    retag::{self, Followed, TrackToTag},
    scan, schema, search, share, spelling, statistics, store,
    studies::{self, Agreement, Heard, HeardAs, Studied, StudiedTrack, StudyFilter, ToStudy},
    suggest, supply,
    undo::{self, Step},
    vaulted::Vaulted,
};

const READER_POOL: usize = 8;

const INDEX_JOIN: &str = " JOIN tracks_fts ON tracks_fts.rowid = tracks.id";

const INDEX_LOOKUP: &str = "tracks.id IN (SELECT rowid FROM tracks_fts WHERE tracks_fts MATCH ?)";

const LISTENS_SINCE: &str =
    "(SELECT count(*) FROM listens WHERE listens.track_id = tracks.id AND listens.at >= ?)";

const NOTHING_MATCHES: &str = "0";

const A_FAVOURITE_TRACK: &str = "tracks.favourite IS NOT NULL";

fn studied_as(column: &str, value: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM track_studies s WHERE s.track_id = tracks.id AND s.{column} = '{value}')"
    )
}

const A_FAVOURITE_ALBUM: &str = "a.favourite IS NOT NULL";

const A_FAVOURITE_ARTIST: &str = "r.favourite IS NOT NULL";

const THE_FAVOURITES_SEARCH: &str = "is:favourite";

const THE_BEST_COPY: &str = "+tracks.alternative_of IS NULL AND tracks.hidden = 0";

const HOLDS_A_BEST_COPY: &str = "EXISTS (SELECT 1 FROM tracks t
      WHERE t.album_id = a.id AND t.alternative_of IS NULL AND t.hidden = 0)";

const CD_SAMPLE_RATE: u32 = 44_100;

const CD_SAMPLE_DEPTH: u8 = 16;

pub(crate) const TRACK_COLUMNS: &str =
    "tracks.id, tracks.path, tracks.title, tracks.artist, tracks.album_id,
     tracks.track_number, tracks.disc_number, tracks.duration, tracks.sample_rate, tracks.channels,
     tracks.sample_format, tracks.codec, tracks.rg_track_gain, tracks.rg_track_peak,
     tracks.rg_album_gain, tracks.rg_album_peak, tracks.file_size, tracks.modified, tracks.added,
     tracks.plays, tracks.played, tracks.span_start, tracks.span_frames, tracks.artist_id,
     tracks.favourite, tracks.genre,
     (SELECT count(*) FROM tracks x WHERE x.alternative_of = tracks.id)";

pub(crate) const BESIDE_A_TRACK: usize = listed(TRACK_COLUMNS);

const fn listed(columns: &str) -> usize {
    let bytes = columns.as_bytes();
    let mut counted = 1;
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b',' {
            counted += 1;
        }
        at += 1;
    }
    counted
}

macro_rules! album_title {
    () => {
        "coalesce(a.release_title, a.title)"
    };
}

macro_rules! album_owner {
    () => {
        "(SELECT r.name FROM artists r WHERE r.id = a.artist_id)"
    };
}

macro_rules! album_tracks {
    () => {
        "(SELECT count(*) FROM tracks t WHERE t.album_id = a.id AND t.alternative_of IS NULL AND t.hidden = 0)"
    };
}

macro_rules! album_added {
    () => {
        "(SELECT max(t.added) FROM tracks t WHERE t.album_id = a.id)"
    };
}

macro_rules! artist_albums {
    () => {
        "(SELECT count(DISTINCT t.album_id) FROM tracks t
           WHERE t.artist_id = r.id AND t.alternative_of IS NULL AND t.hidden = 0)"
    };
}

macro_rules! artist_tracks {
    () => {
        "(SELECT count(*) FROM tracks t WHERE t.artist_id = r.id AND t.alternative_of IS NULL AND t.hidden = 0)"
    };
}

macro_rules! unheld_by_any_album {
    () => {
        "NOT EXISTS (SELECT 1 FROM albums a WHERE a.release_group = r.mbid)"
    };
}

const ALBUM_COLUMNS: &str = concat!(
    "a.id, ",
    album_title!(),
    ", a.artist_id, a.year, ",
    album_tracks!(),
    ",
     (SELECT count(DISTINCT t.artist_id) FROM tracks t WHERE t.album_id = a.id),
     (a.cover_art IS NOT NULL OR a.cover_path IS NOT NULL), a.mbid,
     (SELECT count(*) FROM release_tracks rt WHERE rt.album_id = a.id AND rt.track_id IS NULL), ",
    album_owner!(),
    ", a.favourite"
);

const ARTIST_COLUMNS: &str = concat!(
    "r.id, r.name, ",
    artist_albums!(),
    ", ",
    artist_tracks!(),
    ", r.mbid, r.portrait IS NOT NULL, r.favourite"
);

const LINKS_OF_UNPICTURED_ARTISTS: &str = "SELECT l.artist_id, l.relation, l.provider, l.url
       FROM artist_links l JOIN artists r ON r.id = l.artist_id
      WHERE r.portrait IS NULL
      ORDER BY l.artist_id, l.url";

const WHAT_AN_ARTIST_HOLDS: &str = "SELECT count(DISTINCT album_id), count(*),
            sum(duration * 1.0 / sample_rate), sum(plays), max(played)
       FROM tracks WHERE artist_id = ?1 AND alternative_of IS NULL AND hidden = 0";

const RELEASE_TRACK_COLUMNS: &str = "id, disc, position, number, title, artist, recording_mbid,
     track_mbid, length_ms, isrc, track_id";

const LINKS_OF_RELEASE_TRACKS: &str = "SELECT release_track_id, relation, provider, url
       FROM release_track_links
      WHERE release_track_id IN (SELECT id FROM release_tracks WHERE album_id = ?1)
      ORDER BY release_track_id, url";

const LINKS_OF_WANTS: &str = "SELECT release_track_id, relation, provider, url
       FROM release_track_links
      WHERE release_track_id IN (SELECT release_track_id FROM wants)
      ORDER BY release_track_id, url";

const LINKS_OF_WANTED_RELEASES: &str = "SELECT album_id, relation, provider, url
       FROM album_links
      WHERE album_id IN (SELECT rt.album_id
                           FROM wants w
                           JOIN release_tracks rt ON rt.id = w.release_track_id)
      ORDER BY album_id, url";

const TRACKS_TO_FILE: &str = concat!(
    "SELECT tracks.id, tracks.path, roots.path, tracks.title,
            tracks.artist, tracks.album_id, ",
    album_title!(),
    ", a.year, artists.name,
            tracks.track_number, tracks.disc_number
       FROM tracks
       JOIN roots ON roots.id = tracks.root_id
       LEFT JOIN albums a ON a.id = tracks.album_id
       LEFT JOIN artists ON artists.id = a.artist_id"
);

const FILED_IN_ORDER: &str = " ORDER BY tracks.path, tracks.span_start";

const TRACKS_TO_VAULT: &str = "SELECT tracks.id, tracks.path, tracks.span_start,
            tracks.span_frames, tracks.file_size, tracks.codec, tracks.sample_rate,
            tracks.channels, tracks.sample_format, tracks.album_id,
            tracks.vault_key IS NOT NULL
       FROM tracks
       JOIN roots ON roots.id = tracks.root_id
       LEFT JOIN vault_objects ON vault_objects.key = tracks.vault_key
      WHERE (tracks.vault_key IS NULL OR coalesce(vault_objects.encoding, 0) < ?)";

const TRACKS_IN_THE_VAULT: &str = "SELECT tracks.id, tracks.path
       FROM tracks
       JOIN roots ON roots.id = tracks.root_id
      WHERE tracks.vault_key IS NOT NULL";

const VAULT_OBJECTS: &str = "SELECT key, form, path, bytes, sample_rate, channels,
            sample_format, frames, taken_from, took, was_bytes, was_codec, validated, encoding
       FROM vault_objects ORDER BY path";

const VAULT_OBJECTS_UNHELD: &str = "SELECT key, form, path, bytes, sample_rate, channels,
            sample_format, frames, taken_from, took, was_bytes, was_codec, validated, encoding
       FROM vault_objects
      WHERE key NOT IN (SELECT vault_key FROM tracks WHERE vault_key IS NOT NULL)
      ORDER BY path";

const TRACKS_TO_TAG: &str = "SELECT tracks.id, tracks.path, tracks.span_start, tracks.span_frames,
            tracks.answered IS NOT NULL, tracks.title, tracks.artist, tracks.artist_mbid,
            tracks.mbid, tracks.release_track_mbid, tracks.isrc,
            tracks.track_number, tracks.disc_number, tracks.album_id,
            a.answered IS NOT NULL, a.release_title,
            artists.answered IS NOT NULL, artists.name, artists.mbid,
            a.mbid, a.release_group, a.date, a.label, a.catalog_number, a.barcode,
            tracks.vault_key IS NOT NULL
       FROM tracks
       JOIN roots ON roots.id = tracks.root_id
       LEFT JOIN albums a ON a.id = tracks.album_id
       LEFT JOIN artists ON artists.id = a.artist_id";

const RELEASE_DISC_TRACKS: &str = "SELECT album_id, disc, count(*) FROM release_tracks
      GROUP BY album_id, disc";

const RELEASE_MEDIA: &str = "SELECT album_id, count(*) FROM release_media GROUP BY album_id";

const FIRST_DISC: i64 = 1;

const ALBUM_DISCS: &str = "SELECT album_id, max(disc_number) FROM tracks
      WHERE album_id IS NOT NULL AND disc_number IS NOT NULL
      GROUP BY album_id";

const WANTS: &str = concat!(
    "SELECT w.id, w.release_track_id, rt.album_id, ",
    album_title!(),
    ", rt.title, rt.artist,
            w.wanted, w.tried, w.offered,
            rt.recording_mbid, rt.track_mbid, a.mbid, rt.isrc, rt.length_ms, rt.disc, rt.position,
            rt.track_id
       FROM wants w
       JOIN release_tracks rt ON rt.id = w.release_track_id
       JOIN albums a ON a.id = rt.album_id
      ORDER BY w.wanted DESC, w.id DESC"
);

const MISSING_TRACKS: &str = concat!(
    "SELECT rt.album_id, ",
    album_title!(),
    ", ar.name, rt.id, rt.disc, rt.position, rt.number, rt.title, rt.artist,
            rt.length_ms, w.id
       FROM release_tracks rt
       JOIN albums a ON a.id = rt.album_id
       LEFT JOIN artists ar ON ar.id = a.artist_id
       LEFT JOIN wants w ON w.release_track_id = rt.id"
);

macro_rules! held_or_wanted {
    () => {
        "(EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = rt.album_id)
          OR EXISTS (SELECT 1 FROM wants wn WHERE wn.release_track_id = rt.id))"
    };
}

const SHORT_OF_WHAT_IS_HELD_OR_WANTED: &str =
    concat!(" WHERE rt.track_id IS NULL AND ", held_or_wanted!());

const UNHELD_HOLDS_THE_NAME: &str = " AND rt.folded LIKE ? ESCAPE '\\'";

const UNHELD_MATCHING_IN_ORDER: &str = concat!(
    " ORDER BY w.id IS NULL, ",
    album_title!(),
    " COLLATE NOCASE, a.id, rt.disc, rt.position
      LIMIT ?"
);

const MISSING_TRACKS_IN_ORDER: &str = concat!(
    " WHERE rt.track_id IS NULL AND ",
    held_or_wanted!(),
    "
      ORDER BY ",
    album_title!(),
    " COLLATE NOCASE, a.id, rt.disc, rt.position
      LIMIT ?"
);

const MISSING_TRACKS_COUNTED: &str =
    "SELECT count(*) FROM release_tracks rt JOIN albums a ON a.id = rt.album_id";

const UNHELD_RELEASES: &str = concat!(
    "SELECT r.artist_id, ar.name, r.mbid, r.title, r.kind, r.first_released
       FROM artist_releases r
       JOIN artists ar ON ar.id = r.artist_id
      WHERE ",
    unheld_by_any_album!()
);

const UNHELD_IN_ORDER: &str =
    " ORDER BY ar.name COLLATE NOCASE, r.artist_id, r.first_released IS NULL, r.first_released,
               r.title COLLATE NOCASE
      LIMIT ?";

const UNHELD_COUNTED: &str = concat!(
    "SELECT count(*) FROM artist_releases r JOIN artists ar ON ar.id = r.artist_id WHERE ",
    unheld_by_any_album!()
);

const UNHELD_HOLDS_THE_WORD: &str =
    " AND (r.folded LIKE ? ESCAPE '\\' OR ar.key LIKE ? ESCAPE '\\')";

const UNHELD_OF_ARTIST: &str = concat!(
    "SELECT count(*) FROM artist_releases r WHERE r.artist_id = ?1 AND ",
    unheld_by_any_album!()
);

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
    vault: Option<Arc<Vault>>,
    playlists: AtomicU64,
    named: Arc<AtomicU64>,
    spellings: Mutex<Option<KeptVocabulary>>,
    suggested: Mutex<Option<KeptSuggestions>>,
    playing: Mutex<Option<Playing>>,
    steps: Mutex<Vec<Step>>,
    walked: Mutex<Vec<Step>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct NamesStamp {
    named: u64,
    written_elsewhere: Option<i64>,
}

impl NamesStamp {
    fn still_holds_at(self, now: Self) -> bool {
        self.named == now.named
            && now
                .written_elsewhere
                .is_none_or(|elsewhere| self.written_elsewhere == Some(elsewhere))
    }
}

struct KeptVocabulary {
    stamp: NamesStamp,
    spellings: Arc<Spellings>,
}

struct KeptSuggestions {
    stamp: NamesStamp,
    suggestions: Arc<[Suggestion]>,
}

struct Reader<'a> {
    pool: &'a Inner,
    connection: Option<Connection>,
}

pub(crate) struct Walk {
    inner: Arc<Inner>,
}

impl Drop for Walk {
    fn drop(&mut self) {
        self.inner.walking.store(false, Ordering::Release);
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
    fn opened_vault(&self) -> Result<&Arc<Vault>> {
        self.vault.as_ref().ok_or(Error::NoVault)
    }

    pub(crate) fn in_the_vault(&self, within: &str) -> Result<PathBuf> {
        let vault = self.opened_vault()?;
        vault.at(Path::new(within)).map_err(|source| Error::Vault {
            path: PathBuf::from(within),
            source: Box::new(source),
        })
    }

    pub(crate) fn within_the_vault(&self, path: &Path) -> Result<String> {
        let vault = self.opened_vault()?;
        let within = vault.within(path).map_err(|source| Error::Vault {
            path: path.to_path_buf(),
            source: Box::new(source),
        })?;
        store::path_text(&within).map(str::to_owned)
    }

    pub(crate) fn walk_the_tree(self: &Arc<Self>) -> Result<Walk> {
        if self
            .walking
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(Error::AlreadyWalking);
        }

        Ok(Walk {
            inner: Arc::clone(self),
        })
    }

    pub(crate) fn playlists_revision(&self) -> u64 {
        self.playlists.load(Ordering::Acquire)
    }

    pub(crate) fn playlists_changed(&self) {
        self.playlists.fetch_add(1, Ordering::AcqRel);
    }

    pub(crate) const fn steps(&self) -> &Mutex<Vec<Step>> {
        &self.steps
    }

    pub(crate) const fn walked(&self) -> &Mutex<Vec<Step>> {
        &self.walked
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
            .transaction()
            .map_err(|source| Error::store(StoreOp::Transaction, source))?;

        let value = change(&transaction)?;
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Transaction, source))?;
        Ok(value)
    }

    fn names_stamp(&self) -> NamesStamp {
        NamesStamp {
            named: self.named.load(Ordering::Acquire),
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

const NAMED_TABLES: [&str; 4] = ["tracks", "albums", "artists", "artist_genres"];

fn watch_the_names(connection: &Connection, named: &Arc<AtomicU64>) -> Result<()> {
    let counted = Arc::clone(named);
    connection
        .update_hook(Some(move |_: Action, _: &str, table: &str, _: i64| {
            if NAMED_TABLES.contains(&table) {
                counted.fetch_add(1, Ordering::AcqRel);
            }
        }))
        .map_err(|source| Error::store(StoreOp::Open, source))
}

fn connect(source: &Source, role: schema::Role) -> Result<Connection> {
    let connection = match source {
        Source::File(path) => Connection::open(path),
        Source::Memory => Connection::open_in_memory(),
    }
    .map_err(|source| Error::store(StoreOp::Open, source))?;

    schema::configure(&connection, role)?;
    Ok(connection)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WrittenElsewhere(i64);

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
        store::reconcile_artists(&mut writer)?;

        let named = Arc::new(AtomicU64::new(0));
        watch_the_names(&writer, &named)?;

        Ok(Self {
            inner: Arc::new(Inner {
                source,
                writer: Mutex::new(writer),
                readers: Mutex::new(Pool::default()),
                freed: Condvar::new(),
                walking: AtomicBool::new(false),
                vault,
                playlists: AtomicU64::new(0),
                named,
                spellings: Mutex::new(None),
                suggested: Mutex::new(None),
                playing: Mutex::new(None),
                steps: Mutex::new(Vec::new()),
                walked: Mutex::new(Vec::new()),
            }),
        })
    }

    #[cfg(test)]
    pub(crate) fn inner(&self) -> &Inner {
        &self.inner
    }

    pub(crate) fn shared(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }

    pub fn alternatives_of(&self, id: TrackId) -> Result<Vec<Track>> {
        collect(
            &self.inner,
            &format!(
                "SELECT {TRACK_COLUMNS} FROM tracks WHERE alternative_of = ?1
                  ORDER BY codec, sample_rate DESC, sample_format DESC, id"
            ),
            vec![Value::Integer(id.get() as i64)],
        )
    }

    pub fn track(&self, id: TrackId) -> Result<Option<Track>> {
        let raw = self.inner.read(|connection| {
            connection
                .query_row(
                    &format!("SELECT {TRACK_COLUMNS} FROM tracks WHERE id = ?1"),
                    params![id.get() as i64],
                    RawTrack::read,
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))
        })?;

        raw.map(RawTrack::into_track).transpose()
    }

    pub fn track_at(&self, path: &Path, span: Option<FrameSpan>) -> Result<Option<Track>> {
        let text = store::path_text(path)?;
        let (start, _) = store::span_columns(span);
        let raw = self.inner.read(|connection| {
            connection
                .query_row(
                    &format!(
                        "SELECT {TRACK_COLUMNS} FROM tracks WHERE path = ?1 AND span_start = ?2"
                    ),
                    params![text, start],
                    RawTrack::read,
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))
        })?;

        raw.map(RawTrack::into_track).transpose()
    }

    pub fn tracks(&self, query: &TrackQuery) -> Result<Vec<Track>> {
        tracks(&self.inner, query, None)
    }

    pub fn favourite_tracks(&self, query: &TrackQuery) -> Result<Vec<Track>> {
        tracks(&self.inner, query, Some(THE_FAVOURITES_SEARCH))
    }

    /// Hides a track from library listings without removing its row or audio file.
    pub fn hide_track(&self, id: TrackId) -> Result<bool> {
        self.inner.write(|transaction| {
            transaction
                .execute(
                    "UPDATE tracks SET hidden = 1 WHERE id = ?1 AND hidden = 0",
                    [id.get() as i64],
                )
                .map(|changed| changed > 0)
                .map_err(|source| Error::store(StoreOp::Update, source))
        })
    }

    pub fn favour(&self, what: Favoured, favourite: bool) -> Result<bool> {
        let (table, row) = favoured(what);
        let at = favourite.then(|| store::to_nanos(SystemTime::now()));
        let standing = if favourite { "IS NULL" } else { "IS NOT NULL" };

        self.inner.write(|transaction| {
            transaction
                .execute(
                    &format!(
                        "UPDATE {table} SET favourite = ?1 WHERE id = ?2 AND favourite {standing}"
                    ),
                    params![at, row],
                )
                .map(|changed| changed > 0)
                .map_err(|source| Error::store(StoreOp::Update, source))
        })
    }

    pub fn listened(&self, listen: ListenId, heard: Duration) -> Result<()> {
        let nanos = i64::try_from(heard.as_nanos()).unwrap_or(i64::MAX);

        self.inner.write(|transaction| {
            transaction
                .execute(
                    "UPDATE listens SET heard = ?1 WHERE id = ?2",
                    params![nanos, listen.get() as i64],
                )
                .map(drop)
                .map_err(|source| Error::store(StoreOp::Update, source))
        })
    }

    pub fn track_played(
        &self,
        location: &MediaLocation,
        span: Option<FrameSpan>,
    ) -> Result<Option<Counted>> {
        let Some(path) = location.as_path() else {
            return Ok(None);
        };
        let text = store::path_text(path)?;
        let (start, _) = store::span_columns(span);

        let raw = self.inner.write(|transaction| {
            let now = store::to_nanos(SystemTime::now());
            let counted = transaction
                .execute(
                    "UPDATE tracks SET plays = plays + 1, played = ?1
                     WHERE path = ?2 AND span_start = ?3",
                    params![now, text, start],
                )
                .map_err(|source| Error::store(StoreOp::Update, source))?;
            if counted == 0 {
                return Ok(None);
            }

            let listen = transaction
                .query_row(
                    "INSERT INTO listens (track_id, at)
                     SELECT id, ?1 FROM tracks WHERE path = ?2 AND span_start = ?3
                     RETURNING id",
                    params![now, text, start],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(|source| Error::store(StoreOp::Insert, source))?;

            transaction
                .query_row(
                    &format!(
                        "SELECT {TRACK_COLUMNS} FROM tracks WHERE path = ?1 AND span_start = ?2"
                    ),
                    params![text, start],
                    RawTrack::read,
                )
                .optional()
                .map(|raw| raw.map(|raw| (raw, listen)))
                .map_err(|source| Error::store(StoreOp::Query, source))
        })?;

        raw.map(|(raw, listen)| {
            Ok(Counted {
                track: raw.into_track()?,
                listen: ListenId::new(listen as u64)?,
            })
        })
        .transpose()
    }

    pub fn statistics(&self, window: Window) -> Result<Statistics> {
        statistics::statistics(&self.inner, window)
    }

    pub fn most_listened(&self, window: Window, most: usize) -> Result<MostListened> {
        statistics::most_listened(&self.inner, window, most)
    }

    pub fn listening_by_day(&self, window: Window) -> Result<Vec<Day>> {
        statistics::listening_by_day(&self.inner, window)
    }

    pub fn shareable(&self, track: TrackId) -> Result<Option<Shared>> {
        share::shareable(&self.inner, track)
    }

    pub fn suggestions(&self) -> Result<Arc<[Suggestion]>> {
        let stamp = self.inner.names_stamp();
        if let Some(kept) = self
            .inner
            .suggested
            .lock()
            .as_ref()
            .filter(|kept| kept.stamp.still_holds_at(stamp))
        {
            return Ok(Arc::clone(&kept.suggestions));
        }

        let suggestions: Arc<[Suggestion]> = suggest::suggestions(&self.inner)?.into();
        *self.inner.suggested.lock() = Some(KeptSuggestions {
            stamp,
            suggestions: Arc::clone(&suggestions),
        });
        Ok(suggestions)
    }

    pub fn album(&self, id: AlbumId) -> Result<Option<Album>> {
        self.inner.read(|connection| {
            connection
                .query_row(
                    &format!("SELECT {ALBUM_COLUMNS} FROM albums a WHERE a.id = ?1"),
                    params![id.get() as i64],
                    read_album,
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))?
                .transpose()
        })
    }

    pub fn albums(&self, query: &AlbumQuery) -> Result<Vec<Album>> {
        self.listed_albums(query, None)
    }

    pub fn favourite_albums(&self, query: &AlbumQuery) -> Result<Vec<Album>> {
        self.listed_albums(query, Some(A_FAVOURITE_ALBUM))
    }

    fn listed_albums(&self, query: &AlbumQuery, only: Option<&str>) -> Result<Vec<Album>> {
        let Some(scoped) = scoped_albums(query, only) else {
            return Ok(Vec::new());
        };

        let mut sql = format!("SELECT {ALBUM_COLUMNS} FROM albums a{}", scoped.from);
        let mut binds = scoped.binds;

        sql.push_str(" ORDER BY ");
        sql.push_str(album_order_by(query.sort, query.reading, scoped.ranked));
        sql.push_str(" LIMIT ? OFFSET ?");
        binds.push(Value::Integer(limit(query.limit)));
        binds.push(Value::Integer(query.offset as i64));

        self.inner
            .read(|connection| rows(connection, &sql, binds, read_album))
    }

    pub fn albums_counted(&self, query: &AlbumQuery) -> Result<u32> {
        let Some(scoped) = scoped_albums(query, None) else {
            return Ok(0);
        };

        self.counted(
            &format!("SELECT count(*) FROM albums a{}", scoped.from),
            scoped.binds,
        )
    }

    pub fn artists_counted(&self, query: &ArtistQuery) -> Result<u32> {
        let Some(scoped) = scoped_artists(query, None) else {
            return Ok(0);
        };

        self.counted(
            &format!("SELECT count(*) FROM artists r{}", scoped.from),
            scoped.binds,
        )
    }

    pub fn measured(&self, query: &TrackQuery) -> Result<Measured> {
        measured(&self.inner, query)
    }

    pub fn artist_totals(&self, id: ArtistId) -> Result<ArtistTotals> {
        self.inner.read(|connection| {
            connection
                .query_row(WHAT_AN_ARTIST_HOLDS, [id.get() as i64], |row| {
                    Ok(ArtistTotals {
                        albums: row.get::<_, i64>(0)? as u32,
                        tracks: row.get::<_, i64>(1)? as u32,
                        length: played(row.get::<_, Option<f64>>(2)?),
                        plays: row.get::<_, Option<i64>>(3)?.unwrap_or_default() as u32,
                        played: row.get::<_, Option<i64>>(4)?.map(store::from_nanos),
                    })
                })
                .map_err(|source| Error::store(StoreOp::Query, source))
        })
    }

    fn counted(&self, sql: &str, binds: Vec<Value>) -> Result<u32> {
        self.inner.read(|connection| {
            connection
                .query_row(sql, params_from_iter(binds), |row| row.get::<_, i64>(0))
                .map(|rows| rows as u32)
                .map_err(|source| Error::store(StoreOp::Query, source))
        })
    }

    pub fn artists(&self, query: &ArtistQuery) -> Result<Vec<Artist>> {
        self.listed_artists(query, None)
    }

    pub fn favourite_artists(&self, query: &ArtistQuery) -> Result<Vec<Artist>> {
        self.listed_artists(query, Some(A_FAVOURITE_ARTIST))
    }

    fn listed_artists(&self, query: &ArtistQuery, only: Option<&str>) -> Result<Vec<Artist>> {
        let Some(scoped) = scoped_artists(query, only) else {
            return Ok(Vec::new());
        };

        let mut sql = format!("SELECT {ARTIST_COLUMNS} FROM artists r{}", scoped.from);
        let mut binds = scoped.binds;
        let ranked = scoped.ranked;

        sql.push_str(" ORDER BY ");
        sql.push_str(artist_order_by(query.sort, query.reading, ranked));
        sql.push_str(" LIMIT ? OFFSET ?");
        binds.push(Value::Integer(limit(query.limit)));
        binds.push(Value::Integer(query.offset as i64));

        self.inner
            .read(|connection| rows(connection, &sql, binds, read_artist))
    }

    pub fn search(&self, text: &str, limit: usize) -> Result<SearchResults> {
        let text = Some(text.to_owned());

        Ok(SearchResults {
            tracks: self.tracks(&TrackQuery {
                text: text.clone(),
                limit: Some(limit),
                ..TrackQuery::default()
            })?,
            albums: self.albums(&AlbumQuery {
                text: text.clone(),
                limit: Some(limit),
                ..AlbumQuery::default()
            })?,
            artists: self.artists(&ArtistQuery {
                text,
                limit: Some(limit),
                ..ArtistQuery::default()
            })?,
        })
    }

    pub fn did_you_mean(&self, text: &str) -> Result<Option<String>> {
        if !spelling::worth_asking(&Search::read(text)) {
            return Ok(None);
        }

        Ok(self.inner.vocabulary()?.did_you_mean(text))
    }

    pub fn cover_art(&self, id: AlbumId) -> Result<Option<CoverArt>> {
        let stored = self.inner.read(|connection| {
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
            let Some(vault) = self.inner.vault.as_ref() else {
                tracing::debug!(
                    album = id.get(),
                    "a cover kept in the vault is drawn by no build that has not opened one"
                );
                return Ok(None);
            };
            let path = self.inner.in_the_vault(kept)?;
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

    pub(crate) fn cover_the_vault_lacks(&self, id: AlbumId) -> Result<Option<CoverArt>> {
        let stored = self.inner.read(|connection| {
            connection
                .query_row(
                    "SELECT cover_art, cover_format FROM albums
                      WHERE id = ?1 AND cover_path IS NULL AND cover_art IS NOT NULL",
                    params![id.get() as i64],
                    |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Option<i64>>(1)?)),
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))
        })?;

        stored
            .map(|(bytes, code)| held_cover(id, bytes, code))
            .transpose()
    }

    pub fn roots(&self) -> Result<Vec<PathBuf>> {
        self.inner.read(|connection| {
            let mut statement = connection
                .prepare("SELECT path FROM roots ORDER BY path")
                .map_err(|source| Error::store(StoreOp::Prepare, source))?;
            let found = statement
                .query_map([], |row| row.get::<_, String>(0))
                .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
                .map_err(|source| Error::store(StoreOp::Query, source))?;

            Ok(found.into_iter().map(PathBuf::from).collect())
        })
    }

    pub fn add_root(&self, path: &Path) -> Result<()> {
        if !path.is_dir() {
            return Err(Error::RootNotADirectory {
                path: path.to_path_buf(),
            });
        }
        let canonical = path.canonicalize().map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;

        self.inner
            .write(|transaction| store::register_root(transaction, &canonical).map(drop))
    }

    pub fn remove_root(&self, path: &Path) -> Result<bool> {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        self.inner.write(|transaction| {
            let Some(id) = transaction
                .query_row(
                    "SELECT id FROM roots WHERE path = ?1",
                    params![store::path_text(&canonical)?],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))?
            else {
                return Ok(false);
            };

            transaction
                .execute("DELETE FROM tracks WHERE root_id = ?1", params![id])
                .map_err(|source| Error::store(StoreOp::Delete, source))?;
            transaction
                .execute("DELETE FROM roots WHERE id = ?1", params![id])
                .map_err(|source| Error::store(StoreOp::Delete, source))?;
            transaction
                .execute_batch(store::ORPHANS)
                .map_err(|source| Error::store(StoreOp::Delete, source))?;

            Ok(true)
        })
    }

    pub fn playlists(
        &self,
        order: PlaylistOrder,
        direction: Direction,
        named: Option<&str>,
    ) -> Result<Vec<Playlist>> {
        playlist::all(&self.inner, order, direction, named)
    }

    pub fn playlist_lists(
        &self,
        order: PlaylistOrder,
        direction: Direction,
    ) -> Result<Vec<Playlist>> {
        playlist::lists(&self.inner, order, direction)
    }

    pub fn playlist_names(
        &self,
        order: PlaylistOrder,
        direction: Direction,
        from: usize,
        most: Option<usize>,
    ) -> Result<Vec<NamedPlaylist>> {
        playlist::names(&self.inner, order, direction, from, most)
    }

    pub fn playlist_count(&self, named: Option<&str>) -> Result<u32> {
        playlist::count(&self.inner, named)
    }

    pub fn playlist(&self, id: PlaylistId) -> Result<Option<Playlist>> {
        playlist::one(&self.inner, id)
    }

    pub fn playlist_named(&self, name: &str) -> Result<Option<Playlist>> {
        playlist::named(&self.inner, name)
    }

    pub fn playlist_entries(
        &self,
        id: PlaylistId,
        matching: Option<&str>,
    ) -> Result<Vec<PlaylistEntry>> {
        playlist::entries(&self.inner, id, matching)
    }

    pub fn playlist_cuts(&self, id: PlaylistId) -> Result<Vec<Cut>> {
        playlist::cuts(&self.inner, id)
    }

    pub fn create_playlist(&self, name: &str) -> Result<PlaylistId> {
        playlist::create(&self.inner, name)
    }

    pub fn start_playlist(&self, name: &str, cuts: &[Cut]) -> Result<PlaylistId> {
        playlist::start(&self.inner, name, cuts)
    }

    pub fn save_query(&self, name: &str, query: &SavedQuery) -> Result<PlaylistId> {
        playlist::save_query(&self.inner, name, query)
    }

    pub fn revise_query(&self, id: PlaylistId, name: &str, query: &SavedQuery) -> Result<()> {
        playlist::revise(&self.inner, id, name, query)
    }

    pub fn rename_playlist(&self, id: PlaylistId, name: &str) -> Result<()> {
        playlist::rename(&self.inner, id, name)
    }

    pub fn remove_playlist(&self, id: PlaylistId) -> Result<bool> {
        playlist::remove(&self.inner, id)
    }

    pub fn pin_playlist(&self, id: PlaylistId, pinned: bool) -> Result<bool> {
        playlist::pin(&self.inner, id, pinned)
    }

    pub fn add_to_playlist(&self, id: PlaylistId, cuts: &[Cut]) -> Result<usize> {
        playlist::add(&self.inner, id, cuts)
    }

    pub fn copy_playlist(
        &self,
        from: PlaylistId,
        into: PlaylistId,
        matching: Option<&str>,
    ) -> Result<usize> {
        playlist::copy(&self.inner, from, into, matching)
    }

    pub fn remove_from_playlist(&self, id: PlaylistId, rows: Span) -> Result<bool> {
        playlist::remove_rows(&self.inner, id, rows)
    }

    pub fn remove_matching(&self, id: PlaylistId, matching: &str) -> Result<usize> {
        playlist::remove_matching(&self.inner, id, matching)
    }

    pub fn move_in_playlist(&self, id: PlaylistId, rows: Span, to: usize) -> Result<bool> {
        playlist::move_rows(&self.inner, id, rows, to)
    }

    pub fn sort_playlist(
        &self,
        id: PlaylistId,
        order: RowOrder,
        direction: Direction,
    ) -> Result<usize> {
        playlist::sort_rows(&self.inner, id, order, direction)
    }

    pub fn keep_playlist_in_order(&self, id: PlaylistId, kept: Option<Kept>) -> Result<usize> {
        playlist::keep(&self.inner, id, kept)
    }

    pub fn prune_playlist(&self, id: PlaylistId) -> Result<usize> {
        playlist::prune(&self.inner, id)
    }

    pub fn fold_doubles(&self, id: PlaylistId) -> Result<usize> {
        playlist::fold_doubles(&self.inner, id)
    }

    pub fn import_playlist(&self, path: &Path, name: Option<&str>) -> Result<Imported> {
        playlist::import(&self.inner, path, name)
    }

    pub fn export_playlist(&self, id: PlaylistId, path: &Path) -> Result<Exported> {
        playlist::export(&self.inner, id, path)
    }

    pub fn undo(&self) -> Result<Option<Undoable>> {
        undo::undo(&self.inner)
    }

    pub fn undoable(&self) -> Option<Undoable> {
        undo::undoable(&self.inner)
    }

    pub fn redo(&self) -> Result<Option<Undoable>> {
        undo::redo(&self.inner)
    }

    pub fn redoable(&self) -> Option<Undoable> {
        undo::redoable(&self.inner)
    }

    pub fn playlists_revision(&self) -> u64 {
        self.inner.playlists_revision()
    }

    pub fn written_elsewhere(&self) -> Option<WrittenElsewhere> {
        self.inner.written_elsewhere().map(WrittenElsewhere)
    }

    pub fn playing_playlist(&self, queue: QueueStamp) -> Option<PlaylistId> {
        self.inner
            .playing()
            .filter(|playing| playing.queue == queue)
            .map(|playing| playing.playlist)
    }

    pub fn set_playing_playlist(&self, playing: Option<Playing>) {
        self.inner.set_playing(playing);
    }

    pub fn enrich(
        &self,
        reference: Arc<dyn Reference>,
        fingerprinters: Arc<Fingerprinters>,
        options: EnrichOptions,
    ) -> Result<EnrichHandle> {
        enrich::start(self.shared(), reference, fingerprinters, options)
    }

    pub fn poll(&self, providers: Arc<Providers>, options: PollOptions) -> Result<PollHandle> {
        supply::start(self.shared(), providers, options)
    }

    pub fn scan(&self, options: ScanOptions) -> Result<ScanHandle> {
        scan::start(Arc::clone(&self.inner), options)
    }

    pub fn forget_the_gone(&self, gone: &[PathBuf]) -> Result<u64> {
        scan::forget_the_gone(&self.inner, gone)
    }

    pub fn retag(&self, tags: Arc<dyn TagSink>, options: RetagOptions) -> Result<RetagHandle> {
        retag::start(self.shared(), tags, options)
    }

    pub fn organise(&self, options: OrganiseOptions) -> Result<OrganiseHandle> {
        organise::start(self.shared(), options)
    }

    pub(crate) fn walk_the_tree(&self) -> Result<Walk> {
        self.inner.walk_the_tree()
    }

    pub(crate) fn files_moved(&self, landed: &[Move]) -> Result<()> {
        self.inner
            .write(|transaction| organise::files_moved(transaction, landed))
    }

    pub(crate) fn albums_re_keyed(&self, landed: &[Move]) -> Result<()> {
        self.inner
            .write(|transaction| organise::re_key_the_sleeves(transaction, landed))
    }

    pub(crate) fn tracks_to_tag(&self, roots: &[PathBuf]) -> Result<Vec<TrackToTag>> {
        let named = rooted(roots)?;
        let sql = under_roots(TRACKS_TO_TAG, named.len());

        self.inner.read(|connection| {
            let tracks = counted_by_disc(connection, RELEASE_DISC_TRACKS)?;
            let discs = counted_by_album(connection, RELEASE_MEDIA)?;
            let held = rows(connection, &sql, named.clone(), |row| {
                RawToTag::read(row).map(Ok)
            })?;

            held.into_iter()
                .map(|raw| raw.into_tagged(&tracks, &discs))
                .collect()
        })
    }

    pub(crate) fn files_retagged(&self, followed: &[Followed]) -> Result<()> {
        self.inner
            .write(|transaction| retag::files_retagged(transaction, followed))
    }

    pub fn import(&self, sources: Arc<Sources>, options: ImportOptions) -> Result<ImportHandle> {
        let Some(vault) = self.inner.vault.clone() else {
            return Err(Error::NoVault);
        };
        import::start(self.shared(), vault, sources, options)
    }

    pub fn holdings(&self) -> Result<Holdings> {
        let Some(vault) = self.inner.vault.as_ref() else {
            return Err(Error::NoVault);
        };
        vault.holding().map_err(|source| Error::Vault {
            path: vault.root().to_path_buf(),
            source: Box::new(source),
        })
    }

    pub fn vault_objects(&self) -> Result<Vec<VaultObject>> {
        self.vault_objects_read(VAULT_OBJECTS)
    }

    pub fn vault_objects_nothing_names(&self) -> Result<Vec<VaultObject>> {
        self.vault_objects_read(VAULT_OBJECTS_UNHELD)
    }

    fn vault_objects_read(&self, sql: &str) -> Result<Vec<VaultObject>> {
        self.inner
            .read(|connection| {
                rows(connection, sql, Vec::new(), |row| {
                    RawVaultObject::read(row).map(Ok)
                })
            })?
            .into_iter()
            .map(|raw| raw.into_object(&self.inner))
            .collect()
    }

    pub fn prune_the_vault(&self) -> Result<Pruned> {
        let _walk = self.walk_the_tree()?;
        let vault = Arc::clone(self.inner.opened_vault()?);
        let refused = |path: &Path, source| Error::Vault {
            path: path.to_path_buf(),
            source: Box::new(source),
        };

        let mut pruned = Pruned::default();
        for object in self.vault_objects_nothing_names()? {
            match vault.forget(&object.path) {
                Ok(true) => pruned.objects += 1,
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(path = %object.path.display(), %error, "an object could not be taken away");
                }
            }
            self.forget_vault_object(&object.key)?;
        }

        let named: AHashSet<VaultKey> = self
            .inner
            .read(|connection| {
                rows(
                    connection,
                    "SELECT cover_key FROM albums WHERE cover_key IS NOT NULL",
                    Vec::new(),
                    |row| {
                        let key: String = row.get(0)?;
                        Ok(VaultKey::read(&key).map_err(|_| Error::NotAVaultKey {
                            named: key.into_boxed_str(),
                        }))
                    },
                )
            })?
            .into_iter()
            .collect();

        let root = vault.root().to_path_buf();
        for cover in vault.covers().map_err(|source| refused(&root, source))? {
            if named.contains(&cover.key) {
                continue;
            }
            if vault
                .forget(&cover.path)
                .map_err(|source| refused(&cover.path, source))?
            {
                pruned.covers += 1;
            }
        }

        pruned.staged = vault
            .sweep_the_staging()
            .map_err(|source| refused(&root, source))?;
        Ok(pruned)
    }

    pub fn release_from_vault(&self, roots: &[PathBuf]) -> Result<Released> {
        let _walk = self.walk_the_tree()?;
        let known = self.roots()?;
        if let Some(stranger) = roots.iter().find(|root| !known.contains(root)) {
            return Err(Error::NotARoot {
                path: stranger.clone(),
            });
        }

        let named = rooted(roots)?;
        let sql = and_roots(TRACKS_IN_THE_VAULT, named.len());
        let vaulted: Vec<(i64, PathBuf)> = self.inner.read(|connection| {
            rows(connection, &sql, named.clone(), |row| {
                Ok(Ok((row.get(0)?, PathBuf::from(row.get::<_, String>(1)?))))
            })
        })?;
        let (standing, stranded): (Vec<_>, Vec<_>) =
            vaulted.into_iter().partition(|(_, path)| path.exists());

        let released = self.inner.write(|transaction| {
            let mut released = 0;
            for (id, _) in &standing {
                released += transaction
                    .execute(
                        "UPDATE tracks SET vault_key = NULL, vault_path = NULL WHERE id = ?1",
                        params![id],
                    )
                    .map_err(|source| Error::store(StoreOp::Update, source))?;
            }
            Ok(released as u64)
        })?;

        Ok(Released {
            released,
            stranded: stranded.len() as u64,
        })
    }

    pub fn forget_vault_object(&self, key: &VaultKey) -> Result<bool> {
        let named = key.to_string();
        self.inner.write(|transaction| {
            transaction
                .execute("DELETE FROM vault_objects WHERE key = ?1", params![named])
                .map(|gone| gone > 0)
                .map_err(|source| Error::store(StoreOp::Delete, source))
        })
    }

    pub fn note_validated(&self, key: &VaultKey, validated: bool) -> Result<()> {
        let named = key.to_string();
        self.inner.write(|transaction| {
            transaction
                .execute(
                    "UPDATE vault_objects SET validated = ?1 WHERE key = ?2",
                    params![i64::from(validated), named],
                )
                .map(|_| ())
                .map_err(|source| Error::store(StoreOp::Update, source))
        })
    }

    pub(crate) fn note_vaulted(&self, row: &TrackToVault, kept: &VaultKept) -> Result<()> {
        let key = kept.key.to_string();
        let held = self.inner.within_the_vault(&kept.path)?;
        let taken_from = store::path_text(&row.path)?.to_owned();
        let took = store::to_nanos(SystemTime::now());
        let encoding = weighed_under(kept, row.renewing);

        self.inner.write(|transaction| {
            transaction
                .execute(
                    "INSERT INTO vault_objects (key, form, path, bytes, sample_rate, channels,
                            sample_format, frames, taken_from, took, was_bytes, was_codec,
                            encoding, validated)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 1)
                     ON CONFLICT(key) DO UPDATE
                        SET bytes = excluded.bytes, encoding = excluded.encoding, validated = 1
                      WHERE excluded.encoding > vault_objects.encoding",
                    params![
                        key,
                        store::vault_form_code(kept.form),
                        held,
                        kept.bytes as i64,
                        kept.spec.rate.hz(),
                        kept.spec.channel_count().get(),
                        store::format_code(kept.spec.format),
                        kept.frames.get() as i64,
                        taken_from,
                        took,
                        kept.was as i64,
                        store::codec_code(row.codec),
                        i64::from(encoding.get()),
                    ],
                )
                .map_err(|source| Error::store(StoreOp::Insert, source))?;

            transaction
                .execute(
                    "UPDATE tracks SET vault_key = ?1, vault_path = ?2 WHERE id = ?3",
                    params![key, held, row.id.get() as i64],
                )
                .map(|_| ())
                .map_err(|source| Error::store(StoreOp::Update, source))
        })
    }

    pub(crate) fn note_delivered(
        &self,
        want: &Want,
        kept: &VaultKept,
        from: &MediaLocation,
    ) -> Result<TrackId> {
        let key = kept.key.to_string();
        let held = self.inner.within_the_vault(&kept.path)?;
        let path = store::path_text(&kept.path)?.to_owned();
        let offered = from.to_uri();
        let now = SystemTime::now();
        let took = store::to_nanos(now);
        let modified = std::fs::metadata(&kept.path)
            .and_then(|metadata| metadata.modified())
            .unwrap_or(now);

        let id = self.inner.write(|transaction| {
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

            let artist = want
                .artist
                .as_deref()
                .filter(|name| !name.trim().is_empty())
                .map(|name| store::artist_named_in(transaction, name, None))
                .transpose()?;
            let id: i64 = transaction
                .query_row(
                    "INSERT INTO tracks (
                         root_id, path, title, artist, artist_id, album_id, track_number,
                         disc_number, duration, sample_rate, channels, sample_format, codec,
                         file_size, modified, added, seen, mbid, release_track_mbid, isrc,
                         vault_key, vault_path
                     ) VALUES (
                         NULL, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                         ?15, 0, ?16, ?17, ?18, ?19, ?20
                     )
                     ON CONFLICT(path, span_start) DO UPDATE SET seen = tracks.seen
                     RETURNING id",
                    params![
                        path,
                        want.title,
                        want.artist,
                        artist,
                        want.album.get() as i64,
                        i64::from(want.position),
                        i64::from(want.disc),
                        kept.frames.get() as i64,
                        kept.spec.rate.hz(),
                        kept.spec.channel_count().get(),
                        store::format_code(kept.spec.format),
                        store::codec_code(kept.codec),
                        kept.bytes as i64,
                        store::to_nanos(modified),
                        took,
                        want.recording.as_ref().map(Mbid::as_str),
                        want.track.as_ref().map(Mbid::as_str),
                        want.isrc.as_ref().map(Isrc::as_str),
                        key,
                        held,
                    ],
                    |row| row.get(0),
                )
                .map_err(|source| Error::store(StoreOp::Insert, source))?;

            store::index_row(
                transaction,
                id,
                &want.title,
                want.artist.as_deref().unwrap_or_default(),
                &want.album_title,
                &store::indexed_genre_of(transaction, None, artist)?,
            )?;
            transaction
                .execute(
                    "UPDATE release_tracks SET track_id = ?1 WHERE id = ?2",
                    params![id, want.release_track.get() as i64],
                )
                .map_err(|source| Error::store(StoreOp::Update, source))?;
            Ok(id)
        })?;

        Ok(TrackId::new(id as u64)?)
    }

    pub(crate) fn note_vaulted_cover(&self, album: AlbumId, kept: &KeptCover) -> Result<bool> {
        let within = self.inner.within_the_vault(&kept.path)?;
        self.inner
            .write(|transaction| enriched::vault_the_cover(transaction, album, kept.key, &within))
    }

    pub(crate) fn tracks_to_vault(&self, roots: &[PathBuf]) -> Result<Vec<TrackToVault>> {
        let named = rooted(roots)?;
        let sql = and_roots(TRACKS_TO_VAULT, named.len());
        let mut asked = vec![Value::Integer(i64::from(Encoding::OF_THIS_BUILD.get()))];
        asked.extend(named);

        self.inner.read(|connection| {
            rows(connection, &sql, asked.clone(), |row| {
                RawToVault::read(row).map(Ok)
            })?
            .into_iter()
            .map(RawToVault::into_vaultable)
            .collect()
        })
    }

    pub(crate) fn tracks_to_file(&self, roots: &[PathBuf]) -> Result<Vec<TrackToFile>> {
        let named = rooted(roots)?;
        let sql = under_roots(TRACKS_TO_FILE, named.len());

        self.inner.read(|connection| {
            let discs = counted_by_album(connection, ALBUM_DISCS)?;
            let held = rows(connection, &sql, named.clone(), |row| {
                RawFiled::read(row).map(Ok)
            })?;

            held.into_iter().map(|raw| raw.into_filed(&discs)).collect()
        })
    }

    pub fn release_of(&self, id: AlbumId) -> Result<Option<ReleaseDetail>> {
        let album = id.get() as i64;
        self.inner.read(|connection| {
            let Some(raw) = connection
                .query_row(
                    "SELECT mbid, release_group, date, country, label, catalog_number, barcode,
                            kind, disambiguation, cover_source, asked, answered
                       FROM albums WHERE id = ?1",
                    params![album],
                    RawRelease::read,
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))?
            else {
                return Ok(None);
            };
            let links = rows(
                connection,
                "SELECT relation, provider, url FROM album_links WHERE album_id = ?1 ORDER BY url",
                vec![Value::Integer(album)],
                |row| read_link(row, 0),
            )?;
            let media = rows(
                connection,
                "SELECT position, format, title FROM release_media
                  WHERE album_id = ?1 ORDER BY position",
                vec![Value::Integer(album)],
                |row| {
                    Ok(Ok(HeldMedium {
                        position: row.get::<_, i64>(0)? as u32,
                        format: row.get(1)?,
                        title: row.get(2)?,
                    }))
                },
            )?;

            raw.into_detail(id, links, media).map(Some)
        })
    }

    pub fn release_tracks(&self, id: AlbumId) -> Result<Vec<HeldReleaseTrack>> {
        let album = id.get() as i64;
        self.inner.read(|connection| {
            let held = rows(
                connection,
                &format!(
                    "SELECT {RELEASE_TRACK_COLUMNS} FROM release_tracks
                      WHERE album_id = ?1 ORDER BY disc, position"
                ),
                vec![Value::Integer(album)],
                |row| RawReleaseTrack::read(row).map(Ok),
            )?;
            let mut links = grouped_links(
                connection,
                LINKS_OF_RELEASE_TRACKS,
                vec![Value::Integer(album)],
            )?;

            held.into_iter()
                .map(|raw| {
                    let links = links.remove(&raw.id).unwrap_or_default();
                    raw.into_held(id, links)
                })
                .collect()
        })
    }

    pub fn ask_again_for_covers(&self) -> Result<usize> {
        self.inner.write(enriched::ask_again_for_covers)
    }

    pub(crate) fn albums_wanting_a_cover(&self) -> Result<Vec<CoverWanted>> {
        self.inner.read(enriched::albums_wanting_a_cover)
    }

    pub(crate) fn note_cover_asked(&self, album: AlbumId) -> Result<()> {
        self.inner
            .write(|transaction| enriched::note_cover_asked(transaction, album, SystemTime::now()))
    }

    pub fn artists_wanting_a_portrait(&self) -> Result<Vec<PortraitWanted>> {
        self.inner.read(|connection| {
            let held: Vec<(i64, Link)> =
                rows(connection, LINKS_OF_UNPICTURED_ARTISTS, Vec::new(), |row| {
                    let artist: i64 = row.get(0)?;

                    Ok(read_link(row, 1)?.map(|link| (artist, link)))
                })?;

            let mut wanting: Vec<PortraitWanted> = Vec::new();
            for (artist, link) in held {
                match wanting.last_mut() {
                    Some(last) if last.artist.get() as i64 == artist => last.links.push(link),
                    _ => wanting.push(PortraitWanted {
                        artist: ArtistId::new(artist as u64)?,
                        links: vec![link],
                    }),
                }
            }

            Ok(wanting
                .into_iter()
                .filter(|wanted| crate::may_be_pictured(&wanted.links))
                .collect())
        })
    }

    pub fn artist_detail(&self, id: ArtistId) -> Result<Option<ArtistDetail>> {
        let artist = id.get() as i64;
        self.inner.read(|connection| {
            let Some(raw) = connection
                .query_row(
                    "SELECT mbid, sort_name, kind, gender, country, area, began_in, began, ended,
                            has_ended, disambiguation, asked, answered, name
                       FROM artists WHERE id = ?1",
                    params![artist],
                    RawArtistDetail::read,
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))?
            else {
                return Ok(None);
            };
            let genres = rows(
                connection,
                "SELECT name, weight FROM artist_genres WHERE artist_id = ?1
                  ORDER BY weight DESC, name",
                vec![Value::Integer(artist)],
                |row| {
                    Ok(Ok(Genre {
                        name: row.get(0)?,
                        weight: row.get(1)?,
                    }))
                },
            )?;
            let links = rows(
                connection,
                "SELECT relation, provider, url FROM artist_links WHERE artist_id = ?1
                  ORDER BY url",
                vec![Value::Integer(artist)],
                |row| read_link(row, 0),
            )?;
            let releases_unheld = connection
                .query_row(UNHELD_OF_ARTIST, params![artist], |row| {
                    row.get::<_, u32>(0)
                })
                .map_err(|source| Error::store(StoreOp::Query, source))?;

            raw.into_detail(genres, links, releases_unheld).map(Some)
        })
    }

    pub fn missing_tracks(
        &self,
        narrowing: Option<&str>,
        at_most: Option<usize>,
    ) -> Result<Vec<MissingTrack>> {
        let Some(scoped) = scoped_missing(narrowing) else {
            return Ok(Vec::new());
        };
        let sql = format!("{MISSING_TRACKS}{}{MISSING_TRACKS_IN_ORDER}", scoped.from);
        let mut binds = scoped.binds;
        binds.push(Value::Integer(limit(at_most)));

        self.inner.read(|connection| {
            rows(connection, &sql, binds, |row| {
                RawMissingTrack::read(row).map(RawMissingTrack::into_missing)
            })
        })
    }

    pub fn unheld_matching(&self, text: &str, at_most: Option<usize>) -> Result<Vec<MissingTrack>> {
        let words = elsewhere::words_asked(text);
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let mut sql = format!("{MISSING_TRACKS}{SHORT_OF_WHAT_IS_HELD_OR_WANTED}");
        let mut binds = Vec::with_capacity(words.len() + 1);
        for word in &words {
            for piece in word.split_whitespace() {
                sql.push_str(UNHELD_HOLDS_THE_NAME);
                binds.push(Value::Text(playlist::anywhere(&store::folded_letters(
                    piece,
                ))));
            }
        }
        sql.push_str(UNHELD_MATCHING_IN_ORDER);
        binds.push(Value::Integer(limit(at_most)));

        self.inner.read(|connection| {
            rows(connection, &sql, binds, |row| {
                RawMissingTrack::read(row).map(RawMissingTrack::into_missing)
            })
        })
    }

    pub fn sung(&self, text: &str) -> Result<Option<Sung>> {
        let Some(sung) = Search::read(text).as_sung() else {
            return Ok(None);
        };
        let query = sung.to_string();
        let measured = self.measured(&TrackQuery {
            album: None,
            artist: None,
            text: Some(query.clone()),
            sort: SortOrder::default(),
            reading: SortOrder::default().reads(),
            limit: None,
            offset: 0,
        })?;

        Ok((measured.rows > 0).then_some(Sung {
            query,
            tracks: measured.rows,
        }))
    }

    pub fn found_elsewhere(&self, reference: &dyn Reference, text: &str) -> Result<Vec<Found>> {
        let words = elsewhere::words_asked(text);
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let matches = reference.find_songs(&words.join(" "))?;
        let held = self.recordings_named()?;

        Ok(elsewhere::found_among(matches, |recording| {
            held.contains(recording.as_str())
        }))
    }

    fn recordings_named(&self) -> Result<AHashSet<String>> {
        self.inner
            .read(|connection| {
                rows(
                    connection,
                    "SELECT mbid FROM tracks WHERE mbid IS NOT NULL
                 UNION SELECT recording_mbid FROM release_tracks WHERE recording_mbid IS NOT NULL",
                    Vec::new(),
                    |row| row.get::<_, String>(0).map(Ok),
                )
            })
            .map(|named| named.into_iter().collect())
    }

    pub fn want_found(&self, reference: &dyn Reference, found: &Found) -> Result<WantId> {
        let release = match &found.release {
            Some(release) => release.id.clone(),
            None => reference
                .recording(&found.recording)?
                .and_then(|recording| {
                    elsewhere::first_released(&recording.releases).map(|release| release.id.clone())
                })
                .ok_or_else(|| Error::Unreleased {
                    recording: found.recording.clone(),
                })?,
        };
        let landed = reference
            .release(&release)?
            .ok_or_else(|| Error::UnknownRelease {
                release: release.clone(),
            })?;
        let now = SystemTime::now();

        self.inner.write(|transaction| {
            let album = elsewhere::album_of_release(transaction, &landed, now)?;
            let row = elsewhere::release_track_of(transaction, album, &found.recording)?
                .ok_or_else(|| Error::NotOnTheRelease {
                    recording: found.recording.clone(),
                    release: release.clone(),
                })?;
            elsewhere::want_in(transaction, row, now)
        })
    }

    pub fn unheld_releases(
        &self,
        narrowing: Option<&str>,
        at_most: Option<usize>,
    ) -> Result<Vec<UnheldRelease>> {
        let (held, mut binds) = unheld_holding(narrowing);
        let sql = format!("{UNHELD_RELEASES}{held}{UNHELD_IN_ORDER}");
        binds.push(Value::Integer(limit(at_most)));

        self.inner.read(|connection| {
            rows(connection, &sql, binds, |row| {
                RawUnheldRelease::read(row).map(RawUnheldRelease::into_unheld)
            })
        })
    }

    pub fn missing_counted(&self, narrowing: Option<&str>) -> Result<Missing> {
        let tracks = match scoped_missing(narrowing) {
            Some(scoped) => self.counted(
                &format!(
                    "{MISSING_TRACKS_COUNTED}{}{SHORT_OF_WHAT_IS_HELD_OR_WANTED}",
                    scoped.from
                ),
                scoped.binds,
            )?,
            None => 0,
        };
        let (held, binds) = unheld_holding(narrowing);
        let releases = self.counted(&format!("{UNHELD_COUNTED}{held}"), binds)?;

        Ok(Missing {
            tracks: u64::from(tracks),
            releases: u64::from(releases),
        })
    }

    pub fn portrait(&self, id: ArtistId) -> Result<Option<CoverArt>> {
        let stored = self.inner.read(|connection| {
            connection
                .query_row(
                    "SELECT portrait, portrait_format FROM artists WHERE id = ?1",
                    params![id.get() as i64],
                    |row| {
                        Ok((
                            row.get::<_, Option<Vec<u8>>>(0)?,
                            row.get::<_, Option<i64>>(1)?,
                        ))
                    },
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))
        })?;

        let Some((Some(bytes), code)) = stored else {
            return Ok(None);
        };
        let format = match code {
            Some(code) => store::portrait_format_of(code, id)?,
            None => ImageFormat::sniff(&bytes).ok_or(Error::UntypedPortrait { artist: id })?,
        };

        Ok(Some(CoverArt { format, bytes }))
    }

    pub fn kept_lyrics(
        &self,
        location: &MediaLocation,
        span: Option<FrameSpan>,
    ) -> Result<Option<KeptLyrics>> {
        let path = playlist::local_path(location)?;
        let (start, _) = store::span_columns(span);

        self.inner.read(|connection| {
            connection
                .query_row(
                    "SELECT text, synced, taken FROM lyrics_kept
                      WHERE path = ?1 AND span_start = ?2",
                    params![path, start],
                    |row| {
                        Ok(KeptLyrics {
                            text: row.get(0)?,
                            synced: row.get(1)?,
                            taken: store::from_nanos(row.get(2)?),
                        })
                    },
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))
        })
    }

    pub fn keep_lyrics(
        &self,
        location: &MediaLocation,
        span: Option<FrameSpan>,
        text: Option<&str>,
        synced: bool,
    ) -> Result<()> {
        let path = playlist::local_path(location)?;
        let (start, _) = store::span_columns(span);

        self.inner.write(|transaction| {
            transaction
                .execute(
                    "INSERT INTO lyrics_kept (path, span_start, text, synced, taken)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(path, span_start) DO UPDATE SET
                         text   = excluded.text,
                         synced = excluded.synced,
                         taken  = excluded.taken",
                    params![
                        path,
                        start,
                        text,
                        synced,
                        store::to_nanos(SystemTime::now())
                    ],
                )
                .map_err(|source| Error::store(StoreOp::Insert, source))?;
            store::index_what_is_sung(transaction, path, start, text)
        })
    }

    pub fn resumption(&self) -> Result<Option<Resumption>> {
        self.inner.read(|connection| {
            let Some((row, at, shuffle)) = connection
                .query_row(
                    "SELECT row, at, shuffle FROM resume WHERE id = 1",
                    [],
                    |read| {
                        Ok((
                            read.get::<_, i64>(0)?,
                            read.get::<_, i64>(1)?,
                            read.get::<_, bool>(2)?,
                        ))
                    },
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))?
            else {
                return Ok(None);
            };

            let mut statement = connection
                .prepare(
                    "SELECT uri, span_start, span_frames
                     FROM resume_rows ORDER BY position",
                )
                .map_err(|source| Error::store(StoreOp::Prepare, source))?;
            let kept = statement
                .query_map([], |read| {
                    Ok((
                        read.get::<_, String>(0)?,
                        store::span(read.get(1)?, read.get(2)?),
                    ))
                })
                .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
                .map_err(|source| Error::store(StoreOp::Query, source))?;

            let mut rows = Vec::with_capacity(kept.len());
            for (uri, span) in kept {
                let Some(location) = MediaLocation::from_uri(&uri) else {
                    tracing::warn!(
                        uri,
                        "a kept queue row names nothing openable; none is resumed"
                    );
                    return Ok(None);
                };
                rows.push(Resumable { location, span });
            }

            let mut statement = connection
                .prepare("SELECT loaded_at FROM resume_order ORDER BY position")
                .map_err(|source| Error::store(StoreOp::Prepare, source))?;
            let order = statement
                .query_map([], |read| Ok(read.get::<_, i64>(0)?.max(0) as usize))
                .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
                .map_err(|source| Error::store(StoreOp::Query, source))?;

            Ok((!rows.is_empty()).then(|| Resumption {
                rows,
                order,
                row: row.max(0) as usize,
                at: Frames(at.max(0) as u64),
                shuffle,
            }))
        })
    }

    pub fn keep_resumption(&self, resumption: &Resumption) -> Result<()> {
        self.inner.write(|transaction| {
            transaction
                .execute("DELETE FROM resume_rows", [])
                .map_err(|source| Error::store(StoreOp::Delete, source))?;

            for (position, row) in resumption.rows.iter().enumerate() {
                let (start, frames) = store::span_columns(row.span);
                transaction
                    .execute(
                        "INSERT INTO resume_rows (position, uri, span_start, span_frames)
                         VALUES (?1, ?2, ?3, ?4)",
                        params![position as i64, row.location.to_uri(), start, frames],
                    )
                    .map_err(|source| Error::store(StoreOp::Insert, source))?;
            }

            keep_order(
                transaction,
                &resumption.order,
                resumption.row,
                resumption.at,
                resumption.shuffle,
            )
        })
    }

    pub fn keep_order(&self, reordered: &Reordered) -> Result<()> {
        self.inner.write(|transaction| {
            keep_order(
                transaction,
                &reordered.order,
                reordered.row,
                reordered.at,
                reordered.shuffle,
            )
        })
    }

    pub fn keep_place(&self, row: usize, at: Frames) -> Result<()> {
        self.inner
            .write(|transaction| keep_place(transaction, row, at))
    }

    pub fn forget_resumption(&self) -> Result<()> {
        self.inner.write(|transaction| {
            for emptied in ["resume_rows", "resume_order", "resume"] {
                transaction
                    .execute(&format!("DELETE FROM {emptied}"), [])
                    .map_err(|source| Error::store(StoreOp::Delete, source))?;
            }

            Ok(())
        })
    }

    pub fn kept_corrections_index(&self) -> Result<Option<KeptIndex>> {
        self.inner.read(|connection| {
            connection
                .query_row(
                    "SELECT text, taken FROM corrections_index WHERE id = 1",
                    [],
                    |row| {
                        Ok(KeptIndex {
                            text: row.get(0)?,
                            taken: store::from_nanos(row.get(1)?),
                        })
                    },
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))
        })
    }

    pub fn keep_corrections_index(&self, text: &str) -> Result<()> {
        self.inner.write(|transaction| {
            transaction
                .execute(
                    "INSERT INTO corrections_index (id, text, taken)
                     VALUES (1, ?1, ?2)
                     ON CONFLICT(id) DO UPDATE SET
                         text  = excluded.text,
                         taken = excluded.taken",
                    params![text, store::to_nanos(SystemTime::now())],
                )
                .map(drop)
                .map_err(|source| Error::store(StoreOp::Insert, source))
        })
    }

    pub fn kept_correction(&self, device: &str) -> Result<Option<KeptCorrection>> {
        self.inner.read(|connection| {
            connection
                .query_row(
                    "SELECT text, taken FROM corrections_kept WHERE device = ?1",
                    params![device],
                    |row| {
                        Ok(KeptCorrection {
                            text: row.get(0)?,
                            taken: store::from_nanos(row.get(1)?),
                        })
                    },
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))
        })
    }

    pub fn keep_correction(&self, device: &str, text: Option<&str>) -> Result<()> {
        self.inner.write(|transaction| {
            transaction
                .execute(
                    "INSERT INTO corrections_kept (device, text, taken)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(device) DO UPDATE SET
                         text  = excluded.text,
                         taken = excluded.taken",
                    params![device, text, store::to_nanos(SystemTime::now())],
                )
                .map(drop)
                .map_err(|source| Error::store(StoreOp::Insert, source))
        })
    }

    pub fn want(&self, release_track: ReleaseTrackId) -> Result<WantId> {
        self.inner
            .write(|transaction| elsewhere::want_in(transaction, release_track, SystemTime::now()))
    }

    pub fn unwant(&self, id: WantId) -> Result<bool> {
        self.inner.write(|transaction| {
            transaction
                .execute("DELETE FROM wants WHERE id = ?1", params![id.get() as i64])
                .map(|removed| removed > 0)
                .map_err(|source| Error::store(StoreOp::Delete, source))
        })
    }

    pub fn wants(&self) -> Result<Vec<Want>> {
        self.inner.read(|connection| {
            let held = rows(connection, WANTS, Vec::new(), |row| {
                RawWant::read(row).map(Ok)
            })?;
            let mut links = grouped_links(connection, LINKS_OF_WANTS, Vec::new())?;
            let release_links = grouped_links(connection, LINKS_OF_WANTED_RELEASES, Vec::new())?;

            held.into_iter()
                .map(|raw| {
                    let links = links.remove(&raw.release_track).unwrap_or_default();
                    let release_links = release_links.get(&raw.album).cloned().unwrap_or_default();
                    raw.into_want(links, release_links)
                })
                .collect()
        })
    }

    pub fn note_tried(&self, id: WantId, offered: Option<&MediaLocation>) -> Result<()> {
        self.inner.write(|transaction| {
            let changed = transaction
                .execute(
                    "UPDATE wants SET tried = ?1, offered = coalesce(?2, offered) WHERE id = ?3",
                    params![
                        store::to_nanos(SystemTime::now()),
                        offered.map(MediaLocation::to_uri),
                        id.get() as i64
                    ],
                )
                .map_err(|source| Error::store(StoreOp::Update, source))?;
            if changed == 0 {
                return Err(Error::UnknownWant(id));
            }
            Ok(())
        })
    }

    pub fn artist_named(&self, name: &str) -> Result<Option<ArtistId>> {
        self.inner.read(|connection| {
            connection
                .query_row(
                    "SELECT id FROM artists WHERE key = ?1",
                    params![store::folded_letters(name)],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(|source| Error::store(StoreOp::Query, source))?
                .map(|id| ArtistId::new(id as u64).map_err(Error::from))
                .transpose()
        })
    }

    pub fn stamp_album_asked(&self, album: AlbumId, why: Fruitless) -> Result<()> {
        self.inner.write(|transaction| {
            enriched::stamp_album_asked(transaction, album, why, SystemTime::now())
        })
    }

    pub fn stamp_artist_asked(&self, artist: ArtistId, why: Fruitless) -> Result<()> {
        self.inner.write(|transaction| {
            enriched::stamp_artist_asked(transaction, artist, why, SystemTime::now())
        })
    }

    pub fn land_release(&self, album: AlbumId, release: &Release) -> Result<()> {
        self.inner.write(|transaction| {
            enriched::land_release(transaction, album, release, SystemTime::now())
        })
    }

    pub fn stamp_track_asked(&self, track: TrackId, why: Fruitless) -> Result<()> {
        self.inner.write(|transaction| {
            enriched::stamp_track_asked(transaction, track, why, SystemTime::now())
        })
    }

    pub fn land_recording(
        &self,
        track: TrackId,
        recording: &Recording,
        certainty: Certainty,
        release: Option<&RecordingRelease>,
    ) -> Result<bool> {
        self.inner.write(|transaction| {
            enriched::land_recording(
                transaction,
                track,
                recording,
                certainty,
                release,
                SystemTime::now(),
            )
        })
    }

    pub fn land_release_group(&self, album: AlbumId, group: &ReleaseGroup) -> Result<()> {
        self.inner.write(|transaction| {
            let now = SystemTime::now();
            enriched::stamp_album_asked(transaction, album, Fruitless::Missed, now)?;
            enriched::land_release_group(transaction, album, group, now)
        })
    }

    pub(crate) fn note_enrichment_began(&self, refresh: bool) -> Result<()> {
        self.inner.write(|transaction| {
            enriched::note_enrichment_began(transaction, refresh, SystemTime::now())
        })
    }

    pub(crate) fn note_enrichment_finished(&self) -> Result<()> {
        self.inner.write(enriched::note_enrichment_finished)
    }

    pub fn unfinished_enrichment(&self) -> Result<Option<Unfinished>> {
        self.inner.read(enriched::unfinished_enrichment)
    }

    pub fn rematch(&self, album: AlbumId) -> Result<u32> {
        self.inner
            .write(|transaction| enriched::rematch_release_tracks(transaction, album))
    }

    pub fn land_archive_cover(&self, album: AlbumId, art: &CoverArt) -> Result<bool> {
        let Some(vault) = self.inner.vault.clone() else {
            return self
                .inner
                .write(|transaction| enriched::land_archive_cover(transaction, album, art));
        };

        let kept = vault.keep_cover(art).map_err(|source| Error::Vault {
            path: vault.root().to_path_buf(),
            source: Box::new(source),
        })?;
        let within = self.inner.within_the_vault(&kept.path)?;
        self.inner
            .write(|transaction| enriched::land_vault_cover(transaction, album, kept.key, &within))
    }

    pub fn land_artist(&self, artist: ArtistId, profile: &ArtistProfile) -> Result<()> {
        self.inner.write(|transaction| {
            enriched::land_artist(transaction, artist, profile, SystemTime::now())
        })
    }

    pub fn land_artist_releases(
        &self,
        artist: ArtistId,
        releases: &[ArtistRelease],
    ) -> Result<usize> {
        self.inner
            .write(|transaction| enriched::land_artist_releases(transaction, artist, releases))
    }

    pub fn land_portrait(&self, artist: ArtistId, art: &CoverArt) -> Result<bool> {
        self.inner
            .write(|transaction| enriched::land_portrait(transaction, artist, art))
    }

    pub fn write_artist_mbid(&self, artist: ArtistId, mbid: &Mbid) -> Result<()> {
        self.inner
            .write(|transaction| enriched::write_artist_mbid(transaction, artist, mbid))
    }

    pub fn albums_to_ask(&self, waits: Waits, refresh: bool) -> Result<Vec<AlbumToAsk>> {
        self.inner.read(|connection| {
            enriched::albums_to_ask(connection, waits, refresh, SystemTime::now())
        })
    }

    pub fn album_to_ask(&self, id: AlbumId) -> Result<Option<AlbumToAsk>> {
        self.inner
            .read(|connection| enriched::album_to_ask(connection, id))
    }

    pub fn album_if_due(
        &self,
        id: AlbumId,
        waits: Waits,
        refresh: bool,
    ) -> Result<Option<AlbumToAsk>> {
        self.inner.read(|connection| {
            enriched::album_if_due(connection, id, waits, refresh, SystemTime::now())
        })
    }

    pub fn artist_is_due(&self, id: ArtistId, waits: Waits, refresh: bool) -> Result<bool> {
        self.inner.read(|connection| {
            enriched::artist_is_due(connection, id, waits, refresh, SystemTime::now())
        })
    }

    pub fn tracks_to_ask(&self, waits: Waits, refresh: bool) -> Result<Vec<TrackId>> {
        self.inner.read(|connection| {
            enriched::tracks_to_ask(connection, waits, refresh, SystemTime::now())
        })
    }

    pub(crate) fn track_as_heard(&self, id: TrackId) -> Result<Option<TrackToAsk>> {
        self.inner
            .read(|connection| enriched::track_as_heard(connection, id))
    }

    pub fn study_of(
        &self,
        location: &MediaLocation,
        span: Option<FrameSpan>,
    ) -> Result<Option<(TrackId, Studied)>> {
        self.inner
            .read(|connection| studies::study_of(connection, location, span))
    }

    pub fn track_held(
        &self,
        location: &MediaLocation,
        span: Option<FrameSpan>,
    ) -> Result<Option<TrackId>> {
        self.inner
            .read(|connection| studies::track_held(connection, location, span))
    }

    pub fn note_study(&self, track: TrackId, study: &Study) -> Result<()> {
        self.inner
            .write(|transaction| studies::write_study(transaction, track, study, SystemTime::now()))
    }

    pub fn note_recognition(
        &self,
        track: TrackId,
        heard: Option<&HeardAs>,
        agreement: Agreement,
    ) -> Result<bool> {
        self.inner.write(|transaction| {
            studies::write_recognition(transaction, track, heard, agreement, SystemTime::now())
        })
    }

    pub fn recognise(
        &self,
        fingerprinters: &Fingerprinters,
        track: TrackId,
        print: Chromaprint,
    ) -> Option<Heard> {
        studies::heard_and_noted(self, fingerprinters, track, print)
    }

    pub fn studies(&self, filter: StudyFilter) -> Result<Vec<StudiedTrack>> {
        self.inner
            .read(|connection| studies::studied(connection, filter))
    }

    pub(crate) fn to_study(&self, again: bool) -> Result<Vec<ToStudy>> {
        self.inner
            .read(|connection| studies::to_study(connection, again))
    }

    pub fn track_to_ask(&self, id: TrackId) -> Result<Option<TrackToAsk>> {
        self.inner
            .read(|connection| enriched::track_to_ask(connection, id))
    }

    pub fn artist_to_ask(&self, id: ArtistId) -> Result<Option<ArtistToAsk>> {
        self.inner
            .read(|connection| enriched::artist_to_ask(connection, id))
    }

    pub fn artists_to_ask(&self, waits: Waits, refresh: bool) -> Result<Vec<ArtistId>> {
        self.inner.read(|connection| {
            enriched::artists_to_ask(connection, waits, refresh, SystemTime::now())
        })
    }
}

struct Scoped {
    from: String,
    binds: Vec<Value>,
    ranked: bool,
}

pub(crate) struct Narrowing {
    pub(crate) sql: String,
    pub(crate) binds: Vec<Value>,
}

#[derive(Default)]
struct Matching {
    ranked: bool,
    join: &'static str,
    filters: Vec<String>,
    binds: Vec<Value>,
}

impl Matching {
    fn narrows(&self) -> bool {
        !self.filters.is_empty()
    }

    fn grouped(&self, column: &str, onto: &str) -> String {
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
    let Some((sql, binds)) = listing(query, narrowing) else {
        return Ok(Vec::new());
    };

    collect(inner, &sql, binds)
}

fn listing(query: &TrackQuery, narrowing: Option<&str>) -> Option<(String, Vec<Value>)> {
    let scoped = scoped(query, narrowing)?;
    let sql = format!(
        "SELECT {TRACK_COLUMNS}{} ORDER BY {} LIMIT ? OFFSET ?",
        scoped.from,
        order_by(query.sort, query.reading, scoped.ranked)
    );

    Some((sql, paged(scoped.binds, query)))
}

pub(crate) fn measured(inner: &Inner, query: &TrackQuery) -> Result<Measured> {
    let Some(scoped) = scoped(query, None) else {
        return Ok(Measured::default());
    };
    let sql = format!(
        "SELECT count(*), sum(seconds), sum(lossless) FROM
         (SELECT tracks.duration * 1.0 / tracks.sample_rate AS seconds,
                 tracks.codec IN ({}) AS lossless{}
          ORDER BY {} LIMIT ? OFFSET ?)",
        lossless_codes(),
        scoped.from,
        order_by(query.sort, query.reading, scoped.ranked)
    );

    inner.read(|connection| {
        connection
            .query_row(&sql, params_from_iter(paged(scoped.binds, query)), |row| {
                Ok(Measured {
                    rows: row.get::<_, i64>(0)? as u32,
                    length: played(row.get::<_, Option<f64>>(1)?),
                    lossless: row.get::<_, Option<i64>>(2)?.unwrap_or_default() as u32,
                })
            })
            .map_err(|source| Error::store(StoreOp::Query, source))
    })
}

const COVERED_ALBUM: &str = "(tracks.album_id IN (SELECT id FROM albums
       WHERE cover_art IS NOT NULL OR cover_path IS NOT NULL))";

const PICTURE_OF_THE_ALBUM: &str = "(SELECT coalesce(a.cover_key,
            length(a.cover_art) || ':' || hex(substr(a.cover_art, 1, 256)))
       FROM albums a WHERE a.id = tracks.album_id)";

const PICTURES_WEIGHED_PER_TILE: usize = 4;

pub(crate) fn pictured_by(
    inner: &Inner,
    query: &TrackQuery,
    at_most: usize,
) -> Result<Vec<AlbumId>> {
    let Some(scoped) = scoped(query, None) else {
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
    let mut seen = AHashSet::new();
    let mut pictured = Vec::with_capacity(at_most);
    for (album, picture) in weighed {
        if picture.is_some_and(|picture| !seen.insert(picture)) {
            continue;
        }
        pictured.push(AlbumId::new(album as u64)?);
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

fn scoped_missing(narrowing: Option<&str>) -> Option<Scoped> {
    narrowed_onto(narrowing, "album_id", "a.id")
}

fn unheld_holding(narrowing: Option<&str>) -> (String, Vec<Value>) {
    let mut held = String::new();
    let mut binds = Vec::new();
    for word in playlist::words_of(narrowing) {
        let anywhere = playlist::anywhere(&store::folded_letters(&word));
        held.push_str(UNHELD_HOLDS_THE_WORD);
        binds.push(Value::Text(anywhere.clone()));
        binds.push(Value::Text(anywhere));
    }

    (held, binds)
}

fn scoped_albums(query: &AlbumQuery, only: Option<&str>) -> Option<Scoped> {
    let mut scoped = narrowed_onto(query.text.as_deref(), "album_id", "a.id")?;
    let mut filters = vec![HOLDS_A_BEST_COPY.to_owned()];

    if let Some(artist) = query.artist {
        let owned_or_played_on = artist.get() as i64;
        filters.push(BY_OR_HOLDING_THE_ARTIST.to_owned());
        scoped.binds.push(Value::Integer(owned_or_played_on));
        scoped.binds.push(Value::Integer(owned_or_played_on));
    }
    filters.extend(only.map(str::to_owned));
    scoped.from.push_str(&clause(&filters));

    Some(scoped)
}

const BY_OR_HOLDING_THE_ARTIST: &str = "(a.artist_id = ?
      OR EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = a.id AND t.artist_id = ?))";

fn scoped_artists(query: &ArtistQuery, only: Option<&str>) -> Option<Scoped> {
    let mut scoped = narrowed_onto(query.text.as_deref(), "artist_id", "r.id")?;
    scoped
        .from
        .push_str(&clause(&Vec::from_iter(only.map(str::to_owned))));

    Some(scoped)
}

fn narrowed_onto(text: Option<&str>, column: &str, onto: &str) -> Option<Scoped> {
    let matching = matching(&[text])?;
    let ranked = matching.ranked;
    if !matching.narrows() {
        return Some(Scoped {
            from: String::new(),
            binds: Vec::new(),
            ranked,
        });
    }

    Some(Scoped {
        from: matching.grouped(column, onto),
        binds: matching.binds,
        ranked,
    })
}

fn scoped(query: &TrackQuery, narrowing: Option<&str>) -> Option<Scoped> {
    let mut matching = matching(&[query.text.as_deref(), narrowing])?;
    matching.filters.push(THE_BEST_COPY.to_owned());

    if let Some(album) = query.album {
        matching.filters.push("tracks.album_id = ?".to_owned());
        matching.binds.push(Value::Integer(album.get() as i64));
    }
    if let Some(artist) = query.artist {
        matching.filters.push("tracks.artist_id = ?".to_owned());
        matching.binds.push(Value::Integer(artist.get() as i64));
    }

    Some(Scoped {
        from: format!(" FROM tracks{}{}", matching.join, clause(&matching.filters)),
        binds: matching.binds,
        ranked: matching.ranked,
    })
}

fn matching(texts: &[Option<&str>]) -> Option<Matching> {
    let asking: Vec<Search> = texts
        .iter()
        .flatten()
        .map(|text| Search::read(text))
        .collect();
    if asking.is_empty() {
        return Some(Matching::default());
    }

    let mut ranked: Vec<&Word> = Vec::new();
    let mut narrowing: Vec<Narrowing> = Vec::new();
    for clause in asking.iter().flat_map(|search| &search.clauses) {
        match clause.lone_word() {
            Some(word) => ranked.push(word),
            None => narrowing.extend(any_of(clause)),
        }
    }

    let indexed = indexed(&ranked);
    if indexed.is_none() && narrowing.is_empty() {
        return None;
    }

    let mut matching = Matching::default();
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

pub(crate) fn cuts_matching(text: &str, row: &str) -> Option<Narrowing> {
    let matching = matching(&[Some(text)])?;

    Some(Narrowing {
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

fn narrowed(condition: &Condition) -> Option<Narrowing> {
    match condition {
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

fn joined(mut held: Vec<String>, by: &str) -> Option<String> {
    match held.len() {
        0 => None,
        1 => Some(held.remove(0)),
        _ => Some(format!("({})", held.join(by))),
    }
}

fn indexed(words: &[&Word]) -> Option<String> {
    let mut matched = Vec::with_capacity(words.len());

    for word in words {
        let pieces = search::pieces_of(word);
        if pieces.is_empty() {
            continue;
        }

        let scoped = match word.column {
            Some(column) => format!("{} : ", column.name()),
            None => format!("{{{}}} : ", names_a_bare_word_reaches()),
        };
        if word.phrase {
            matched.push(format!("{scoped}\"{}\"", pieces.join(" ")));
            continue;
        }
        for piece in pieces {
            matched.push(format!("{scoped}\"{piece}\"*"));
        }
    }

    (!matched.is_empty()).then(|| matched.join(" "))
}

fn names_a_bare_word_reaches() -> String {
    Column::NAMES
        .iter()
        .map(|column| column.name())
        .collect::<Vec<_>>()
        .join(" ")
}

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
        Term::Length { compare, length } => {
            binds.push(Value::Real(length.as_secs_f64()));
            format!(
                "(tracks.duration IS NOT NULL \
                 AND tracks.duration * 1.0 / tracks.sample_rate {} ?)",
                compare.operator()
            )
        }
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

const fn holds(compare: Compare, held: u8, wanted: u8) -> bool {
    match compare {
        Compare::Below => held < wanted,
        Compare::AtMost => held <= wanted,
        Compare::Exactly => held == wanted,
        Compare::AtLeast => held >= wanted,
        Compare::Above => held > wanted,
    }
}

fn coded(column: &str, codes: impl Iterator<Item = i64>) -> String {
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

fn paged(mut binds: Vec<Value>, query: &TrackQuery) -> Vec<Value> {
    binds.push(Value::Integer(limit(query.limit)));
    binds.push(Value::Integer(query.offset as i64));
    binds
}

fn played(seconds: Option<f64>) -> Option<Duration> {
    seconds
        .filter(|seconds| *seconds > 0.0)
        .map(|seconds| Duration::try_from_secs_f64(seconds).unwrap_or_default())
}

fn collect(inner: &Inner, sql: &str, binds: Vec<Value>) -> Result<Vec<Track>> {
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

fn keep_order(
    transaction: &rusqlite::Transaction<'_>,
    order: &[usize],
    row: usize,
    at: Frames,
    shuffle: bool,
) -> Result<()> {
    transaction
        .execute("DELETE FROM resume_order", [])
        .map_err(|source| Error::store(StoreOp::Delete, source))?;

    for (position, loaded_at) in order.iter().enumerate() {
        transaction
            .execute(
                "INSERT INTO resume_order (position, loaded_at) VALUES (?1, ?2)",
                params![
                    i64::try_from(position).unwrap_or(i64::MAX),
                    i64::try_from(*loaded_at).unwrap_or(i64::MAX)
                ],
            )
            .map_err(|source| Error::store(StoreOp::Insert, source))?;
    }

    keep_place(transaction, row, at)?;
    transaction
        .execute(
            "UPDATE resume SET shuffle = ?1 WHERE id = 1",
            params![shuffle],
        )
        .map(drop)
        .map_err(|source| Error::store(StoreOp::Update, source))
}

fn keep_place(transaction: &rusqlite::Transaction<'_>, row: usize, at: Frames) -> Result<()> {
    transaction
        .execute(
            "INSERT INTO resume (id, row, at, taken)
             VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET
                 row   = excluded.row,
                 at    = excluded.at,
                 taken = excluded.taken",
            params![
                i64::try_from(row).unwrap_or(i64::MAX),
                i64::try_from(at.get()).unwrap_or(i64::MAX),
                store::to_nanos(SystemTime::now())
            ],
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
        named.push(Value::Text(store::path_text(root)?.to_owned()));
    }
    Ok(named)
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

struct RawToTag {
    id: i64,
    path: String,
    span_start: i64,
    span_frames: Option<i64>,
    answered: bool,
    title: String,
    artist: Option<String>,
    artist_mbid: Option<String>,
    mbid: Option<String>,
    release_track_mbid: Option<String>,
    isrc: Option<String>,
    track_number: Option<i64>,
    disc_number: Option<i64>,
    album: Option<i64>,
    album_answered: bool,
    release_title: Option<String>,
    album_artist_answered: bool,
    album_artist: Option<String>,
    album_artist_mbid: Option<String>,
    album_mbid: Option<String>,
    release_group: Option<String>,
    date: Option<String>,
    label: Option<String>,
    catalog_number: Option<String>,
    barcode: Option<String>,
    vaulted: bool,
}

impl RawToTag {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            path: row.get(1)?,
            span_start: row.get(2)?,
            span_frames: row.get(3)?,
            answered: row.get(4)?,
            title: row.get(5)?,
            artist: row.get(6)?,
            artist_mbid: row.get(7)?,
            mbid: row.get(8)?,
            release_track_mbid: row.get(9)?,
            isrc: row.get(10)?,
            track_number: row.get(11)?,
            disc_number: row.get(12)?,
            album: row.get(13)?,
            album_answered: row.get(14)?,
            release_title: row.get(15)?,
            album_artist_answered: row.get(16)?,
            album_artist: row.get(17)?,
            album_artist_mbid: row.get(18)?,
            album_mbid: row.get(19)?,
            release_group: row.get(20)?,
            date: row.get(21)?,
            label: row.get(22)?,
            catalog_number: row.get(23)?,
            barcode: row.get(24)?,
            vaulted: row.get(25)?,
        })
    }

    fn into_tagged(
        self,
        tracks: &AHashMap<(i64, i64), u32>,
        discs: &AHashMap<i64, u32>,
    ) -> Result<TrackToTag> {
        let seat = self.disc_number.unwrap_or(FIRST_DISC);
        Ok(TrackToTag {
            id: TrackId::new(self.id as u64)?,
            path: PathBuf::from(self.path),
            album_id: self.album.map(|id| AlbumId::new(id as u64)).transpose()?,
            cut: store::span(self.span_start, self.span_frames).is_some(),
            vaulted: self.vaulted,
            answered: self.answered,
            title: self.title,
            artist: self.artist,
            artist_mbid: self.artist_mbid,
            mbid: self.mbid,
            release_track_mbid: self.release_track_mbid,
            isrc: self.isrc,
            track_number: counting(self.track_number),
            disc_number: counting(self.disc_number),
            album_answered: self.album_answered,
            album: self.release_title,
            album_artist_answered: self.album_artist_answered,
            album_artist: self.album_artist,
            album_artist_mbid: self.album_artist_mbid,
            album_mbid: self.album_mbid,
            release_group: self.release_group,
            date: self.date,
            label: self.label,
            catalog_number: self.catalog_number,
            barcode: self.barcode,
            track_total: self
                .album
                .and_then(|album| tracks.get(&(album, seat)).copied()),
            disc_total: self.album.and_then(|album| discs.get(&album).copied()),
        })
    }
}

fn counting(value: Option<i64>) -> Option<u32> {
    value.and_then(|value| u32::try_from(value).ok())
}

struct RawFiled {
    id: i64,
    path: String,
    root: String,
    title: String,
    artist: Option<String>,
    album: Option<i64>,
    album_title: Option<String>,
    year: Option<i64>,
    album_artist: Option<String>,
    track: Option<i64>,
    disc: Option<i64>,
}

impl RawFiled {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            path: row.get(1)?,
            root: row.get(2)?,
            title: row.get(3)?,
            artist: row.get(4)?,
            album: row.get(5)?,
            album_title: row.get(6)?,
            year: row.get(7)?,
            album_artist: row.get(8)?,
            track: row.get(9)?,
            disc: row.get(10)?,
        })
    }

    fn into_filed(self, discs: &AHashMap<i64, u32>) -> Result<TrackToFile> {
        Ok(TrackToFile {
            id: TrackId::new(self.id as u64)?,
            path: PathBuf::from(self.path),
            root: PathBuf::from(self.root),
            title: self.title,
            artist: self.artist,
            album: self.album_title,
            album_id: self.album.map(|id| AlbumId::new(id as u64)).transpose()?,
            album_artist: self.album_artist,
            year: self.year.and_then(|year| i32::try_from(year).ok()),
            track: self.track.and_then(|number| u32::try_from(number).ok()),
            disc: self
                .disc
                .and_then(|number| u32::try_from(number).ok())
                .and_then(NonZeroU32::new),
            discs: self
                .album
                .and_then(|album| discs.get(&album).copied())
                .unwrap_or_default(),
        })
    }
}

fn rows<T>(
    connection: &Connection,
    sql: &str,
    binds: Vec<Value>,
    read: impl Fn(&Row<'_>) -> rusqlite::Result<Result<T>>,
) -> Result<Vec<T>> {
    let mut statement = connection
        .prepare(sql)
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let found = statement
        .query_map(params_from_iter(binds), read)
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    found.into_iter().collect()
}

const fn limit(limit: Option<usize>) -> i64 {
    match limit {
        Some(limit) => limit as i64,
        None => -1,
    }
}

const fn read_as(direction: Direction, order: Reading) -> &'static str {
    match direction {
        Direction::Ascending => order.up,
        Direction::Descending => order.down,
    }
}

struct Reading {
    up: &'static str,
    down: &'static str,
}

const fn order_by(sort: SortOrder, reading: Direction, ranked: bool) -> &'static str {
    let order = match sort {
        SortOrder::Relevance if ranked => Reading {
            up: "rank",
            down: "rank DESC",
        },
        SortOrder::Relevance | SortOrder::AlbumThenTrack => Reading {
            up: "tracks.album_id, tracks.disc_number, tracks.track_number, \
                 tracks.title COLLATE NOCASE",
            down: "tracks.album_id DESC, tracks.disc_number DESC, tracks.track_number DESC, \
                   tracks.title COLLATE NOCASE DESC",
        },
        SortOrder::Title => Reading {
            up: "tracks.title COLLATE NOCASE",
            down: "tracks.title COLLATE NOCASE DESC",
        },
        SortOrder::Artist => Reading {
            up: "tracks.artist COLLATE NOCASE, tracks.album_id, tracks.disc_number, \
                 tracks.track_number",
            down: "tracks.artist COLLATE NOCASE DESC, tracks.album_id DESC, \
                   tracks.disc_number DESC, tracks.track_number DESC",
        },
        SortOrder::DateAdded => Reading {
            up: "tracks.added",
            down: "tracks.added DESC",
        },
        SortOrder::Duration => Reading {
            up: "tracks.duration",
            down: "tracks.duration DESC",
        },
        SortOrder::Plays => Reading {
            up: "tracks.plays, tracks.title COLLATE NOCASE DESC",
            down: "tracks.plays DESC, tracks.title COLLATE NOCASE",
        },
        SortOrder::Played => Reading {
            up: "tracks.played, tracks.title COLLATE NOCASE DESC",
            down: "tracks.played DESC, tracks.title COLLATE NOCASE",
        },
        SortOrder::Favourited => Reading {
            up: "tracks.favourite, tracks.title COLLATE NOCASE DESC",
            down: "tracks.favourite DESC, tracks.title COLLATE NOCASE",
        },
    };

    read_as(reading, order)
}

const fn album_order_by(sort: AlbumOrder, reading: Direction, ranked: bool) -> &'static str {
    let order = match sort {
        AlbumOrder::Relevance if ranked => Reading {
            up: concat!("matched.score, ", album_title!(), " COLLATE NOCASE"),
            down: concat!(
                "matched.score DESC, ",
                album_title!(),
                " COLLATE NOCASE DESC"
            ),
        },
        AlbumOrder::Relevance | AlbumOrder::Title => Reading {
            up: concat!(album_title!(), " COLLATE NOCASE"),
            down: concat!(album_title!(), " COLLATE NOCASE DESC"),
        },
        AlbumOrder::Artist => Reading {
            up: concat!(
                album_owner!(),
                " COLLATE NOCASE, a.year, ",
                album_title!(),
                " COLLATE NOCASE"
            ),
            down: concat!(
                album_owner!(),
                " COLLATE NOCASE DESC, a.year DESC, ",
                album_title!(),
                " COLLATE NOCASE DESC"
            ),
        },
        AlbumOrder::Year => Reading {
            up: concat!(
                "a.year IS NULL, a.year, ",
                album_title!(),
                " COLLATE NOCASE"
            ),
            down: concat!(
                "a.year IS NULL DESC, a.year DESC, ",
                album_title!(),
                " COLLATE NOCASE DESC"
            ),
        },
        AlbumOrder::Tracks => Reading {
            up: concat!(album_tracks!(), ", ", album_title!(), " COLLATE NOCASE"),
            down: concat!(
                album_tracks!(),
                " DESC, ",
                album_title!(),
                " COLLATE NOCASE DESC"
            ),
        },
        AlbumOrder::Added => Reading {
            up: concat!(album_added!(), ", ", album_title!(), " COLLATE NOCASE"),
            down: concat!(
                album_added!(),
                " DESC, ",
                album_title!(),
                " COLLATE NOCASE DESC"
            ),
        },
        AlbumOrder::Favourited => Reading {
            up: concat!("a.favourite, ", album_title!(), " COLLATE NOCASE"),
            down: concat!("a.favourite DESC, ", album_title!(), " COLLATE NOCASE DESC"),
        },
    };

    read_as(reading, order)
}

const fn artist_order_by(sort: ArtistOrder, reading: Direction, ranked: bool) -> &'static str {
    let order = match sort {
        ArtistOrder::Relevance if ranked => Reading {
            up: "matched.score, r.name COLLATE NOCASE",
            down: "matched.score DESC, r.name COLLATE NOCASE DESC",
        },
        ArtistOrder::Relevance | ArtistOrder::Name => Reading {
            up: "r.name COLLATE NOCASE",
            down: "r.name COLLATE NOCASE DESC",
        },
        ArtistOrder::Albums => Reading {
            up: concat!(artist_albums!(), ", r.name COLLATE NOCASE"),
            down: concat!(artist_albums!(), " DESC, r.name COLLATE NOCASE DESC"),
        },
        ArtistOrder::Tracks => Reading {
            up: concat!(artist_tracks!(), ", r.name COLLATE NOCASE"),
            down: concat!(artist_tracks!(), " DESC, r.name COLLATE NOCASE DESC"),
        },
        ArtistOrder::Favourited => Reading {
            up: "r.favourite, r.name COLLATE NOCASE",
            down: "r.favourite DESC, r.name COLLATE NOCASE DESC",
        },
    };

    read_as(reading, order)
}

pub(crate) struct RawTrack {
    id: i64,
    path: String,
    title: String,
    artist: Option<String>,
    album_id: Option<i64>,
    track_number: Option<u32>,
    disc_number: Option<u32>,
    duration: Option<i64>,
    sample_rate: u32,
    channels: u16,
    sample_format: i64,
    codec: i64,
    rg_track_gain: Option<f64>,
    rg_track_peak: Option<f64>,
    rg_album_gain: Option<f64>,
    rg_album_peak: Option<f64>,
    file_size: i64,
    modified: i64,
    added: i64,
    plays: u32,
    played: Option<i64>,
    span_start: i64,
    span_frames: Option<i64>,
    artist_id: Option<i64>,
    favourite: Option<i64>,
    genre: Option<String>,
    alternatives: u32,
}

impl RawTrack {
    pub(crate) fn read_where_joined(row: &Row<'_>) -> rusqlite::Result<Option<Self>> {
        if row.get::<_, Option<i64>>(0)?.is_none() {
            return Ok(None);
        }
        Self::read(row).map(Some)
    }

    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            path: row.get(1)?,
            title: row.get(2)?,
            artist: row.get(3)?,
            album_id: row.get(4)?,
            track_number: row.get(5)?,
            disc_number: row.get(6)?,
            duration: row.get(7)?,
            sample_rate: row.get(8)?,
            channels: row.get(9)?,
            sample_format: row.get(10)?,
            codec: row.get(11)?,
            rg_track_gain: row.get(12)?,
            rg_track_peak: row.get(13)?,
            rg_album_gain: row.get(14)?,
            rg_album_peak: row.get(15)?,
            file_size: row.get(16)?,
            modified: row.get(17)?,
            added: row.get(18)?,
            plays: row.get(19)?,
            played: row.get(20)?,
            span_start: row.get(21)?,
            span_frames: row.get(22)?,
            artist_id: row.get(23)?,
            favourite: row.get(24)?,
            genre: row.get(25)?,
            alternatives: row.get(26)?,
        })
    }

    pub(crate) fn into_track(self) -> Result<Track> {
        let id = TrackId::new(self.id as u64)?;
        let spec = StreamSpec::new(
            SampleRate::new(self.sample_rate)?,
            ChannelLayout::from_count(ChannelCount::new(self.channels)?),
            store::format_from_code(self.sample_format, id)?,
        );

        Ok(Track {
            id,
            location: MediaLocation::local(self.path),
            title: self.title,
            artist: self.artist,
            artist_id: self
                .artist_id
                .map(|id| ArtistId::new(id as u64))
                .transpose()?,
            album_id: self
                .album_id
                .map(|id| AlbumId::new(id as u64))
                .transpose()?,
            track_number: self.track_number,
            disc_number: self.disc_number,
            duration: self.duration.map(|frames| Frames(frames as u64)),
            spec,
            codec: store::codec_from_code(self.codec, id)?,
            replay_gain: store::replay_gain(
                self.rg_track_gain,
                self.rg_track_peak,
                self.rg_album_gain,
                self.rg_album_peak,
            ),
            file_size: self.file_size as u64,
            modified: store::from_nanos(self.modified),
            added: store::from_nanos(self.added),
            plays: self.plays,
            played: self.played.map(store::from_nanos),
            span: store::span(self.span_start, self.span_frames),
            favourite: self.favourite.map(store::from_nanos),
            genre: self.genre,
            alternatives: self.alternatives,
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TrackToVault {
    pub id: TrackId,
    pub path: PathBuf,
    pub span: Option<FrameSpan>,
    pub file_size: u64,
    pub codec: Codec,
    pub spec: StreamSpec,
    pub album_id: Option<AlbumId>,
    pub renewing: bool,
}

impl TrackToVault {
    pub(crate) fn location(&self) -> MediaLocation {
        MediaLocation::local(&self.path)
    }
}

struct RawToVault {
    id: i64,
    path: String,
    span_start: i64,
    span_frames: Option<i64>,
    file_size: i64,
    codec: i64,
    sample_rate: u32,
    channels: u16,
    sample_format: i64,
    album_id: Option<i64>,
    renewing: bool,
}

impl RawToVault {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            path: row.get(1)?,
            span_start: row.get(2)?,
            span_frames: row.get(3)?,
            file_size: row.get(4)?,
            codec: row.get(5)?,
            sample_rate: row.get(6)?,
            channels: row.get(7)?,
            sample_format: row.get(8)?,
            album_id: row.get(9)?,
            renewing: row.get(10)?,
        })
    }

    fn into_vaultable(self) -> Result<TrackToVault> {
        let id = TrackId::new(self.id as u64)?;
        Ok(TrackToVault {
            id,
            path: PathBuf::from(self.path),
            span: store::span(self.span_start, self.span_frames),
            file_size: self.file_size as u64,
            codec: store::codec_from_code(self.codec, id)?,
            spec: StreamSpec::new(
                SampleRate::new(self.sample_rate)?,
                ChannelLayout::from_count(ChannelCount::new(self.channels)?),
                store::format_from_code(self.sample_format, id)?,
            ),
            album_id: self
                .album_id
                .map(|held| AlbumId::new(held as u64))
                .transpose()?,
            renewing: self.renewing,
        })
    }
}

struct RawVaultObject {
    key: String,
    form: i64,
    path: String,
    bytes: i64,
    sample_rate: u32,
    channels: u16,
    sample_format: i64,
    frames: Option<i64>,
    taken_from: String,
    took: i64,
    was_bytes: i64,
    was_codec: i64,
    validated: bool,
    encoding: u16,
}

impl RawVaultObject {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            key: row.get(0)?,
            form: row.get(1)?,
            path: row.get(2)?,
            bytes: row.get(3)?,
            sample_rate: row.get(4)?,
            channels: row.get(5)?,
            sample_format: row.get(6)?,
            frames: row.get(7)?,
            taken_from: row.get(8)?,
            took: row.get(9)?,
            was_bytes: row.get(10)?,
            was_codec: row.get(11)?,
            validated: row.get(12)?,
            encoding: row.get(13)?,
        })
    }

    fn into_object(self, inner: &Inner) -> Result<VaultObject> {
        let key = VaultKey::read(&self.key).map_err(|_| Error::NotAVaultKey {
            named: self.key.clone().into_boxed_str(),
        })?;

        Ok(VaultObject {
            key,
            form: store::vault_form_of(self.form)?,
            path: inner.in_the_vault(&self.path)?,
            bytes: self.bytes as u64,
            spec: StreamSpec::new(
                SampleRate::new(self.sample_rate)?,
                ChannelLayout::from_count(ChannelCount::new(self.channels)?),
                store::vault_format_of(key, self.sample_format)?,
            ),
            frames: self.frames.map(|held| Frames(held as u64)),
            taken_from: PathBuf::from(self.taken_from),
            took: store::from_nanos(self.took),
            was_bytes: self.was_bytes as u64,
            was_codec: store::vault_codec_of(key, self.was_codec)?,
            validated: self.validated,
            encoding: Encoding::of(self.encoding),
        })
    }
}

fn read_album(row: &Row<'_>) -> rusqlite::Result<Result<Album>> {
    RawAlbum::read(row).map(RawAlbum::into_album)
}

struct RawAlbum {
    id: i64,
    title: String,
    artist_id: Option<i64>,
    year: Option<i32>,
    track_count: u32,
    artist_count: u32,
    has_cover_art: bool,
    mbid: Option<String>,
    missing: u32,
    artist: Option<String>,
    favourite: Option<i64>,
}

impl RawAlbum {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            title: row.get(1)?,
            artist_id: row.get(2)?,
            year: row.get(3)?,
            track_count: row.get(4)?,
            artist_count: row.get(5)?,
            has_cover_art: row.get(6)?,
            mbid: row.get(7)?,
            missing: row.get(8)?,
            artist: row.get(9)?,
            favourite: row.get(10)?,
        })
    }

    fn into_album(self) -> Result<Album> {
        Ok(Album {
            id: AlbumId::new(self.id as u64)?,
            title: self.title,
            artist_id: self
                .artist_id
                .map(|id| ArtistId::new(id as u64))
                .transpose()?,
            artist: self.artist,
            year: self.year,
            track_count: self.track_count,
            artist_count: self.artist_count,
            has_cover_art: self.has_cover_art,
            mbid: mbid_of(self.mbid)?,
            missing: self.missing,
            favourite: self.favourite.map(store::from_nanos),
        })
    }
}

fn read_artist(row: &Row<'_>) -> rusqlite::Result<Result<Artist>> {
    let id: i64 = row.get(0)?;
    let name: String = row.get(1)?;
    let album_count: u32 = row.get(2)?;
    let track_count: u32 = row.get(3)?;
    let mbid: Option<String> = row.get(4)?;
    let has_portrait: bool = row.get(5)?;
    let favourite: Option<i64> = row.get(6)?;

    Ok(ArtistId::new(id as u64)
        .map_err(Error::from)
        .and_then(|id| {
            Ok(Artist {
                id,
                name,
                album_count,
                track_count,
                mbid: mbid_of(mbid)?,
                has_portrait,
                favourite: favourite.map(store::from_nanos),
            })
        }))
}

const fn favoured(what: Favoured) -> (&'static str, i64) {
    match what {
        Favoured::Track(id) => ("tracks", id.get() as i64),
        Favoured::Album(id) => ("albums", id.get() as i64),
        Favoured::Artist(id) => ("artists", id.get() as i64),
    }
}

fn mbid_of(text: Option<String>) -> Result<Option<Mbid>> {
    Ok(text.as_deref().map(Mbid::new).transpose()?)
}

fn read_link(row: &Row<'_>, from: usize) -> rusqlite::Result<Result<Link>> {
    let relation: i64 = row.get(from)?;
    let provider: i64 = row.get(from + 1)?;
    let url: String = row.get(from + 2)?;

    Ok(store::relation_of(relation).and_then(|relation| {
        Ok(Link {
            relation,
            service: store::service_of(provider)?,
            url,
        })
    }))
}

fn read_owned_link(row: &Row<'_>) -> rusqlite::Result<Result<(i64, Link)>> {
    let owner: i64 = row.get(0)?;
    Ok(read_link(row, 1)?.map(|link| (owner, link)))
}

fn grouped_links(
    connection: &Connection,
    sql: &str,
    binds: Vec<Value>,
) -> Result<AHashMap<i64, Vec<Link>>> {
    let mut grouped: AHashMap<i64, Vec<Link>> = AHashMap::new();
    for (owner, link) in rows(connection, sql, binds, read_owned_link)? {
        grouped.entry(owner).or_default().push(link);
    }
    Ok(grouped)
}

struct RawRelease {
    mbid: Option<String>,
    group: Option<String>,
    date: Option<String>,
    country: Option<String>,
    label: Option<String>,
    catalog_number: Option<String>,
    barcode: Option<String>,
    kind: Option<String>,
    disambiguation: Option<String>,
    cover_source: i64,
    asked: Option<i64>,
    answered: Option<i64>,
}

impl RawRelease {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            mbid: row.get(0)?,
            group: row.get(1)?,
            date: row.get(2)?,
            country: row.get(3)?,
            label: row.get(4)?,
            catalog_number: row.get(5)?,
            barcode: row.get(6)?,
            kind: row.get(7)?,
            disambiguation: row.get(8)?,
            cover_source: row.get(9)?,
            asked: row.get(10)?,
            answered: row.get(11)?,
        })
    }

    fn into_detail(
        self,
        album: AlbumId,
        links: Vec<Link>,
        media: Vec<HeldMedium>,
    ) -> Result<ReleaseDetail> {
        Ok(ReleaseDetail {
            mbid: mbid_of(self.mbid)?,
            group: mbid_of(self.group)?,
            date: self.date,
            country: self.country,
            label: self.label,
            catalog_number: self.catalog_number,
            barcode: self.barcode,
            kind: self.kind,
            disambiguation: self.disambiguation,
            cover_source: store::cover_source_of(album, self.cover_source)?,
            asked: self.asked.map(store::from_nanos),
            answered: self.answered.map(store::from_nanos),
            links,
            media,
        })
    }
}

struct RawReleaseTrack {
    id: i64,
    disc: u32,
    position: u32,
    number: Option<String>,
    title: String,
    artist: Option<String>,
    recording: Option<String>,
    track_mbid: Option<String>,
    length_ms: Option<i64>,
    isrc: Option<String>,
    track: Option<i64>,
}

impl RawReleaseTrack {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            disc: row.get(1)?,
            position: row.get(2)?,
            number: row.get(3)?,
            title: row.get(4)?,
            artist: row.get(5)?,
            recording: row.get(6)?,
            track_mbid: row.get(7)?,
            length_ms: row.get(8)?,
            isrc: row.get(9)?,
            track: row.get(10)?,
        })
    }

    fn into_held(self, album: AlbumId, links: Vec<Link>) -> Result<HeldReleaseTrack> {
        Ok(HeldReleaseTrack {
            id: ReleaseTrackId::new(self.id as u64)?,
            album,
            disc: self.disc,
            position: self.position,
            number: self.number,
            title: self.title,
            artist: self.artist,
            recording: mbid_of(self.recording)?,
            track_mbid: mbid_of(self.track_mbid)?,
            length: self
                .length_ms
                .map(|millis| Duration::from_millis(millis.max(0) as u64)),
            isrc: self.isrc,
            track: self.track.map(|id| TrackId::new(id as u64)).transpose()?,
            links,
        })
    }
}

struct RawArtistDetail {
    mbid: Option<String>,
    sort_name: Option<String>,
    kind: Option<String>,
    gender: Option<String>,
    country: Option<String>,
    area: Option<String>,
    began_in: Option<String>,
    began: Option<String>,
    ended: Option<String>,
    has_ended: bool,
    disambiguation: Option<String>,
    asked: Option<i64>,
    answered: Option<i64>,
    name: String,
}

impl RawArtistDetail {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            mbid: row.get(0)?,
            sort_name: row.get(1)?,
            kind: row.get(2)?,
            gender: row.get(3)?,
            country: row.get(4)?,
            area: row.get(5)?,
            began_in: row.get(6)?,
            began: row.get(7)?,
            ended: row.get(8)?,
            has_ended: row.get(9)?,
            disambiguation: row.get(10)?,
            asked: row.get(11)?,
            answered: row.get(12)?,
            name: row.get(13)?,
        })
    }

    fn into_detail(
        self,
        genres: Vec<Genre>,
        links: Vec<Link>,
        releases_unheld: u32,
    ) -> Result<ArtistDetail> {
        Ok(ArtistDetail {
            name: self.name,
            mbid: mbid_of(self.mbid)?,
            sort_name: self.sort_name,
            kind: self.kind,
            gender: self.gender,
            country: self.country,
            area: self.area,
            began_in: self.began_in,
            span: LifeSpan {
                begin: self.began,
                end: self.ended,
                ended: self.has_ended,
            },
            disambiguation: self.disambiguation,
            asked: self.asked.map(store::from_nanos),
            answered: self.answered.map(store::from_nanos),
            genres,
            links,
            releases_unheld,
        })
    }
}

struct RawMissingTrack {
    album: i64,
    album_title: String,
    owner: Option<String>,
    release_track: i64,
    disc: u32,
    position: u32,
    number: Option<String>,
    title: String,
    artist: Option<String>,
    length_ms: Option<i64>,
    want: Option<i64>,
}

impl RawMissingTrack {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            album: row.get(0)?,
            album_title: row.get(1)?,
            owner: row.get(2)?,
            release_track: row.get(3)?,
            disc: row.get(4)?,
            position: row.get(5)?,
            number: row.get(6)?,
            title: row.get(7)?,
            artist: row.get(8)?,
            length_ms: row.get(9)?,
            want: row.get(10)?,
        })
    }

    fn into_missing(self) -> Result<MissingTrack> {
        Ok(MissingTrack {
            album: AlbumId::new(self.album as u64)?,
            album_title: self.album_title,
            owner: self.owner,
            release_track: ReleaseTrackId::new(self.release_track as u64)?,
            disc: self.disc,
            position: self.position,
            number: self.number,
            title: self.title,
            artist: self.artist,
            length: self
                .length_ms
                .map(|millis| Duration::from_millis(millis.max(0) as u64)),
            want: self.want.map(|id| WantId::new(id as u64)).transpose()?,
        })
    }
}

struct RawUnheldRelease {
    artist: i64,
    artist_name: String,
    mbid: String,
    title: String,
    kind: Option<String>,
    first_released: Option<String>,
}

impl RawUnheldRelease {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            artist: row.get(0)?,
            artist_name: row.get(1)?,
            mbid: row.get(2)?,
            title: row.get(3)?,
            kind: row.get(4)?,
            first_released: row.get(5)?,
        })
    }

    fn into_unheld(self) -> Result<UnheldRelease> {
        Ok(UnheldRelease {
            artist: ArtistId::new(self.artist as u64)?,
            artist_name: self.artist_name,
            mbid: Mbid::new(&self.mbid)?,
            title: self.title,
            kind: self.kind,
            first_released: self.first_released,
        })
    }
}

struct RawWant {
    id: i64,
    release_track: i64,
    album: i64,
    album_title: String,
    title: String,
    artist: Option<String>,
    wanted: i64,
    tried: Option<i64>,
    offered: Option<String>,
    recording: Option<String>,
    track: Option<String>,
    release: Option<String>,
    isrc: Option<String>,
    length_ms: Option<i64>,
    disc: i64,
    position: i64,
    held: Option<i64>,
}

impl RawWant {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            release_track: row.get(1)?,
            album: row.get(2)?,
            album_title: row.get(3)?,
            title: row.get(4)?,
            artist: row.get(5)?,
            wanted: row.get(6)?,
            tried: row.get(7)?,
            offered: row.get(8)?,
            recording: row.get(9)?,
            track: row.get(10)?,
            release: row.get(11)?,
            isrc: row.get(12)?,
            length_ms: row.get(13)?,
            disc: row.get(14)?,
            position: row.get(15)?,
            held: row.get(16)?,
        })
    }

    fn into_want(self, links: Vec<Link>, release_links: Vec<Link>) -> Result<Want> {
        Ok(Want {
            id: WantId::new(self.id as u64)?,
            release_track: ReleaseTrackId::new(self.release_track as u64)?,
            album: AlbumId::new(self.album as u64)?,
            album_title: self.album_title,
            title: self.title,
            artist: self.artist,
            recording: store::mbid_in(self.recording.as_deref()),
            track: store::mbid_in(self.track.as_deref()),
            release: store::mbid_in(self.release.as_deref()),
            isrc: store::isrc_in(self.isrc.as_deref()),
            length: self
                .length_ms
                .map(|millis| Duration::from_millis(millis.max(0) as u64)),
            disc: self.disc.max(0) as u32,
            position: self.position.max(0) as u32,
            wanted: store::from_nanos(self.wanted),
            tried: self.tried.map(store::from_nanos),
            offered: self.offered,
            held: self
                .held
                .map(|held| TrackId::new(held as u64))
                .transpose()?,
            links,
            release_links,
        })
    }
}

fn held_cover(id: AlbumId, bytes: Vec<u8>, code: Option<i64>) -> Result<CoverArt> {
    let format = match code {
        Some(code) => store::image_format_from_code(code, id)?,
        None => ImageFormat::sniff(&bytes).ok_or(Error::UntypedCoverArt { album: id })?,
    };
    Ok(CoverArt { format, bytes })
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        num::NonZeroUsize,
        process,
        sync::mpsc::{self, Receiver, Sender},
        thread,
    };

    use super::*;
    use crate::SchemaFingerprint;

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
        let (sql, binds) = listing(query, None).expect("an unnarrowed listing is always scoped");

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
    fn every_order_the_panes_offer_is_read_off_an_index_either_way_round() {
        let library = Library::open_in_memory().expect("an in-memory catalog opens");

        for sort in SortOrder::ALL {
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

    #[test]
    fn a_saved_query_remembers_which_way_round_it_was_read() {
        for sort in SortOrder::ALL {
            for reading in Direction::ALL {
                let code = store::sort_code(sort, reading);
                let playlist = PlaylistId::new(1).expect("one names a playlist");

                assert_eq!(
                    store::sort_of(playlist, code).expect("a code this build wrote reads back"),
                    (sort, reading)
                );
            }
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
    fn no_more_readers_are_open_at_once_than_the_pool_parks() {
        let scratch = Scratch::new("readers");
        let library =
            Library::open(&scratch.path.join("library.db")).expect("a catalog opens on disk");
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
}
