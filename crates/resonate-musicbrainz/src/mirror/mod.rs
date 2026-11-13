mod amended;
mod copy;
mod documents;
mod import;
mod looked_up;
mod offered;
pub(crate) mod replicated;
mod search;
mod tables;

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use parking_lot::Mutex;
use resonate_core::{Isrc, Mbid};
use resonate_library::{
    ArtistPressings, ArtistProfile, ArtistRelease, Barcode, BarcodeMatch, Discography, Recording,
    Release, ReleaseGroup,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use self::amended::{Amended, Amendment};
pub use self::{
    amended::{AMENDED_FILE, AmendedTo},
    import::{
        Archives, Building, Built, Keeping, Phase, built_from, held_for_building,
        removed_beside_the_mirror, settled,
    },
    offered::{CopyPart, Offered, OfferedFile, received_amendments, received_copy, removed_copy},
    tables::{KeptTables, TABLES_FILE},
};
use crate::{
    Answer, Asked, Entity, Error, PackOp, Result, Store, StoreOp,
    wire::{Reader, Writer, mbid_bytes},
};

pub const FORMAT: u32 = 1;
pub const MIRROR_FILE: &str = "musicbrainz.sqlite";
pub const PRESSINGS_READ: usize = 25;
pub const ARTIST_PRESSINGS_PAGE: usize = 100;
pub const GROUPS_AT_MOST: usize = 1_000;
pub const DISCOGRAPHY_KINDS: [&str; 3] = ["Album", "EP", "Single"];
pub const OFFICIAL: &str = "Official";
const BLOCKS_KEPT_UNPACKED: usize = 64;
const ARTISTS_KEPT_LISTED: usize = 4;
const MOST_UNPACKED_BLOCK: usize = 16 * 1024 * 1024;

pub(crate) const SCHEMA: &str = "
CREATE TABLE made (format INTEGER NOT NULL, dump TEXT NOT NULL, built INTEGER NOT NULL);
CREATE TABLE dictionaries (kind INTEGER PRIMARY KEY, bytes BLOB NOT NULL);
CREATE TABLE blocks (
    id INTEGER PRIMARY KEY,
    kind INTEGER NOT NULL,
    first INTEGER NOT NULL,
    unpacked INTEGER NOT NULL,
    packed BLOB NOT NULL
);
CREATE TABLE entities (mbid BLOB PRIMARY KEY, kind INTEGER NOT NULL, id INTEGER NOT NULL) WITHOUT ROWID;
CREATE TABLE isrcs (isrc TEXT NOT NULL, id INTEGER NOT NULL, PRIMARY KEY (isrc, id)) WITHOUT ROWID;
CREATE TABLE barcodes (barcode TEXT NOT NULL, id INTEGER NOT NULL, PRIMARY KEY (barcode, id)) WITHOUT ROWID;
CREATE VIRTUAL TABLE recording_words USING fts5(words, content='', detail=none, tokenize='unicode61 remove_diacritics 2');
CREATE VIRTUAL TABLE release_words USING fts5(words, content='', detail=none, tokenize='unicode61 remove_diacritics 2');
CREATE VIRTUAL TABLE group_words USING fts5(words, content='', detail=none, tokenize='unicode61 remove_diacritics 2');
CREATE VIRTUAL TABLE artist_words USING fts5(words, content='', detail=none, tokenize='unicode61 remove_diacritics 2');
";

pub(crate) const BLOCKS_INDEXED: &str = "CREATE INDEX blocks_by_first ON blocks(kind, first);";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Made {
    pub dump: String,
    pub built: u64,
}

pub(crate) struct ReleaseDoc {
    pub release: Release,
    pub status: Option<String>,
}

pub(crate) struct GroupDoc {
    pub group: ReleaseGroup,
    pub secondary: Vec<String>,
}

pub(crate) struct ArtistDoc {
    pub artist: ArtistProfile,
    pub groups: Vec<ArtistRelease>,
    pub credited: u32,
}

impl ReleaseDoc {
    pub fn write(&self, writer: &mut Writer) {
        writer.release(&self.release);
        writer.maybe_text(self.status.as_deref());
    }

    fn read(reader: &mut Reader<'_>) -> Result<Self> {
        Ok(Self {
            release: reader.release()?,
            status: reader.maybe_text()?,
        })
    }
}

impl GroupDoc {
    pub fn write(&self, writer: &mut Writer) {
        writer.group(&self.group);
        writer.texts(&self.secondary);
    }

    fn read(reader: &mut Reader<'_>) -> Result<Self> {
        Ok(Self {
            group: reader.group()?,
            secondary: reader.texts()?,
        })
    }
}

impl ArtistDoc {
    pub fn write(&self, writer: &mut Writer) {
        writer.artist(&self.artist);
        writer.artist_releases(&self.groups);
        writer.number(u64::from(self.credited));
    }

    fn read(reader: &mut Reader<'_>) -> Result<Self> {
        Ok(Self {
            artist: reader.artist()?,
            groups: reader.artist_releases()?,
            credited: reader.small()?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum At {
    Built(i64),
    Amended(i64),
}

pub(crate) trait Identified {
    fn mbid(&self) -> &Mbid;
}

impl Identified for Recording {
    fn mbid(&self) -> &Mbid {
        &self.id
    }
}

impl Identified for ReleaseDoc {
    fn mbid(&self) -> &Mbid {
        &self.release.id
    }
}

impl Identified for GroupDoc {
    fn mbid(&self) -> &Mbid {
        &self.group.id
    }
}

impl Identified for ArtistDoc {
    fn mbid(&self) -> &Mbid {
        &self.artist.mbid
    }
}

struct Unpacked {
    kind: Entity,
    first: i64,
    bytes: Arc<[u8]>,
}

struct Listed {
    artist: Mbid,
    credited: Arc<[Mbid]>,
}

pub struct Mirror {
    path: PathBuf,
    connection: Mutex<Connection>,
    dictionaries: Vec<(Entity, Vec<u8>)>,
    unpacked: Mutex<VecDeque<Unpacked>>,
    listed: Mutex<VecDeque<Listed>>,
    amended: Option<Mutex<Amended>>,
    amended_to: Option<AmendedTo>,
    made: Made,
}

#[cfg(fuzzing)]
pub(crate) fn read_a_copy_line(line: &[u8]) {
    let mut split = Vec::new();
    copy::fields(line, &mut split);
    for field in split {
        let _ = copy::unescaped(field);
    }
}

pub fn mirror_in(folder: &Path) -> PathBuf {
    folder.join(MIRROR_FILE)
}

impl Mirror {
    pub fn open(path: &Path) -> Result<Self> {
        if !path.is_file() {
            return Err(Error::NoMirror {
                path: path.to_path_buf(),
            });
        }
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(Error::store(Store::Mirror, StoreOp::Open))?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(Error::store(Store::Mirror, StoreOp::Configure))?;
        let (format, dump, built): (u32, String, i64) = connection
            .query_row("SELECT format, dump, built FROM made", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(Error::store(Store::Mirror, StoreOp::Read))?;
        if format != FORMAT {
            return Err(Error::MirrorFormat {
                found: format,
                reads: FORMAT,
            });
        }
        let dictionaries = {
            let mut statement = connection
                .prepare("SELECT kind, bytes FROM dictionaries")
                .map_err(Error::store(Store::Mirror, StoreOp::Read))?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, u8>(0)?, row.get::<_, Vec<u8>>(1)?))
                })
                .map_err(Error::store(Store::Mirror, StoreOp::Read))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Error::store(Store::Mirror, StoreOp::Read))?
                .into_iter()
                .filter_map(|(kind, bytes)| Entity::coded(kind).map(|kind| (kind, bytes)))
                .collect()
        };
        let amended = match path.parent() {
            Some(folder) => Amended::open(&folder.join(AMENDED_FILE), &dump).unwrap_or_else(|error| {
                tracing::warn!(%error, "the changes kept beside the copy of MusicBrainz could not be opened");
                None
            }),
            None => None,
        };
        Ok(Self {
            path: path.to_path_buf(),
            connection: Mutex::new(connection),
            dictionaries,
            unpacked: Mutex::new(VecDeque::new()),
            listed: Mutex::new(VecDeque::new()),
            amended_to: amended.as_ref().map(|amended| amended.to.clone()),
            amended: amended.map(Mutex::new),
            made: Made {
                dump,
                built: u64::try_from(built).unwrap_or(0),
            },
        })
    }

    pub fn amended_to(&self) -> Option<&AmendedTo> {
        self.amended_to.as_ref()
    }

    pub(crate) fn dictionaries(&self) -> &[(Entity, Vec<u8>)] {
        &self.dictionaries
    }

    pub(crate) fn holds_built(&self, id: &Mbid, kind: Entity) -> Result<bool> {
        Ok(self.built_placed(id, kind)?.is_some())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn made(&self) -> &Made {
        &self.made
    }

    pub fn answer(&self, asked: &Asked) -> Result<Answer> {
        Ok(match asked {
            Asked::Release(id) => Answer::Release(self.release(id)?),
            Asked::FindRelease(asked) => Answer::Releases(self.find_release(asked)?),
            Asked::Barcode(barcode) => Answer::Barcoded(self.releases_by_barcode(barcode)?),
            Asked::Recording(id) => Answer::Recording(self.recording(id)?),
            Asked::Isrc(isrc) => Answer::Recordings(self.recordings_of_isrc(isrc)?),
            Asked::FindRecording(asked) => Answer::RecordingMatches(self.find_recording(asked)?),
            Asked::FindSongs(asked) => Answer::RecordingMatches(self.find_songs(asked)?),
            Asked::Group(id) => Answer::Group(self.release_group(id)?),
            Asked::FindGroup(asked) => Answer::Groups(self.find_release_group(asked)?),
            Asked::FindAlbums(words) => Answer::Albums(self.find_albums(words)?),
            Asked::Artist(id) => Answer::Artist(self.artist(id)?),
            Asked::FindArtist(name) => Answer::Artists(self.find_artist(name)?),
            Asked::GroupsOf { artist, from } => {
                Answer::Discography(self.release_groups_of(artist, *from)?)
            }
            Asked::ReleasesOfGroup(id) => Answer::Pressings(self.releases_of_group(id)?),
            Asked::ReleasesOfArtist { artist, from } => {
                Answer::ArtistPressings(self.releases_of_artist(artist, *from)?)
            }
        })
    }

    pub fn release(&self, id: &Mbid) -> Result<Option<Release>> {
        Ok(self.release_doc(id)?.map(|doc| doc.release))
    }

    pub fn recording(&self, id: &Mbid) -> Result<Option<Recording>> {
        match self.placed(id, Entity::Recording)? {
            Some(at) => self.recording_at(at),
            None => Ok(None),
        }
    }

    pub fn release_group(&self, id: &Mbid) -> Result<Option<ReleaseGroup>> {
        Ok(self.group_doc(id)?.map(|doc| doc.group))
    }

    pub fn artist(&self, id: &Mbid) -> Result<Option<ArtistProfile>> {
        Ok(self.artist_doc(id)?.map(|doc| doc.artist))
    }

    pub fn recordings_of_isrc(&self, isrc: &Isrc) -> Result<Vec<Recording>> {
        let ids = self.coded(
            Entity::Recording,
            "SELECT id FROM isrcs WHERE isrc = ?1",
            isrc.as_str(),
        )?;
        let mut recordings = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(recording) = self.recording_at(id)? {
                recordings.push(recording);
            }
        }
        Ok(recordings)
    }

    pub fn releases_by_barcode(&self, barcode: &Barcode) -> Result<Vec<BarcodeMatch>> {
        let mut found = Vec::new();
        let mut seen = Vec::new();
        for spelt in barcode.spellings() {
            for id in self.coded(
                Entity::Release,
                "SELECT id FROM barcodes WHERE barcode = ?1",
                &spelt,
            )? {
                if seen.contains(&id) {
                    continue;
                }
                seen.push(id);
                if let Some(doc) = self.release_at(id)? {
                    let release = doc.release;
                    found.push(BarcodeMatch {
                        release: release.id,
                        group: release.group,
                        barcode: release.barcode,
                        title: release.title,
                        credit: release.credit,
                    });
                }
            }
        }
        Ok(found)
    }

    pub fn releases_of_group(&self, id: &Mbid) -> Result<Vec<Release>> {
        let Some(doc) = self.group_doc(id)? else {
            return Ok(Vec::new());
        };
        let mut pressings = Vec::new();
        for release in &doc.group.releases {
            if pressings.len() == PRESSINGS_READ {
                break;
            }
            if let Some(pressing) = self.release_doc(&release.id)?
                && pressing.status.as_deref() == Some(OFFICIAL)
            {
                pressings.push(pressing.release);
            }
        }
        Ok(pressings)
    }

    pub fn release_groups_of(&self, artist: &Mbid, from: u32) -> Result<Discography> {
        let Some(doc) = self.artist_doc(artist)? else {
            return Ok(Discography::default());
        };
        let held = doc.groups.len();
        let start = usize::try_from(from).unwrap_or(usize::MAX).min(held);
        let end = start.saturating_add(GROUPS_AT_MOST).min(held);
        let read_to = u32::try_from(end).unwrap_or(u32::MAX);
        Ok(Discography {
            releases: doc.groups[start..end].to_vec(),
            unread: doc.credited.saturating_sub(read_to),
            read_to,
        })
    }

    pub fn releases_of_artist(&self, artist: &Mbid, from: u32) -> Result<ArtistPressings> {
        let Some(credited) = self.credited_releases(artist)? else {
            return Ok(ArtistPressings::default());
        };
        let start = usize::try_from(from)
            .unwrap_or(usize::MAX)
            .min(credited.len());
        let mut pressings = Vec::new();
        let mut read_to = start;
        for id in &credited[start..] {
            if pressings.len() == ARTIST_PRESSINGS_PAGE {
                break;
            }
            read_to += 1;
            if let Some(pressing) = self.release_doc(id)?
                && pressing.status.as_deref() == Some(OFFICIAL)
            {
                pressings.push(pressing.release);
            }
        }
        Ok(ArtistPressings {
            pressings,
            credited: u32::try_from(credited.len()).unwrap_or(u32::MAX),
            read_to: u32::try_from(read_to).unwrap_or(u32::MAX),
        })
    }

    fn credited_releases(&self, artist: &Mbid) -> Result<Option<Arc<[Mbid]>>> {
        if let Some(listed) = self
            .listed
            .lock()
            .iter()
            .find(|listed| listed.artist == *artist)
        {
            return Ok(Some(Arc::clone(&listed.credited)));
        }
        let Some(doc) = self.artist_doc(artist)? else {
            return Ok(None);
        };
        let mut credited = Vec::new();
        for group in &doc.groups {
            if let Some(group) = self.group_doc(&group.mbid)? {
                credited.extend(group.group.releases.into_iter().map(|release| release.id));
            }
        }
        let credited: Arc<[Mbid]> = credited.into();
        let mut listed = self.listed.lock();
        if listed.len() == ARTISTS_KEPT_LISTED {
            listed.pop_front();
        }
        listed.push_back(Listed {
            artist: artist.clone(),
            credited: Arc::clone(&credited),
        });
        Ok(Some(credited))
    }

    pub(crate) fn release_doc(&self, id: &Mbid) -> Result<Option<ReleaseDoc>> {
        match self.placed(id, Entity::Release)? {
            Some(at) => self.release_at(at),
            None => Ok(None),
        }
    }

    pub(crate) fn group_doc(&self, id: &Mbid) -> Result<Option<GroupDoc>> {
        match self.placed(id, Entity::Group)? {
            Some(at) => self.group_at(at),
            None => Ok(None),
        }
    }

    pub(crate) fn artist_doc(&self, id: &Mbid) -> Result<Option<ArtistDoc>> {
        match self.placed(id, Entity::Artist)? {
            Some(at) => self.artist_at(at),
            None => Ok(None),
        }
    }

    pub(crate) fn recording_at(&self, at: At) -> Result<Option<Recording>> {
        self.document(Entity::Recording, at, |reader| reader.recording())
    }

    pub(crate) fn release_at(&self, at: At) -> Result<Option<ReleaseDoc>> {
        self.document(Entity::Release, at, ReleaseDoc::read)
    }

    pub(crate) fn group_at(&self, at: At) -> Result<Option<GroupDoc>> {
        self.document(Entity::Group, at, GroupDoc::read)
    }

    pub(crate) fn artist_at(&self, at: At) -> Result<Option<ArtistDoc>> {
        self.document(Entity::Artist, at, ArtistDoc::read)
    }

    fn placed(&self, id: &Mbid, kind: Entity) -> Result<Option<At>> {
        if let Some(amended) = &self.amended {
            match amended.lock().placed(id, kind)? {
                Some(Amendment::Rewritten(at)) => return Ok(Some(At::Amended(at))),
                Some(Amendment::Gone) => return Ok(None),
                None => {}
            }
        }
        Ok(self.built_placed(id, kind)?.map(At::Built))
    }

    fn built_placed(&self, id: &Mbid, kind: Entity) -> Result<Option<i64>> {
        let connection = self.connection.lock();
        connection
            .prepare_cached("SELECT id FROM entities WHERE mbid = ?1 AND kind = ?2")
            .and_then(|mut statement| {
                statement
                    .query_row(params![mbid_bytes(id).to_vec(), kind.code()], |row| {
                        row.get(0)
                    })
                    .optional()
            })
            .map_err(Error::store(Store::Mirror, StoreOp::Read))
    }

    fn coded(&self, kind: Entity, query: &str, value: &str) -> Result<Vec<At>> {
        let mut found: Vec<At> = self
            .ids_where(query, value)?
            .into_iter()
            .map(At::Built)
            .collect();
        if let Some(amended) = &self.amended {
            found.extend(
                amended
                    .lock()
                    .coded(kind, value)?
                    .into_iter()
                    .map(At::Amended),
            );
        }
        Ok(found)
    }

    fn ids_where(&self, query: &str, value: &str) -> Result<Vec<i64>> {
        let connection = self.connection.lock();
        let mut statement = connection
            .prepare_cached(query)
            .map_err(Error::store(Store::Mirror, StoreOp::Read))?;
        let rows = statement
            .query_map([value], |row| row.get(0))
            .map_err(Error::store(Store::Mirror, StoreOp::Read))?;
        rows.collect::<rusqlite::Result<Vec<i64>>>()
            .map_err(Error::store(Store::Mirror, StoreOp::Read))
    }

    pub(crate) fn matched(&self, table: &str, query: &str, most: usize) -> Result<Vec<At>> {
        let mut found: Vec<At> = self
            .built_matched(table, query, most)?
            .into_iter()
            .map(At::Built)
            .collect();
        if let Some(amended) = &self.amended {
            found.extend(
                amended
                    .lock()
                    .matched(table, query, most)?
                    .into_iter()
                    .map(At::Amended),
            );
        }
        Ok(found)
    }

    fn built_matched(&self, table: &str, query: &str, most: usize) -> Result<Vec<i64>> {
        let connection = self.connection.lock();
        let sql =
            format!("SELECT rowid FROM {table} WHERE {table} MATCH ?1 ORDER BY rank LIMIT ?2");
        let mut statement = connection
            .prepare_cached(&sql)
            .map_err(Error::store(Store::Mirror, StoreOp::Search))?;
        let rows = statement
            .query_map(
                params![query, i64::try_from(most).unwrap_or(i64::MAX)],
                |row| row.get(0),
            )
            .map_err(Error::store(Store::Mirror, StoreOp::Search))?;
        rows.collect::<rusqlite::Result<Vec<i64>>>()
            .map_err(Error::store(Store::Mirror, StoreOp::Search))
    }

    fn document<T: Identified>(
        &self,
        kind: Entity,
        at: At,
        read: impl FnOnce(&mut Reader<'_>) -> Result<T>,
    ) -> Result<Option<T>> {
        match at {
            At::Built(at) => {
                let Some(found) = self.built_document(kind, at, read)? else {
                    return Ok(None);
                };
                if let Some(amended) = &self.amended
                    && amended.lock().supersedes(kind, found.mbid())?
                {
                    return Ok(None);
                }
                Ok(Some(found))
            }
            At::Amended(at) => {
                let Some(amended) = &self.amended else {
                    return Ok(None);
                };
                let Some((held, packed)) = amended.lock().packed(at)? else {
                    return Ok(None);
                };
                if held != kind {
                    return Ok(None);
                }
                let bytes = self.unpack(kind, &packed, MOST_UNPACKED_BLOCK)?;
                let mut reader = Reader::new(&bytes);
                let read = read(&mut reader)?;
                reader.done()?;
                Ok(Some(read))
            }
        }
    }

    fn built_document<T>(
        &self,
        kind: Entity,
        at: i64,
        read: impl FnOnce(&mut Reader<'_>) -> Result<T>,
    ) -> Result<Option<T>> {
        let block = self.block(kind, at)?;
        let Some(block) = block else {
            return Ok(None);
        };
        let mut reader = Reader::new(&block);
        while !reader.is_done() {
            let id = reader.signed()?;
            let bytes = reader.bytes(MOST_UNPACKED_BLOCK)?;
            if id == at {
                let mut within = Reader::new(bytes);
                let read = read(&mut within)?;
                within.done()?;
                return Ok(Some(read));
            }
        }
        Ok(None)
    }

    fn block(&self, kind: Entity, at: i64) -> Result<Option<Arc<[u8]>>> {
        let held: Option<(i64, i64, Vec<u8>)> = {
            let connection = self.connection.lock();
            connection
                .prepare_cached(
                    "SELECT first, unpacked, packed FROM blocks \
                     WHERE kind = ?1 AND first <= ?2 ORDER BY first DESC LIMIT 1",
                )
                .and_then(|mut statement| {
                    statement
                        .query_row(params![kind.code(), at], |row| {
                            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                        })
                        .optional()
                })
                .map_err(Error::store(Store::Mirror, StoreOp::Read))?
        };
        let Some((first, unpacked, packed)) = held else {
            return Ok(None);
        };
        if let Some(cached) = self
            .unpacked
            .lock()
            .iter()
            .find(|cached| cached.kind == kind && cached.first == first)
        {
            return Ok(Some(Arc::clone(&cached.bytes)));
        }
        let capacity = usize::try_from(unpacked)
            .unwrap_or(usize::MAX)
            .min(MOST_UNPACKED_BLOCK);
        let bytes: Arc<[u8]> = self.unpack(kind, &packed, capacity)?.into();
        let mut cache = self.unpacked.lock();
        if cache.len() == BLOCKS_KEPT_UNPACKED {
            cache.pop_front();
        }
        cache.push_back(Unpacked {
            kind,
            first,
            bytes: Arc::clone(&bytes),
        });
        Ok(Some(bytes))
    }

    fn unpack(&self, kind: Entity, packed: &[u8], capacity: usize) -> Result<Vec<u8>> {
        let unpack = |source| Error::Pack {
            op: PackOp::Unpack,
            source,
        };
        let mut decompressor = match self.dictionaries.iter().find(|(held, _)| *held == kind) {
            Some((_, dictionary)) => {
                zstd::bulk::Decompressor::with_dictionary(dictionary).map_err(unpack)?
            }
            None => zstd::bulk::Decompressor::new().map_err(unpack)?,
        };
        decompressor
            .decompress(packed, capacity)
            .map_err(|source| Error::Pack {
                op: PackOp::Unpack,
                source,
            })
    }
}
