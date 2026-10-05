use std::{
    ffi::OsStr,
    fs::{self, File},
    io::{self, Read},
    num::{NonZeroU64, NonZeroUsize},
    os::unix::fs::MetadataExt as _,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use ahash::AHashMap;
use parking_lot::Mutex;
use resonate_codec::Sources;
use resonate_core::{Frames, MediaLocation, SampleRate, SourceId, WantId};
use resonate_providers::{Asking, Away, Delivered, Delivery, Identity, Providers};
use resonate_vault::{Keeping, Taking};

use crate::{
    Error, Library, Result, ScanOptions, Want,
    filed::{self, Filed, Unfiled},
    linked::LENGTHS_AGREE_WITHIN,
    pass::{Cancelling, PassHandle, PassKind, PollHandle},
};

pub const POLL_AGAIN_AFTER: Duration = Duration::from_secs(6 * 60 * 60);
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
}

impl PollOptions {
    pub const ASKING_EVERY_WANT: Self = Self {
        every_want: true,
        answers_within: ANSWERS_WITHIN,
    };
}

impl Default for PollOptions {
    fn default() -> Self {
        Self {
            every_want: false,
            answers_within: ANSWERS_WITHIN,
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

#[derive(Debug, Default)]
pub struct PollProgress {
    asked: AtomicU64,
    offered: AtomicU64,
    kept: AtomicU64,
    unkept: AtomicU64,
    nothing: AtomicU64,
    refused: AtomicU64,
    late: AtomicU64,
    asking: AtomicU64,
    asking_provider: Mutex<Option<SourceId>>,
    received: AtomicU64,
    cancelled: AtomicBool,
}

const ASKING_NOTHING: u64 = 0;

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
        NonZeroU64::new(self.asking.load(Ordering::Relaxed)).map(WantId::of)
    }

    pub fn asking_provider(&self) -> Option<SourceId> {
        self.asking_provider.lock().clone()
    }

    pub fn received(&self) -> u64 {
        self.received.load(Ordering::Relaxed)
    }

    fn asks_about(&self, want: Option<WantId>) {
        self.turns_to(None);
        self.received.store(0, Ordering::Relaxed);
        self.asking
            .store(want.map_or(ASKING_NOTHING, WantId::get), Ordering::Relaxed);
    }

    fn turns_to(&self, provider: Option<&SourceId>) {
        *self.asking_provider.lock() = provider.cloned();
    }

    fn received_more(&self, bytes: usize) {
        self.received.fetch_add(bytes as u64, Ordering::Relaxed);
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
    away: &'a mut Away,
    filed: &'a mut Vec<Filed>,
}

fn landed(
    library: &Library,
    want: &Want,
    delivered: Delivered,
    landing: &mut Landing<'_>,
) -> Result<Option<MediaLocation>> {
    let Landing {
        options,
        progress,
        ref mut away,
        ..
    } = *landing;
    let taken_from = delivered.taken_from();
    let Some(vault) = library.vault().cloned() else {
        return filed_in_the_music_folder(library, want, delivered, landing);
    };

    let keeping = match delivered.delivery {
        Delivery::File(path) => {
            if let Ok(metadata) = fs::metadata(&path) {
                progress.received_more(usize::try_from(metadata.len()).unwrap_or(usize::MAX));
            }
            vault.keep(&Taking {
                sources: &Sources::local(),
                location: &MediaLocation::local(path),
                span: None,
                renewing: false,
                foretold: None,
            })
        }
        Delivery::Stream {
            extension, reader, ..
        } => {
            let mut pumped = match Pumped::from(reader, progress, options.answers_within) {
                Ok(pumped) => pumped,
                Err(source) => return Err(Error::ThreadSpawn { source }),
            };
            let keeping = vault.keep_delivered(&mut pumped, extension.as_str());
            if pumped.stalled {
                tracing::warn!(%taken_from, "a delivery stopped sending and was given up");
                progress.late.fetch_add(1, Ordering::Relaxed);
                away.note(&delivered.provider);
                return Ok(None);
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
            Ok(None)
        }
        Ok(Keeping::Kept(kept)) => {
            if library.note_delivered(want, &kept, &taken_from)?.is_none() {
                tracing::info!(%taken_from, want = %want.id, "a delivery landed after its wanted row was held by another and was not paired");
                progress.unkept.fetch_add(1, Ordering::Relaxed);
                return Ok(None);
            }
            progress.kept.fetch_add(1, Ordering::Relaxed);
            Ok(Some(MediaLocation::local(&kept.path)))
        }
        _ if progress.is_cancelled() => Ok(None),
        Err(source) => {
            tracing::warn!(%taken_from, %source, "a delivered track could not be kept");
            progress.unkept.fetch_add(1, Ordering::Relaxed);
            Ok(None)
        }
        Ok(Keeping::Refused(refusal)) => {
            tracing::warn!(%taken_from, refused = refusal.as_str(), "a delivered track was refused");
            progress.unkept.fetch_add(1, Ordering::Relaxed);
            Ok(None)
        }
    }
}

type Chunk = io::Result<Vec<u8>>;

struct Pumped<'a> {
    chunks: Receiver<Chunk>,
    held: Vec<u8>,
    read: usize,
    ended: bool,
    stalled: bool,
    stalls_after: Duration,
    progress: &'a PollProgress,
}

impl<'a> Pumped<'a> {
    fn from(
        mut reader: Box<dyn Read + Send>,
        progress: &'a PollProgress,
        stalls_after: Duration,
    ) -> io::Result<Self> {
        let (sender, chunks) = mpsc::sync_channel(CHUNKS_AHEAD);
        thread::Builder::new()
            .name("resonate-delivery".to_owned())
            .spawn(move || pump(&mut *reader, &sender))?;

        Ok(Self {
            chunks,
            held: Vec::new(),
            read: 0,
            ended: false,
            stalled: false,
            stalls_after,
            progress,
        })
    }

    fn next_chunk(&mut self) -> Chunk {
        let began = Instant::now();
        loop {
            match self.chunks.recv_timeout(HEEDED_EVERY) {
                Ok(chunk) => return chunk,
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
            self.progress.received_more(self.held.len());
            self.read = 0;
            self.ended = self.held.is_empty();
        }
    }
}

struct Counted<'a, R> {
    reader: R,
    progress: &'a PollProgress,
}

impl<R: Read> Read for Counted<'_, R> {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        let read = self.reader.read(into)?;
        self.progress.received_more(read);
        Ok(read)
    }
}

fn pump(reader: &mut dyn Read, into: &SyncSender<Chunk>) {
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
        if into.send(answered).is_err() || last {
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
            run(&library, &providers, options, &progress)
        })
        .map_err(|source| Error::ThreadSpawn { source })?;

    Ok(PassHandle::of(PassKind::Poll, owned, thread))
}

fn filed_in_the_music_folder(
    library: &Library,
    want: &Want,
    delivered: Delivered,
    landing: &mut Landing<'_>,
) -> Result<Option<MediaLocation>> {
    let progress = landing.progress;
    let taken_from = delivered.taken_from();
    let Some(into) = library.delivering_into() else {
        return Ok(match delivered.delivery {
            Delivery::File(_) => Some(taken_from),
            Delivery::Stream { .. } => {
                tracing::warn!(%taken_from, "a streamed delivery has neither a vault nor a music folder to land in");
                progress.unkept.fetch_add(1, Ordering::Relaxed);
                None
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
                    },
                    &extension,
                ),
                Err(error) => Err(Unfiled::Io(error)),
            }
        }
        Delivery::Stream {
            extension, reader, ..
        } => {
            let mut pumped = Pumped::from(reader, progress, landing.options.answers_within)
                .map_err(|source| Error::ThreadSpawn { source })?;
            let outcome = filed::filed(library, &into, want, &mut pumped, extension.as_str());
            if pumped.stalled {
                tracing::warn!(%taken_from, "a delivery stopped sending and was given up");
                progress.late.fetch_add(1, Ordering::Relaxed);
                landing.away.note(&delivered.provider);
                return Ok(None);
            }
            outcome
        }
    };

    match outcome {
        Ok(landed) => {
            progress.kept.fetch_add(1, Ordering::Relaxed);
            let at = MediaLocation::local(&landed.path);
            landing.filed.push(landed);
            Ok(Some(at))
        }
        _ if progress.is_cancelled() => Ok(None),
        Err(Unfiled::Catalog(error)) => Err(error),
        Err(Unfiled::Io(error)) => {
            tracing::warn!(%taken_from, %error, "a delivered track could not be written into the music folder");
            progress.unkept.fetch_add(1, Ordering::Relaxed);
            Ok(None)
        }
        Err(Unfiled::NotAsLongAsWanted { heard }) => {
            tracing::warn!(
                %taken_from,
                wanted = ?want.length,
                ?heard,
                "a delivery not as long as the track it was wanted for was refused"
            );
            progress.unkept.fetch_add(1, Ordering::Relaxed);
            Ok(None)
        }
        Err(unfiled) => {
            tracing::warn!(%taken_from, ?unfiled, "a delivered track could not be filed in the music folder");
            progress.unkept.fetch_add(1, Ordering::Relaxed);
            Ok(None)
        }
    }
}

fn scanned_and_paired(library: &Library, filed: &[Filed]) -> Result<()> {
    if !filed.is_empty() {
        let options = ScanOptions {
            roots: filed::roots_of(filed),
            incremental: true,
            follow_symlinks: false,
            extract_cover_art: true,
            workers: thread::available_parallelism().unwrap_or(NonZeroUsize::MIN),
        };
        match library.scan(options).and_then(PassHandle::join) {
            Ok(summary) => {
                tracing::debug!(?summary, "the folders deliveries were filed in were read")
            }
            Err(error) => {
                tracing::warn!(%error, "the folders deliveries were filed in could not be read yet");
            }
        }
    }
    let paired = library.pair_what_landed()?;
    if paired > 0 {
        tracing::info!(paired, "wants were paired with the tracks filed for them");
    }
    Ok(())
}

fn run(
    library: &Library,
    providers: &Providers,
    options: PollOptions,
    progress: &PollProgress,
) -> Result<PollSummary> {
    scanned_and_paired(library, &[])?;
    let now = SystemTime::now();
    let wants = library.wants()?;
    let forgotten = library.forgotten_deliveries()?;
    let mut away = Away::default();
    let mut filed = Vec::new();

    for want in wants.iter().filter(|want| due(want, options, now)) {
        if progress.is_cancelled() {
            break;
        }
        progress.asked.fetch_add(1, Ordering::Relaxed);
        progress.asks_about(Some(want.id));
        let cancelled = || progress.is_cancelled();
        let turning_to = |provider: &SourceId| progress.turns_to(Some(provider));
        let forgotten_for_it = forgotten.get(&want.id).map_or(&[][..], Vec::as_slice);
        let declined = |delivered: &Delivered| {
            forgotten_for_it
                .iter()
                .any(|forgotten| forgotten.declines(delivered))
        };
        let answer = providers.first(
            &want.identity(),
            &Asking {
                within: options.answers_within,
                cancelled: &cancelled,
                turning_to: &turning_to,
                declined: &declined,
            },
            &mut away,
        );
        progress
            .refused
            .fetch_add(answer.refused, Ordering::Relaxed);
        progress.late.fetch_add(answer.late, Ordering::Relaxed);
        if answer.delivered.is_none() {
            progress.turns_to(None);
        }
        if answer.cancelled {
            break;
        }
        match answer.delivered {
            Some(delivered) => {
                progress.offered.fetch_add(1, Ordering::Relaxed);
                let noted = landed(
                    library,
                    want,
                    delivered,
                    &mut Landing {
                        options,
                        progress,
                        away: &mut away,
                        filed: &mut filed,
                    },
                )?;
                let cancelled = progress.is_cancelled();
                if noted.is_some() || !cancelled {
                    tried(library, want.id, noted.as_ref())?;
                }
                if cancelled {
                    break;
                }
            }
            None if answer.heard_from_every_provider() => {
                progress.nothing.fetch_add(1, Ordering::Relaxed);
                tried(library, want.id, None)?;
            }
            None => tracing::debug!(
                want = %want.id,
                refused = answer.refused,
                late = answer.late,
                passed_over = answer.passed_over,
                narrowed = answer.narrowed,
                "not every provider answered, so the want stays due"
            ),
        }
    }
    progress.asks_about(None);

    scanned_and_paired(library, &filed)?;

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
        Ok(self.wants()?.iter().any(|want| due(want, options, now)))
    }

    pub fn next_want_due(&self) -> Result<Option<SystemTime>> {
        Ok(self.wants()?.iter().filter_map(Want::due_at).min())
    }

    pub(crate) fn forgotten_deliveries(&self) -> Result<AHashMap<WantId, Vec<ForgottenDelivery>>> {
        let mut forgotten: AHashMap<WantId, Vec<ForgottenDelivery>> = AHashMap::new();
        for (want, delivery) in self.forgotten_delivery_rows()? {
            forgotten.entry(want).or_default().push(delivery);
        }
        Ok(forgotten)
    }

    pub fn last_tried(&self) -> Result<Option<SystemTime>> {
        Ok(self
            .wants()?
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
