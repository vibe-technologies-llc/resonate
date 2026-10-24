use std::{collections::BTreeMap, path::Path, time::Duration};

use resonate_core::{Recording, RecordingKey};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use crate::{
    DeviceId, Error, Result, SpaceId, StoreOp,
    catalog::{
        Catalog, LocalPlaylist, LocalPlaylistId, LocalQuery, Refused, RemoteListen, Resumed,
    },
    change::{
        Change, Clock, DeviceForgotten, DeviceNote, FavouriteNote, Handoff, Listen, Platform,
        PlaylistBody, PlaylistKey, PlaylistNote, QueryBody, QueryNote, Stamp, wall_nanos,
    },
    merge::{DIGEST_BYTES, Synced, names_alike, projection_digest, query_digest, spliced},
    wire::{
        decoded_recording, decoded_recordings, encoded_change, encoded_recording,
        encoded_recordings,
    },
};

pub const BUNDLE_BYTES: usize = 192 * 1024;
pub const LISTENS_LOOKED_AT_ONCE: usize = 2_000;
pub const LISTENS_KEPT_FOR: Duration = Duration::from_secs(400 * 24 * 60 * 60);

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

const V2: &str = "
DROP INDEX listens_uncredited;
CREATE INDEX listens_uncredited ON listens(credited) WHERE credited <> 1;
";

const MIGRATIONS: &[&str] = &[V1, V2];

const NOT_CREDITED: i64 = 0;
const CREDITED: i64 = 1;
const CREDITING: i64 = 2;

const OWE_EVERYTHING: &str = "
UPDATE devices SET owed = 1;
UPDATE playlists SET owed = 1;
UPDATE queries SET owed = 1;
UPDATE favourites SET owed = 1;
UPDATE listens SET owed = 1;
UPDATE handoffs SET owed = 1;
";

const FORGET_THE_SPACE: &str = "
DELETE FROM devices;
DELETE FROM playlists;
DELETE FROM queries;
DELETE FROM favourites;
DELETE FROM listens;
DELETE FROM handoffs;
UPDATE state SET cursor = 0, since_snapshot = 0, bytes_since = 0, listens_mark = 0, resumed = NULL;
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

pub struct Ledger {
    connection: Connection,
    device: DeviceId,
    clock: Clock,
}

fn listens_kept_since() -> i64 {
    wall_nanos().saturating_sub(i64::try_from(LISTENS_KEPT_FOR.as_nanos()).unwrap_or(i64::MAX))
}

struct HeldFavourite {
    key: String,
    recording: Option<Recording>,
    favoured: bool,
    here: bool,
}

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
}

struct StoredQuery {
    key: PlaylistKey,
    local: Option<LocalPlaylistId>,
    body: QueryBody,
    alias: Option<String>,
    gone: bool,
    stamp: Stamp,
    seen: Option<Vec<u8>>,
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
        self.look_at_playlists(catalog)?;
        self.look_at_queries(catalog)?;
        self.look_at_favourites(catalog)?;
        self.look_at_listens(catalog)?;
        self.look_at_resumption(catalog)?;
        self.keep_the_clock()?;
        self.age_listens()?;

        Ok(Looked {
            owed: self.owed_count()?,
        })
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

    fn look_at_playlists(&mut self, catalog: &dyn Catalog) -> Result<()> {
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
                    self.edited_here(synced, local, &digest)?;
                }
                None => {
                    if let Some(adopted) = stored.iter().enumerate().find(|(at, synced)| {
                        synced.local.is_none()
                            && !synced.gone
                            && !still_here[*at]
                            && names_alike(&synced.name, &local.name)
                    }) {
                        still_here[adopted.0] = true;
                        self.adopted_here(adopted.1, local, &digest)?;
                        continue;
                    }
                    self.made_here(local, &digest)?;
                }
            }
        }

        for (at, synced) in stored.iter().enumerate() {
            if synced.local.is_some() && !synced.gone && !still_here[at] {
                let stamp = self.clock.now(self.device);
                self.connection
                    .execute(
                        "UPDATE playlists SET gone = 1, local_id = NULL, local_created = NULL, nanos = ?2, stamped_by = ?3, owed = 1, seen = NULL WHERE key = ?1",
                        params![synced.key.0.as_slice(), stamp.nanos, self.device.0.as_slice()],
                    )
                    .map_err(|source| Error::store(StoreOp::Write, source))?;
            }
        }
        Ok(())
    }

    fn made_here(&mut self, local: &LocalPlaylist, digest: &[u8; DIGEST_BYTES]) -> Result<()> {
        let key = PlaylistKey::minted()?;
        let entries = local.entries.iter().flatten().cloned().collect::<Vec<_>>();
        let here = vec![true; entries.len()];
        let stamp = self.clock.now(self.device);
        self.connection
            .execute(
                "INSERT INTO playlists (key, local_id, local_created, name, entries, here, nanos, stamped_by, owed, seen)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9)",
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
        synced: &StoredPlaylist,
        local: &LocalPlaylist,
        digest: &[u8; DIGEST_BYTES],
    ) -> Result<()> {
        let mut entries = synced.entries.clone();
        let mut here = vec![false; entries.len()];
        for recording in local.entries.iter().flatten() {
            match entries
                .iter()
                .zip(here.iter_mut())
                .find(|(entry, held)| !**held && entry.names_the_same_song_as(recording))
            {
                Some((_, held)) => *held = true,
                None => {
                    entries.push(recording.clone());
                    here.push(true);
                }
            }
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
        synced: &StoredPlaylist,
        local: &LocalPlaylist,
        digest: &[u8; DIGEST_BYTES],
    ) -> Result<()> {
        let held = synced
            .entries
            .iter()
            .zip(&synced.here)
            .map(|(recording, here)| Synced {
                recording,
                here: *here,
            })
            .collect::<Vec<_>>();
        let local_entries = local.entries.iter().flatten().cloned().collect::<Vec<_>>();
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

    fn look_at_queries(&mut self, catalog: &dyn Catalog) -> Result<()> {
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
                None => {
                    if let Some((at, synced)) = stored.iter().enumerate().find(|(at, synced)| {
                        synced.local.is_none()
                            && !synced.gone
                            && !still_here[*at]
                            && names_alike(&synced.body.name, &local.body.name)
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
                self.connection
                    .execute(
                        "UPDATE queries SET gone = 1, local_id = NULL, local_created = NULL, nanos = ?2, stamped_by = ?3, owed = 1, seen = NULL WHERE key = ?1",
                        params![synced.key.0.as_slice(), stamp.nanos, self.device.0.as_slice()],
                    )
                    .map_err(|source| Error::store(StoreOp::Write, source))?;
            }
        }
        Ok(())
    }

    fn query_made_here(&mut self, local: &LocalQuery, digest: &[u8; DIGEST_BYTES]) -> Result<()> {
        let key = PlaylistKey::minted()?;
        let stamp = self.clock.now(self.device);
        self.connection
            .execute(
                "INSERT INTO queries (key, local_id, local_created, name, text, sort, reading, max_rows, nanos, stamped_by, owed, seen)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, ?11)",
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

    fn look_at_favourites(&mut self, catalog: &dyn Catalog) -> Result<()> {
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
        for row in &held {
            if !row.here || favoured_by_key.contains_key(&row.key) {
                continue;
            }
            let still_favoured = row.recording.as_ref().is_some_and(|recording| {
                favoured_by_name
                    .get(&recording.named_key())
                    .is_some_and(|alike| {
                        alike
                            .iter()
                            .any(|favoured| favoured.names_the_same_song_as(recording))
                    })
            });
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
                      + (SELECT count(*) FROM handoffs WHERE owed = 1)",
                [],
                |row| row.get(0),
            )
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        Ok(usize::try_from(owed).unwrap_or_default())
    }

    pub fn owed(&self) -> Result<Vec<Vec<Change>>> {
        let mut changes = Vec::new();
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
        Ok(bundled(changes))
    }

    pub fn snapshot(&self) -> Result<Vec<Vec<Change>>> {
        let mut changes = Vec::new();
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
            };
            cleared.map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Write, source))
    }

    pub fn arrived(&mut self, catalog: &dyn Catalog, changes: &[Change]) -> Result<Applied> {
        let mut applied = Applied::default();
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
                Change::Favourite(note) => self.favourite_arrived(catalog, note)?,
                Change::Listen(listen) => self.listen_arrived(catalog, listen)?,
                Change::Handoff(handoff) => {
                    self.handoff_arrived(handoff)?;
                    false
                }
            };
            applied.changed_here += usize::from(changed);
        }
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

        let mut local = stored.as_ref().and_then(|stored| stored.local);
        let Some(body) = &note.body else {
            if let Some(at) = local {
                catalog.remove_playlist(at).map_err(Error::Catalog)?;
            }
            self.connection
                .execute(
                    "INSERT INTO playlists (key, name, entries, here, gone, nanos, stamped_by) VALUES (?1, '', ?2, ?3, 1, ?4, ?5)
                     ON CONFLICT (key) DO UPDATE SET local_id = NULL, local_created = NULL, alias = NULL, entries = ?2, here = ?3,
                         gone = 1, nanos = ?4, stamped_by = ?5, owed = 0, seen = NULL",
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
        if local.is_none() {
            let unmapped = catalog
                .playlists()
                .map_err(Error::Catalog)?
                .into_iter()
                .filter(|playlist| names_alike(&playlist.name, &body.name))
                .find(|playlist| !self.maps_local(playlist.at).unwrap_or(true));
            if let Some(unmapped) = unmapped {
                local = Some(unmapped.at);
                for recording in unmapped.entries.iter().flatten() {
                    if !entries
                        .iter()
                        .any(|entry| entry.names_the_same_song_as(recording))
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
        let written = catalog
            .write_playlist(local, &wanted_name, &entries)
            .map_err(Error::Catalog)?;
        let here = held_in(&entries, &written);
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
                     gone = 0, nanos = ?8, stamped_by = ?9, owed = ?10, seen = ?11",
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
        Ok(true)
    }

    fn query_arrived(&mut self, catalog: &dyn Catalog, note: &QueryNote) -> Result<bool> {
        let stored = self.stored_query(note.key)?;
        if stored
            .as_ref()
            .is_some_and(|stored| stored.stamp >= note.stamp)
        {
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
                         gone = 1, nanos = ?2, stamped_by = ?3, owed = 0, seen = NULL",
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
            if local.is_none() {
                local = catalog
                    .queries()
                    .map_err(Error::Catalog)?
                    .into_iter()
                    .filter(|query| names_alike(&query.body.name, &body.name))
                    .find(|query| !self.maps_local_query(query.at).unwrap_or(true))
                    .map(|query| query.at);
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
            written = match catalog.write_query(local, &wanted) {
                Ok(written) => Some(written),
                Err(Refused::Unwritable) => None,
                Err(refused) => return Err(Error::Catalog(refused)),
            };
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
                     reading = ?8, max_rows = ?9, gone = 0, nanos = ?10, stamped_by = ?11, owed = 0, seen = ?12",
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

    fn favourite_arrived(&mut self, catalog: &dyn Catalog, note: &FavouriteNote) -> Result<bool> {
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

        let first_meeting = held.is_none();
        let here = if note.favoured || !first_meeting {
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
        Ok(here || !note.favoured)
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
        self.credit_once(
            catalog,
            listen.device.0.as_slice(),
            listen.id.cast_signed(),
            &RemoteListen {
                recording: listen.recording.clone(),
                at: listen.at,
                heard: Duration::from_millis(millis(listen.heard).cast_unsigned()),
            },
        )
    }

    fn credit_once(
        &self,
        catalog: &dyn Catalog,
        device: &[u8],
        id: i64,
        listen: &RemoteListen,
    ) -> Result<bool> {
        let before: i64 = self
            .connection
            .query_row(
                "SELECT credited FROM listens WHERE device = ?1 AND id = ?2",
                params![device, id],
                |row| row.get(0),
            )
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        if before == CREDITED {
            return Ok(true);
        }
        if before == CREDITING && catalog.credited(listen).map_err(Error::Catalog)? {
            self.mark_credited(device, id, CREDITED)?;
            return Ok(true);
        }

        self.mark_credited(device, id, CREDITING)?;
        let credited = catalog.credit(listen).map_err(Error::Catalog)?;
        self.mark_credited(device, id, if credited { CREDITED } else { NOT_CREDITED })?;
        Ok(credited)
    }

    fn mark_credited(&self, device: &[u8], id: i64, state: i64) -> Result<()> {
        self.connection
            .execute(
                "UPDATE listens SET credited = ?3 WHERE device = ?1 AND id = ?2",
                params![device, id, state],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    pub fn settle(&mut self, catalog: &dyn Catalog) -> Result<usize> {
        let mut settled = 0;

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
            let name = stored.alias.clone().unwrap_or_else(|| stored.name.clone());
            let written = catalog
                .write_playlist(Some(local), &name, &stored.entries)
                .map_err(Error::Catalog)?;
            let here = held_in(&stored.entries, &written);
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
            if self.credit_once(catalog, &device, id, &listen)? {
                settled += 1;
            }
        }
        Ok(settled)
    }

    fn favoured(&self) -> Result<Vec<Recording>> {
        Ok(self
            .favourites_where("favoured = 1")?
            .into_iter()
            .map(|(_, recording)| recording)
            .collect())
    }

    fn favourites_not_here(&self) -> Result<Vec<(String, Recording)>> {
        self.favourites_where("favoured = 1 AND here = 0")
    }

    fn favourites_where(&self, condition: &str) -> Result<Vec<(String, Recording)>> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT key, recording FROM favourites WHERE {condition}"
            ))
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let mut favourites = Vec::new();
        for row in rows {
            let (key, recording) = row.map_err(|source| Error::store(StoreOp::Read, source))?;
            favourites.push((key, decoded_recording(&recording)?));
        }
        Ok(favourites)
    }

    fn uncredited_listens(&self) -> Result<Vec<(Vec<u8>, i64, RemoteListen)>> {
        let mut statement = self
            .connection
            .prepare("SELECT device, id, recording, at, heard FROM listens WHERE credited <> 1")
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
            listens.push((
                device,
                id,
                RemoteListen {
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
        for recording in wanted.into_values() {
            if !catalog.holds(&recording).map_err(Error::Catalog)? {
                still_wanted.push(recording);
            }
        }
        Ok(still_wanted)
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
                "SELECT key, local_id, local_created, name, alias, text, sort, reading, max_rows, gone, nanos, stamped_by, seen
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
                ))
            })
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let mut queries = Vec::new();
        for row in rows {
            let (key, id, created, name, alias, text, sort, reading, rows, gone, nanos, by, seen) =
                row.map_err(|source| Error::store(StoreOp::Read, source))?;
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
                "SELECT key, local_id, local_created, name, alias, entries, here, gone, nanos, stamped_by, seen
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
                ))
            })
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let mut playlists = Vec::new();
        for row in rows {
            let (key, id, created, name, alias, entries, here, gone, nanos, by, seen) =
                row.map_err(|source| Error::store(StoreOp::Read, source))?;
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
            });
        }
        Ok(playlists)
    }
}

fn held_in(entries: &[Recording], written: &LocalPlaylist) -> Vec<bool> {
    let mut unmatched = written.entries.iter().flatten().collect::<Vec<_>>();
    entries
        .iter()
        .map(|entry| {
            match unmatched
                .iter()
                .position(|local| local.names_the_same_song_as(entry))
            {
                Some(at) => {
                    unmatched.remove(at);
                    true
                }
                None => false,
            }
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ledger_laid_out_at_its_first_step_gains_the_index_for_listens_still_being_credited() {
        let connection = Connection::open_in_memory().expect("a connection");
        connection.execute_batch(V1).expect("the first step");
        connection
            .pragma_update(None, "user_version", 1)
            .expect("a stamp");

        let ledger = Ledger::over(connection).expect("a ledger");

        let index: String = ledger
            .connection
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'listens_uncredited'",
                [],
                |row| row.get(0),
            )
            .expect("an index");
        assert!(index.ends_with("WHERE credited <> 1"));
    }
}
