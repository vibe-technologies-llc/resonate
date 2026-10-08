use std::{
    collections::VecDeque,
    ffi::OsStr,
    fs::{self, File},
    io::{self, Read},
    num::NonZeroUsize,
    os::unix::fs::MetadataExt as _,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SendError, SyncSender},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use ahash::{AHashMap, AHashSet};
use parking_lot::Mutex;
use resonate_codec::Sources;
use resonate_core::{Frames, MediaLocation, SampleRate, SourceId, WantId};
use resonate_providers::{
    Asking, Away, Delivered, Delivery, Identity, Opened, Opening, Providers, TURNED_TO_APART,
    WAITED_ON_AFTER_AN_OFFER,
};
use resonate_vault::{Halt, Keeping, Taking};

use crate::{
    Error, Library, Result, ScanOptions, Want,
    filed::{self, Filed, Unfiled},
    linked::LENGTHS_AGREE_WITHIN,
    organise,
    pass::{Cancelling, PassHandle, PassKind, PollHandle},
};

pub const POLL_AGAIN_AFTER: Duration = Duration::from_secs(6 * 60 * 60);
pub(crate) const REFUSALS_REMEMBERED_FOR: Duration = Duration::from_secs(30 * 24 * 60 * 60);
pub const RETRY_WAITS: [Duration; 5] = [
    Duration::from_secs(60),
    Duration::from_secs(5 * 60),
    Duration::from_secs(15 * 60),
    Duration::from_secs(60 * 60),
    Duration::from_secs(6 * 60 * 60),
];
pub const TRIES_BEFORE_GIVING_UP: u32 = RETRY_WAITS.len() as u32 + 1;
pub const ANSWERS_WITHIN: Duration = Duration::from_secs(30);
const CHUNK_BYTES: usize = 64 * 1024;
const CHUNKS_AHEAD: usize = 4;
const HEEDED_EVERY: Duration = Duration::from_millis(50);
pub const WANTS_ASKED_AT_ONCE: NonZeroUsize = NonZeroUsize::new(3).unwrap();

impl Want {
    pub fn identity(&self) -> Identity {
        Identity {
            recording: self.recording.clone(),
            track: self.track.clone(),
            release: self.release.clone(),
            isrc: self.isrc.clone(),
            title: self.title.clone(),
            artist: self.artist.clone(),
            album: Some(self.album_title.clone()),
            length: self.length,
            disc: Some(self.disc),
            position: Some(self.position),
            links: self.links.clone(),
            release_links: self.release_links.clone(),
        }
    }

    pub const fn gave_up(&self) -> bool {
        self.held.is_none() && self.misses >= TRIES_BEFORE_GIVING_UP
    }

    pub(crate) fn lasts_as_long_as(&self, heard: Frames, rate: SampleRate) -> bool {
        let heard = heard.to_duration(rate);
        self.length
            .is_none_or(|wanted| wanted.abs_diff(heard) <= LENGTHS_AGREE_WITHIN)
    }

    pub fn due_at(&self) -> Option<SystemTime> {
        if self.held.is_some() {
            return None;
        }
        let Some(tried) = self.tried else {
            return Some(self.wanted);
        };
        let wait = match self.misses {
            0 => POLL_AGAIN_AFTER,
            misses => *RETRY_WAITS.get(misses as usize - 1)?,
        };

        tried.checked_add(wait)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ForgottenDelivery {
    pub(crate) taken_from: String,
    pub(crate) forgotten: SystemTime,
}

impl ForgottenDelivery {
    fn declines(&self, delivered: &Delivered) -> bool {
        if delivered.taken_from().to_uri() != self.taken_from {
            return false;
        }
        match &delivered.delivery {
            Delivery::Stream { .. } => true,
            Delivery::File(path) => {
                last_changed(path).is_some_and(|changed| changed <= self.forgotten)
            }
        }
    }
}

fn last_changed(path: &Path) -> Option<SystemTime> {
    let metadata = fs::metadata(path).ok()?;
    metadata.modified().ok().max(status_changed(&metadata))
}

fn status_changed(metadata: &fs::Metadata) -> Option<SystemTime> {
    let seconds = u64::try_from(metadata.ctime()).ok()?;
    let nanos = u32::try_from(metadata.ctime_nsec()).ok()?;
    SystemTime::UNIX_EPOCH.checked_add(Duration::new(seconds, nanos))
}

#[derive(Clone, Copy, Debug)]
pub struct PollOptions {
    pub every_want: bool,
    pub answers_within: Duration,
    pub lanes: NonZeroUsize,
}

impl PollOptions {
    pub const ASKING_EVERY_WANT: Self = Self {
        every_want: true,
        answers_within: ANSWERS_WITHIN,
        lanes: WANTS_ASKED_AT_ONCE,
    };
}

impl Default for PollOptions {
    fn default() -> Self {
        Self {
            every_want: false,
            answers_within: ANSWERS_WITHIN,
            lanes: WANTS_ASKED_AT_ONCE,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PollStats {
    pub asked: u64,
    pub offered: u64,
    pub kept: u64,
    pub unkept: u64,
    pub nothing: u64,
    pub refused: u64,
    pub late: u64,
}

#[derive(Debug)]
struct Lane {
    want: WantId,
    provider: Option<SourceId>,
    received: u64,
}

#[derive(Debug, Default)]
struct Rereading {
    wanted: bool,
    closed: bool,
    owed: bool,
}

#[derive(Debug, Default)]
pub struct PollProgress {
    asked: AtomicU64,
    offered: AtomicU64,
    kept: AtomicU64,
    unkept: AtomicU64,
    nothing: AtomicU64,
    refused: AtomicU64,
    late: AtomicU64,
    lanes: Mutex<Vec<Lane>>,
    rereading: Mutex<Rereading>,
    cancelled: AtomicBool,
}

impl PollProgress {
    pub fn snapshot(&self) -> PollStats {
        PollStats {
            asked: self.asked.load(Ordering::Relaxed),
            offered: self.offered.load(Ordering::Relaxed),
            kept: self.kept.load(Ordering::Relaxed),
            unkept: self.unkept.load(Ordering::Relaxed),
            nothing: self.nothing.load(Ordering::Relaxed),
            refused: self.refused.load(Ordering::Relaxed),
            late: self.late.load(Ordering::Relaxed),
        }
    }

    pub fn asking(&self) -> Option<WantId> {
        self.lanes.lock().last().map(|lane| lane.want)
    }

    pub fn asking_all(&self) -> Vec<WantId> {
        self.lanes.lock().iter().map(|lane| lane.want).collect()
    }

    pub fn asking_provider(&self) -> Option<SourceId> {
        self.lanes
            .lock()
            .last()
            .and_then(|lane| lane.provider.clone())
    }

    pub fn provider_of(&self, want: WantId) -> Option<SourceId> {
        self.lanes
            .lock()
            .iter()
            .find(|lane| lane.want == want)
            .and_then(|lane| lane.provider.clone())
    }

    pub fn received(&self) -> u64 {
        self.lanes.lock().iter().map(|lane| lane.received).sum()
    }

    pub fn received_for(&self, want: WantId) -> u64 {
        self.lanes
            .lock()
            .iter()
            .find(|lane| lane.want == want)
            .map_or(0, |lane| lane.received)
    }

    fn asks_about(&self, want: WantId) {
        self.lanes.lock().push(Lane {
            want,
            provider: None,
            received: 0,
        });
    }

    fn turns_to(&self, want: WantId, provider: Option<&SourceId>) {
        if let Some(lane) = self.lanes.lock().iter_mut().find(|lane| lane.want == want) {
            lane.provider = provider.cloned();
        }
    }

    fn received_more(&self, want: WantId, bytes: usize) {
        if let Some(lane) = self.lanes.lock().iter_mut().find(|lane| lane.want == want) {
            lane.received += bytes as u64;
        }
    }

    fn done_asking_about(&self, want: WantId) {
        self.lanes.lock().retain(|lane| lane.want != want);
    }

    pub fn wants_changed(&self) -> bool {
        let mut rereading = self.rereading.lock();
        if rereading.closed || self.is_cancelled() {
            return false;
        }
        rereading.wanted = true;
        true
    }

    fn take_wants_changed(&self) -> bool {
        std::mem::take(&mut self.rereading.lock().wanted)
    }

    fn close_unless_wants_changed(&self) -> bool {
        let mut rereading = self.rereading.lock();
        if std::mem::take(&mut rereading.wanted) {
            return false;
        }
        rereading.closed = true;
        true
    }

    fn is_closed(&self) -> bool {
        self.rereading.lock().closed
    }

    fn close(&self) {
        let mut rereading = self.rereading.lock();
        if rereading.closed {
            return;
        }
        rereading.owed = std::mem::take(&mut rereading.wanted);
        rereading.closed = true;
    }

    pub fn owes_a_poll(&self) -> bool {
        self.rereading.lock().owed
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

impl Cancelling for PollProgress {
    fn cancel(&self) {
        PollProgress::cancel(self);
    }
}

struct Landing<'a> {
    options: PollOptions,
    progress: &'a PollProgress,
    away: &'a Mutex<Away>,
    filed: &'a Mutex<Vec<Filed>>,
}

enum Landed {
    Kept(MediaLocation),
    NotTheSong,
    Unopened(Unopened),
    Unkept,
}

enum Unopened {
    Gone,
    Refused(resonate_providers::Error),
}

fn landed(
    library: &Library,
    want: &Want,
    delivered: Delivered,
    landing: &mut Landing<'_>,
) -> Result<Landed> {
    let Landing {
        options,
        progress,
        away,
        ..
    } = *landing;
    let taken_from = delivered.taken_from();
    let Some(vault) = library.vault().cloned() else {
        return filed_in_the_music_folder(library, want, delivered, landing);
    };

    let keeping = match delivered.delivery {
        Delivery::File(path) => {
            if let Ok(metadata) = fs::metadata(&path) {
                progress.received_more(
                    want.id,
                    usize::try_from(metadata.len()).unwrap_or(usize::MAX),
                );
            }
            vault.keep(&Taking {
                sources: &Sources::local(),
                location: &MediaLocation::local(path),
                span: None,
                renewing: false,
                foretold: None,
                replacing: None,
                halt: Halt::NEVER,
            })
        }
        Delivery::Stream {
            extension, opening, ..
        } => {
            let mut pumped = match Pumped::from(opening, progress, want.id, options.answers_within)
            {
                Ok(pumped) => pumped,
                Err(source) => return Err(Error::ThreadSpawn { source }),
            };
            let keeping = vault.keep_delivered(&mut pumped, extension.as_str());
            if let Some(unopened) = pumped.unopened.take() {
                return Ok(Landed::Unopened(unopened));
            }
            if pumped.stalled {
                tracing::warn!(%taken_from, "a delivery stopped sending and was given up");
                progress.late.fetch_add(1, Ordering::Relaxed);
                away.lock().note(&delivered.provider);
                return Ok(Landed::Unkept);
            }
            keeping
        }
    };

    match keeping {
        Ok(Keeping::Kept(kept)) if !want.lasts_as_long_as(kept.frames, kept.spec.rate) => {
            tracing::warn!(
                %taken_from,
                wanted = ?want.length,
                heard = ?kept.frames.to_duration(kept.spec.rate),
                "a delivery not as long as the track it was wanted for was refused"
            );
            progress.unkept.fetch_add(1, Ordering::Relaxed);
            Ok(Landed::NotTheSong)
        }
        Ok(Keeping::Kept(kept)) => {
            if library.note_delivered(want, &kept, &taken_from)?.is_none() {
                tracing::info!(%taken_from, want = %want.id, "a delivery landed after its wanted row was held by another and was not paired");
                progress.unkept.fetch_add(1, Ordering::Relaxed);
                return Ok(Landed::Unkept);
            }
            progress.kept.fetch_add(1, Ordering::Relaxed);
            Ok(Landed::Kept(MediaLocation::local(&kept.path)))
        }
        _ if progress.is_cancelled() => Ok(Landed::Unkept),
        Err(source) => {
            tracing::warn!(%taken_from, %source, "a delivered track could not be kept");
            progress.unkept.fetch_add(1, Ordering::Relaxed);
            Ok(Landed::Unkept)
        }
        Ok(Keeping::Refused(refusal)) => {
            tracing::warn!(%taken_from, refused = refusal.as_str(), "a delivered track was refused");
            progress.unkept.fetch_add(1, Ordering::Relaxed);
            Ok(Landed::NotTheSong)
        }
    }
}

type Chunk = io::Result<Vec<u8>>;

enum Pumping {
    Read(Chunk),
    Unopened(Unopened),
}

struct Pumped<'a> {
    want: WantId,
    chunks: Receiver<Pumping>,
    held: Vec<u8>,
    read: usize,
    ended: bool,
    stalled: bool,
    unopened: Option<Unopened>,
    stalls_after: Duration,
    progress: &'a PollProgress,
}

impl<'a> Pumped<'a> {
    fn from(
        opening: Opening,
        progress: &'a PollProgress,
        want: WantId,
        stalls_after: Duration,
    ) -> io::Result<Self> {
        let (sender, chunks) = mpsc::sync_channel(CHUNKS_AHEAD);
        thread::Builder::new()
            .name("resonate-delivery".to_owned())
            .spawn(move || opened_and_pumped(opening, &sender))?;

        Ok(Self {
            want,
            chunks,
            held: Vec::new(),
            read: 0,
            ended: false,
            stalled: false,
            unopened: None,
            stalls_after,
            progress,
        })
    }

    fn next_chunk(&mut self) -> Chunk {
        let began = Instant::now();
        loop {
            match self.chunks.recv_timeout(HEEDED_EVERY) {
                Ok(Pumping::Read(chunk)) => return chunk,
                Ok(Pumping::Unopened(unopened)) => {
                    self.unopened = Some(unopened);
                    return Err(io::Error::from(io::ErrorKind::NotFound));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::from(io::ErrorKind::BrokenPipe));
                }
                Err(RecvTimeoutError::Timeout) => {
                    if self.progress.is_cancelled() {
                        return Err(io::Error::from(io::ErrorKind::ConnectionAborted));
                    }
                    if began.elapsed() >= self.stalls_after {
                        self.stalled = true;
                        return Err(io::Error::from(io::ErrorKind::TimedOut));
                    }
                }
            }
        }
    }
}

impl Read for Pumped<'_> {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.progress.is_cancelled() {
                return Err(io::Error::from(io::ErrorKind::ConnectionAborted));
            }
            let left = &self.held[self.read..];
            if !left.is_empty() || into.is_empty() {
                let taken = left.len().min(into.len());
                into[..taken].copy_from_slice(&left[..taken]);
                self.read += taken;
                return Ok(taken);
            }
            if self.ended {
                return Ok(0);
            }
            self.held = self.next_chunk()?;
            self.progress.received_more(self.want, self.held.len());
            self.read = 0;
            self.ended = self.held.is_empty();
        }
    }
}

struct Counted<'a, R> {
    reader: R,
    progress: &'a PollProgress,
    want: WantId,
}

impl<R: Read> Read for Counted<'_, R> {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        let read = self.reader.read(into)?;
        self.progress.received_more(self.want, read);
        Ok(read)
    }
}

fn opened_and_pumped(opening: Opening, into: &SyncSender<Pumping>) {
    match opening.open() {
        Ok(Opened::Reading(mut reader)) => pump(&mut *reader, into),
        Ok(Opened::Gone) => {
            let _ = into.send(Pumping::Unopened(Unopened::Gone));
        }
        Err(refused) => {
            let _ = into.send(Pumping::Unopened(Unopened::Refused(refused)));
        }
    }
}

fn pump(reader: &mut dyn Read, into: &SyncSender<Pumping>) {
    loop {
        let mut chunk = vec![0; CHUNK_BYTES];
        let answered = match reader.read(&mut chunk) {
            Ok(read) => {
                chunk.truncate(read);
                Ok(chunk)
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => Err(error),
        };
        let last = answered.as_ref().map_or(true, Vec::is_empty);
        if into.send(Pumping::Read(answered)).is_err() || last {
            return;
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PollSummary {
    pub stats: PollStats,
    pub cancelled: bool,
}

pub(crate) fn start(
    library: Library,
    providers: Arc<Providers>,
    options: PollOptions,
) -> Result<PollHandle> {
    let progress = Arc::new(PollProgress::default());
    let owned = Arc::clone(&progress);
    let asking = library.asking_alone(PassKind::Poll)?;

    let thread = thread::Builder::new()
        .name("resonate-poll".to_owned())
        .spawn(move || {
            let _asking = asking;
            let outcome = run(&library, &providers, options, &progress);
            progress.close();
            outcome
        })
        .map_err(|source| Error::ThreadSpawn { source })?;

    Ok(PassHandle::of(PassKind::Poll, owned, thread))
}

fn filed_in_the_music_folder(
    library: &Library,
    want: &Want,
    delivered: Delivered,
    landing: &mut Landing<'_>,
) -> Result<Landed> {
    let progress = landing.progress;
    let taken_from = delivered.taken_from();
    let Some(into) = library.delivering_into() else {
        return Ok(match delivered.delivery {
            Delivery::File(_) => Landed::Kept(taken_from),
            Delivery::Stream { .. } => {
                tracing::warn!(%taken_from, "a streamed delivery has neither a vault nor a music folder to land in");
                progress.unkept.fetch_add(1, Ordering::Relaxed);
                Landed::Unkept
            }
        });
    };

    let outcome = match delivered.delivery {
        Delivery::File(path) => {
            let extension = path
                .extension()
                .and_then(OsStr::to_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            match File::open(&path) {
                Ok(file) => filed::filed(
                    library,
                    &into,
                    want,
                    &mut Counted {
                        reader: file,
                        progress,
                        want: want.id,
                    },
                    &extension,
                ),
                Err(error) => Err(Unfiled::Io(error)),
            }
        }
        Delivery::Stream {
            extension, opening, ..
        } => {
            let mut pumped =
                Pumped::from(opening, progress, want.id, landing.options.answers_within)
                    .map_err(|source| Error::ThreadSpawn { source })?;
            let outcome = filed::filed(library, &into, want, &mut pumped, extension.as_str());
            if let Some(unopened) = pumped.unopened.take() {
                return Ok(Landed::Unopened(unopened));
            }
            if pumped.stalled {
                tracing::warn!(%taken_from, "a delivery stopped sending and was given up");
                progress.late.fetch_add(1, Ordering::Relaxed);
                landing.away.lock().note(&delivered.provider);
                return Ok(Landed::Unkept);
            }
            outcome
        }
    };

    match outcome {
        Ok(landed) => {
            progress.kept.fetch_add(1, Ordering::Relaxed);
            let at = MediaLocation::local(&landed.path);
            landing.filed.lock().push(landed);
            Ok(Landed::Kept(at))
        }
        _ if progress.is_cancelled() => Ok(Landed::Unkept),
        Err(Unfiled::Catalog(error)) => Err(error),
        Err(Unfiled::Io(error)) => {
            tracing::warn!(%taken_from, %error, "a delivered track could not be written into the music folder");
            progress.unkept.fetch_add(1, Ordering::Relaxed);
            Ok(Landed::Unkept)
        }
        Err(Unfiled::NotAsLongAsWanted { heard }) => {
            tracing::warn!(
                %taken_from,
                wanted = ?want.length,
                ?heard,
                "a delivery not as long as the track it was wanted for was refused"
            );
            progress.unkept.fetch_add(1, Ordering::Relaxed);
            Ok(Landed::NotTheSong)
        }
        Err(unfiled @ (Unfiled::TooLarge | Unfiled::Undecodable)) => {
            tracing::warn!(%taken_from, ?unfiled, "a delivered track was refused");
            progress.unkept.fetch_add(1, Ordering::Relaxed);
            Ok(Landed::NotTheSong)
        }
        Err(unfiled) => {
            tracing::warn!(%taken_from, ?unfiled, "a delivered track could not be filed in the music folder");
            progress.unkept.fetch_add(1, Ordering::Relaxed);
            Ok(Landed::Unkept)
        }
    }
}

fn scanning(filed: &[Filed]) -> ScanOptions {
    ScanOptions {
        roots: filed::roots_of(filed),
        incremental: true,
        follow_symlinks: false,
        extract_cover_art: true,
        workers: thread::available_parallelism().unwrap_or(NonZeroUsize::MIN),
    }
}

fn paired(library: &Library) -> Result<()> {
    let paired = library.pair_what_landed()?;
    if paired > 0 {
        tracing::info!(paired, "wants were paired with the tracks filed for them");
    }
    Ok(())
}

fn scanned_and_paired(library: &Library, filed: &[Filed]) -> Result<()> {
    if !filed.is_empty() {
        match library.scan(scanning(filed)).and_then(PassHandle::join) {
            Ok(summary) => {
                tracing::debug!(?summary, "the folders deliveries were filed in were read")
            }
            Err(error) => {
                tracing::warn!(%error, "the folders deliveries were filed in could not be read yet");
            }
        }
    }
    paired(library)
}

type Forgotten = AHashMap<WantId, Vec<ForgottenDelivery>>;

struct Queue {
    pending: VecDeque<Want>,
    claimed: AHashSet<WantId>,
    forgotten: Arc<Forgotten>,
    read: bool,
    busy: usize,
}

enum Claim {
    Want(Box<Want>, Arc<Forgotten>),
    Idle,
    Done,
}

struct Lanes<'a> {
    library: &'a Library,
    providers: &'a Providers,
    options: PollOptions,
    progress: &'a PollProgress,
    queue: Mutex<Queue>,
    away: Mutex<Away>,
    filed: Mutex<Vec<Filed>>,
    failure: Mutex<Option<Error>>,
    first_answered: AtomicBool,
}

enum Flow {
    Onward,
    Stop,
}

struct Claimed<'l, 'a> {
    lanes: &'l Lanes<'a>,
    want: Box<Want>,
    forgotten: Arc<Forgotten>,
}

impl Drop for Claimed<'_, '_> {
    fn drop(&mut self) {
        self.lanes.progress.done_asking_about(self.want.id);
        self.lanes.released();
    }
}

struct Rounds {
    passing: Vec<SourceId>,
    every_provider_heard: bool,
}

impl Rounds {
    const fn first() -> Self {
        Self {
            passing: Vec::new(),
            every_provider_heard: true,
        }
    }
}

enum Asked {
    Settled(Flow),
    Offered(Vec<Delivered>),
}

struct Keep<'l, 'a> {
    claimed: Claimed<'l, 'a>,
    rounds: Rounds,
    offers: Vec<Delivered>,
}

impl<'a> Lanes<'a> {
    fn refill(&self, queue: &mut Queue) -> Result<()> {
        let now = SystemTime::now();
        queue.forgotten = Arc::new(self.library.forgotten_deliveries()?);
        queue.read = true;
        queue.pending = self
            .library
            .wants_unheld()?
            .into_iter()
            .filter(|want| !queue.claimed.contains(&want.id) && due(want, self.options, now))
            .collect();
        Ok(())
    }

    fn claim(&self) -> Result<Claim> {
        let mut queue = self.queue.lock();
        if self.progress.take_wants_changed() || !queue.read {
            self.refill(&mut queue)?;
        }
        loop {
            if let Some(want) = queue.pending.pop_front() {
                if queue.claimed.insert(want.id) {
                    queue.busy += 1;
                    return Ok(Claim::Want(Box::new(want), Arc::clone(&queue.forgotten)));
                }
                continue;
            }
            if queue.busy > 0 {
                return Ok(Claim::Idle);
            }
            if self.progress.close_unless_wants_changed() {
                return Ok(Claim::Done);
            }
            self.refill(&mut queue)?;
        }
    }

    fn claimed<'l>(&'l self, want: Box<Want>, forgotten: Arc<Forgotten>) -> Claimed<'l, 'a> {
        self.progress.asked.fetch_add(1, Ordering::Relaxed);
        self.progress.asks_about(want.id);
        Claimed {
            lanes: self,
            want,
            forgotten,
        }
    }

    fn released(&self) {
        self.queue.lock().busy -= 1;
    }

    fn fail(&self, error: Error) {
        let mut failure = self.failure.lock();
        if failure.is_none() {
            *failure = Some(error);
        }
    }

    fn has_failed(&self) -> bool {
        self.failure.lock().is_some()
    }

    fn lane(&self, leads: bool) {
        let (handing, handed) = mpsc::sync_channel::<Keep<'_, 'a>>(0);
        thread::scope(|scope| {
            let keeper = thread::Builder::new()
                .name("resonate-keeper".to_owned())
                .spawn_scoped(scope, move || self.keep_what_is_handed(&handed))
                .map_err(|error| {
                    tracing::warn!(%error, "a poll lane's keeper did not start, so the lane keeps what it is offered itself");
                })
                .ok();
            let handing = keeper.is_some().then_some(handing);
            self.ask_until_done(leads, handing.as_ref());
            drop(handing);
            if let Some(keeper) = keeper
                && keeper.join().is_err()
            {
                self.fail(Error::Stopped {
                    pass: PassKind::Poll,
                });
            }
        });
    }

    fn ask_until_done<'l>(&'l self, leads: bool, keeper: Option<&SyncSender<Keep<'l, 'a>>>) {
        loop {
            if self.progress.is_cancelled() || self.has_failed() || self.progress.is_closed() {
                return;
            }
            if !leads && !self.first_answered.load(Ordering::Acquire) {
                thread::sleep(HEEDED_EVERY);
                continue;
            }
            match self.claim() {
                Ok(Claim::Want(want, forgotten)) => {
                    let claimed = self.claimed(want, forgotten);
                    let mut rounds = Rounds::first();
                    let flow = match self.asked(&claimed, &mut rounds) {
                        Ok(Asked::Offered(offers)) => {
                            let keep = Keep {
                                claimed,
                                rounds,
                                offers,
                            };
                            let unhanded = match keeper {
                                Some(keeper) => keeper.send(keep).err().map(|SendError(keep)| keep),
                                None => Some(keep),
                            };
                            match unhanded {
                                Some(keep) => self.kept(keep),
                                None => continue,
                            }
                        }
                        Ok(Asked::Settled(flow)) => Ok(flow),
                        Err(error) => Err(error),
                    };
                    match flow {
                        Ok(Flow::Onward) => {}
                        Ok(Flow::Stop) => return,
                        Err(error) => {
                            self.fail(error);
                            return;
                        }
                    }
                }
                Ok(Claim::Idle) => thread::sleep(HEEDED_EVERY),
                Ok(Claim::Done) => return,
                Err(error) => {
                    self.fail(error);
                    return;
                }
            }
        }
    }

    fn keep_what_is_handed(&self, handed: &Receiver<Keep<'_, 'a>>) {
        for keep in handed {
            if let Err(error) = self.kept(keep) {
                self.fail(error);
            }
        }
    }

    fn pair_what_was_filed(&self) -> Result<()> {
        let filed = std::mem::take(&mut *self.filed.lock());
        if filed.is_empty() {
            return Ok(());
        }
        match self.library.scan(scanning(&filed)) {
            Ok(scanning) => {
                if let Err(error) = scanning.join() {
                    tracing::warn!(%error, "the folder a delivery was filed in could not be read yet");
                }
                paired(self.library)
            }
            Err(error) => {
                tracing::debug!(%error, "a delivery is paired when the poll ends, the tree being walked");
                self.filed.lock().extend(filed);
                Ok(())
            }
        }
    }

    fn asked(&self, claimed: &Claimed<'_, 'a>, rounds: &mut Rounds) -> Result<Asked> {
        let progress = self.progress;
        let want = &claimed.want;
        let cancelled = || progress.is_cancelled();
        let turning_to = |provider: &SourceId| progress.turns_to(want.id, Some(provider));
        let forgotten_for_it = claimed
            .forgotten
            .get(&want.id)
            .map_or(&[][..], Vec::as_slice);
        let declined = |delivered: &Delivered| {
            forgotten_for_it
                .iter()
                .any(|forgotten| forgotten.declines(delivered))
        };
        let mut away = self.away.lock().clone();
        let answer = self.providers.first(
            &want.identity(),
            &Asking {
                within: self.options.answers_within,
                cancelled: &cancelled,
                turning_to: &turning_to,
                declined: &declined,
                passing: &rounds.passing,
                apart: TURNED_TO_APART,
                grace: WAITED_ON_AFTER_AN_OFFER,
            },
            &mut away,
        );
        self.away.lock().join(&away);
        self.first_answered.store(true, Ordering::Release);
        progress
            .refused
            .fetch_add(answer.refused, Ordering::Relaxed);
        progress.late.fetch_add(answer.late, Ordering::Relaxed);
        rounds.every_provider_heard &= answer.heard_from_every_provider();
        if answer.delivered.is_none() {
            progress.turns_to(want.id, None);
        }
        if answer.cancelled {
            return Ok(Asked::Settled(Flow::Stop));
        }
        let Some(delivered) = answer.delivered else {
            if rounds.every_provider_heard {
                progress.nothing.fetch_add(1, Ordering::Relaxed);
                tried(self.library, want.id, None)?;
            } else {
                tracing::debug!(
                    want = %want.id,
                    refused = answer.refused,
                    late = answer.late,
                    passed_over = answer.passed_over,
                    narrowed = answer.narrowed,
                    "not every provider answered, so the want stays due"
                );
            }
            return Ok(Asked::Settled(Flow::Onward));
        };
        rounds.passing.extend(answer.heard);

        Ok(Asked::Offered(
            std::iter::once(delivered).chain(answer.held_back).collect(),
        ))
    }

    fn kept(&self, keep: Keep<'_, 'a>) -> Result<Flow> {
        let Keep {
            claimed,
            mut rounds,
            mut offers,
        } = keep;
        loop {
            for delivered in offers {
                match self.offer_landed(&claimed.want, delivered)? {
                    Offer::Settled(flow) => return Ok(flow),
                    Offer::FellThrough { heard } => rounds.every_provider_heard &= heard,
                }
            }
            match self.asked(&claimed, &mut rounds)? {
                Asked::Settled(flow) => return Ok(flow),
                Asked::Offered(again) => offers = again,
            }
        }
    }

    fn offer_landed(&self, want: &Want, delivered: Delivered) -> Result<Offer> {
        let progress = self.progress;
        progress.turns_to(want.id, Some(&delivered.provider));
        progress.offered.fetch_add(1, Ordering::Relaxed);
        let taken_from = delivered.taken_from();
        let provider = delivered.provider.clone();
        let landing = landed(
            self.library,
            want,
            delivered,
            &mut Landing {
                options: self.options,
                progress,
                away: &self.away,
                filed: &self.filed,
            },
        )?;
        if progress.is_cancelled() {
            if let Landed::Kept(at) = &landing {
                tried(self.library, want.id, Some(at))?;
            }
            return Ok(Offer::Settled(Flow::Stop));
        }
        match landing {
            Landed::Kept(at) => {
                tried(self.library, want.id, Some(&at))?;
                self.pair_what_was_filed()?;
                Ok(Offer::Settled(Flow::Onward))
            }
            Landed::Unkept => {
                tried(self.library, want.id, None)?;
                Ok(Offer::Settled(Flow::Onward))
            }
            Landed::NotTheSong => {
                self.library
                    .note_refused_offer(want.id, &taken_from, SystemTime::now())?;
                tracing::info!(want = %want.id, %taken_from, "a delivery that was not the song is passed over and the next offer taken");
                Ok(Offer::FellThrough { heard: true })
            }
            Landed::Unopened(Unopened::Gone) => {
                tracing::info!(want = %want.id, %taken_from, "an offer was gone by the time it was opened, and the next is taken");
                Ok(Offer::FellThrough { heard: true })
            }
            Landed::Unopened(Unopened::Refused(error)) => {
                tracing::warn!(%error, want = %want.id, %taken_from, "an offer could not be opened, and the next is taken");
                progress.refused.fetch_add(1, Ordering::Relaxed);
                if error.is_the_provider_away() {
                    self.away.lock().note(&provider);
                }
                Ok(Offer::FellThrough { heard: false })
            }
        }
    }
}

enum Offer {
    Settled(Flow),
    FellThrough { heard: bool },
}

fn run(
    library: &Library,
    providers: &Providers,
    options: PollOptions,
    progress: &PollProgress,
) -> Result<PollSummary> {
    organise::sweep_what_a_dead_writer_staged(library);
    scanned_and_paired(library, &[])?;
    let lanes = Lanes {
        library,
        providers,
        options,
        progress,
        queue: Mutex::new(Queue {
            pending: VecDeque::new(),
            claimed: AHashSet::new(),
            forgotten: Arc::new(Forgotten::new()),
            read: false,
            busy: 0,
        }),
        away: Mutex::new(Away::default()),
        filed: Mutex::new(Vec::new()),
        failure: Mutex::new(None),
        first_answered: AtomicBool::new(false),
    };

    thread::scope(|scope| {
        let started: Vec<_> = (1..options.lanes.get())
            .filter_map(|nth| {
                thread::Builder::new()
                    .name(format!("resonate-poll-{nth}"))
                    .spawn_scoped(scope, || lanes.lane(false))
                    .map_err(|error| {
                        tracing::warn!(%error, "a poll lane did not start");
                    })
                    .ok()
            })
            .collect();
        lanes.lane(true);
        for lane in started {
            let _ = lane.join();
        }
    });

    if let Some(error) = lanes.failure.into_inner() {
        return Err(error);
    }
    scanned_and_paired(library, &lanes.filed.into_inner())?;

    Ok(PollSummary {
        stats: progress.snapshot(),
        cancelled: progress.is_cancelled(),
    })
}

fn tried(library: &Library, want: WantId, offered: Option<&MediaLocation>) -> Result<()> {
    match library.note_tried(want, offered) {
        Err(Error::UnknownWant(_)) => {
            tracing::debug!(%want, "a want was dismissed or withdrawn while it was asked about");
            Ok(())
        }
        noted => noted,
    }
}

impl Library {
    pub fn is_a_want_due(&self, options: PollOptions) -> Result<bool> {
        let now = SystemTime::now();
        Ok(self
            .wants_as_they_stand()?
            .iter()
            .any(|want| due(want, options, now)))
    }

    pub fn next_want_due(&self) -> Result<Option<SystemTime>> {
        Ok(self
            .wants_as_they_stand()?
            .iter()
            .filter_map(Want::due_at)
            .min())
    }

    pub(crate) fn forgotten_deliveries(&self) -> Result<AHashMap<WantId, Vec<ForgottenDelivery>>> {
        let mut forgotten: AHashMap<WantId, Vec<ForgottenDelivery>> = AHashMap::new();
        for (want, delivery) in self.declined_offer_rows(SystemTime::now())? {
            forgotten.entry(want).or_default().push(delivery);
        }
        Ok(forgotten)
    }

    pub fn last_tried(&self) -> Result<Option<SystemTime>> {
        Ok(self
            .wants_as_they_stand()?
            .iter()
            .filter(|want| want.held.is_none())
            .filter_map(|want| want.tried)
            .max())
    }
}

fn due(want: &Want, options: PollOptions, now: SystemTime) -> bool {
    if want.held.is_some() {
        return false;
    }
    if options.every_want || want.tried.is_some_and(|tried| tried > now) {
        return true;
    }

    want.due_at().is_some_and(|at| at <= now)
}

#[cfg(test)]
mod tests {
    use resonate_core::{AlbumId, ReleaseTrackId, TrackId, WantId};

    use super::*;

    fn want(tried: Option<SystemTime>) -> Want {
        Want {
            id: WantId::new(1).expect("a non-zero id"),
            release_track: ReleaseTrackId::new(1).expect("a non-zero id"),
            album: AlbumId::new(1).expect("a non-zero id"),
            album_title: "Orbits".to_owned(),
            title: "San Tropez".to_owned(),
            artist: None,
            recording: None,
            track: None,
            release: None,
            isrc: None,
            length: None,
            disc: 1,
            position: 4,
            wanted: SystemTime::UNIX_EPOCH,
            tried,
            offered: None,
            misses: 0,
            held: None,
            links: Vec::new(),
            release_links: Vec::new(),
        }
    }

    fn at(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
    }

    #[test]
    fn a_want_never_tried_is_due_and_one_offered_is_due_again_after_six_hours() {
        let tried = at(1_000);
        let offered = Want {
            offered: Some("vault:abc".to_owned()),
            ..want(Some(tried))
        };
        let six_hours = POLL_AGAIN_AFTER.as_secs();

        assert!(due(&want(None), PollOptions::default(), at(1)));
        assert!(!due(
            &offered,
            PollOptions::default(),
            at(1_000 + six_hours - 1)
        ));
        assert!(due(&offered, PollOptions::default(), at(1_000 + six_hours)));
        assert!(due(&offered, PollOptions::default(), at(10)));
    }

    #[test]
    fn a_want_tried_in_vain_waits_longer_after_each_try_and_is_given_up_after_the_last() {
        let tried = at(1_000);
        let missed = |misses: u32| Want {
            misses,
            ..want(Some(tried))
        };

        for (misses, wait) in (1..).zip(RETRY_WAITS) {
            let after = 1_000 + wait.as_secs();
            assert!(!missed(misses).gave_up());
            assert!(!due(&missed(misses), PollOptions::default(), at(after - 1)));
            assert!(due(&missed(misses), PollOptions::default(), at(after)));
        }
        let given_up = missed(TRIES_BEFORE_GIVING_UP);

        assert!(given_up.gave_up());
        assert_eq!(given_up.due_at(), None);
        assert!(!due(
            &given_up,
            PollOptions::default(),
            at(u64::from(u32::MAX))
        ));
        assert!(due(&given_up, PollOptions::ASKING_EVERY_WANT, at(1_001)));
    }

    #[test]
    fn a_want_the_catalog_already_holds_a_row_for_is_never_due() {
        let held = Want {
            held: Some(TrackId::new(7).expect("a non-zero id")),
            ..want(None)
        };
        assert!(!due(
            &held,
            PollOptions::ASKING_EVERY_WANT,
            SystemTime::UNIX_EPOCH
        ));
        assert_eq!(held.due_at(), None);
    }
}
