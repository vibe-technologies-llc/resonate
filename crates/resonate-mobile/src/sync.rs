use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use parking_lot::Mutex;
use resonate_codec::{Sources, probe_scanned};
use resonate_core::{
    AUDIO_EXTENSIONS, Answer, Answered as Learnt, Frames, Mbid, MediaLocation, Recording,
    Resumable, Resumption, SampleRate, SharedPrint, TrackId, WholeFile, ZoneId, folded_letters,
    naming::named_within, title_words,
};
use resonate_engine::Command;
use resonate_sync::{
    Catalog, Follow, Handoff, Held, LocalListen, LocalPlaylist, LocalPlaylistId, LocalQuery, Me,
    Nearby, Opened, Platform, QueryBody, Refused, RemoteListen, Resumed, Server, Settings,
    SongTags, Standing, Sync, SyncDevice, SyncKey, TransferState,
};
use rusqlite::{Connection, OptionalExtension, Row, params};
use unicode_normalization::UnicodeNormalization;

use crate::{
    Error, MobilePlayer, Result, StoreOp, Track, catalog,
    documents::location,
    downloads::{file_stem_of, fresh_destination},
    error::{ReceivedOp, SyncOp},
    features::SearchSong,
    lyrics::KeptSheets,
    playlists,
    saved_searches::{StoredQuery, write_stored_query},
    vanished::is_vanished,
};

mod answers;

use answers::the_same_file;

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
const LISTEN_SETTLES_AFTER: Duration = Duration::from_secs(15 * 60);
const THE_SAME_LENGTH_WITHIN: Duration = Duration::from_secs(2);
const PLAYLIST_NAME_BYTES_AT_MOST: usize = 255;
const UNNAMED_PLAYLIST: &str = "Synced";
const NANOS_A_SECOND: i64 = 1_000_000_000;
const COMPARED_AT_ONCE: usize = 64 * 1024;
pub(crate) const RECORDING_COLUMNS: &str = "d.title, d.artist, d.album, d.recording, d.duration_ms";

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
    sheets: Arc<KeptSheets>,
    shown: Arc<AtomicU32>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SyncConfig {
    pub enabled: bool,
    pub nearby_only: bool,
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
    Nearby,
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
    pub id: String,
    pub name: String,
    pub platform: DevicePlatform,
    pub online: bool,
    pub nearby: bool,
    pub this_device: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SyncTransferState {
    Waiting,
    Moving,
    Landed,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SyncTransfer {
    pub title: String,
    pub artist: String,
    pub from: String,
    pub received: u64,
    pub size: u64,
    pub state: SyncTransferState,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SyncStatus {
    pub standing: SyncStanding,
    pub code: String,
    pub devices: Vec<SyncedDevice>,
    pub moving: u32,
    pub downloading: u32,
    pub waiting: u32,
    pub owed: u32,
    pub arrivals: u64,
    pub settled: bool,
    pub transfers: Vec<SyncTransfer>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ReceivedSong {
    pub id: u64,
    pub name: String,
    pub extension: String,
    pub song: SearchSong,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct StandingDocument {
    pub uri: String,
    pub name: String,
}

struct Waiting {
    received: ReceivedSong,
    path: PathBuf,
    file: Option<WholeFile>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Found {
    Visible(u64),
    Withdrawn(u64),
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct HandedOff {
    pub id: String,
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
pub fn sync_device_called(host: String) -> String {
    Me::called("", &host, THIS_PLATFORM, DEVICE_NAME).name
}

#[uniffi::export]
pub fn sync_reads_as_a_code(code: String) -> bool {
    SyncKey::from_code(&code).is_ok()
}

#[uniffi::export]
pub fn sync_code_in_link(link: String) -> Option<String> {
    SyncKey::from_link(&link).ok().map(|key| key.code())
}

#[uniffi::export]
impl MobilePlayer {
    pub fn sync_start(&self, mut config: SyncConfig) -> Result<SyncStatus> {
        let mut running = self.syncing.running.lock();
        if let Some(previous) = running.take() {
            if config.key.trim().is_empty() {
                config.key = previous.sync.code().to_owned();
            }
            previous.sync.leave();
        }
        if config.enabled {
            if !config.nearby_only {
                self.require_online()?;
            }
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
                downloading: 0,
                waiting: 0,
                owed: 0,
                arrivals,
                settled: true,
                transfers: Vec::new(),
            };
        };
        let shown = running.sync.shown();
        let downloading = counted(shown.downloading());
        let waiting = counted(shown.waiting());
        let transfers = shown
            .transfers
            .iter()
            .map(|transfer| SyncTransfer {
                title: transfer.recording.title.clone(),
                artist: transfer.recording.artist.clone(),
                from: shown
                    .devices
                    .iter()
                    .find(|device| device.id == transfer.from)
                    .map(|device| device.name.clone())
                    .unwrap_or_default(),
                received: transfer.received,
                size: transfer.size,
                state: match transfer.state {
                    TransferState::Waiting => SyncTransferState::Waiting,
                    TransferState::Moving => SyncTransferState::Moving,
                    TransferState::Landed => SyncTransferState::Landed,
                    TransferState::Failed => SyncTransferState::Failed,
                },
            })
            .collect::<Vec<_>>();
        SyncStatus {
            standing: match shown.standing {
                Standing::Starting | Standing::Connecting => SyncStanding::Connecting,
                Standing::Connected => SyncStanding::Connected,
                Standing::Unreachable => SyncStanding::Unreachable,
                Standing::Refused(_) => SyncStanding::Refused,
                Standing::Nearby => SyncStanding::Nearby,
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
                    id: device.id.to_string(),
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
            downloading,
            waiting,
            owed: counted(shown.owed),
            arrivals,
            settled: shown.settled,
            transfers,
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
        let waiting = self.waiting_songs()?;
        if waiting.is_empty() {
            return Ok(Vec::new());
        }
        let indexed = Indexed::read(&self.catalog.0.lock()).map_err(read_error)?;

        let mut still = Vec::with_capacity(waiting.len());
        for song in waiting {
            match self.held_already(&song, &indexed) {
                Ok(Some(track)) => self.settled(&song, track),
                Ok(None) => still.push(song.received),
                Err(error) => {
                    tracing::debug!(%error, "a waiting song could not be looked for among the songs held");
                    still.push(song.received);
                }
            }
        }
        Ok(still)
    }

    pub fn sync_receive(&self, id: u64, descriptor: i32, uri: String) -> Result<Track> {
        let mut output = fresh_destination(descriptor)?;
        let waiting = self.waiting_song(id)?;
        let landed = copied(&waiting.path, &mut output).and_then(|_| {
            drop(output);
            self.indexed_download(uri, waiting.received.name.clone(), &waiting.received.song)
        });
        let track = landed?;
        self.settled(&waiting, track.id);
        Ok(track)
    }

    pub fn sync_receive_standing(
        &self,
        id: u64,
        standing: Vec<StandingDocument>,
    ) -> Result<Option<Track>> {
        let waiting = self.waiting_song(id)?;
        for document in standing
            .iter()
            .filter(|document| named_alike(&waiting.received.name, &document.name))
        {
            if !self.holds_the_same_bytes(&document.uri, &waiting.path) {
                continue;
            }
            let track = self.indexed_download(
                document.uri.clone(),
                document.name.clone(),
                &waiting.received.song,
            )?;
            self.settled(&waiting, track.id);
            return Ok(Some(track));
        }
        Ok(None)
    }

    pub fn sync_refuse_received(&self, id: u64) -> Result<()> {
        let waiting = self.waiting_song(id)?;
        self.forget_received(id, &waiting.path)?;
        self.refused_to_land(&waiting.received.song);
        Ok(())
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
        let device = the_device(&running.sync, &device).ok_or(Error::UnknownSyncDevice)?;
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

    pub fn sync_forget_device(&self, id: String) -> Result<bool> {
        let running = self.syncing.running.lock();
        let running = running.as_ref().ok_or(Error::SyncOff)?;
        let device = running
            .sync
            .shown()
            .devices
            .into_iter()
            .find(|device| !device.this && device.id.to_string() == id.trim())
            .ok_or(Error::UnknownSyncDevice)?;
        running
            .sync
            .forget(device.id)
            .map_err(|source| Error::sync(SyncOp::Forget, source))
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
                    id: device.id.to_string(),
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
            let device = the_device(&running.sync, &device).ok_or(Error::UnknownSyncDevice)?;
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
    fn refused_to_land(&self, song: &SearchSong) {
        if let Some(running) = self.syncing.running.lock().as_ref() {
            running.sync.refused(&recording_of(song));
        }
    }

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
            sheets: Arc::clone(&self.services.lyrics),
            shown: self.playing.shown_zone_cell(),
        });
        let settings = Settings {
            server: if config.nearby_only {
                Server::Nowhere
            } else {
                Server::At(config.server.clone())
            },
            trusted: None,
            stun: None,
            key,
            me: Me::called(&config.device, "", THIS_PLATFORM, DEVICE_NAME),
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
            relay_only: false,
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

    #[cfg(test)]
    fn note_received_as(&self, song: &SearchSong, track: u64) -> Result<()> {
        note_received(&self.catalog.0.lock(), &keys_of(&recording_of(song)), track)
            .map_err(|source| Error::store(StoreOp::Write, source))
    }

    fn waiting_songs(&self) -> Result<Vec<Waiting>> {
        let connection = self.catalog.0.lock();
        let mut query = connection
            .prepare(
                "SELECT id, extension, title, artist, album, recording, duration_ms, path, packets, frames, rate
                   FROM received ORDER BY id",
            )
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
                let file = match (
                    row.get::<_, Option<i64>>(8)?,
                    row.get::<_, Option<i64>>(9)?,
                    row.get::<_, Option<u32>>(10)?,
                ) {
                    (Some(packets), Some(frames), Some(rate)) => Some(WholeFile {
                        packets: packets.cast_unsigned(),
                        frames: frames.cast_unsigned(),
                        rate,
                    }),
                    _ => None,
                };
                Ok(Waiting {
                    received: ReceivedSong {
                        id: row.get::<_, i64>(0)? as u64,
                        name: file_name_for(&song, &extension),
                        extension,
                        song,
                    },
                    path: PathBuf::from(row.get::<_, String>(7)?),
                    file,
                })
            })
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)
    }

    fn waiting_song(&self, id: u64) -> Result<Waiting> {
        self.waiting_songs()?
            .into_iter()
            .find(|waiting| waiting.received.id == id)
            .ok_or(Error::UnknownReceivedSong)
    }

    fn held_already(&self, waiting: &Waiting, indexed: &Indexed) -> Result<Option<u64>> {
        let keys = self.keys_waiting(&waiting.received)?;
        let mut held = Vec::new();
        {
            let connection = self.catalog.0.lock();
            for key in &keys {
                held.extend(received_under(&connection, key, false).map_err(read_error)?);
            }
        }
        if let Some(file) = waiting.file {
            held.extend(the_same_file(&self.catalog, &self.sources, file).unwrap_or_default());
        }
        held.extend(
            indexed
                .visible
                .resolved(&recording_of(&waiting.received.song)),
        );
        for id in held {
            if not_gone(&self.catalog, &self.sources, id).map_err(read_error)? {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    fn keys_waiting(&self, received: &ReceivedSong) -> Result<Vec<String>> {
        let mut keys = keys_of(&recording_of(&received.song));
        let connection = self.catalog.0.lock();
        let mut query = connection
            .prepare_cached(
                "SELECT recording FROM received_also WHERE received = ?1 ORDER BY recording",
            )
            .map_err(read_error)?;
        let also = query
            .query_map([received.id as i64], |row| row.get::<_, String>(0))
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)?;
        for key in also {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        Ok(keys)
    }

    fn settled(&self, waiting: &Waiting, track: u64) {
        if let Err(error) = self.landed(waiting, track) {
            tracing::warn!(%error, "a landed song from another device is still listed as waiting");
        }
    }

    fn landed(&self, waiting: &Waiting, track: u64) -> Result<()> {
        let keys = self.keys_waiting(&waiting.received)?;
        {
            let mut connection = self.catalog.0.lock();
            let transaction = connection
                .transaction()
                .map_err(|source| Error::store(StoreOp::Write, source))?;
            note_received(&transaction, &keys, track)
                .and_then(|()| {
                    transaction.execute(
                        "DELETE FROM received WHERE id = ?1",
                        [waiting.received.id as i64],
                    )
                })
                .and_then(|_| transaction.commit())
                .map_err(|source| Error::store(StoreOp::Write, source))?;
        }
        removed(&waiting.path)
    }

    fn holds_the_same_bytes(&self, uri: &str, path: &Path) -> bool {
        let Ok(at) = location(uri) else {
            return false;
        };
        let Ok(media) = self.sources.open(&at) else {
            return false;
        };
        let Ok(file) = File::open(path) else {
            return false;
        };
        let size = file.metadata().map(|metadata| metadata.len()).ok();
        if media
            .stream
            .byte_len()
            .is_some_and(|held| Some(held) != size)
        {
            return false;
        }
        bytes_alike(media.stream, file).unwrap_or(false)
    }

    fn forget_received(&self, id: u64, path: &Path) -> Result<()> {
        self.catalog
            .0
            .lock()
            .execute("DELETE FROM received WHERE id = ?1", [id as i64])
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        removed(path)
    }

    fn continue_from(&self, handoff: &Handoff) -> Result<u32> {
        let first = usize::try_from(handoff.at).unwrap_or_default();
        let mut tracks = Vec::new();
        let mut handed_row_held = false;
        {
            let indexed = Indexed::read(&self.catalog.0.lock()).map_err(read_error)?;
            for (row, recording) in handoff.rows.iter().enumerate().skip(first) {
                if let Some(id) = indexed.visible.resolved(recording).first().copied() {
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

    fn candidates(&self, recording: &Recording) -> Answered<Vec<Found>> {
        let connection = self.catalog.0.lock();
        let indexed = self.indexed(&connection).map_err(unavailable)?;
        let mut found = Vec::new();
        let mut add = |held: Found| {
            if !found.contains(&held) {
                found.push(held);
            }
        };
        for id in indexed.visible.resolved(recording) {
            add(Found::Visible(id));
        }
        for id in received_as(&connection, recording, false).map_err(unavailable)? {
            add(Found::Visible(id));
        }
        for id in indexed.withdrawn.resolved(recording) {
            add(Found::Withdrawn(id));
        }
        for id in received_as(&connection, recording, true).map_err(unavailable)? {
            add(Found::Withdrawn(id));
        }
        Ok(found)
    }

    pub(super) fn resolved_in(
        &self,
        connection: &Connection,
        recording: &Recording,
    ) -> Answered<Option<u64>> {
        if let Some(id) = self
            .indexed(connection)
            .map_err(unavailable)?
            .visible
            .resolved(recording)
            .first()
        {
            return Ok(Some(*id));
        }
        Ok(received_as(connection, recording, false)
            .map_err(unavailable)?
            .first()
            .copied())
    }

    fn opened(&self, id: u64) -> Answered<Option<Opened>> {
        let held = self
            .catalog
            .0
            .lock()
            .query_row(
                "SELECT uri, name FROM documents WHERE id = ?1",
                [id as i64],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(unavailable)?;
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

    fn not_gone(&self, id: u64) -> Answered<bool> {
        not_gone(&self.catalog, &self.sources, id).map_err(unavailable)
    }

    fn waiting_as(&self, connection: &Connection, recording: &Recording) -> rusqlite::Result<bool> {
        let mut query = connection.prepare_cached(
            "SELECT title, artist, album, recording, duration_ms FROM received ORDER BY id",
        )?;
        let named = query
            .query_map([], |row| recording_at(row, 0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .iter()
            .any(|waiting| waiting.names_the_same_song_as(recording));
        if named {
            return Ok(true);
        }
        let mut also = connection
            .prepare_cached("SELECT EXISTS(SELECT 1 FROM received_also WHERE recording = ?1)")?;
        for key in keys_of(recording) {
            if also.query_row([key], |row| row.get::<_, bool>(0))? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn held_as_the_same_file(&self, file: WholeFile) -> Answered<Option<u64>> {
        for id in the_same_file(&self.catalog, &self.sources, file)? {
            if self.not_gone(id)? {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    fn waiting_as_the_same_file(&self, file: WholeFile) -> Answered<Option<i64>> {
        self.catalog
            .0
            .lock()
            .query_row(
                "SELECT id FROM received WHERE packets = ?1 AND frames = ?2 AND rate = ?3 ORDER BY id",
                params![
                    file.packets.cast_signed(),
                    file.frames.cast_signed(),
                    file.rate
                ],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(unavailable)
    }

    fn folded_into_the_waiting(&self, waiting: i64, recording: &Recording) -> Answered<()> {
        let connection = self.catalog.0.lock();
        for key in keys_of(recording) {
            connection
                .execute(
                    "INSERT OR IGNORE INTO received_also (received, recording) VALUES (?1, ?2)",
                    params![waiting, key],
                )
                .map_err(unavailable)?;
        }
        Ok(())
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
            .prepare("SELECT id, created, name FROM playlists WHERE id NOT IN (SELECT playlist FROM playlist_queries) ORDER BY id")
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
        let mut rows = Vec::with_capacity(entries.len());
        for entry in entries {
            rows.extend(self.resolved_in(&transaction, entry)?);
        }
        let held = match at {
            Some(at) => transaction
                .query_row(
                    "SELECT id FROM playlists WHERE id = ?1 AND created = ?2 AND id NOT IN (SELECT playlist FROM playlist_queries)",
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

    fn keeps_queries(&self) -> bool {
        true
    }

    fn queries(&self) -> Answered<Vec<LocalQuery>> {
        let connection = self.catalog.0.lock();
        connection
            .prepare(
                "SELECT p.id, p.created, p.name, q.text, q.sort, q.reading, q.max_rows
                   FROM playlists p JOIN playlist_queries q ON q.playlist = p.id
                  ORDER BY p.id",
            )
            .and_then(|mut query| {
                query
                    .query_map([], |row| {
                        Ok(LocalQuery {
                            at: LocalPlaylistId {
                                id: row.get(0)?,
                                created: row.get(1)?,
                            },
                            body: QueryBody {
                                name: row.get(2)?,
                                text: row.get(3)?,
                                sort: row.get(4)?,
                                reading: row.get(5)?,
                                rows: row.get(6)?,
                            },
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(unavailable)
    }

    fn write_query(&self, at: Option<LocalPlaylistId>, body: &QueryBody) -> Answered<LocalQuery> {
        let stored = StoredQuery {
            text: body.text.clone(),
            sort: body.sort,
            reading: body.reading,
            rows: body.rows,
        };
        if stored.query().is_none() {
            return Err(Refused::Unwritable);
        }

        let mut connection = self.catalog.0.lock();
        let transaction = connection.transaction().map_err(unavailable)?;
        let held = match at {
            Some(at) => transaction
                .query_row(
                    "SELECT p.id FROM playlists p JOIN playlist_queries q ON q.playlist = p.id WHERE p.id = ?1 AND p.created = ?2",
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
                let named = a_free_name(&transaction, &body.name, None).map_err(unavailable)?;
                transaction
                    .execute(
                        "INSERT INTO playlists(name, folded, created) VALUES (?1, ?2, ?3)",
                        params![named, playlists::folded(&named), playlists::created_now()],
                    )
                    .map_err(unavailable)?;
                transaction.last_insert_rowid()
            }
        };
        let name = a_free_name(&transaction, &body.name, Some(id)).map_err(unavailable)?;
        transaction
            .execute(
                "UPDATE playlists SET name = ?2, folded = ?3, revision = revision + 1 WHERE id = ?1",
                params![id, name, playlists::folded(&name)],
            )
            .map_err(unavailable)?;
        write_stored_query(&transaction, id as u64, &stored).map_err(unavailable)?;
        let created: i64 = transaction
            .query_row("SELECT created FROM playlists WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .map_err(unavailable)?;
        transaction.commit().map_err(unavailable)?;
        self.arrived();
        Ok(LocalQuery {
            at: LocalPlaylistId { id, created },
            body: QueryBody {
                name,
                ..body.clone()
            },
        })
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
        let Some(id) = self.resolved_in(&connection, recording)? else {
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
        let Some(id) = self.resolved_in(&connection, &listen.recording)? else {
            return Ok(false);
        };
        let inserted = connection
            .execute(
                "INSERT OR IGNORE INTO listens(track, began, heard_ms, counted, elsewhere, origin_device, origin_id) VALUES (?1, ?2, ?3, 1, 1, ?4, ?5)",
                params![
                    id as i64,
                    listen.at.div_euclid(NANOS_A_SECOND),
                    i64::try_from(listen.heard.as_millis()).unwrap_or(i64::MAX),
                    listen.from.0.as_slice(),
                    listen.id.cast_signed()
                ],
            )
            .map_err(unavailable)?;
        if inserted > 0 {
            self.arrived();
        }
        Ok(true)
    }

    fn resumption(&self) -> Answered<Option<Resumed>> {
        let zone = ZoneId::new(self.shown.load(Ordering::Acquire)).unwrap_or(ZoneId::FIRST);
        let kept = catalog::QueueKept::of(zone);
        let connection = self.catalog.0.lock();
        let Some((row_at, position_ms)) = connection
            .query_row(kept.read_place, [zone.get()], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })
            .optional()
            .map_err(unavailable)?
        else {
            return Ok(None);
        };
        let held: i64 = connection
            .query_row(kept.held, [zone.get()], |row| row.get(0))
            .map_err(unavailable)?;
        let held = usize::try_from(held).unwrap_or_default();
        let (playing, position_ms) =
            catalog::place_among_rows(&connection, zone, row_at, position_ms.max(0) as u64)
                .map_err(unavailable)?;
        let window = Resumed::window_around(playing, held);
        let rows = connection
            .prepare(&format!("SELECT {RECORDING_COLUMNS} {}", kept.window))
            .and_then(|mut query| {
                query
                    .query_map(
                        params![zone.get(), window.len() as i64, window.start as i64],
                        |row| recording_at(row, 0),
                    )?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(unavailable)?;
        if rows.is_empty() {
            return Ok(None);
        }
        let at = playing.saturating_sub(window.start).min(rows.len() - 1);
        Ok(Some(Resumed {
            rows,
            at: u32::try_from(at).unwrap_or(u32::MAX),
            position: Duration::from_millis(position_ms),
            playing: false,
        }))
    }

    fn holds(&self, recording: &Recording) -> Answered<bool> {
        for found in self.candidates(recording)? {
            match found {
                Found::Withdrawn(_) => return Ok(true),
                Found::Visible(id) => {
                    if self.not_gone(id)? {
                        return Ok(true);
                    }
                }
            }
        }
        let connection = self.catalog.0.lock();
        self.waiting_as(&connection, recording).map_err(unavailable)
    }

    fn held_as(&self, recording: &Recording) -> Answered<Option<Recording>> {
        let connection = self.catalog.0.lock();
        let Some(id) = self.resolved_in(&connection, recording)? else {
            return Ok(None);
        };
        connection
            .query_row(
                &format!("SELECT {RECORDING_COLUMNS} FROM documents d WHERE d.id = ?1"),
                [id as i64],
                |row| recording_at(row, 0),
            )
            .optional()
            .map_err(unavailable)
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
        for found in self.candidates(recording)? {
            if let Found::Visible(id) = found
                && let Some(opened) = self.opened(id)?
            {
                return Ok(Some(opened));
            }
        }
        Ok(None)
    }

    fn land(
        &self,
        recording: &Recording,
        _tags: &SongTags,
        extension: &str,
        staged: &Path,
    ) -> Answered<bool> {
        let extension = extension.to_ascii_lowercase();
        if !AUDIO_EXTENSIONS.contains(&extension.as_str()) {
            return Err(Refused::Unwritable);
        }
        if self.holds(recording)? {
            let _ = fs::remove_file(staged);
            return Ok(true);
        }
        fs::create_dir_all(&self.received).map_err(|_| Refused::Unwritable)?;
        let path = self.landed_name(&extension);
        let text = path.to_str().ok_or(Refused::Unwritable)?.to_owned();
        fs::rename(staged, &path).map_err(|_| Refused::Unwritable)?;
        let file = file_of(&self.sources, &path);
        if let Some(file) = file {
            if let Some(track) = self.held_as_the_same_file(file)? {
                note_received(&self.catalog.0.lock(), &keys_of(recording), track)
                    .map_err(unavailable)?;
                let _ = fs::remove_file(&path);
                self.arrived();
                return Ok(true);
            }
            if let Some(waiting) = self.waiting_as_the_same_file(file)? {
                self.folded_into_the_waiting(waiting, recording)?;
                let _ = fs::remove_file(&path);
                return Ok(true);
            }
        }
        let kept = self.catalog.0.lock().execute(
            "INSERT INTO received(path, extension, title, artist, album, recording, duration_ms, packets, frames, rate) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                text,
                extension,
                recording.title,
                recording.artist,
                recording.album,
                recording.mbid.as_ref().map(ToString::to_string),
                recording
                    .length
                    .map(|length| i64::try_from(length.as_millis()).unwrap_or(i64::MAX)),
                file.map(|file| file.packets.cast_signed()),
                file.map(|file| file.frames.cast_signed()),
                file.map(|file| file.rate),
            ],
        );
        if let Err(error) = kept {
            let _ = fs::remove_file(&path);
            return Err(unavailable(error));
        }
        self.arrived();
        Ok(true)
    }

    fn takes_answers(&self) -> bool {
        true
    }

    fn answers_after(&self, mark: i64, most: usize) -> Answered<Vec<Learnt>> {
        let mut learnt = self.lyrics_to_share(mark, most)?;
        learnt.extend(self.studies_to_share(mark, most)?);
        learnt.extend(self.band_peaks_to_share(mark, most)?);
        learnt.sort_by_key(|learnt| learnt.at);
        learnt.truncate(most);
        Ok(learnt)
    }

    fn take_answer(&self, recording: &Recording, answer: &Answer) -> Answered<bool> {
        match answer {
            Answer::Lyrics(lyrics) => self.take_lyrics(recording, lyrics),
            Answer::Study(study) => self.take_study(study),
            Answer::BandPeaks(peaks) => self.take_band_peaks(peaks),
            Answer::Release(_) => Ok(true),
        }
    }

    fn prints_wanted(&self, skip: usize, most: usize) -> Answered<Vec<WholeFile>> {
        self.files_wanting_a_print(skip, most)
    }

    fn print(&self, file: WholeFile) -> Answered<Option<SharedPrint>> {
        self.print_to_share(file)
    }

    fn take_print(&self, file: WholeFile, print: &SharedPrint) -> Answered<bool> {
        self.take_shared_print(file, print)
    }

    fn revision(&self) -> u64 {
        self.catalog.revision()
    }
}

struct Entry {
    id: u64,
    words: String,
    album: String,
    length: Option<Duration>,
}

#[derive(Default)]
struct Index {
    recordings: BTreeMap<String, Vec<u64>>,
    artists: BTreeMap<String, Vec<Entry>>,
}

#[derive(Default)]
struct Indexed {
    visible: Index,
    withdrawn: Index,
}

impl Indexed {
    fn read(connection: &Connection) -> rusqlite::Result<Self> {
        let mut query = connection.prepare_cached(
            "SELECT id, recording, title, artist, album, duration_ms, withdrawn FROM documents
              WHERE missing = 0 OR withdrawn = 1 ORDER BY id",
        )?;
        let mut rows = query.query([])?;
        let mut indexed = Self::default();
        while let Some(row) = rows.next()? {
            let index = if row.get::<_, bool>(6)? {
                &mut indexed.withdrawn
            } else {
                &mut indexed.visible
            };
            let id = row.get::<_, i64>(0)? as u64;
            if let Some(recording) = row.get::<_, Option<String>>(1)? {
                index.recordings.entry(recording).or_default().push(id);
            }
            let artist = folded_letters(row.get::<_, String>(3)?.trim());
            index.artists.entry(artist).or_default().push(Entry {
                id,
                words: title_words(&row.get::<_, String>(2)?),
                album: folded_letters(row.get::<_, String>(4)?.trim()),
                length: length_of(row.get(5)?),
            });
        }
        Ok(indexed)
    }
}

impl Index {
    fn resolved(&self, recording: &Recording) -> Vec<u64> {
        let mut found = recording
            .mbid
            .as_ref()
            .and_then(|mbid| self.recordings.get(mbid.as_str()))
            .cloned()
            .unwrap_or_default();
        let words = title_words(&recording.title);
        if words.is_empty() {
            return found;
        }
        let album = folded_letters(recording.album.trim());
        let named = self
            .artists
            .get(&folded_letters(recording.artist.trim()))
            .into_iter()
            .flatten()
            .filter(|held| {
                held.words == words
                    && (album.is_empty() || held.album == album)
                    && lasts_alike(held.length, recording.length)
            });
        for held in named {
            if !found.contains(&held.id) {
                found.push(held.id);
            }
        }
        found
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

pub(crate) fn recording_at(row: &Row<'_>, first: usize) -> rusqlite::Result<Recording> {
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
    named_within("", OsStr::new(&file_stem_of(song)), format!(".{extension}"))
        .to_string_lossy()
        .into_owned()
}

fn received_as(
    connection: &Connection,
    recording: &Recording,
    withdrawn: bool,
) -> rusqlite::Result<Vec<u64>> {
    let mut found = Vec::new();
    for key in keys_of(recording) {
        for id in received_under(connection, &key, withdrawn)? {
            if !found.contains(&id) {
                found.push(id);
            }
        }
    }
    Ok(found)
}

fn received_under(
    connection: &Connection,
    key: &str,
    withdrawn: bool,
) -> rusqlite::Result<Vec<u64>> {
    let mut query = connection.prepare_cached(
        "SELECT s.track FROM songs_received s JOIN documents d ON d.id = s.track
          WHERE s.recording = ?1 AND d.missing = ?2 AND d.withdrawn = ?2 ORDER BY d.id",
    )?;
    query
        .query_map(params![key, withdrawn], |row| {
            row.get::<_, i64>(0).map(|id| id as u64)
        })?
        .collect()
}

fn not_gone(catalog: &catalog::Catalog, sources: &Sources, id: u64) -> rusqlite::Result<bool> {
    let uri = catalog
        .0
        .lock()
        .query_row(
            "SELECT uri FROM documents WHERE id = ?1",
            [id as i64],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(at) = uri.and_then(|uri| location(&uri).ok()) else {
        return Ok(false);
    };
    match sources.open(&at) {
        Ok(_) => Ok(true),
        Err(error) if is_vanished(&error) => Ok(false),
        Err(error) => {
            tracing::debug!(%error, "a song held here is out of reach for now and counts as held");
            Ok(true)
        }
    }
}

fn note_received(connection: &Connection, keys: &[String], track: u64) -> rusqlite::Result<()> {
    for key in keys {
        connection.execute(
            "INSERT INTO songs_received (recording, track) VALUES (?1, ?2)
             ON CONFLICT (recording) DO UPDATE SET track = excluded.track",
            params![key, track as i64],
        )?;
    }
    Ok(())
}

fn file_of(sources: &Sources, path: &Path) -> Option<WholeFile> {
    let scanned = probe_scanned(sources, &MediaLocation::local(path.to_path_buf())).ok()?;
    Some(WholeFile {
        packets: scanned.packets?.0,
        frames: scanned.info.duration?.get(),
        rate: scanned.info.spec.rate.hz(),
    })
}

fn named_alike(name: &str, candidate: &str) -> bool {
    if candidate == name {
        return true;
    }
    let path = Path::new(name);
    let Some(stem) = path.file_stem().and_then(OsStr::to_str) else {
        return false;
    };
    let numbered = match path.extension().and_then(OsStr::to_str) {
        Some(extension) => candidate
            .strip_suffix(extension)
            .and_then(|rest| rest.strip_suffix('.')),
        None => Some(candidate),
    };
    numbered
        .and_then(|rest| rest.strip_prefix(stem))
        .and_then(|rest| rest.strip_prefix(" ("))
        .and_then(|rest| rest.strip_suffix(')'))
        .is_some_and(|nth| !nth.is_empty() && nth.bytes().all(|byte| byte.is_ascii_digit()))
}

fn bytes_alike(mut held: impl Read, mut received: impl Read) -> io::Result<bool> {
    let mut ours = vec![0_u8; COMPARED_AT_ONCE];
    let mut theirs = vec![0_u8; COMPARED_AT_ONCE];
    loop {
        let read = filled(&mut held, &mut ours)?;
        if filled(&mut received, &mut theirs)? != read || ours[..read] != theirs[..read] {
            return Ok(false);
        }
        if read == 0 {
            return Ok(true);
        }
    }
}

fn filled(from: &mut impl Read, buffer: &mut [u8]) -> io::Result<usize> {
    let mut read = 0;
    while read < buffer.len() {
        match from.read(&mut buffer[read..]) {
            Ok(0) => break,
            Ok(more) => read += more,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(read)
}

fn removed(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::Received {
            op: ReceivedOp::Finish,
            source,
        }),
    }
}

fn keys_of(recording: &Recording) -> Vec<String> {
    let identified = recording
        .mbid
        .is_some()
        .then(|| recording.key().to_string());
    let named = recording
        .is_named()
        .then(|| recording.named_key().to_string());
    identified.into_iter().chain(named).collect()
}

fn recording_of(song: &SearchSong) -> Recording {
    Recording {
        title: song.title.clone(),
        artist: song.artist.clone(),
        album: song.album.clone(),
        mbid: Mbid::new(&song.recording).ok(),
        length: song.duration_ms.map(Duration::from_millis),
    }
}

fn the_device(sync: &Sync, given: &str) -> Option<SyncDevice> {
    let given = given.trim();
    let others = sync
        .shown()
        .devices
        .into_iter()
        .filter(|device| !device.this)
        .collect::<Vec<_>>();
    others
        .iter()
        .find(|device| device.id.to_string() == given)
        .or_else(|| {
            others
                .iter()
                .find(|device| device.name.eq_ignore_ascii_case(given))
        })
        .cloned()
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

    use resonate_library::{Direction, SortOrder};
    use resonate_sync::DeviceId;

    use super::*;
    use crate::{PlaybackPreferences, SavedSearch, SearchOrder, features::Services};

    const RECORDING: &str = "b1a9c0e9-d987-4042-ae91-78d6a3267d69";

    pub(super) fn player(folder: &Path) -> MobilePlayer {
        MobilePlayer {
            playing: crate::zoning::Playing::closed(),
            catalog: Arc::new(catalog::Catalog::open(Path::new(":memory:")).expect("a catalog")),
            sources: Arc::new(Sources::local()),
            preferences: Mutex::new(PlaybackPreferences::default()),
            services: Services::default(),
            playlist_undo: Mutex::new(Vec::new()),
            downloads: crate::downloads::Downloads::default(),
            syncing: Syncing::under(folder.to_path_buf()),
            enriching: crate::enrichment::Enriching::default(),
            room: crate::room::Room::under(std::env::temp_dir()),
            tagging: crate::tagging::Tagging::under(std::env::temp_dir()),
        }
    }

    pub(super) fn documents_of(player: &MobilePlayer, folder: &Path) -> DocumentCatalog {
        DocumentCatalog {
            catalog: Arc::clone(&player.catalog),
            sources: Arc::clone(&player.sources),
            received: folder.join(RECEIVED),
            arrivals: Arc::clone(&player.syncing.arrivals),
            landings: AtomicU64::new(0),
            indexed: Mutex::new(None),
            sheets: Arc::clone(&player.services.lyrics),
            shown: player.playing.shown_zone_cell(),
        }
    }

    pub(super) fn folder(name: &str) -> PathBuf {
        let folder = env::temp_dir().join(format!(
            "resonate-mobile-sync-{name}-{}",
            playlists::created_now()
        ));
        fs::create_dir_all(&folder).expect("a test folder");
        folder
    }

    pub(super) fn song(
        player: &MobilePlayer,
        uri: &str,
        title: &str,
        recording: Option<&str>,
    ) -> Track {
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

    pub(super) fn named(title: &str, artist: &str, seconds: u64) -> Recording {
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
                from: DeviceId([1; 16]),
                id: 7,
                recording: named("Joga", "Bjork", 305),
                at: an_hour_ago * NANOS_A_SECOND,
                heard: Duration::from_secs(300),
            })
            .expect("a credited listen");
        let unheld = documents
            .credit(&RemoteListen {
                from: DeviceId([1; 16]),
                id: 8,
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
    fn a_listen_credited_twice_counts_once_and_two_devices_at_one_moment_count_twice() {
        let folder = folder("credited-twice");
        let player = player(&folder);
        song(&player, "content://test/joga.flac", "Jóga", Some(RECORDING));
        let documents = documents_of(&player, &folder);
        let at = unix_seconds(SystemTime::now()) * NANOS_A_SECOND;
        let from = |device: u8, id: u64| RemoteListen {
            from: DeviceId([device; 16]),
            id,
            recording: named("Joga", "Bjork", 305),
            at,
            heard: Duration::from_secs(300),
        };

        let first = documents.credit(&from(1, 7)).expect("credited");
        let again = documents.credit(&from(1, 7)).expect("credited again");
        let another_device = documents.credit(&from(2, 7)).expect("another device");
        let another_listen = documents.credit(&from(1, 8)).expect("another listen");

        assert!(first && again && another_device && another_listen);
        assert_eq!(player.statistics(0).expect("statistics").plays, 3);
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_saved_query_from_another_device_is_held_as_a_saved_search_apart_from_the_lists() {
        let folder = folder("queries");
        let player = player(&folder);
        let documents = documents_of(&player, &folder);
        song(&player, "content://test/joga.flac", "Jóga", None);
        let list = player.create_playlist("Mix".into()).expect("a list");
        let body = |name: &str, sort: u8| QueryBody {
            name: name.into(),
            text: Some("artist:bjork".into()),
            sort,
            reading: 1,
            rows: Some(10),
        };
        let list_at = LocalPlaylistId {
            id: list as i64,
            created: player.synced_playlist(list).expect("listed").created,
        };

        let written = documents
            .write_query(Some(list_at), &body("Mix", 2))
            .expect("written");

        assert_ne!(written.at, list_at);
        assert_eq!(written.body.name, "Mix (2)");
        assert_eq!(
            documents.queries().expect("the queries"),
            vec![written.clone()]
        );
        assert_eq!(
            documents
                .playlists()
                .expect("the lists")
                .iter()
                .map(|playlist| playlist.at)
                .collect::<Vec<_>>(),
            vec![list_at]
        );
        let shown = player.playlists().expect("the playlists");
        let held = shown
            .iter()
            .find(|playlist| playlist.id == written.at.id as u64)
            .expect("the saved search");
        assert_eq!(held.songs, 1);
        assert_eq!(
            held.search.as_ref().map(|search| search.text.as_str()),
            Some("artist:bjork")
        );
        assert_eq!(
            documents.write_query(None, &body("Later", u8::MAX)),
            Err(Refused::Unwritable)
        );
        assert_ne!(
            documents
                .write_playlist(Some(written.at), "Mix (2)", &[])
                .expect("a list")
                .at,
            written.at
        );

        documents.remove_playlist(written.at).expect("removed");

        assert!(documents.queries().expect("the queries").is_empty());
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_saved_search_revised_on_the_phone_is_offered_to_the_other_devices_in_its_new_form() {
        let folder = folder("revised-query");
        let player = player(&folder);
        let documents = documents_of(&player, &folder);
        let id = player
            .save_search(
                "Jazz".into(),
                SavedSearch {
                    text: "genre:jazz".into(),
                    ..SavedSearch::default()
                },
            )
            .expect("saved");
        let offered = documents.queries().expect("the queries");
        let revision = documents.revision();

        player
            .revise_search(
                id,
                player
                    .playlists()
                    .expect("the playlists")
                    .iter()
                    .find(|playlist| playlist.id == id)
                    .expect("the saved search")
                    .revision,
                "Loved jazz".into(),
                SavedSearch {
                    text: "genre:jazz is:favourite".into(),
                    order: SearchOrder::Plays,
                    descending: true,
                    rows: Some(25),
                },
            )
            .expect("revised");

        let revised = documents.queries().expect("the queries");
        assert_eq!(revised.len(), 1);
        assert_eq!(revised[0].at, offered[0].at);
        assert_eq!(
            revised[0].body,
            QueryBody {
                name: "Loved jazz".into(),
                text: Some("genre:jazz is:favourite".into()),
                sort: SortOrder::Plays.code(),
                reading: Direction::Descending.code(),
                rows: Some(25),
            }
        );
        assert!(documents.revision() > revision);
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_catalog_laid_out_at_its_first_version_is_carried_forward() {
        let folder = folder("migrated");
        let path = folder.join("documents.sqlite");
        let connection = rusqlite::Connection::open(&path).expect("a database");
        connection
            .create_scalar_function(
                "folded",
                1,
                rusqlite::functions::FunctionFlags::SQLITE_UTF8
                    | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
                |context| Ok(resonate_core::folded_letters(&context.get::<String>(0)?)),
            )
            .expect("the fold");
        connection
            .execute_batch(catalog::FIRST_VERSION)
            .expect("the first version");
        connection
            .pragma_update(None, "user_version", 1)
            .expect("its stamp");
        drop(connection);

        let opened = catalog::Catalog::open(&path).expect("carried forward");

        let columns: i64 = opened
            .0
            .lock()
            .query_row(
                "SELECT count(*) FROM pragma_table_info('listens') WHERE name IN ('origin_device', 'origin_id')",
                [],
                |row| row.get(0),
            )
            .expect("the columns");
        let queries: i64 = opened
            .0
            .lock()
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = 'playlist_queries'",
                [],
                |row| row.get(0),
            )
            .expect("the table");

        assert_eq!(columns, 2);
        assert_eq!(queries, 1);
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
            documents.land(&hunter, &SongTags::default(), "exe", &staged),
            Err(Refused::Unwritable)
        ));
        assert!(
            documents
                .land(&hunter, &SongTags::default(), "FLAC", &staged)
                .expect("a landing")
        );

        let waiting = player.sync_received().expect("the waiting songs");

        assert!(!staged.exists());
        assert!(documents.holds(&hunter).expect("a waiting song"));
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].name, "Björk - Hunter.flac");
        assert_eq!(waiting[0].song.recording, RECORDING);
        assert_eq!(waiting[0].song.duration_ms, Some(255_000));
        assert!(
            player
                .waiting_song(waiting[0].id)
                .expect("its file")
                .path
                .exists()
        );
        assert!(matches!(
            player.sync_receive(waiting[0].id + 1, -1, "content://test/x.flac".into()),
            Err(Error::InvalidDescriptor)
        ));

        fs::write(&staged, b"fLaC").expect("the same song again");
        assert!(
            documents
                .land(&hunter, &SongTags::default(), "flac", &staged)
                .expect("a second landing")
        );

        assert!(!staged.exists());
        assert_eq!(player.sync_received().expect("the waiting songs").len(), 1);
        let _ = fs::remove_dir_all(folder);
    }

    struct Here(PathBuf);

    impl crate::Documents for Here {
        fn open_document(&self, uri: String) -> i32 {
            use std::os::fd::IntoRawFd;

            let Some(name) = uri.strip_prefix("content://test/") else {
                return crate::documents::DOCUMENT_REFUSED;
            };
            File::open(self.0.join(name))
                .map_or(crate::documents::DOCUMENT_NOT_FOUND, IntoRawFd::into_raw_fd)
        }
    }

    #[test]
    fn a_song_that_fails_to_land_waits_for_the_front_end_to_try_again_or_give_it_up() {
        use std::os::fd::IntoRawFd;

        let folder = folder("unlanded");
        let player = player(&folder);
        let documents = documents_of(&player, &folder);
        let staged = folder.join("staged.part");
        fs::write(&staged, b"fLaC").expect("a damaged song");
        let hunter = named("Hunter", "Björk", 255);
        documents
            .land(&hunter, &SongTags::default(), "flac", &staged)
            .expect("a landing");
        let waiting = player.sync_received().expect("the waiting songs");
        let descriptor = File::create(folder.join("unlanded.flac"))
            .expect("a destination")
            .into_raw_fd();

        let landed = player.sync_receive(
            waiting[0].id,
            descriptor,
            "content://test/unlanded.flac".into(),
        );
        let still = player.sync_received().expect("the waiting songs");

        assert!(landed.is_err());
        assert_eq!(still.len(), 1);
        assert!(documents.holds(&hunter).expect("a waiting song"));

        player.sync_refuse_received(still[0].id).expect("given up");

        assert!(player.sync_received().expect("none waiting").is_empty());
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_song_landed_under_the_names_of_its_own_file_is_held_by_what_was_asked_for() {
        let folder = folder("asked");
        let player = player(&folder);
        let sources = Arc::new(
            Sources::local().and(Arc::new(
                crate::documents::GrantedFiles::new(Arc::new(Here(folder.clone())), "content")
                    .expect("a granted provider"),
            )),
        );
        let documents = DocumentCatalog {
            sources,
            ..documents_of(&player, &folder)
        };
        fs::write(folder.join("landed.flac"), b"fLaC").expect("a landed file");
        let landed = song(
            &player,
            "content://test/landed.flac",
            "Hunter (2015 Remaster)",
            None,
        );
        let asked = SearchSong {
            title: "Hunter".into(),
            artist: "Björk".into(),
            album: "Homogenic".into(),
            recording: RECORDING.into(),
            duration_ms: Some(305_000),
            ..SearchSong::default()
        };
        let hunter = recording_of(&asked);
        let another = named("Joga", "Björk", 305);

        assert!(!documents.holds(&hunter).expect("before it was noted"));
        assert!(documents.open(&hunter).expect("an answer").is_none());

        player
            .note_received_as(&asked, landed.id)
            .expect("noted as the one asked for");

        assert!(documents.holds(&hunter).expect("after it was noted"));
        assert!(documents.open(&hunter).expect("an answer").is_some());
        assert!(!documents.holds(&another).expect("another song"));

        fs::remove_file(folder.join("landed.flac")).expect("deleted by hand");

        assert!(!documents.holds(&hunter).expect("once the file is gone"));
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_song_landed_under_the_names_of_its_own_file_stays_in_its_playlist_and_favourites_and_settles_once()
     {
        use resonate_sync::{Change, Ledger, Me, Platform, SpaceId};

        let there = folder("sender");
        let sender = player(&there);
        let hunter = song(&sender, "content://test/hunter.flac", "Hunter", None);
        sender
            .make_playlist("Evening".into(), vec![hunter.id])
            .expect("a playlist");
        sender.favourite(hunter.id, true).expect("a favourite");
        let sending_documents = documents_of(&sender, &there);

        let here = folder("receiver");
        let receiver = player(&here);
        let documents = DocumentCatalog {
            sources: Arc::new(
                Sources::local().and(Arc::new(
                    crate::documents::GrantedFiles::new(Arc::new(Here(here.clone())), "content")
                        .expect("a granted provider"),
                )),
            ),
            ..documents_of(&receiver, &here)
        };
        fs::write(here.join("landed.flac"), b"fLaC").expect("a landed file");
        let landed = song(
            &receiver,
            "content://test/landed.flac",
            "Hunter (2015 Remaster)",
            None,
        );
        receiver
            .note_received_as(
                &SearchSong {
                    title: "Hunter".into(),
                    artist: "Björk".into(),
                    album: "Homogenic".into(),
                    duration_ms: Some(305_000),
                    ..SearchSong::default()
                },
                landed.id,
            )
            .expect("noted as the one asked for");
        let me = |name: &str| Me {
            name: name.into(),
            platform: Platform::Android,
        };
        let ledger = || {
            let mut ledger = Ledger::in_memory().expect("a ledger");
            ledger.enter_space(SpaceId([9; 16])).expect("a space");
            ledger
        };
        let mut sending = ledger();
        let mut receiving = ledger();

        sending
            .look(&sending_documents, &me("Phone"))
            .expect("a look");
        let told = sending
            .owed()
            .expect("owed")
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        receiving.arrived(&documents, &told).expect("arrived");
        let written_at = documents.revision();
        let settled = receiving.settle(&documents).expect("settled");
        receiving.look(&documents, &me("Tablet")).expect("a look");
        let echoed = receiving
            .owed()
            .expect("owed")
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let settled_again = receiving.settle(&documents).expect("settled");
        let evening = receiver.playlists().expect("the playlists");
        let rows = receiver
            .playlist_tracks(evening[0].id, 0, 10)
            .expect("the rows")
            .iter()
            .map(|track| track.id)
            .collect::<Vec<_>>();
        let favourites = documents.favourites().expect("the favourites");

        let _ = fs::remove_dir_all(there);
        let _ = fs::remove_dir_all(here);
        assert_eq!(rows, [landed.id]);
        assert_eq!((settled, settled_again), (0, 0));
        assert_eq!(documents.revision(), written_at);
        assert_eq!(
            favourites
                .iter()
                .map(|recording| recording.title.as_str())
                .collect::<Vec<_>>(),
            ["Hunter (2015 Remaster)"]
        );
        assert!(
            !echoed
                .iter()
                .any(|change| matches!(change, Change::Playlist(_) | Change::Favourite(_))),
            "{echoed:?}"
        );
    }

    #[test]
    fn a_song_fetched_whose_file_names_it_otherwise_is_held_and_not_fetched_again() {
        use std::os::fd::IntoRawFd;

        use resonate_codec::{FileTags, TagEdit, TagField, TagSink, Writing};
        use resonate_core::MediaLocation;

        let folder = folder("fetched");
        let sources = Arc::new(
            Sources::local().and(Arc::new(
                crate::documents::GrantedFiles::new(Arc::new(Here(folder.clone())), "content")
                    .expect("a granted provider"),
            )),
        );
        let player = MobilePlayer {
            sources: Arc::clone(&sources),
            ..player(&folder)
        };
        let documents = DocumentCatalog {
            sources,
            ..documents_of(&player, &folder)
        };
        let tagged = folder.join("tagged.wav");
        fs::write(&tagged, crate::harness::silence()).expect("a song");
        let edits = [
            (TagField::Title, "Hunter (2015 Remaster)"),
            (TagField::Artist, "Björk"),
            (TagField::Album, "Homogenic"),
        ]
        .map(|(field, value)| TagEdit {
            field,
            value: value.into(),
        });
        FileTags::default()
            .write(
                &MediaLocation::local(tagged.clone()),
                Writing {
                    edits: &edits,
                    taken: &[],
                    picture: None,
                    unpictured: false,
                    popularity: None,
                },
            )
            .expect("a tagged file");
        let staged = folder.join("staged.part");
        fs::copy(&tagged, &staged).expect("a staged song");
        let hunter = Recording {
            mbid: None,
            title: "Hunter".into(),
            artist: "Björk".into(),
            album: "Homogenic".into(),
            length: Some(Duration::from_secs(2)),
        };

        assert!(!documents.holds(&hunter).expect("before it landed"));
        assert!(
            documents
                .land(&hunter, &SongTags::default(), "wav", &staged)
                .expect("a landing")
        );
        let waiting = player.sync_received().expect("the waiting songs");
        let descriptor = File::create(folder.join("landed.wav"))
            .expect("a destination")
            .into_raw_fd();
        player
            .sync_receive(
                waiting[0].id,
                descriptor,
                "content://test/landed.wav".into(),
            )
            .expect("a landed song");
        let by_name = {
            let connection = player.catalog.0.lock();
            Indexed::read(&connection)
                .expect("an index")
                .visible
                .resolved(&hunter)
        };

        assert!(by_name.is_empty());
        assert!(player.sync_received().expect("none waiting").is_empty());
        assert!(documents.holds(&hunter).expect("held once landed"));
        assert!(documents.open(&hunter).expect("an answer").is_some());
        let _ = fs::remove_dir_all(folder);
    }

    fn granted(folder: &Path) -> (MobilePlayer, DocumentCatalog) {
        let sources = Arc::new(
            Sources::local().and(Arc::new(
                crate::documents::GrantedFiles::new(
                    Arc::new(Here(folder.to_path_buf())),
                    "content",
                )
                .expect("a granted provider"),
            )),
        );
        let player = MobilePlayer {
            sources: Arc::clone(&sources),
            ..player(folder)
        };
        let documents = DocumentCatalog {
            sources,
            ..documents_of(&player, folder)
        };
        (player, documents)
    }

    fn landed_waiting(documents: &DocumentCatalog, folder: &Path, recording: &Recording) {
        let staged = folder.join(format!("{}.part", recording.title));
        fs::write(&staged, crate::harness::silence()).expect("a staged song");
        assert!(
            documents
                .land(recording, &SongTags::default(), "wav", &staged)
                .expect("a landing")
        );
    }

    fn two_seconds_of(title: &str) -> Recording {
        Recording {
            album: "Homogenic".into(),
            ..named(title, "Björk", 2)
        }
    }

    #[test]
    fn a_landing_name_is_alike_only_to_itself_and_its_numbered_copies() {
        let name = "Björk - Hunter.flac";

        assert!(named_alike(name, "Björk - Hunter.flac"));
        assert!(named_alike(name, "Björk - Hunter (1).flac"));
        assert!(named_alike(name, "Björk - Hunter (12).flac"));
        assert!(!named_alike(name, "Björk - Hunter ().flac"));
        assert!(!named_alike(name, "Björk - Hunter (one).flac"));
        assert!(!named_alike(name, "Björk - Hunter (1).wav"));
        assert!(!named_alike(name, "Björk - Hunter Live.flac"));
        assert!(!named_alike(name, "Björk - Jóga.flac"));
    }

    #[test]
    fn a_song_whose_bytes_stand_at_its_name_already_is_taken_as_that_file_and_not_copied_again() {
        let folder = folder("standing");
        let (player, documents) = granted(&folder);
        let hunter = two_seconds_of("Hunter");
        landed_waiting(&documents, &folder, &hunter);
        let waiting = player.sync_received().expect("the waiting songs");
        let name = waiting[0].name.clone();
        let numbered = name.replace(".wav", " (1).wav");
        fs::write(folder.join(&name), b"RIFF of another song").expect("another file");
        fs::write(folder.join(&numbered), crate::harness::silence()).expect("the same bytes");
        fs::write(folder.join("Elsewhere.wav"), crate::harness::silence()).expect("a copy");
        let standing = |names: &[&str]| {
            names
                .iter()
                .map(|name| StandingDocument {
                    uri: format!("content://test/{name}"),
                    name: (*name).to_owned(),
                })
                .collect::<Vec<_>>()
        };

        let unlike = player
            .sync_receive_standing(waiting[0].id, standing(&[&name, "Elsewhere.wav"]))
            .expect("compared");
        let still = player.sync_received().expect("the waiting songs");
        let taken = player
            .sync_receive_standing(waiting[0].id, standing(&[&name, &numbered]))
            .expect("compared")
            .expect("the standing file");
        let held = documents.holds(&hunter).expect("held once taken");

        let _ = fs::remove_dir_all(&folder);
        assert!(unlike.is_none());
        assert_eq!(still.len(), 1);
        assert_eq!(taken.uri, format!("content://test/{numbered}"));
        assert!(player.sync_received().expect("none waiting").is_empty());
        assert!(held);
        assert_eq!(player.song_uris().expect("the songs").len(), 1);
    }

    #[test]
    fn a_song_out_of_reach_for_now_is_held_and_only_one_not_found_is_fetched_again() {
        let folder = folder("away");
        let (player, documents) = granted(&folder);
        song(
            &player,
            "content://card/hunter.flac",
            "Hunter",
            Some(RECORDING),
        );
        song(&player, "content://test/joga.flac", "Jóga", None);
        let hunter = Recording {
            mbid: Mbid::new(RECORDING).ok(),
            ..named("Hunter", "Björk", 305)
        };
        let joga = Recording {
            album: "Homogenic".into(),
            ..named("Jóga", "Björk", 305)
        };

        let away = documents.holds(&hunter).expect("an answer");
        let gone = documents.holds(&joga).expect("an answer");

        let _ = fs::remove_dir_all(&folder);
        assert!(away);
        assert!(!gone);
    }

    fn tagged_silence(at: &Path) {
        use resonate_codec::{FileTags, TagEdit, TagField, TagSink, Writing};

        fs::create_dir_all(at.parent().expect("a folder")).expect("a folder");
        fs::write(at, crate::harness::silence()).expect("a song");
        let edits = [
            (TagField::Title, "Hunter"),
            (TagField::Artist, "Björk"),
            (TagField::Album, "Homogenic"),
        ]
        .map(|(field, value)| TagEdit {
            field,
            value: value.into(),
        });
        FileTags::default()
            .write(
                &MediaLocation::local(at.to_path_buf()),
                Writing {
                    edits: &edits,
                    taken: &[],
                    picture: None,
                    unpictured: false,
                    popularity: None,
                },
            )
            .expect("a tagged file");
    }

    #[test]
    fn a_song_of_a_withdrawn_folder_stays_held_and_only_a_scan_of_another_folder_takes_its_place() {
        let folder = folder("withdrawn");
        let (player, documents) = granted(&folder);
        for place in ["a/hunter.wav", "b/hunter.wav", "landed/hunter.wav"] {
            tagged_silence(&folder.join(place));
        }
        let held = player
            .index_document("content://test/a/hunter.wav".into(), "hunter.wav".into())
            .expect("a scanned song");
        player.favourite(held.id, true).expect("a favourite");
        let hunter = two_seconds_of("Hunter");
        let asked = SearchSong {
            title: "Hunter".into(),
            artist: "Björk".into(),
            album: "Homogenic".into(),
            duration_ms: Some(2_000),
            ..SearchSong::default()
        };
        let still_withdrawn = |player: &MobilePlayer| -> Option<bool> {
            player
                .catalog
                .0
                .lock()
                .query_row(
                    "SELECT withdrawn FROM documents WHERE id = ?1",
                    [held.id as i64],
                    |row| row.get(0),
                )
                .optional()
                .expect("a row read")
        };

        player
            .hide_songs_under("content://test/a/".into())
            .expect("withdrawn");
        let withdrawn_held = documents.holds(&hunter).expect("an answer");
        let landed = player
            .indexed_download(
                "content://test/landed/hunter.wav".into(),
                "hunter.wav".into(),
                &asked,
            )
            .expect("a landing");
        let after_landing = still_withdrawn(&player);
        let scanned = player
            .index_document("content://test/b/hunter.wav".into(), "hunter.wav".into())
            .expect("a scan of another folder");
        let after_scan = still_withdrawn(&player);
        let favoured = (
            player
                .catalog
                .track(landed.id)
                .expect("the landed song")
                .favourite,
            player
                .catalog
                .track(scanned.id)
                .expect("the scanned song")
                .favourite,
        );

        let _ = fs::remove_dir_all(&folder);
        assert!(withdrawn_held);
        assert_eq!(after_landing, Some(true));
        assert_eq!(after_scan, None);
        assert_eq!(favoured, (false, true));
    }

    #[test]
    fn a_folder_granted_again_restores_its_withdrawn_rows_with_their_history() {
        let folder = folder("regranted");
        let player = player(&folder);
        let held = song(
            &player,
            "content://test/tree/document/hunter.flac",
            "Hunter",
            None,
        );
        player.favourite(held.id, true).expect("a favourite");

        player
            .hide_songs_under("content://test/tree/document/".into())
            .expect("withdrawn");
        let regranted = song(
            &player,
            "content://test/tree/document/hunter.flac",
            "Hunter",
            None,
        );
        let withdrawn: bool = player
            .catalog
            .0
            .lock()
            .query_row(
                "SELECT withdrawn FROM documents WHERE id = ?1",
                [held.id as i64],
                |row| row.get(0),
            )
            .expect("the row");

        let _ = fs::remove_dir_all(&folder);
        assert_eq!(regranted.id, held.id);
        assert!(player.catalog.track(held.id).expect("the song").favourite);
        assert!(!withdrawn);
    }

    #[test]
    fn every_copy_a_song_resolves_to_is_tried_so_an_unreadable_one_hides_no_readable_one() {
        let folder = folder("copies");
        let (player, documents) = granted(&folder);
        song(
            &player,
            "content://test/gone.flac",
            "Hunter",
            Some(RECORDING),
        );
        song(
            &player,
            "content://test/kept.flac",
            "Hunter",
            Some(RECORDING),
        );
        song(&player, "content://test/gone-joga.flac", "Jóga", None);
        song(&player, "content://test/kept-joga.flac", "Jóga", None);
        fs::write(folder.join("kept.flac"), b"fLaC").expect("a kept file");
        fs::write(folder.join("kept-joga.flac"), b"fLaC").expect("a kept file");
        let hunter = Recording {
            mbid: Mbid::new(RECORDING).ok(),
            ..named("Hunter", "Björk", 305)
        };
        let joga = Recording {
            album: "Homogenic".into(),
            ..named("Jóga", "Björk", 305)
        };

        let held = (
            documents.holds(&hunter).expect("an answer"),
            documents.holds(&joga).expect("an answer"),
        );
        let opened = (
            documents.open(&hunter).expect("an answer").is_some(),
            documents.open(&joga).expect("an answer").is_some(),
        );

        let _ = fs::remove_dir_all(&folder);
        assert_eq!(held, (true, true));
        assert_eq!(opened, (true, true));
    }

    #[test]
    fn a_song_asked_for_under_another_spelling_is_known_by_its_file_and_not_landed_again() {
        use std::os::fd::IntoRawFd;

        let first = folder("spelling");
        let second = folder("spelling-waiting");
        let (player, documents) = granted(&first);
        let hunter = two_seconds_of("Hunter");
        let renamed = two_seconds_of("Hunter (Remastered)");
        let elsewhere = two_seconds_of("Hunter (Live at Shepherd's Bush)");
        landed_waiting(&documents, &first, &hunter);
        let waiting = player.sync_received().expect("the waiting songs");
        let descriptor = File::create(first.join("landed.wav"))
            .expect("a destination")
            .into_raw_fd();
        player
            .sync_receive(
                waiting[0].id,
                descriptor,
                "content://test/landed.wav".into(),
            )
            .expect("a landed song");

        let unheld_before = documents.holds(&renamed).expect("an answer");
        landed_waiting(&documents, &first, &renamed);
        let after_renamed = player.sync_received().expect("the waiting songs").len();
        let held_renamed = documents.holds(&renamed).expect("an answer");

        let (waiting_player, waiting_documents) = granted(&second);
        landed_waiting(&waiting_documents, &second, &hunter);
        landed_waiting(&waiting_documents, &second, &elsewhere);
        let folded = waiting_player.sync_received().expect("the waiting songs");
        let held_while_waiting = waiting_documents.holds(&elsewhere).expect("an answer");
        let descriptor = File::create(second.join("landed.wav"))
            .expect("a destination")
            .into_raw_fd();
        waiting_player
            .sync_receive(folded[0].id, descriptor, "content://test/landed.wav".into())
            .expect("a landed song");
        let held_once_landed = (
            waiting_documents.holds(&hunter).expect("an answer"),
            waiting_documents.holds(&elsewhere).expect("an answer"),
        );

        let _ = fs::remove_dir_all(&first);
        let _ = fs::remove_dir_all(&second);
        assert!(!unheld_before);
        assert_eq!(after_renamed, 0);
        assert!(held_renamed);
        assert_eq!(player.song_uris().expect("the songs").len(), 1);
        assert_eq!(folded.len(), 1);
        assert!(held_while_waiting);
        assert_eq!(held_once_landed, (true, true));
        assert!(
            waiting_player
                .sync_received()
                .expect("none waiting")
                .is_empty()
        );
    }

    #[test]
    fn a_waiting_song_another_encode_of_which_was_indexed_meanwhile_settles_onto_that_row() {
        let folder = folder("another-encode");
        let (player, documents) = granted(&folder);
        let hunter = two_seconds_of("Hunter");
        landed_waiting(&documents, &folder, &hunter);
        let waiting = player.sync_received().expect("the waiting songs");
        let kept = player.waiting_song(waiting[0].id).expect("its file").path;
        fs::write(folder.join("downloaded.flac"), b"another encode").expect("a download");
        let downloaded = player
            .catalog
            .upsert(Track {
                uri: "content://test/downloaded.flac".into(),
                name: "Björk - Hunter.flac".into(),
                title: hunter.title.clone(),
                artist: hunter.artist.clone(),
                album: hunter.album.clone(),
                duration_ms: 2_000,
                rate: 44_100,
                codec: "FLAC".into(),
                ..Track::default()
            })
            .expect("an indexed download");

        let still = player.sync_received().expect("the waiting songs");
        let settled = documents
            .resolved_in(&player.catalog.0.lock(), &hunter)
            .expect("an answer");

        let _ = fs::remove_dir_all(&folder);
        assert!(
            still.is_empty(),
            "a song already held was left to land again"
        );
        assert!(!kept.exists());
        assert_eq!(settled, Some(downloaded.id));
    }

    #[test]
    fn a_song_indexed_before_its_waiting_row_was_forgotten_is_not_landed_twice() {
        let folder = folder("killed");
        let (player, documents) = granted(&folder);
        let hunter = two_seconds_of("Hunter");
        landed_waiting(&documents, &folder, &hunter);
        let waiting = player.sync_received().expect("the waiting songs");
        let kept = player.waiting_song(waiting[0].id).expect("its file").path;
        fs::copy(&kept, folder.join("landed.wav")).expect("the bytes copied");
        player
            .indexed_download(
                "content://test/landed.wav".into(),
                waiting[0].name.clone(),
                &waiting[0].song,
            )
            .expect("indexed");

        let still = player.sync_received().expect("the waiting songs");
        let held = documents.holds(&hunter).expect("an answer");

        let _ = fs::remove_dir_all(&folder);
        assert!(still.is_empty());
        assert!(!kept.exists());
        assert!(held);
    }

    #[test]
    fn a_pairing_code_is_minted_and_read_back() {
        let code = sync_new_code().expect("a pairing code");

        assert!(sync_reads_as_a_code(code));
        assert!(!sync_reads_as_a_code("not a code".into()));
    }

    #[test]
    fn a_queue_longer_than_a_handoff_hands_off_the_songs_around_the_playhead() {
        let folder = folder("long-queue");
        let player = player(&folder);
        let songs: Vec<u64> = (0..700)
            .map(|number| {
                song(
                    &player,
                    &format!("content://test/{number}.flac"),
                    &format!("Song {number}"),
                    None,
                )
                .id
            })
            .collect();
        player
            .catalog
            .save_queue(&songs, 600, 90_000)
            .expect("a saved queue");
        let documents = documents_of(&player, &folder);

        let resumed = documents
            .resumption()
            .expect("answered")
            .expect("a resumption");

        let _ = fs::remove_dir_all(folder);
        assert_eq!(resumed.rows[resumed.at as usize].title, "Song 600");
        assert_eq!(resumed.position, Duration::from_secs(90));
        assert!(resumed.rows.len() <= 500);
    }

    #[test]
    fn a_handoff_offers_the_queue_and_place_of_the_zone_shown_and_not_the_first_zones() {
        let folder = folder("zone-handoff");
        let player = player(&folder);
        let first: Vec<u64> = ["One", "Two"]
            .iter()
            .map(|title| {
                song(
                    &player,
                    &format!("content://test/{title}.flac"),
                    title,
                    None,
                )
                .id
            })
            .collect();
        let kitchen: Vec<u64> = ["Three", "Four", "Five"]
            .iter()
            .map(|title| {
                song(
                    &player,
                    &format!("content://test/{title}.flac"),
                    title,
                    None,
                )
                .id
            })
            .collect();
        let zone = ZoneId::new(2).expect("a second zone");
        player
            .catalog
            .keep_zone(&crate::zoning::KeptZone {
                id: zone,
                name: "Kitchen".into(),
                route: String::new(),
                volume: resonate_core::Volume::MAX,
            })
            .expect("a kept zone");
        player
            .catalog
            .save_queue_of(ZoneId::FIRST, &first, 1, 20_000)
            .expect("the first zone's queue");
        player
            .catalog
            .save_queue_of(zone, &kitchen, 2, 40_000)
            .expect("the kitchen's queue");
        let documents = documents_of(&player, &folder);

        let from_the_first = documents
            .resumption()
            .expect("answered")
            .expect("a resumption");
        player
            .playing
            .shown_zone_cell()
            .store(zone.get(), Ordering::Release);
        let from_the_kitchen = documents
            .resumption()
            .expect("answered")
            .expect("a resumption");
        player.playing.shown_zone_cell().store(9, Ordering::Release);
        let from_a_zone_without_a_queue = documents.resumption().expect("answered");

        let _ = fs::remove_dir_all(folder);
        assert_eq!(
            from_the_first
                .rows
                .iter()
                .map(|row| row.title.as_str())
                .collect::<Vec<_>>(),
            vec!["One", "Two"]
        );
        assert_eq!(from_the_first.rows[from_the_first.at as usize].title, "Two");
        assert_eq!(from_the_first.position, Duration::from_secs(20));
        assert_eq!(
            from_the_kitchen
                .rows
                .iter()
                .map(|row| row.title.as_str())
                .collect::<Vec<_>>(),
            vec!["Three", "Four", "Five"]
        );
        assert_eq!(
            from_the_kitchen.rows[from_the_kitchen.at as usize].title,
            "Five"
        );
        assert_eq!(from_the_kitchen.position, Duration::from_secs(40));
        assert!(from_a_zone_without_a_queue.is_none());
    }
}
