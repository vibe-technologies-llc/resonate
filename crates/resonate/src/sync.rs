use std::sync::Arc;
#[cfg(feature = "sync")]
use std::{
    collections::BTreeSet,
    fs::{self, File},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant, SystemTime},
};

#[cfg(feature = "sync")]
use crossbeam_channel::{RecvTimeoutError, Sender, bounded};
#[cfg(feature = "sync")]
use parking_lot::Mutex;
#[cfg(feature = "sync")]
use resonate_core::{Frames, PlaylistId, Recording, SampleRate, SyncedSongs};
#[cfg(feature = "sync")]
use resonate_engine::{QueueItem, Unclaimed};
use resonate_library::Library;
#[cfg(feature = "sync")]
use resonate_library::{Cut, Error as LibraryError, PlaylistName, SongFiled, SyncedPlaylist};
#[cfg(feature = "sync")]
use resonate_sync::{
    Catalog, Follow, Handoff, Held, LocalListen, LocalPlaylist, LocalPlaylistId, Me, Nearby,
    Opened, Platform, Refused, RemoteListen, Resumed, Server, Settings, Standing, Sync, SyncDevice,
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

#[cfg(feature = "sync")]
const SETTINGS_LOOKED_AT_EVERY: Duration = Duration::from_secs(2);
#[cfg(feature = "sync")]
const RELEASED_WITHIN: Duration = Duration::from_secs(5);
#[cfg(feature = "sync")]
const RELEASE_LOOKED_AT_EVERY: Duration = Duration::from_millis(50);

pub(crate) struct Syncing {
    #[cfg(feature = "sync")]
    service: Arc<Service>,
    #[cfg(feature = "sync")]
    watching: Option<Sender<()>>,
}

#[cfg(feature = "sync")]
struct Service {
    running: Mutex<Option<Arc<Running>>>,
}

#[cfg(feature = "sync")]
#[derive(Clone, PartialEq, Eq)]
struct Arrangement {
    syncs: bool,
    key: Option<String>,
    server: Server,
    certificate: Option<PathBuf>,
    device: Option<String>,
    follow: Follow,
}

#[cfg(feature = "sync")]
impl Arrangement {
    fn of(config: &Config) -> Self {
        if !config.syncs() {
            return Self::off();
        }
        Self {
            syncs: true,
            key: config
                .sync_key
                .as_deref()
                .and_then(|code| SyncKey::from_code(code).ok())
                .map(|key| key.code()),
            server: server_of(config),
            certificate: config.sync_server_certificate.clone(),
            device: config.sync_device.clone(),
            follow: follow_of(config.sync_songs),
        }
    }

    fn off() -> Self {
        Self {
            syncs: false,
            key: None,
            server: Server::default(),
            certificate: None,
            device: None,
            follow: Follow::default(),
        }
    }

    fn keyed(self, code: &str) -> Self {
        Self {
            key: SyncKey::from_code(code).ok().map(|key| key.code()),
            ..self
        }
    }
}

#[cfg(feature = "sync")]
impl Service {
    fn with<Answer>(&self, absent: Answer, ask: impl FnOnce(&Running) -> Answer) -> Answer {
        let running = self.running.lock().clone();
        running.as_deref().map_or(absent, ask)
    }

    fn arranged(
        &self,
        config: &Config,
        settings: Option<&Path>,
        library: &Arc<Library>,
    ) -> Arrangement {
        let arrangement = Arrangement::of(config);
        let previous = self.running.lock().take();
        if let Some(previous) = previous {
            released(previous);
        }

        match started(config, settings, library, follow_of(config.sync_songs)) {
            Ok(Some(running)) => {
                let code = running.sync.code().to_owned();
                *self.running.lock() = Some(Arc::new(running));
                arrangement.keyed(&code)
            }
            Ok(None) => arrangement,
            Err(error) => {
                tracing::warn!(%error, "devices are not synchronised");
                arrangement
            }
        }
    }
}

#[cfg(feature = "sync")]
fn released(mut running: Arc<Running>) {
    let deadline = Instant::now() + RELEASED_WITHIN;
    loop {
        match Arc::try_unwrap(running) {
            Ok(running) => return drop(running),
            Err(again) if Instant::now() >= deadline => return drop(again),
            Err(again) => running = again,
        }
        thread::sleep(RELEASE_LOOKED_AT_EVERY);
    }
}

#[cfg(feature = "sync")]
fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|held| held.modified()).ok()
}

#[cfg(feature = "sync")]
fn watching(
    settings: PathBuf,
    seen: Option<SystemTime>,
    arranged: Arrangement,
    every: Duration,
    mut arrange: impl FnMut(&Config) -> Arrangement + Send + 'static,
) -> Option<Sender<()>> {
    let (stop, stopped) = bounded(1);
    let started = thread::Builder::new()
        .name("resonate-sync-settings".to_owned())
        .spawn(move || {
            let mut seen = seen;
            let mut arranged = arranged;
            loop {
                match stopped.recv_timeout(every) {
                    Err(RecvTimeoutError::Timeout) => {}
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
                }
                let stamp = modified(&settings);
                if stamp == seen {
                    continue;
                }
                seen = stamp;
                match config::load(Some(&settings)) {
                    Ok(config) if Arrangement::of(&config) != arranged => {
                        arranged = arrange(&config);
                    }
                    Ok(_) => {}
                    Err(error) => tracing::warn!(
                        %error,
                        "the sync settings went unread, so the ones held are kept"
                    ),
                }
            }
        });
    match started {
        Ok(_) => Some(stop),
        Err(error) => {
            tracing::warn!(%error, "no thread could be started to follow the sync settings");
            None
        }
    }
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
        {
            if let Some(watching) = self.watching {
                let _ = watching.try_send(());
            }
            let running = self.service.running.lock().take();
            drop(running);
        }
    }

    #[cfg(all(feature = "sync", feature = "ui"))]
    pub(crate) fn for_the_window(&self) -> Option<Arc<dyn resonate_ui::Syncing>> {
        Some(Arc::clone(&self.service) as Arc<dyn resonate_ui::Syncing>)
    }

    #[cfg(all(not(feature = "sync"), feature = "ui"))]
    pub(crate) fn for_the_window(&self) -> Option<Arc<dyn resonate_ui::Syncing>> {
        None
    }
}

#[cfg(feature = "sync")]
pub(crate) fn start(config: &Config, settings: Option<&Path>, library: &Arc<Library>) -> Syncing {
    let seen = settings.and_then(modified);
    let service = Arc::new(Service {
        running: Mutex::new(None),
    });
    let arranged = service.arranged(config, settings, library);
    let watching = settings.map(Path::to_path_buf).and_then(|settings| {
        let service = Arc::clone(&service);
        let library = Arc::clone(library);
        let own = settings.clone();
        watching(
            settings,
            seen,
            arranged,
            SETTINGS_LOOKED_AT_EVERY,
            move |config| service.arranged(config, Some(&own), &library),
        )
    });

    Syncing { service, watching }
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
        server: server_of(config),
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
fn server_of(config: &Config) -> Server {
    if config.syncs_nearby_only() {
        return Server::Nowhere;
    }
    config
        .sync_server
        .clone()
        .map_or(Server::Hosted, Server::At)
}

#[cfg(all(feature = "sync", feature = "ui"))]
pub(crate) fn code_reader() -> Option<fn(&str) -> bool> {
    Some(|code| SyncKey::from_code(code).is_ok())
}

#[cfg(all(not(feature = "sync"), feature = "ui"))]
pub(crate) fn code_reader() -> Option<fn(&str) -> bool> {
    None
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
            Ok(SongFiled::Filed(_) | SongFiled::Vaulted(_)) => Ok(true),
            Ok(SongFiled::Refused) => Ok(false),
            Ok(SongFiled::Nowhere) => {
                tracing::warn!(
                    "a song from another device has nowhere to land: set music-folder to the \
                     folder songs are filed in, or open a vault"
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
) -> Result<Option<Handed>> {
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
            match server_of(config) {
                Server::Nowhere => said!("Server\tnone, devices on this network only"),
                Server::At(server) => said!("Server\t{server}"),
                Server::Hosted => said!("Server\t{}", resonate_sync::HOSTED_SERVER),
            }
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
        Standing::Nearby => "syncing with devices on this network",
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
    let mut waiting = asked.iter().map(Recording::key).collect::<BTreeSet<_>>();
    let mut landed = 0;
    loop {
        for transfer in sync.shown().transfers {
            let arrived = match transfer.state {
                TransferState::Landed => true,
                TransferState::Failed => false,
                TransferState::Waiting | TransferState::Moving => continue,
            };
            if waiting.remove(&transfer.recording.key()) && arrived {
                landed += 1;
            }
        }
        if waiting.is_empty() {
            return landed;
        }
        thread::sleep(TRANSFERS_LOOKED_AT_EVERY);
    }
}

#[cfg(feature = "sync")]
pub(crate) struct Handed {
    pub(crate) items: Vec<QueueItem>,
    pub(crate) at: Frames,
}

#[cfg(feature = "sync")]
fn continuing(
    library: &Library,
    rows: &[Recording],
    first: u32,
    position: Duration,
) -> resonate_library::Result<(Vec<Cut>, Frames)> {
    let first = usize::try_from(first).unwrap_or_default();
    let mut cuts = Vec::new();
    let mut handed_row_held = false;
    for (row, recording) in rows.iter().enumerate().skip(first) {
        if let Some(cut) = library.resolve_recording(recording)? {
            handed_row_held |= row == first;
            cuts.push(cut);
        }
    }

    let at = cuts
        .first()
        .filter(|_| handed_row_held)
        .and_then(|cut| rate_of(library, cut))
        .map_or(Frames(0), |rate| Frames::from_duration(position, rate));
    Ok((cuts, at))
}

#[cfg(feature = "sync")]
fn rate_of(library: &Library, cut: &Cut) -> Option<SampleRate> {
    let path = cut.location.as_path()?;
    library
        .track_at(path, cut.span)
        .ok()
        .flatten()
        .map(|track| track.spec.rate)
}

#[cfg(feature = "sync")]
fn handed_off(library: &Library, handoff: &Handoff) -> Result<Handed> {
    let (cuts, at) = continuing(library, &handoff.rows, handoff.at, handoff.position)?;
    if cuts.is_empty() {
        return Err(Error::NothingHandedOff(handoff.device.to_string().into()));
    }

    let mut minting = Unclaimed::beside(&[]);
    let items = cuts
        .into_iter()
        .map(|cut| QueueItem {
            id: minting.mint(),
            location: cut.location,
            span: cut.span,
        })
        .collect();
    Ok(Handed { items, at })
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
impl resonate_ui::Syncing for Service {
    fn force(&self) {
        self.with((), <Running as resonate_ui::Syncing>::force);
    }

    fn shown(&self) -> resonate_ui::SyncShown {
        self.with(
            resonate_ui::SyncShown::default(),
            <Running as resonate_ui::Syncing>::shown,
        )
    }

    fn forget(&self, device: &str) -> bool {
        self.with(false, |running| {
            resonate_ui::Syncing::forget(running, device)
        })
    }

    fn missing_from(&self, playlist: PlaylistId) -> usize {
        self.with(0, |running| {
            resonate_ui::Syncing::missing_from(running, playlist)
        })
    }

    fn fetch_missing(&self, playlist: PlaylistId, from: &str) {
        self.with((), |running| {
            resonate_ui::Syncing::fetch_missing(running, playlist, from);
        });
    }

    fn keeps_songs_of(&self, playlist: PlaylistId) -> bool {
        self.with(false, |running| {
            resonate_ui::Syncing::keeps_songs_of(running, playlist)
        })
    }

    fn keep_songs_of(&self, playlist: PlaylistId, keep: bool) {
        self.with((), |running| {
            resonate_ui::Syncing::keep_songs_of(running, playlist, keep);
        });
    }

    fn handed_off(&self) -> Vec<resonate_ui::HandedOff> {
        self.with(Vec::new(), <Running as resonate_ui::Syncing>::handed_off)
    }

    fn continue_from(&self, device: &str) -> resonate_ui::Continuing {
        self.with(resonate_ui::Continuing::default(), |running| {
            resonate_ui::Syncing::continue_from(running, device)
        })
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
                Standing::Nearby => resonate_ui::SyncStanding::Nearby,
            },
            code: self.sync.code().to_owned(),
            link: self.sync.link(),
            devices: shown
                .devices
                .into_iter()
                .map(|device| resonate_ui::SyncedDevice {
                    id: device.id.to_string(),
                    name: device.name,
                    platform: match device.platform {
                        Platform::Linux => resonate_ui::SyncedPlatform::Linux,
                        Platform::Android => resonate_ui::SyncedPlatform::Android,
                        Platform::Ios => resonate_ui::SyncedPlatform::Ios,
                        Platform::Other => resonate_ui::SyncedPlatform::Other,
                    },
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

    fn continue_from(&self, device: &str) -> resonate_ui::Continuing {
        let Some(device) = self.named(device) else {
            return resonate_ui::Continuing::default();
        };
        let Some(handoff) = self
            .handoffs()
            .into_iter()
            .find(|handoff| handoff.device == device.id)
        else {
            return resonate_ui::Continuing::default();
        };
        match continuing(&self.library, &handoff.rows, handoff.at, handoff.position) {
            Ok((cuts, at)) => resonate_ui::Continuing { cuts, at },
            Err(error) => {
                tracing::debug!(%error, "what another device was playing could not be resolved");
                resonate_ui::Continuing::default()
            }
        }
    }
}

#[cfg(all(test, feature = "sync"))]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn key() -> SyncKey {
        SyncKey::minted().expect("a key is minted")
    }

    fn syncing_with(key: &SyncKey) -> Config {
        Config {
            sync_key: Some(key.code()),
            ..Config::default()
        }
    }

    #[test]
    fn a_code_spelled_another_way_is_the_same_arrangement_and_another_code_is_not() {
        let first = key();
        let spelled = Config {
            sync_key: Some(first.code().to_lowercase().replace('-', " ")),
            ..Config::default()
        };

        assert!(Arrangement::of(&syncing_with(&first)) == Arrangement::of(&spelled));
        assert!(Arrangement::of(&syncing_with(&first)) != Arrangement::of(&syncing_with(&key())));
    }

    #[test]
    fn each_setting_the_service_is_started_with_moves_the_arrangement() {
        let held = key();
        let base = syncing_with(&held);
        let moved = |change: fn(&mut Config)| {
            let mut config = syncing_with(&held);
            change(&mut config);
            Arrangement::of(&config) != Arrangement::of(&base)
        };

        assert!(moved(|config| config.sync_nearby_only = Some(true)));
        assert!(moved(
            |config| config.sync_server = Some("example.org".to_owned())
        ));
        assert!(moved(|config| config.sync_device = Some("Study".to_owned())));
        assert!(moved(
            |config| config.sync_server_certificate = Some(PathBuf::from("a.pem"))
        ));
        assert!(moved(|config| {
            config.sync_songs = Some(SyncedSongs {
                favourites: true,
                ..SyncedSongs::default()
            });
        }));
        assert!(moved(|config| config.sync = Some(false)));
        assert!(moved(|config| config.sync_key = None));
    }

    #[test]
    fn a_device_not_syncing_holds_one_arrangement_whatever_else_is_set() {
        let off = Config {
            sync: Some(false),
            sync_device: Some("Study".to_owned()),
            sync_key: Some(key().code()),
            ..Config::default()
        };

        assert!(
            Arrangement::of(&off)
                == Arrangement::of(&Config {
                    sync: Some(false),
                    ..Config::default()
                })
        );
    }

    #[test]
    fn a_key_minted_at_start_is_the_arrangement_the_file_then_reads_as() {
        let unkeyed = Config::default();
        let minted = key();

        let started = Arrangement::of(&unkeyed).keyed(&minted.code());

        assert!(started == Arrangement::of(&syncing_with(&minted)));
        assert!(started != Arrangement::of(&unkeyed));
    }

    fn wait_for(counted: &AtomicUsize, calls: usize) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if counted.load(Ordering::Acquire) >= calls {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[test]
    fn the_settings_are_followed_and_only_a_change_in_how_the_device_syncs_rearranges_it() {
        let folder = crate::fixtures::scratch("sync-watching");
        let settings = folder.join("config.toml");
        let held = key();
        let joined = key();
        fs::write(&settings, format!("sync-key = \"{}\"\n", held.code())).expect("a file");
        let arranged = Arrangement::of(&syncing_with(&held));
        let rearranged = Arc::new(AtomicUsize::new(0));
        let counting = Arc::clone(&rearranged);

        let stop = watching(
            settings.clone(),
            modified(&settings),
            arranged,
            Duration::from_millis(20),
            move |config| {
                counting.fetch_add(1, Ordering::AcqRel);
                Arrangement::of(config)
            },
        )
        .expect("a thread follows the settings");

        fs::write(
            &settings,
            format!("sync-key = \"{}\"\nvolume = 0.5\n", held.code()),
        )
        .expect("a file");
        thread::sleep(Duration::from_millis(300));
        let unmoved = rearranged.load(Ordering::Acquire);

        fs::write(&settings, format!("sync-key = \"{}\"\n", joined.code())).expect("a file");
        let moved = wait_for(&rearranged, 1);

        fs::write(&settings, "sync-key = [\n").expect("a file");
        thread::sleep(Duration::from_millis(300));
        let after_a_bad_file = rearranged.load(Ordering::Acquire);

        let _ = stop.try_send(());
        let _ = fs::remove_dir_all(&folder);
        assert_eq!(unmoved, 0);
        assert!(moved);
        assert_eq!(after_a_bad_file, 1);
    }

    fn heard(title: &str) -> Recording {
        Recording {
            mbid: None,
            title: title.to_owned(),
            artist: "Artist".to_owned(),
            album: "Album".to_owned(),
            length: Some(Duration::from_millis(100)),
        }
    }

    fn held_song() -> (PathBuf, Library) {
        use crate::fixtures::{ALBUM, ARTIST, TITLE, scanned, scratch, tagged_wave};

        let folder = scratch("sync-continuing");
        fs::write(
            folder.join("held.wav"),
            tagged_wave(&[(TITLE, "Held"), (ARTIST, "Artist"), (ALBUM, "Album")]),
        )
        .expect("a song");
        let library = scanned(&folder);
        (folder, library)
    }

    #[test]
    fn a_handoff_carries_its_place_in_the_song_only_when_that_song_is_held_here() {
        let (folder, library) = held_song();
        let rows = [heard("Missing"), heard("Held")];
        let five_seconds = Duration::from_secs(5);

        let whole = continuing(&library, &rows[1..], 0, five_seconds).expect("resolved");
        let behind_a_missing_one = continuing(&library, &rows, 0, five_seconds).expect("resolved");
        let from_the_held_one = continuing(&library, &rows, 1, five_seconds).expect("resolved");
        let with_no_place_given = continuing(&library, &rows, 0, Duration::ZERO).expect("resolved");

        let _ = fs::remove_dir_all(&folder);
        assert_eq!(whole.0.len(), 1);
        assert_eq!(whole.1, Frames(5 * 44_100));
        assert_eq!(behind_a_missing_one.0.len(), 1);
        assert_eq!(behind_a_missing_one.1, Frames(0));
        assert_eq!(from_the_held_one.1, Frames(5 * 44_100));
        assert_eq!(with_no_place_given.1, Frames(0));
    }

    #[test]
    fn a_handoff_of_songs_none_of_which_is_held_continues_from_nothing() {
        let (folder, library) = held_song();

        let nothing =
            continuing(&library, &[heard("Missing")], 0, Duration::from_secs(5)).expect("resolved");

        let _ = fs::remove_dir_all(&folder);
        assert!(nothing.0.is_empty());
        assert_eq!(nothing.1, Frames(0));
    }
}
