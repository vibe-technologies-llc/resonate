use std::{
    io::{self, Read},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use resonate_codec::Sources;
use resonate_core::MediaLocation;
use resonate_providers::{Asking, Delivered, Delivery, Identity, Providers};
use resonate_vault::{Keeping, Taking};

use crate::{
    Error, Library, Result, Want,
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

fn landed(
    library: &Library,
    want: &Want,
    delivered: Delivered,
    options: PollOptions,
    progress: &PollProgress,
) -> Result<Option<MediaLocation>> {
    let taken_from = delivered.taken_from();
    let Some(vault) = library.vault().cloned() else {
        return Ok(match delivered.delivery {
            Delivery::File(_) => Some(taken_from),
            Delivery::Stream { .. } => {
                tracing::warn!(%taken_from, "a streamed delivery has no vault to land in");
                progress.unkept.fetch_add(1, Ordering::Relaxed);
                None
            }
        });
    };

    let keeping = match delivered.delivery {
        Delivery::File(path) => vault.keep(&Taking {
            sources: &Sources::local(),
            location: &MediaLocation::local(path),
            span: None,
            renewing: false,
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
                return Ok(None);
            }
            keeping
        }
    };

    if progress.is_cancelled() {
        return Ok(None);
    }
    match keeping {
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
        Ok(Keeping::Kept(kept)) => {
            library.note_delivered(want, &kept, &taken_from)?;
            progress.kept.fetch_add(1, Ordering::Relaxed);
            Ok(Some(MediaLocation::local(&kept.path)))
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

fn run(
    library: &Library,
    providers: &Providers,
    options: PollOptions,
    progress: &PollProgress,
) -> Result<PollSummary> {
    let now = SystemTime::now();
    let wants = library.wants()?;

    for want in wants
        .iter()
        .filter(|want| due(want, options.again_after, now))
    {
        if progress.is_cancelled() {
            break;
        }
        progress.asked.fetch_add(1, Ordering::Relaxed);
        let cancelled = || progress.is_cancelled();
        let answer = providers.first(
            &want.identity(),
            &Asking {
                within: options.answers_within,
                cancelled: &cancelled,
            },
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
                let noted = landed(library, want, delivered, options, progress)?;
                if progress.is_cancelled() {
                    break;
                }
                library.note_tried(want.id, noted.as_ref())?;
            }
            None => {
                progress.nothing.fetch_add(1, Ordering::Relaxed);
                library.note_tried(want.id, None)?;
            }
        }
    }

    Ok(PollSummary {
        stats: progress.snapshot(),
        cancelled: progress.is_cancelled(),
    })
}

impl Library {
    pub fn is_a_want_due(&self, options: PollOptions) -> Result<bool> {
        let now = SystemTime::now();
        Ok(self
            .wants()?
            .iter()
            .any(|want| due(want, options.again_after, now)))
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
