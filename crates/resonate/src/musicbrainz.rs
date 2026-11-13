#[cfg(feature = "ui")]
use std::time::{SystemTime, UNIX_EPOCH};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
    thread,
    time::{Duration, Instant},
};

#[cfg(feature = "ui")]
use parking_lot::Mutex;
use parking_lot::RwLock;
#[cfg(feature = "ui")]
use resonate_core::Gate;
use resonate_library::{RecordingAsked, Reference, Wording};
#[cfg(feature = "sync")]
use resonate_musicbrainz::Consultation;
use resonate_musicbrainz::{
    Answer, Archives, Asked as Lookup, Building, KEPT_FILE, Keeping, Kept, Layered, Mirror,
    Mirrored, Peers, Phase, Replicated, built_from,
    download::{self, CORE_ARCHIVE, DERIVED_ARCHIVE},
    held_for_building, mirror_in, removed_beside_the_mirror, replicated, settled,
};

use crate::{Error, Result, config, config::Config, metered, words};

const FOLDER: &str = "musicbrainz";
const GIVEN_DUMP: &str = "given";
#[cfg(feature = "ui")]
pub const LOOKED_AT_AGAIN_AFTER: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const TOLD_EVERY: Duration = Duration::from_secs(5);
const RECORDINGS_SHOWN: usize = 10;
const BYTES_IN_A_GIGABYTE: f64 = 1_000_000_000.0;
#[cfg(feature = "ui")]
pub const FOLLOWED_EVERY: Duration = Duration::from_secs(60 * 60);
#[cfg(feature = "ui")]
const FOLLOWING_LOOKED_AT_EVERY: Duration = Duration::from_secs(60);

static KEPT: OnceLock<Option<Arc<Kept>>> = OnceLock::new();
static MIRRORED: OnceLock<Arc<Mirrored>> = OnceLock::new();
static PEERS: RwLock<Option<Arc<dyn Peers>>> = RwLock::new(None);
#[cfg(feature = "ui")]
static BUILDING: Mutex<Option<Arc<Building>>> = Mutex::new(None);
#[cfg(feature = "ui")]
static FAILED: Mutex<Option<String>> = Mutex::new(None);
#[cfg(feature = "ui")]
static FOLLOWING: Mutex<Option<Arc<Building>>> = Mutex::new(None);

fn failed(error: resonate_musicbrainz::Error) -> Error {
    Error::Musicbrainz(Box::new(error))
}

pub fn folder() -> Result<PathBuf> {
    Ok(config::data_dir()?.join(FOLDER))
}

pub fn kept() -> Option<Arc<Kept>> {
    KEPT.get_or_init(|| {
        let path = folder().ok()?.join(KEPT_FILE);
        match Kept::open(&path) {
            Ok(kept) => Some(Arc::new(kept)),
            Err(error) => {
                tracing::warn!(%error, "the kept lookup answers could not be opened");
                None
            }
        }
    })
    .clone()
}

fn held_mirror() -> Option<Arc<Mirror>> {
    let path = mirror_in(&folder().ok()?);
    if !path.is_file() {
        return None;
    }
    match Mirror::open(&path) {
        Ok(mirror) => Some(Arc::new(mirror)),
        Err(error) => {
            tracing::warn!(%error, "the copy of MusicBrainz could not be opened");
            None
        }
    }
}

fn current(config: &Config) -> Config {
    config::load(config.read_from.as_deref()).unwrap_or_else(|_| config.clone())
}

#[cfg(feature = "ui")]
fn wanted(config: &Config) -> bool {
    let current = current(config);
    current.mirrors_musicbrainz() && current.online_enabled()
}

fn landed(config: &Config, mirrored: &Mirrored, held: impl FnOnce() -> Option<Arc<Mirror>>) {
    if current(config).mirrors_musicbrainz() {
        mirrored.hold(held());
    }
}

pub fn mirrored(config: &Config) -> Arc<Mirrored> {
    Arc::clone(MIRRORED.get_or_init(|| {
        let mirrored = Mirrored::default();
        if config.mirrors_musicbrainz() {
            mirrored.hold(held_mirror());
        }
        Arc::new(mirrored)
    }))
}

#[cfg(feature = "ui")]
pub fn follow(config: &Config, on: bool) {
    let mirrored = mirrored(config);
    if on {
        if mirrored.held().is_none() {
            mirrored.hold(held_mirror());
        }
        keep_fresh(config);
    } else {
        stop_building();
        mirrored.hold(None);
    }
}

#[cfg(feature = "ui")]
pub fn stop_building() {
    if let Some(building) = BUILDING.lock().as_ref() {
        building.cancel();
    }
    if let Some(following) = FOLLOWING.lock().as_ref() {
        following.cancel();
    }
}

struct Registered;

impl Peers for Registered {
    fn consult(&self, asked: &Lookup) -> Option<Answer> {
        let peers = PEERS.read().clone()?;
        peers.consult(asked)
    }
}

#[cfg(feature = "sync")]
pub fn consulting_through(peers: Option<Arc<dyn Peers>>) {
    *PEERS.write() = peers;
}

#[cfg(feature = "sync")]
pub fn consultation(config: &Config) -> Consultation {
    Consultation::new(Some(mirrored(config)), kept())
}

pub fn layered(
    config: &Config,
    fallback: Option<Arc<dyn Reference>>,
) -> Option<Arc<dyn Reference>> {
    let mirrored = mirrored(config);
    if fallback.is_none() && mirrored.held().is_none() {
        return None;
    }
    Some(Arc::new(
        Layered::new()
            .keeping(kept())
            .mirrored(Some(mirrored))
            .consulting(cfg!(feature = "sync").then(|| Arc::new(Registered) as Arc<dyn Peers>))
            .falling_back_on(fallback),
    ))
}

fn given(from: &Path) -> Archives {
    let dump = from
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| download::is_a_dump_name(name))
        .unwrap_or_else(|| GIVEN_DUMP.to_owned());
    Archives {
        core: from.join(CORE_ARCHIVE),
        derived: from.join(DERIVED_ARCHIVE),
        dump,
    }
}

fn keeping(config: &Config) -> Keeping {
    if current(config).musicbrainz_replication_token().is_some() {
        Keeping::TheMirrorAndItsTables
    } else {
        Keeping::TheMirror
    }
}

pub fn build(config: &Config, from: Option<&Path>, building: &Building) -> Result<PathBuf> {
    if from.is_none() && !current(config).online_enabled() {
        return Err(Error::OnlineOff);
    }
    let keeping = keeping(config);
    let folder = folder()?;
    let held = held_for_building(&folder).map_err(failed)?;
    let archives = match from {
        Some(from) => given(from),
        None => {
            let agent = download::agent();
            let latest = download::latest(&agent).map_err(failed)?;
            let gate = metered::for_downloads(config);
            download::fetched(&agent, &latest, &folder, &gate, building, &held).map_err(failed)?
        }
    };
    let built = built_from(&archives, &folder, keeping, building, &held).map_err(failed)?;
    let mirrored = mirrored(config);
    mirrored.hold(None);
    let path = settled(&built, &folder).map_err(failed)?;
    if from.is_none() {
        download::forget_downloads(&folder);
    }
    landed(config, &mirrored, held_mirror);
    Ok(path)
}

fn token_of(config: &Config) -> Result<String> {
    let token = current(config)
        .musicbrainz_replication_token()
        .map(str::to_owned)
        .ok_or(Error::NoReplicationToken)?;
    if !download::is_a_replication_token(&token) {
        return Err(Error::MalformedReplicationToken);
    }
    Ok(token)
}

pub fn replicate(config: &Config, following: &Building) -> Result<Replicated> {
    if !current(config).online_enabled() {
        return Err(Error::OnlineOff);
    }
    let token = token_of(config)?;
    let mirror = held_mirror().ok_or(Error::NoMusicbrainzCopy)?;
    let folder = folder()?;
    let held = held_for_building(&folder).map_err(failed)?;
    let agent = download::agent();
    let done = replicated(
        &folder,
        &mirror,
        |sequence| download::packet(&agent, &token, sequence),
        following,
        &held,
    )
    .map_err(failed)?;
    drop(held);
    if done.to != done.from {
        landed(config, &mirrored(config), held_mirror);
    }
    Ok(done)
}

#[cfg(feature = "ui")]
fn follows_the_changes(config: &Config) -> bool {
    let current = current(config);
    current.mirrors_musicbrainz()
        && current.online_enabled()
        && current.musicbrainz_replication_token().is_some()
}

#[cfg(feature = "ui")]
pub fn follow_the_changes(config: &Config) {
    let mut current = FOLLOWING.lock();
    if current.is_some() || !follows_the_changes(config) {
        return;
    }
    let following = Arc::new(Building::default());
    *current = Some(Arc::clone(&following));
    let config = config.clone();
    let spawned = thread::Builder::new()
        .name("resonate-musicbrainz-follow".to_owned())
        .spawn(move || {
            let lookups = metered::for_lookups(&config);
            let mut last: Option<Instant> = None;
            while !following.is_cancelled() && follows_the_changes(&config) {
                let due = last.is_none_or(|last| last.elapsed() >= FOLLOWED_EVERY);
                let ready = BUILDING.lock().is_none()
                    && !lookups.is_shut()
                    && folder().is_ok_and(|folder| {
                        folder.join(resonate_musicbrainz::TABLES_FILE).is_file()
                    });
                if due && ready {
                    last = Some(Instant::now());
                    match replicate(&config, &following) {
                        Ok(done) if done.to != done.from => tracing::info!(
                            to = done.to,
                            changes = done.changes,
                            documents = done.documents,
                            "the copy of MusicBrainz follows the changes published"
                        ),
                        Ok(_) => {}
                        Err(error) if was_cancelled(&error) => break,
                        Err(error) => {
                            tracing::warn!(%error, "MusicBrainz's changes could not be followed");
                        }
                    }
                }
                thread::sleep(FOLLOWING_LOOKED_AT_EVERY);
            }
            *FOLLOWING.lock() = None;
        });
    if let Err(error) = spawned {
        tracing::debug!(%error, "MusicBrainz's changes could not begin to be followed");
        *current = None;
    }
}

#[cfg(feature = "ui")]
pub fn build_in_the_background(config: &Config) -> bool {
    let mut current = BUILDING.lock();
    if current.is_some() {
        return false;
    }
    let building = Arc::new(Building::default());
    *current = Some(Arc::clone(&building));
    *FAILED.lock() = None;
    let config = config.clone();
    let spawned = thread::Builder::new()
        .name("resonate-musicbrainz".to_owned())
        .spawn(move || {
            match build(&config, None, &building) {
                Ok(path) => {
                    tracing::info!(path = %path.display(), "the copy of MusicBrainz is built")
                }
                Err(error) if was_cancelled(&error) => {
                    tracing::info!("building the copy of MusicBrainz was stopped");
                }
                Err(error) => {
                    tracing::warn!(%error, "the copy of MusicBrainz could not be built");
                    *FAILED.lock() = Some(said_of(&error));
                }
            }
            *BUILDING.lock() = None;
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "the copy of MusicBrainz could not begin to be built");
        *current = None;
        return false;
    }
    true
}

#[cfg(feature = "ui")]
fn was_cancelled(error: &Error) -> bool {
    matches!(
        error,
        Error::Musicbrainz(inner) if matches!(**inner, resonate_musicbrainz::Error::Cancelled)
    )
}

#[cfg(feature = "ui")]
fn said_of(error: &Error) -> String {
    let mut said = error.to_string();
    let mut cause = std::error::Error::source(error);
    while let Some(under) = cause {
        said.push_str(": ");
        said.push_str(&under.to_string());
        cause = under.source();
    }
    said
}

#[cfg(feature = "ui")]
fn due_for_a_look(
    config: &Config,
    lookups: &Gate,
    held: Option<&resonate_musicbrainz::Made>,
    at: u64,
) -> bool {
    config.mirrors_musicbrainz()
        && config.online_enabled()
        && !lookups.is_shut()
        && held.is_none_or(|made| {
            download::is_a_dump_name(&made.dump)
                && at.saturating_sub(made.built) >= LOOKED_AT_AGAIN_AFTER.as_secs()
        })
}

#[cfg(feature = "ui")]
pub fn keep_fresh(config: &Config) {
    follow_the_changes(config);
    if BUILDING.lock().is_some() {
        return;
    }
    let built = mirrored(config).held().map(|mirror| mirror.made().clone());
    let lookups = metered::for_lookups(config);
    if !due_for_a_look(config, &lookups, built.as_ref(), now()) {
        return;
    }
    let config = config.clone();
    let spawned = thread::Builder::new()
        .name("resonate-musicbrainz-look".to_owned())
        .spawn(move || {
            if let Some(made) = built {
                if lookups.is_shut() || !wanted(&config) {
                    return;
                }
                match download::latest(&download::agent()) {
                    Ok(latest) if latest == made.dump => return,
                    Ok(_) => {}
                    Err(error) => {
                        tracing::debug!(%error, "the newest MusicBrainz export could not be asked for");
                        return;
                    }
                }
            }
            if wanted(&config) {
                build_in_the_background(&config);
            }
        });
    if let Err(error) = spawned {
        tracing::debug!(%error, "the copy of MusicBrainz could not be looked at");
    }
}

pub fn remove(config: &Config) -> Result<bool> {
    let folder = folder()?;
    let _held = held_for_building(&folder).map_err(failed)?;
    mirrored(config).hold(None);
    download::forget_downloads(&folder);
    removed_beside_the_mirror(&folder).map_err(failed)?;
    let path = mirror_in(&folder);
    match fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(Error::RemoveMirror { path, source }),
    }
}

#[cfg(feature = "ui")]
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

fn bytes_of(path: &Path) -> u64 {
    fs::metadata(path).map_or(0, |held| held.len())
}

fn gigabytes(bytes: u64) -> f64 {
    bytes as f64 / BYTES_IN_A_GIGABYTE
}

pub struct Asked<'a> {
    pub build: bool,
    pub replicate: bool,
    pub from: Option<&'a Path>,
    pub remove: bool,
    pub search: Option<&'a str>,
}

fn replicated_now(config: &Config) -> Result<()> {
    let following = Arc::new(Building::default());
    let cancelling = Arc::clone(&following);
    let interrupting = crate::signals::cancel_when_told(move || cancelling.cancel());
    let done = replicate(config, &following);
    drop(interrupting);
    let done = done?;
    if done.to == done.from {
        said!(
            "{}",
            words::musicbrainz::already_replicated(done.to.to_string())
        );
    } else {
        said!(
            "{}",
            words::musicbrainz::replicated(done.to.to_string(), done.changes, done.documents)
        );
    }
    Ok(())
}

pub fn run(config: &Config, asked: Asked<'_>) -> Result<()> {
    let Asked {
        build: build_now,
        replicate,
        from,
        remove: removing,
        search,
    } = asked;
    if replicate {
        return replicated_now(config);
    }
    if removing {
        if remove(config)? {
            said!("{}", words::musicbrainz::removed());
        } else {
            said!("{}", words::musicbrainz::none_was_held());
        }
        return Ok(());
    }
    if build_now {
        let building = Arc::new(Building::default());
        let watched = Arc::clone(&building);
        let began = Instant::now();
        let built = thread::scope(|scope| {
            let telling = scope.spawn(|| {
                let mut last = Phase::Waiting;
                let mut written = 0;
                while watched.phase() != Phase::Done && !watched.is_cancelled() {
                    thread::sleep(TOLD_EVERY);
                    let phase = watched.phase();
                    let (read, of) = watched.bytes();
                    let documents = watched.documents();
                    if phase != last || of > 0 || documents != written {
                        written = documents;
                        tracing::info!(
                            doing = phase.label(),
                            read = gigabytes(read),
                            of = gigabytes(of),
                            rows = watched.rows(),
                            documents = watched.documents(),
                            "building the copy of MusicBrainz"
                        );
                        last = phase;
                    }
                }
            });
            let cancelling = Arc::clone(&building);
            let interrupting = crate::signals::cancel_when_told(move || cancelling.cancel());
            let built = build(config, from, &building);
            drop(interrupting);
            building.cancel();
            let _ = telling.join();
            built
        })?;
        said!(
            "{}",
            words::musicbrainz::built(
                format!("{:.1}", gigabytes(bytes_of(&built))),
                began.elapsed().as_secs() / 60,
                built.display().to_string(),
            )
        );
        if !config.mirrors_musicbrainz() {
            said!("{}", words::musicbrainz::mirror_off());
        }
        return Ok(());
    }
    let Some(mirror) = held_mirror() else {
        said!("{}", words::musicbrainz::none_held());
        return Ok(());
    };
    if let Some(asked) = search {
        let found = mirror
            .find_recording(&RecordingAsked {
                title: asked.to_owned(),
                artist: None,
                artist_mbid: None,
                release: None,
                length: None,
                wording: Wording::Words,
            })
            .map_err(failed)?;
        for recording in found.iter().take(RECORDINGS_SHOWN) {
            let credited: String = recording
                .credit
                .iter()
                .map(|named| format!("{}{}", named.name, named.joined_by))
                .collect();
            said!(
                "{:>3}  {}  {}",
                recording.score,
                recording.recording,
                words::musicbrainz::found(credited, &recording.title)
            );
        }
        return Ok(());
    }
    let made = mirror.made();
    let dump = made.dump.to_string();
    let path = mirror.path().display().to_string();
    let weighed = format!("{:.1}", gigabytes(bytes_of(mirror.path())));

    said!(
        "{}",
        if config.mirrors_musicbrainz() {
            words::musicbrainz::held(dump, weighed, path)
        } else {
            words::musicbrainz::held_though_off(dump, weighed, path)
        }
    );
    if let Some(amended) = mirror.amended_to() {
        let applied = std::time::UNIX_EPOCH + Duration::from_secs(amended.at);
        said!(
            "{}",
            words::musicbrainz::amended(
                amended.sequence.to_string(),
                resonate_core::elapsed::since(applied, std::time::SystemTime::now())
            )
        );
    }
    if let Some(kept) = kept() {
        said!(
            "{}",
            words::musicbrainz::answers_kept(kept.counted().map_err(failed)?)
        );
    }
    Ok(())
}

#[cfg(feature = "ui")]
pub struct Mirroring {
    config: Config,
}

#[cfg(feature = "ui")]
impl Mirroring {
    pub fn of(config: &Config) -> Arc<dyn resonate_ui::Mirroring> {
        Arc::new(Self {
            config: config.clone(),
        })
    }
}

#[cfg(feature = "ui")]
impl resonate_ui::Mirroring for Mirroring {
    fn told(&self) -> resonate_ui::MirrorTold {
        if let Some(building) = BUILDING.lock().clone() {
            let (read, of) = building.bytes();
            return resonate_ui::MirrorTold::Building {
                doing: building.phase().label(),
                done: (of > 0).then_some((read, of)),
            };
        }
        if let Some(why) = FAILED.lock().clone() {
            return resonate_ui::MirrorTold::Failed(why);
        }
        match mirrored(&self.config).held().or_else(held_mirror) {
            Some(mirror) => resonate_ui::MirrorTold::Held {
                dump: mirror.made().dump.clone(),
                bytes: bytes_of(mirror.path()),
                amended_at: mirror.amended_to().map(|amended| amended.at),
            },
            None => resonate_ui::MirrorTold::Absent,
        }
    }

    fn build(&self) {
        build_in_the_background(&self.config);
    }

    fn remove(&self) {
        if let Err(error) = remove(&self.config) {
            tracing::warn!(%error, "the copy of MusicBrainz could not be removed");
        }
    }
}

#[cfg(feature = "sync")]
const ASKED_OF_A_DEVICE_WITHIN: Duration = Duration::from_secs(15);

#[cfg(feature = "sync")]
pub struct ConsultedHere(Consultation);

#[cfg(feature = "sync")]
impl ConsultedHere {
    pub fn of(config: &Config) -> Arc<dyn resonate_sync::Consultable> {
        Arc::new(Self(consultation(config)))
    }
}

#[cfg(feature = "sync")]
impl resonate_sync::Consultable for ConsultedHere {
    fn consult(&self, asked: &[u8]) -> Option<Vec<u8>> {
        self.0.consulted(asked)
    }

    fn kept_after(
        &self,
        after: u64,
        most: usize,
        asker: resonate_sync::DeviceId,
    ) -> Vec<resonate_sync::KeptAnswer> {
        self.0
            .kept_after(after, most, &asker.to_string())
            .into_iter()
            .map(|kept| resonate_sync::KeptAnswer {
                seq: kept.seq,
                at: kept.at,
                key: kept.key,
                packed: kept.packed,
            })
            .collect()
    }

    fn kept_mark(&self, device: resonate_sync::DeviceId) -> u64 {
        self.0.kept_mark(&device.to_string())
    }

    fn take_kept(
        &self,
        device: resonate_sync::DeviceId,
        answers: Vec<resonate_sync::KeptAnswer>,
        mark: u64,
    ) {
        let answers: Vec<resonate_musicbrainz::KeptAnswer> = answers
            .into_iter()
            .map(|kept| resonate_musicbrainz::KeptAnswer {
                seq: kept.seq,
                at: kept.at,
                key: kept.key,
                packed: kept.packed,
            })
            .collect();
        let taken = self.0.take_kept(&device.to_string(), &answers, mark);
        if taken > 0 {
            tracing::debug!(taken, "lookup answers another device kept were taken");
        }
    }

    fn export(&self, part: resonate_sync::ExportPart) -> Option<resonate_sync::OfferedExport> {
        let part = match part {
            resonate_sync::ExportPart::Copy => resonate_musicbrainz::CopyPart::TheCopy,
            resonate_sync::ExportPart::Amendments => resonate_musicbrainz::CopyPart::ItsAmendments,
        };
        let offered = self.0.offered(part)?;
        Some(resonate_sync::OfferedExport {
            dump: offered.dump,
            sequence: offered.sequence,
            size: offered.size,
            reader: Box::new(offered.file),
        })
    }
}

#[cfg(feature = "sync")]
pub struct AskedOfDevices<F>(pub F);

#[cfg(feature = "sync")]
impl<F> resonate_musicbrainz::Ask for AskedOfDevices<F>
where
    F: Fn(&mut dyn FnMut(&resonate_sync::Sync)) + Send + std::marker::Sync,
{
    fn devices(&self) -> Vec<String> {
        let mut devices = Vec::new();
        (self.0)(&mut |sync| {
            let mut online: Vec<resonate_sync::SyncDevice> = sync
                .shown()
                .devices
                .into_iter()
                .filter(|device| device.online && !device.this)
                .collect();
            online.sort_by_key(|device| !device.nearby);
            devices = online.iter().map(|device| device.id.to_string()).collect();
        });
        devices
    }

    fn ask(&self, device: &str, asked: Vec<u8>) -> resonate_musicbrainz::Asking {
        let Some(id) = device_named(device) else {
            return resonate_musicbrainz::Asking::Unanswered;
        };
        let mut asking = None;
        (self.0)(&mut |sync| asking = Some(sync.ask_to_consult(id, asked.clone())));
        match asking.map(|asking| asking.answer(ASKED_OF_A_DEVICE_WITHIN)) {
            Some(resonate_sync::Consulting::Answered(answer)) => {
                resonate_musicbrainz::Asking::Answered(answer)
            }
            Some(resonate_sync::Consulting::Unconsulted) => {
                resonate_musicbrainz::Asking::Unconsulted
            }
            Some(resonate_sync::Consulting::Unanswered) | None => {
                resonate_musicbrainz::Asking::Unanswered
            }
        }
    }
}

#[cfg(feature = "sync")]
fn device_named(hex: &str) -> Option<resonate_sync::DeviceId> {
    let mut id = resonate_sync::DeviceId([0; resonate_sync::DEVICE_BYTES]);
    if hex.len() != id.0.len() * 2 {
        return None;
    }
    for (at, byte) in id.0.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(at * 2..at * 2 + 2)?, 16).ok()?;
    }
    Some(id)
}

#[cfg(test)]
mod tests {
    use std::{
        process,
        sync::atomic::{AtomicU64, Ordering},
    };

    #[cfg(feature = "ui")]
    use resonate_core::{Meter, Metering};

    use super::*;

    static SERIAL: AtomicU64 = AtomicU64::new(0);

    fn settings_file(text: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "resonate-musicbrainz-settings-{}-{}.toml",
            process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, text).expect("a settings file");
        path
    }

    fn started_with(online: bool, mirror: bool, file: &Path) -> Config {
        Config {
            online: Some(online),
            musicbrainz_mirror: Some(mirror),
            read_from: Some(file.to_path_buf()),
            ..Config::default()
        }
    }

    #[test]
    fn a_download_is_refused_when_the_settings_now_say_online_is_off_whatever_they_said_at_start() {
        let file = settings_file("online = false\n");
        let config = started_with(true, true, &file);

        let refused = build(&config, None, &Building::default());
        let _ = fs::remove_file(&file);

        assert!(matches!(refused, Err(Error::OnlineOff)));
        assert!(config.online_enabled());
    }

    #[test]
    fn the_settings_as_they_stand_are_read_not_the_copy_made_at_start() {
        let file = settings_file("online = true\nmusicbrainz-mirror = false\n");
        let config = started_with(false, true, &file);

        let live = current(&config);
        let _ = fs::remove_file(&file);

        assert!(live.online_enabled());
        assert!(!live.mirrors_musicbrainz());
    }

    #[test]
    fn settings_that_will_not_read_leave_the_start_as_it_was() {
        let missing = std::env::temp_dir().join("resonate-musicbrainz-no-such-settings.toml");
        let config = started_with(true, true, &missing);

        let live = current(&config);

        assert!(live.online_enabled());
        assert!(live.mirrors_musicbrainz());
    }

    #[test]
    fn a_build_landing_after_the_mirror_was_switched_off_is_not_held() {
        let off = settings_file("musicbrainz-mirror = false\n");
        let on = settings_file("musicbrainz-mirror = true\n");
        let mirrored = Mirrored::default();
        let looked_at = AtomicU64::new(0);
        let held = || {
            looked_at.fetch_add(1, Ordering::Relaxed);
            None
        };

        landed(&started_with(true, true, &off), &mirrored, held);
        let after_off = looked_at.load(Ordering::Relaxed);
        landed(&started_with(true, false, &on), &mirrored, held);
        let after_on = looked_at.load(Ordering::Relaxed);
        let _ = fs::remove_file(&off);
        let _ = fs::remove_file(&on);

        assert_eq!(after_off, 0);
        assert_eq!(after_on, 1);
    }

    #[cfg(feature = "ui")]
    #[test]
    fn switching_online_or_the_mirror_off_cancels_the_build_running() {
        let running = Arc::new(Building::default());
        *BUILDING.lock() = Some(Arc::clone(&running));

        let before = running.is_cancelled();
        stop_building();
        let after = running.is_cancelled();

        let following = Arc::new(Building::default());
        *BUILDING.lock() = Some(Arc::clone(&following));
        follow(&Config::default(), false);
        let followed = following.is_cancelled();

        let kept_on = Arc::new(Building::default());
        *BUILDING.lock() = Some(Arc::clone(&kept_on));
        let untouched = !kept_on.is_cancelled();
        *BUILDING.lock() = None;

        assert!(!before);
        assert!(after);
        assert!(followed);
        assert!(untouched);
    }

    #[cfg(feature = "ui")]
    #[test]
    fn a_look_at_the_newest_export_waits_for_the_lookup_gate_the_setting_and_the_age_of_the_copy() {
        let metering = Metering::new();
        let gate = Gate::new(metering.clone(), true);
        let wanting = Config {
            online: Some(true),
            musicbrainz_mirror: Some(true),
            ..Config::default()
        };
        let silent = Config {
            online: Some(false),
            ..wanting.clone()
        };
        let not_mirroring = Config {
            musicbrainz_mirror: Some(false),
            ..wanting.clone()
        };
        let built = 1_000_000;
        let old = resonate_musicbrainz::Made {
            dump: "20260101-000000".to_owned(),
            built,
        };
        let later = built + LOOKED_AT_AGAIN_AFTER.as_secs();

        let open = due_for_a_look(&wanting, &gate, None, later);
        let old_is_due = due_for_a_look(&wanting, &gate, Some(&old), later);
        let fresh_is_not = !due_for_a_look(&wanting, &gate, Some(&old), later - 1);
        let offline_is_not = !due_for_a_look(&silent, &gate, None, later);
        let not_mirroring_is_not = !due_for_a_look(&not_mirroring, &gate, None, later);
        metering.report(Meter::Metered);
        let metered_is_not = !due_for_a_look(&wanting, &gate, None, later)
            && !due_for_a_look(&wanting, &gate, Some(&old), later);

        assert!(open);
        assert!(old_is_due);
        assert!(fresh_is_not);
        assert!(offline_is_not);
        assert!(not_mirroring_is_not);
        assert!(metered_is_not);
    }
}
