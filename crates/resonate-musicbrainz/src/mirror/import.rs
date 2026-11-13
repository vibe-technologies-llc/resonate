use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{self, BufRead, BufReader, Read},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
        mpsc,
    },
    thread,
};

use resonate_core::{gaming, writer::Held};
use rusqlite::{Connection, params};

use super::{
    AMENDED_FILE, BLOCKS_INDEXED, FORMAT, MIRROR_FILE, SCHEMA,
    copy::{Archive, TABLES, Table, fields, table_named, unescaped},
    documents::{Builders, CODES_JOINED_BY, Document, rows_of, words_of},
    looked_up::{LookedUp, store_read},
    tables::{Sequenced, TABLES_FILE, kept_beside_the_build},
};
use crate::{ArchiveOp, Entity, Error, PackOp, Result, Store, StoreOp, kept::now, wire::Writer};

pub const BLOCK_BYTES: usize = 64 * 1024;
pub const BLOCKS_PACKED_AT: i32 = 15;
pub const DICTIONARY_BYTES: usize = 112 * 1024;
pub const SAMPLED_BYTES: usize = 16 * 1024 * 1024;
const ROWS_BETWEEN_LOOKS: u64 = 65_536;
const READ_AT_ONCE: usize = 1024 * 1024;
const STAGING_FILE: &str = "musicbrainz.staging.sqlite";
const PLACED_FILE: &str = "musicbrainz.placed.sqlite";
const BUILDING_FILE: &str = "musicbrainz.sqlite.building";
const TABLES_BUILDING_FILE: &str = "musicbrainz.tables.sqlite.building";
const BUILDING_LOCK: &str = "musicbrainz.building";
const STAGING_CACHE_KIB: i64 = 1024 * 1024;
const REPLICATION_SEQUENCE: &str = "REPLICATION_SEQUENCE";
const SCHEMA_SEQUENCE: &str = "SCHEMA_SEQUENCE";
const LONGEST_SEQUENCE_FILE: u64 = 64;

#[derive(Clone, Debug)]
pub struct Archives {
    pub core: PathBuf,
    pub derived: PathBuf,
    pub dump: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Waiting,
    Downloading,
    Verifying,
    Importing,
    Indexing,
    Writing,
    Settling,
    Done,
}

impl Phase {
    const ALL: [Self; 8] = [
        Self::Waiting,
        Self::Downloading,
        Self::Verifying,
        Self::Importing,
        Self::Indexing,
        Self::Writing,
        Self::Settling,
        Self::Done,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Downloading => "downloading the dump",
            Self::Verifying => "checking the download",
            Self::Importing => "reading the dump",
            Self::Indexing => "indexing what was read",
            Self::Writing => "writing the compressed mirror",
            Self::Settling => "settling the index",
            Self::Done => "done",
        }
    }
}

#[derive(Default)]
pub struct Building {
    phase: AtomicU8,
    rows: AtomicU64,
    documents: AtomicU64,
    read: AtomicU64,
    total: AtomicU64,
    cancelled: AtomicBool,
}

impl Building {
    pub fn phase(&self) -> Phase {
        let at = usize::from(self.phase.load(Ordering::Relaxed));
        Phase::ALL.get(at).copied().unwrap_or(Phase::Waiting)
    }

    pub fn enter(&self, phase: Phase) {
        let at = Phase::ALL
            .iter()
            .position(|held| *held == phase)
            .unwrap_or(0);
        self.phase
            .store(u8::try_from(at).unwrap_or(0), Ordering::Relaxed);
        self.read.store(0, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
    }

    pub fn rows(&self) -> u64 {
        self.rows.load(Ordering::Relaxed)
    }

    pub fn documents(&self) -> u64 {
        self.documents.load(Ordering::Relaxed)
    }

    pub fn bytes(&self) -> (u64, u64) {
        (
            self.read.load(Ordering::Relaxed),
            self.total.load(Ordering::Relaxed),
        )
    }

    pub fn of(&self, total: u64) {
        self.total.store(total, Ordering::Relaxed);
    }

    pub fn reached(&self, read: u64) {
        self.read.store(read, Ordering::Relaxed);
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    pub(crate) fn looked_at(&self) -> Result<()> {
        gaming::wait(&|| self.is_cancelled());
        if self.is_cancelled() {
            return Err(Error::Cancelled);
        }
        Ok(())
    }
}

pub fn held_for_building(folder: &Path) -> Result<Held> {
    fs::create_dir_all(folder).map_err(Error::archive(folder, ArchiveOp::Stage))?;
    let lock = folder.join(BUILDING_LOCK);
    Held::taken_over(&lock).map_err(|source| {
        if source.kind() == io::ErrorKind::WouldBlock {
            Error::AlreadyBuilding
        } else {
            Error::archive(&lock, ArchiveOp::Stage)(source)
        }
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keeping {
    TheMirror,
    TheMirrorAndItsTables,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Built {
    pub mirror: PathBuf,
    pub tables: Option<PathBuf>,
}

pub fn built_from(
    archives: &Archives,
    folder: &Path,
    keeping: Keeping,
    building: &Building,
    _held: &Held,
) -> Result<Built> {
    let staging_path = folder.join(STAGING_FILE);
    let placed_path = folder.join(PLACED_FILE);
    let building_path = folder.join(BUILDING_FILE);
    let tables_path = folder.join(TABLES_BUILDING_FILE);
    for leftover in [&staging_path, &placed_path, &building_path, &tables_path] {
        removed(leftover)?;
    }

    let outcome = (|| {
        let staging = staged(&staging_path)?;
        building.enter(Phase::Importing);
        let sequenced = imported(&archives.core, Archive::Core, &staging, building)?;
        imported(&archives.derived, Archive::Derived, &staging, building)?;
        building.enter(Phase::Indexing);
        indexed(&staging, building)?;
        placed_with_their_recordings(&staging, building)?;
        building.enter(Phase::Writing);
        let looked_up = LookedUp::read(&staging, building)?;
        written(
            &staging,
            &looked_up,
            &building_path,
            &placed_path,
            &archives.dump,
            building,
        )?;
        drop(looked_up);
        if keeping == Keeping::TheMirrorAndItsTables {
            let Some(sequenced) = sequenced else {
                return Err(Error::Unsequenced);
            };
            building.enter(Phase::Settling);
            kept_beside_the_build(&staging, &archives.dump, sequenced)?;
            drop(staging);
            fs::rename(&staging_path, &tables_path)
                .map_err(Error::archive(&staging_path, ArchiveOp::Rename))?;
            return Ok(Some(tables_path.clone()));
        }
        Ok(None)
    })();

    removed(&staging_path)?;
    removed(&placed_path)?;
    match outcome {
        Ok(tables) => {
            building.enter(Phase::Done);
            Ok(Built {
                mirror: building_path,
                tables,
            })
        }
        Err(error) => {
            removed(&building_path)?;
            removed(&tables_path)?;
            Err(error)
        }
    }
}

pub fn settled(built: &Built, folder: &Path) -> Result<PathBuf> {
    let settled = folder.join(MIRROR_FILE);
    let tables = folder.join(TABLES_FILE);
    removed(&folder.join(AMENDED_FILE))?;
    match &built.tables {
        Some(kept) => {
            fs::rename(kept, &tables).map_err(Error::archive(kept, ArchiveOp::Rename))?;
        }
        None => removed(&tables)?,
    }
    fs::rename(&built.mirror, &settled)
        .map_err(Error::archive(&built.mirror, ArchiveOp::Rename))?;
    Ok(settled)
}

pub fn removed_beside_the_mirror(folder: &Path) -> Result<()> {
    for file in [AMENDED_FILE, TABLES_FILE] {
        removed(&folder.join(file))?;
    }
    Ok(())
}

pub(crate) fn removed(path: &Path) -> Result<()> {
    for suffix in ["", "-journal", "-wal", "-shm"] {
        let mut named = path.as_os_str().to_owned();
        named.push(suffix);
        match fs::remove_file(PathBuf::from(named)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(Error::archive(path, ArchiveOp::Remove)(error)),
        }
    }
    Ok(())
}

fn scratch(path: &Path, store: Store) -> Result<Connection> {
    let connection = Connection::open(path).map_err(Error::store(store, StoreOp::Open))?;
    connection
        .execute_batch(&format!(
            "PRAGMA journal_mode = OFF; PRAGMA synchronous = OFF; \
             PRAGMA locking_mode = EXCLUSIVE; PRAGMA cache_size = -{STAGING_CACHE_KIB}; \
             PRAGMA temp_store = FILE;"
        ))
        .map_err(Error::store(store, StoreOp::Configure))?;
    Ok(connection)
}

fn staged(path: &Path) -> Result<Connection> {
    let staging = scratch(path, Store::Staging)?;
    for table in TABLES {
        staging
            .execute_batch(&format!("CREATE TABLE {} ({});", table.name, table.laid))
            .map_err(Error::store(Store::Staging, StoreOp::LayOut))?;
    }
    Ok(staging)
}

struct Counted<R> {
    inner: R,
    read: u64,
    building: Arc<AtomicU64>,
}

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(into)?;
        self.read += read as u64;
        self.building.store(self.read, Ordering::Relaxed);
        Ok(read)
    }
}

fn sequence_in(entry: impl Read, path: &Path) -> Result<Option<u64>> {
    let mut text = String::new();
    entry
        .take(LONGEST_SEQUENCE_FILE)
        .read_to_string(&mut text)
        .map_err(Error::archive(path, ArchiveOp::Read))?;
    Ok(text.trim().parse().ok())
}

fn imported(
    path: &Path,
    from: Archive,
    staging: &Connection,
    building: &Building,
) -> Result<Option<Sequenced>> {
    let file = File::open(path).map_err(Error::archive(path, ArchiveOp::Open))?;
    let total = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    building.of(total);
    building.reached(0);
    let progress = Arc::new(AtomicU64::new(0));
    let counted = Counted {
        inner: BufReader::with_capacity(READ_AT_ONCE, file),
        read: 0,
        building: Arc::clone(&progress),
    };
    let decoded = bzip2::read::MultiBzDecoder::new(counted);
    let mut archive = tar::Archive::new(decoded);
    let entries = archive
        .entries()
        .map_err(Error::archive(path, ArchiveOp::Entry))?;
    let mut seen = BTreeSet::new();
    let mut replication = None;
    let mut schema = None;
    for entry in entries {
        let entry = entry.map_err(Error::archive(path, ArchiveOp::Entry))?;
        let name = entry
            .path()
            .map_err(Error::archive(path, ArchiveOp::Entry))?
            .to_string_lossy()
            .into_owned();
        if name == REPLICATION_SEQUENCE {
            replication = sequence_in(entry, path)?;
            continue;
        }
        if name == SCHEMA_SEQUENCE {
            schema = sequence_in(entry, path)?.and_then(|schema| u32::try_from(schema).ok());
            continue;
        }
        let Some(table) = table_named(&name).filter(|table| table.from == from) else {
            continue;
        };
        tracing::info!(table = table.name, "reading a table of the dump");
        loaded(
            table,
            BufReader::with_capacity(READ_AT_ONCE, entry),
            path,
            staging,
            building,
            &progress,
        )?;
        seen.insert(table.name);
    }
    for table in TABLES.iter().filter(|table| table.from == from) {
        if !seen.contains(table.name) {
            return Err(Error::MissingTable { table: table.name });
        }
    }
    Ok(replication
        .zip(schema)
        .map(|(replication, schema)| Sequenced {
            replication,
            schema,
        }))
}

fn loaded(
    table: &'static Table,
    mut rows: impl BufRead,
    path: &Path,
    staging: &Connection,
    building: &Building,
    progress: &AtomicU64,
) -> Result<()> {
    let wanted = table.columns.iter().max().map_or(0, |last| last + 1);
    let placeholders = vec!["?"; table.columns.len()].join(", ");
    staging
        .execute_batch("BEGIN")
        .map_err(Error::store(Store::Staging, StoreOp::Write))?;
    {
        let mut insert = staging
            .prepare(&format!(
                "INSERT INTO {} VALUES ({placeholders})",
                table.name
            ))
            .map_err(Error::store(Store::Staging, StoreOp::Write))?;
        let mut line = Vec::new();
        let mut values: Vec<Option<String>> = Vec::with_capacity(table.columns.len());
        let mut counted = 0_u64;
        loop {
            line.clear();
            let read = rows
                .read_until(b'\n', &mut line)
                .map_err(Error::archive(path, ArchiveOp::Read))?;
            if read == 0 {
                break;
            }
            if line.last() != Some(&b'\n') {
                return Err(Error::CutRow { table: table.name });
            }
            line.pop();
            let mut split = Vec::with_capacity(wanted);
            fields(&line, &mut split);
            if split.len() < wanted {
                return Err(Error::Columns {
                    table: table.name,
                    found: split.len(),
                    wanted,
                });
            }
            values.clear();
            values.extend(table.columns.iter().map(|column| unescaped(split[*column])));
            insert
                .execute(rusqlite::params_from_iter(values.iter()))
                .map_err(Error::store(Store::Staging, StoreOp::Write))?;
            counted += 1;
            if counted.is_multiple_of(ROWS_BETWEEN_LOOKS) {
                building
                    .rows
                    .fetch_add(ROWS_BETWEEN_LOOKS, Ordering::Relaxed);
                building.reached(progress.load(Ordering::Relaxed));
                building.looked_at()?;
            }
        }
        building
            .rows
            .fetch_add(counted % ROWS_BETWEEN_LOOKS, Ordering::Relaxed);
    }
    staging
        .execute_batch("COMMIT")
        .map_err(Error::store(Store::Staging, StoreOp::Write))?;
    Ok(())
}

fn indexed(staging: &Connection, building: &Building) -> Result<()> {
    for table in TABLES {
        for (at, columns) in table.indexed.iter().enumerate() {
            building.looked_at()?;
            staging
                .execute_batch(&format!(
                    "CREATE INDEX {name}_{at} ON {name}({columns});",
                    name = table.name
                ))
                .map_err(Error::store(Store::Staging, StoreOp::LayOut))?;
        }
    }
    staging
        .execute_batch("ANALYZE;")
        .map_err(Error::store(Store::Staging, StoreOp::LayOut))
}

const TRACKS_PLACED: &str = "
CREATE TABLE placed_track (
    medium INTEGER, position INTEGER, number TEXT, name TEXT, artist_credit INTEGER, gid TEXT,
    length INTEGER, recording INTEGER, recording_gid TEXT, recording_length INTEGER, isrcs TEXT
);
";

const TRACKS_SORTED: &str = "
CREATE TABLE placed_tracks (
    medium INTEGER NOT NULL, position INTEGER, number TEXT, name TEXT, artist_credit INTEGER,
    gid TEXT NOT NULL, length INTEGER, recording INTEGER, recording_gid TEXT,
    recording_length INTEGER, isrcs TEXT,
    PRIMARY KEY (medium, position, gid)
) WITHOUT ROWID;
INSERT OR IGNORE INTO placed_tracks
    SELECT medium, position, number, name, artist_credit, gid, length, recording, recording_gid,
           recording_length, isrcs
    FROM placed_track WHERE medium IS NOT NULL AND gid IS NOT NULL
    ORDER BY medium, position, gid;
DROP TABLE placed_track;
";

type TrackRow = (
    i64,
    Option<i64>,
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<i64>,
);

fn placed_with_their_recordings(staging: &Connection, building: &Building) -> Result<()> {
    let failed = store_read(Store::Staging);
    let write = || Error::store(Store::Staging, StoreOp::Write);
    staging.execute_batch(TRACKS_PLACED).map_err(write())?;
    staging.execute_batch("BEGIN").map_err(write())?;
    {
        let mut insert = staging
            .prepare(
                "INSERT INTO placed_track VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )
            .map_err(write())?;
        let mut tracks_of = staging
            .prepare(
                "SELECT recording, medium, position, number, name, artist_credit, gid, length \
                 FROM track ORDER BY recording",
            )
            .map_err(&failed)?;
        let mut recordings_of = staging
            .prepare("SELECT id, gid, length FROM recording ORDER BY id")
            .map_err(&failed)?;
        let mut codes_of = staging
            .prepare("SELECT recording, isrc FROM isrc ORDER BY recording, isrc")
            .map_err(&failed)?;
        let mut tracks = tracks_of.query([]).map_err(&failed)?;
        let mut recordings = recordings_of.query([]).map_err(&failed)?;
        let mut codes = codes_of.query([]).map_err(&failed)?;
        let mut recording: Option<(i64, Option<String>, Option<i64>)> = None;
        let mut code: Option<(i64, Option<String>)> = None;
        let mut held_codes: (i64, String) = (i64::MIN, String::new());
        let mut counted = 0_u64;
        while let Some(row) = tracks.next().map_err(&failed)? {
            let track: TrackRow = (
                row.get(0).map_err(&failed)?,
                row.get(1).map_err(&failed)?,
                row.get(2).map_err(&failed)?,
                row.get(3).map_err(&failed)?,
                row.get(4).map_err(&failed)?,
                row.get(5).map_err(&failed)?,
                row.get(6).map_err(&failed)?,
                row.get(7).map_err(&failed)?,
            );
            let wanted = track.0;
            while recording.as_ref().is_none_or(|(id, ..)| *id < wanted) {
                match recordings.next().map_err(&failed)? {
                    Some(row) => {
                        recording = Some((
                            row.get(0).map_err(&failed)?,
                            row.get(1).map_err(&failed)?,
                            row.get(2).map_err(&failed)?,
                        ));
                    }
                    None => {
                        recording = Some((i64::MAX, None, None));
                    }
                }
            }
            if held_codes.0 != wanted {
                held_codes = (wanted, String::new());
                loop {
                    if code.is_none() {
                        code = match codes.next().map_err(&failed)? {
                            Some(row) => {
                                Some((row.get(0).map_err(&failed)?, row.get(1).map_err(&failed)?))
                            }
                            None => Some((i64::MAX, None)),
                        };
                    }
                    let Some((at, isrc)) = &code else {
                        break;
                    };
                    if *at > wanted {
                        break;
                    }
                    if *at == wanted
                        && let Some(isrc) = isrc
                    {
                        if !held_codes.1.is_empty() {
                            held_codes.1.push_str(CODES_JOINED_BY);
                        }
                        held_codes.1.push_str(isrc);
                    }
                    code = None;
                }
            }
            let (gid, length) = match &recording {
                Some((id, gid, length)) if *id == wanted => (gid.clone(), *length),
                _ => (None, None),
            };
            insert
                .execute(params![
                    track.1,
                    track.2,
                    track.3,
                    track.4,
                    track.5,
                    track.6,
                    track.7,
                    wanted,
                    gid,
                    length,
                    held_codes.1,
                ])
                .map_err(write())?;
            counted += 1;
            if counted.is_multiple_of(ROWS_BETWEEN_LOOKS) {
                building
                    .rows
                    .fetch_add(ROWS_BETWEEN_LOOKS, Ordering::Relaxed);
                building.looked_at()?;
            }
        }
    }
    staging.execute_batch("COMMIT").map_err(write())?;
    building.looked_at()?;
    staging.execute_batch(TRACKS_SORTED).map_err(write())
}

struct Packed {
    kind: Entity,
    first: i64,
    unpacked: usize,
    packed: Vec<u8>,
}

struct Job {
    kind: Entity,
    first: i64,
    bytes: Vec<u8>,
    dictionary: Option<Arc<Vec<u8>>>,
}

struct Packer<'a> {
    kind: Entity,
    ordinal: i64,
    first: i64,
    block: Writer,
    block_len: usize,
    samples: Option<Vec<Vec<u8>>>,
    sampled: usize,
    waiting: Vec<(i64, Vec<u8>)>,
    dictionary: Option<Arc<Vec<u8>>>,
    jobs: &'a mpsc::SyncSender<Job>,
    mirror: &'a Connection,
}

impl<'a> Packer<'a> {
    fn new(kind: Entity, jobs: &'a mpsc::SyncSender<Job>, mirror: &'a Connection) -> Self {
        Self {
            kind,
            ordinal: 0,
            first: 1,
            block: Writer::new(),
            block_len: 0,
            samples: Some(Vec::new()),
            sampled: 0,
            waiting: Vec::new(),
            dictionary: None,
            jobs,
            mirror,
        }
    }

    fn add(&mut self, document: Vec<u8>) -> Result<i64> {
        self.ordinal += 1;
        if self.block_len == 0 {
            self.first = self.ordinal;
        }
        let mut entry = Writer::new();
        entry.signed(self.ordinal);
        entry.bytes(&document);
        let entry = entry.finish();
        self.block_len += entry.len();
        self.block.raw(&entry);
        if let Some(samples) = &mut self.samples {
            self.sampled += document.len();
            samples.push(document);
        }
        if self.block_len >= BLOCK_BYTES {
            self.cut()?;
        }
        if self.sampled >= SAMPLED_BYTES && self.samples.is_some() {
            self.trained()?;
        }
        Ok(self.ordinal)
    }

    fn cut(&mut self) -> Result<()> {
        let block = std::mem::take(&mut self.block).finish();
        self.block_len = 0;
        if self.samples.is_some() {
            self.waiting.push((self.first, block));
            return Ok(());
        }
        self.sent(self.first, block)
    }

    fn sent(&self, first: i64, bytes: Vec<u8>) -> Result<()> {
        self.jobs
            .send(Job {
                kind: self.kind,
                first,
                bytes,
                dictionary: self.dictionary.clone(),
            })
            .map_err(|_| Error::Pack {
                op: PackOp::Pack,
                source: io::Error::from(io::ErrorKind::BrokenPipe),
            })
    }

    fn trained(&mut self) -> Result<()> {
        let samples = self.samples.take().unwrap_or_default();
        self.dictionary = match zstd::dict::from_samples(&samples, DICTIONARY_BYTES) {
            Ok(dictionary) => {
                self.mirror
                    .execute(
                        "INSERT INTO dictionaries (kind, bytes) VALUES (?1, ?2)",
                        params![self.kind.code(), dictionary],
                    )
                    .map_err(Error::store(Store::Mirror, StoreOp::Write))?;
                Some(Arc::new(dictionary))
            }
            Err(error) => {
                tracing::debug!(%error, kind = ?self.kind, "no dictionary was trained; blocks go without one");
                None
            }
        };
        for (first, block) in std::mem::take(&mut self.waiting) {
            self.sent(first, block)?;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<()> {
        if self.block_len > 0 {
            self.cut()?;
        }
        if self.samples.is_some() {
            self.trained()?;
        }
        Ok(())
    }
}

fn packed_by_a_worker(job: &Job) -> io::Result<Vec<u8>> {
    match &job.dictionary {
        Some(dictionary) => {
            let mut compressor =
                zstd::bulk::Compressor::with_dictionary(BLOCKS_PACKED_AT, dictionary)?;
            compressor.compress(&job.bytes)
        }
        None => zstd::bulk::compress(&job.bytes, BLOCKS_PACKED_AT),
    }
}

struct Writing<'a> {
    staging: &'a Connection,
    looked_up: &'a LookedUp<'a>,
    mirror: &'a Connection,
    placed: &'a Connection,
    building: &'a Building,
}

fn written(
    staging: &Connection,
    looked_up: &LookedUp<'_>,
    building_path: &Path,
    placed_path: &Path,
    dump: &str,
    building: &Building,
) -> Result<()> {
    let mirror = scratch(building_path, Store::Mirror)?;
    mirror
        .execute_batch(SCHEMA)
        .map_err(Error::store(Store::Mirror, StoreOp::LayOut))?;
    let placed = scratch(placed_path, Store::Staging)?;
    placed
        .execute_batch(
            "CREATE TABLE placed (mbid BLOB NOT NULL, kind INTEGER NOT NULL, ordinal INTEGER NOT NULL);
             CREATE TABLE placed_isrcs (isrc TEXT NOT NULL, ordinal INTEGER NOT NULL);
             CREATE TABLE placed_barcodes (barcode TEXT NOT NULL, ordinal INTEGER NOT NULL);",
        )
        .map_err(Error::store(Store::Staging, StoreOp::LayOut))?;
    for connection in [&mirror, &placed] {
        connection
            .execute_batch("BEGIN")
            .map_err(Error::store(Store::Mirror, StoreOp::Write))?;
    }

    let workers = thread::available_parallelism()
        .map_or(1, |count| count.get().saturating_sub(1))
        .max(1);
    let (jobs, taken) = mpsc::sync_channel::<Job>(workers * 2);
    let (done, packed) = mpsc::channel::<io::Result<Packed>>();
    let taken = Arc::new(parking_lot::Mutex::new(taken));
    let outcome = thread::scope(|scope| {
        for _ in 0..workers {
            let taken = Arc::clone(&taken);
            let done = done.clone();
            scope.spawn(move || {
                loop {
                    let job = taken.lock().recv();
                    let Ok(job) = job else {
                        return;
                    };
                    let packed = packed_by_a_worker(&job).map(|packed| Packed {
                        kind: job.kind,
                        first: job.first,
                        unpacked: job.bytes.len(),
                        packed,
                    });
                    if done.send(packed).is_err() {
                        return;
                    }
                }
            });
        }
        drop(done);
        let writing = Writing {
            staging,
            looked_up,
            mirror: &mirror,
            placed: &placed,
            building,
        };
        let wrote = (|| {
            let mut builders = Builders::prepared(staging)?;
            for kind in Entity::ALL {
                let mut packer = Packer::new(kind, &jobs, &mirror);
                writing.every(&mut builders, &mut packer, &packed)?;
                packer.finish()?;
            }
            Ok::<_, Error>(())
        })();
        drop(jobs);
        let mut kept = wrote;
        for block in packed.iter() {
            if kept.is_ok() {
                kept = block_kept(&mirror, block);
            }
        }
        kept
    });
    outcome?;

    building.enter(Phase::Settling);
    placed
        .execute_batch("COMMIT")
        .map_err(Error::store(Store::Staging, StoreOp::Write))?;
    drop(placed);
    let placed_text = placed_path.to_string_lossy().into_owned();
    mirror
        .execute("ATTACH DATABASE ?1 AS placed", [placed_text])
        .map_err(Error::store(Store::Mirror, StoreOp::Write))?;
    mirror
        .execute_batch(
            "INSERT OR IGNORE INTO entities SELECT mbid, kind, ordinal FROM placed.placed ORDER BY mbid;
             INSERT OR IGNORE INTO isrcs SELECT isrc, ordinal FROM placed.placed_isrcs ORDER BY isrc, ordinal;
             INSERT OR IGNORE INTO barcodes SELECT barcode, ordinal FROM placed.placed_barcodes ORDER BY barcode, ordinal;",
        )
        .map_err(Error::store(Store::Mirror, StoreOp::Write))?;
    mirror
        .execute_batch(BLOCKS_INDEXED)
        .map_err(Error::store(Store::Mirror, StoreOp::LayOut))?;
    for table in [
        "recording_words",
        "release_words",
        "group_words",
        "artist_words",
    ] {
        building.looked_at()?;
        mirror
            .execute_batch(&format!(
                "INSERT INTO {table}({table}) VALUES ('optimize');"
            ))
            .map_err(Error::store(Store::Mirror, StoreOp::Write))?;
    }
    mirror
        .execute(
            "INSERT INTO made (format, dump, built) VALUES (?1, ?2, ?3)",
            params![FORMAT, dump, i64::try_from(now()).unwrap_or(0)],
        )
        .map_err(Error::store(Store::Mirror, StoreOp::Write))?;
    mirror
        .execute_batch("COMMIT")
        .map_err(Error::store(Store::Mirror, StoreOp::Write))?;
    mirror
        .execute_batch("DETACH DATABASE placed; PRAGMA journal_mode = DELETE;")
        .map_err(Error::store(Store::Mirror, StoreOp::Write))?;
    Ok(())
}

fn block_kept(mirror: &Connection, block: io::Result<Packed>) -> Result<()> {
    let block = block.map_err(|source| Error::Pack {
        op: PackOp::Pack,
        source,
    })?;
    mirror
        .prepare_cached(
            "INSERT INTO blocks (kind, first, unpacked, packed) VALUES (?1, ?2, ?3, ?4)",
        )
        .and_then(|mut statement| {
            statement.execute(params![
                block.kind.code(),
                block.first,
                i64::try_from(block.unpacked).unwrap_or(i64::MAX),
                block.packed
            ])
        })
        .map_err(Error::store(Store::Mirror, StoreOp::Write))?;
    Ok(())
}

impl Writing<'_> {
    fn drained(&self, packed: &mpsc::Receiver<io::Result<Packed>>) -> Result<()> {
        while let Ok(block) = packed.try_recv() {
            block_kept(self.mirror, block)?;
        }
        Ok(())
    }

    fn documented(
        &self,
        packer: &mut Packer<'_>,
        packed: &mpsc::Receiver<io::Result<Packed>>,
        built: Document,
    ) -> Result<()> {
        let kind = packer.kind;
        let ordinal = packer.add(built.bytes)?;
        let write = || Error::store(Store::Staging, StoreOp::Write);
        self.placed
            .prepare_cached("INSERT INTO placed (mbid, kind, ordinal) VALUES (?1, ?2, ?3)")
            .and_then(|mut statement| {
                statement.execute(params![built.gid.to_vec(), kind.code(), ordinal])
            })
            .map_err(write())?;
        self.mirror
            .prepare_cached(&format!(
                "INSERT INTO {} (rowid, words) VALUES (?1, ?2)",
                words_of(kind)
            ))
            .and_then(|mut statement| statement.execute(params![ordinal, built.words]))
            .map_err(Error::store(Store::Mirror, StoreOp::Write))?;
        let coded = match kind {
            Entity::Release => {
                Some("INSERT INTO placed_barcodes (barcode, ordinal) VALUES (?1, ?2)")
            }
            Entity::Recording => Some("INSERT INTO placed_isrcs (isrc, ordinal) VALUES (?1, ?2)"),
            Entity::Group | Entity::Artist => None,
        };
        if let Some(coded) = coded {
            for code in &built.codes {
                self.placed
                    .prepare_cached(coded)
                    .and_then(|mut statement| statement.execute(params![code, ordinal]))
                    .map_err(write())?;
            }
        }
        let documents = self.building.documents.fetch_add(1, Ordering::Relaxed) + 1;
        if documents.is_multiple_of(ROWS_BETWEEN_LOOKS) {
            self.drained(packed)?;
            self.building.looked_at()?;
        }
        Ok(())
    }

    fn every(
        &self,
        builders: &mut Builders<'_>,
        packer: &mut Packer<'_>,
        packed: &mpsc::Receiver<io::Result<Packed>>,
    ) -> Result<()> {
        let failed = store_read(Store::Staging);
        let kind = packer.kind;
        let mut rows_of = self
            .staging
            .prepare(&format!("{} ORDER BY id", rows_of(kind)))
            .map_err(&failed)?;
        let mut rows = rows_of.query([]).map_err(&failed)?;
        while let Some(row) = rows.next().map_err(&failed)? {
            if let Some(built) = builders.built(kind, self.looked_up, row)? {
                self.documented(packer, packed, built)?;
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn area() -> &'static Table {
        table_named("mbdump/area").expect("the area table")
    }

    fn laid_out() -> Connection {
        let staging = Connection::open_in_memory().expect("a staging store");
        staging
            .execute_batch(&format!("CREATE TABLE area ({});", area().laid))
            .expect("the area table laid out");
        staging
    }

    fn read(rows: &[u8], staging: &Connection) -> Result<()> {
        loaded(
            area(),
            Cursor::new(rows.to_vec()),
            Path::new("mbdump.tar.bz2"),
            staging,
            &Building::default(),
            &AtomicU64::new(0),
        )
    }

    fn areas(staging: &Connection) -> Vec<(i64, Option<String>)> {
        staging
            .prepare("SELECT id, name FROM area ORDER BY id")
            .expect("a statement")
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("the rows")
            .collect::<std::result::Result<_, _>>()
            .expect("every row")
    }

    #[test]
    fn a_row_is_read_with_its_escapes_its_nulls_and_the_columns_it_does_not_keep_left_out() {
        let staging = laid_out();
        let rows = b"1\tgid\tCaf\\303\\251\\ttab\\nline\tx\n2\tgid\t\\N\tx\n3\tgid\t\tx\n";

        read(rows, &staging).expect("whole rows");

        assert_eq!(
            areas(&staging),
            [
                (1, Some("Caf\u{e9}\ttab\nline".to_owned())),
                (2, None),
                (3, Some(String::new())),
            ]
        );
    }

    #[test]
    fn a_row_with_fewer_columns_than_the_table_keeps_is_refused_with_what_it_had() {
        let staging = laid_out();

        let refused = read(b"1\tgid\n", &staging);

        assert!(matches!(
            refused,
            Err(Error::Columns {
                table: "area",
                found: 2,
                wanted: 3
            })
        ));
    }

    #[test]
    fn a_final_row_with_no_line_end_is_a_cut_row_and_is_not_kept() {
        let staging = laid_out();

        let refused = read(b"1\tgid\tOne\tx\n2\tgid\tTw", &staging);

        assert!(matches!(refused, Err(Error::CutRow { table: "area" })));
    }

    #[test]
    fn a_table_with_no_rows_loads_nothing() {
        let staging = laid_out();

        read(b"", &staging).expect("an empty table");

        assert!(areas(&staging).is_empty());
    }
}
