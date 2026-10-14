use std::{collections::BTreeMap, path::Path, time::Duration};

use resonate_core::{Recording, RecordingKey};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use crate::{
    DeviceId, Error, Result, SpaceId, StoreOp,
    catalog::{Catalog, LocalPlaylist, LocalPlaylistId, Refused, RemoteListen, Resumed},
    change::{
        Change, Clock, DeviceNote, FavouriteNote, Handoff, Listen, Platform, PlaylistBody,
        PlaylistKey, PlaylistNote, Stamp, wall_nanos,
    },
    merge::{DIGEST_BYTES, Synced, names_alike, projection_digest, spliced},
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
    resumed BLOB
) STRICT;
CREATE TABLE devices (
    device BLOB PRIMARY KEY,
    name TEXT NOT NULL,
    platform INTEGER NOT NULL,
    nanos INTEGER NOT NULL,
    stamped_by BLOB NOT NULL,
    owed INTEGER NOT NULL DEFAULT 0
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
";

const MIGRATIONS: &[&str] = &[V1];

const FORGET_THE_SPACE: &str = "
DELETE FROM devices;
DELETE FROM playlists;
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

    fn look_at_favourites(&mut self, catalog: &dyn Catalog) -> Result<()> {
        let favourites = catalog.favourites().map_err(Error::Catalog)?;
        let mut favoured_here = BTreeMap::new();
        for recording in favourites {
            favoured_here.insert(recording.key().as_str().to_owned(), recording);
        }

        let mut held = BTreeMap::new();
        {
            let mut statement = self
                .connection
                .prepare("SELECT key, favoured, here FROM favourites")
                .map_err(|source| Error::store(StoreOp::Read, source))?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, bool>(1)?,
                        row.get::<_, bool>(2)?,
                    ))
                })
                .map_err(|source| Error::store(StoreOp::Read, source))?;
            for row in rows {
                let (key, favoured, here) =
                    row.map_err(|source| Error::store(StoreOp::Read, source))?;
                held.insert(key, (favoured, here));
            }
        }

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        for (key, recording) in &favoured_here {
            let known = held.get(key).copied();
            if known.is_some_and(|(_, here)| here) {
                continue;
            }
            let favoured_already = known.is_some_and(|(favoured, _)| favoured);
            let stamp = self.clock.now(self.device);
            transaction
                .execute(
                    "INSERT INTO favourites (key, recording, favoured, here, nanos, stamped_by, owed) VALUES (?1, ?2, 1, 1, ?3, ?4, ?5)
                     ON CONFLICT (key) DO UPDATE SET favoured = 1, here = 1, nanos = CASE WHEN ?5 THEN ?3 ELSE nanos END,
                         stamped_by = CASE WHEN ?5 THEN ?4 ELSE stamped_by END, owed = max(owed, ?5)",
                    params![
                        key,
                        encoded_recording(recording),
                        stamp.nanos,
                        self.device.0.as_slice(),
                        !favoured_already
                    ],
                )
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        for (key, (_, here)) in &held {
            if *here && !favoured_here.contains_key(key) {
                let stamp = self.clock.now(self.device);
                transaction
                    .execute(
                        "UPDATE favourites SET favoured = 0, here = 0, nanos = ?2, stamped_by = ?3, owed = 1 WHERE key = ?1",
                        params![key, stamp.nanos, self.device.0.as_slice()],
                    )
                    .map_err(|source| Error::store(StoreOp::Write, source))?;
            }
        }
        transaction
            .commit()
            .map_err(|source| Error::store(StoreOp::Write, source))
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
        let horizon = wall_nanos()
            .saturating_sub(i64::try_from(LISTENS_KEPT_FOR.as_nanos()).unwrap_or(i64::MAX));
        self.connection
            .execute(
                "DELETE FROM listens WHERE at < ?1 AND owed = 0",
                params![horizon],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    fn owed_count(&self) -> Result<usize> {
        let owed: i64 = self
            .connection
            .query_row(
                "SELECT (SELECT count(*) FROM devices WHERE owed = 1) + (SELECT count(*) FROM playlists WHERE owed = 1)
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
        changes.extend(
            self.devices_where("owed = 1")?
                .into_iter()
                .map(Change::Device),
        );
        changes.extend(
            self.playlist_notes("owed = 1")?
                .into_iter()
                .map(Change::Playlist),
        );
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
        changes.extend(self.devices_where("1")?.into_iter().map(Change::Device));
        changes.extend(self.playlist_notes("1")?.into_iter().map(Change::Playlist));
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
                Change::Playlist(note) => transaction.execute(
                    "UPDATE playlists SET owed = 0 WHERE key = ?1 AND nanos = ?2 AND stamped_by = ?3",
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
                Change::Playlist(note) => self.playlist_arrived(catalog, note)?,
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
                 ON CONFLICT (device) DO UPDATE SET name = ?2, platform = ?3, nanos = ?4, stamped_by = ?5
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
        if listen.device == self.device {
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
        Ok(settled)
    }

    fn favourites_not_here(&self) -> Result<Vec<(String, Recording)>> {
        let mut statement = self
            .connection
            .prepare("SELECT key, recording FROM favourites WHERE favoured = 1 AND here = 0")
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
                for (recording, here) in stored.entries.iter().zip(&stored.here) {
                    if !here {
                        wanted.insert(recording.key(), recording.clone());
                    }
                }
            }
        }
        if follow.favourites || follow.everything {
            for (_, recording) in self.favourites_not_here()? {
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
        self.devices_where("1")
    }

    pub fn handoffs(&self) -> Result<Vec<Handoff>> {
        self.handoffs_where("1")
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
