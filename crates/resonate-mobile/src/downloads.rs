use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    fs::File,
    io::{self, Read, Write},
    os::fd::FromRawFd,
    path::Path,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, RecvTimeoutError},
    },
    thread,
    time::{Duration, SystemTime},
};

use parking_lot::Mutex;
use resonate_core::{Isrc, Mbid, SourceId};
use resonate_library::ANSWERS_WITHIN;
use resonate_providers::{
    Asking, Authorizing, Away, Client, Delivered, Delivery, Identity, Obtained, Provider,
    Providers, Reopened, SignsIn, TURNED_TO_APART, WAITED_ON_AFTER_AN_OFFER,
};

use crate::{
    Error, MobilePlayer, Result,
    error::DownloadOp,
    features::SearchSong,
    partials::{Partial, Partials},
};

const JOBS_SETTLED_KEPT: usize = 16;
const CHUNK_BYTES: usize = 64 * 1024;
const LARGEST_DOWNLOAD: u64 = 8 * 1024 * 1024 * 1024;
const HEEDED_EVERY: Duration = Duration::from_millis(50);
const OFFER_HELD_FOR: Duration = Duration::from_secs(120);
const EXTENSION_UNNAMED: &str = "flac";

#[derive(Clone, Copy)]
struct OfferHeldFor(Duration);

impl Default for OfferHeldFor {
    fn default() -> Self {
        Self(OFFER_HELD_FOR)
    }
}

#[derive(Default)]
pub(crate) struct Downloads {
    partials: Option<Partials>,
    jobs: Mutex<BTreeMap<u64, Arc<Job>>>,
    next: AtomicU64,
    parkings: AtomicU64,
    offer_held_for: OfferHeldFor,
    authorizing: Mutex<Option<SigningIn>>,
    sign_ins: AtomicU64,
    refreshed: Arc<Mutex<Option<String>>>,
    hosted_services_declined: AtomicBool,
}

struct SigningIn {
    attempt: u64,
    client: Client,
    authorizing: Authorizing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MusicProvider {
    Monochrome,
    Subsonic,
    Tidal,
    HifiApi,
}

impl fmt::Display for MusicProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Monochrome => "Monochrome",
            Self::Subsonic => "The Subsonic server",
            Self::Tidal => "TIDAL",
            Self::HifiApi => "The hifi-api server",
        })
    }
}

#[derive(Clone, uniffi::Record)]
pub struct DownloadAccount {
    pub provider: MusicProvider,
    pub server: String,
    pub user: String,
    pub password: String,
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct DownloadOffer {
    pub job: u64,
    pub extension: String,
    pub name: String,
    pub provider: MusicProvider,
    pub total: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum DownloadPhase {
    Finding,
    Downloading,
    Validating,
    Done,
    Failed,
    Cancelled,
}

impl DownloadPhase {
    const fn is_settled(self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Cancelled)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DownloadProgress {
    pub phase: DownloadPhase,
    pub received: u64,
    pub total: Option<u64>,
    pub provider: Option<MusicProvider>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct TidalAuthorization {
    pub user_code: String,
    pub verify_at: String,
}

pub(crate) struct Job {
    id: u64,
    pub(crate) song: SearchSong,
    cancelled: AtomicBool,
    received: AtomicU64,
    standing: Mutex<Standing>,
}

struct Standing {
    phase: DownloadPhase,
    total: Option<u64>,
    provider: Option<MusicProvider>,
    taken_from: Option<String>,
    opened: Option<Parked>,
}

struct Parked {
    reader: Box<dyn Read + Send>,
    partial: Option<Partial>,
    parking: u64,
    _watched: mpsc::Sender<()>,
}

impl Job {
    fn new(id: u64, song: SearchSong) -> Self {
        Self {
            id,
            song,
            cancelled: AtomicBool::new(false),
            received: AtomicU64::new(0),
            standing: Mutex::new(Standing {
                phase: DownloadPhase::Finding,
                total: None,
                provider: None,
                taken_from: None,
                opened: None,
            }),
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    fn progress(&self) -> DownloadProgress {
        let standing = self.standing.lock();
        DownloadProgress {
            phase: standing.phase,
            received: self.received.load(Ordering::Acquire),
            total: standing.total,
            provider: standing.provider,
        }
    }

    pub(crate) fn moved_to(&self, phase: DownloadPhase) -> Result<()> {
        let mut standing = self.standing.lock();
        if self.is_cancelled() || standing.phase == DownloadPhase::Cancelled {
            standing.phase = DownloadPhase::Cancelled;
            return Err(Error::DownloadCancelled);
        }
        standing.phase = phase;
        Ok(())
    }

    pub(crate) fn settled<T>(&self, result: Result<T>) -> Result<T> {
        let mut standing = self.standing.lock();
        standing.phase = match &result {
            Ok(_) => standing.phase,
            Err(Error::DownloadCancelled) => DownloadPhase::Cancelled,
            Err(_) if self.is_cancelled() => DownloadPhase::Cancelled,
            Err(_) => DownloadPhase::Failed,
        };
        if result.is_err() {
            standing.opened = None;
        }
        result
    }

    pub(crate) fn done(&self) {
        self.standing.lock().phase = DownloadPhase::Done;
    }

    fn expire(&self, parking: u64) {
        let mut standing = self.standing.lock();
        if standing
            .opened
            .as_ref()
            .is_some_and(|parked| parked.parking == parking)
        {
            standing.opened = None;
            if !standing.phase.is_settled() {
                standing.phase = DownloadPhase::Failed;
            }
        }
    }

    pub(crate) fn taken_from(&self) -> Option<String> {
        self.standing.lock().taken_from.clone()
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        let mut standing = self.standing.lock();
        standing.opened = None;
        if !standing.phase.is_settled() {
            standing.phase = DownloadPhase::Cancelled;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tried {
    Offered,
    Missed,
    Unheard,
}

pub(crate) struct Searched {
    pub(crate) offer: Result<DownloadOffer>,
    pub(crate) tried: Tried,
}

struct Heard {
    provider: MusicProvider,
    failure: Option<Error>,
}

struct Told {
    asked: Box<dyn Provider>,
    provider: MusicProvider,
    heard: Arc<Mutex<Vec<Heard>>>,
}

impl Provider for Told {
    fn source(&self) -> &SourceId {
        self.asked.source()
    }

    fn find(&self, identity: &Identity) -> resonate_providers::Result<Obtained> {
        let found = self.asked.find(identity);
        let failure = found.as_ref().err().map(|error| {
            tracing::warn!(%error, provider = %self.provider, "a download provider refused");
            failure_of(self.provider, error)
        });
        self.heard.lock().push(Heard {
            provider: self.provider,
            failure,
        });
        found
    }
}

pub(crate) fn failure_of(provider: MusicProvider, error: &resonate_providers::Error) -> Error {
    use resonate_providers::Error as Refusal;

    match error {
        Refusal::Io { source, .. } if source.kind() == io::ErrorKind::TimedOut => {
            Error::ProviderAway { provider }
        }
        Refusal::Io { .. } => Error::Unreachable { provider },
        Refusal::Refused { status, .. } => Error::ProviderRefused {
            provider,
            status: *status,
        },
        Refusal::Unwelcome { code, .. } => Error::AccountRefused {
            provider,
            code: *code,
        },
        Refusal::TurnedAway { .. } => Error::NotFound,
        Refusal::Untrusted { .. } => Error::ProviderUntrusted { provider },
        Refusal::Unaddressable { .. } | Refusal::NotTheService { .. } => {
            Error::ProviderMisaddressed { provider }
        }
        Refusal::StillQueued { .. } | Refusal::StillArriving { .. } | Refusal::Busy { .. } => {
            Error::ProviderAway { provider }
        }
        Refusal::Unreadable { .. } | Refusal::OffItsHosts { .. } | Refusal::NotAnExtension => {
            Error::ProviderUnreadable { provider }
        }
        Refusal::AuthorizationLapsed { .. } => Error::SignInLapsed { provider },
        Refusal::AuthorizationDenied { .. } => Error::SignInDenied { provider },
    }
}

pub(crate) fn identity_of(song: &SearchSong) -> Identity {
    Identity {
        recording: Mbid::new(&song.recording).ok(),
        release: Mbid::new(&song.release).ok(),
        isrcs: song
            .isrcs
            .iter()
            .filter_map(|isrc| Isrc::new(isrc).ok())
            .collect(),
        title: song.title.clone(),
        artist: (!song.artist.is_empty()).then(|| song.artist.clone()),
        album: (!song.album.is_empty()).then(|| song.album.clone()),
        length: song.duration_ms.map(Duration::from_millis),
        disc: song.disc,
        position: song.position,
        ..Identity::default()
    }
}

const UNFILEABLE: [char; 9] = ['/', '\\', '?', ':', '*', '"', '<', '>', '|'];

pub(crate) fn file_stem_of(song: &SearchSong) -> String {
    format!("{} - {}", song.artist, song.title)
        .chars()
        .map(|letter| {
            if letter.is_control() || UNFILEABLE.contains(&letter) {
                '_'
            } else {
                letter
            }
        })
        .collect()
}

struct Taken {
    reader: Box<dyn Read + Send>,
    partial: Option<Partial>,
    length: Option<u64>,
    extension: String,
    provider: MusicProvider,
    taken_from: String,
}

enum Unopened {
    Gone,
    Failed(Error),
}

impl Downloads {
    pub(crate) fn under(folder: &Path) -> Self {
        let partials = Partials::under(folder);
        partials.sweep(SystemTime::now());
        Self {
            partials: Some(partials),
            ..Self::default()
        }
    }

    pub(crate) fn begin(&self, song: SearchSong) -> u64 {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let mut jobs = self.jobs.lock();
        let settled: Vec<u64> = jobs
            .iter()
            .filter(|(_, job)| job.standing.lock().phase.is_settled())
            .map(|(id, _)| *id)
            .collect();
        for gone in settled
            .iter()
            .take(settled.len().saturating_sub(JOBS_SETTLED_KEPT))
        {
            jobs.remove(gone);
        }
        jobs.insert(id, Arc::new(Job::new(id, song)));
        id
    }

    pub(crate) fn job(&self, job: u64) -> Result<Arc<Job>> {
        self.jobs
            .lock()
            .get(&job)
            .cloned()
            .ok_or(Error::UnknownDownload)
    }

    pub(crate) fn progress(&self, job: u64) -> Option<DownloadProgress> {
        self.jobs.lock().get(&job).map(|job| job.progress())
    }

    pub(crate) fn cancel(&self, job: u64) {
        if let Some(job) = self.jobs.lock().get(&job) {
            job.cancel();
        }
    }

    pub(crate) fn cancel_every_job(&self) {
        for job in self.jobs.lock().values() {
            job.cancel();
        }
    }

    fn sign_in_asked(&self) -> u64 {
        self.sign_ins.fetch_add(1, Ordering::AcqRel) + 1
    }

    fn sign_in_began(&self, attempt: u64, client: Client, authorizing: Authorizing) {
        let mut slot = self.authorizing.lock();
        if self.sign_ins.load(Ordering::Acquire) == attempt {
            *slot = Some(SigningIn {
                attempt,
                client,
                authorizing,
            });
        }
    }

    fn cancel_sign_in(&self) {
        self.sign_ins.fetch_add(1, Ordering::AcqRel);
    }

    fn signed_in(
        &self,
        signs_in: &dyn SignsIn,
        unreached: &(dyn Fn() -> bool + Sync),
    ) -> Result<Option<String>> {
        let SigningIn {
            attempt,
            client,
            authorizing,
        } = self
            .authorizing
            .lock()
            .take()
            .ok_or(Error::DownloadExpired)?;
        let ended = || self.sign_ins.load(Ordering::Acquire) != attempt || unreached();
        signs_in
            .authorized(&client, &authorizing, &ended)
            .map(|token| token.map(|token| token.into_string()))
            .map_err(|source| failure_of(MusicProvider::Tidal, &source))
    }

    pub(crate) fn hosted_services_allowed(&self, allowed: bool) {
        self.hosted_services_declined
            .store(!allowed, Ordering::Release);
    }

    fn is_asked(&self, account: &DownloadAccount) -> bool {
        let hosted = matches!(
            account.provider,
            MusicProvider::Monochrome | MusicProvider::HifiApi
        ) && account.server.trim().is_empty()
            && !(account.provider == MusicProvider::HifiApi && !account.password.trim().is_empty());
        !(hosted && self.hosted_services_declined.load(Ordering::Acquire))
    }

    fn provider_of(&self, account: DownloadAccount) -> Box<dyn Provider> {
        let DownloadAccount {
            provider,
            server,
            user,
            password,
            client_id,
            client_secret,
            refresh_token,
        } = account;
        let custom = !server.trim().is_empty();
        match provider {
            MusicProvider::Monochrome if custom => {
                Box::new(resonate_monochrome::Monochrome::at(&server))
            }
            MusicProvider::Monochrome => Box::new(resonate_monochrome::Monochrome::hosted()),
            MusicProvider::Subsonic => {
                Box::new(resonate_subsonic::Subsonic::at(resonate_subsonic::Server {
                    url: server,
                    user,
                    password,
                }))
            }
            MusicProvider::Tidal => {
                let refreshed = Arc::clone(&self.refreshed);
                Box::new(
                    resonate_tidal::Tidal::signed_in(resonate_tidal::Account {
                        client_id,
                        client_secret: (!client_secret.is_empty()).then_some(client_secret),
                        refresh_token,
                    })
                    .telling(move |token| {
                        *refreshed.lock() = Some(token.into_string());
                    }),
                )
            }
            MusicProvider::HifiApi if !password.trim().is_empty() => Box::new(
                resonate_tidal::HifiApi::at(resonate_tidal::OWN_SERVER)
                    .with_secret(&password)
                    .known_as_resonate(),
            ),
            MusicProvider::HifiApi if custom => Box::new(resonate_tidal::HifiApi::at(&server)),
            MusicProvider::HifiApi => Box::new(resonate_tidal::HifiApi::hosted()),
        }
    }

    pub(crate) fn find(
        &self,
        job: &Arc<Job>,
        accounts: Vec<DownloadAccount>,
        refused: &BTreeSet<String>,
        reaching: &(dyn Fn() -> bool + Sync),
    ) -> Searched {
        let accounts: Vec<DownloadAccount> = accounts
            .into_iter()
            .filter(|account| self.is_asked(account))
            .collect();
        if accounts.is_empty() {
            return Searched {
                offer: job.settled(Err(Error::NoProvider)),
                tried: Tried::Unheard,
            };
        }
        if let Err(cancelled) = job.moved_to(DownloadPhase::Finding) {
            return Searched {
                offer: Err(cancelled),
                tried: Tried::Unheard,
            };
        }
        job.received.store(0, Ordering::Release);

        let heard = Arc::new(Mutex::new(Vec::new()));
        let mut asked = Vec::new();
        let mut providers = Providers::none();
        for account in accounts {
            let provider = account.provider;
            let told = Told {
                asked: self.provider_of(account),
                provider,
                heard: Arc::clone(&heard),
            };
            asked.push((told.source().clone(), provider));
            providers = providers.and(Arc::new(told));
        }
        let cancelled = || job.is_cancelled() || !reaching();
        let declined = |delivered: &Delivered| refused.contains(&delivered.taken_from().to_uri());
        let answer = providers.first(
            &identity_of(&job.song),
            &Asking {
                within: ANSWERS_WITHIN,
                cancelled: &cancelled,
                turning_to: &|_| {},
                declined: &declined,
                passing: &[],
                apart: TURNED_TO_APART,
                grace: WAITED_ON_AFTER_AN_OFFER,
            },
            &mut Away::default(),
        );

        let tried = if answer.delivered.is_some() {
            Tried::Offered
        } else if answer.heard_from_every_provider() {
            Tried::Missed
        } else {
            Tried::Unheard
        };
        let kind_of = |source: &SourceId| {
            asked
                .iter()
                .find(|(asked, _)| asked == source)
                .map(|(_, provider)| *provider)
        };
        let offers: Vec<Delivered> = answer
            .delivered
            .into_iter()
            .chain(answer.held_back)
            .collect();
        let heard = std::mem::take(&mut *heard.lock());
        let offer = if offers.is_empty() {
            Err(nothing_found(job, reaching, heard, &asked))
        } else {
            self.first_opened(job, offers, &kind_of, reaching)
        };
        Searched {
            offer: job.settled(offer),
            tried,
        }
    }

    fn first_opened(
        &self,
        job: &Arc<Job>,
        offers: Vec<Delivered>,
        kind_of: &dyn Fn(&SourceId) -> Option<MusicProvider>,
        reaching: &(dyn Fn() -> bool + Sync),
    ) -> Result<DownloadOffer> {
        let mut failure = None;
        for offered in offers {
            let Some(provider) = kind_of(&offered.provider) else {
                continue;
            };
            match opened(job, self.partials.as_ref(), offered, provider, reaching)? {
                Ok(taken) => return Ok(self.held_for_a_destination(job, taken)),
                Err(Unopened::Gone) => {}
                Err(Unopened::Failed(error)) => {
                    failure.get_or_insert(error);
                }
            }
        }
        Err(failure.unwrap_or(Error::NotFound))
    }

    fn held_for_a_destination(&self, job: &Arc<Job>, taken: Taken) -> DownloadOffer {
        let name = resonate_core::naming::named_within(
            "",
            std::ffi::OsStr::new(&file_stem_of(&job.song)),
            format!(".{}", taken.extension),
        )
        .to_string_lossy()
        .into_owned();
        let mut standing = job.standing.lock();
        standing.total = taken.length;
        standing.provider = Some(taken.provider);
        standing.taken_from = Some(taken.taken_from);
        standing.opened = Some(self.parked(job, taken.reader, taken.partial));
        DownloadOffer {
            job: job.id,
            extension: taken.extension,
            name,
            provider: taken.provider,
            total: taken.length,
        }
    }

    fn parked(
        &self,
        job: &Arc<Job>,
        reader: Box<dyn Read + Send>,
        partial: Option<Partial>,
    ) -> Parked {
        let parking = self.parkings.fetch_add(1, Ordering::Relaxed);
        let (watched, released) = mpsc::channel::<()>();
        let held_for = self.offer_held_for.0;
        let job = Arc::downgrade(job);
        let watchdog = thread::Builder::new()
            .name("resonate-download-offer".into())
            .spawn(move || expire_unless_taken(&released, &job, parking, held_for));
        if let Err(error) = watchdog {
            tracing::warn!(%error, "an offered download is not watched for being left");
        }
        Parked {
            reader,
            partial,
            parking,
            _watched: watched,
        }
    }

    pub(crate) fn download(
        &self,
        job: &Job,
        mut output: File,
        reaching: &dyn Fn() -> bool,
    ) -> Result<u64> {
        let parked = job.standing.lock().opened.take();
        let Parked {
            mut reader,
            partial,
            ..
        } = parked.ok_or(Error::DownloadExpired)?;
        job.moved_to(DownloadPhase::Downloading)?;
        let from = partial.as_ref().map_or(0, Partial::from);
        job.received.store(from, Ordering::Release);
        let total = partial.as_ref().and_then(|_| job.standing.lock().total);
        let mut bytes = from;
        let mut buffer = vec![0_u8; CHUNK_BYTES];
        let copied = loop {
            if job.is_cancelled() {
                break Err(Error::DownloadCancelled);
            }
            if !reaching() {
                break Err(Error::OnlineOff);
            }
            let count = match reader.read(&mut buffer) {
                Ok(0) if total.is_some_and(|total| bytes < total) => {
                    break Err(Error::DownloadIo {
                        op: DownloadOp::Read,
                        source: io::ErrorKind::UnexpectedEof.into(),
                    });
                }
                Ok(0) => break Ok(bytes),
                Ok(count) => count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(source) => {
                    break Err(Error::DownloadIo {
                        op: DownloadOp::Read,
                        source,
                    });
                }
            };
            bytes = bytes.saturating_add(count as u64);
            if bytes > LARGEST_DOWNLOAD {
                break Err(Error::DownloadTooLarge);
            }
            let written = match &partial {
                Some(partial) => partial.append(&buffer[..count]),
                None => output.write_all(&buffer[..count]),
            };
            if let Err(source) = written {
                break Err(Error::DownloadIo {
                    op: DownloadOp::Write,
                    source,
                });
            }
            job.received.store(bytes, Ordering::Release);
        };
        let delivered = match partial {
            None => copied,
            Some(partial) => handed_over(job, partial, copied, &mut output),
        };
        let finished = delivered.and_then(|bytes| {
            output
                .flush()
                .and_then(|()| output.sync_all())
                .map(|()| bytes)
                .map_err(|source| Error::DownloadIo {
                    op: DownloadOp::Finish,
                    source,
                })
        });
        job.settled(finished)
    }
}

fn handed_over(
    job: &Job,
    partial: Partial,
    streamed: Result<u64>,
    output: &mut File,
) -> Result<u64> {
    let bytes = match streamed {
        Ok(bytes) => bytes,
        Err(Error::DownloadTooLarge) => {
            partial.finished();
            return Err(Error::DownloadTooLarge);
        }
        Err(error) => {
            if let Err(error) = partial.kept() {
                tracing::warn!(%error, "a partial download could not be kept");
            }
            return Err(error);
        }
    };
    partial.kept().map_err(|source| Error::DownloadIo {
        op: DownloadOp::Finish,
        source,
    })?;
    partial.replay(&mut |chunk| {
        if job.is_cancelled() {
            return Err(Error::DownloadCancelled);
        }
        output.write_all(chunk).map_err(|source| Error::DownloadIo {
            op: DownloadOp::Write,
            source,
        })
    })?;
    partial.finished();
    Ok(bytes)
}

fn expire_unless_taken(
    released: &mpsc::Receiver<()>,
    job: &Weak<Job>,
    parking: u64,
    held_for: Duration,
) {
    if matches!(
        released.recv_timeout(held_for),
        Err(RecvTimeoutError::Timeout)
    ) && let Some(job) = job.upgrade()
    {
        job.expire(parking);
    }
}

fn nothing_found(
    job: &Job,
    reaching: &(dyn Fn() -> bool + Sync),
    heard: Vec<Heard>,
    asked: &[(SourceId, MusicProvider)],
) -> Error {
    if job.is_cancelled() {
        return Error::DownloadCancelled;
    }
    if !reaching() {
        return Error::OnlineOff;
    }
    let late = asked
        .iter()
        .map(|(_, provider)| *provider)
        .find(|provider| heard.iter().all(|heard| heard.provider != *provider));
    if let Some(failure) = heard.into_iter().find_map(|heard| heard.failure) {
        return failure;
    }
    late.map_or(Error::NotFound, |provider| Error::ProviderAway { provider })
}

fn opened(
    job: &Job,
    partials: Option<&Partials>,
    offered: Delivered,
    provider: MusicProvider,
    reaching: &(dyn Fn() -> bool + Sync),
) -> Result<std::result::Result<Taken, Unopened>> {
    let taken_from = offered.taken_from().to_uri();
    match offered.delivery {
        Delivery::File(path) => {
            let extension = path
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or(EXTENSION_UNNAMED)
                .to_owned();
            let file = File::open(&path).map_err(|source| Error::DownloadIo {
                op: DownloadOp::Open,
                source,
            })?;
            let length = file.metadata().ok().map(|metadata| metadata.len());
            Ok(Ok(Taken {
                reader: Box::new(file),
                partial: None,
                length,
                extension,
                provider,
                taken_from,
            }))
        }
        Delivery::Stream {
            extension, opening, ..
        } => {
            let claimed = partials
                .filter(|_| opening.is_resumable())
                .and_then(|partials| partials.claim(&job.song, &taken_from));
            let held = claimed.as_ref().and_then(Partial::held);
            let reopened = heeding(job, reaching, move || opening.open_resuming(held))?;
            Ok(match reopened {
                Ok(Reopened::Reading(resumed)) => {
                    let begun = claimed.map(|claimed| {
                        claimed.begun(
                            &job.song,
                            &taken_from,
                            resumed.from,
                            resumed.length,
                            resumed.validator,
                        )
                    });
                    let partial = match begun {
                        Some(Ok(partial)) => Some(partial),
                        Some(Err(source)) if resumed.from > 0 => {
                            return Ok(Err(Unopened::Failed(Error::DownloadIo {
                                op: DownloadOp::Write,
                                source,
                            })));
                        }
                        Some(Err(error)) => {
                            tracing::warn!(%error, "a download is not kept as it arrives");
                            None
                        }
                        None => None,
                    };
                    Ok(Taken {
                        reader: resumed.reader,
                        partial,
                        length: resumed.length,
                        extension: extension.as_str().to_owned(),
                        provider,
                        taken_from,
                    })
                }
                Ok(Reopened::Gone) => {
                    if let Some(claimed) = claimed {
                        claimed.finished();
                    }
                    Err(Unopened::Gone)
                }
                Err(error) => {
                    tracing::warn!(%error, %provider, "an offered download could not be opened");
                    Err(Unopened::Failed(failure_of(provider, &error)))
                }
            })
        }
    }
}

fn heeding<T: Send + 'static>(
    job: &Job,
    reaching: &(dyn Fn() -> bool + Sync),
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T> {
    let (told, answer) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("resonate-download-opening".to_owned())
        .spawn(move || {
            let _ = told.send(work());
        })
        .map_err(|source| Error::DownloadIo {
            op: DownloadOp::Open,
            source,
        })?;
    loop {
        match answer.recv_timeout(HEEDED_EVERY) {
            Ok(value) => return Ok(value),
            Err(RecvTimeoutError::Timeout) if job.is_cancelled() => {
                return Err(Error::DownloadCancelled);
            }
            Err(RecvTimeoutError::Timeout) if !reaching() => return Err(Error::OnlineOff),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err(Error::DownloadIo {
                    op: DownloadOp::Open,
                    source: io::Error::from(io::ErrorKind::BrokenPipe),
                });
            }
        }
    }
}

#[uniffi::export]
impl MobilePlayer {
    pub fn begin_download(&self, song: SearchSong) -> u64 {
        self.downloads.begin(song)
    }

    pub fn find_download(&self, job: u64, accounts: Vec<DownloadAccount>) -> Result<DownloadOffer> {
        let job = self.downloads.job(job)?;
        job.settled(self.require_online())?;
        let client = Arc::clone(self.services.online.client());
        let reaching = move || client.is_reaching();
        let refused = self.catalog.refused_offers(&job.song, SystemTime::now())?;
        let searched = self.downloads.find(&job, accounts, &refused, &reaching);
        if let Err(error) = self.catalog.note_wanted_tried(&job.song, searched.tried) {
            tracing::warn!(%error, "a wanted song's try could not be noted");
        }
        searched.offer
    }

    pub fn download_offer(&self, job: u64, descriptor: i32) -> Result<u64> {
        let output = fresh_destination(descriptor)?;
        let job = self.downloads.job(job)?;
        job.settled(self.require_online())?;
        let client = Arc::clone(self.services.online.client());
        self.downloads
            .download(&job, output, &move || client.is_reaching())
    }

    pub fn configure_hosted_services(&self, enabled: bool) {
        self.downloads.hosted_services_allowed(enabled);
    }

    pub fn download_progress(&self, job: u64) -> Option<DownloadProgress> {
        self.downloads.progress(job)
    }

    pub fn cancel_download(&self, job: u64) {
        self.downloads.cancel(job);
    }

    pub fn authorize_tidal(&self, client_id: String, secret: String) -> Result<TidalAuthorization> {
        self.require_online()?;
        let attempt = self.downloads.sign_in_asked();
        let client = Client {
            id: client_id,
            secret: (!secret.is_empty()).then_some(secret),
        };
        let authorizing = resonate_tidal::TidalSignIn::at(resonate_tidal::Endpoints::tidal())
            .authorizing(&client)
            .map_err(|source| failure_of(MusicProvider::Tidal, &source))?;
        let request = TidalAuthorization {
            user_code: authorizing.user_code.clone(),
            verify_at: authorizing.verify_at.clone(),
        };
        self.downloads.sign_in_began(attempt, client, authorizing);
        Ok(request)
    }

    pub fn finish_tidal_authorization(&self) -> Result<Option<String>> {
        self.require_online()?;
        self.downloads.signed_in(
            &resonate_tidal::TidalSignIn::at(resonate_tidal::Endpoints::tidal()),
            &|| !self.services.online.client().is_reaching(),
        )
    }

    pub fn cancel_tidal_authorization(&self) {
        self.downloads.cancel_sign_in();
    }

    pub fn refreshed_tidal_token(&self) -> Option<String> {
        self.downloads.refreshed.lock().take()
    }
}

impl MobilePlayer {
    pub(crate) fn validate_download(&self, uri: String, duration_ms: Option<u64>) -> Result<()> {
        let info = resonate_codec::probe(&self.sources, &crate::documents::location(&uri)?)?;
        if let Some(expected) = duration_ms
            && let Some(frames) = info.duration
            && frames
                .to_duration(info.spec.rate)
                .abs_diff(Duration::from_millis(expected))
                > resonate_providers::LENGTHS_AGREE_WITHIN
        {
            return Err(Error::WrongSong);
        }
        Ok(())
    }
}

pub(crate) fn fresh_destination(descriptor: i32) -> Result<File> {
    if descriptor < 0 {
        return Err(Error::InvalidDescriptor);
    }
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_title_holding_what_a_shared_or_fat_folder_refuses_is_a_name_it_takes() {
        let song = SearchSong {
            title: "Why? Who: \"This\" <or> That*|".into(),
            artist: "AC/DC".into(),
            ..SearchSong::default()
        };

        assert_eq!(file_stem_of(&song), "AC_DC - Why_ Who_ _This_ _or_ That__");
    }
    use std::{
        io::{BufRead, BufReader},
        net::{TcpListener, TcpStream},
        path::PathBuf,
        time::{Instant, SystemTime, UNIX_EPOCH},
    };

    use super::*;

    const ECHOES_ISRC: &str = "GBN9Y1100065";
    const ECHOES_TRACK: &str = "154140652551016448";
    const SEARCH_HELD_FOR: Duration = Duration::from_secs(20);
    const CANCEL_HEARD_WITHIN: Duration = Duration::from_secs(2);

    struct Canned {
        status: u16,
        body: Vec<u8>,
        held_for: Duration,
    }

    fn served(answering: impl Fn(&str) -> Canned + Send + Sync + 'static) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
        let url = format!("http://{}", listener.local_addr().expect("a bound address"));
        let answering = Arc::new(answering);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    return;
                };
                let answering = Arc::clone(&answering);
                thread::spawn(move || answer(stream, answering.as_ref()));
            }
        });
        url
    }

    fn answer(stream: TcpStream, answering: &dyn Fn(&str) -> Canned) {
        let mut reader = BufReader::new(stream);
        let mut request = String::new();
        if reader.read_line(&mut request).is_err() {
            return;
        }
        loop {
            let mut header = String::new();
            match reader.read_line(&mut header) {
                Ok(0) | Err(_) => return,
                Ok(_) if header == "\r\n" => break,
                Ok(_) => {}
            }
        }
        let target = request.split(' ').nth(1).unwrap_or_default();
        let canned = answering(target);
        thread::sleep(canned.held_for);

        let head = format!(
            "HTTP/1.1 {} Canned\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            canned.status,
            canned.body.len()
        );
        let mut stream = reader.into_inner();
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&canned.body);
    }

    fn listing() -> Vec<u8> {
        format!(
            r#"{{"tracks":[{{"id":"{ECHOES_TRACK}","trackId":"{ECHOES_TRACK}","title":"Echoes","artistNames":["Pink Floyd"],"duration":1412451,"isrc":"{ECHOES_ISRC}","playable":true}}]}}"#
        )
        .into_bytes()
    }

    fn monochrome_serving(audio: &'static [u8], search_held_for: Duration) -> String {
        served(move |target| {
            if target.starts_with("/search/tracks") {
                Canned {
                    status: 200,
                    body: listing(),
                    held_for: search_held_for,
                }
            } else {
                Canned {
                    status: 200,
                    body: audio.to_vec(),
                    held_for: Duration::ZERO,
                }
            }
        })
    }

    fn refusing(status: u16) -> String {
        served(move |_| Canned {
            status,
            body: Vec::new(),
            held_for: Duration::ZERO,
        })
    }

    fn account(provider: MusicProvider, server: &str) -> DownloadAccount {
        DownloadAccount {
            provider,
            server: server.to_owned(),
            user: "listener".to_owned(),
            password: "secret".to_owned(),
            client_id: String::new(),
            client_secret: String::new(),
            refresh_token: String::new(),
        }
    }

    fn unreached_address() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a local port");
        let url = format!("http://{}", listener.local_addr().expect("a bound address"));
        drop(listener);
        url
    }

    fn echoes() -> SearchSong {
        SearchSong {
            recording: "8f2eb3e7-4ff2-4b14-8c0e-8b7c1e7c2b7d".to_owned(),
            title: "Echoes".to_owned(),
            artist: "Pink Floyd".to_owned(),
            duration_ms: Some(1_412_451),
            isrcs: vec![ECHOES_ISRC.to_owned()],
            ..SearchSong::default()
        }
    }

    fn reaching() -> bool {
        true
    }

    fn destination(name: &str) -> (PathBuf, File) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("a clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("resonate-download-{name}-{unique}"));
        let file = File::options()
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("a fresh destination");
        (path, file)
    }

    fn found(downloads: &Downloads, job: u64, accounts: Vec<DownloadAccount>) -> Searched {
        downloads.find(
            &downloads.job(job).expect("the job"),
            accounts,
            &BTreeSet::new(),
            &reaching,
        )
    }

    #[test]
    fn the_second_provider_answers_when_the_first_is_away_and_says_how_long_the_song_is() {
        const AUDIO: &[u8] = b"fLaC and the rest of a lossless song";
        let downloads = Downloads::default();
        let job = downloads.begin(echoes());

        let searched = found(
            &downloads,
            job,
            vec![
                account(MusicProvider::Subsonic, &unreached_address()),
                account(
                    MusicProvider::Monochrome,
                    &monochrome_serving(AUDIO, Duration::ZERO),
                ),
            ],
        );
        let offer = searched.offer.expect("an offer");

        assert_eq!(offer.job, job);
        assert_eq!(offer.provider, MusicProvider::Monochrome);
        assert_eq!(offer.total, Some(AUDIO.len() as u64));
        assert_eq!(offer.extension, "flac");
        assert_eq!(searched.tried, Tried::Offered);
        assert_eq!(
            downloads.progress(job),
            Some(DownloadProgress {
                phase: DownloadPhase::Finding,
                received: 0,
                total: Some(AUDIO.len() as u64),
                provider: Some(MusicProvider::Monochrome),
            })
        );
    }

    #[test]
    fn a_cancel_during_a_find_is_heard_at_once() {
        let downloads = Arc::new(Downloads::default());
        let job = downloads.begin(echoes());
        let server = monochrome_serving(b"fLaC", SEARCH_HELD_FOR);

        let finding = {
            let downloads = Arc::clone(&downloads);
            thread::spawn(move || {
                let began = Instant::now();
                let searched = found(
                    &downloads,
                    job,
                    vec![account(MusicProvider::Monochrome, &server)],
                );
                (searched, began.elapsed())
            })
        };
        thread::sleep(Duration::from_millis(300));
        downloads.cancel(job);
        let (searched, took) = finding.join().expect("the find ends");

        assert!(matches!(searched.offer, Err(Error::DownloadCancelled)));
        assert_eq!(searched.tried, Tried::Unheard);
        assert!(took < CANCEL_HEARD_WITHIN, "the cancel took {took:?}");
        assert_eq!(
            downloads.progress(job).map(|progress| progress.phase),
            Some(DownloadPhase::Cancelled)
        );
    }

    #[test]
    fn a_cancel_before_the_find_is_kept() {
        let downloads = Downloads::default();
        let job = downloads.begin(echoes());

        downloads.cancel(job);
        let searched = found(
            &downloads,
            job,
            vec![account(MusicProvider::Monochrome, &unreached_address())],
        );

        assert!(matches!(searched.offer, Err(Error::DownloadCancelled)));
    }

    #[test]
    fn a_refusal_reaches_the_listener_as_the_provider_and_its_status() {
        let downloads = Downloads::default();
        let refused = downloads.begin(echoes());
        let unreached = downloads.begin(echoes());
        let unprovided = downloads.begin(echoes());

        let refusal = found(
            &downloads,
            refused,
            vec![account(MusicProvider::Monochrome, &refusing(401))],
        )
        .offer
        .expect_err("a refusal");
        let away = found(
            &downloads,
            unreached,
            vec![account(MusicProvider::Monochrome, &unreached_address())],
        )
        .offer
        .expect_err("nobody there");
        let nobody = found(&downloads, unprovided, Vec::new())
            .offer
            .expect_err("no provider");

        assert!(matches!(
            refusal,
            Error::ProviderRefused {
                provider: MusicProvider::Monochrome,
                status: 401
            }
        ));
        assert_eq!(refusal.to_string(), "Monochrome refused the request (401)");
        assert!(matches!(
            away,
            Error::Unreachable {
                provider: MusicProvider::Monochrome
            }
        ));
        assert!(matches!(nobody, Error::NoProvider));
        assert_eq!(
            downloads.progress(refused).map(|progress| progress.phase),
            Some(DownloadPhase::Failed)
        );
    }

    #[test]
    fn a_hifi_api_account_with_a_key_is_asked_though_hosted_services_are_declined() {
        let downloads = Downloads::default();
        downloads.hosted_services_allowed(false);
        let keyed = DownloadAccount {
            password: "key".to_owned(),
            ..account(MusicProvider::HifiApi, "")
        };
        let monochrome = account(MusicProvider::Monochrome, "");

        assert!(downloads.is_asked(&keyed));
        assert!(!downloads.is_asked(&monochrome));
    }

    #[test]
    fn declined_hosted_services_leave_a_hosted_account_unasked_and_a_named_server_asked() {
        let downloads = Downloads::default();
        downloads.hosted_services_allowed(false);
        let hosted = downloads.begin(echoes());
        let named = downloads.begin(echoes());

        let unasked = found(
            &downloads,
            hosted,
            vec![
                account(MusicProvider::Monochrome, ""),
                DownloadAccount {
                    password: String::new(),
                    ..account(MusicProvider::HifiApi, "  ")
                },
            ],
        )
        .offer
        .expect_err("nothing asked");
        let asked = found(
            &downloads,
            named,
            vec![account(MusicProvider::Monochrome, &unreached_address())],
        )
        .offer
        .expect_err("nobody there");

        assert!(matches!(unasked, Error::NoProvider));
        assert!(matches!(asked, Error::Unreachable { .. }));
    }

    #[test]
    fn every_provider_failure_is_a_sentence_naming_its_cause() {
        let tidal = SourceId::new("tidal").expect("a source");
        let op = resonate_providers::ProviderOp::Search;
        let cases = [
            (
                resonate_providers::Error::Refused {
                    provider: tidal.clone(),
                    op,
                    status: 401,
                },
                "TIDAL refused the request (401)",
            ),
            (
                resonate_providers::Error::Io {
                    provider: tidal.clone(),
                    op,
                    source: io::Error::from(io::ErrorKind::ConnectionRefused),
                },
                "TIDAL could not be reached",
            ),
            (
                resonate_providers::Error::Io {
                    provider: tidal.clone(),
                    op,
                    source: io::Error::from(io::ErrorKind::TimedOut),
                },
                "TIDAL did not answer in time",
            ),
            (
                resonate_providers::Error::Unwelcome {
                    provider: tidal.clone(),
                    op,
                    code: 11003,
                },
                "TIDAL did not accept the account (error 11003)",
            ),
            (
                resonate_providers::Error::TurnedAway {
                    provider: tidal.clone(),
                    op,
                    code: 70,
                },
                "No provider had this song",
            ),
            (
                resonate_providers::Error::Unreadable {
                    provider: tidal,
                    op,
                },
                "TIDAL answered with something that could not be read",
            ),
        ];

        for (refusal, told) in cases {
            assert_eq!(failure_of(MusicProvider::Tidal, &refusal).to_string(), told);
        }
    }

    fn briefly_holding(held_for: Duration) -> Downloads {
        Downloads {
            offer_held_for: OfferHeldFor(held_for),
            ..Downloads::default()
        }
    }

    fn phase_of(downloads: &Downloads, job: u64) -> DownloadPhase {
        downloads.progress(job).expect("the job").phase
    }

    #[test]
    fn an_offer_nobody_takes_is_let_go_and_its_job_fails() {
        const HELD_FOR: Duration = Duration::from_millis(100);
        const LET_GO_WITHIN: Duration = Duration::from_secs(5);
        let downloads = briefly_holding(HELD_FOR);
        let job = downloads.begin(echoes());
        let server = monochrome_serving(b"fLaC", Duration::ZERO);

        found(
            &downloads,
            job,
            vec![account(MusicProvider::Monochrome, &server)],
        )
        .offer
        .expect("an offer");
        let waited = Instant::now();
        while phase_of(&downloads, job) != DownloadPhase::Failed {
            assert!(
                waited.elapsed() < LET_GO_WITHIN,
                "the offer was never let go"
            );
            thread::sleep(Duration::from_millis(10));
        }
        let (path, file) = destination("let-go");
        let refused = downloads.download(&downloads.job(job).expect("the job"), file, &reaching);

        assert!(matches!(refused, Err(Error::DownloadExpired)));
        assert!(
            downloads
                .job(job)
                .expect("the job")
                .standing
                .lock()
                .opened
                .is_none()
        );

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn an_offer_taken_in_time_is_not_let_go_afterwards() {
        const AUDIO: &[u8] = b"fLaC taken at once";
        const HELD_FOR: Duration = Duration::from_millis(100);
        let downloads = briefly_holding(HELD_FOR);
        let job = downloads.begin(echoes());
        let server = monochrome_serving(AUDIO, Duration::ZERO);

        found(
            &downloads,
            job,
            vec![account(MusicProvider::Monochrome, &server)],
        )
        .offer
        .expect("an offer");
        let (path, file) = destination("taken");
        let copied = downloads
            .download(&downloads.job(job).expect("the job"), file, &reaching)
            .expect("the song downloads");
        thread::sleep(HELD_FOR * 4);

        assert_eq!(copied, AUDIO.len() as u64);
        assert_eq!(phase_of(&downloads, job), DownloadPhase::Downloading);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn two_jobs_move_on_their_own() {
        const SHORT: &[u8] = b"fLaC short";
        const LONG: &[u8] = b"fLaC a much longer song than the other one";
        let downloads = Downloads::default();
        let short = downloads.begin(echoes());
        let long = downloads.begin(echoes());

        for (job, audio) in [(short, SHORT), (long, LONG)] {
            let server = monochrome_serving(audio, Duration::ZERO);
            found(
                &downloads,
                job,
                vec![account(MusicProvider::Monochrome, &server)],
            )
            .offer
            .expect("an offer");
        }
        let (path, file) = destination("short");
        let copied = downloads
            .download(&downloads.job(short).expect("the job"), file, &reaching)
            .expect("the short song downloads");

        assert_eq!(copied, SHORT.len() as u64);
        assert_eq!(std::fs::read(&path).expect("the written song"), SHORT);
        assert_eq!(
            downloads.progress(short),
            Some(DownloadProgress {
                phase: DownloadPhase::Downloading,
                received: SHORT.len() as u64,
                total: Some(SHORT.len() as u64),
                provider: Some(MusicProvider::Monochrome),
            })
        );
        assert_eq!(
            downloads.progress(long),
            Some(DownloadProgress {
                phase: DownloadPhase::Finding,
                received: 0,
                total: Some(LONG.len() as u64),
                provider: Some(MusicProvider::Monochrome),
            })
        );

        downloads.cancel(long);
        let (cancelled_path, cancelled) = destination("long");
        let refused =
            downloads.download(&downloads.job(long).expect("the job"), cancelled, &reaching);

        assert!(matches!(refused, Err(Error::DownloadExpired)));
        assert_eq!(
            downloads.progress(short).map(|progress| progress.phase),
            Some(DownloadPhase::Downloading)
        );
        assert_eq!(
            downloads.progress(long).map(|progress| progress.phase),
            Some(DownloadPhase::Cancelled)
        );

        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(cancelled_path);
    }

    fn unreached_tidal() -> resonate_tidal::TidalSignIn {
        let unreached = unreached_address();
        resonate_tidal::TidalSignIn::at(resonate_tidal::Endpoints {
            auth: unreached.clone(),
            device: unreached,
            ..resonate_tidal::Endpoints::tidal()
        })
    }

    fn waiting_for_a_code() -> Authorizing {
        Authorizing::new(
            "ABCDE".to_owned(),
            "https://link.tidal.com/ABCDE".to_owned(),
            Duration::from_secs(300),
            Duration::from_secs(30),
            "device".to_owned(),
        )
    }

    fn tidal_client() -> Client {
        Client {
            id: "client".to_owned(),
            secret: None,
        }
    }

    #[test]
    fn a_sign_in_cancelled_while_its_code_was_asked_for_is_never_waited_for() {
        let downloads = Downloads::default();
        let tidal = unreached_tidal();

        let attempt = downloads.sign_in_asked();
        downloads.cancel_sign_in();
        downloads.sign_in_began(attempt, tidal_client(), waiting_for_a_code());
        let answer = downloads.signed_in(&tidal, &|| false);

        assert!(matches!(answer, Err(Error::DownloadExpired)));
    }

    #[test]
    fn a_sign_in_cancelled_and_asked_again_waits_for_the_new_code_alone() {
        let downloads = Downloads::default();

        let cancelled = downloads.sign_in_asked();
        downloads.cancel_sign_in();
        let again = downloads.sign_in_asked();
        downloads.sign_in_began(again, tidal_client(), waiting_for_a_code());
        downloads.sign_in_began(cancelled, tidal_client(), waiting_for_a_code());
        let kept = downloads
            .authorizing
            .lock()
            .as_ref()
            .map(|signing| signing.attempt);

        assert_eq!(kept, Some(again));
    }

    #[test]
    fn a_cancel_ends_a_sign_in_waiting_for_its_code() {
        let downloads = Downloads::default();
        let tidal = unreached_tidal();

        let attempt = downloads.sign_in_asked();
        downloads.sign_in_began(attempt, tidal_client(), waiting_for_a_code());
        let began = Instant::now();
        let answer = thread::scope(|scope| {
            scope.spawn(|| {
                thread::sleep(Duration::from_millis(200));
                downloads.cancel_sign_in();
            });
            downloads.signed_in(&tidal, &|| false)
        });

        assert!(matches!(answer, Ok(None)));
        assert!(began.elapsed() < Duration::from_secs(5));
    }

    mod validating {
        use super::*;
        use crate::harness::{FOLDER, Harness, SECONDS_A_SONG};

        fn uri() -> String {
            format!("{FOLDER}a.wav")
        }

        #[test]
        fn a_delivery_of_the_length_the_song_names_is_the_song() {
            let harness = Harness::new();
            let length = u64::from(SECONDS_A_SONG) * 1_000;

            harness
                .player
                .validate_download(uri(), Some(length))
                .expect("a song of its own length");
        }

        #[test]
        fn a_delivery_within_the_slack_the_providers_allow_is_still_the_song() {
            let harness = Harness::new();
            let length = u64::from(SECONDS_A_SONG) * 1_000;
            let slack = resonate_providers::LENGTHS_AGREE_WITHIN.as_millis() as u64;

            harness
                .player
                .validate_download(uri(), Some(length + slack))
                .expect("a song a little long");
        }

        #[test]
        fn a_delivery_of_another_length_is_the_wrong_song() {
            let harness = Harness::new();

            let refused = harness.player.validate_download(uri(), Some(60_000));

            assert!(matches!(refused, Err(Error::WrongSong)), "{refused:?}");
        }

        #[test]
        fn a_song_naming_no_length_asks_none_of_the_delivery() {
            let harness = Harness::new();

            harness
                .player
                .validate_download(uri(), None)
                .expect("no length to disagree with");
        }

        #[test]
        fn a_document_that_will_not_open_is_not_validated() {
            let harness = Harness::new();

            let refused = harness
                .player
                .validate_download(format!("{FOLDER}missing.wav"), Some(2_000));

            assert!(refused.is_err());
        }
    }
}
