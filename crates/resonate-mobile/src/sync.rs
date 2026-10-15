use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use parking_lot::Mutex;
use resonate_codec::Sources;
use resonate_core::{
    AUDIO_EXTENSIONS, Frames, Mbid, Recording, Resumable, Resumption, SampleRate, TrackId,
    folded_letters, naming::named_within, title_words,
};
use resonate_engine::Command;
use resonate_sync::{
    Catalog, Follow, Handoff, Held, LocalListen, LocalPlaylist, LocalPlaylistId, Me, Nearby,
    Opened, Platform, Refused, RemoteListen, Resumed, Settings, Standing, Sync, SyncDevice,
    SyncKey, TransferState,
};
use rusqlite::{Connection, OptionalExtension, Row, params};
use unicode_normalization::UnicodeNormalization;

use crate::{
    Error, MobilePlayer, Result, StoreOp, Track, catalog,
    documents::location,
    downloads::fresh_destination,
    error::{ReceivedOp, SyncOp},
    features::SearchSong,
    playlists,
};

const LEDGER: &str = "sync.sqlite";
const STAGING: &str = "sync-staging";
const RECEIVED: &str = "sync-received";
#[cfg(not(target_os = "ios"))]
const DEVICE_NAME: &str = "Android";
#[cfg(target_os = "ios")]
const DEVICE_NAME: &str = "iPhone";
#[cfg(not(target_os = "ios"))]
const THIS_PLATFORM: Platform = Platform::Android;
#[cfg(target_os = "ios")]
const THIS_PLATFORM: Platform = Platform::Ios;
const HANDED_OFF_AT_MOST: usize = 500;
const LISTEN_SETTLES_AFTER: Duration = Duration::from_secs(15 * 60);
const THE_SAME_LENGTH_WITHIN: Duration = Duration::from_secs(2);
const PLAYLIST_NAME_BYTES_AT_MOST: usize = 255;
const UNNAMED_PLAYLIST: &str = "Synced";
const NANOS_A_SECOND: i64 = 1_000_000_000;
const RECORDING_COLUMNS: &str = "d.title, d.artist, d.album, d.recording, d.duration_ms";

type Answered<T> = std::result::Result<T, Refused>;

pub(crate) struct Syncing {
    folder: PathBuf,
    running: Mutex<Option<Running>>,
    unmetered: AtomicBool,
    arrivals: Arc<AtomicU64>,
}

struct Running {
    sync: Sync,
    metered_allowed: bool,
}

impl Syncing {
    pub(crate) fn under(folder: PathBuf) -> Self {
        Self {
            folder,
            running: Mutex::new(None),
            unmetered: AtomicBool::new(false),
            arrivals: Arc::new(AtomicU64::new(0)),
        }
    }
}

struct DocumentCatalog {
    catalog: Arc<catalog::Catalog>,
    sources: Arc<Sources>,
    received: PathBuf,
    arrivals: Arc<AtomicU64>,
    landings: AtomicU64,
    indexed: Mutex<Option<(u64, Arc<Indexed>)>>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SyncConfig {
    pub enabled: bool,
    pub server: String,
    pub key: String,
    pub device: String,
    pub favourites: bool,
    pub playlists: bool,
    pub everything: bool,
    pub metered_allowed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SyncStanding {
    Off,
    Connecting,
    Connected,
    Unreachable,
    Refused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum DevicePlatform {
    Linux,
    Android,
    Ios,
    Other,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SyncedDevice {
    pub name: String,
    pub platform: DevicePlatform,
    pub online: bool,
    pub nearby: bool,
    pub this_device: bool,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SyncStatus {
    pub standing: SyncStanding,
    pub code: String,
    pub devices: Vec<SyncedDevice>,
    pub moving: u32,
    pub owed: u32,
    pub arrivals: u64,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ReceivedSong {
    pub id: u64,
    pub name: String,
    pub extension: String,
    pub song: SearchSong,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct HandedOff {
    pub device: String,
    pub songs: u32,
    pub playing: bool,
}

#[cfg(not(target_os = "ios"))]
fn nearby() -> Nearby {
    Nearby::on_the_network()
}

#[cfg(target_os = "ios")]
const fn nearby() -> Nearby {
    Nearby::told_by_the_platform()
}

#[uniffi::export]
pub fn sync_new_code() -> Result<String> {
    SyncKey::minted()
        .map(|key| key.code())
        .map_err(|source| Error::sync(SyncOp::Mint, source))
}

#[uniffi::export]
pub fn sync_reads_as_a_code(code: String) -> bool {
    SyncKey::from_code(&code).is_ok()
}

#[uniffi::export]
impl MobilePlayer {
    pub fn sync_start(&self, config: SyncConfig) -> Result<SyncStatus> {
        let mut running = self.syncing.running.lock();
        if let Some(previous) = running.take() {
            previous.sync.leave();
        }
        if config.enabled {
            self.require_online()?;
            *running = Some(self.started(config)?);
        }
        drop(running);
        Ok(self.sync_status())
    }

    pub fn sync_stop(&self) {
        let previous = self.syncing.running.lock().take();
        if let Some(previous) = previous {
            previous.sync.leave();
        }
    }

    pub fn sync_status(&self) -> SyncStatus {
        let arrivals = self.syncing.arrivals.load(Ordering::Acquire);
        let running = self.syncing.running.lock();
        let Some(running) = running.as_ref() else {
            return SyncStatus {
                standing: SyncStanding::Off,
                code: String::new(),
                devices: Vec::new(),
                moving: 0,
                owed: 0,
                arrivals,
            };
        };
        let shown = running.sync.shown();
        SyncStatus {
            standing: match shown.standing {
                Standing::Starting | Standing::Connecting => SyncStanding::Connecting,
                Standing::Connected => SyncStanding::Connected,
                Standing::Unreachable => SyncStanding::Unreachable,
                Standing::Refused(_) => SyncStanding::Refused,
            },
            code: running.sync.code().to_owned(),
            devices: shown
                .devices
                .into_iter()
                .map(|device| SyncedDevice {
                    platform: match device.platform {
                        Platform::Linux => DevicePlatform::Linux,
                        Platform::Android => DevicePlatform::Android,
                        Platform::Ios => DevicePlatform::Ios,
                        Platform::Other => DevicePlatform::Other,
                    },
                    name: device.name,
                    online: device.online,
                    nearby: device.nearby,
                    this_device: device.this,
                })
                .collect(),
            moving: counted(
                shown
                    .transfers
                    .iter()
                    .filter(|transfer| {
                        matches!(
                            transfer.state,
                            TransferState::Waiting | TransferState::Moving
                        )
                    })
                    .count(),
            ),
            owed: counted(shown.owed),
            arrivals,
        }
    }

    pub fn sync_force(&self) {
        if let Some(running) = self.syncing.running.lock().as_ref() {
            running.sync.force();
        }
    }

    pub fn sync_network(&self, unmetered: bool) {
        self.syncing.unmetered.store(unmetered, Ordering::Release);
        if let Some(running) = self.syncing.running.lock().as_ref() {
            running
                .sync
                .allow_transfers(unmetered || running.metered_allowed);
        }
    }

    pub fn sync_nearby_record(&self) -> Option<String> {
        self.syncing
            .running
            .lock()
            .as_ref()
            .and_then(|running| running.sync.nearby_record())
    }

    pub fn sync_nearby_heard(&self, record: String) {
        if let Some(running) = self.syncing.running.lock().as_ref() {
            running.sync.nearby_heard(&record);
        }
    }

    pub fn sync_received(&self) -> Result<Vec<ReceivedSong>> {
        let connection = self.catalog.0.lock();
        let mut query = connection
            .prepare("SELECT id, extension, title, artist, album, recording, duration_ms FROM received ORDER BY id")
            .map_err(read_error)?;
        query
            .query_map([], |row| {
                let extension: String = row.get(1)?;
                let song = SearchSong {
                    title: row.get(2)?,
                    artist: row.get(3)?,
                    album: row.get(4)?,
                    recording: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                    duration_ms: row
                        .get::<_, Option<i64>>(6)?
                        .map(|duration| duration.max(0) as u64),
                    ..SearchSong::default()
                };
                Ok(ReceivedSong {
                    id: row.get::<_, i64>(0)? as u64,
                    name: file_name_for(&song, &extension),
                    extension,
                    song,
                })
            })
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)
    }

    pub fn sync_receive(&self, id: u64, descriptor: i32, uri: String) -> Result<Track> {
        let mut output = fresh_destination(descriptor)?;
        let received = self
            .sync_received()?
            .into_iter()
            .find(|received| received.id == id)
            .ok_or(Error::UnknownReceivedSong)?;
        let path = self.waiting_path(id)?.ok_or(Error::UnknownReceivedSong)?;
        let landed = copied(&path, &mut output).and_then(|_| {
            drop(output);
            self.indexed_download(uri, received.name, &received.song)
        });
        if let Err(error) = self.forget_received(id, &path) {
            tracing::warn!(%error, "a landed song from another device is still listed as waiting");
        }
        landed
    }

    pub fn sync_missing_from(&self, playlist: u64) -> Result<u32> {
        let at = self.synced_playlist(playlist)?;
        let running = self.syncing.running.lock();
        let Some(running) = running.as_ref() else {
            return Ok(0);
        };
        running
            .sync
            .missing_from(at)
            .map(|missing| counted(missing.len()))
            .map_err(|source| Error::sync(SyncOp::Missing, source))
    }

    pub fn sync_fetch_missing(&self, playlist: u64, device: String) -> Result<u32> {
        let at = self.synced_playlist(playlist)?;
        let running = self.syncing.running.lock();
        let running = running.as_ref().ok_or(Error::SyncOff)?;
        let device = named(&running.sync, &device).ok_or(Error::UnknownSyncDevice)?;
        let missing = running
            .sync
            .missing_from(at)
            .map_err(|source| Error::sync(SyncOp::Missing, source))?;
        let asked = counted(missing.len());
        if !missing.is_empty() {
            running.sync.fetch(missing, device.id);
        }
        Ok(asked)
    }

    pub fn sync_keeps_songs(&self, playlist: u64) -> Result<bool> {
        let at = self.synced_playlist(playlist)?;
        let running = self.syncing.running.lock();
        let Some(running) = running.as_ref() else {
            return Ok(false);
        };
        running
            .sync
            .kept_songs_of(at)
            .map_err(|source| Error::sync(SyncOp::Keep, source))
    }

    pub fn sync_keep_songs(&self, playlist: u64, keep: bool) -> Result<()> {
        let at = self.synced_playlist(playlist)?;
        let running = self.syncing.running.lock();
        let running = running.as_ref().ok_or(Error::SyncOff)?;
        running
            .sync
            .keep_songs_of(at, keep)
            .map(drop)
            .map_err(|source| Error::sync(SyncOp::Keep, source))
    }

    pub fn sync_handoffs(&self) -> Result<Vec<HandedOff>> {
        let running = self.syncing.running.lock();
        let Some(running) = running.as_ref() else {
            return Ok(Vec::new());
        };
        let devices = running.sync.shown().devices;
        let handoffs = running
            .sync
            .handoffs()
            .map_err(|source| Error::sync(SyncOp::Handoffs, source))?;
        Ok(handoffs
            .into_iter()
            .filter_map(|handoff| {
                let device = devices.iter().find(|device| device.id == handoff.device)?;
                let first = usize::try_from(handoff.at).unwrap_or_default();
                Some(HandedOff {
                    device: device.name.clone(),
                    songs: counted(handoff.rows.len().saturating_sub(first)),
                    playing: handoff.playing,
                })
            })
            .filter(|handed| handed.songs > 0)
            .collect())
    }

    pub fn sync_continue_from(&self, device: String) -> Result<u32> {
        let handoff = {
            let running = self.syncing.running.lock();
            let running = running.as_ref().ok_or(Error::SyncOff)?;
            let device = named(&running.sync, &device).ok_or(Error::UnknownSyncDevice)?;
            running
                .sync
                .handoffs()
                .map_err(|source| Error::sync(SyncOp::Handoffs, source))?
                .into_iter()
                .find(|handoff| handoff.device == device.id)
                .ok_or(Error::NothingHandedOff)?
        };
        self.continue_from(&handoff)
    }
}

impl MobilePlayer {
    fn started(&self, config: SyncConfig) -> Result<Running> {
        let key = if config.key.trim().is_empty() {
            SyncKey::minted()
        } else {
            SyncKey::from_code(&config.key)
        }
        .map_err(|source| Error::sync(SyncOp::Start, source))?;
        let folder = &self.syncing.folder;
        let catalog = Arc::new(DocumentCatalog {
            catalog: Arc::clone(&self.catalog),
            sources: Arc::clone(&self.sources),
            received: folder.join(RECEIVED),
            arrivals: Arc::clone(&self.syncing.arrivals),
            landings: AtomicU64::new(0),
            indexed: Mutex::new(None),
        });
        let settings = Settings {
            server: Some(config.server.trim().to_owned()).filter(|server| !server.is_empty()),
            trusted: None,
            key,
            me: Me {
                name: Some(config.device.trim().to_owned())
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| DEVICE_NAME.to_owned()),
                platform: THIS_PLATFORM,
            },
            follow: Follow {
                favourites: config.favourites,
                playlists: config.playlists,
                everything: config.everything,
            },
            ledger: folder.join(LEDGER),
            staging: folder.join(STAGING),
            transfers_allowed: config.metered_allowed
                || self.syncing.unmetered.load(Ordering::Acquire),
            nearby: Some(nearby()),
        };
        let sync =
            Sync::start(settings, catalog).map_err(|source| Error::sync(SyncOp::Start, source))?;
        Ok(Running {
            sync,
            metered_allowed: config.metered_allowed,
        })
    }

    fn synced_playlist(&self, id: u64) -> Result<LocalPlaylistId> {
        let created: i64 = self
            .catalog
            .0
            .lock()
            .query_row(
                "SELECT created FROM playlists WHERE id = ?1",
                [id as i64],
                |row| row.get(0),
            )
            .optional()
            .map_err(read_error)?
            .ok_or(Error::UnknownPlaylist)?;
        Ok(LocalPlaylistId {
            id: id as i64,
            created,
        })
    }

    fn waiting_path(&self, id: u64) -> Result<Option<PathBuf>> {
        self.catalog
            .0
            .lock()
            .query_row(
                "SELECT path FROM received WHERE id = ?1",
                [id as i64],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map(|path| path.map(PathBuf::from))
            .map_err(read_error)
    }

    fn forget_received(&self, id: u64, path: &Path) -> Result<()> {
        self.catalog
            .0
            .lock()
            .execute("DELETE FROM received WHERE id = ?1", [id as i64])
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(Error::Received {
                op: ReceivedOp::Finish,
                source,
            }),
        }
    }

    fn continue_from(&self, handoff: &Handoff) -> Result<u32> {
        let first = usize::try_from(handoff.at).unwrap_or_default();
        let mut tracks = Vec::new();
        let mut handed_row_held = false;
        {
            let indexed = Indexed::read(&self.catalog.0.lock()).map_err(read_error)?;
            for (row, recording) in handoff.rows.iter().enumerate().skip(first) {
                if let Some(id) = indexed.resolved(recording) {
                    handed_row_held |= row == first;
                    tracks.push(id);
                }
            }
        }
        if tracks.is_empty() {
            return Err(Error::NothingHandedOff);
        }
        let tracks = tracks
            .into_iter()
            .map(|id| self.catalog.track(id))
            .collect::<Result<Vec<_>>>()?;
        let rate = tracks
            .first()
            .and_then(|track| SampleRate::new(track.rate).ok())
            .unwrap_or(SampleRate::HZ_48000);
        let position = if handed_row_held {
            handoff.position
        } else {
            Duration::ZERO
        };
        let rows = tracks
            .iter()
            .map(|track| {
                Ok(Resumable {
                    location: location(&track.uri)?,
                    span: None,
                    held: Some(TrackId::new(track.id)?),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        self.command(Command::Resume(Resumption {
            rows,
            row: 0,
            at: Frames::from_duration(position, rate),
            ..Resumption::default()
        }))?;
        Ok(counted(tracks.len()))
    }
}

impl DocumentCatalog {
    fn arrived(&self) {
        self.arrivals.fetch_add(1, Ordering::AcqRel);
    }

    fn indexed(&self, connection: &Connection) -> rusqlite::Result<Arc<Indexed>> {
        let revision = self.catalog.revision();
        let mut held = self.indexed.lock();
        if let Some((read_at, indexed)) = held.as_ref()
            && *read_at == revision
        {
            return Ok(Arc::clone(indexed));
        }
        let indexed = Arc::new(Indexed::read(connection)?);
        *held = Some((revision, Arc::clone(&indexed)));
        Ok(indexed)
    }

    fn readable(&self, id: u64) -> Answered<bool> {
        let uri = self
            .catalog
            .0
            .lock()
            .query_row(
                "SELECT uri FROM documents WHERE id = ?1",
                [id as i64],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(unavailable)?;
        let Some(at) = uri.and_then(|uri| location(&uri).ok()) else {
            return Ok(false);
        };
        Ok(self.sources.open(&at).is_ok())
    }

    fn waiting(&self, connection: &Connection) -> rusqlite::Result<Vec<Recording>> {
        let mut query = connection.prepare_cached(
            "SELECT title, artist, album, recording, duration_ms FROM received ORDER BY id",
        )?;
        query.query_map([], |row| recording_at(row, 0))?.collect()
    }

    fn landed_name(&self, extension: &str) -> PathBuf {
        let stamp = playlists::created_now();
        let nth = self.landings.fetch_add(1, Ordering::AcqRel);
        self.received.join(format!("{stamp}-{nth}.{extension}"))
    }
}

impl Catalog for DocumentCatalog {
    fn playlists(&self) -> Answered<Vec<LocalPlaylist>> {
        let connection = self.catalog.0.lock();
        let listed = connection
            .prepare("SELECT id, created, name FROM playlists ORDER BY id")
            .and_then(|mut query| {
                query
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(unavailable)?;
        listed
            .into_iter()
            .map(|(id, created, name)| {
                Ok(LocalPlaylist {
                    at: LocalPlaylistId { id, created },
                    name,
                    entries: entries_of(&connection, id).map_err(unavailable)?,
                })
            })
            .collect()
    }

    fn write_playlist(
        &self,
        at: Option<LocalPlaylistId>,
        name: &str,
        entries: &[Recording],
    ) -> Answered<LocalPlaylist> {
        let mut connection = self.catalog.0.lock();
        let transaction = connection.transaction().map_err(unavailable)?;
        let indexed = self.indexed(&transaction).map_err(unavailable)?;
        let rows = entries
            .iter()
            .filter_map(|entry| indexed.resolved(entry))
            .collect::<Vec<_>>();
        let held = match at {
            Some(at) => transaction
                .query_row(
                    "SELECT id FROM playlists WHERE id = ?1 AND created = ?2",
                    params![at.id, at.created],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(unavailable)?,
            None => None,
        };
        let id = match held {
            Some(id) => id,
            None => {
                let named = a_free_name(&transaction, name, None).map_err(unavailable)?;
                transaction
                    .execute(
                        "INSERT INTO playlists(name, folded, created) VALUES (?1, ?2, ?3)",
                        params![named, playlists::folded(&named), playlists::created_now()],
                    )
                    .map_err(unavailable)?;
                transaction.last_insert_rowid()
            }
        };
        let name = a_free_name(&transaction, name, Some(id)).map_err(unavailable)?;
        transaction
            .execute(
                "UPDATE playlists SET name = ?2, folded = ?3, revision = revision + 1 WHERE id = ?1",
                params![id, name, playlists::folded(&name)],
            )
            .map_err(unavailable)?;
        playlists::replace_rows(&transaction, id as u64, &rows).map_err(|error| {
            tracing::warn!(%error, "a synced playlist could not be written");
            Refused::Unavailable
        })?;
        let created: i64 = transaction
            .query_row("SELECT created FROM playlists WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .map_err(unavailable)?;
        let entries = entries_of(&transaction, id).map_err(unavailable)?;
        transaction.commit().map_err(unavailable)?;
        self.arrived();
        Ok(LocalPlaylist {
            at: LocalPlaylistId { id, created },
            name,
            entries,
        })
    }

    fn remove_playlist(&self, at: LocalPlaylistId) -> Answered<()> {
        let removed = self
            .catalog
            .0
            .lock()
            .execute(
                "DELETE FROM playlists WHERE id = ?1 AND created = ?2",
                params![at.id, at.created],
            )
            .map_err(unavailable)?;
        if removed > 0 {
            self.arrived();
        }
        Ok(())
    }

    fn favourites(&self) -> Answered<Vec<Recording>> {
        let connection = self.catalog.0.lock();
        connection
            .prepare(&format!(
                "SELECT {RECORDING_COLUMNS} FROM documents d WHERE d.favourite = 1 ORDER BY d.id"
            ))
            .and_then(|mut query| {
                query
                    .query_map([], |row| recording_at(row, 0))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(unavailable)
    }

    fn favour(&self, recording: &Recording, favoured: bool) -> Answered<bool> {
        let connection = self.catalog.0.lock();
        let Some(id) = self
            .indexed(&connection)
            .map_err(unavailable)?
            .resolved(recording)
        else {
            return Ok(false);
        };
        let changed = connection
            .execute(
                "UPDATE documents SET favourite = ?2 WHERE id = ?1 AND favourite != ?2",
                params![id as i64, favoured],
            )
            .map_err(unavailable)?;
        if changed > 0 {
            self.arrived();
        }
        Ok(true)
    }

    fn listens_after(&self, mark: u64, most: usize) -> Answered<Vec<LocalListen>> {
        let settled_before = unix_seconds(
            SystemTime::now()
                .checked_sub(LISTEN_SETTLES_AFTER)
                .unwrap_or(UNIX_EPOCH),
        );
        let connection = self.catalog.0.lock();
        let rows = connection
            .prepare_cached(&format!(
                "SELECT l.id, l.began, l.heard_ms,
                        l.id = (SELECT max(id) FROM listens WHERE counted = 1 AND elsewhere = 0),
                        {RECORDING_COLUMNS}
                   FROM listens l JOIN documents d ON d.id = l.track
                  WHERE l.id > ?1 AND l.counted = 1 AND l.elsewhere = 0
                  ORDER BY l.id LIMIT ?2"
            ))
            .and_then(|mut query| {
                query
                    .query_map(
                        params![
                            i64::try_from(mark).unwrap_or(i64::MAX),
                            i64::try_from(most).unwrap_or(i64::MAX)
                        ],
                        |row| {
                            Ok((
                                row.get::<_, i64>(0)?,
                                row.get::<_, i64>(1)?,
                                row.get::<_, i64>(2)?,
                                row.get::<_, bool>(3)?,
                                recording_at(row, 4)?,
                            ))
                        },
                    )?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(unavailable)?;
        let mut listens = Vec::with_capacity(rows.len());
        for (id, began, heard_ms, newest, recording) in rows {
            if newest && began > settled_before {
                break;
            }
            listens.push(LocalListen {
                id: id as u64,
                recording,
                at: began.saturating_mul(NANOS_A_SECOND),
                heard: Duration::from_millis(heard_ms.max(0) as u64),
            });
        }
        Ok(listens)
    }

    fn credit(&self, listen: &RemoteListen) -> Answered<bool> {
        let connection = self.catalog.0.lock();
        let Some(id) = self
            .indexed(&connection)
            .map_err(unavailable)?
            .resolved(&listen.recording)
        else {
            return Ok(false);
        };
        connection
            .execute(
                "INSERT INTO listens(track, began, heard_ms, counted, elsewhere) VALUES (?1, ?2, ?3, 1, 1)",
                params![
                    id as i64,
                    listen.at.div_euclid(NANOS_A_SECOND),
                    i64::try_from(listen.heard.as_millis()).unwrap_or(i64::MAX)
                ],
            )
            .map_err(unavailable)?;
        self.arrived();
        Ok(true)
    }

    fn resumption(&self) -> Answered<Option<Resumed>> {
        let connection = self.catalog.0.lock();
        let Some((row_at, position_ms)) = connection
            .query_row(
                "SELECT row_at, position_ms FROM resume WHERE id = 1",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()
            .map_err(unavailable)?
        else {
            return Ok(None);
        };
        let queued = connection
            .prepare(&format!(
                "SELECT q.position, {RECORDING_COLUMNS} FROM queue q JOIN documents d ON d.id = q.track ORDER BY q.position LIMIT ?1"
            ))
            .and_then(|mut query| {
                query
                    .query_map([HANDED_OFF_AT_MOST as i64], |row| {
                        Ok((row.get::<_, i64>(0)?, recording_at(row, 1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(unavailable)?;
        if queued.is_empty() {
            return Ok(None);
        }
        let mut rows = Vec::with_capacity(queued.len());
        let mut at = 0;
        for (position, recording) in queued {
            if position <= row_at {
                at = rows.len();
            }
            rows.push(recording);
        }
        Ok(Some(Resumed {
            rows,
            at: u32::try_from(at).unwrap_or(u32::MAX),
            position: Duration::from_millis(position_ms.max(0) as u64),
            playing: false,
        }))
    }

    fn holds(&self, recording: &Recording) -> Answered<bool> {
        let resolved = {
            let connection = self.catalog.0.lock();
            self.indexed(&connection)
                .map_err(unavailable)?
                .resolved(recording)
        };
        if let Some(id) = resolved
            && self.readable(id)?
        {
            return Ok(true);
        }
        let connection = self.catalog.0.lock();
        Ok(self
            .waiting(&connection)
            .map_err(unavailable)?
            .iter()
            .any(|waiting| waiting.names_the_same_song_as(recording)))
    }

    fn holdings(&self) -> Answered<Vec<Held>> {
        let connection = self.catalog.0.lock();
        connection
            .prepare(&format!(
                "SELECT d.name, {RECORDING_COLUMNS} FROM documents d WHERE d.missing = 0 ORDER BY d.id"
            ))
            .and_then(|mut query| {
                query
                    .query_map([], |row| {
                        Ok(Held {
                            extension: extension_of(&row.get::<_, String>(0)?),
                            recording: recording_at(row, 1)?,
                            size: 0,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(unavailable)
    }

    fn open(&self, recording: &Recording) -> Answered<Option<Opened>> {
        let held = {
            let connection = self.catalog.0.lock();
            match self
                .indexed(&connection)
                .map_err(unavailable)?
                .resolved(recording)
            {
                Some(id) => connection
                    .query_row(
                        "SELECT uri, name FROM documents WHERE id = ?1",
                        [id as i64],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    )
                    .optional()
                    .map_err(unavailable)?,
                None => None,
            }
        };
        let Some((uri, name)) = held else {
            return Ok(None);
        };
        let Ok(at) = location(&uri) else {
            return Ok(None);
        };
        let media = match self.sources.open(&at) {
            Ok(media) => media,
            Err(error) => {
                tracing::debug!(%error, "a song asked for by another device could not be opened");
                return Ok(None);
            }
        };
        let Some(size) = media.stream.byte_len() else {
            return Ok(None);
        };
        Ok(Some(Opened {
            reader: Box::new(media.stream),
            size,
            extension: extension_of(&name),
        }))
    }

    fn land(&self, recording: &Recording, extension: &str, staged: &Path) -> Answered<bool> {
        let extension = extension.to_ascii_lowercase();
        if !AUDIO_EXTENSIONS.contains(&extension.as_str()) {
            return Err(Refused::Unwritable);
        }
        fs::create_dir_all(&self.received).map_err(|_| Refused::Unwritable)?;
        let path = self.landed_name(&extension);
        let text = path.to_str().ok_or(Refused::Unwritable)?.to_owned();
        fs::rename(staged, &path).map_err(|_| Refused::Unwritable)?;
        let kept = self.catalog.0.lock().execute(
            "INSERT INTO received(path, extension, title, artist, album, recording, duration_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                text,
                extension,
                recording.title,
                recording.artist,
                recording.album,
                recording.mbid.as_ref().map(ToString::to_string),
                recording
                    .length
                    .map(|length| i64::try_from(length.as_millis()).unwrap_or(i64::MAX))
            ],
        );
        if let Err(error) = kept {
            let _ = fs::remove_file(&path);
            return Err(unavailable(error));
        }
        self.arrived();
        Ok(true)
    }

    fn revision(&self) -> u64 {
        self.catalog.revision()
    }
}

struct Candidate {
    id: u64,
    words: String,
    album: String,
    length: Option<Duration>,
}

#[derive(Default)]
struct Indexed {
    recordings: BTreeMap<String, u64>,
    artists: BTreeMap<String, Vec<Candidate>>,
}

impl Indexed {
    fn read(connection: &Connection) -> rusqlite::Result<Self> {
        let mut query = connection.prepare_cached(
            "SELECT id, recording, title, artist, album, duration_ms FROM documents WHERE missing = 0 ORDER BY id",
        )?;
        let mut rows = query.query([])?;
        let mut indexed = Self::default();
        while let Some(row) = rows.next()? {
            let id = row.get::<_, i64>(0)? as u64;
            if let Some(recording) = row.get::<_, Option<String>>(1)? {
                indexed.recordings.entry(recording).or_insert(id);
            }
            let artist = folded_letters(row.get::<_, String>(3)?.trim());
            indexed.artists.entry(artist).or_default().push(Candidate {
                id,
                words: title_words(&row.get::<_, String>(2)?),
                album: folded_letters(row.get::<_, String>(4)?.trim()),
                length: length_of(row.get(5)?),
            });
        }
        Ok(indexed)
    }

    fn resolved(&self, recording: &Recording) -> Option<u64> {
        if let Some(id) = recording
            .mbid
            .as_ref()
            .and_then(|mbid| self.recordings.get(mbid.as_str()))
        {
            return Some(*id);
        }
        let words = title_words(&recording.title);
        if words.is_empty() {
            return None;
        }
        let album = folded_letters(recording.album.trim());
        self.artists
            .get(&folded_letters(recording.artist.trim()))?
            .iter()
            .find(|held| {
                held.words == words
                    && (album.is_empty() || held.album == album)
                    && lasts_alike(held.length, recording.length)
            })
            .map(|held| held.id)
    }
}

fn lasts_alike(held: Option<Duration>, wanted: Option<Duration>) -> bool {
    match (held, wanted) {
        (Some(held), Some(wanted)) => held.abs_diff(wanted) <= THE_SAME_LENGTH_WITHIN,
        _ => true,
    }
}

fn length_of(duration_ms: i64) -> Option<Duration> {
    (duration_ms > 0).then(|| Duration::from_millis(duration_ms as u64))
}

fn recording_at(row: &Row<'_>, first: usize) -> rusqlite::Result<Recording> {
    Ok(Recording {
        title: row.get(first)?,
        artist: row.get(first + 1)?,
        album: row.get(first + 2)?,
        mbid: row
            .get::<_, Option<String>>(first + 3)?
            .and_then(|mbid| Mbid::new(&mbid).ok()),
        length: row.get::<_, Option<i64>>(first + 4)?.and_then(length_of),
    })
}

fn entries_of(connection: &Connection, id: i64) -> rusqlite::Result<Vec<Option<Recording>>> {
    let mut query = connection.prepare_cached(&format!(
        "SELECT {RECORDING_COLUMNS} FROM playlist_rows r JOIN documents d ON d.id = r.track
          WHERE r.playlist = ?1 ORDER BY r.position"
    ))?;
    query
        .query_map([id], |row| recording_at(row, 0).map(Some))?
        .collect()
}

fn a_free_name(
    connection: &Connection,
    name: &str,
    keeping: Option<i64>,
) -> rusqlite::Result<String> {
    let trimmed = name.trim();
    let wanted: String = if trimmed.is_empty() {
        UNNAMED_PLAYLIST
    } else {
        trimmed
    }
    .nfc()
    .collect();
    let mut nth = 1_u32;
    loop {
        let candidate = if nth == 1 {
            within(&wanted, PLAYLIST_NAME_BYTES_AT_MOST).to_owned()
        } else {
            let alias = format!(" ({nth})");
            format!(
                "{}{alias}",
                within(&wanted, PLAYLIST_NAME_BYTES_AT_MOST - alias.len())
            )
        };
        let holder: Option<i64> = connection
            .query_row(
                "SELECT id FROM playlists WHERE folded = ?1",
                [playlists::folded(&candidate)],
                |row| row.get(0),
            )
            .optional()?;
        if holder.is_none_or(|holder| Some(holder) == keeping) {
            return Ok(candidate);
        }
        nth += 1;
    }
}

fn within(text: &str, bytes: usize) -> &str {
    let mut end = bytes.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn extension_of(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn file_name_for(song: &SearchSong, extension: &str) -> String {
    let stem = format!("{} - {}", song.artist, song.title)
        .chars()
        .map(|letter| {
            if letter.is_control() || matches!(letter, '/' | '\\') {
                '_'
            } else {
                letter
            }
        })
        .collect::<String>();
    named_within("", OsStr::new(&stem), format!(".{extension}"))
        .to_string_lossy()
        .into_owned()
}

fn named(sync: &Sync, name: &str) -> Option<SyncDevice> {
    sync.shown()
        .devices
        .into_iter()
        .find(|device| !device.this && device.name.eq_ignore_ascii_case(name.trim()))
}

fn copied(from: &Path, into: &mut File) -> Result<u64> {
    let mut source = File::open(from).map_err(|source| Error::Received {
        op: ReceivedOp::Open,
        source,
    })?;
    let bytes = io::copy(&mut source, into).map_err(|source| Error::Received {
        op: ReceivedOp::Copy,
        source,
    })?;
    into.flush().map_err(|source| Error::Received {
        op: ReceivedOp::Finish,
        source,
    })?;
    Ok(bytes)
}

fn unix_seconds(at: SystemTime) -> i64 {
    at.duration_since(UNIX_EPOCH).map_or(0, |since| {
        i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
    })
}

fn counted(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

fn unavailable(error: rusqlite::Error) -> Refused {
    tracing::warn!(%error, "the document catalog could not answer the sync");
    Refused::Unavailable
}

fn read_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Read, source)
}

#[cfg(test)]
mod tests {
    use std::{env, path::Path};

    use super::*;
    use crate::{PlaybackPreferences, features::Services};

    const RECORDING: &str = "b1a9c0e9-d987-4042-ae91-78d6a3267d69";

    fn player(folder: &Path) -> MobilePlayer {
        MobilePlayer {
            player: Mutex::new(None),
            catalog: Arc::new(catalog::Catalog::open(Path::new(":memory:")).expect("a catalog")),
            sources: Arc::new(Sources::local()),
            failure: Mutex::new(None),
            saved: Mutex::new(None),
            preferences: Mutex::new(PlaybackPreferences::default()),
            services: Services::default(),
            playlist_undo: Mutex::new(Vec::new()),
            downloads: crate::downloads::Downloads::default(),
            syncing: Syncing::under(folder.to_path_buf()),
            routing: Arc::new(crate::output::Routing::new()),
            enriching: crate::enrichment::Enriching::default(),
        }
    }

    fn documents_of(player: &MobilePlayer, folder: &Path) -> DocumentCatalog {
        DocumentCatalog {
            catalog: Arc::clone(&player.catalog),
            sources: Arc::clone(&player.sources),
            received: folder.join(RECEIVED),
            arrivals: Arc::clone(&player.syncing.arrivals),
            landings: AtomicU64::new(0),
            indexed: Mutex::new(None),
        }
    }

    fn folder(name: &str) -> PathBuf {
        let folder = env::temp_dir().join(format!(
            "resonate-mobile-sync-{name}-{}",
            playlists::created_now()
        ));
        fs::create_dir_all(&folder).expect("a test folder");
        folder
    }

    fn song(player: &MobilePlayer, uri: &str, title: &str, recording: Option<&str>) -> Track {
        let track = player
            .catalog
            .upsert(Track {
                uri: uri.into(),
                name: format!("{title}.flac"),
                title: title.into(),
                artist: "Björk".into(),
                album: "Homogenic".into(),
                duration_ms: 305_000,
                rate: 44_100,
                codec: "FLAC".into(),
                ..Track::default()
            })
            .expect("an indexed song");
        player
            .catalog
            .learned(track.id, recording, None, None, "")
            .expect("a recording identity");
        track
    }

    fn named(title: &str, artist: &str, seconds: u64) -> Recording {
        Recording {
            mbid: None,
            title: title.into(),
            artist: artist.into(),
            album: String::new(),
            length: Some(Duration::from_secs(seconds)),
        }
    }

    #[test]
    fn a_synced_playlist_resolves_its_songs_by_identity_or_name_and_ends_the_undo_under_it() {
        let folder = folder("playlist");
        let player = player(&folder);
        let joga = song(&player, "content://test/joga.flac", "Jóga", Some(RECORDING));
        let bachelorette = song(&player, "content://test/b.flac", "Bachelorette", None);
        let documents = documents_of(&player, &folder);

        let id = player
            .make_playlist("Evening".into(), vec![joga.id])
            .expect("a playlist");
        let at = player.synced_playlist(id).expect("its sync identity");
        let revision_before = documents.revision();
        let written = documents
            .write_playlist(
                Some(at),
                "Evening",
                &[
                    named("bachelorette", "BJORK", 306),
                    Recording {
                        mbid: Mbid::new(RECORDING).ok(),
                        ..named("Another name", "Someone", 1)
                    },
                    named("Bachelorette", "Björk", 400),
                    named("Hunter", "Björk", 255),
                ],
            )
            .expect("a synced write");

        assert_eq!(written.at, at);
        assert_eq!(written.entries.len(), 2);
        assert_eq!(
            player
                .playlist_tracks(id, 0, 10)
                .expect("the written rows")
                .iter()
                .map(|track| track.id)
                .collect::<Vec<_>>(),
            vec![bachelorette.id, joga.id]
        );
        assert!(documents.revision() > revision_before);
        assert!(matches!(
            player.undo_playlist(),
            Err(Error::PlaylistChanged)
        ));

        let made = documents
            .write_playlist(None, "EVENING", &[named("Jóga", "Björk", 305)])
            .expect("a playlist made by another device");

        assert_eq!(made.name, "EVENING (2)");
        assert_ne!(made.at, at);

        documents.remove_playlist(made.at).expect("a removal");

        assert_eq!(player.playlists().expect("the playlists").len(), 1);
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_listen_from_another_device_counts_here_but_is_neither_sent_back_nor_scrobbled() {
        let folder = folder("listens");
        let player = player(&folder);
        let joga = song(&player, "content://test/joga.flac", "Jóga", Some(RECORDING));
        let documents = documents_of(&player, &folder);
        let an_hour_ago = unix_seconds(SystemTime::now()) - 3600;

        player
            .catalog
            .0
            .lock()
            .execute(
                "INSERT INTO listens(track, began, heard_ms, counted) VALUES (?1, ?2, 300000, 1), (?1, ?3, 1000, 1)",
                params![joga.id as i64, an_hour_ago, unix_seconds(SystemTime::now())],
            )
            .expect("two of this device's listens");
        let credited = documents
            .credit(&RemoteListen {
                recording: named("Joga", "Bjork", 305),
                at: an_hour_ago * NANOS_A_SECOND,
                heard: Duration::from_secs(300),
            })
            .expect("a credited listen");
        let unheld = documents
            .credit(&RemoteListen {
                recording: named("Hunter", "Björk", 255),
                at: an_hour_ago * NANOS_A_SECOND,
                heard: Duration::from_secs(250),
            })
            .expect("an unheld listen");
        let sent = documents
            .listens_after(0, 100)
            .expect("this device's listens");
        let scrobbled: i64 = player
            .catalog
            .0
            .lock()
            .query_row(
                "SELECT count(*) FROM listens l WHERE l.counted = 1 AND l.elsewhere = 0",
                [],
                |row| row.get(0),
            )
            .expect("the listens a scrobbler reads");

        assert!(credited);
        assert!(!unheld);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].at, an_hour_ago * NANOS_A_SECOND);
        assert_eq!(sent[0].heard, Duration::from_secs(300));
        assert_eq!(scrobbled, 2);
        assert_eq!(player.statistics(0).expect("statistics").plays, 3);
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_song_from_another_device_waits_for_its_document_and_is_held_meanwhile() {
        let folder = folder("received");
        let player = player(&folder);
        let documents = documents_of(&player, &folder);
        let staged = folder.join("staged.part");
        fs::write(&staged, b"fLaC").expect("a staged song");
        let hunter = Recording {
            mbid: Mbid::new(RECORDING).ok(),
            ..named("Hunter", "Björk", 255)
        };

        assert!(!documents.holds(&hunter).expect("an unheld song"));
        assert!(matches!(
            documents.land(&hunter, "exe", &staged),
            Err(Refused::Unwritable)
        ));
        assert!(documents.land(&hunter, "FLAC", &staged).expect("a landing"));

        let waiting = player.sync_received().expect("the waiting songs");

        assert!(!staged.exists());
        assert!(documents.holds(&hunter).expect("a waiting song"));
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].name, "Björk - Hunter.flac");
        assert_eq!(waiting[0].song.recording, RECORDING);
        assert_eq!(waiting[0].song.duration_ms, Some(255_000));
        assert!(
            player
                .waiting_path(waiting[0].id)
                .expect("its file")
                .expect("a kept file")
                .exists()
        );
        assert!(matches!(
            player.sync_receive(waiting[0].id + 1, -1, "content://test/x.flac".into()),
            Err(Error::InvalidDescriptor)
        ));
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_pairing_code_is_minted_and_read_back() {
        let code = sync_new_code().expect("a pairing code");

        assert!(sync_reads_as_a_code(code));
        assert!(!sync_reads_as_a_code("not a code".into()));
    }
}
