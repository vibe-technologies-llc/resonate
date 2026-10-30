use std::{collections::BTreeMap, path::Path, time::Duration};

use resonate_core::{Answer, Answered, Recording, RecordingKey, WholeFile};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use crate::{
    DeviceId, Error, Result, SpaceId, StoreOp,
    catalog::{
        Catalog, LocalPlaylist, LocalPlaylistId, LocalQuery, Refused, RemoteListen, Resumed,
    },
    change::{
        AnswerNote, Change, Clock, DeviceForgotten, DeviceNote, FavouriteNote, Handoff, Listen,
        Platform, PlaylistBody, PlaylistFolded, PlaylistKey, PlaylistNote, QueryBody, QueryFolded,
        QueryNote, Stamp, wall_nanos,
    },
    merge::{
        DIGEST_BYTES, Folds, Synced, absorbed, named_as_a_free_alias_of, names_alike,
        one_recording_a_song, projection_digest, query_digest, spliced,
    },
    wire::{
        decoded_answer, decoded_recording, decoded_recordings, encoded_answer, encoded_change,
        encoded_recording, encoded_recordings,
    },
};

pub const BUNDLE_BYTES: usize = 192 * 1024;
pub const LISTENS_LOOKED_AT_ONCE: usize = 2_000;
pub const LISTENS_KEPT_FOR: Duration = Duration::from_secs(400 * 24 * 60 * 60);
pub const ANSWERS_LOOKED_AT_ONCE: usize = 500;
const ANSWER_PAGES_A_LOOK: usize = 8;
const ANSWER_OF_LYRICS: i64 = 0;
const ANSWER_OF_A_STUDY: i64 = 1;
const ANSWER_OF_A_RELEASE: i64 = 2;
const ANSWER_OF_BAND_PEAKS: i64 = 3;

const V1: &str = "
CREATE TABLE state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    device BLOB NOT NULL,
    space BLOB,
    cursor INTEGER NOT NULL DEFAULT 0,
    since_snapshot INTEGER NOT NULL DEFAULT 0,
    bytes_since INTEGER NOT NULL DEFAULT 0,
    listens_mark INTEGER NOT NULL DEFAULT 0,
    clock INTEGER NOT NULL DEFAULT 0,
    resumed BLOB,
    serverless INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE devices (
    device BLOB PRIMARY KEY,
    name TEXT NOT NULL,
    platform INTEGER NOT NULL,
    nanos INTEGER NOT NULL,
    stamped_by BLOB NOT NULL,
    owed INTEGER NOT NULL DEFAULT 0,
    gone INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE playlists (
    key BLOB PRIMARY KEY,
    local_id INTEGER,
    local_created INTEGER,
    name TEXT NOT NULL,
    alias TEXT,
    entries BLOB NOT NULL,
    here BLOB NOT NULL,
    gone INTEGER NOT NULL DEFAULT 0,
    nanos INTEGER NOT NULL,
    stamped_by BLOB NOT NULL,
    owed INTEGER NOT NULL DEFAULT 0,
    seen BLOB,
    keep_songs INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE UNIQUE INDEX playlists_by_local ON playlists(local_id, local_created) WHERE local_id IS NOT NULL;
CREATE TABLE favourites (
    key TEXT PRIMARY KEY,
    recording BLOB NOT NULL,
    favoured INTEGER NOT NULL,
    here INTEGER NOT NULL DEFAULT 0,
    nanos INTEGER NOT NULL,
    stamped_by BLOB NOT NULL,
    owed INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE listens (
    device BLOB NOT NULL,
    id INTEGER NOT NULL,
    recording BLOB NOT NULL,
    at INTEGER NOT NULL,
    heard INTEGER NOT NULL,
    credited INTEGER NOT NULL DEFAULT 0,
    owed INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (device, id)
) STRICT;
CREATE INDEX listens_owed ON listens(owed) WHERE owed = 1;
CREATE INDEX listens_uncredited ON listens(credited) WHERE credited = 0;
CREATE TABLE handoffs (
    device BLOB PRIMARY KEY,
    rows BLOB NOT NULL,
    at INTEGER NOT NULL,
    position INTEGER NOT NULL,
    playing INTEGER NOT NULL,
    nanos INTEGER NOT NULL,
    stamped_by BLOB NOT NULL,
    owed INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE queries (
    key BLOB PRIMARY KEY,
    local_id INTEGER,
    local_created INTEGER,
    name TEXT NOT NULL,
    alias TEXT,
    text TEXT,
    sort INTEGER NOT NULL DEFAULT 0,
    reading INTEGER NOT NULL DEFAULT 0,
    max_rows INTEGER,
    gone INTEGER NOT NULL DEFAULT 0,
    nanos INTEGER NOT NULL,
    stamped_by BLOB NOT NULL,
    owed INTEGER NOT NULL DEFAULT 0,
    seen BLOB
) STRICT;
CREATE UNIQUE INDEX queries_by_local ON queries(local_id, local_created) WHERE local_id IS NOT NULL;
";

const ANSWERS_TRAVEL: &str = "
ALTER TABLE state ADD COLUMN answers_mark INTEGER NOT NULL DEFAULT 0;
CREATE TABLE answers (
    key TEXT NOT NULL,
    kind INTEGER NOT NULL,
    file BLOB NOT NULL,
    recording BLOB NOT NULL,
    answer BLOB NOT NULL,
    here INTEGER NOT NULL DEFAULT 0,
    nanos INTEGER NOT NULL,
    stamped_by BLOB NOT NULL,
    owed INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (key, kind, file)
) STRICT;
CREATE INDEX answers_owed ON answers(owed) WHERE owed = 1;
CREATE INDEX answers_not_here ON answers(here) WHERE here = 0;
";

const SONGS_REFUSED_ARE_REMEMBERED: &str = "
CREATE TABLE refusals (
    key TEXT PRIMARY KEY,
    refused INTEGER NOT NULL
) STRICT;
";

const NAMESAKES_MADE_APART_BECOME_ONE: &str = "
ALTER TABLE playlists ADD COLUMN minted_here INTEGER NOT NULL DEFAULT 0;
ALTER TABLE playlists ADD COLUMN gone_id INTEGER;
ALTER TABLE playlists ADD COLUMN gone_created INTEGER;
ALTER TABLE queries ADD COLUMN minted_here INTEGER NOT NULL DEFAULT 0;
ALTER TABLE queries ADD COLUMN gone_id INTEGER;
ALTER TABLE queries ADD COLUMN gone_created INTEGER;
UPDATE playlists SET minted_here = 1 WHERE stamped_by = (SELECT device FROM state WHERE id = 1);
UPDATE queries SET minted_here = 1 WHERE stamped_by = (SELECT device FROM state WHERE id = 1);
CREATE TABLE playlist_folds (
    folded BLOB NOT NULL,
    kept BLOB NOT NULL,
    entries BLOB NOT NULL,
    absorbed INTEGER NOT NULL DEFAULT 0,
    owed INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (folded, kept)
) STRICT;
CREATE TABLE query_folds (
    folded BLOB NOT NULL,
    kept BLOB NOT NULL,
    owed INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (folded, kept)
) STRICT;
";

const MIGRATIONS: &[&str] = &[
    V1,
    ANSWERS_TRAVEL,
    SONGS_REFUSED_ARE_REMEMBERED,
    NAMESAKES_MADE_APART_BECOME_ONE,
];

const OWE_EVERYTHING: &str = "
UPDATE devices SET owed = 1;
UPDATE playlists SET owed = 1;
UPDATE queries SET owed = 1;
UPDATE favourites SET owed = 1;
UPDATE listens SET owed = 1;
UPDATE handoffs SET owed = 1;
UPDATE answers SET owed = 1;
UPDATE playlist_folds SET owed = 1;
UPDATE query_folds SET owed = 1;
";

const FORGET_THE_SPACE: &str = "
DELETE FROM devices;
DELETE FROM playlists;
DELETE FROM queries;
DELETE FROM favourites;
DELETE FROM listens;
DELETE FROM handoffs;
DELETE FROM answers;
DELETE FROM playlist_folds;
DELETE FROM query_folds;
UPDATE state SET cursor = 0, since_snapshot = 0, bytes_since = 0, listens_mark = 0, answers_mark = 0, resumed = NULL;
";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Me {
    pub name: String,
    pub platform: Platform,
}

impl Me {
    pub fn called(chosen: &str, host: &str, platform: Platform, alone: &str) -> Me {
        let name = match (chosen.trim(), host.trim()) {
            ("", "") => alone.to_owned(),
            ("", host) => format!("{host} ({})", platform.label()),
            (chosen, _) => chosen.to_owned(),
        };
        Me { name, platform }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Follow {
    pub favourites: bool,
    pub playlists: bool,
    pub everything: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Looked {
    pub owed: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    pub changed_here: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Looking {
    Everything,
    AtWhatIsHeld,
}

pub struct Ledger {
    connection: Connection,
    device: DeviceId,
    clock: Clock,
    answers_tried_at: Option<u64>,
}

struct AnswerSlot {
    key: RecordingKey,
    kind: i64,
    file: Vec<u8>,
}

impl AnswerSlot {
    fn slot_of(file: WholeFile) -> Vec<u8> {
        let mut slot = Vec::with_capacity(20);
        slot.extend_from_slice(&file.packets.to_le_bytes());
        slot.extend_from_slice(&file.frames.to_le_bytes());
        slot.extend_from_slice(&file.rate.to_le_bytes());
        slot
    }

    fn of(recording: &Recording, answer: &Answer) -> Self {
        let (kind, file) = match answer {
            Answer::Lyrics(_) => (ANSWER_OF_LYRICS, Vec::new()),
            Answer::Release(_) => (ANSWER_OF_A_RELEASE, Vec::new()),
            Answer::Study(study) => (ANSWER_OF_A_STUDY, Self::slot_of(study.file)),
            Answer::BandPeaks(peaks) => (ANSWER_OF_BAND_PEAKS, Self::slot_of(peaks.file)),
        };
        Self {
            key: recording.key(),
            kind,
            file,
        }
    }
}

fn listens_kept_since() -> i64 {
    wall_nanos().saturating_sub(i64::try_from(LISTENS_KEPT_FOR.as_nanos()).unwrap_or(i64::MAX))
}

struct WeighedFavourite {
    key: String,
    recording: Recording,
    favoured: bool,
    here: bool,
    stamp: Stamp,
}

#[derive(Default)]
struct WeighedFavourites {
    rows: Vec<WeighedFavourite>,
    by_key: BTreeMap<String, usize>,
    by_name: BTreeMap<RecordingKey, Vec<usize>>,
}

impl WeighedFavourites {
    fn of(rows: Vec<WeighedFavourite>) -> Self {
        let mut weighed = Self::default();
        for row in rows {
            weighed.noted(row);
        }
        weighed
    }

    fn noted(&mut self, row: WeighedFavourite) {
        let named = row.recording.named_key();
        let at = match self.by_key.get(&row.key) {
            Some(&at) => {
                self.rows[at] = row;
                at
            }
            None => {
                self.by_key.insert(row.key.clone(), self.rows.len());
                self.rows.push(row);
                self.rows.len() - 1
            }
        };
        let alike = self.by_name.entry(named).or_default();
        if !alike.contains(&at) {
            alike.push(at);
        }
    }

    fn newest_alike(&self, recording: &Recording, except: &str) -> Option<&WeighedFavourite> {
        let by_mbid = recording
            .mbid
            .as_ref()
            .and_then(|_| self.by_key.get(recording.key().as_str()));
        self.by_name
            .get(&recording.named_key())
            .into_iter()
            .flatten()
            .chain(by_mbid)
            .map(|at| &self.rows[*at])
            .filter(|row| row.key != except && row.recording.names_the_same_song_as(recording))
            .max_by_key(|row| row.stamp)
    }

    fn outranks(&self, recording: &Recording, key: &str, favoured: bool, stamp: Stamp) -> bool {
        self.newest_alike(recording, key)
            .is_some_and(|row| row.stamp > stamp && row.favoured != favoured)
    }

    fn standing(&self) -> impl Iterator<Item = &WeighedFavourite> {
        self.rows.iter().filter(|row| {
            row.favoured && !self.outranks(&row.recording, &row.key, row.favoured, row.stamp)
        })
    }
}

struct HeldFavourite {
    key: String,
    recording: Option<Recording>,
    favoured: bool,
    here: bool,
}

#[derive(Clone)]
struct StoredPlaylist {
    key: PlaylistKey,
    local: Option<LocalPlaylistId>,
    name: String,
    alias: Option<String>,
    entries: Vec<Recording>,
    here: Vec<bool>,
    gone: bool,
    stamp: Stamp,
    seen: Option<Vec<u8>>,
    minted_here: bool,
    gone_from: Option<LocalPlaylistId>,
    keep_songs: bool,
}

#[derive(Clone)]
struct StoredQuery {
    key: PlaylistKey,
    local: Option<LocalPlaylistId>,
    body: QueryBody,
    alias: Option<String>,
    gone: bool,
    stamp: Stamp,
    seen: Option<Vec<u8>>,
    minted_here: bool,
    gone_from: Option<LocalPlaylistId>,
}

struct PlaylistFold {
    folded: PlaylistKey,
    into: PlaylistKey,
    entries: Vec<Recording>,
}

impl Ledger {
    pub fn open(path: &Path) -> Result<Self> {
        let connection =
            Connection::open(path).map_err(|source| Error::store(StoreOp::Open, source))?;
        Self::over(connection)
    }

    pub fn in_memory() -> Result<Self> {
        let connection =
            Connection::open_in_memory().map_err(|source| Error::store(StoreOp::Open, source))?;
        Self::over(connection)
    }

    fn over(mut connection: Connection) -> Result<Self> {
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|source| Error::store(StoreOp::Open, source))?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(|source| Error::store(StoreOp::Open, source))?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| Error::store(StoreOp::Migrate, source))?;
        let version: u32 = transaction
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|source| Error::store(StoreOp::Migrate, source))?;
        let laid = usize::try_from(version).unwrap_or(usize::MAX);
        if laid > MIGRATIONS.len() {
            return Err(Error::LedgerVersion { found: version });
        }
        for step in &MIGRATIONS[laid..] {
            transaction
                .execute_batch(step)
                .map_err(|source| Error::store(StoreOp::Migrate, source))?;
        }
        transaction
            .pragma_update(
                None,
                "user_version",
                u32::try_from(MIGRATIONS.len()).unwrap_or(u32::MAX),
            )
            .map_err(|source| Error::store(StoreOp::Migrate, source))?;

        let held: Option<(Vec<u8>, i64)> = transaction
            .query_row("SELECT device, clock FROM state WHERE id = 1", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let (device, clock) = match held {
            Some((device, clock)) => (device_from(&device), clock),
            None => {
                let device = DeviceId::minted()?;
                transaction
                    .execute(
                        "INSERT INTO state (id, device) VALUES (1, ?1)",
                        params![device.0.as_slice()],
                    )
                    .map_err(|source| Error::store(StoreOp::Write, source))?;
                (device, 0)
            }
        };
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Migrate, source))?;

        Ok(Self {
            connection,
            device,
            clock: Clock::from_last(clock),
            answers_tried_at: None,
        })
    }

    pub const fn device(&self) -> DeviceId {
        self.device
    }

    pub fn enter_space(&mut self, space: SpaceId) -> Result<bool> {
        let held: Option<Vec<u8>> = self
            .connection
            .query_row("SELECT space FROM state WHERE id = 1", [], |row| row.get(0))
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        if held.as_deref() == Some(space.0.as_slice()) {
            return Ok(false);
        }
        let transaction = self.writing()?;
        transaction
            .execute_batch(FORGET_THE_SPACE)
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        transaction
            .execute(
                "UPDATE state SET space = ?1 WHERE id = 1",
                params![space.0.as_slice()],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(true)
    }

    pub fn through_a_server(&mut self, through: bool) -> Result<bool> {
        let serverless = self.state_number("serverless")? != 0;
        if serverless != through {
            return Ok(false);
        }
        let transaction = self.writing()?;
        if through {
            transaction
                .execute_batch(OWE_EVERYTHING)
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        transaction
            .execute(
                "UPDATE state SET serverless = ?1 WHERE id = 1",
                params![!through],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(through)
    }

    pub fn owe_queries(&mut self) -> Result<usize> {
        self.connection
            .execute("UPDATE query_folds SET owed = 1", [])
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        self.connection
            .execute("UPDATE queries SET owed = 1", [])
            .map_err(|source| Error::store(StoreOp::Write, source))
    }

    pub fn space_made_anew(&mut self) -> Result<()> {
        let transaction = self.writing()?;
        transaction
            .execute_batch(OWE_EVERYTHING)
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        transaction
            .execute(
                "UPDATE state SET cursor = 0, since_snapshot = 0, bytes_since = 0 WHERE id = 1",
                [],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Write, source))
    }

    pub fn cursor(&self) -> Result<u64> {
        self.state_number("cursor")
    }

    pub fn since_snapshot(&self) -> Result<(u64, u64)> {
        Ok((
            self.state_number("since_snapshot")?,
            self.state_number("bytes_since")?,
        ))
    }

    fn state_number(&self, column: &str) -> Result<u64> {
        let value: i64 = self
            .connection
            .query_row(
                &format!("SELECT {column} FROM state WHERE id = 1"),
                [],
                |row| row.get(0),
            )
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        Ok(value.cast_unsigned())
    }

    fn writing(&mut self) -> Result<Transaction<'_>> {
        self.connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| Error::store(StoreOp::Write, source))
    }

    pub fn passed(&mut self, through: u64, bytes: usize) -> Result<()> {
        self.connection
            .execute(
                "UPDATE state SET cursor = max(cursor, ?1), since_snapshot = since_snapshot + 1, bytes_since = bytes_since + ?2 WHERE id = 1",
                params![through.cast_signed(), i64::try_from(bytes).unwrap_or(i64::MAX)],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    pub fn compacted(&mut self, through: u64) -> Result<()> {
        self.connection
            .execute(
                "UPDATE state SET cursor = max(cursor, ?1), since_snapshot = 0, bytes_since = 0 WHERE id = 1",
                params![through.cast_signed()],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    fn keep_the_clock(&self) -> Result<()> {
        self.connection
            .execute(
                "UPDATE state SET clock = ?1 WHERE id = 1",
                params![self.clock.last()],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    pub fn look(&mut self, catalog: &dyn Catalog, me: &Me) -> Result<Looked> {
        self.look_at_me(me)?;
        self.look_at_playlists(catalog, Looking::Everything)?;
        self.look_at_queries(catalog, Looking::Everything)?;
        self.look_at_favourites(catalog, Looking::Everything)?;
        self.look_at_listens(catalog)?;
        self.look_at_resumption(catalog)?;
        self.look_at_answers(catalog)?;
        self.keep_the_clock()?;
        self.age_listens()?;

        Ok(Looked {
            owed: self.owed_count()?,
        })
    }

    fn look_at_what_is_held(&mut self, catalog: &dyn Catalog) -> Result<()> {
        self.look_at_playlists(catalog, Looking::AtWhatIsHeld)?;
        self.look_at_queries(catalog, Looking::AtWhatIsHeld)?;
        self.look_at_favourites(catalog, Looking::AtWhatIsHeld)
    }

    fn look_at_me(&mut self, me: &Me) -> Result<()> {
        let held: Option<(String, i64)> = self
            .connection
            .query_row(
                "SELECT name, platform FROM devices WHERE device = ?1",
                params![self.device.0.as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        if held.is_some_and(|(name, platform)| {
            name == me.name && platform == platform_tag(me.platform)
        }) {
            return Ok(());
        }
        let stamp = self.clock.now(self.device);
        self.connection
            .execute(
                "INSERT INTO devices (device, name, platform, nanos, stamped_by, owed) VALUES (?1, ?2, ?3, ?4, ?1, 1)
                 ON CONFLICT (device) DO UPDATE SET name = ?2, platform = ?3, nanos = ?4, stamped_by = ?1, owed = 1",
                params![self.device.0.as_slice(), me.name, platform_tag(me.platform), stamp.nanos],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    fn look_at_playlists(&mut self, catalog: &dyn Catalog, looking: Looking) -> Result<()> {
        let locals = catalog.playlists().map_err(Error::Catalog)?;
        let stored = self.stored_playlists()?;
        let mut by_local = BTreeMap::new();
        for (at, playlist) in stored.iter().enumerate() {
            if let Some(local) = playlist.local {
                by_local.insert(local, at);
            }
        }

        let mut still_here = vec![false; stored.len()];
        for local in &locals {
            let digest = projection_digest(&local.name, &local.entries);
            match by_local.get(&local.at) {
                Some(&at) => {
                    still_here[at] = true;
                    let synced = &stored[at];
                    if synced.seen.as_deref() == Some(digest.as_slice()) {
                        continue;
                    }
                    self.edited_here(catalog, synced, local, &digest)?;
                }
                None if looking == Looking::AtWhatIsHeld => {}
                None => {
                    if let Some(at) = self.revivable(catalog, &stored, &still_here, local)? {
                        still_here[at] = true;
                        self.revived_here(catalog, &stored[at], local, &digest)?;
                        continue;
                    }
                    let mut adoptable = None;
                    for (at, synced) in stored.iter().enumerate() {
                        if synced.local.is_none()
                            && !synced.gone
                            && !still_here[at]
                            && (names_alike(&synced.name, &local.name)
                                || orphaned_by_a_write(catalog, synced, local)?)
                        {
                            adoptable = Some(at);
                            break;
                        }
                    }
                    if let Some(at) = adoptable {
                        still_here[at] = true;
                        self.adopted_here(catalog, &stored[at], local, &digest)?;
                        continue;
                    }
                    self.made_here(local, &digest)?;
                }
            }
        }

        for (at, synced) in stored.iter().enumerate() {
            if synced.local.is_some() && !synced.gone && !still_here[at] {
                let stamp = self.clock.now(self.device);
                let gone_from = synced.local;
                self.connection
                    .execute(
                        "UPDATE playlists SET gone = 1, local_id = NULL, local_created = NULL, nanos = ?2, stamped_by = ?3, owed = 1, seen = NULL,
                             gone_id = ?4, gone_created = ?5 WHERE key = ?1",
                        params![
                            synced.key.0.as_slice(),
                            stamp.nanos,
                            self.device.0.as_slice(),
                            gone_from.map(|at| at.id),
                            gone_from.map(|at| at.created)
                        ],
                    )
                    .map_err(|source| Error::store(StoreOp::Write, source))?;
            }
        }

        if looking == Looking::Everything {
            for (at, synced) in stored.iter().enumerate() {
                if synced.local.is_none() && !synced.gone && !still_here[at] {
                    self.unfinished_write_finished(catalog, synced)?;
                }
            }
        }
        Ok(())
    }

    fn revivable(
        &self,
        catalog: &dyn Catalog,
        stored: &[StoredPlaylist],
        still_here: &[bool],
        local: &LocalPlaylist,
    ) -> Result<Option<usize>> {
        if let Some(at) = stored.iter().enumerate().position(|(at, synced)| {
            synced.gone && !still_here[at] && synced.gone_from == Some(local.at)
        }) {
            return Ok(Some(at));
        }
        let folds = self.playlist_folds()?;
        let listed = local.entries.iter().flatten().collect::<Vec<_>>();
        for (at, synced) in stored.iter().enumerate() {
            if !synced.gone
                || still_here[at]
                || synced.gone_from.is_some()
                || synced.stamp.device != self.device
                || synced.name != local.name
                || folds.folds_away(synced.key).is_some()
                || synced.here.iter().filter(|here| **here).count() != listed.len()
            {
                continue;
            }
            let held = synced
                .entries
                .iter()
                .zip(&synced.here)
                .filter(|(_, here)| **here)
                .map(|(recording, _)| recording)
                .collect::<Vec<_>>();
            if paired(catalog, &held, &listed)?.iter().all(Option::is_some) {
                return Ok(Some(at));
            }
        }
        Ok(None)
    }

    fn revived_here(
        &mut self,
        catalog: &dyn Catalog,
        synced: &StoredPlaylist,
        local: &LocalPlaylist,
        digest: &[u8; DIGEST_BYTES],
    ) -> Result<()> {
        self.connection
            .execute(
                "UPDATE playlists SET gone = 0, local_id = ?2, local_created = ?3, gone_id = NULL, gone_created = NULL WHERE key = ?1",
                params![synced.key.0.as_slice(), local.at.id, local.at.created],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        let revived = StoredPlaylist {
            local: Some(local.at),
            gone: false,
            seen: None,
            ..synced.clone()
        };
        self.edited_here(catalog, &revived, local, digest)
    }

    fn unfinished_write_finished(
        &mut self,
        catalog: &dyn Catalog,
        synced: &StoredPlaylist,
    ) -> Result<()> {
        let name = synced.alias.clone().unwrap_or_else(|| synced.name.clone());
        let written = catalog
            .write_playlist(None, &name, &synced.entries)
            .map_err(Error::Catalog)?;
        self.playlist_written(catalog, synced, &synced.entries, &written)
    }

    fn playlist_written(
        &mut self,
        catalog: &dyn Catalog,
        synced: &StoredPlaylist,
        entries: &[Recording],
        written: &LocalPlaylist,
    ) -> Result<()> {
        let here = held_in(catalog, entries, written)?;
        let alias = (written.name != synced.name).then(|| written.name.clone());
        self.connection
            .execute(
                "UPDATE playlists SET local_id = ?2, local_created = ?3, alias = ?4, entries = ?5, here = ?6, seen = ?7 WHERE key = ?1",
                params![
                    synced.key.0.as_slice(),
                    written.at.id,
                    written.at.created,
                    alias,
                    encoded_recordings(entries),
                    flags(&here),
                    projection_digest(&written.name, &written.entries).as_slice()
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    fn made_here(&mut self, local: &LocalPlaylist, digest: &[u8; DIGEST_BYTES]) -> Result<()> {
        let entries = local.entries.iter().flatten().cloned().collect::<Vec<_>>();
        let here = vec![true; entries.len()];
        let stamp = self.clock.now(self.device);
        let key = PlaylistKey::minted_at(stamp)?;
        self.connection
            .execute(
                "INSERT INTO playlists (key, local_id, local_created, name, entries, here, nanos, stamped_by, owed, seen, minted_here)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9, 1)",
                params![
                    key.0.as_slice(),
                    local.at.id,
                    local.at.created,
                    local.name,
                    encoded_recordings(&entries),
                    flags(&here),
                    stamp.nanos,
                    self.device.0.as_slice(),
                    digest.as_slice()
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    fn adopted_here(
        &mut self,
        catalog: &dyn Catalog,
        synced: &StoredPlaylist,
        local: &LocalPlaylist,
        digest: &[u8; DIGEST_BYTES],
    ) -> Result<()> {
        let listed = local.entries.iter().flatten().collect::<Vec<_>>();
        let pairs = paired(catalog, &synced.entries.iter().collect::<Vec<_>>(), &listed)?;
        let mut entries = synced.entries.clone();
        let mut here = pairs.iter().map(Option::is_some).collect::<Vec<_>>();
        for (recording, _) in listed
            .iter()
            .zip(unpaired(&pairs, listed.len()))
            .filter(|(_, unpaired)| *unpaired)
        {
            entries.push((*recording).clone());
            here.push(true);
        }
        if entries.len() == synced.entries.len() {
            let alias = (local.name != synced.name).then(|| local.name.clone());
            self.connection
                .execute(
                    "UPDATE playlists SET local_id = ?2, local_created = ?3, alias = ?4, here = ?5, seen = ?6 WHERE key = ?1",
                    params![
                        synced.key.0.as_slice(),
                        local.at.id,
                        local.at.created,
                        alias,
                        flags(&here),
                        digest.as_slice()
                    ],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
            return Ok(());
        }
        let stamp = self.clock.now(self.device);
        self.connection
            .execute(
                "UPDATE playlists SET local_id = ?2, local_created = ?3, entries = ?4, here = ?5, nanos = ?6, stamped_by = ?7, owed = 1, seen = ?8 WHERE key = ?1",
                params![
                    synced.key.0.as_slice(),
                    local.at.id,
                    local.at.created,
                    encoded_recordings(&entries),
                    flags(&here),
                    stamp.nanos,
                    self.device.0.as_slice(),
                    digest.as_slice()
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    fn edited_here(
        &mut self,
        catalog: &dyn Catalog,
        synced: &StoredPlaylist,
        local: &LocalPlaylist,
        digest: &[u8; DIGEST_BYTES],
    ) -> Result<()> {
        let local_entries = as_synced(catalog, synced, local)?;
        let held = synced
            .entries
            .iter()
            .zip(&synced.here)
            .map(|(recording, here)| Synced {
                recording,
                here: *here,
            })
            .collect::<Vec<_>>();
        let entries = spliced(&local_entries, &held);
        let mut here = vec![false; entries.len()];
        let mut unmatched = local_entries.iter().collect::<Vec<_>>();
        for (entry, held) in entries.iter().zip(here.iter_mut()) {
            if let Some(at) = unmatched
                .iter()
                .position(|local| local.names_the_same_song_as(entry))
            {
                unmatched.remove(at);
                *held = true;
            }
        }

        let (name, alias) = match &synced.alias {
            Some(alias) if *alias == local.name => (synced.name.clone(), Some(alias.clone())),
            _ => (local.name.clone(), None),
        };
        let stamp = self.clock.now(self.device);
        self.connection
            .execute(
                "UPDATE playlists SET name = ?2, alias = ?3, entries = ?4, here = ?5, nanos = ?6, stamped_by = ?7, owed = 1, seen = ?8 WHERE key = ?1",
                params![
                    synced.key.0.as_slice(),
                    name,
                    alias,
                    encoded_recordings(&entries),
                    flags(&here),
                    stamp.nanos,
                    self.device.0.as_slice(),
                    digest.as_slice()
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    fn look_at_queries(&mut self, catalog: &dyn Catalog, looking: Looking) -> Result<()> {
        if !catalog.keeps_queries() {
            return Ok(());
        }
        let locals = catalog.queries().map_err(Error::Catalog)?;
        let stored = self.stored_queries()?;
        let mut by_local = BTreeMap::new();
        for (at, query) in stored.iter().enumerate() {
            if let Some(local) = query.local {
                by_local.insert(local, at);
            }
        }

        let mut still_here = vec![false; stored.len()];
        for local in &locals {
            let digest = query_digest(&local.body);
            match by_local.get(&local.at) {
                Some(&at) => {
                    still_here[at] = true;
                    let synced = &stored[at];
                    if synced.seen.as_deref() == Some(digest.as_slice()) {
                        continue;
                    }
                    self.query_edited_here(synced, local, &digest)?;
                }
                None if looking == Looking::AtWhatIsHeld => {}
                None => {
                    if let Some(at) = stored.iter().enumerate().position(|(at, synced)| {
                        synced.gone && !still_here[at] && synced.gone_from == Some(local.at)
                    }) {
                        still_here[at] = true;
                        self.query_revived_here(&stored[at], local, &digest)?;
                        continue;
                    }
                    if let Some((at, synced)) = stored.iter().enumerate().find(|(at, synced)| {
                        synced.local.is_none()
                            && !synced.gone
                            && !still_here[*at]
                            && (names_alike(&synced.body.name, &local.body.name)
                                || orphaned_by_a_query_write(synced, local))
                    }) {
                        still_here[at] = true;
                        self.query_adopted_here(catalog, synced, local)?;
                        continue;
                    }
                    self.query_made_here(local, &digest)?;
                }
            }
        }

        for (at, synced) in stored.iter().enumerate() {
            if synced.local.is_some() && !synced.gone && !still_here[at] {
                let stamp = self.clock.now(self.device);
                let gone_from = synced.local;
                self.connection
                    .execute(
                        "UPDATE queries SET gone = 1, local_id = NULL, local_created = NULL, nanos = ?2, stamped_by = ?3, owed = 1, seen = NULL,
                             gone_id = ?4, gone_created = ?5 WHERE key = ?1",
                        params![
                            synced.key.0.as_slice(),
                            stamp.nanos,
                            self.device.0.as_slice(),
                            gone_from.map(|at| at.id),
                            gone_from.map(|at| at.created)
                        ],
                    )
                    .map_err(|source| Error::store(StoreOp::Write, source))?;
            }
        }
        Ok(())
    }

    fn query_revived_here(
        &mut self,
        synced: &StoredQuery,
        local: &LocalQuery,
        digest: &[u8; DIGEST_BYTES],
    ) -> Result<()> {
        self.connection
            .execute(
                "UPDATE queries SET gone = 0, local_id = ?2, local_created = ?3, gone_id = NULL, gone_created = NULL WHERE key = ?1",
                params![synced.key.0.as_slice(), local.at.id, local.at.created],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        self.query_edited_here(synced, local, digest)
    }

    fn query_made_here(&mut self, local: &LocalQuery, digest: &[u8; DIGEST_BYTES]) -> Result<()> {
        let stamp = self.clock.now(self.device);
        let key = PlaylistKey::minted_at(stamp)?;
        self.connection
            .execute(
                "INSERT INTO queries (key, local_id, local_created, name, text, sort, reading, max_rows, nanos, stamped_by, owed, seen, minted_here)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, ?11, 1)",
                params![
                    key.0.as_slice(),
                    local.at.id,
                    local.at.created,
                    local.body.name,
                    local.body.text,
                    local.body.sort,
                    local.body.reading,
                    local.body.rows,
                    stamp.nanos,
                    self.device.0.as_slice(),
                    digest.as_slice()
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    fn query_edited_here(
        &mut self,
        synced: &StoredQuery,
        local: &LocalQuery,
        digest: &[u8; DIGEST_BYTES],
    ) -> Result<()> {
        let aliased = synced
            .alias
            .as_ref()
            .is_some_and(|alias| *alias == local.body.name);
        let (name, alias) = if aliased {
            (synced.body.name.clone(), synced.alias.clone())
        } else {
            (local.body.name.clone(), None)
        };
        let stamp = self.clock.now(self.device);
        self.connection
            .execute(
                "UPDATE queries SET name = ?2, alias = ?3, text = ?4, sort = ?5, reading = ?6, max_rows = ?7, nanos = ?8, stamped_by = ?9, owed = 1, seen = ?10 WHERE key = ?1",
                params![
                    synced.key.0.as_slice(),
                    name,
                    alias,
                    local.body.text,
                    local.body.sort,
                    local.body.reading,
                    local.body.rows,
                    stamp.nanos,
                    self.device.0.as_slice(),
                    digest.as_slice()
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    fn query_adopted_here(
        &mut self,
        catalog: &dyn Catalog,
        synced: &StoredQuery,
        local: &LocalQuery,
    ) -> Result<()> {
        let written = match catalog.write_query(Some(local.at), &synced.body) {
            Ok(written) => written,
            Err(Refused::Unwritable) => return Ok(()),
            Err(refused) => return Err(Error::Catalog(refused)),
        };
        let alias = (written.body.name != synced.body.name).then(|| written.body.name.clone());
        self.connection
            .execute(
                "UPDATE queries SET local_id = ?2, local_created = ?3, alias = ?4, seen = ?5 WHERE key = ?1",
                params![
                    synced.key.0.as_slice(),
                    written.at.id,
                    written.at.created,
                    alias,
                    query_digest(&written.body).as_slice()
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    fn look_at_favourites(&mut self, catalog: &dyn Catalog, looking: Looking) -> Result<()> {
        let favourites = catalog.favourites().map_err(Error::Catalog)?;
        let held = self.held_favourites()?;
        let mut held_by_key = BTreeMap::new();
        let mut held_by_name: BTreeMap<RecordingKey, Vec<usize>> = BTreeMap::new();
        for (at, row) in held.iter().enumerate() {
            held_by_key.insert(row.key.as_str(), at);
            if let Some(recording) = &row.recording {
                held_by_name
                    .entry(recording.named_key())
                    .or_default()
                    .push(at);
            }
        }
        let mut favoured_by_key = BTreeMap::new();
        let mut favoured_by_name: BTreeMap<RecordingKey, Vec<&Recording>> = BTreeMap::new();
        for recording in &favourites {
            favoured_by_key.insert(recording.key().as_str().to_owned(), recording);
            favoured_by_name
                .entry(recording.named_key())
                .or_default()
                .push(recording);
        }
        let named_alike = |recording: &Recording| {
            favoured_by_name
                .get(&recording.named_key())
                .is_some_and(|alike| {
                    alike
                        .iter()
                        .any(|favoured| favoured.names_the_same_song_as(recording))
                })
        };
        let unmatched = favourites.iter().any(|recording| {
            !held_by_key.contains_key(recording.key().as_str())
                && !held_by_name
                    .get(&recording.named_key())
                    .into_iter()
                    .flatten()
                    .any(|at| {
                        held[*at]
                            .recording
                            .as_ref()
                            .is_some_and(|held| held.names_the_same_song_as(recording))
                    })
        });
        let mut held_as = BTreeMap::new();
        if unmatched {
            for (at, row) in held.iter().enumerate() {
                let Some(recording) = row.recording.as_ref().filter(|recording| {
                    row.here && !favoured_by_key.contains_key(&row.key) && !named_alike(recording)
                }) else {
                    continue;
                };
                if let Some(own) = catalog.held_as(recording).map_err(Error::Catalog)? {
                    held_as.insert(at, own);
                }
            }
        }
        let held_as_one = |at: &usize, recording: &Recording| {
            held_as
                .get(at)
                .is_some_and(|own| own == recording || own.names_the_same_song_as(recording))
        };

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        for (key, recording) in &favoured_by_key {
            let alike = held_by_key
                .get(key.as_str())
                .copied()
                .into_iter()
                .chain(
                    held_by_name
                        .get(&recording.named_key())
                        .into_iter()
                        .flatten()
                        .copied()
                        .filter(|at| {
                            held[*at]
                                .recording
                                .as_ref()
                                .is_some_and(|held| held.names_the_same_song_as(recording))
                        }),
                )
                .chain(
                    held_as
                        .keys()
                        .filter(|at| held_as_one(at, recording))
                        .copied(),
                )
                .collect::<Vec<_>>();
            if alike.iter().any(|at| held[*at].here) {
                continue;
            }
            let stamp = self.clock.now(self.device);
            match alike.first().map(|at| &held[*at]) {
                Some(row) => {
                    let newly = !row.favoured;
                    transaction
                        .execute(
                            "UPDATE favourites SET favoured = 1, here = 1, nanos = CASE WHEN ?3 THEN ?2 ELSE nanos END,
                                 stamped_by = CASE WHEN ?3 THEN ?4 ELSE stamped_by END, owed = max(owed, ?3)
                              WHERE key = ?1",
                            params![row.key, stamp.nanos, newly, self.device.0.as_slice()],
                        )
                        .map_err(|source| Error::store(StoreOp::Write, source))?;
                }
                None if looking == Looking::AtWhatIsHeld => {}
                None => {
                    transaction
                        .execute(
                            "INSERT INTO favourites (key, recording, favoured, here, nanos, stamped_by, owed) VALUES (?1, ?2, 1, 1, ?3, ?4, 1)",
                            params![
                                key,
                                encoded_recording(recording),
                                stamp.nanos,
                                self.device.0.as_slice()
                            ],
                        )
                        .map_err(|source| Error::store(StoreOp::Write, source))?;
                }
            }
        }
        for (at, row) in held.iter().enumerate() {
            if !row.here || favoured_by_key.contains_key(&row.key) {
                continue;
            }
            let still_favoured = row.recording.as_ref().is_some_and(named_alike)
                || favourites.iter().any(|favoured| held_as_one(&at, favoured));
            if still_favoured {
                continue;
            }
            let stamp = self.clock.now(self.device);
            transaction
                .execute(
                    "UPDATE favourites SET favoured = 0, here = 0, nanos = ?2, stamped_by = ?3, owed = 1 WHERE key = ?1",
                    params![row.key.as_str(), stamp.nanos, self.device.0.as_slice()],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Write, source))
    }

    fn held_favourites(&self) -> Result<Vec<HeldFavourite>> {
        let mut statement = self
            .connection
            .prepare("SELECT key, recording, favoured, here FROM favourites")
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, bool>(2)?,
                    row.get::<_, bool>(3)?,
                ))
            })
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let mut held = Vec::new();
        for row in rows {
            let (key, recording, favoured, here) =
                row.map_err(|source| Error::store(StoreOp::Read, source))?;
            held.push(HeldFavourite {
                key,
                recording: decoded_recording(&recording).ok(),
                favoured,
                here,
            });
        }
        Ok(held)
    }

    fn look_at_listens(&mut self, catalog: &dyn Catalog) -> Result<()> {
        let mark = self.state_number("listens_mark")?;
        let listens = catalog
            .listens_after(mark, LISTENS_LOOKED_AT_ONCE)
            .map_err(Error::Catalog)?;
        let Some(last) = listens.iter().map(|listen| listen.id).max() else {
            return Ok(());
        };
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        for listen in &listens {
            transaction
                .execute(
                    "INSERT OR IGNORE INTO listens (device, id, recording, at, heard, credited, owed) VALUES (?1, ?2, ?3, ?4, ?5, 1, 1)",
                    params![
                        self.device.0.as_slice(),
                        listen.id.cast_signed(),
                        encoded_recording(&listen.recording),
                        listen.at,
                        millis(listen.heard)
                    ],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        transaction
            .execute(
                "UPDATE state SET listens_mark = max(listens_mark, ?1) WHERE id = 1",
                params![last.cast_signed()],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Write, source))
    }

    fn look_at_resumption(&mut self, catalog: &dyn Catalog) -> Result<()> {
        let Some(resumed) = catalog.resumption().map_err(Error::Catalog)? else {
            return Ok(());
        };
        let digest = resumed_digest(&resumed);
        let seen: Option<Vec<u8>> = self
            .connection
            .query_row("SELECT resumed FROM state WHERE id = 1", [], |row| {
                row.get(0)
            })
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        if seen.as_deref() == Some(digest.as_slice()) {
            return Ok(());
        }
        let stamp = self.clock.now(self.device);
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        transaction
            .execute(
                "INSERT INTO handoffs (device, rows, at, position, playing, nanos, stamped_by, owed) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?1, 1)
                 ON CONFLICT (device) DO UPDATE SET rows = ?2, at = ?3, position = ?4, playing = ?5, nanos = ?6, stamped_by = ?1, owed = 1",
                params![
                    self.device.0.as_slice(),
                    encoded_recordings(&resumed.rows),
                    resumed.at,
                    millis(resumed.position),
                    resumed.playing,
                    stamp.nanos
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        transaction
            .execute(
                "UPDATE state SET resumed = ?1 WHERE id = 1",
                params![digest.as_slice()],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Write, source))
    }

    fn look_at_answers(&mut self, catalog: &dyn Catalog) -> Result<()> {
        if !catalog.takes_answers() {
            return Ok(());
        }
        let mut mark = self.state_number("answers_mark")?.cast_signed();
        for _ in 0..ANSWER_PAGES_A_LOOK {
            let answered = catalog
                .answers_after(mark, ANSWERS_LOOKED_AT_ONCE)
                .map_err(Error::Catalog)?;
            let Some(last) = answered.iter().map(|answered| answered.at).max() else {
                break;
            };
            let whole_page = answered.len() >= ANSWERS_LOOKED_AT_ONCE;

            let transaction = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|source| Error::store(StoreOp::Write, source))?;
            for answered in answered.iter().filter(|answered| answered.answer.travels()) {
                let Answered {
                    recording, answer, ..
                } = answered;
                let slot = AnswerSlot::of(recording, answer);
                let encoded = encoded_answer(answer);
                let held: Option<Vec<u8>> = transaction
                    .query_row(
                        "SELECT answer FROM answers WHERE key = ?1 AND kind = ?2 AND file = ?3",
                        params![slot.key.as_str(), slot.kind, slot.file],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(|source| Error::store(StoreOp::Read, source))?;
                if held.as_deref() == Some(encoded.as_slice()) {
                    continue;
                }
                let stamp = self.clock.now(self.device);
                transaction
                    .execute(
                        "INSERT INTO answers (key, kind, file, recording, answer, here, nanos, stamped_by, owed)
                         VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7, 1)
                         ON CONFLICT (key, kind, file) DO UPDATE SET recording = ?4, answer = ?5, here = 1,
                             nanos = ?6, stamped_by = ?7, owed = 1",
                        params![
                            slot.key.as_str(),
                            slot.kind,
                            slot.file,
                            encoded_recording(recording),
                            encoded,
                            stamp.nanos,
                            self.device.0.as_slice()
                        ],
                    )
                    .map_err(|source| Error::store(StoreOp::Write, source))?;
            }
            mark = if whole_page && last <= mark {
                mark.saturating_add(1)
            } else {
                last
            };
            transaction
                .execute(
                    "UPDATE state SET answers_mark = max(answers_mark, ?1) WHERE id = 1",
                    params![mark],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
            transaction
                .commit()
                .map_err(|source| Error::store(StoreOp::Write, source))?;
            if !whole_page {
                break;
            }
        }
        Ok(())
    }

    fn age_listens(&mut self) -> Result<()> {
        let horizon = listens_kept_since();
        self.connection
            .execute(
                "DELETE FROM listens WHERE at < ?1 AND owed = 0",
                params![horizon],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    pub fn owed_count(&self) -> Result<usize> {
        let owed: i64 = self
            .connection
            .query_row(
                "SELECT (SELECT count(*) FROM devices WHERE owed = 1) + (SELECT count(*) FROM playlists WHERE owed = 1)
                      + (SELECT count(*) FROM queries WHERE owed = 1)
                      + (SELECT count(*) FROM favourites WHERE owed = 1) + (SELECT count(*) FROM listens WHERE owed = 1)
                      + (SELECT count(*) FROM handoffs WHERE owed = 1)
                      + (SELECT count(*) FROM answers WHERE owed = 1)
                      + (SELECT count(*) FROM playlist_folds WHERE owed = 1)
                      + (SELECT count(*) FROM query_folds WHERE owed = 1)",
                [],
                |row| row.get(0),
            )
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        Ok(usize::try_from(owed).unwrap_or_default())
    }

    pub fn owed(&self) -> Result<Vec<Vec<Change>>> {
        let mut changes = self.fold_changes_where("owed = 1")?;
        changes.extend(self.device_changes_where("owed = 1")?);
        changes.extend(
            self.playlist_notes("owed = 1")?
                .into_iter()
                .map(Change::Playlist),
        );
        changes.extend(self.query_notes("owed = 1")?.into_iter().map(Change::Query));
        changes.extend(
            self.favourite_notes("owed = 1")?
                .into_iter()
                .map(Change::Favourite),
        );
        changes.extend(
            self.handoffs_where("owed = 1")?
                .into_iter()
                .map(Change::Handoff),
        );
        changes.extend(
            self.listens_where("owed = 1")?
                .into_iter()
                .map(Change::Listen),
        );
        changes.extend(
            self.answer_notes("owed = 1")?
                .into_iter()
                .map(Change::Answer),
        );
        Ok(bundled(changes))
    }

    pub fn snapshot(&self) -> Result<Vec<Vec<Change>>> {
        let mut changes = self.fold_changes_where("1")?;
        changes.extend(self.device_changes_where("1")?);
        changes.extend(self.playlist_notes("1")?.into_iter().map(Change::Playlist));
        changes.extend(self.query_notes("1")?.into_iter().map(Change::Query));
        changes.extend(
            self.favourite_notes("1")?
                .into_iter()
                .map(Change::Favourite),
        );
        changes.extend(self.handoffs_where("1")?.into_iter().map(Change::Handoff));
        changes.extend(self.listens_where("1")?.into_iter().map(Change::Listen));
        changes.extend(self.answer_notes("1")?.into_iter().map(Change::Answer));
        Ok(bundled(changes))
    }

    pub fn sent(&mut self, changes: &[Change]) -> Result<()> {
        let transaction = self.writing()?;
        for change in changes {
            let cleared = match change {
                Change::Device(note) => transaction.execute(
                    "UPDATE devices SET owed = 0 WHERE device = ?1 AND nanos = ?2",
                    params![note.device.0.as_slice(), note.stamp.nanos],
                ),
                Change::Forgotten(forgotten) => transaction.execute(
                    "UPDATE devices SET owed = 0 WHERE device = ?1 AND nanos = ?2",
                    params![forgotten.device.0.as_slice(), forgotten.stamp.nanos],
                ),
                Change::Playlist(note) => transaction.execute(
                    "UPDATE playlists SET owed = 0 WHERE key = ?1 AND nanos = ?2 AND stamped_by = ?3",
                    params![note.key.0.as_slice(), note.stamp.nanos, note.stamp.device.0.as_slice()],
                ),
                Change::Query(note) => transaction.execute(
                    "UPDATE queries SET owed = 0 WHERE key = ?1 AND nanos = ?2 AND stamped_by = ?3",
                    params![note.key.0.as_slice(), note.stamp.nanos, note.stamp.device.0.as_slice()],
                ),
                Change::Favourite(note) => transaction.execute(
                    "UPDATE favourites SET owed = 0 WHERE key = ?1 AND nanos = ?2 AND stamped_by = ?3",
                    params![
                        note.recording.key().as_str(),
                        note.stamp.nanos,
                        note.stamp.device.0.as_slice()
                    ],
                ),
                Change::Listen(listen) => transaction.execute(
                    "UPDATE listens SET owed = 0 WHERE device = ?1 AND id = ?2",
                    params![listen.device.0.as_slice(), listen.id.cast_signed()],
                ),
                Change::Handoff(handoff) => transaction.execute(
                    "UPDATE handoffs SET owed = 0 WHERE device = ?1 AND nanos = ?2",
                    params![handoff.device.0.as_slice(), handoff.stamp.nanos],
                ),
                Change::PlaylistFolded(folded) => transaction.execute(
                    "UPDATE playlist_folds SET owed = 0 WHERE folded = ?1 AND kept = ?2",
                    params![folded.folded.0.as_slice(), folded.into.0.as_slice()],
                ),
                Change::QueryFolded(folded) => transaction.execute(
                    "UPDATE query_folds SET owed = 0 WHERE folded = ?1 AND kept = ?2",
                    params![folded.folded.0.as_slice(), folded.into.0.as_slice()],
                ),
                Change::Answer(note) => {
                    let slot = AnswerSlot::of(&note.recording, &note.answer);
                    transaction.execute(
                        "UPDATE answers SET owed = 0
                          WHERE key = ?1 AND kind = ?2 AND file = ?3 AND nanos = ?4 AND stamped_by = ?5",
                        params![
                            slot.key.as_str(),
                            slot.kind,
                            slot.file,
                            note.stamp.nanos,
                            note.stamp.device.0.as_slice()
                        ],
                    )
                }
            };
            cleared.map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Write, source))
    }

    pub fn arrived(&mut self, catalog: &dyn Catalog, changes: &[Change]) -> Result<Applied> {
        self.look_at_what_is_held(catalog)?;
        let mut applied = Applied::default();
        let mut weighed = None;
        for change in changes {
            if let Some(stamp) = change.stamp() {
                self.clock.saw(stamp);
            }
            let changed = match change {
                Change::Device(note) => {
                    self.device_arrived(note)?;
                    false
                }
                Change::Forgotten(forgotten) => {
                    self.forgotten_arrived(forgotten)?;
                    false
                }
                Change::Playlist(note) => self.playlist_arrived(catalog, note)?,
                Change::Query(note) => self.query_arrived(catalog, note)?,
                Change::Favourite(note) => self.favourite_arrived(catalog, note, &mut weighed)?,
                Change::Listen(listen) => self.listen_arrived(catalog, listen)?,
                Change::Handoff(handoff) => {
                    self.handoff_arrived(handoff)?;
                    false
                }
                Change::Answer(note) => self.answer_arrived(catalog, note)?,
                Change::PlaylistFolded(folded) => {
                    self.playlist_folded(folded)?;
                    false
                }
                Change::QueryFolded(folded) => {
                    self.query_folded(folded, false)?;
                    false
                }
            };
            applied.changed_here += usize::from(changed);
        }
        applied.changed_here += self.fold_playlists(catalog)?;
        applied.changed_here += self.fold_queries(catalog)?;
        self.keep_the_clock()?;
        Ok(applied)
    }

    fn device_arrived(&mut self, note: &DeviceNote) -> Result<()> {
        if note.device == self.device {
            return Ok(());
        }
        self.connection
            .execute(
                "INSERT INTO devices (device, name, platform, nanos, stamped_by) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (device) DO UPDATE SET name = ?2, platform = ?3, nanos = ?4, stamped_by = ?5, gone = 0
                 WHERE (?4, ?5) > (devices.nanos, devices.stamped_by)",
                params![
                    note.device.0.as_slice(),
                    note.name,
                    platform_tag(note.platform),
                    note.stamp.nanos,
                    note.stamp.device.0.as_slice()
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    fn forgotten_arrived(&mut self, forgotten: &DeviceForgotten) -> Result<()> {
        if forgotten.device == self.device {
            return self.speak_up_for_myself();
        }
        let transaction = self.writing()?;
        let changed = transaction
            .execute(
                "INSERT INTO devices (device, name, platform, nanos, stamped_by, gone) VALUES (?1, '', ?2, ?3, ?4, 1)
                 ON CONFLICT (device) DO UPDATE SET nanos = ?3, stamped_by = ?4, gone = 1
                 WHERE (?3, ?4) > (devices.nanos, devices.stamped_by)",
                params![
                    forgotten.device.0.as_slice(),
                    platform_tag(Platform::Other),
                    forgotten.stamp.nanos,
                    forgotten.stamp.device.0.as_slice()
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        if changed > 0 {
            transaction
                .execute(
                    "DELETE FROM handoffs WHERE device = ?1",
                    params![forgotten.device.0.as_slice()],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Write, source))
    }

    fn speak_up_for_myself(&mut self) -> Result<()> {
        let stamp = self.clock.now(self.device);
        self.connection
            .execute(
                "UPDATE devices SET nanos = ?2, stamped_by = ?1, owed = 1, gone = 0 WHERE device = ?1",
                params![self.device.0.as_slice(), stamp.nanos],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    pub fn forget_device(&mut self, device: DeviceId) -> Result<bool> {
        if device == self.device {
            return Ok(false);
        }
        let stamp = self.clock.now(self.device);
        let transaction = self.writing()?;
        let changed = transaction
            .execute(
                "UPDATE devices SET gone = 1, nanos = ?2, stamped_by = ?3, owed = 1 WHERE device = ?1 AND gone = 0",
                params![device.0.as_slice(), stamp.nanos, stamp.device.0.as_slice()],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        if changed > 0 {
            transaction
                .execute(
                    "DELETE FROM handoffs WHERE device = ?1",
                    params![device.0.as_slice()],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        self.keep_the_clock()?;
        Ok(changed > 0)
    }

    fn handoff_arrived(&mut self, handoff: &Handoff) -> Result<()> {
        if handoff.device == self.device {
            return Ok(());
        }
        self.connection
            .execute(
                "INSERT INTO handoffs (device, rows, at, position, playing, nanos, stamped_by) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (device) DO UPDATE SET rows = ?2, at = ?3, position = ?4, playing = ?5, nanos = ?6, stamped_by = ?7
                 WHERE (?6, ?7) > (handoffs.nanos, handoffs.stamped_by)",
                params![
                    handoff.device.0.as_slice(),
                    encoded_recordings(&handoff.rows),
                    handoff.at,
                    millis(handoff.position),
                    handoff.playing,
                    handoff.stamp.nanos,
                    handoff.stamp.device.0.as_slice()
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    fn playlist_arrived(&mut self, catalog: &dyn Catalog, note: &PlaylistNote) -> Result<bool> {
        let stored = self.stored_playlist(note.key)?;
        if stored
            .as_ref()
            .is_some_and(|stored| stored.stamp >= note.stamp)
        {
            return Ok(false);
        }
        if let Some(survivor) = self.playlist_folds()?.folds_away(note.key)
            && stored.as_ref().is_none_or(|stored| stored.gone)
            && self.stored_playlist(survivor)?.is_some()
        {
            return self.folded_note_arrived(catalog, note, survivor);
        }

        let mut local = stored.as_ref().and_then(|stored| stored.local);
        let Some(body) = &note.body else {
            if let Some(at) = local {
                catalog.remove_playlist(at).map_err(Error::Catalog)?;
            }
            self.connection
                .execute(
                    "INSERT INTO playlists (key, name, entries, here, gone, nanos, stamped_by) VALUES (?1, '', ?2, ?3, 1, ?4, ?5)
                     ON CONFLICT (key) DO UPDATE SET local_id = NULL, local_created = NULL, alias = NULL, entries = ?2, here = ?3,
                         gone = 1, nanos = ?4, stamped_by = ?5, owed = 0, seen = NULL, gone_id = NULL, gone_created = NULL",
                    params![
                        note.key.0.as_slice(),
                        encoded_recordings(&[]),
                        flags(&[]),
                        note.stamp.nanos,
                        note.stamp.device.0.as_slice()
                    ],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
            return Ok(local.is_some());
        };

        let mut entries = body.entries.clone();
        let mut namesake = None;
        if local.is_none() {
            if stored.is_none() {
                self.fold_a_namesake_made_apart(note.key, body)?;
            }
            namesake = self.namesake_folded_into(note.key)?;
            if let Some(namesake) = &namesake {
                self.fold_tombstoned(namesake)?;
                local = namesake.local;
                entries = absorbed(&body.entries, &namesake.entries);
            } else {
                let pending = stored
                    .as_ref()
                    .filter(|stored| !stored.gone && stored.local.is_none());
                let mut unmapped = None;
                for playlist in catalog.playlists().map_err(Error::Catalog)? {
                    let alike = names_alike(&playlist.name, &body.name)
                        || match pending {
                            Some(pending) => orphaned_by_a_write(catalog, pending, &playlist)?,
                            None => false,
                        };
                    if alike && !self.maps_local(playlist.at)? {
                        unmapped = Some(playlist);
                        break;
                    }
                }
                if let Some(unmapped) = unmapped {
                    local = Some(unmapped.at);
                    let listed = unmapped.entries.iter().flatten().collect::<Vec<_>>();
                    let pairs = paired(catalog, &body.entries.iter().collect::<Vec<_>>(), &listed)?;
                    let left = unpaired(&pairs, listed.len());
                    for (recording, _) in listed
                        .into_iter()
                        .zip(left)
                        .filter(|(_, unpaired)| *unpaired)
                    {
                        entries.push(recording.clone());
                    }
                }
            }
        }
        let merged_locally = entries.len() != body.entries.len();

        let wanted_name = match stored.as_ref().and_then(|stored| stored.alias.clone()) {
            Some(alias)
                if stored
                    .as_ref()
                    .is_some_and(|stored| stored.name == body.name) =>
            {
                alias
            }
            _ => body.name.clone(),
        };
        if local.is_none() {
            self.connection
                .execute(
                    "INSERT INTO playlists (key, name, entries, here, gone, nanos, stamped_by) VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6)
                     ON CONFLICT (key) DO UPDATE SET local_id = NULL, local_created = NULL, name = ?2, alias = NULL, entries = ?3,
                         here = ?4, gone = 0, nanos = ?5, stamped_by = ?6, owed = 0, seen = NULL, gone_id = NULL, gone_created = NULL",
                    params![
                        note.key.0.as_slice(),
                        body.name,
                        encoded_recordings(&entries),
                        flags(&vec![false; entries.len()]),
                        note.stamp.nanos,
                        note.stamp.device.0.as_slice()
                    ],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        let written = catalog
            .write_playlist(local, &wanted_name, &entries)
            .map_err(Error::Catalog)?;
        let here = held_in(catalog, &entries, &written)?;
        let alias = (written.name != body.name).then(|| written.name.clone());
        let digest = projection_digest(&written.name, &written.entries);
        let (nanos, by, owed) = if merged_locally {
            let stamp = self.clock.now(self.device);
            (stamp.nanos, stamp.device, true)
        } else {
            (note.stamp.nanos, note.stamp.device, false)
        };
        self.connection
            .execute(
                "INSERT INTO playlists (key, local_id, local_created, name, alias, entries, here, gone, nanos, stamped_by, owed, seen)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9, ?10, ?11)
                 ON CONFLICT (key) DO UPDATE SET local_id = ?2, local_created = ?3, name = ?4, alias = ?5, entries = ?6, here = ?7,
                     gone = 0, nanos = ?8, stamped_by = ?9, owed = ?10, seen = ?11, gone_id = NULL, gone_created = NULL",
                params![
                    note.key.0.as_slice(),
                    written.at.id,
                    written.at.created,
                    body.name,
                    alias,
                    encoded_recordings(&entries),
                    flags(&here),
                    nanos,
                    by.0.as_slice(),
                    owed,
                    digest.as_slice()
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        if namesake.is_some_and(|namesake| namesake.keep_songs) {
            self.songs_kept_for(note.key)?;
        }
        Ok(true)
    }

    fn fold_a_namesake_made_apart(&mut self, key: PlaylistKey, body: &PlaylistBody) -> Result<()> {
        let folds = self.playlist_folds()?;
        let made_apart = self
            .stored_playlists_where("gone = 0 AND local_id IS NOT NULL")?
            .into_iter()
            .find(|made| {
                made.minted_here
                    && made.key != key
                    && names_alike(&made.name, &body.name)
                    && made.key.minted_after(&key)
                    && folds.folds_away(made.key).is_none()
            });
        if let Some(made) = made_apart {
            self.playlist_folded(&PlaylistFolded {
                folded: made.key,
                into: key,
                entries: made.entries,
            })?;
            self.connection
                .execute(
                    "UPDATE playlist_folds SET absorbed = 1, owed = 1 WHERE folded = ?1 AND kept = ?2",
                    params![made.key.0.as_slice(), key.0.as_slice()],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        Ok(())
    }

    fn namesake_folded_into(&self, key: PlaylistKey) -> Result<Option<StoredPlaylist>> {
        let folds = self.playlist_folds()?;
        Ok(self
            .stored_playlists_where("gone = 0 AND local_id IS NOT NULL")?
            .into_iter()
            .find(|namesake| folds.folds_away(namesake.key) == Some(key)))
    }

    fn folded_note_arrived(
        &mut self,
        catalog: &dyn Catalog,
        note: &PlaylistNote,
        survivor: PlaylistKey,
    ) -> Result<bool> {
        let changed = match &note.body {
            Some(body) => self.absorbed_into(catalog, survivor, &body.entries)?,
            None => false,
        };
        let (name, entries) = note
            .body
            .as_ref()
            .map_or((String::new(), Vec::new()), |body| {
                (body.name.clone(), body.entries.clone())
            });
        self.connection
            .execute(
                "INSERT INTO playlists (key, name, entries, here, gone, nanos, stamped_by) VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6)
                 ON CONFLICT (key) DO UPDATE SET nanos = ?5, stamped_by = ?6, owed = 0",
                params![
                    note.key.0.as_slice(),
                    name,
                    encoded_recordings(&entries),
                    flags(&vec![false; entries.len()]),
                    note.stamp.nanos,
                    note.stamp.device.0.as_slice()
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(changed)
    }

    fn fold_playlists(&mut self, catalog: &dyn Catalog) -> Result<usize> {
        let folds = self.playlist_folds()?;
        let mut changed = 0;
        for folded in self.stored_playlists_where("gone = 0")? {
            let Some(survivor) = folds.folds_away(folded.key) else {
                continue;
            };
            let Some(survivor) = self.stored_playlist(survivor)? else {
                continue;
            };
            self.folded_into(catalog, &folded, &survivor)?;
            changed += 1;
        }
        for fold in self.unabsorbed_playlist_folds()? {
            let survivor = folds.survivor_of(fold.into).unwrap_or(fold.into);
            if self.stored_playlist(survivor)?.is_none() {
                continue;
            }
            changed += usize::from(self.absorbed_into(catalog, survivor, &fold.entries)?);
            self.connection
                .execute(
                    "UPDATE playlist_folds SET absorbed = 1 WHERE folded = ?1 AND kept = ?2",
                    params![fold.folded.0.as_slice(), fold.into.0.as_slice()],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        Ok(changed)
    }

    fn folded_into(
        &mut self,
        catalog: &dyn Catalog,
        folded: &StoredPlaylist,
        survivor: &StoredPlaylist,
    ) -> Result<()> {
        if survivor.gone {
            if let Some(at) = folded.local {
                catalog.remove_playlist(at).map_err(Error::Catalog)?;
            }
            return self.fold_tombstoned(folded);
        }
        let entries = absorbed(&survivor.entries, &folded.entries);
        let grew = entries.len() > survivor.entries.len();
        let keeps_the_folded = folded.local.is_some()
            && (survivor.local.is_none() || (survivor.alias.is_some() && folded.alias.is_none()));
        self.fold_tombstoned(folded)?;
        let written_into = if keeps_the_folded {
            if let Some(at) = survivor.local {
                catalog.remove_playlist(at).map_err(Error::Catalog)?;
            }
            folded.local
        } else {
            if let Some(at) = folded.local {
                catalog.remove_playlist(at).map_err(Error::Catalog)?;
            }
            survivor.local.filter(|_| grew)
        };
        if grew || written_into.is_some() {
            self.survivor_rewritten(catalog, survivor, written_into, &entries, grew)?;
        }
        if folded.keep_songs {
            self.songs_kept_for(survivor.key)?;
        }
        Ok(())
    }

    fn absorbed_into(
        &mut self,
        catalog: &dyn Catalog,
        survivor: PlaylistKey,
        extra: &[Recording],
    ) -> Result<bool> {
        let Some(survivor) = self.stored_playlist(survivor)?.filter(|kept| !kept.gone) else {
            return Ok(false);
        };
        let entries = absorbed(&survivor.entries, extra);
        if entries.len() == survivor.entries.len() {
            return Ok(false);
        }
        self.survivor_rewritten(catalog, &survivor, survivor.local, &entries, true)?;
        Ok(true)
    }

    fn survivor_rewritten(
        &mut self,
        catalog: &dyn Catalog,
        survivor: &StoredPlaylist,
        written_into: Option<LocalPlaylistId>,
        entries: &[Recording],
        restamped: bool,
    ) -> Result<()> {
        let (local, alias, here, seen) = match written_into {
            Some(at) => {
                let name = match &survivor.alias {
                    Some(alias) if survivor.local == Some(at) => alias.clone(),
                    _ => survivor.name.clone(),
                };
                let written = catalog
                    .write_playlist(Some(at), &name, entries)
                    .map_err(Error::Catalog)?;
                (
                    Some(written.at),
                    (written.name != survivor.name).then(|| written.name.clone()),
                    held_in(catalog, entries, &written)?,
                    Some(projection_digest(&written.name, &written.entries).to_vec()),
                )
            }
            None => {
                let mut here = survivor.here.clone();
                here.resize(entries.len(), false);
                (
                    survivor.local,
                    survivor.alias.clone(),
                    here,
                    survivor.seen.clone(),
                )
            }
        };
        let stamp = restamped.then(|| self.clock.now(self.device));
        self.connection
            .execute(
                "UPDATE playlists SET local_id = ?2, local_created = ?3, alias = ?4, entries = ?5, here = ?6, seen = ?7,
                     nanos = coalesce(?8, nanos), stamped_by = coalesce(?9, stamped_by), owed = max(owed, ?10) WHERE key = ?1",
                params![
                    survivor.key.0.as_slice(),
                    local.map(|at| at.id),
                    local.map(|at| at.created),
                    alias,
                    encoded_recordings(entries),
                    flags(&here),
                    seen,
                    stamp.map(|stamp| stamp.nanos),
                    stamp.map(|stamp| stamp.device.0.to_vec()),
                    restamped
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    fn fold_tombstoned(&mut self, folded: &StoredPlaylist) -> Result<()> {
        self.connection
            .execute(
                "UPDATE playlists SET gone = 1, local_id = NULL, local_created = NULL, alias = NULL, seen = NULL,
                     gone_id = NULL, gone_created = NULL, nanos = ?2, stamped_by = ?3, owed = 1 WHERE key = ?1",
                params![
                    folded.key.0.as_slice(),
                    folded.stamp.nanos.saturating_add(1),
                    self.device.0.as_slice()
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    fn songs_kept_for(&mut self, key: PlaylistKey) -> Result<()> {
        self.connection
            .execute(
                "UPDATE playlists SET keep_songs = 1 WHERE key = ?1",
                params![key.0.as_slice()],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    fn playlist_folded(&mut self, folded: &PlaylistFolded) -> Result<()> {
        self.connection
            .execute(
                "INSERT INTO playlist_folds (folded, kept, entries) VALUES (?1, ?2, ?3) ON CONFLICT (folded, kept) DO NOTHING",
                params![
                    folded.folded.0.as_slice(),
                    folded.into.0.as_slice(),
                    encoded_recordings(&folded.entries)
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    fn query_folded(&mut self, folded: &QueryFolded, owed: bool) -> Result<()> {
        self.connection
            .execute(
                "INSERT INTO query_folds (folded, kept, owed) VALUES (?1, ?2, ?3) ON CONFLICT (folded, kept) DO NOTHING",
                params![folded.folded.0.as_slice(), folded.into.0.as_slice(), owed],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    fn playlist_folds(&self) -> Result<Folds> {
        Ok(Folds::of(
            self.playlist_folds_where("1")?
                .into_iter()
                .map(|fold| (fold.folded, fold.into)),
        ))
    }

    fn unabsorbed_playlist_folds(&self) -> Result<Vec<PlaylistFold>> {
        self.playlist_folds_where("absorbed = 0")
    }

    fn playlist_folds_where(&self, condition: &str) -> Result<Vec<PlaylistFold>> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT folded, kept, entries FROM playlist_folds WHERE {condition}"
            ))
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            })
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let mut folds = Vec::new();
        for row in rows {
            let (folded, into, entries) =
                row.map_err(|source| Error::store(StoreOp::Read, source))?;
            folds.push(PlaylistFold {
                folded: playlist_key_from(&folded),
                into: playlist_key_from(&into),
                entries: decoded_recordings(&entries)?,
            });
        }
        Ok(folds)
    }

    fn query_folds(&self) -> Result<Folds> {
        Ok(Folds::of(
            self.query_folds_where("1")?
                .into_iter()
                .map(|fold| (fold.folded, fold.into)),
        ))
    }

    fn query_folds_where(&self, condition: &str) -> Result<Vec<QueryFolded>> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT folded, kept FROM query_folds WHERE {condition}"
            ))
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let mut folds = Vec::new();
        for row in rows {
            let (folded, into) = row.map_err(|source| Error::store(StoreOp::Read, source))?;
            folds.push(QueryFolded {
                folded: playlist_key_from(&folded),
                into: playlist_key_from(&into),
            });
        }
        Ok(folds)
    }

    fn fold_changes_where(&self, condition: &str) -> Result<Vec<Change>> {
        let mut changes = self
            .playlist_folds_where(condition)?
            .into_iter()
            .map(|fold| {
                Change::PlaylistFolded(PlaylistFolded {
                    folded: fold.folded,
                    into: fold.into,
                    entries: fold.entries,
                })
            })
            .collect::<Vec<_>>();
        changes.extend(
            self.query_folds_where(condition)?
                .into_iter()
                .map(Change::QueryFolded),
        );
        Ok(changes)
    }

    fn query_arrived(&mut self, catalog: &dyn Catalog, note: &QueryNote) -> Result<bool> {
        let stored = self.stored_query(note.key)?;
        if stored
            .as_ref()
            .is_some_and(|stored| stored.stamp >= note.stamp)
        {
            return Ok(false);
        }
        if self.query_folds()?.folds_away(note.key).is_some()
            && stored.as_ref().is_none_or(|stored| stored.gone)
        {
            self.connection
                .execute(
                    "INSERT INTO queries (key, name, gone, nanos, stamped_by) VALUES (?1, '', 1, ?2, ?3)
                     ON CONFLICT (key) DO UPDATE SET nanos = ?2, stamped_by = ?3, owed = 0",
                    params![
                        note.key.0.as_slice(),
                        note.stamp.nanos,
                        note.stamp.device.0.as_slice()
                    ],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
            return Ok(false);
        }

        let mut local = stored.as_ref().and_then(|stored| stored.local);
        let Some(body) = &note.body else {
            if let (Some(at), true) = (local, catalog.keeps_queries()) {
                catalog.remove_playlist(at).map_err(Error::Catalog)?;
            }
            self.connection
                .execute(
                    "INSERT INTO queries (key, name, gone, nanos, stamped_by) VALUES (?1, '', 1, ?2, ?3)
                     ON CONFLICT (key) DO UPDATE SET local_id = NULL, local_created = NULL, alias = NULL, text = NULL,
                         gone = 1, nanos = ?2, stamped_by = ?3, owed = 0, seen = NULL, gone_id = NULL, gone_created = NULL",
                    params![
                        note.key.0.as_slice(),
                        note.stamp.nanos,
                        note.stamp.device.0.as_slice()
                    ],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
            return Ok(local.is_some());
        };

        let mut written = None;
        if catalog.keeps_queries() {
            let mut folding = None;
            if local.is_none() {
                if stored.is_none() {
                    folding = self.query_namesake_made_apart(note.key, body)?;
                }
                if folding.is_none() {
                    folding = self
                        .query_namesake_folded_into(note.key)?
                        .map(|namesake| (namesake, false));
                }
                local = match &folding {
                    Some((namesake, _)) => namesake.local,
                    None => {
                        let pending = stored
                            .as_ref()
                            .filter(|stored| !stored.gone && stored.local.is_none());
                        let mut unmapped = None;
                        for query in catalog.queries().map_err(Error::Catalog)? {
                            let alike = names_alike(&query.body.name, &body.name)
                                || pending.is_some_and(|pending| {
                                    orphaned_by_a_query_write(pending, &query)
                                });
                            if alike && !self.maps_local_query(query.at)? {
                                unmapped = Some(query.at);
                                break;
                            }
                        }
                        unmapped
                    }
                };
            }
            let aliased = stored
                .as_ref()
                .filter(|stored| stored.body.name == body.name)
                .and_then(|stored| stored.alias.clone());
            let wanted = match aliased {
                Some(alias) => QueryBody {
                    name: alias,
                    ..body.clone()
                },
                None => body.clone(),
            };
            if local.is_none() {
                self.query_pending(note, body)?;
            }
            written = match catalog.write_query(local, &wanted) {
                Ok(written) => Some(written),
                Err(Refused::Unwritable) => None,
                Err(refused) => return Err(Error::Catalog(refused)),
            };
            if written.is_some()
                && let Some((namesake, made_apart)) = folding
            {
                if made_apart {
                    self.query_folded(
                        &QueryFolded {
                            folded: namesake.key,
                            into: note.key,
                        },
                        true,
                    )?;
                }
                self.query_fold_tombstoned(&namesake)?;
            }
        }

        let alias = written
            .as_ref()
            .filter(|written| written.body.name != body.name)
            .map(|written| written.body.name.clone());
        let digest = written.as_ref().map(|written| query_digest(&written.body));
        self.connection
            .execute(
                "INSERT INTO queries (key, local_id, local_created, name, alias, text, sort, reading, max_rows, gone, nanos, stamped_by, owed, seen)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?11, 0, ?12)
                 ON CONFLICT (key) DO UPDATE SET local_id = ?2, local_created = ?3, name = ?4, alias = ?5, text = ?6, sort = ?7,
                     reading = ?8, max_rows = ?9, gone = 0, nanos = ?10, stamped_by = ?11, owed = 0, seen = ?12,
                     gone_id = NULL, gone_created = NULL",
                params![
                    note.key.0.as_slice(),
                    written.as_ref().map(|written| written.at.id),
                    written.as_ref().map(|written| written.at.created),
                    body.name,
                    alias,
                    body.text,
                    body.sort,
                    body.reading,
                    body.rows,
                    note.stamp.nanos,
                    note.stamp.device.0.as_slice(),
                    digest.as_ref().map(<[u8; DIGEST_BYTES]>::as_slice)
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(written.is_some())
    }

    fn query_pending(&mut self, note: &QueryNote, body: &QueryBody) -> Result<()> {
        self.connection
            .execute(
                "INSERT INTO queries (key, name, text, sort, reading, max_rows, gone, nanos, stamped_by) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8)
                 ON CONFLICT (key) DO UPDATE SET local_id = NULL, local_created = NULL, name = ?2, alias = NULL, text = ?3, sort = ?4,
                     reading = ?5, max_rows = ?6, gone = 0, nanos = ?7, stamped_by = ?8, owed = 0, seen = NULL,
                     gone_id = NULL, gone_created = NULL",
                params![
                    note.key.0.as_slice(),
                    body.name,
                    body.text,
                    body.sort,
                    body.reading,
                    body.rows,
                    note.stamp.nanos,
                    note.stamp.device.0.as_slice()
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    fn query_namesake_made_apart(
        &self,
        key: PlaylistKey,
        body: &QueryBody,
    ) -> Result<Option<(StoredQuery, bool)>> {
        let folds = self.query_folds()?;
        Ok(self
            .stored_queries_where("gone = 0 AND local_id IS NOT NULL")?
            .into_iter()
            .find(|made| {
                made.minted_here
                    && made.key != key
                    && names_alike(&made.body.name, &body.name)
                    && made.key.minted_after(&key)
                    && folds.folds_away(made.key).is_none()
            })
            .map(|made| (made, true)))
    }

    fn query_namesake_folded_into(&self, key: PlaylistKey) -> Result<Option<StoredQuery>> {
        let folds = self.query_folds()?;
        Ok(self
            .stored_queries_where("gone = 0 AND local_id IS NOT NULL")?
            .into_iter()
            .find(|namesake| folds.folds_away(namesake.key) == Some(key)))
    }

    fn fold_queries(&mut self, catalog: &dyn Catalog) -> Result<usize> {
        if !catalog.keeps_queries() {
            return Ok(0);
        }
        let folds = self.query_folds()?;
        let mut changed = 0;
        for folded in self.stored_queries_where("gone = 0")? {
            let Some(survivor) = folds.folds_away(folded.key) else {
                continue;
            };
            let Some(survivor) = self.stored_query(survivor)? else {
                continue;
            };
            let keeps_the_folded = !survivor.gone
                && folded.local.is_some()
                && (survivor.local.is_none()
                    || (survivor.alias.is_some() && folded.alias.is_none()));
            if keeps_the_folded {
                if let Some(at) = survivor.local {
                    catalog.remove_playlist(at).map_err(Error::Catalog)?;
                }
                let written = match catalog.write_query(folded.local, &survivor.body) {
                    Ok(written) => written,
                    Err(Refused::Unwritable) => continue,
                    Err(refused) => return Err(Error::Catalog(refused)),
                };
                self.query_fold_tombstoned(&folded)?;
                self.query_written(&survivor, &written)?;
            } else {
                if let Some(at) = folded.local {
                    catalog.remove_playlist(at).map_err(Error::Catalog)?;
                }
                self.query_fold_tombstoned(&folded)?;
            }
            changed += 1;
        }
        Ok(changed)
    }

    fn query_written(&mut self, synced: &StoredQuery, written: &LocalQuery) -> Result<()> {
        let alias = (written.body.name != synced.body.name).then(|| written.body.name.clone());
        self.connection
            .execute(
                "UPDATE queries SET local_id = ?2, local_created = ?3, alias = ?4, seen = ?5 WHERE key = ?1",
                params![
                    synced.key.0.as_slice(),
                    written.at.id,
                    written.at.created,
                    alias,
                    query_digest(&written.body).as_slice()
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    fn query_fold_tombstoned(&mut self, folded: &StoredQuery) -> Result<()> {
        self.connection
            .execute(
                "UPDATE queries SET gone = 1, local_id = NULL, local_created = NULL, alias = NULL, seen = NULL,
                     gone_id = NULL, gone_created = NULL, nanos = ?2, stamped_by = ?3, owed = 1 WHERE key = ?1",
                params![
                    folded.key.0.as_slice(),
                    folded.stamp.nanos.saturating_add(1),
                    self.device.0.as_slice()
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    fn maps_local_query(&self, at: LocalPlaylistId) -> Result<bool> {
        self.connection
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM queries WHERE local_id = ?1 AND local_created = ?2)",
                params![at.id, at.created],
                |row| row.get(0),
            )
            .map_err(|source| Error::store(StoreOp::Read, source))
    }

    fn maps_local(&self, at: LocalPlaylistId) -> Result<bool> {
        self.connection
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM playlists WHERE local_id = ?1 AND local_created = ?2)",
                params![at.id, at.created],
                |row| row.get(0),
            )
            .map_err(|source| Error::store(StoreOp::Read, source))
    }

    fn favourite_arrived(
        &mut self,
        catalog: &dyn Catalog,
        note: &FavouriteNote,
        weighed: &mut Option<WeighedFavourites>,
    ) -> Result<bool> {
        let key = note.recording.key();
        let held: Option<(i64, Vec<u8>, bool)> = self
            .connection
            .query_row(
                "SELECT nanos, stamped_by, here FROM favourites WHERE key = ?1",
                params![key.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        if let Some((nanos, by, _)) = &held
            && (Stamp {
                nanos: *nanos,
                device: device_from(by),
            }) >= note.stamp
        {
            return Ok(false);
        }

        let weighed = match weighed {
            Some(weighed) => weighed,
            None => weighed.insert(self.weighed_favourites()?),
        };
        let outranked = weighed.outranks(&note.recording, key.as_str(), note.favoured, note.stamp);
        let first_meeting = held.is_none();
        let here = if !outranked && (note.favoured || !first_meeting) {
            catalog
                .favour(&note.recording, note.favoured)
                .map_err(Error::Catalog)?
                && note.favoured
        } else {
            false
        };
        self.connection
            .execute(
                "INSERT INTO favourites (key, recording, favoured, here, nanos, stamped_by) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (key) DO UPDATE SET recording = ?2, favoured = ?3, here = ?4, nanos = ?5, stamped_by = ?6, owed = 0",
                params![
                    key.as_str(),
                    encoded_recording(&note.recording),
                    note.favoured,
                    here,
                    note.stamp.nanos,
                    note.stamp.device.0.as_slice()
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        weighed.noted(WeighedFavourite {
            key: key.as_str().to_owned(),
            recording: note.recording.clone(),
            favoured: note.favoured,
            here,
            stamp: note.stamp,
        });
        Ok(!outranked && (here || !note.favoured))
    }

    fn weighed_favourites(&self) -> Result<WeighedFavourites> {
        let mut statement = self
            .connection
            .prepare("SELECT key, recording, favoured, here, nanos, stamped_by FROM favourites")
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, bool>(2)?,
                    row.get::<_, bool>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                ))
            })
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let mut weighed = Vec::new();
        for row in rows {
            let (key, recording, favoured, here, nanos, by) =
                row.map_err(|source| Error::store(StoreOp::Read, source))?;
            let Ok(recording) = decoded_recording(&recording) else {
                continue;
            };
            weighed.push(WeighedFavourite {
                key,
                recording,
                favoured,
                here,
                stamp: Stamp {
                    nanos,
                    device: device_from(&by),
                },
            });
        }
        Ok(WeighedFavourites::of(weighed))
    }

    fn listen_arrived(&mut self, catalog: &dyn Catalog, listen: &Listen) -> Result<bool> {
        if listen.device == self.device || listen.at < listens_kept_since() {
            return Ok(false);
        }
        let inserted = self
            .connection
            .execute(
                "INSERT OR IGNORE INTO listens (device, id, recording, at, heard) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    listen.device.0.as_slice(),
                    listen.id.cast_signed(),
                    encoded_recording(&listen.recording),
                    listen.at,
                    millis(listen.heard)
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        if inserted == 0 {
            return Ok(false);
        }
        let credited = catalog
            .credit(&RemoteListen {
                from: listen.device,
                id: listen.id,
                recording: listen.recording.clone(),
                at: listen.at,
                heard: listen.heard,
            })
            .map_err(Error::Catalog)?;
        if credited {
            self.connection
                .execute(
                    "UPDATE listens SET credited = 1 WHERE device = ?1 AND id = ?2",
                    params![listen.device.0.as_slice(), listen.id.cast_signed()],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        Ok(credited)
    }

    fn answer_arrived(&mut self, catalog: &dyn Catalog, note: &AnswerNote) -> Result<bool> {
        if !note.answer.travels() {
            return Ok(false);
        }
        let slot = AnswerSlot::of(&note.recording, &note.answer);
        let encoded = encoded_answer(&note.answer);
        let held: Option<(Vec<u8>, i64, Vec<u8>, bool)> = self
            .connection
            .query_row(
                "SELECT answer, nanos, stamped_by, here FROM answers WHERE key = ?1 AND kind = ?2 AND file = ?3",
                params![slot.key.as_str(), slot.kind, slot.file],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        if held
            .as_ref()
            .is_some_and(|(answer, _, _, here)| *answer == encoded && *here)
        {
            return Ok(false);
        }

        let taken = catalog.takes_answers()
            && catalog
                .take_answer(&note.recording, &note.answer)
                .map_err(Error::Catalog)?;
        let newer = held.as_ref().is_none_or(|(_, nanos, by, _)| {
            Stamp {
                nanos: *nanos,
                device: device_from(by),
            } < note.stamp
        });
        if newer {
            self.connection
                .execute(
                    "INSERT INTO answers (key, kind, file, recording, answer, here, nanos, stamped_by)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT (key, kind, file) DO UPDATE SET recording = ?4, answer = ?5, here = ?6,
                         nanos = ?7, stamped_by = ?8, owed = 0",
                    params![
                        slot.key.as_str(),
                        slot.kind,
                        slot.file,
                        encoded_recording(&note.recording),
                        encoded,
                        taken,
                        note.stamp.nanos,
                        note.stamp.device.0.as_slice()
                    ],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        Ok(taken)
    }

    fn settle_answers(&mut self, catalog: &dyn Catalog) -> Result<usize> {
        if !catalog.takes_answers() {
            return Ok(0);
        }
        let revision = catalog.revision();
        if self.answers_tried_at == Some(revision) {
            return Ok(0);
        }

        let mut settled = 0;
        for note in self.answer_notes("here = 0")? {
            if !catalog
                .take_answer(&note.recording, &note.answer)
                .map_err(Error::Catalog)?
            {
                continue;
            }
            let slot = AnswerSlot::of(&note.recording, &note.answer);
            self.connection
                .execute(
                    "UPDATE answers SET here = 1 WHERE key = ?1 AND kind = ?2 AND file = ?3",
                    params![slot.key.as_str(), slot.kind, slot.file],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
            settled += 1;
        }
        self.answers_tried_at = Some(catalog.revision());
        Ok(settled)
    }

    fn answer_notes(&self, condition: &str) -> Result<Vec<AnswerNote>> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT recording, answer, nanos, stamped_by FROM answers WHERE {condition}"
            ))
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                ))
            })
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let mut notes = Vec::new();
        for row in rows {
            let (recording, answer, nanos, by) =
                row.map_err(|source| Error::store(StoreOp::Read, source))?;
            let Some(answer) = decoded_answer(&answer)? else {
                continue;
            };
            notes.push(AnswerNote {
                recording: decoded_recording(&recording)?,
                stamp: Stamp {
                    nanos,
                    device: device_from(&by),
                },
                answer,
            });
        }
        Ok(notes)
    }

    pub fn settle(&mut self, catalog: &dyn Catalog) -> Result<usize> {
        let mut settled = 0;

        let mut locals = None;
        for stored in self.stored_playlists()? {
            let Some(local) = stored.local else {
                continue;
            };
            if stored.gone || stored.here.iter().all(|here| *here) {
                continue;
            }
            let newly = stored
                .entries
                .iter()
                .zip(&stored.here)
                .filter(|(_, here)| !**here)
                .map(|(recording, _)| catalog.holds(recording))
                .collect::<std::result::Result<Vec<_>, Refused>>()
                .map_err(Error::Catalog)?;
            if !newly.iter().any(|held| *held) {
                continue;
            }
            if locals.is_none() {
                locals = Some(catalog.playlists().map_err(Error::Catalog)?);
            }
            let unlooked = locals
                .iter()
                .flatten()
                .find(|playlist| playlist.at == local)
                .is_none_or(|playlist| {
                    stored.seen.as_deref()
                        != Some(projection_digest(&playlist.name, &playlist.entries).as_slice())
                });
            if unlooked {
                continue;
            }
            let name = stored.alias.clone().unwrap_or_else(|| stored.name.clone());
            let written = catalog
                .write_playlist(Some(local), &name, &stored.entries)
                .map_err(Error::Catalog)?;
            let here = held_in(catalog, &stored.entries, &written)?;
            let digest = projection_digest(&written.name, &written.entries);
            self.connection
                .execute(
                    "UPDATE playlists SET local_id = ?2, local_created = ?3, here = ?4, seen = ?5 WHERE key = ?1",
                    params![
                        stored.key.0.as_slice(),
                        written.at.id,
                        written.at.created,
                        flags(&here),
                        digest.as_slice()
                    ],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
            settled += 1;
        }

        settled += self.settle_queries(catalog)?;

        for (key, recording) in self.favourites_not_here()? {
            if catalog.favour(&recording, true).map_err(Error::Catalog)? {
                self.connection
                    .execute(
                        "UPDATE favourites SET here = 1 WHERE key = ?1",
                        params![key],
                    )
                    .map_err(|source| Error::store(StoreOp::Write, source))?;
                settled += 1;
            }
        }

        for (device, id, listen) in self.uncredited_listens()? {
            if catalog.credit(&listen).map_err(Error::Catalog)? {
                self.connection
                    .execute(
                        "UPDATE listens SET credited = 1 WHERE device = ?1 AND id = ?2",
                        params![device, id],
                    )
                    .map_err(|source| Error::store(StoreOp::Write, source))?;
                settled += 1;
            }
        }

        settled += self.settle_answers(catalog)?;
        Ok(settled)
    }

    fn settle_queries(&mut self, catalog: &dyn Catalog) -> Result<usize> {
        if !catalog.keeps_queries() {
            return Ok(0);
        }
        let mut settled = self.fold_queries(catalog)?;
        let unwritten = self.stored_queries_where("local_id IS NULL AND gone = 0")?;
        if unwritten.is_empty() {
            return Ok(settled);
        }

        let folds = self.query_folds()?;
        let mut locals = catalog.queries().map_err(Error::Catalog)?;
        for stored in unwritten {
            if folds.folds_away(stored.key).is_some() {
                continue;
            }
            let mut alike = None;
            for (at, local) in locals.iter().enumerate() {
                if (names_alike(&local.body.name, &stored.body.name)
                    || orphaned_by_a_query_write(&stored, local))
                    && !self.maps_local_query(local.at)?
                {
                    alike = Some(at);
                    break;
                }
            }
            let mut adopted = alike.map(|at| locals.remove(at).at);
            let made_unseen = match adopted {
                Some(_) => None,
                None => self.made_beside_an_unwritten(&stored, &folds)?,
            };
            if let Some(made) = &made_unseen {
                adopted = made.local;
            }
            let wanted = QueryBody {
                name: stored
                    .alias
                    .clone()
                    .unwrap_or_else(|| stored.body.name.clone()),
                ..stored.body.clone()
            };
            let written = match catalog.write_query(adopted, &wanted) {
                Ok(written) => written,
                Err(Refused::Unwritable) => continue,
                Err(refused) => return Err(Error::Catalog(refused)),
            };
            if let Some(made) = &made_unseen {
                self.query_folded(
                    &QueryFolded {
                        folded: made.key,
                        into: stored.key,
                    },
                    true,
                )?;
                self.query_fold_tombstoned(made)?;
            }
            self.query_written(&stored, &written)?;
            settled += 1;
        }
        Ok(settled)
    }

    fn made_beside_an_unwritten(
        &self,
        unwritten: &StoredQuery,
        folds: &Folds,
    ) -> Result<Option<StoredQuery>> {
        Ok(self
            .stored_queries_where("gone = 0 AND local_id IS NOT NULL")?
            .into_iter()
            .find(|made| {
                made.minted_here
                    && made.key != unwritten.key
                    && names_alike(&made.body.name, &unwritten.body.name)
                    && folds.survivor_of(made.key).is_none()
            }))
    }

    fn favoured(&self) -> Result<Vec<Recording>> {
        Ok(self
            .weighed_favourites()?
            .standing()
            .map(|row| row.recording.clone())
            .collect())
    }

    fn favourites_not_here(&self) -> Result<Vec<(String, Recording)>> {
        Ok(self
            .weighed_favourites()?
            .standing()
            .filter(|row| !row.here)
            .map(|row| (row.key.clone(), row.recording.clone()))
            .collect())
    }

    fn uncredited_listens(&self) -> Result<Vec<(Vec<u8>, i64, RemoteListen)>> {
        let mut statement = self
            .connection
            .prepare("SELECT device, id, recording, at, heard FROM listens WHERE credited = 0")
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let mut listens = Vec::new();
        for row in rows {
            let (device, id, recording, at, heard) =
                row.map_err(|source| Error::store(StoreOp::Read, source))?;
            let from = device_from(&device);
            listens.push((
                device,
                id,
                RemoteListen {
                    from,
                    id: id.cast_unsigned(),
                    recording: decoded_recording(&recording)?,
                    at,
                    heard: Duration::from_millis(heard.cast_unsigned()),
                },
            ));
        }
        Ok(listens)
    }

    pub fn wanted(&self, follow: Follow, catalog: &dyn Catalog) -> Result<Vec<Recording>> {
        let mut wanted: BTreeMap<RecordingKey, Recording> = BTreeMap::new();
        if follow.playlists || follow.everything {
            for stored in self.stored_playlists()? {
                if stored.gone || !(follow.everything || self.keeps_songs(stored.key)?) {
                    continue;
                }
                for recording in &stored.entries {
                    wanted.insert(recording.key(), recording.clone());
                }
            }
        }
        if follow.favourites || follow.everything {
            for recording in self.favoured()? {
                wanted.insert(recording.key(), recording);
            }
        }
        let mut still_wanted = Vec::with_capacity(wanted.len());
        for recording in one_recording_a_song(wanted.into_values()) {
            if lacked(catalog, &recording)? {
                still_wanted.push(recording);
            }
        }
        Ok(still_wanted)
    }

    pub fn refused(&mut self, recording: &Recording) -> Result<()> {
        let now = wall_nanos();
        for key in [recording.key(), recording.named_key()] {
            self.connection
                .execute(
                    "INSERT INTO refusals (key, refused) VALUES (?1, ?2)
                     ON CONFLICT (key) DO UPDATE SET refused = ?2",
                    params![key.as_str(), now],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        Ok(())
    }

    pub fn asked_again(&mut self, recordings: &[Recording]) -> Result<()> {
        for recording in recordings {
            for key in [recording.key(), recording.named_key()] {
                self.connection
                    .execute("DELETE FROM refusals WHERE key = ?1", params![key.as_str()])
                    .map_err(|source| Error::store(StoreOp::Write, source))?;
            }
        }
        Ok(())
    }

    pub fn forget_refusals(&mut self) -> Result<()> {
        self.connection
            .execute("DELETE FROM refusals", [])
            .map(drop)
            .map_err(|source| Error::store(StoreOp::Write, source))
    }

    pub fn not_refused_within(
        &mut self,
        recordings: Vec<Recording>,
        waits: Duration,
    ) -> Result<Vec<Recording>> {
        let waited = i64::try_from(waits.as_nanos()).unwrap_or(i64::MAX);
        self.connection
            .execute(
                "DELETE FROM refusals WHERE refused <= ?1",
                params![wall_nanos().saturating_sub(waited)],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        let mut refused = self
            .connection
            .prepare_cached("SELECT 1 FROM refusals WHERE key = ?1")
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let mut kept = Vec::with_capacity(recordings.len());
        for recording in recordings {
            let mut held_back = false;
            for key in [recording.key(), recording.named_key()] {
                held_back |= refused
                    .exists(params![key.as_str()])
                    .map_err(|source| Error::store(StoreOp::Read, source))?;
            }
            if !held_back {
                kept.push(recording);
            }
        }
        Ok(kept)
    }

    fn keeps_songs(&self, key: PlaylistKey) -> Result<bool> {
        self.connection
            .query_row(
                "SELECT keep_songs FROM playlists WHERE key = ?1",
                params![key.0.as_slice()],
                |row| row.get(0),
            )
            .map_err(|source| Error::store(StoreOp::Read, source))
    }

    pub fn keep_songs_of(&mut self, at: LocalPlaylistId, keep: bool) -> Result<bool> {
        let changed = self
            .connection
            .execute(
                "UPDATE playlists SET keep_songs = ?3 WHERE local_id = ?1 AND local_created = ?2",
                params![at.id, at.created, keep],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(changed > 0)
    }

    pub fn kept_songs_of(&self, at: LocalPlaylistId) -> Result<bool> {
        self.connection
            .query_row(
                "SELECT keep_songs FROM playlists WHERE local_id = ?1 AND local_created = ?2",
                params![at.id, at.created],
                |row| row.get(0),
            )
            .optional()
            .map(Option::unwrap_or_default)
            .map_err(|source| Error::store(StoreOp::Read, source))
    }

    pub fn missing_from(&self, at: LocalPlaylistId) -> Result<Vec<Recording>> {
        let stored = self
            .stored_playlists()?
            .into_iter()
            .find(|stored| stored.local == Some(at));
        Ok(stored
            .map(|stored| {
                stored
                    .entries
                    .into_iter()
                    .zip(stored.here)
                    .filter(|(_, here)| !here)
                    .map(|(recording, _)| recording)
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn devices(&self) -> Result<Vec<DeviceNote>> {
        self.devices_where("gone = 0")
    }

    pub fn handoffs(&self) -> Result<Vec<Handoff>> {
        self.handoffs_where("device NOT IN (SELECT device FROM devices WHERE gone = 1)")
    }

    fn device_changes_where(&self, condition: &str) -> Result<Vec<Change>> {
        let forgotten = self
            .devices_where(&format!("gone = 1 AND ({condition})"))?
            .into_iter()
            .map(|note| {
                Change::Forgotten(DeviceForgotten {
                    device: note.device,
                    stamp: note.stamp,
                })
            });
        let noted = self
            .devices_where(&format!("gone = 0 AND ({condition})"))?
            .into_iter()
            .map(Change::Device);
        Ok(noted.chain(forgotten).collect())
    }

    fn devices_where(&self, condition: &str) -> Result<Vec<DeviceNote>> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT device, name, platform, nanos, stamped_by FROM devices WHERE {condition}"
            ))
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let rows = statement
            .query_map([], |row| {
                Ok(DeviceNote {
                    device: device_from(&row.get::<_, Vec<u8>>(0)?),
                    name: row.get(1)?,
                    platform: platform_of(row.get(2)?),
                    stamp: Stamp {
                        nanos: row.get(3)?,
                        device: device_from(&row.get::<_, Vec<u8>>(4)?),
                    },
                })
            })
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|source| Error::store(StoreOp::Read, source))
    }

    fn handoffs_where(&self, condition: &str) -> Result<Vec<Handoff>> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT device, rows, at, position, playing, nanos, stamped_by FROM handoffs WHERE {condition}"
            ))
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, bool>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Vec<u8>>(6)?,
                ))
            })
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let mut handoffs = Vec::new();
        for row in rows {
            let (device, rows, at, position, playing, nanos, by) =
                row.map_err(|source| Error::store(StoreOp::Read, source))?;
            handoffs.push(Handoff {
                device: device_from(&device),
                stamp: Stamp {
                    nanos,
                    device: device_from(&by),
                },
                rows: decoded_recordings(&rows)?,
                at: u32::try_from(at).unwrap_or_default(),
                position: Duration::from_millis(position.cast_unsigned()),
                playing,
            });
        }
        Ok(handoffs)
    }

    fn playlist_notes(&self, condition: &str) -> Result<Vec<PlaylistNote>> {
        Ok(self
            .stored_playlists_where(condition)?
            .into_iter()
            .map(|stored| PlaylistNote {
                key: stored.key,
                stamp: stored.stamp,
                body: (!stored.gone).then_some(PlaylistBody {
                    name: stored.name,
                    entries: stored.entries,
                }),
            })
            .collect())
    }

    fn query_notes(&self, condition: &str) -> Result<Vec<QueryNote>> {
        Ok(self
            .stored_queries_where(condition)?
            .into_iter()
            .map(|stored| QueryNote {
                key: stored.key,
                stamp: stored.stamp,
                body: (!stored.gone).then_some(stored.body),
            })
            .collect())
    }

    fn stored_query(&self, key: PlaylistKey) -> Result<Option<StoredQuery>> {
        let mut found = self.stored_queries_query("key = ?1", params![key.0.as_slice()])?;
        Ok(found.pop())
    }

    fn stored_queries(&self) -> Result<Vec<StoredQuery>> {
        self.stored_queries_where("1")
    }

    fn stored_queries_where(&self, condition: &str) -> Result<Vec<StoredQuery>> {
        self.stored_queries_query(condition, params![])
    }

    fn stored_queries_query(
        &self,
        condition: &str,
        arguments: &[&dyn rusqlite::ToSql],
    ) -> Result<Vec<StoredQuery>> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT key, local_id, local_created, name, alias, text, sort, reading, max_rows, gone, nanos, stamped_by, seen,
                        minted_here, gone_id, gone_created
                 FROM queries WHERE {condition}"
            ))
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let rows = statement
            .query_map(arguments, |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, u8>(6)?,
                    row.get::<_, u8>(7)?,
                    row.get::<_, Option<u32>>(8)?,
                    row.get::<_, bool>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, Vec<u8>>(11)?,
                    row.get::<_, Option<Vec<u8>>>(12)?,
                    row.get::<_, bool>(13)?,
                    row.get::<_, Option<i64>>(14)?,
                    row.get::<_, Option<i64>>(15)?,
                ))
            })
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let mut queries = Vec::new();
        for row in rows {
            let (
                key,
                id,
                created,
                name,
                alias,
                text,
                sort,
                reading,
                rows,
                gone,
                nanos,
                by,
                seen,
                minted_here,
                gone_id,
                gone_created,
            ) = row.map_err(|source| Error::store(StoreOp::Read, source))?;
            queries.push(StoredQuery {
                key: playlist_key_from(&key),
                local: id
                    .zip(created)
                    .map(|(id, created)| LocalPlaylistId { id, created }),
                body: QueryBody {
                    name,
                    text,
                    sort,
                    reading,
                    rows,
                },
                alias,
                gone,
                stamp: Stamp {
                    nanos,
                    device: device_from(&by),
                },
                seen,
                minted_here,
                gone_from: local_of(gone_id, gone_created),
            });
        }
        Ok(queries)
    }

    fn favourite_notes(&self, condition: &str) -> Result<Vec<FavouriteNote>> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT recording, favoured, nanos, stamped_by FROM favourites WHERE {condition}"
            ))
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, bool>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                ))
            })
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let mut notes = Vec::new();
        for row in rows {
            let (recording, favoured, nanos, by) =
                row.map_err(|source| Error::store(StoreOp::Read, source))?;
            notes.push(FavouriteNote {
                recording: decoded_recording(&recording)?,
                favoured,
                stamp: Stamp {
                    nanos,
                    device: device_from(&by),
                },
            });
        }
        Ok(notes)
    }

    fn listens_where(&self, condition: &str) -> Result<Vec<Listen>> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT device, id, recording, at, heard FROM listens WHERE {condition} ORDER BY at"
            ))
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let mut listens = Vec::new();
        for row in rows {
            let (device, id, recording, at, heard) =
                row.map_err(|source| Error::store(StoreOp::Read, source))?;
            listens.push(Listen {
                device: device_from(&device),
                id: id.cast_unsigned(),
                recording: decoded_recording(&recording)?,
                at,
                heard: Duration::from_millis(heard.cast_unsigned()),
            });
        }
        Ok(listens)
    }

    fn stored_playlist(&self, key: PlaylistKey) -> Result<Option<StoredPlaylist>> {
        let mut found = self.stored_playlists_where_key(key)?;
        Ok(found.pop())
    }

    fn stored_playlists(&self) -> Result<Vec<StoredPlaylist>> {
        self.stored_playlists_where("1")
    }

    fn stored_playlists_where_key(&self, key: PlaylistKey) -> Result<Vec<StoredPlaylist>> {
        self.stored_playlists_query("key = ?1", params![key.0.as_slice()])
    }

    fn stored_playlists_where(&self, condition: &str) -> Result<Vec<StoredPlaylist>> {
        self.stored_playlists_query(condition, params![])
    }

    fn stored_playlists_query(
        &self,
        condition: &str,
        arguments: &[&dyn rusqlite::ToSql],
    ) -> Result<Vec<StoredPlaylist>> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT key, local_id, local_created, name, alias, entries, here, gone, nanos, stamped_by, seen,
                        minted_here, gone_id, gone_created, keep_songs
                 FROM playlists WHERE {condition}"
            ))
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let rows = statement
            .query_map(arguments, |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Vec<u8>>(5)?,
                    row.get::<_, Vec<u8>>(6)?,
                    row.get::<_, bool>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, Vec<u8>>(9)?,
                    row.get::<_, Option<Vec<u8>>>(10)?,
                    row.get::<_, bool>(11)?,
                    row.get::<_, Option<i64>>(12)?,
                    row.get::<_, Option<i64>>(13)?,
                    row.get::<_, bool>(14)?,
                ))
            })
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let mut playlists = Vec::new();
        for row in rows {
            let (
                key,
                id,
                created,
                name,
                alias,
                entries,
                here,
                gone,
                nanos,
                by,
                seen,
                minted_here,
                gone_id,
                gone_created,
                keep_songs,
            ) = row.map_err(|source| Error::store(StoreOp::Read, source))?;
            let entries = decoded_recordings(&entries)?;
            let mut here = here.into_iter().map(|held| held != 0).collect::<Vec<_>>();
            here.resize(entries.len(), false);
            playlists.push(StoredPlaylist {
                key: playlist_key_from(&key),
                local: id
                    .zip(created)
                    .map(|(id, created)| LocalPlaylistId { id, created }),
                name,
                alias,
                entries,
                here,
                gone,
                stamp: Stamp {
                    nanos,
                    device: device_from(&by),
                },
                seen,
                minted_here,
                gone_from: local_of(gone_id, gone_created),
                keep_songs,
            });
        }
        Ok(playlists)
    }
}

fn held_in(
    catalog: &dyn Catalog,
    entries: &[Recording],
    written: &LocalPlaylist,
) -> Result<Vec<bool>> {
    let listed = written.entries.iter().flatten().collect::<Vec<_>>();
    Ok(
        paired(catalog, &entries.iter().collect::<Vec<_>>(), &listed)?
            .into_iter()
            .map(|pair| pair.is_some())
            .collect(),
    )
}

fn as_synced(
    catalog: &dyn Catalog,
    synced: &StoredPlaylist,
    local: &LocalPlaylist,
) -> Result<Vec<Recording>> {
    let held = synced
        .entries
        .iter()
        .zip(&synced.here)
        .filter(|(_, here)| **here)
        .map(|(recording, _)| recording)
        .collect::<Vec<_>>();
    let listed = local.entries.iter().flatten().collect::<Vec<_>>();
    let mut entries = listed
        .iter()
        .map(|recording| (*recording).clone())
        .collect::<Vec<_>>();
    for (recording, pair) in held.iter().zip(paired(catalog, &held, &listed)?) {
        if let Some(at) = pair {
            entries[at] = (*recording).clone();
        }
    }
    Ok(entries)
}

fn orphaned_by_a_write(
    catalog: &dyn Catalog,
    synced: &StoredPlaylist,
    local: &LocalPlaylist,
) -> Result<bool> {
    let wanted = synced.alias.as_deref().unwrap_or(&synced.name);
    if !named_as_a_free_alias_of(wanted, &local.name) {
        return Ok(false);
    }
    let listed = local.entries.iter().flatten().collect::<Vec<_>>();
    let pairs = paired(catalog, &synced.entries.iter().collect::<Vec<_>>(), &listed)?;
    Ok(unpaired(&pairs, listed.len())
        .iter()
        .all(|unpaired| !unpaired))
}

fn orphaned_by_a_query_write(synced: &StoredQuery, local: &LocalQuery) -> bool {
    let wanted = synced.alias.as_deref().unwrap_or(&synced.body.name);
    named_as_a_free_alias_of(wanted, &local.body.name)
        && QueryBody {
            name: local.body.name.clone(),
            ..synced.body.clone()
        } == local.body
}

fn unpaired(pairs: &[Option<usize>], listed: usize) -> Vec<bool> {
    let mut unpaired = vec![true; listed];
    for at in pairs.iter().flatten() {
        unpaired[*at] = false;
    }
    unpaired
}

fn paired(
    catalog: &dyn Catalog,
    asked: &[&Recording],
    listed: &[&Recording],
) -> Result<Vec<Option<usize>>> {
    let mut taken = vec![false; listed.len()];
    let mut pairs = Vec::with_capacity(asked.len());
    for entry in asked {
        let pair =
            (0..listed.len()).find(|at| !taken[*at] && listed[*at].names_the_same_song_as(entry));
        if let Some(at) = pair {
            taken[at] = true;
        }
        pairs.push(pair);
    }

    for (entry, pair) in asked.iter().zip(pairs.iter_mut()) {
        if pair.is_some() || taken.iter().all(|taken| *taken) {
            continue;
        }
        let Some(own) = catalog.held_as(entry).map_err(Error::Catalog)? else {
            continue;
        };
        *pair = (0..listed.len()).find(|at| {
            !taken[*at] && (*listed[*at] == own || listed[*at].names_the_same_song_as(&own))
        });
        if let Some(at) = *pair {
            taken[at] = true;
        }
    }
    Ok(pairs)
}

fn local_of(id: Option<i64>, created: Option<i64>) -> Option<LocalPlaylistId> {
    id.zip(created)
        .map(|(id, created)| LocalPlaylistId { id, created })
}

fn bundled(changes: Vec<Change>) -> Vec<Vec<Change>> {
    let mut bundles = Vec::new();
    let mut bundle = Vec::new();
    let mut bytes = 0;
    for change in changes {
        let size = encoded_change(&change).len();
        if bytes + size > BUNDLE_BYTES && !bundle.is_empty() {
            bundles.push(std::mem::take(&mut bundle));
            bytes = 0;
        }
        bytes += size;
        bundle.push(change);
    }
    if !bundle.is_empty() {
        bundles.push(bundle);
    }
    bundles
}

fn resumed_digest(resumed: &Resumed) -> [u8; DIGEST_BYTES] {
    let rows = resumed.rows.iter().cloned().map(Some).collect::<Vec<_>>();
    let shape = format!(
        "{}:{}:{}",
        resumed.at,
        resumed.position.as_secs() / 10,
        resumed.playing
    );
    projection_digest(&shape, &rows)
}

fn flags(here: &[bool]) -> Vec<u8> {
    here.iter().map(|held| u8::from(*held)).collect()
}

fn millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

fn device_from(bytes: &[u8]) -> DeviceId {
    let mut device = [0; 16];
    let length = bytes.len().min(device.len());
    device[..length].copy_from_slice(&bytes[..length]);
    DeviceId(device)
}

fn playlist_key_from(bytes: &[u8]) -> PlaylistKey {
    let mut key = [0; 16];
    let length = bytes.len().min(key.len());
    key[..length].copy_from_slice(&bytes[..length]);
    PlaylistKey(key)
}

const fn platform_tag(platform: Platform) -> i64 {
    match platform {
        Platform::Linux => 0,
        Platform::Android => 1,
        Platform::Ios => 2,
        Platform::Other => 255,
    }
}

const fn platform_of(tag: i64) -> Platform {
    match tag {
        0 => Platform::Linux,
        1 => Platform::Android,
        2 => Platform::Ios,
        _ => Platform::Other,
    }
}

pub(crate) fn lacked(catalog: &dyn Catalog, recording: &Recording) -> Result<bool> {
    if !catalog.deleted_here(recording).map_err(Error::Catalog)? {
        return catalog
            .holds(recording)
            .map(|held| !held)
            .map_err(Error::Catalog);
    }
    if catalog.holds(recording).map_err(Error::Catalog)? {
        catalog
            .forgive_deletion(recording)
            .map_err(Error::Catalog)?;
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ledger_laid_out_at_the_first_release_is_carried_forward_with_what_it_held() {
        let connection = Connection::open_in_memory().expect("a connection");
        connection
            .execute_batch(V1)
            .expect("the first release's ledger");
        connection
            .execute(
                "INSERT INTO state (id, device, listens_mark) VALUES (1, ?1, 9)",
                params![[3_u8; 16].as_slice()],
            )
            .expect("a state row");
        connection
            .pragma_update(None, "user_version", 1)
            .expect("stamped");

        let ledger = Ledger::over(connection).expect("carried forward");

        assert_eq!(ledger.device(), DeviceId([3; 16]));
        assert_eq!(ledger.state_number("listens_mark").expect("a mark"), 9);
        assert_eq!(ledger.state_number("answers_mark").expect("a mark"), 0);
        assert!(ledger.answer_notes("1").expect("answers").is_empty());
    }

    #[test]
    fn a_ledger_of_before_refusals_were_kept_gains_them_and_forgets_one_once_it_has_waited() {
        let connection = Connection::open_in_memory().expect("a connection");
        connection
            .execute_batch(&format!("{V1}{ANSWERS_TRAVEL}"))
            .expect("the ledger of before");
        connection
            .pragma_update(None, "user_version", 2)
            .expect("stamped");
        let song = Recording {
            mbid: None,
            title: "Hunter".into(),
            artist: "Björk".into(),
            album: "Homogenic".into(),
            length: None,
        };

        let mut ledger = Ledger::over(connection).expect("carried forward");
        ledger.refused(&song).expect("refused");
        let waiting = ledger
            .not_refused_within(vec![song.clone()], Duration::from_secs(60))
            .expect("read");
        let forgotten = ledger
            .not_refused_within(vec![song.clone()], Duration::ZERO)
            .expect("read");

        assert!(waiting.is_empty());
        assert_eq!(forgotten, [song]);
    }

    #[test]
    fn a_ledger_of_before_namesakes_were_folded_gains_the_folds_and_takes_what_it_stamped_as_made_here()
     {
        let connection = Connection::open_in_memory().expect("a connection");
        connection
            .execute_batch(&format!(
                "{V1}{ANSWERS_TRAVEL}{SONGS_REFUSED_ARE_REMEMBERED}"
            ))
            .expect("the ledger of before");
        connection
            .execute(
                "INSERT INTO state (id, device) VALUES (1, ?1)",
                params![[3_u8; 16].as_slice()],
            )
            .expect("a state row");
        for (key, by) in [([1_u8; 16], [3_u8; 16]), ([2_u8; 16], [4_u8; 16])] {
            connection
                .execute(
                    "INSERT INTO queries (key, name, nanos, stamped_by) VALUES (?1, 'Top', 1, ?2)",
                    params![key.as_slice(), by.as_slice()],
                )
                .expect("a query row");
        }
        connection
            .pragma_update(None, "user_version", 3)
            .expect("stamped");

        let ledger = Ledger::over(connection).expect("carried forward");
        let made_here = ledger
            .stored_queries()
            .expect("queries")
            .into_iter()
            .filter(|query| query.minted_here)
            .map(|query| query.key)
            .collect::<Vec<_>>();

        assert_eq!(made_here, [PlaylistKey([1; 16])]);
        assert!(ledger.fold_changes_where("1").expect("folds").is_empty());
    }
}
