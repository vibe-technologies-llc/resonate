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
use resonate_library::{RecordingAsked, Reference, Wording};
#[cfg(feature = "sync")]
use resonate_musicbrainz::Consultation;
use resonate_musicbrainz::{
    Answer, Archives, Asked, Building, KEPT_FILE, Kept, Layered, Mirror, Mirrored, Peers, Phase,
    built_from,
    download::{self, CORE_ARCHIVE, DERIVED_ARCHIVE},
    held_for_building, mirror_in, settled,
};

use crate::{Error, Result, config, config::Config, metered};

const FOLDER: &str = "musicbrainz";
const GIVEN_DUMP: &str = "given";
#[cfg(feature = "ui")]
pub const LOOKED_AT_AGAIN_AFTER: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const TOLD_EVERY: Duration = Duration::from_secs(5);
const RECORDINGS_SHOWN: usize = 10;
const BYTES_IN_A_GIGABYTE: f64 = 1_000_000_000.0;

static KEPT: OnceLock<Option<Arc<Kept>>> = OnceLock::new();
static MIRRORED: OnceLock<Arc<Mirrored>> = OnceLock::new();
static PEERS: RwLock<Option<Arc<dyn Peers>>> = RwLock::new(None);
#[cfg(feature = "ui")]
static BUILDING: Mutex<Option<Arc<Building>>> = Mutex::new(None);
#[cfg(feature = "ui")]
static FAILED: Mutex<Option<String>> = Mutex::new(None);

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
        mirrored.hold(None);
    }
}

struct Registered;

impl Peers for Registered {
    fn consult(&self, asked: &Asked) -> Option<Answer> {
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

pub fn build(config: &Config, from: Option<&Path>, building: &Building) -> Result<PathBuf> {
    let folder = folder()?;
    let held = held_for_building(&folder).map_err(failed)?;
    let archives = match from {
        Some(from) => given(from),
        None => {
            if !config.online_enabled() {
                return Err(Error::OnlineOff);
            }
            let agent = download::agent();
            let latest = download::latest(&agent).map_err(failed)?;
            let gate = metered::for_downloads(config);
            download::fetched(&agent, &latest, &folder, &gate, building, &held).map_err(failed)?
        }
    };
    let built = built_from(&archives, &folder, building, &held).map_err(failed)?;
    let mirrored = mirrored(config);
    mirrored.hold(None);
    let path = settled(&built, &folder).map_err(failed)?;
    if from.is_none() {
        download::forget_downloads(&folder);
    }
    if config.mirrors_musicbrainz() {
        mirrored.hold(held_mirror());
    }
    Ok(path)
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
pub fn keep_fresh(config: &Config) {
    if !config.mirrors_musicbrainz() || !config.online_enabled() || BUILDING.lock().is_some() {
        return;
    }
    let held = mirrored(config).held();
    let built = held.as_ref().map(|mirror| mirror.made().clone());
    let due = built.as_ref().is_none_or(|made| {
        download::is_a_dump_name(&made.dump)
            && now().saturating_sub(made.built) >= LOOKED_AT_AGAIN_AFTER.as_secs()
    });
    if !due {
        return;
    }
    let config = config.clone();
    let spawned = thread::Builder::new()
        .name("resonate-musicbrainz-look".to_owned())
        .spawn(move || {
            if let Some(made) = built {
                match download::latest(&download::agent()) {
                    Ok(latest) if latest == made.dump => return,
                    Ok(_) => {}
                    Err(error) => {
                        tracing::debug!(%error, "the newest MusicBrainz export could not be asked for");
                        return;
                    }
                }
            }
            build_in_the_background(&config);
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

pub fn run(
    config: &Config,
    build_now: bool,
    from: Option<&Path>,
    removing: bool,
    search: Option<&str>,
) -> Result<()> {
    if removing {
        if remove(config)? {
            said!("The copy of MusicBrainz is removed.");
        } else {
            said!("No copy of MusicBrainz was held.");
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
                while watched.phase() != Phase::Done && !watched.is_cancelled() {
                    thread::sleep(TOLD_EVERY);
                    let phase = watched.phase();
                    let (read, of) = watched.bytes();
                    if phase != last || of > 0 {
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
            "The copy of MusicBrainz is built at {} ({:.1} GB) in {} minutes.",
            built.display(),
            gigabytes(bytes_of(&built)),
            began.elapsed().as_secs() / 60
        );
        if !config.mirrors_musicbrainz() {
            said!("musicbrainz-mirror is off, so lookups do not ask it until it is turned on.");
        }
        return Ok(());
    }
    let Some(mirror) = held_mirror() else {
        said!("No copy of MusicBrainz is held. `resonate musicbrainz --build` makes one.");
        return Ok(());
    };
    if let Some(words) = search {
        let found = mirror
            .find_recording(&RecordingAsked {
                title: words.to_owned(),
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
                "{:>3}  {}  {} by {}",
                recording.score,
                recording.recording,
                recording.title,
                credited
            );
        }
        return Ok(());
    }
    let made = mirror.made();
    said!(
        "The export of {} is held at {} ({:.1} GB){}.",
        made.dump,
        mirror.path().display(),
        gigabytes(bytes_of(mirror.path())),
        if config.mirrors_musicbrainz() {
            ""
        } else {
            ", though musicbrainz-mirror is off"
        }
    );
    if let Some(kept) = kept() {
        said!(
            "{} lookup answers are kept to share with synced devices.",
            kept.counted().map_err(failed)?
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
            },
            None => resonate_ui::MirrorTold::Absent,
        }
    }

    fn build(&self) {
        let config = Config {
            musicbrainz_mirror: Some(true),
            ..self.config.clone()
        };
        build_in_the_background(&config);
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
