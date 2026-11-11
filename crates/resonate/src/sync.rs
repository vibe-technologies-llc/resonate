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
use resonate_core::{
    Answer, Frames, Mbid, PlaylistId, Recording, RemoteTrust, SampleRate, SharedLyricsfile,
    SharedPrint, SyncedSongs, WholeFile, ZoneId,
};
#[cfg(feature = "sync")]
use resonate_engine::{QueueItem, Unclaimed};
use resonate_library::Library;
#[cfg(feature = "sync")]
use resonate_library::{
    Cut, Error as LibraryError, HeldTags, PlaylistName, SongFiled, SyncedPlaylist, SyncedQuery,
    SyncedQueryBody,
};
#[cfg(feature = "sync")]
use resonate_mpris::ZoneAsked;
#[cfg(feature = "sync")]
use resonate_sync::{
    Catalog, Controlled, Controlling, DeviceId, Follow, Handoff, Held, HeldShare, LocalListen,
    LocalPlaylist, LocalPlaylistId, LocalQuery, Me, Mirror, Nearby, Opened, Platform, QueryBody,
    Refused, RemoteCommand, RemoteListen, RemotePlayback, RemoteRow, RemoteZone, Resumed, Server,
    Settings, SongTags, Standing, Sync, SyncDevice, SyncKey, TransferState, Watched,
};

use crate::config::Config;
#[cfg(feature = "sync")]
use crate::{Error, Result, config, error::ConfigKey, words};

#[cfg(all(feature = "sync", not(windows), not(target_os = "macos")))]
const DEVICE_NAME: &str = "Linux";
#[cfg(all(feature = "sync", windows))]
const DEVICE_NAME: &str = "Windows";
#[cfg(all(feature = "sync", target_os = "macos"))]
const DEVICE_NAME: &str = "macOS";
#[cfg(all(feature = "sync", not(windows), not(target_os = "macos")))]
const THIS_PLATFORM: Platform = Platform::Linux;
#[cfg(all(feature = "sync", windows))]
const THIS_PLATFORM: Platform = Platform::Windows;
#[cfg(all(feature = "sync", target_os = "macos"))]
const THIS_PLATFORM: Platform = Platform::MacOs;
#[cfg(all(feature = "sync", any(unix, test)))]
const THE_LOCAL_DOMAIN: &str = ".local";
#[cfg(feature = "sync")]
const CONNECTED_WITHIN: Duration = Duration::from_secs(15);
#[cfg(feature = "sync")]
const TRANSFERS_LOOKED_AT_EVERY: Duration = Duration::from_millis(250);
#[cfg(feature = "sync")]
const FETCHED_AT_A_TIME: usize = 100;
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
    driven: Mutex<Option<Arc<dyn Controlled>>>,
    trust: Mutex<RemoteTrust>,
}

#[cfg(feature = "sync")]
#[derive(Clone, PartialEq, Eq)]
struct Arrangement {
    syncs: bool,
    key: Option<String>,
    server: Server,
    certificate: Option<PathBuf>,
    stun: Option<String>,
    device: Option<String>,
    follow: Follow,
    trust: RemoteTrust,
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
            stun: config.sync_stun.clone(),
            device: config.sync_device.clone(),
            follow: follow_of(config.sync_songs),
            trust: config.sync_control.unwrap_or_default(),
        }
    }

    fn off() -> Self {
        Self {
            syncs: false,
            key: None,
            server: Server::default(),
            certificate: None,
            stun: None,
            device: None,
            follow: Follow::default(),
            trust: RemoteTrust::default(),
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
    #[cfg(feature = "ui")]
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

        *self.trust.lock() = arrangement.trust;
        match started(config, settings, library, follow_of(config.sync_songs)) {
            Ok(Some(running)) => {
                let code = running.sync.code().to_owned();
                if let Some(driven) = self.driven.lock().clone() {
                    running.sync.control_with(driven, arrangement.trust);
                }
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
struct Settling {
    read_at: Option<SystemTime>,
    moved_to: Option<Option<SystemTime>>,
}

#[cfg(feature = "sync")]
impl Settling {
    fn from(read_at: Option<SystemTime>) -> Self {
        Self {
            read_at,
            moved_to: None,
        }
    }

    fn settled_at(&mut self, stamp: Option<SystemTime>) -> bool {
        if stamp == self.read_at {
            self.moved_to = None;
            return false;
        }
        if self.moved_to != Some(stamp) {
            self.moved_to = Some(stamp);
            return false;
        }
        self.read_at = stamp;
        self.moved_to = None;
        true
    }
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
            let mut stamps = Settling::from(seen);
            let mut arranged = arranged;
            loop {
                match stopped.recv_timeout(every) {
                    Err(RecvTimeoutError::Timeout) => {}
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
                }
                if !stamps.settled_at(modified(&settings)) {
                    continue;
                }
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

    #[cfg(feature = "sync")]
    pub(crate) fn drive(&self, driven: resonate_driven::Driven) {
        let driven = Arc::new(driven) as Arc<dyn Controlled>;
        *self.service.driven.lock() = Some(Arc::clone(&driven));
        let trust = *self.service.trust.lock();
        if let Some(running) = self.service.running.lock().as_ref() {
            running.sync.control_with(driven, trust);
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
        driven: Mutex::new(None),
        trust: Mutex::new(RemoteTrust::default()),
    });
    let arranged = service.arranged(config, settings, library);
    #[cfg(feature = "online")]
    {
        let reached = Arc::downgrade(&service);
        let asked = crate::musicbrainz::AskedOfDevices(move |reach: &mut dyn FnMut(&Sync)| {
            let running = reached
                .upgrade()
                .and_then(|service| service.running.lock().clone());
            if let Some(running) = running {
                reach(&running.sync);
            }
        });
        crate::musicbrainz::consulting_through(Some(Arc::new(
            resonate_musicbrainz::Consulting::new(asked),
        )));
    }
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
    let folder = config::private_data_dir()?;
    let ledger = folder.join(LEDGER);
    config::private_store(&ledger)?;
    let settings = Settings {
        server: server_of(config),
        trusted: config.sync_server_certificate.clone(),
        stun: config.sync_stun.clone(),
        key,
        me: Me::called(
            config.sync_device.as_deref().unwrap_or_default(),
            &host_name(),
            THIS_PLATFORM,
            DEVICE_NAME,
        ),
        follow,
        ledger,
        staging: folder.join(STAGING),
        transfers_allowed: true,
        relay_only: false,
        nearby: Some(Nearby::on_the_network()),
    };
    let catalog = Arc::new(Catalogued {
        library: Arc::clone(library),
    });
    let sync = Sync::start(settings, catalog).map_err(Error::Sync)?;
    #[cfg(feature = "online")]
    sync.consult_with(crate::musicbrainz::ConsultedHere::of(config));
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
pub(crate) fn server_of(config: &Config) -> Server {
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
    let folder = config::private_data_dir()?;
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
fn local_query(query: SyncedQuery) -> LocalQuery {
    LocalQuery {
        at: LocalPlaylistId {
            id: query.id.get().cast_signed(),
            created: query.created,
        },
        body: QueryBody {
            name: query.body.name,
            text: query.body.text,
            sort: query.body.sort,
            reading: query.body.reading,
            rows: query.body.rows,
        },
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

    fn keeps_queries(&self) -> bool {
        true
    }

    fn queries(&self) -> Answered<Vec<LocalQuery>> {
        self.library
            .synced_queries()
            .map(|queries| queries.into_iter().map(local_query).collect())
            .map_err(|error| refused(&error))
    }

    fn write_query(&self, at: Option<LocalPlaylistId>, body: &QueryBody) -> Answered<LocalQuery> {
        let at = at
            .map(|at| playlist_id(at).map(|id| (id, at.created)))
            .transpose()?;
        let body = SyncedQueryBody {
            name: body.name.clone(),
            text: body.text.clone(),
            sort: body.sort,
            reading: body.reading,
            rows: body.rows,
        };
        self.library
            .write_synced_query(at, &body)
            .map_err(|error| refused(&error))?
            .map(local_query)
            .ok_or(Refused::Unwritable)
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
            .credit_listen_elsewhere(
                &listen.recording,
                listen.from.0.as_slice(),
                listen.id,
                listen.at,
                listen.heard,
            )
            .map_err(|error| refused(&error))
    }

    fn resumption(&self) -> Answered<Option<Resumed>> {
        let Some(resumption) = self.library.resumption().map_err(|error| refused(&error))? else {
            return Ok(None);
        };
        let playing = resumption.landing();
        let order = resumption.in_play_order();
        let window = Resumed::window_around(playing, order.len());
        let mut rows = Vec::new();
        let mut at = None;
        let mut playing_named = false;
        for place in window {
            let Some(resumable) = order
                .get(place)
                .and_then(|loaded| resumption.rows.get(*loaded))
            else {
                continue;
            };
            let recording = self
                .library
                .recording_at(&resumable.location, resumable.span)
                .map_err(|error| refused(&error))?;
            if let Some(recording) = recording {
                if place >= playing && at.is_none() {
                    at = Some(rows.len());
                    playing_named = place == playing;
                }
                rows.push(recording);
            }
        }
        let at = at.unwrap_or(rows.len());
        let heard = order
            .get(playing)
            .and_then(|loaded| resumption.rows.get(*loaded));
        let position = heard
            .and_then(|resumable| resumable.location.as_path().map(Path::to_path_buf))
            .and_then(|path| {
                let span = heard.and_then(|row| row.span);
                self.library.track_at(&path, span).ok().flatten()
            })
            .filter(|_| playing_named)
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
            .holds_recording(recording)
            .map_err(|error| refused(&error))
    }

    fn deleted_here(&self, recording: &Recording) -> Answered<bool> {
        self.library
            .deleted_recording(recording)
            .map_err(|error| refused(&error))
    }

    fn forgive_deletion(&self, recording: &Recording) -> Answered<()> {
        self.library
            .forgive_deleted_recording(recording)
            .map_err(|error| refused(&error))
    }

    fn forgive_deletions(&self) -> Answered<()> {
        self.library
            .forgive_deleted_recordings()
            .map_err(|error| refused(&error))
    }

    fn held_as(&self, recording: &Recording) -> Answered<Option<Recording>> {
        self.library
            .held_as(recording)
            .map_err(|error| refused(&error))
    }

    fn holdings(&self) -> Answered<Vec<Held>> {
        self.library
            .held_songs()
            .map(|songs| {
                songs
                    .into_iter()
                    .map(|song| Held {
                        extension: song.extension,
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
        let opened = self
            .library
            .open_song(&path)
            .map_err(|error| refused(&error))?;

        Ok(opened.map(|song| Opened {
            size: song.size,
            extension: song.extension.clone(),
            reader: Box::new(song),
        }))
    }

    fn tags(&self, recording: &Recording) -> Answered<SongTags> {
        let Some(path) = self
            .library
            .resolve_recording(recording)
            .map_err(|error| refused(&error))?
            .filter(|cut| cut.span.is_none())
            .and_then(|cut| cut.location.as_path().map(Path::to_path_buf))
        else {
            return Ok(SongTags::default());
        };
        let held = self
            .library
            .song_tags(&path)
            .map_err(|error| refused(&error))?;

        Ok(SongTags {
            album_artist: held.album_artist,
            date: held.date,
            track_number: held.track_number,
            track_total: held.track_total,
            disc_number: held.disc_number,
            disc_total: held.disc_total,
            compilation: held.compilation,
        })
    }

    fn land(
        &self,
        recording: &Recording,
        tags: &SongTags,
        extension: &str,
        staged: &Path,
    ) -> Answered<bool> {
        let mut file = File::open(staged).map_err(|_| Refused::Unwritable)?;
        let passed_on = HeldTags {
            album_artist: tags.album_artist.clone(),
            date: tags.date.clone(),
            track_number: tags.track_number,
            track_total: tags.track_total,
            disc_number: tags.disc_number,
            disc_total: tags.disc_total,
            compilation: tags.compilation,
        };
        match self
            .library
            .file_song_from_elsewhere(recording, &passed_on, extension, &mut file)
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

    fn takes_answers(&self) -> bool {
        true
    }

    fn answers_after(&self, mark: i64, most: usize) -> Answered<Vec<resonate_core::Answered>> {
        self.library
            .answers_to_share(mark, most)
            .map_err(|error| refused(&error))
    }

    fn take_answer(&self, recording: &Recording, answer: &Answer) -> Answered<bool> {
        self.library
            .take_shared_answer(recording, answer)
            .map_err(|error| refused(&error))
    }

    fn covers_wanted(&self, most: usize) -> Answered<Vec<Mbid>> {
        self.library
            .covers_wanted(most)
            .map_err(|error| refused(&error))
    }

    fn cover(&self, release: &Mbid) -> Answered<Option<Vec<u8>>> {
        self.library
            .cover_to_share(release)
            .map(|art| art.map(|art| art.bytes))
            .map_err(|error| refused(&error))
    }

    fn take_cover(&self, release: &Mbid, picture: &[u8]) -> Answered<bool> {
        self.library
            .take_shared_cover(release, picture)
            .map_err(|error| refused(&error))
    }

    fn lyricsfiles_wanted(&self, skip: usize, most: usize) -> Answered<Vec<Recording>> {
        self.library
            .lyricsfiles_wanted(skip, most)
            .map_err(|error| refused(&error))
    }

    fn lyricsfile(&self, recording: &Recording) -> Answered<Option<SharedLyricsfile>> {
        self.library
            .lyricsfile_to_share(recording)
            .map_err(|error| refused(&error))
    }

    fn take_lyricsfile(
        &self,
        recording: &Recording,
        lyricsfile: &SharedLyricsfile,
    ) -> Answered<bool> {
        self.library
            .take_shared_lyricsfile(recording, lyricsfile)
            .map_err(|error| refused(&error))
    }

    fn prints_wanted(&self, skip: usize, most: usize) -> Answered<Vec<WholeFile>> {
        self.library
            .prints_wanted(skip, most)
            .map_err(|error| refused(&error))
    }

    fn print(&self, file: WholeFile) -> Answered<Option<SharedPrint>> {
        self.library
            .print_to_share(file)
            .map_err(|error| refused(&error))
    }

    fn take_print(&self, file: WholeFile, print: &SharedPrint) -> Answered<bool> {
        self.library
            .take_shared_print(file, print)
            .map_err(|error| refused(&error))
    }

    fn hides_as_shared(&self, at: LocalPlaylistId) -> Answered<bool> {
        crate::together::hides_as_shared(&self.library, at)
    }

    fn shares_here(&self) -> Answered<Option<Vec<HeldShare>>> {
        Ok(crate::together::shares_here())
    }

    fn take_share(&self, share: &HeldShare, _taking_over: Option<LocalPlaylistId>) -> Answered<()> {
        if let Some(shared) = crate::together::running() {
            shared.adopt(share.clone());
        }
        Ok(())
    }

    fn drop_share(&self, code: &str) -> Answered<()> {
        if let Some(shared) = crate::together::running() {
            shared.drop_code(code.to_owned());
        }
        Ok(())
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
    Fetch {
        playlist: &'a str,
        from: &'a str,
    },
    Handoff {
        from: &'a str,
    },
    Control {
        device: &'a str,
        zone: Option<ZoneAsked>,
        command: Option<RemoteCommand>,
    },
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
            said!("{}", words::sync::joins(key.code()));
            Ok(None)
        }
        Asked::Leave => {
            config::clear(settings, ConfigKey::SyncKey)?;
            said!("{}", words::sync::left());
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
            said!("{}", words::sync::forgotten(device.name.as_str()));
            Ok(None)
        }
        Asked::Standing => {
            let library = library.ok_or(Error::SyncOff)?;
            let running = running(config, settings, &library)?;
            let sync = &running.sync;
            said!("{}\t{}", words::sync::pairing_code(), sync.code());
            let server = words::sync::server();
            match server_of(config) {
                Server::Nowhere => said!("{server}\t{}", words::sync::no_server()),
                Server::At(at) => said!("{server}\t{at}"),
                Server::Hosted => said!("{server}\t{}", resonate_sync::HOSTED_SERVER),
            }
            let standing = connected(sync);
            said!("{}\t{}", words::sync::standing(), standing_of(standing));
            for device in sync.shown().devices {
                let reached = match (device.nearby, device.online) {
                    (true, _) => words::sync::on_this_network(),
                    (false, true) => words::sync::online(),
                    (false, false) => words::sync::away(),
                };
                said!(
                    "{}\t{}",
                    device.name,
                    if device.this {
                        words::sync::this_device(reached)
                    } else {
                        reached.to_owned()
                    }
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
                said!("{}", words::sync::lacks_nothing(from, playlist));
                return Ok(None);
            }
            let landed = fetched_in_batches(
                &missing,
                |batch| sync.fetch(batch, device.id),
                |batch| transferred(sync, batch),
            );
            said!("{}", words::sync::came_from(from, landed, missing.len()));
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
        Asked::Control {
            device,
            zone,
            command,
        } => {
            let library = library.ok_or(Error::SyncOff)?;
            let running = running(config, settings, &library)?;
            controlled(&running.sync, device, zone.as_ref(), command)?;
            Ok(None)
        }
    }
}

#[cfg(feature = "sync")]
fn mirrored(sync: &Sync, device: DeviceId, named: &str, since: Instant) -> Result<Mirror> {
    let deadline = Instant::now() + CONNECTED_WITHIN;
    loop {
        sync.watch(device);
        match sync.watched(device) {
            Watched::Mirrored(mirror) if mirror.heard_at >= since => return Ok(mirror),
            Watched::Refused(answer) => {
                return Err(Error::NotControlled {
                    device: named.into(),
                    answer,
                });
            }
            Watched::Waiting | Watched::Mirrored(_) => {}
        }
        if Instant::now() >= deadline {
            return Err(Error::NothingMirrored(named.into()));
        }
        thread::sleep(TRANSFERS_LOOKED_AT_EVERY);
    }
}

#[cfg(feature = "sync")]
fn zone_asked<'a>(
    mirror: &'a Mirror,
    named: &str,
    zone: Option<&ZoneAsked>,
) -> Result<&'a RemoteZone> {
    let found = match zone {
        None => mirror.zones.first(),
        Some(ZoneAsked::Numbered(id)) => mirror.zone(*id),
        Some(ZoneAsked::Named(name)) => mirror
            .zones
            .iter()
            .find(|held| held.name.to_lowercase() == name.to_lowercase()),
    };
    found.ok_or_else(|| Error::NoSuchRemoteZone {
        device: named.into(),
        zone: zone.cloned().unwrap_or(ZoneAsked::Numbered(ZoneId::FIRST)),
    })
}

#[cfg(feature = "sync")]
fn controlled(
    sync: &Sync,
    named: &str,
    zone: Option<&ZoneAsked>,
    command: Option<RemoteCommand>,
) -> Result<()> {
    let _ = connected(sync);
    let device = device_named(sync, named)?;
    let mirror = mirrored(
        sync,
        device.id,
        &device.name,
        Instant::now() - CONNECTED_WITHIN,
    )?;
    let Some(command) = command else {
        let shown: Vec<&RemoteZone> = match zone {
            Some(_) => vec![zone_asked(&mirror, &device.name, zone)?],
            None => mirror.zones.iter().collect(),
        };
        for held in shown {
            said_zone(&mirror, held);
        }
        return Ok(());
    };
    let asked_of = zone_asked(&mirror, &device.name, zone)?;
    let (target, drawn) = (asked_of.zone, asked_of.queue_revision);
    let asked_at = Instant::now();
    match sync
        .ask_to_control(device.id, target, command, drawn)
        .answer()
    {
        Controlling::Done => {}
        answer => {
            return Err(Error::NotControlled {
                device: device.name.as_str().into(),
                answer,
            });
        }
    }
    let after = mirrored(sync, device.id, &device.name, asked_at)?;
    if let Some(held) = after.zone(target) {
        said_zone(&after, held);
    }
    Ok(())
}

#[cfg(feature = "sync")]
fn said_zone(mirror: &Mirror, zone: &RemoteZone) {
    said!("{}\t{} {}", words::sync::zone(), zone.zone, zone.name);
    said!("{}\t{}", words::sync::state(), playback_word(zone.playback));
    if let Some(row) = zone.playing_row() {
        let place = clock_of(mirror.position_of(zone, Instant::now()));
        let length = zone
            .length
            .map_or_else(String::new, |length| format!(" / {}", clock_of(length)));
        said!(
            "{}\t{}\t{place}{length}",
            words::sync::playing(),
            billed(row)
        );
    }
    said!(
        "{}\t{}",
        words::sync::volume(),
        words::sync::percent((u32::from(zone.volume.get()) + 5) / 10)
    );
    said!(
        "{}\t{}",
        words::sync::queue(),
        words::sync::songs(zone.queue_length)
    );
    for (at, row) in zone.rows.iter().enumerate() {
        let number = zone.first_row + at;
        let mark = if zone.playing == Some(number) {
            ">"
        } else {
            " "
        };
        said!("{mark}{}\t{}", number + 1, billed(row));
    }
}

#[cfg(feature = "sync")]
fn billed(row: &RemoteRow) -> String {
    if row.artist.is_empty() {
        row.title.clone()
    } else {
        words::sync::billed(&row.artist, &row.title)
    }
}

#[cfg(feature = "sync")]
fn playback_word(playback: RemotePlayback) -> &'static str {
    match playback {
        RemotePlayback::Idle => words::sync::idle(),
        RemotePlayback::Playing => words::sync::playing_now(),
        RemotePlayback::Paused => words::sync::paused(),
        RemotePlayback::Stopped => words::sync::stopped(),
    }
}

#[cfg(feature = "sync")]
fn clock_of(length: Duration) -> String {
    let seconds = length.as_secs();
    let (hours, minutes, seconds) = (seconds / 3_600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
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
        Standing::Starting | Standing::Connecting => words::sync::connecting(),
        Standing::Connected => words::sync::connected(),
        Standing::Unreachable => words::sync::unreachable(),
        Standing::Refused(_) => words::sync::refused(),
        Standing::Nearby => words::sync::nearby(),
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
fn fetched_in_batches(
    missing: &[Recording],
    mut fetch: impl FnMut(Vec<Recording>),
    mut wait: impl FnMut(&[Recording]) -> usize,
) -> usize {
    missing
        .chunks(FETCHED_AT_A_TIME)
        .map(|batch| {
            fetch(batch.to_vec());
            wait(batch)
        })
        .sum()
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

    fn watch(&self, device: &str) -> resonate_ui::RemoteShown {
        self.with(resonate_ui::RemoteShown::Refused, |running| {
            resonate_ui::Syncing::watch(running, device)
        })
    }

    fn unwatch(&self, device: &str) {
        self.with((), |running| resonate_ui::Syncing::unwatch(running, device));
    }

    fn press(
        &self,
        device: &str,
        zone: u32,
        key: resonate_ui::RemoteKey,
        drawn: Option<u64>,
    ) -> bool {
        self.with(false, |running| {
            resonate_ui::Syncing::press(running, device, zone, key, drawn)
        })
    }
}

#[cfg(all(feature = "sync", feature = "ui"))]
impl Running {
    fn other_device(&self, device: &str) -> Option<DeviceId> {
        self.sync
            .shown()
            .devices
            .into_iter()
            .find(|known| !known.this && known.id.to_string() == device)
            .map(|known| known.id)
    }
}

#[cfg(all(feature = "sync", feature = "ui"))]
fn zone_shown(mirror: &Mirror, zone: &RemoteZone) -> resonate_ui::RemoteZoneShown {
    resonate_ui::RemoteZoneShown {
        zone: zone.zone.get(),
        name: zone.name.clone(),
        song: zone.playing_row().map(billed),
        sounding: zone.playback == RemotePlayback::Playing,
        position: mirror.position_of(zone, Instant::now()),
        length: zone.length,
        volume: zone.volume.get(),
        playing: zone.playing,
        queue_length: zone.queue_length,
        first_row: zone.first_row,
        queue_revision: zone.queue_revision,
        rows: zone
            .rows
            .iter()
            .map(|row| resonate_ui::RemoteRowShown {
                title: row.title.clone(),
                artist: row.artist.clone(),
                length: row.length,
            })
            .collect(),
    }
}

#[cfg(all(feature = "sync", feature = "ui"))]
const fn command_of(key: resonate_ui::RemoteKey) -> RemoteCommand {
    match key {
        resonate_ui::RemoteKey::Toggle => RemoteCommand::Toggle,
        resonate_ui::RemoteKey::Next => RemoteCommand::Next,
        resonate_ui::RemoteKey::Previous => RemoteCommand::Previous,
        resonate_ui::RemoteKey::Volume(permille) => {
            RemoteCommand::Volume(resonate_sync::Permille::new(permille))
        }
        resonate_ui::RemoteKey::Seek(at) => RemoteCommand::Seek(at),
        resonate_ui::RemoteKey::Jump(row) => RemoteCommand::Jump(row),
        resonate_ui::RemoteKey::Move { from, to } => RemoteCommand::Move { from, to },
        resonate_ui::RemoteKey::Remove(row) => RemoteCommand::Remove(row),
    }
}

#[cfg(all(feature = "sync", feature = "ui"))]
impl resonate_ui::Syncing for Running {
    fn watch(&self, device: &str) -> resonate_ui::RemoteShown {
        let Some(id) = self.other_device(device) else {
            return resonate_ui::RemoteShown::Refused;
        };
        self.sync.watch(id);
        match self.sync.watched(id) {
            Watched::Waiting => resonate_ui::RemoteShown::Waiting,
            Watched::Refused(_) => resonate_ui::RemoteShown::Refused,
            Watched::Mirrored(mirror) => resonate_ui::RemoteShown::Zones(
                mirror
                    .zones
                    .iter()
                    .map(|zone| zone_shown(&mirror, zone))
                    .collect(),
            ),
        }
    }

    fn unwatch(&self, device: &str) {
        if let Some(id) = self.other_device(device) {
            self.sync.unwatch(id);
        }
    }

    fn press(
        &self,
        device: &str,
        zone: u32,
        key: resonate_ui::RemoteKey,
        drawn: Option<u64>,
    ) -> bool {
        let (Some(id), Some(zone)) = (self.other_device(device), ZoneId::new(zone)) else {
            return false;
        };
        self.sync
            .ask_to_control(id, zone, command_of(key), drawn)
            .answer()
            == Controlling::Done
    }

    fn force(&self) {
        self.sync.force();
    }

    fn shown(&self) -> resonate_ui::SyncShown {
        let shown = self.sync.shown();
        let downloading = shown.downloading();
        let waiting = shown.waiting();
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
                        Platform::Windows => resonate_ui::SyncedPlatform::Windows,
                        Platform::MacOs => resonate_ui::SyncedPlatform::MacOs,
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
            downloading,
            waiting,
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

#[cfg(all(feature = "sync", unix))]
fn host_name() -> String {
    let named = rustix::system::uname();
    let named = named.nodename().to_string_lossy();

    match cfg!(target_os = "macos") {
        true => named_without_its_domain(&named).to_owned(),
        false => named.trim().to_owned(),
    }
}

#[cfg(all(feature = "sync", any(unix, test)))]
fn named_without_its_domain(host: &str) -> &str {
    let host = host.trim();
    host.strip_suffix(THE_LOCAL_DOMAIN).unwrap_or(host)
}

#[cfg(all(feature = "sync", windows))]
fn host_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_default()
}

#[cfg(all(test, feature = "sync"))]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn a_mac_is_named_as_its_host_without_the_domain_bonjour_gives_it() {
        assert_eq!(named_without_its_domain("studio.local\n"), "studio");
        assert_eq!(named_without_its_domain("studio"), "studio");
        assert_eq!(
            named_without_its_domain("studio.example.org"),
            "studio.example.org"
        );
        assert_eq!(named_without_its_domain(""), "");
    }

    fn recording(title: usize) -> Recording {
        Recording {
            mbid: None,
            title: title.to_string(),
            artist: "Artist".into(),
            album: "Album".into(),
            length: None,
        }
    }

    #[test]
    fn a_fetch_asks_for_no_more_songs_at_once_than_the_transfer_list_keeps() {
        let missing = (0..250).map(recording).collect::<Vec<_>>();
        let asked = std::cell::RefCell::new(Vec::new());

        let landed = fetched_in_batches(
            &missing,
            |batch| asked.borrow_mut().push(batch.len()),
            |batch| batch.len(),
        );

        assert_eq!(landed, 250);
        assert_eq!(*asked.borrow(), [100, 100, 50]);
    }

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
        assert!(moved(|config| {
            config.sync_nearby_only = Some(false);
            config.sync_server = Some("example.org".to_owned());
        }));
        assert!(moved(|config| config.sync_device = Some("Study".to_owned())));
        assert!(moved(
            |config| config.sync_stun = Some("stun.example.org".to_owned())
        ));
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

    fn written_whole(settings: &Path, text: &str) {
        let staged = settings.with_extension("staged");
        fs::write(&staged, text).expect("a file");
        fs::rename(&staged, settings).expect("the file put in place");
    }

    #[test]
    fn a_settings_file_is_read_only_once_its_stamp_has_held_still_for_a_look() {
        let first = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
        let truncated = first + Duration::from_secs(1);
        let written = truncated + Duration::from_millis(1);
        let mut stamps = Settling::from(Some(first));

        let unmoved = stamps.settled_at(Some(first));
        let caught_truncated = stamps.settled_at(Some(truncated));
        let caught_written = stamps.settled_at(Some(written));
        let held_still = stamps.settled_at(Some(written));
        let read_again = stamps.settled_at(Some(written));
        let removed = stamps.settled_at(None);
        let still_removed = stamps.settled_at(None);

        assert!(!unmoved);
        assert!(!caught_truncated);
        assert!(!caught_written);
        assert!(held_still);
        assert!(!read_again);
        assert!(!removed);
        assert!(still_removed);
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

        written_whole(
            &settings,
            &format!("sync-key = \"{}\"\nvolume = 0.5\n", held.code()),
        );
        thread::sleep(Duration::from_millis(300));
        let unmoved = rearranged.load(Ordering::Acquire);

        written_whole(&settings, &format!("sync-key = \"{}\"\n", joined.code()));
        let moved = wait_for(&rearranged, 1);

        written_whole(&settings, "sync-key = [\n");
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

    #[test]
    fn a_hidden_song_is_held_so_no_follow_fetches_it_again_and_one_deleted_by_hand_is_not() {
        let (folder, library) = held_song();
        let path = folder.join("held.wav");
        let track = library
            .track_at(&path, None)
            .expect("read")
            .expect("catalogued");
        library.hide_track(track.id, true).expect("hidden");
        let catalog = Catalogued {
            library: Arc::new(library),
        };

        let hidden = catalog.holds(&heard("Held")).expect("asked");
        fs::remove_file(&path).expect("the song goes");
        let deleted = catalog.holds(&heard("Held")).expect("asked");

        let _ = fs::remove_dir_all(&folder);
        assert!(hidden);
        assert!(!deleted);
    }

    #[test]
    fn a_song_deleted_through_the_library_is_marked_for_the_sync_and_forgiven_by_it() {
        let (folder, library) = held_song();
        let trash = folder.join("trash");
        fs::create_dir_all(&trash).expect("a trash");
        library.trash_into(Some(trash));
        let track = library
            .track_at(&folder.join("held.wav"), None)
            .expect("read")
            .expect("catalogued");
        let catalog = Catalogued {
            library: Arc::new(library),
        };

        let before = catalog.deleted_here(&heard("Held")).expect("asked");
        catalog.library.delete_tracks(&[track.id]).expect("deleted");
        let after = catalog.deleted_here(&heard("Held")).expect("asked");
        let held = catalog.holds(&heard("Held")).expect("asked");
        catalog.forgive_deletions().expect("forgiven");
        let forgiven = catalog.deleted_here(&heard("Held")).expect("asked");

        let _ = fs::remove_dir_all(&folder);
        assert!(!before);
        assert!(after);
        assert!(!held);
        assert!(!forgiven);
    }

    fn kept_songs(titles: &[&str]) -> (PathBuf, Library, Vec<resonate_core::MediaLocation>) {
        use crate::fixtures::{ALBUM, ARTIST, TITLE, scanned, scratch, tagged_wave};

        let folder = scratch("sync-handoff-order");
        for title in titles {
            fs::write(
                folder.join(format!("{title}.wav")),
                tagged_wave(&[(TITLE, title), (ARTIST, "Artist"), (ALBUM, "Album")]),
            )
            .expect("a song");
        }
        let library = scanned(&folder);
        let locations = titles
            .iter()
            .map(|title| resonate_core::MediaLocation::local(folder.join(format!("{title}.wav"))))
            .collect();
        (folder, library, locations)
    }

    fn named(entries: &[Option<Recording>]) -> Vec<Option<&str>> {
        entries
            .iter()
            .map(|entry| entry.as_ref().map(|recording| recording.title.as_str()))
            .collect()
    }

    #[test]
    fn a_song_from_another_device_named_otherwise_stays_in_its_playlist_and_favourites_and_settles_once()
     {
        use std::io::Cursor;

        use resonate_library::{DeliveryFolder, Layout};
        use resonate_sync::{Change, Ledger, SpaceId};

        use crate::fixtures::{ALBUM, ARTIST, TITLE, tagged_wave};

        let (folder, there, locations) = kept_songs(&["San Tropez (2011 Remaster)"]);
        there
            .start_playlist("Mix", &[Cut::whole(locations[0].clone())])
            .expect("a playlist");
        let sender = Catalogued {
            library: Arc::new(there),
        };
        let asked = sender.playlists().expect("the playlists")[0].entries[0]
            .clone()
            .expect("a named song");
        sender.favour(&asked, true).expect("a favourite");
        let received = folder.join("received");
        fs::create_dir_all(&received).expect("a music folder");
        let here = Library::open_in_memory().expect("a catalog");
        here.deliver_into(Some(DeliveryFolder {
            path: received,
            layout: Layout::default(),
        }));
        let filed = here
            .file_song_from_elsewhere(
                &asked,
                &HeldTags::default(),
                "wav",
                &mut Cursor::new(tagged_wave(&[
                    (TITLE, "San Tropez"),
                    (ARTIST, "Artist"),
                    (ALBUM, "Album"),
                ])),
            )
            .expect("a song filed");
        let receiver = Catalogued {
            library: Arc::new(here),
        };
        let me = |name: &str| Me {
            name: name.into(),
            platform: Platform::Linux,
        };
        let ledger = || {
            let mut ledger = Ledger::in_memory().expect("a ledger");
            ledger.enter_space(SpaceId([7; 16])).expect("a space");
            ledger
        };
        let mut sending = ledger();
        let mut receiving = ledger();

        sending.look(&sender, &me("Desk")).expect("a look");
        let told = sending
            .owed()
            .expect("owed")
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        receiving.arrived(&receiver, &told).expect("arrived");
        let written_at = receiver.revision();
        let settled = receiving.settle(&receiver).expect("settled");
        receiving.look(&receiver, &me("Laptop")).expect("a look");
        let echoed = receiving
            .owed()
            .expect("owed")
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let settled_again = receiving.settle(&receiver).expect("settled");

        let _ = fs::remove_dir_all(&folder);
        assert!(matches!(filed, SongFiled::Filed(_)), "{filed:?}");
        assert_eq!((settled, settled_again), (0, 0));
        assert_eq!(receiver.revision(), written_at);
        assert_eq!(
            named(&receiver.playlists().expect("the playlists")[0].entries),
            [Some("San Tropez")]
        );
        assert_eq!(
            receiver
                .favourites()
                .expect("the favourites")
                .iter()
                .map(|recording| recording.title.as_str())
                .collect::<Vec<_>>(),
            ["San Tropez"]
        );
        assert!(
            !echoed
                .iter()
                .any(|change| matches!(change, Change::Playlist(_) | Change::Favourite(_))),
            "{echoed:?}"
        );
    }

    fn handed_titles(resumed: &Resumed) -> Vec<&str> {
        resumed.rows.iter().map(|row| row.title.as_str()).collect()
    }

    #[test]
    fn a_shuffled_queue_is_handed_off_in_the_order_it_plays_at_the_song_heard() {
        let (folder, library, locations) = kept_songs(&["A", "B", "C"]);
        let rows = locations
            .into_iter()
            .map(|location| resonate_core::Resumable {
                location,
                span: None,
                held: None,
            })
            .collect();
        library
            .keep_resumption(
                resonate_core::ZoneId::FIRST,
                &resonate_core::Resumption {
                    rows,
                    order: vec![2, 0, 1],
                    row: 1,
                    at: Frames(3 * 44_100),
                    shuffle: true,
                    next: None,
                },
            )
            .expect("a resumption kept");
        let catalogued = Catalogued {
            library: Arc::new(library),
        };

        let resumed = catalogued
            .resumption()
            .expect("answered")
            .expect("a resumption");

        let _ = fs::remove_dir_all(&folder);
        assert_eq!(handed_titles(&resumed), ["C", "A", "B"]);
        assert_eq!(resumed.at, 1);
        assert_eq!(resumed.position, Duration::from_secs(3));
    }

    #[test]
    fn a_song_heard_that_the_catalog_cannot_name_hands_off_the_next_one_named_from_its_start() {
        let (folder, library, locations) = kept_songs(&["A", "C"]);
        let unscanned = resonate_core::MediaLocation::local(folder.join("B.wav"));
        let rows = [locations[0].clone(), unscanned, locations[1].clone()]
            .into_iter()
            .map(|location| resonate_core::Resumable {
                location,
                span: None,
                held: None,
            })
            .collect();
        library
            .keep_resumption(
                resonate_core::ZoneId::FIRST,
                &resonate_core::Resumption {
                    rows,
                    order: vec![0, 1, 2],
                    row: 1,
                    at: Frames(3 * 44_100),
                    shuffle: false,
                    next: None,
                },
            )
            .expect("a resumption kept");
        let catalogued = Catalogued {
            library: Arc::new(library),
        };

        let resumed = catalogued
            .resumption()
            .expect("answered")
            .expect("a resumption");

        let _ = fs::remove_dir_all(&folder);
        assert_eq!(handed_titles(&resumed), ["A", "C"]);
        assert_eq!(resumed.at, 1);
        assert_eq!(resumed.position, Duration::ZERO);
    }
}
