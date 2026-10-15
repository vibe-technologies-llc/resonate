use std::sync::Arc;
#[cfg(feature = "sync")]
use std::{
    fs::{self, File},
    path::Path,
    thread,
    time::{Duration, Instant},
};

#[cfg(feature = "sync")]
use resonate_core::{PlaylistId, Recording, SyncedSongs};
#[cfg(feature = "sync")]
use resonate_engine::{QueueItem, Unclaimed};
use resonate_library::Library;
#[cfg(feature = "sync")]
use resonate_library::{Error as LibraryError, PlaylistName, SongFiled, SyncedPlaylist};
#[cfg(feature = "sync")]
use resonate_sync::{
    Catalog, Follow, Handoff, Held, LocalListen, LocalPlaylist, LocalPlaylistId, Me, Nearby,
    Opened, Platform, Refused, RemoteListen, Resumed, Settings, Standing, Sync, SyncDevice,
    SyncKey, TransferState,
};

use crate::config::Config;
#[cfg(feature = "sync")]
use crate::{Error, Result, config, error::ConfigKey};

#[cfg(feature = "sync")]
const DEVICE_NAME: &str = "Linux";
#[cfg(feature = "sync")]
const HANDED_OFF_AT_MOST: usize = 500;
#[cfg(feature = "sync")]
const CONNECTED_WITHIN: Duration = Duration::from_secs(15);
#[cfg(feature = "sync")]
const TRANSFERS_LOOKED_AT_EVERY: Duration = Duration::from_millis(250);
#[cfg(feature = "sync")]
const LEDGER: &str = "sync.sqlite";
#[cfg(feature = "sync")]
const STAGING: &str = "sync-staging";
#[cfg(feature = "sync")]
const SYNCING_ALONE: &str = "sync.lock";

pub(crate) struct Syncing {
    #[cfg(feature = "sync")]
    running: Option<Arc<Running>>,
}

#[cfg(feature = "sync")]
struct Running {
    sync: Sync,
    #[cfg(feature = "ui")]
    library: Arc<Library>,
    #[cfg(feature = "ui")]
    handed_off: parking_lot::Mutex<Option<(Instant, Vec<Handoff>)>>,
    _alone: File,
}

#[cfg(feature = "sync")]
struct Catalogued {
    library: Arc<Library>,
}

impl Syncing {
    pub(crate) fn leave(self) {
        #[cfg(feature = "sync")]
        drop(self.running);
    }

    #[cfg(all(feature = "sync", feature = "ui"))]
    pub(crate) fn for_the_window(&self) -> Option<Arc<dyn resonate_ui::Syncing>> {
        self.running
            .as_ref()
            .map(|running| Arc::clone(running) as Arc<dyn resonate_ui::Syncing>)
    }

    #[cfg(all(not(feature = "sync"), feature = "ui"))]
    pub(crate) fn for_the_window(&self) -> Option<Arc<dyn resonate_ui::Syncing>> {
        None
    }
}

#[cfg(feature = "sync")]
pub(crate) fn start(config: &Config, settings: Option<&Path>, library: &Arc<Library>) -> Syncing {
    let running = match started(config, settings, library, follow_of(config.sync_songs)) {
        Ok(running) => running.map(Arc::new),
        Err(error) => {
            tracing::warn!(%error, "devices are not synchronised");
            None
        }
    };
    Syncing { running }
}

#[cfg(not(feature = "sync"))]
pub(crate) fn start(
    config: &Config,
    _settings: Option<&std::path::Path>,
    _library: &Arc<Library>,
) -> Syncing {
    if config.sync == Some(true) || config.sync_key.is_some() {
        tracing::warn!("this build carries no device sync, so nothing is synchronised");
    }
    Syncing {}
}

#[cfg(feature = "sync")]
type Answered<T> = std::result::Result<T, Refused>;

#[cfg(feature = "sync")]
fn follow_of(songs: Option<SyncedSongs>) -> Follow {
    let songs = songs.unwrap_or_default();
    Follow {
        favourites: songs.favourites,
        playlists: songs.playlists,
        everything: songs.everything,
    }
}

#[cfg(feature = "sync")]
fn started(
    config: &Config,
    settings: Option<&Path>,
    library: &Arc<Library>,
    follow: Follow,
) -> Result<Option<Running>> {
    if !config.syncs() {
        return Ok(None);
    }
    let Some(alone) = alone()? else {
        tracing::debug!("another Resonate synchronises this catalog");
        return Ok(None);
    };
    let key = key_of(config, settings)?;
    let folder = config::data_dir()?;
    let settings = Settings {
        server: config.sync_server.clone(),
        trusted: config.sync_server_certificate.clone(),
        key,
        me: Me {
            name: config
                .sync_device
                .clone()
                .unwrap_or_else(|| DEVICE_NAME.to_owned()),
            platform: Platform::Linux,
        },
        follow,
        ledger: folder.join(LEDGER),
        staging: folder.join(STAGING),
        transfers_allowed: true,
        nearby: Some(Nearby::on_the_network()),
    };
    let catalog = Arc::new(Catalogued {
        library: Arc::clone(library),
    });
    let sync = Sync::start(settings, catalog).map_err(Error::Sync)?;
    Ok(Some(Running {
        sync,
        #[cfg(feature = "ui")]
        library: Arc::clone(library),
        #[cfg(feature = "ui")]
        handed_off: parking_lot::Mutex::new(None),
        _alone: alone,
    }))
}

#[cfg(feature = "sync")]
fn alone() -> Result<Option<File>> {
    let folder = config::data_dir()?;
    fs::create_dir_all(&folder).map_err(|source| Error::CreateDir {
        path: folder.clone(),
        source,
    })?;
    let path = folder.join(SYNCING_ALONE);
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|source| Error::SyncLock {
            path: path.clone(),
            source,
        })?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(fs::TryLockError::WouldBlock) => Ok(None),
        Err(fs::TryLockError::Error(source)) => Err(Error::SyncLock { path, source }),
    }
}

#[cfg(feature = "sync")]
fn key_of(config: &Config, settings: Option<&Path>) -> Result<SyncKey> {
    if let Some(code) = &config.sync_key {
        return SyncKey::from_code(code).map_err(Error::Sync);
    }
    let key = SyncKey::minted().map_err(Error::Sync)?;
    if let Some(settings) = settings {
        config::store(settings, ConfigKey::SyncKey, key.code())?;
    }
    Ok(key)
}

#[cfg(feature = "sync")]
fn local(playlist: SyncedPlaylist) -> LocalPlaylist {
    LocalPlaylist {
        at: LocalPlaylistId {
            id: playlist.id.get().cast_signed(),
            created: playlist.created,
        },
        name: playlist.name,
        entries: playlist.entries,
    }
}

#[cfg(feature = "sync")]
fn refused(error: &LibraryError) -> Refused {
    tracing::warn!(%error, "the catalog could not answer the sync");
    Refused::Unavailable
}

#[cfg(feature = "sync")]
fn playlist_id(at: LocalPlaylistId) -> Answered<PlaylistId> {
    PlaylistId::new(at.id.cast_unsigned()).map_err(|_| Refused::NotHere)
}

#[cfg(feature = "sync")]
fn extension_of(path: &Path) -> String {
    path.extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

#[cfg(feature = "sync")]
impl Catalog for Catalogued {
    fn playlists(&self) -> Answered<Vec<LocalPlaylist>> {
        self.library
            .synced_playlists()
            .map(|playlists| playlists.into_iter().map(local).collect())
            .map_err(|error| refused(&error))
    }

    fn write_playlist(
        &self,
        at: Option<LocalPlaylistId>,
        name: &str,
        entries: &[Recording],
    ) -> Answered<LocalPlaylist> {
        let at = at
            .map(|at| playlist_id(at).map(|id| (id, at.created)))
            .transpose()?;
        self.library
            .write_synced_playlist(at, name, entries)
            .map(local)
            .map_err(|error| refused(&error))
    }

    fn remove_playlist(&self, at: LocalPlaylistId) -> Answered<()> {
        self.library
            .remove_synced_playlist(playlist_id(at)?, at.created)
            .map(drop)
            .map_err(|error| refused(&error))
    }

    fn favourites(&self) -> Answered<Vec<Recording>> {
        self.library
            .favoured_recordings()
            .map_err(|error| refused(&error))
    }

    fn favour(&self, recording: &Recording, favoured: bool) -> Answered<bool> {
        self.library
            .favour_recording(recording, favoured)
            .map_err(|error| refused(&error))
    }

    fn listens_after(&self, mark: u64, most: usize) -> Answered<Vec<LocalListen>> {
        self.library
            .listens_to_sync(mark, most)
            .map(|listens| {
                listens
                    .into_iter()
                    .map(|listen| LocalListen {
                        id: listen.id,
                        recording: listen.recording,
                        at: listen.at,
                        heard: listen.heard,
                    })
                    .collect()
            })
            .map_err(|error| refused(&error))
    }

    fn credit(&self, listen: &RemoteListen) -> Answered<bool> {
        self.library
            .credit_listen_elsewhere(&listen.recording, listen.at, listen.heard)
            .map_err(|error| refused(&error))
    }

    fn resumption(&self) -> Answered<Option<Resumed>> {
        let Some(resumption) = self.library.resumption().map_err(|error| refused(&error))? else {
            return Ok(None);
        };
        let playing = resumption.landing();
        let mut rows = Vec::new();
        let mut at = 0;
        for (row, resumable) in resumption.rows.iter().enumerate().take(HANDED_OFF_AT_MOST) {
            let recording = self
                .library
                .recording_at(&resumable.location, resumable.span)
                .map_err(|error| refused(&error))?;
            if let Some(recording) = recording {
                if row <= playing {
                    at = rows.len();
                }
                rows.push(recording);
            }
        }
        let position = resumption
            .rows
            .get(playing)
            .and_then(|resumable| resumable.location.as_path().map(Path::to_path_buf))
            .and_then(|path| {
                let span = resumption.rows.get(playing).and_then(|row| row.span);
                self.library.track_at(&path, span).ok().flatten()
            })
            .map(|track| resumption.at.to_duration(track.spec.rate))
            .unwrap_or_default();
        Ok(Some(Resumed {
            rows,
            at: u32::try_from(at).unwrap_or(u32::MAX),
            position,
            playing: false,
        }))
    }

    fn holds(&self, recording: &Recording) -> Answered<bool> {
        self.library
            .resolve_recording(recording)
            .map(|cut| {
                cut.is_some_and(|cut| cut.location.as_path().is_none_or(|path| path.exists()))
            })
            .map_err(|error| refused(&error))
    }

    fn holdings(&self) -> Answered<Vec<Held>> {
        self.library
            .held_songs()
            .map(|songs| {
                songs
                    .into_iter()
                    .map(|song| Held {
                        extension: extension_of(&song.path),
                        recording: song.recording,
                        size: song.size,
                    })
                    .collect()
            })
            .map_err(|error| refused(&error))
    }

    fn open(&self, recording: &Recording) -> Answered<Option<Opened>> {
        let cut = self
            .library
            .resolve_recording(recording)
            .map_err(|error| refused(&error))?;
        let Some(path) = cut
            .filter(|cut| cut.span.is_none())
            .and_then(|cut| cut.location.as_path().map(Path::to_path_buf))
        else {
            return Ok(None);
        };
        let Ok(file) = File::open(&path) else {
            return Ok(None);
        };
        let size = file
            .metadata()
            .map(|metadata| metadata.len())
            .map_err(|_| Refused::Unavailable)?;
        Ok(Some(Opened {
            reader: Box::new(file),
            size,
            extension: extension_of(&path),
        }))
    }

    fn land(&self, recording: &Recording, extension: &str, staged: &Path) -> Answered<bool> {
        let mut file = File::open(staged).map_err(|_| Refused::Unwritable)?;
        match self
            .library
            .file_song_from_elsewhere(recording, extension, &mut file)
        {
            Ok(SongFiled::Filed(_)) => Ok(true),
            Ok(SongFiled::Refused) => Ok(false),
            Ok(SongFiled::Nowhere) => {
                tracing::warn!(
                    "a song from another device has nowhere to land: set music-folder to the \
                     folder songs are filed in"
                );
                Err(Refused::Unwritable)
            }
            Err(error) => Err(refused(&error)),
        }
    }

    fn revision(&self) -> u64 {
        self.library.catalog_revision()
    }
}

#[cfg(feature = "sync")]
pub(crate) enum Asked<'a> {
    Standing,
    Join(&'a str),
    Leave,
    Forget(&'a str),
    Fetch { playlist: &'a str, from: &'a str },
    Handoff { from: &'a str },
}

#[cfg(feature = "sync")]
pub(crate) fn asked(
    config: &Config,
    settings: &Path,
    library: Option<Arc<Library>>,
    asked: Asked<'_>,
) -> Result<Option<Vec<QueueItem>>> {
    match asked {
        Asked::Join(code) => {
            let key = SyncKey::from_code(code).map_err(Error::Sync)?;
            config::store(settings, ConfigKey::SyncKey, key.code())?;
            said!("This device joins the devices of {}", key.code());
            Ok(None)
        }
        Asked::Leave => {
            config::clear(settings, ConfigKey::SyncKey)?;
            said!("This device left its devices; a new pairing code is made when it next syncs");
            Ok(None)
        }
        Asked::Forget(name) => {
            let library = library.ok_or(Error::SyncOff)?;
            let running = running(config, settings, &library)?;
            let sync = &running.sync;
            let _ = connected(sync);
            let device = device_named(sync, name)?;
            if device.online {
                return Err(Error::SyncDeviceOnline(name.into()));
            }
            sync.forget(device.id).map_err(Error::Sync)?;
            said!("{} is off the list of your devices", device.name);
            Ok(None)
        }
        Asked::Standing => {
            let library = library.ok_or(Error::SyncOff)?;
            let running = running(config, settings, &library)?;
            let sync = &running.sync;
            said!("Pairing code\t{}", sync.code());
            said!(
                "Server\t{}",
                config
                    .sync_server
                    .as_deref()
                    .unwrap_or(resonate_sync::HOSTED_SERVER)
            );
            let standing = connected(sync);
            said!("Standing\t{}", standing_of(standing));
            for device in sync.shown().devices {
                said!(
                    "{}\t{}{}",
                    device.name,
                    match (device.nearby, device.online) {
                        (true, _) => "on this network",
                        (false, true) => "online",
                        (false, false) => "away",
                    },
                    if device.this { ", this device" } else { "" }
                );
            }
            Ok(None)
        }
        Asked::Fetch { playlist, from } => {
            let library = library.ok_or(Error::SyncOff)?;
            let held = library
                .playlist_named(playlist)?
                .ok_or_else(|| Error::NoSuchPlaylist(PlaylistName::new(playlist)))?;
            let running = running(config, settings, &library)?;
            let sync = &running.sync;
            let standing = connected(sync);
            let device = device_named(sync, from)?;
            if standing != Standing::Connected && !device.nearby {
                return Err(Error::SyncUnreachable);
            }
            let created = library
                .synced_playlists()?
                .into_iter()
                .find(|synced| synced.id == held.id)
                .map(|synced| synced.created)
                .ok_or_else(|| Error::NoSuchPlaylist(PlaylistName::new(playlist)))?;
            let missing = sync
                .missing_from(LocalPlaylistId {
                    id: held.id.get().cast_signed(),
                    created,
                })
                .map_err(Error::Sync)?;
            if missing.is_empty() {
                said!("{playlist} lacks nothing {from} could send");
                return Ok(None);
            }
            sync.fetch(missing.clone(), device.id);
            let landed = transferred(sync, &missing);
            said!("{landed} of {} songs came from {from}", missing.len());
            Ok(None)
        }
        Asked::Handoff { from } => {
            let library = library.ok_or(Error::SyncOff)?;
            let running = running(config, settings, &library)?;
            let sync = &running.sync;
            let _ = connected(sync);
            let device = device_named(sync, from)?;
            let handoff = sync
                .handoffs()
                .map_err(Error::Sync)?
                .into_iter()
                .find(|handoff| handoff.device == device.id)
                .ok_or_else(|| Error::NothingHandedOff(from.into()))?;
            Ok(Some(handed_off(&library, &handoff)?))
        }
    }
}

#[cfg(feature = "sync")]
fn running(config: &Config, settings: &Path, library: &Arc<Library>) -> Result<Running> {
    started(config, Some(settings), library, Follow::default())?.ok_or(Error::SyncBusy)
}

#[cfg(feature = "sync")]
fn connected(sync: &Sync) -> Standing {
    let deadline = Instant::now() + CONNECTED_WITHIN;
    loop {
        let shown = sync.shown();
        let refused = matches!(shown.standing, Standing::Refused(_));
        if shown.settled || refused || Instant::now() >= deadline {
            return shown.standing;
        }
        thread::sleep(TRANSFERS_LOOKED_AT_EVERY);
    }
}

#[cfg(feature = "sync")]
fn standing_of(standing: Standing) -> &'static str {
    match standing {
        Standing::Starting | Standing::Connecting => "connecting",
        Standing::Connected => "connected",
        Standing::Unreachable => "the server could not be reached",
        Standing::Refused(_) => "the server refused this device",
    }
}

#[cfg(feature = "sync")]
fn device_named(sync: &Sync, name: &str) -> Result<SyncDevice> {
    let deadline = Instant::now() + CONNECTED_WITHIN;
    loop {
        if let Some(device) = sync
            .shown()
            .devices
            .into_iter()
            .find(|device| !device.this && device.name.eq_ignore_ascii_case(name.trim()))
        {
            return Ok(device);
        }
        if Instant::now() >= deadline {
            return Err(Error::NoSuchDevice(name.into()));
        }
        thread::sleep(TRANSFERS_LOOKED_AT_EVERY);
    }
}

#[cfg(feature = "sync")]
fn transferred(sync: &Sync, asked: &[Recording]) -> usize {
    loop {
        let transfers = sync.shown().transfers;
        let settled = asked.iter().all(|recording| {
            transfers.iter().any(|transfer| {
                transfer.recording == *recording
                    && matches!(
                        transfer.state,
                        TransferState::Landed | TransferState::Failed
                    )
            })
        });
        if settled {
            return transfers
                .iter()
                .filter(|transfer| transfer.state == TransferState::Landed)
                .count();
        }
        thread::sleep(TRANSFERS_LOOKED_AT_EVERY);
    }
}

#[cfg(feature = "sync")]
fn handed_off(library: &Library, handoff: &Handoff) -> Result<Vec<QueueItem>> {
    let mut minting = Unclaimed::beside(&[]);
    let mut items = Vec::new();
    let first = usize::try_from(handoff.at).unwrap_or_default();
    for recording in handoff.rows.iter().skip(first) {
        if let Some(cut) = library.resolve_recording(recording)? {
            items.push(QueueItem {
                id: minting.mint(),
                location: cut.location,
                span: cut.span,
            });
        }
    }
    if items.is_empty() {
        return Err(Error::NothingHandedOff(handoff.device.to_string().into()));
    }
    Ok(items)
}

#[cfg(all(feature = "sync", feature = "ui"))]
const HANDOFFS_READ_EVERY: Duration = Duration::from_secs(5);

#[cfg(all(feature = "sync", feature = "ui"))]
impl Running {
    fn at(&self, playlist: PlaylistId) -> Option<LocalPlaylistId> {
        let held = self.library.playlist(playlist).ok().flatten()?;
        let created = held
            .created
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|since| i64::try_from(since.as_nanos()).ok())?;
        Some(LocalPlaylistId {
            id: playlist.get().cast_signed(),
            created,
        })
    }

    fn handoffs(&self) -> Vec<Handoff> {
        let mut held = self.handed_off.lock();
        if let Some((read, handoffs)) = held.as_ref()
            && read.elapsed() < HANDOFFS_READ_EVERY
        {
            return handoffs.clone();
        }
        let handoffs = self.sync.handoffs().unwrap_or_else(|error| {
            tracing::debug!(%error, "what other devices were playing could not be read");
            Vec::new()
        });
        *held = Some((Instant::now(), handoffs.clone()));
        handoffs
    }

    fn named(&self, name: &str) -> Option<SyncDevice> {
        self.sync
            .shown()
            .devices
            .into_iter()
            .find(|device| !device.this && device.name == name)
    }
}

#[cfg(all(feature = "sync", feature = "ui"))]
impl resonate_ui::Syncing for Running {
    fn force(&self) {
        self.sync.force();
    }

    fn shown(&self) -> resonate_ui::SyncShown {
        let shown = self.sync.shown();
        resonate_ui::SyncShown {
            standing: match shown.standing {
                Standing::Starting | Standing::Connecting => resonate_ui::SyncStanding::Connecting,
                Standing::Connected => resonate_ui::SyncStanding::Connected,
                Standing::Unreachable => resonate_ui::SyncStanding::Unreachable,
                Standing::Refused(_) => resonate_ui::SyncStanding::Refused,
            },
            code: self.sync.code().to_owned(),
            devices: shown
                .devices
                .into_iter()
                .map(|device| resonate_ui::SyncedDevice {
                    id: device.id.to_string(),
                    name: device.name,
                    online: device.online,
                    nearby: device.nearby,
                    this: device.this,
                })
                .collect(),
            moving: shown
                .transfers
                .iter()
                .filter(|transfer| {
                    matches!(
                        transfer.state,
                        TransferState::Waiting | TransferState::Moving
                    )
                })
                .count(),
        }
    }

    fn reads_as_a_code(&self, code: &str) -> bool {
        SyncKey::from_code(code).is_ok()
    }

    fn forget(&self, device: &str) -> bool {
        let Some(found) = self
            .sync
            .shown()
            .devices
            .into_iter()
            .find(|known| !known.this && known.id.to_string() == device)
        else {
            return false;
        };
        self.sync.forget(found.id).unwrap_or_else(|error| {
            tracing::warn!(%error, "a device could not be taken off the list");
            false
        })
    }

    fn missing_from(&self, playlist: PlaylistId) -> usize {
        self.at(playlist)
            .and_then(|at| self.sync.missing_from(at).ok())
            .map_or(0, |missing| missing.len())
    }

    fn fetch_missing(&self, playlist: PlaylistId, from: &str) {
        let (Some(at), Some(device)) = (self.at(playlist), self.named(from)) else {
            return;
        };
        match self.sync.missing_from(at) {
            Ok(missing) if !missing.is_empty() => self.sync.fetch(missing, device.id),
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "a playlist's missing songs could not be read"),
        }
    }

    fn keeps_songs_of(&self, playlist: PlaylistId) -> bool {
        self.at(playlist)
            .and_then(|at| self.sync.kept_songs_of(at).ok())
            .unwrap_or_default()
    }

    fn keep_songs_of(&self, playlist: PlaylistId, keep: bool) {
        if let Some(at) = self.at(playlist)
            && let Err(error) = self.sync.keep_songs_of(at, keep)
        {
            tracing::warn!(%error, "whether a playlist keeps its songs here could not be kept");
        }
    }

    fn handed_off(&self) -> Vec<resonate_ui::HandedOff> {
        let devices = self.sync.shown().devices;
        self.handoffs()
            .into_iter()
            .filter_map(|handoff| {
                let device = devices.iter().find(|device| device.id == handoff.device)?;
                let first = usize::try_from(handoff.at).unwrap_or_default();
                Some(resonate_ui::HandedOff {
                    device: device.name.clone(),
                    songs: handoff.rows.len().saturating_sub(first),
                })
            })
            .filter(|handed| handed.songs > 0)
            .collect()
    }

    fn continue_from(&self, device: &str) -> Vec<resonate_library::Cut> {
        let Some(device) = self.named(device) else {
            return Vec::new();
        };
        let Some(handoff) = self
            .handoffs()
            .into_iter()
            .find(|handoff| handoff.device == device.id)
        else {
            return Vec::new();
        };
        let first = usize::try_from(handoff.at).unwrap_or_default();
        handoff
            .rows
            .iter()
            .skip(first)
            .filter_map(|recording| self.library.resolve_recording(recording).ok().flatten())
            .collect()
    }
}
