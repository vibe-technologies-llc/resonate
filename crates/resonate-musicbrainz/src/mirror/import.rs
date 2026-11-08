use std::{
    collections::{BTreeSet, HashMap},
    fs::{self, File},
    io::{self, BufRead, BufReader, Read},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use resonate_core::{Isrc, Link, Mbid, folded_letters, gaming, writer::Held};
use resonate_library::{
    ArtistProfile, ArtistRelease, Credit, Genre, GroupRelease, Issued, LifeSpan, Medium, Recording,
    RecordingRelease, Release, ReleaseGroup, ReleaseTrack,
};
use rusqlite::{Connection, OptionalExtension, Statement, params};

use super::{
    ArtistDoc, BLOCKS_INDEXED, DISCOGRAPHY_KINDS, FORMAT, GroupDoc, MIRROR_FILE, ReleaseDoc,
    SCHEMA,
    copy::{Archive, TABLES, Table, fields, table_named, unescaped},
    search::words_of_a_credited,
};
use crate::{
    ArchiveOp, Entity, Error, PackOp, Result, Store, StoreOp,
    kept::now,
    wire::{MBID_BYTES, Writer, mbid_of},
};

pub const BLOCK_BYTES: usize = 64 * 1024;
pub const BLOCKS_PACKED_AT: i32 = 15;
pub const DICTIONARY_BYTES: usize = 112 * 1024;
pub const SAMPLED_BYTES: usize = 16 * 1024 * 1024;
pub const RELEASES_OF_A_RECORDING_KEPT: usize = 64;
pub const GROUPS_OF_AN_ARTIST_KEPT: usize = 10_000;
pub const GENRES_KEPT: usize = 32;
const ROWS_BETWEEN_LOOKS: u64 = 65_536;
const READ_AT_ONCE: usize = 1024 * 1024;
const STAGING_FILE: &str = "musicbrainz.staging.sqlite";
const PLACED_FILE: &str = "musicbrainz.placed.sqlite";
const BUILDING_FILE: &str = "musicbrainz.sqlite.building";
const BUILDING_LOCK: &str = "musicbrainz.building";
const STAGING_CACHE_KIB: i64 = 1024 * 1024;
const STATED_AS_NOTHING: [&str; 2] = ["[none]", "[no label]"];
const COVER_PRESENT: &str = "present";
const ENDED: &str = "t";

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

pub fn built_from(
    archives: &Archives,
    folder: &Path,
    building: &Building,
    _held: &Held,
) -> Result<PathBuf> {
    let staging_path = folder.join(STAGING_FILE);
    let placed_path = folder.join(PLACED_FILE);
    let building_path = folder.join(BUILDING_FILE);
    for leftover in [&staging_path, &placed_path, &building_path] {
        removed(leftover)?;
    }

    let outcome = (|| {
        let staging = staged(&staging_path)?;
        building.enter(Phase::Importing);
        imported(&archives.core, Archive::Core, &staging, building)?;
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
        )
    })();

    removed(&staging_path)?;
    removed(&placed_path)?;
    match outcome {
        Ok(()) => {
            building.enter(Phase::Done);
            Ok(building_path)
        }
        Err(error) => {
            removed(&building_path)?;
            Err(error)
        }
    }
}

pub fn settled(built: &Path, folder: &Path) -> Result<PathBuf> {
    let settled = folder.join(MIRROR_FILE);
    fs::rename(built, &settled).map_err(Error::archive(built, ArchiveOp::Rename))?;
    Ok(settled)
}

fn removed(path: &Path) -> Result<()> {
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

fn imported(path: &Path, from: Archive, staging: &Connection, building: &Building) -> Result<()> {
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
    for entry in entries {
        let entry = entry.map_err(Error::archive(path, ArchiveOp::Entry))?;
        let name = entry
            .path()
            .map_err(Error::archive(path, ArchiveOp::Entry))?
            .to_string_lossy()
            .into_owned();
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
    Ok(())
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
            if line.last() == Some(&b'\n') {
                line.pop();
            }
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

type EventOrder = (i64, i64, i64);

type Event = (EventOrder, Option<String>, Option<u32>);

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

const CODES_JOINED_BY: &str = " ";

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

struct CreditRow {
    artist: u32,
    name: Box<str>,
    joined_by: Box<str>,
}

struct GroupInfo {
    gid: [u8; MBID_BYTES],
    title: Box<str>,
    kind: Option<u32>,
    secondary: Box<[u32]>,
    first_released: Option<Box<str>>,
}

struct ReleaseInfo {
    gid: [u8; MBID_BYTES],
    title: Box<str>,
    group: u32,
    status: Option<u32>,
    date: Option<Box<str>>,
    country: Option<Box<str>>,
    discs: Box<[u32]>,
}

struct LookedUp {
    named: HashMap<(&'static str, u32), Box<str>>,
    artists: Vec<Option<[u8; MBID_BYTES]>>,
    credits: Vec<Option<Box<[CreditRow]>>>,
    groups: Vec<Option<GroupInfo>>,
    releases: Vec<Option<ReleaseInfo>>,
}

fn at(id: i64) -> usize {
    usize::try_from(id).unwrap_or(0)
}

fn small(id: i64) -> u32 {
    u32::try_from(id).unwrap_or(0)
}

fn placed<T>(slots: &mut Vec<Option<T>>, id: i64, value: T) {
    let at = at(id);
    if slots.len() <= at {
        slots.resize_with(at + 1, || None);
    }
    slots[at] = Some(value);
}

pub fn gid_bytes(gid: &str) -> Option<[u8; MBID_BYTES]> {
    let mbid = Mbid::new(gid).ok()?;
    Some(crate::wire::mbid_bytes(&mbid))
}

pub fn dated(year: Option<i64>, month: Option<i64>, day: Option<i64>) -> Option<String> {
    let year = year?;
    Some(match (month, day) {
        (Some(month), Some(day)) => format!("{year:04}-{month:02}-{day:02}"),
        (Some(month), None) => format!("{year:04}-{month:02}"),
        _ => format!("{year:04}"),
    })
}

fn stated(text: Option<String>) -> Option<String> {
    text.map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty() && !STATED_AS_NOTHING.contains(&text.as_str()))
}

fn store_read(store: Store) -> impl Fn(rusqlite::Error) -> Error {
    move |source| Error::Store {
        store,
        op: StoreOp::Read,
        source: Box::new(source),
    }
}

impl LookedUp {
    fn read(staging: &Connection, building: &Building) -> Result<Self> {
        let failed = store_read(Store::Staging);
        let mut named = HashMap::new();
        for table in [
            "artist_type",
            "gender",
            "area",
            "release_status",
            "release_group_primary_type",
            "release_group_secondary_type",
            "medium_format",
            "link_type",
            "label",
            "tag",
        ] {
            let mut statement = staging
                .prepare(&format!("SELECT id, name FROM {table}"))
                .map_err(&failed)?;
            let mut rows = statement.query([]).map_err(&failed)?;
            while let Some(row) = rows.next().map_err(&failed)? {
                let id: i64 = row.get(0).map_err(&failed)?;
                let name: Option<String> = row.get(1).map_err(&failed)?;
                if let Some(name) = name {
                    named.insert((table, small(id)), name.into_boxed_str());
                }
            }
        }
        {
            let mut statement = staging
                .prepare("SELECT area, code FROM iso_3166_1")
                .map_err(&failed)?;
            let mut rows = statement.query([]).map_err(&failed)?;
            while let Some(row) = rows.next().map_err(&failed)? {
                let area: i64 = row.get(0).map_err(&failed)?;
                let code: Option<String> = row.get(1).map_err(&failed)?;
                if let Some(code) = code {
                    named.insert(("iso_3166_1", small(area)), code.into_boxed_str());
                }
            }
        }
        building.looked_at()?;

        let mut artists = Vec::new();
        {
            let mut statement = staging
                .prepare("SELECT id, gid FROM artist")
                .map_err(&failed)?;
            let mut rows = statement.query([]).map_err(&failed)?;
            while let Some(row) = rows.next().map_err(&failed)? {
                let id: i64 = row.get(0).map_err(&failed)?;
                let gid: String = row.get(1).map_err(&failed)?;
                if let Some(gid) = gid_bytes(&gid) {
                    placed(&mut artists, id, gid);
                }
            }
        }
        building.looked_at()?;

        let mut credits: Vec<Option<Box<[CreditRow]>>> = Vec::new();
        {
            let mut statement = staging
                .prepare(
                    "SELECT artist_credit, artist, name, join_phrase FROM artist_credit_name \
                     ORDER BY artist_credit, position",
                )
                .map_err(&failed)?;
            let mut rows = statement.query([]).map_err(&failed)?;
            let mut current: Option<(i64, Vec<CreditRow>)> = None;
            while let Some(row) = rows.next().map_err(&failed)? {
                let credit: i64 = row.get(0).map_err(&failed)?;
                let named = CreditRow {
                    artist: small(row.get(1).map_err(&failed)?),
                    name: row
                        .get::<_, Option<String>>(2)
                        .map_err(&failed)?
                        .unwrap_or_default()
                        .into_boxed_str(),
                    joined_by: row
                        .get::<_, Option<String>>(3)
                        .map_err(&failed)?
                        .unwrap_or_default()
                        .into_boxed_str(),
                };
                match &mut current {
                    Some((held, rows)) if *held == credit => rows.push(named),
                    _ => {
                        if let Some((held, rows)) = current.take() {
                            placed(&mut credits, held, rows.into_boxed_slice());
                        }
                        current = Some((credit, vec![named]));
                    }
                }
            }
            if let Some((held, rows)) = current {
                placed(&mut credits, held, rows.into_boxed_slice());
            }
        }
        building.looked_at()?;

        let mut secondary: HashMap<u32, Vec<u32>> = HashMap::new();
        {
            let mut statement = staging
                .prepare(
                    "SELECT release_group, secondary_type FROM release_group_secondary_type_join",
                )
                .map_err(&failed)?;
            let mut rows = statement.query([]).map_err(&failed)?;
            while let Some(row) = rows.next().map_err(&failed)? {
                secondary
                    .entry(small(row.get(0).map_err(&failed)?))
                    .or_default()
                    .push(small(row.get(1).map_err(&failed)?));
            }
        }
        let mut first_released: HashMap<u32, String> = HashMap::new();
        {
            let mut statement = staging
                .prepare("SELECT id, y, m, d FROM release_group_meta")
                .map_err(&failed)?;
            let mut rows = statement.query([]).map_err(&failed)?;
            while let Some(row) = rows.next().map_err(&failed)? {
                if let Some(date) = dated(
                    row.get(1).map_err(&failed)?,
                    row.get(2).map_err(&failed)?,
                    row.get(3).map_err(&failed)?,
                ) {
                    first_released.insert(small(row.get(0).map_err(&failed)?), date);
                }
            }
        }
        let mut groups = Vec::new();
        {
            let mut statement = staging
                .prepare("SELECT id, gid, name, type FROM release_group")
                .map_err(&failed)?;
            let mut rows = statement.query([]).map_err(&failed)?;
            while let Some(row) = rows.next().map_err(&failed)? {
                let id: i64 = row.get(0).map_err(&failed)?;
                let gid: String = row.get(1).map_err(&failed)?;
                let Some(gid) = gid_bytes(&gid) else {
                    continue;
                };
                let kind: Option<i64> = row.get(3).map_err(&failed)?;
                placed(
                    &mut groups,
                    id,
                    GroupInfo {
                        gid,
                        title: row
                            .get::<_, Option<String>>(2)
                            .map_err(&failed)?
                            .unwrap_or_default()
                            .into_boxed_str(),
                        kind: kind.map(small),
                        secondary: secondary
                            .remove(&small(id))
                            .unwrap_or_default()
                            .into_boxed_slice(),
                        first_released: first_released
                            .remove(&small(id))
                            .map(String::into_boxed_str),
                    },
                );
            }
        }
        building.looked_at()?;

        let mut events: HashMap<u32, Event> = HashMap::new();
        for query in [
            "SELECT release, y, m, d, country FROM release_country",
            "SELECT release, y, m, d, NULL FROM release_unknown_country",
        ] {
            let mut statement = staging.prepare(query).map_err(&failed)?;
            let mut rows = statement.query([]).map_err(&failed)?;
            while let Some(row) = rows.next().map_err(&failed)? {
                let release = small(row.get(0).map_err(&failed)?);
                let year: Option<i64> = row.get(1).map_err(&failed)?;
                let month: Option<i64> = row.get(2).map_err(&failed)?;
                let day: Option<i64> = row.get(3).map_err(&failed)?;
                let country: Option<i64> = row.get(4).map_err(&failed)?;
                let order = (
                    year.unwrap_or(i64::MAX),
                    month.unwrap_or(i64::MAX),
                    day.unwrap_or(i64::MAX),
                );
                let earlier = events
                    .get(&release)
                    .is_none_or(|(held, _, _)| order < *held);
                if earlier {
                    events.insert(
                        release,
                        (order, dated(year, month, day), country.map(small)),
                    );
                }
            }
        }
        let mut discs: HashMap<u32, Vec<u32>> = HashMap::new();
        {
            let mut statement = staging
                .prepare("SELECT release, track_count FROM medium ORDER BY release, position")
                .map_err(&failed)?;
            let mut rows = statement.query([]).map_err(&failed)?;
            while let Some(row) = rows.next().map_err(&failed)? {
                let count: Option<i64> = row.get(1).map_err(&failed)?;
                discs
                    .entry(small(row.get(0).map_err(&failed)?))
                    .or_default()
                    .push(small(count.unwrap_or(0)));
            }
        }
        building.looked_at()?;
        let mut releases = Vec::new();
        {
            let mut statement = staging
                .prepare("SELECT id, gid, name, release_group, status FROM release")
                .map_err(&failed)?;
            let mut rows = statement.query([]).map_err(&failed)?;
            while let Some(row) = rows.next().map_err(&failed)? {
                let id: i64 = row.get(0).map_err(&failed)?;
                let gid: String = row.get(1).map_err(&failed)?;
                let Some(gid) = gid_bytes(&gid) else {
                    continue;
                };
                let status: Option<i64> = row.get(4).map_err(&failed)?;
                let (date, country) = events
                    .remove(&small(id))
                    .map(|(_, date, country)| (date, country))
                    .unwrap_or_default();
                let country = country
                    .and_then(|area| named.get(&("iso_3166_1", area)))
                    .cloned();
                placed(
                    &mut releases,
                    id,
                    ReleaseInfo {
                        gid,
                        title: row
                            .get::<_, Option<String>>(2)
                            .map_err(&failed)?
                            .unwrap_or_default()
                            .into_boxed_str(),
                        group: small(row.get(3).map_err(&failed)?),
                        status: status.map(small),
                        date: date.map(String::into_boxed_str),
                        country,
                        discs: discs
                            .remove(&small(id))
                            .unwrap_or_default()
                            .into_boxed_slice(),
                    },
                );
            }
        }
        building.looked_at()?;

        Ok(Self {
            named,
            artists,
            credits,
            groups,
            releases,
        })
    }

    fn name(&self, table: &'static str, id: Option<i64>) -> Option<String> {
        id.and_then(|id| self.named.get(&(table, small(id))))
            .map(|name| name.to_string())
    }

    fn credit(&self, id: i64) -> Vec<Credit> {
        let Some(Some(rows)) = self.credits.get(at(id)) else {
            return Vec::new();
        };
        rows.iter()
            .map(|row| Credit {
                name: row.name.to_string(),
                joined_by: row.joined_by.to_string(),
                mbid: self
                    .artists
                    .get(row.artist as usize)
                    .copied()
                    .flatten()
                    .map(mbid_of),
            })
            .collect()
    }

    fn credited_as(&self, id: i64) -> String {
        let Some(Some(rows)) = self.credits.get(at(id)) else {
            return String::new();
        };
        let mut credited = String::new();
        for row in rows.iter() {
            credited.push_str(&row.name);
            credited.push_str(&row.joined_by);
        }
        credited
    }

    fn group(&self, id: u32) -> Option<&GroupInfo> {
        self.groups.get(id as usize).and_then(Option::as_ref)
    }

    fn release(&self, id: i64) -> Option<&ReleaseInfo> {
        self.releases.get(at(id)).and_then(Option::as_ref)
    }

    fn kind_of(&self, group: &GroupInfo) -> Option<String> {
        group
            .kind
            .and_then(|kind| self.named.get(&("release_group_primary_type", kind)))
            .map(|name| name.to_string())
    }

    fn secondary_of(&self, group: &GroupInfo) -> Vec<String> {
        group
            .secondary
            .iter()
            .filter_map(|kind| self.named.get(&("release_group_secondary_type", *kind)))
            .map(|name| name.to_string())
            .collect()
    }
}

fn credited_as(credit: &[Credit]) -> String {
    credit
        .iter()
        .map(|named| format!("{}{}", named.name, named.joined_by))
        .collect()
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
    looked_up: &'a LookedUp,
    mirror: &'a Connection,
    placed: &'a Connection,
    building: &'a Building,
}

fn written(
    staging: &Connection,
    looked_up: &LookedUp,
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
            for kind in Entity::ALL {
                let mut packer = Packer::new(kind, &jobs, &mirror);
                match kind {
                    Entity::Release => writing.releases(&mut packer, &packed)?,
                    Entity::Recording => writing.recordings(&mut packer, &packed)?,
                    Entity::Group => writing.groups(&mut packer, &packed)?,
                    Entity::Artist => writing.artists(&mut packer, &packed)?,
                }
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

fn links_of(statement: &mut Statement<'_>, entity: i64) -> Result<Vec<Link>> {
    let failed = store_read(Store::Staging);
    let mut rows = statement.query([entity]).map_err(&failed)?;
    let mut seen = BTreeSet::new();
    let mut links = Vec::new();
    while let Some(row) = rows.next().map_err(&failed)? {
        let kind: Option<String> = row.get(0).map_err(&failed)?;
        let url: Option<String> = row.get(1).map_err(&failed)?;
        if let Some(url) = url
            && seen.insert(url.clone())
        {
            links.push(Link::new(kind.as_deref().unwrap_or_default(), url));
        }
    }
    Ok(links)
}

fn links_query(table: &str) -> String {
    format!(
        "SELECT lt.name, u.url FROM {table} l JOIN link k ON k.id = l.link \
         LEFT JOIN link_type lt ON lt.id = k.link_type JOIN url u ON u.id = l.entity1 \
         WHERE l.entity0 = ?1"
    )
}

fn texts_of(statement: &mut Statement<'_>, id: i64) -> Result<Vec<String>> {
    let failed = store_read(Store::Staging);
    let mut rows = statement.query([id]).map_err(&failed)?;
    let mut texts = Vec::new();
    while let Some(row) = rows.next().map_err(&failed)? {
        if let Some(text) = row.get::<_, Option<String>>(0).map_err(&failed)? {
            texts.push(text);
        }
    }
    Ok(texts)
}

fn present(text: Option<String>) -> Option<String> {
    text.filter(|text| !text.trim().is_empty())
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
        gid: [u8; MBID_BYTES],
        document: Vec<u8>,
        words: (&str, String),
    ) -> Result<i64> {
        let ordinal = packer.add(document)?;
        self.placed
            .prepare_cached("INSERT INTO placed (mbid, kind, ordinal) VALUES (?1, ?2, ?3)")
            .and_then(|mut statement| {
                statement.execute(params![gid.to_vec(), packer.kind.code(), ordinal])
            })
            .map_err(Error::store(Store::Staging, StoreOp::Write))?;
        let (table, words) = words;
        self.mirror
            .prepare_cached(&format!(
                "INSERT INTO {table} (rowid, words) VALUES (?1, ?2)"
            ))
            .and_then(|mut statement| statement.execute(params![ordinal, words]))
            .map_err(Error::store(Store::Mirror, StoreOp::Write))?;
        let documents = self.building.documents.fetch_add(1, Ordering::Relaxed) + 1;
        if documents.is_multiple_of(ROWS_BETWEEN_LOOKS) {
            self.drained(packed)?;
            self.building.looked_at()?;
        }
        Ok(ordinal)
    }

    fn releases(
        &self,
        packer: &mut Packer<'_>,
        packed: &mpsc::Receiver<io::Result<Packed>>,
    ) -> Result<()> {
        let failed = store_read(Store::Staging);
        let staging = self.staging;
        let mut media = staging
            .prepare(
                "SELECT id, position, format, name FROM medium WHERE release = ?1 \
                 ORDER BY position",
            )
            .map_err(&failed)?;
        let mut tracks = staging
            .prepare(
                "SELECT position, number, name, artist_credit, gid, length, recording_gid, \
                 recording_length, recording, isrcs FROM placed_tracks WHERE medium = ?1 \
                 ORDER BY position",
            )
            .map_err(&failed)?;
        let mut recording_links = staging
            .prepare(&links_query("l_recording_url"))
            .map_err(&failed)?;
        let mut release_links = staging
            .prepare(&links_query("l_release_url"))
            .map_err(&failed)?;
        let mut label = staging
            .prepare(
                "SELECT l.name, rl.catalog_number FROM release_label rl \
                 LEFT JOIN label l ON l.id = rl.label WHERE rl.release = ?1 LIMIT 1",
            )
            .map_err(&failed)?;
        let mut cover = staging
            .prepare("SELECT cover_art_presence FROM release_meta WHERE id = ?1")
            .map_err(&failed)?;
        let mut rows_of = staging
            .prepare("SELECT id, artist_credit, barcode, comment FROM release ORDER BY id")
            .map_err(&failed)?;
        let mut rows = rows_of.query([]).map_err(&failed)?;
        while let Some(row) = rows.next().map_err(&failed)? {
            let id: i64 = row.get(0).map_err(&failed)?;
            let Some(info) = self.looked_up.release(id) else {
                continue;
            };
            let credit = self.looked_up.credit(row.get(1).map_err(&failed)?);
            let credited = credited_as(&credit);
            let barcode = present(row.get(2).map_err(&failed)?);
            let disambiguation = present(row.get(3).map_err(&failed)?);
            let group = self.looked_up.group(info.group);

            let mut held_media = Vec::new();
            let mut medium_rows = media.query([id]).map_err(&failed)?;
            let mut read_media = Vec::new();
            while let Some(medium) = medium_rows.next().map_err(&failed)? {
                read_media.push((
                    medium.get::<_, i64>(0).map_err(&failed)?,
                    medium.get::<_, Option<i64>>(1).map_err(&failed)?,
                    medium.get::<_, Option<i64>>(2).map_err(&failed)?,
                    medium.get::<_, Option<String>>(3).map_err(&failed)?,
                ));
            }
            drop(medium_rows);
            for (medium, position, format, title) in read_media {
                let mut held_tracks = Vec::new();
                let mut track_rows = tracks.query([medium]).map_err(&failed)?;
                let mut read_tracks = Vec::new();
                while let Some(track) = track_rows.next().map_err(&failed)? {
                    read_tracks.push((
                        track.get::<_, Option<i64>>(0).map_err(&failed)?,
                        track.get::<_, Option<String>>(1).map_err(&failed)?,
                        track.get::<_, Option<String>>(2).map_err(&failed)?,
                        track.get::<_, Option<i64>>(3).map_err(&failed)?,
                        track.get::<_, Option<String>>(4).map_err(&failed)?,
                        track.get::<_, Option<i64>>(5).map_err(&failed)?,
                        track.get::<_, Option<String>>(6).map_err(&failed)?,
                        track.get::<_, Option<i64>>(7).map_err(&failed)?,
                        track.get::<_, i64>(8).map_err(&failed)?,
                        track.get::<_, Option<String>>(9).map_err(&failed)?,
                    ));
                }
                drop(track_rows);
                for (
                    place,
                    number,
                    title,
                    track_credit,
                    gid,
                    length,
                    recording,
                    own,
                    recording_id,
                    codes,
                ) in read_tracks
                {
                    let own_credit = track_credit
                        .map(|credit| self.looked_up.credited_as(credit))
                        .filter(|own| !own.is_empty() && *own != credited);
                    let place = u32::try_from(place.unwrap_or(0)).unwrap_or(0);
                    held_tracks.push(ReleaseTrack {
                        position: place,
                        number: number.unwrap_or_else(|| place.to_string()),
                        title: title.unwrap_or_default(),
                        artist: own_credit,
                        recording: recording.and_then(|gid| Mbid::new(&gid).ok()),
                        track: gid.and_then(|gid| Mbid::new(&gid).ok()),
                        length: length
                            .or(own)
                            .and_then(|millis| u64::try_from(millis).ok())
                            .map(Duration::from_millis),
                        isrcs: codes
                            .as_deref()
                            .unwrap_or_default()
                            .split(CODES_JOINED_BY)
                            .filter(|code| !code.is_empty())
                            .map(str::to_owned)
                            .collect(),
                        links: links_of(&mut recording_links, recording_id)?,
                    });
                }
                held_media.push(Medium {
                    position: u32::try_from(position.unwrap_or(0)).unwrap_or(0),
                    format: self.looked_up.name("medium_format", format),
                    title: present(title),
                    tracks: held_tracks,
                });
            }

            let (label_name, catalog_number) = label
                .query_row([id], |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                    ))
                })
                .optional()
                .map_err(&failed)?
                .unwrap_or_default();
            let covered: Option<String> = cover
                .query_row([id], |row| row.get(0))
                .optional()
                .map_err(&failed)?;

            let release = Release {
                id: mbid_of(info.gid),
                group: group.map(|group| mbid_of(group.gid)),
                title: info.title.to_string(),
                credit,
                date: info.date.as_deref().map(str::to_owned),
                country: info.country.as_deref().map(str::to_owned),
                label: stated(label_name),
                catalog_number: stated(catalog_number),
                barcode: barcode.clone(),
                kind: group.and_then(|group| self.looked_up.kind_of(group)),
                disambiguation,
                has_front_cover: covered.as_deref() == Some(COVER_PRESENT),
                links: links_of(&mut release_links, id)?,
                media: held_media,
            };
            let words = words_of_a_credited(&release.title, &release.credit);
            let doc = ReleaseDoc {
                release,
                status: info
                    .status
                    .and_then(|status| self.looked_up.named.get(&("release_status", status)))
                    .map(|name| name.to_string()),
            };
            let mut writer = Writer::new();
            doc.write(&mut writer);
            let ordinal = self.documented(
                packer,
                packed,
                info.gid,
                writer.finish(),
                ("release_words", words),
            )?;
            if let Some(barcode) = barcode {
                self.placed
                    .prepare_cached(
                        "INSERT INTO placed_barcodes (barcode, ordinal) VALUES (?1, ?2)",
                    )
                    .and_then(|mut statement| statement.execute(params![barcode, ordinal]))
                    .map_err(Error::store(Store::Staging, StoreOp::Write))?;
            }
        }
        Ok(())
    }

    fn recordings(
        &self,
        packer: &mut Packer<'_>,
        packed: &mpsc::Receiver<io::Result<Packed>>,
    ) -> Result<()> {
        let failed = store_read(Store::Staging);
        let staging = self.staging;
        let mut isrcs = staging
            .prepare("SELECT isrc FROM isrc WHERE recording = ?1 ORDER BY isrc")
            .map_err(&failed)?;
        let mut appearances = staging
            .prepare(
                "SELECT m.release, m.position, t.position FROM track t \
                 JOIN medium m ON m.id = t.medium WHERE t.recording = ?1",
            )
            .map_err(&failed)?;
        let mut rows_of = staging
            .prepare("SELECT id, gid, name, artist_credit, length FROM recording ORDER BY id")
            .map_err(&failed)?;
        let mut rows = rows_of.query([]).map_err(&failed)?;
        while let Some(row) = rows.next().map_err(&failed)? {
            let id: i64 = row.get(0).map_err(&failed)?;
            let gid: String = row.get(1).map_err(&failed)?;
            let Some(gid) = gid_bytes(&gid) else {
                continue;
            };
            let mut on: Vec<RecordingRelease> = Vec::new();
            let mut seen = BTreeSet::new();
            let mut appeared = appearances.query([id]).map_err(&failed)?;
            while let Some(appearance) = appeared.next().map_err(&failed)? {
                let release: i64 = appearance.get(0).map_err(&failed)?;
                if !seen.insert(release) {
                    continue;
                }
                let Some(info) = self.looked_up.release(release) else {
                    continue;
                };
                let group = self.looked_up.group(info.group);
                let disc: Option<i64> = appearance.get(1).map_err(&failed)?;
                let position: Option<i64> = appearance.get(2).map_err(&failed)?;
                on.push(RecordingRelease {
                    id: mbid_of(info.gid),
                    title: info.title.to_string(),
                    date: info.date.as_deref().map(str::to_owned),
                    disc: disc.and_then(|disc| u32::try_from(disc).ok()),
                    position: position.and_then(|position| u32::try_from(position).ok()),
                    issued: Issued {
                        kind: group.and_then(|group| self.looked_up.kind_of(group)),
                        secondary: group
                            .map(|group| self.looked_up.secondary_of(group))
                            .unwrap_or_default(),
                        status: info
                            .status
                            .and_then(|status| {
                                self.looked_up.named.get(&("release_status", status))
                            })
                            .map(|name| name.to_string()),
                    },
                });
            }
            drop(appeared);
            on.sort_by(|one, other| {
                (one.date.is_none(), &one.date, &one.title).cmp(&(
                    other.date.is_none(),
                    &other.date,
                    &other.title,
                ))
            });
            on.truncate(RELEASES_OF_A_RECORDING_KEPT);
            let codes = texts_of(&mut isrcs, id)?;
            let length: Option<i64> = row.get(4).map_err(&failed)?;
            let recording = Recording {
                id: mbid_of(gid),
                title: row
                    .get::<_, Option<String>>(2)
                    .map_err(&failed)?
                    .unwrap_or_default(),
                credit: self.looked_up.credit(row.get(3).map_err(&failed)?),
                length: length
                    .and_then(|millis| u64::try_from(millis).ok())
                    .map(Duration::from_millis),
                isrcs: codes
                    .iter()
                    .filter_map(|code| Isrc::new(code).ok())
                    .collect(),
                releases: on,
            };
            let words = words_of_a_credited(&recording.title, &recording.credit);
            let mut writer = Writer::new();
            writer.recording(&recording);
            let ordinal = self.documented(
                packer,
                packed,
                gid,
                writer.finish(),
                ("recording_words", words),
            )?;
            for isrc in &recording.isrcs {
                self.placed
                    .prepare_cached("INSERT INTO placed_isrcs (isrc, ordinal) VALUES (?1, ?2)")
                    .and_then(|mut statement| statement.execute(params![isrc.as_str(), ordinal]))
                    .map_err(Error::store(Store::Staging, StoreOp::Write))?;
            }
        }
        Ok(())
    }

    fn groups(
        &self,
        packer: &mut Packer<'_>,
        packed: &mpsc::Receiver<io::Result<Packed>>,
    ) -> Result<()> {
        let failed = store_read(Store::Staging);
        let staging = self.staging;
        let mut links = staging
            .prepare(&links_query("l_release_group_url"))
            .map_err(&failed)?;
        let mut releases = staging
            .prepare("SELECT id FROM release WHERE release_group = ?1")
            .map_err(&failed)?;
        let mut rows_of = staging
            .prepare("SELECT id, artist_credit, comment FROM release_group ORDER BY id")
            .map_err(&failed)?;
        let mut rows = rows_of.query([]).map_err(&failed)?;
        while let Some(row) = rows.next().map_err(&failed)? {
            let id: i64 = row.get(0).map_err(&failed)?;
            let Some(info) = self.looked_up.group(small(id)) else {
                continue;
            };
            let mut pressings = Vec::new();
            let mut pressed = releases.query([id]).map_err(&failed)?;
            while let Some(release) = pressed.next().map_err(&failed)? {
                let Some(release) = self.looked_up.release(release.get(0).map_err(&failed)?) else {
                    continue;
                };
                pressings.push(GroupRelease {
                    id: mbid_of(release.gid),
                    title: release.title.to_string(),
                    date: release.date.as_deref().map(str::to_owned),
                    country: release.country.as_deref().map(str::to_owned),
                    track_count: (!release.discs.is_empty()).then(|| release.discs.iter().sum()),
                    discs: release.discs.to_vec(),
                });
            }
            drop(pressed);
            pressings.sort_by(|one, other| {
                (one.date.is_none(), &one.date, &one.title).cmp(&(
                    other.date.is_none(),
                    &other.date,
                    &other.title,
                ))
            });
            let group = ReleaseGroup {
                id: mbid_of(info.gid),
                title: info.title.to_string(),
                credit: self.looked_up.credit(row.get(1).map_err(&failed)?),
                kind: self.looked_up.kind_of(info),
                first_released: info.first_released.as_deref().map(str::to_owned),
                disambiguation: present(row.get(2).map_err(&failed)?),
                links: links_of(&mut links, id)?,
                releases: pressings,
            };
            let words = words_of_a_credited(&group.title, &group.credit);
            let doc = GroupDoc {
                group,
                secondary: self.looked_up.secondary_of(info),
            };
            let mut writer = Writer::new();
            doc.write(&mut writer);
            self.documented(
                packer,
                packed,
                info.gid,
                writer.finish(),
                ("group_words", words),
            )?;
        }
        Ok(())
    }

    fn artists(
        &self,
        packer: &mut Packer<'_>,
        packed: &mpsc::Receiver<io::Result<Packed>>,
    ) -> Result<()> {
        let failed = store_read(Store::Staging);
        let staging = self.staging;
        let mut links = staging
            .prepare(&links_query("l_artist_url"))
            .map_err(&failed)?;
        let mut aliases = staging
            .prepare("SELECT name FROM artist_alias WHERE artist = ?1")
            .map_err(&failed)?;
        let mut tags = staging
            .prepare(&format!(
                "SELECT t.name, a.count FROM artist_tag a JOIN tag t ON t.id = a.tag \
                 WHERE a.artist = ?1 AND a.count > 0 ORDER BY a.count DESC, t.name LIMIT {GENRES_KEPT}"
            ))
            .map_err(&failed)?;
        let mut credited = staging
            .prepare(
                "SELECT DISTINCT rg.id FROM artist_credit_name acn \
                 JOIN release_group rg ON rg.artist_credit = acn.artist_credit \
                 WHERE acn.artist = ?1",
            )
            .map_err(&failed)?;
        let mut rows_of = staging
            .prepare(
                "SELECT id, gid, name, sort_name, by, bm, bd, ey, em, ed, type, area, gender, \
                 comment, ended, begin_area FROM artist ORDER BY id",
            )
            .map_err(&failed)?;
        let mut rows = rows_of.query([]).map_err(&failed)?;
        while let Some(row) = rows.next().map_err(&failed)? {
            let id: i64 = row.get(0).map_err(&failed)?;
            let gid: String = row.get(1).map_err(&failed)?;
            let Some(gid) = gid_bytes(&gid) else {
                continue;
            };
            let name: String = row
                .get::<_, Option<String>>(2)
                .map_err(&failed)?
                .unwrap_or_default();
            let billed = folded_letters(&name);
            let mut folds = vec![billed.clone()];
            let mut known_as = Vec::new();
            for alias in texts_of(&mut aliases, id)? {
                let fold = folded_letters(&alias);
                if alias.trim().is_empty() || folds.contains(&fold) {
                    continue;
                }
                folds.push(fold);
                known_as.push(alias);
            }
            let mut genres = Vec::new();
            let mut tagged = tags.query([id]).map_err(&failed)?;
            while let Some(tag) = tagged.next().map_err(&failed)? {
                let named: Option<String> = tag.get(0).map_err(&failed)?;
                let count: i64 = tag.get(1).map_err(&failed)?;
                if let Some(named) = present(named) {
                    genres.push(Genre {
                        name: named,
                        weight: u32::try_from(count).unwrap_or(u32::MAX),
                    });
                }
            }
            drop(tagged);
            let mut groups = Vec::new();
            let mut credits = credited.query([id]).map_err(&failed)?;
            while let Some(group) = credits.next().map_err(&failed)? {
                let Some(info) = self.looked_up.group(small(group.get(0).map_err(&failed)?)) else {
                    continue;
                };
                let kind = self.looked_up.kind_of(info);
                if !kind
                    .as_deref()
                    .is_some_and(|kind| DISCOGRAPHY_KINDS.contains(&kind))
                {
                    continue;
                }
                groups.push(ArtistRelease {
                    mbid: mbid_of(info.gid),
                    title: info.title.to_string(),
                    kind,
                    secondary: self.looked_up.secondary_of(info),
                    first_released: info.first_released.as_deref().map(str::to_owned),
                });
            }
            drop(credits);
            groups.sort_by(|one, other| {
                (
                    one.first_released.is_none(),
                    &one.first_released,
                    &one.title,
                )
                    .cmp(&(
                        other.first_released.is_none(),
                        &other.first_released,
                        &other.title,
                    ))
            });
            let credited_count = u32::try_from(groups.len()).unwrap_or(u32::MAX);
            groups.truncate(GROUPS_OF_AN_ARTIST_KEPT);

            let area: Option<i64> = row.get(11).map_err(&failed)?;
            let ended: Option<String> = row.get(14).map_err(&failed)?;
            let sort_name = present(row.get(3).map_err(&failed)?);
            let artist = ArtistProfile {
                mbid: mbid_of(gid),
                name: name.clone(),
                sort_name: sort_name.clone(),
                kind: self
                    .looked_up
                    .name("artist_type", row.get(10).map_err(&failed)?),
                gender: self.looked_up.name("gender", row.get(12).map_err(&failed)?),
                country: self.looked_up.name("iso_3166_1", area),
                area: self.looked_up.name("area", area),
                began_in: self.looked_up.name("area", row.get(15).map_err(&failed)?),
                span: LifeSpan {
                    begin: dated(
                        row.get(4).map_err(&failed)?,
                        row.get(5).map_err(&failed)?,
                        row.get(6).map_err(&failed)?,
                    ),
                    end: dated(
                        row.get(7).map_err(&failed)?,
                        row.get(8).map_err(&failed)?,
                        row.get(9).map_err(&failed)?,
                    ),
                    ended: ended.as_deref() == Some(ENDED),
                },
                disambiguation: present(row.get(13).map_err(&failed)?),
                aliases: known_as,
                genres,
                links: links_of(&mut links, id)?,
            };
            let mut words = vec![artist.name.clone()];
            words.extend(sort_name);
            words.extend(artist.aliases.iter().cloned());
            let words = words
                .iter()
                .flat_map(|named| resonate_core::words_of_a_name(named))
                .collect::<Vec<_>>()
                .join(" ");
            let doc = ArtistDoc {
                artist,
                groups,
                credited: credited_count,
            };
            let mut writer = Writer::new();
            doc.write(&mut writer);
            self.documented(
                packer,
                packed,
                gid,
                writer.finish(),
                ("artist_words", words),
            )?;
        }
        Ok(())
    }
}
