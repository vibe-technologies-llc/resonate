use std::{
    ffi::OsStr,
    fs::File,
    io::{self, Read},
    num::{NonZeroU64, NonZeroUsize},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use resonate_codec::Sources;
use resonate_core::{MediaLocation, WantId};
use resonate_providers::{Asking, Away, Delivered, Delivery, Identity, Providers};
use resonate_vault::{Keeping, Taking};

use crate::{
    Error, Library, Result, ScanOptions, Want,
    filed::{self, Filed, Unfiled},
    pass::{Cancelling, PassHandle, PassKind, PollHandle},
};

pub const POLL_AGAIN_AFTER: Duration = Duration::from_secs(6 * 60 * 60);
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
}

#[derive(Clone, Copy, Debug)]
pub struct PollOptions {
    pub again_after: Duration,
    pub answers_within: Duration,
}

impl PollOptions {
    pub const ASKING_EVERY_WANT: Self = Self {
        again_after: Duration::ZERO,
        answers_within: ANSWERS_WITHIN,
    };
}

impl Default for PollOptions {
    fn default() -> Self {
        Self {
            again_after: POLL_AGAIN_AFTER,
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

    fn asks_about(&self, want: Option<WantId>) {
        self.asking
            .store(want.map_or(ASKING_NOTHING, WantId::get), Ordering::Relaxed);
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
        Delivery::File(path) => vault.keep(&Taking {
            sources: &Sources::local(),
            location: &MediaLocation::local(path),
            span: None,
            renewing: false,
            foretold: None,
        }),
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
        Ok(Keeping::Kept(kept)) => {
            library.note_delivered(want, &kept, &taken_from)?;
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
            self.read = 0;
            self.ended = self.held.is_empty();
        }
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

    let thread = thread::Builder::new()
        .name("resonate-poll".to_owned())
        .spawn(move || run(&library, &providers, options, &progress))
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
                Ok(mut file) => filed::filed(library, &into, want, &mut file, &extension),
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
    let mut away = Away::default();
    let mut filed = Vec::new();

    for want in wants
        .iter()
        .filter(|want| due(want, options.again_after, now))
    {
        if progress.is_cancelled() {
            break;
        }
        progress.asked.fetch_add(1, Ordering::Relaxed);
        progress.asks_about(Some(want.id));
        let cancelled = || progress.is_cancelled();
        let answer = providers.first(
            &want.identity(),
            &Asking {
                within: options.answers_within,
                cancelled: &cancelled,
            },
            &mut away,
        );
        progress
            .refused
            .fetch_add(answer.refused, Ordering::Relaxed);
        progress.late.fetch_add(answer.late, Ordering::Relaxed);
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
        Ok(self
            .wants()?
            .iter()
            .any(|want| due(want, options.again_after, now)))
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

fn due(want: &Want, again_after: Duration, now: SystemTime) -> bool {
    if want.held.is_some() {
        return false;
    }
    match want.tried.map(|tried| now.duration_since(tried)) {
        None | Some(Err(_)) => true,
        Some(Ok(ago)) => ago >= again_after,
    }
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
            held: None,
            links: Vec::new(),
            release_links: Vec::new(),
        }
    }

    #[test]
    fn a_want_is_due_where_it_was_never_tried_or_was_tried_at_least_the_window_ago() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let window = Duration::from_secs(10);
        assert!(due(&want(None), window, now));
        assert!(due(
            &want(Some(SystemTime::UNIX_EPOCH + Duration::from_secs(90))),
            window,
            now
        ));
        assert!(due(
            &want(Some(SystemTime::UNIX_EPOCH + Duration::from_secs(50))),
            window,
            now
        ));
        assert!(!due(
            &want(Some(SystemTime::UNIX_EPOCH + Duration::from_secs(95))),
            window,
            now
        ));
        assert!(due(
            &want(Some(SystemTime::UNIX_EPOCH + Duration::from_secs(200))),
            window,
            now
        ));
    }

    #[test]
    fn a_want_the_catalog_already_holds_a_row_for_is_never_due() {
        let held = Want {
            held: Some(TrackId::new(7).expect("a non-zero id")),
            ..want(None)
        };
        assert!(!due(&held, Duration::ZERO, SystemTime::UNIX_EPOCH));
    }
}
