use std::{
    cmp::Reverse,
    num::NonZeroUsize,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
};

use ahash::AHashSet;
use parking_lot::Mutex;
use resonate_codec::{Codec, Sources};
use resonate_core::{AlbumId, TrackId};
use resonate_vault::{Form, Keeping, Refusal, Taking, Vault};

use crate::{
    Error, Library, Result,
    db::TrackToVault,
    pass::{Cancelling, ImportHandle, PassHandle, PassKind},
};

#[derive(Clone, Debug)]
pub struct ImportOptions {
    pub roots: Vec<PathBuf>,
    pub apply: bool,
    pub at_most: Option<NonZeroUsize>,
    pub workers: NonZeroUsize,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            apply: false,
            at_most: None,
            workers: thread::available_parallelism().unwrap_or(NonZeroUsize::MIN),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wanted {
    pub track: TrackId,
    pub from: PathBuf,
    pub form: Form,
    pub bytes: u64,
    pub codec: Codec,
    pub renewing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vaulted {
    pub wanted: Wanted,
    pub to: PathBuf,
    pub bytes: u64,
    pub deduped: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Passing {
    SourceGone,
    Unreadable,
    Refused(Refusal),
}

impl Passing {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SourceGone => "is not there any more",
            Self::Unreadable => "could not be read",
            Self::Refused(refusal) => refusal.as_str(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Passed {
    pub from: PathBuf,
    pub why: Passing,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportPlan {
    pub wanted: Vec<Wanted>,
    pub vaulted: Vec<Vaulted>,
    pub passed: Vec<Passed>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImportStats {
    pub walked: u64,
    pub vaulted: u64,
    pub deduped: u64,
    pub covers: u64,
    pub covers_passed: u64,
    pub passed: u64,
    pub was_bytes: u64,
    pub bytes: u64,
}

impl ImportStats {
    pub const fn saved(&self) -> u64 {
        self.was_bytes.saturating_sub(self.bytes)
    }
}

#[derive(Debug, Default)]
pub struct ImportProgress {
    walked: AtomicU64,
    vaulted: AtomicU64,
    deduped: AtomicU64,
    covers: AtomicU64,
    covers_passed: AtomicU64,
    passed: AtomicU64,
    was_bytes: AtomicU64,
    bytes: AtomicU64,
    cancelled: AtomicBool,
}

impl ImportProgress {
    pub fn snapshot(&self) -> ImportStats {
        ImportStats {
            walked: self.walked.load(Ordering::Relaxed),
            vaulted: self.vaulted.load(Ordering::Relaxed),
            deduped: self.deduped.load(Ordering::Relaxed),
            covers: self.covers.load(Ordering::Relaxed),
            covers_passed: self.covers_passed.load(Ordering::Relaxed),
            passed: self.passed.load(Ordering::Relaxed),
            was_bytes: self.was_bytes.load(Ordering::Relaxed),
            bytes: self.bytes.load(Ordering::Relaxed),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

impl Cancelling for ImportProgress {
    fn cancel(&self) {
        ImportProgress::cancel(self);
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportSummary {
    pub stats: ImportStats,
    pub plan: ImportPlan,
    pub cancelled: bool,
}

pub(crate) fn start(
    library: Library,
    vault: Arc<Vault>,
    sources: Arc<Sources>,
    options: ImportOptions,
) -> Result<ImportHandle> {
    let walking = library.walk_the_tree()?;
    let progress = Arc::new(ImportProgress::default());
    let owned = Arc::clone(&progress);

    let thread = thread::Builder::new()
        .name("resonate-import".to_owned())
        .spawn(move || {
            let outcome = run(&library, &vault, &sources, &options, &progress);
            drop(walking);
            outcome
        })
        .map_err(|source| Error::ThreadSpawn { source })?;

    Ok(PassHandle::of(PassKind::Import, owned, thread))
}

fn run(
    library: &Library,
    vault: &Vault,
    sources: &Sources,
    options: &ImportOptions,
    progress: &ImportProgress,
) -> Result<ImportSummary> {
    let known = library.roots()?;
    for root in &options.roots {
        if !known.contains(root) {
            return Err(Error::NotARoot { path: root.clone() });
        }
    }

    let mut rows = library.tracks_to_vault(&options.roots)?;
    rows.retain(worth_weighing);
    if let Some(cap) = options.at_most {
        rows.truncate(cap.get());
    }
    progress.walked.store(rows.len() as u64, Ordering::Relaxed);

    let mut plan = ImportPlan::default();
    if !options.apply {
        plan.wanted = rows.iter().map(wanted).collect();
        return Ok(ImportSummary {
            stats: progress.snapshot(),
            plan,
            cancelled: false,
        });
    }

    let shared = Shared {
        library,
        vault,
        sources,
        progress,
        rows: &rows,
        claimed_in: costliest_first(&rows),
        next: AtomicUsize::new(0),
        failed: AtomicBool::new(false),
        covered: Mutex::new(AHashSet::new()),
    };
    let workers = options
        .workers
        .min(NonZeroUsize::new(rows.len()).unwrap_or(NonZeroUsize::MIN));
    let mut settled = thread::scope(|scope| shared.worked_through(scope, workers))?;
    settled.sort_by_key(|(at, _)| *at);
    for (_, outcome) in settled {
        match outcome {
            Outcome::Vaulted(vaulted) => plan.vaulted.push(vaulted),
            Outcome::Passed(passed) => plan.passed.push(passed),
        }
    }

    Ok(ImportSummary {
        stats: progress.snapshot(),
        plan,
        cancelled: progress.is_cancelled(),
    })
}

enum Outcome {
    Vaulted(Vaulted),
    Passed(Passed),
}

struct Shared<'a> {
    library: &'a Library,
    vault: &'a Vault,
    sources: &'a Sources,
    progress: &'a ImportProgress,
    rows: &'a [TrackToVault],
    claimed_in: Vec<usize>,
    next: AtomicUsize,
    failed: AtomicBool,
    covered: Mutex<AHashSet<AlbumId>>,
}

impl<'a> Shared<'a> {
    fn worked_through<'scope>(
        &'scope self,
        scope: &'scope thread::Scope<'scope, '_>,
        workers: NonZeroUsize,
    ) -> Result<Vec<(usize, Outcome)>> {
        let mut spawned = Vec::with_capacity(workers.get());
        for index in 0..workers.get() {
            match thread::Builder::new()
                .name(format!("resonate-import-{index}"))
                .spawn_scoped(scope, || self.work())
            {
                Ok(worker) => spawned.push(worker),
                Err(source) => {
                    self.failed.store(true, Ordering::Relaxed);
                    return Err(Error::ThreadSpawn { source });
                }
            }
        }

        let mut settled = Vec::with_capacity(self.rows.len());
        let mut first_error = None;
        for worker in spawned {
            match worker.join() {
                Ok(Ok(done)) => settled.extend(done),
                Ok(Err(error)) => {
                    first_error.get_or_insert(error);
                }
                Err(_) => {
                    first_error.get_or_insert(Error::Stopped {
                        pass: PassKind::Import,
                    });
                }
            }
        }
        first_error.map_or(Ok(settled), Err)
    }

    fn work(&self) -> Result<Vec<(usize, Outcome)>> {
        let mut done = Vec::new();
        while !self.progress.is_cancelled() && !self.failed.load(Ordering::Relaxed) {
            let claimed = self.next.fetch_add(1, Ordering::Relaxed);
            let Some(&at) = self.claimed_in.get(claimed) else {
                break;
            };
            let row = &self.rows[at];
            let kept = keep_one(self.library, self.vault, self.sources, row, self.progress)
                .and_then(|outcome| self.cover_of(row).map(|()| outcome));
            match kept {
                Ok(outcome) => done.push((at, outcome)),
                Err(error) => {
                    self.failed.store(true, Ordering::Relaxed);
                    return Err(error);
                }
            }
        }
        Ok(done)
    }

    fn cover_of(&self, row: &TrackToVault) -> Result<()> {
        let Some(album) = row.album_id else {
            return Ok(());
        };
        if !self.covered.lock().insert(album) {
            return Ok(());
        }
        cover_of(self.library, self.vault, album, self.progress)
    }
}

fn keep_one(
    library: &Library,
    vault: &Vault,
    sources: &Sources,
    row: &TrackToVault,
    progress: &ImportProgress,
) -> Result<Outcome> {
    let asked = wanted(row);
    if !row.path.is_file() {
        return Ok(passed(progress, &asked, Passing::SourceGone));
    }

    let location = row.location();
    let taking = Taking {
        sources,
        location: &location,
        span: row.span,
        renewing: row.renewing,
        foretold: row.foretold,
    };

    match vault.keep(&taking) {
        Err(source) if vault.failed_itself(&source) => Err(Error::Vault {
            path: row.path.clone(),
            source: Box::new(source),
        }),
        Err(source) => {
            tracing::warn!(path = %row.path.display(), %source, "the vault could not keep a track");
            Ok(passed(progress, &asked, Passing::Unreadable))
        }
        Ok(Keeping::Refused(refusal)) => {
            library.note_vault_refused(row)?;
            Ok(passed(progress, &asked, Passing::Refused(refusal)))
        }
        Ok(Keeping::Kept(kept)) => {
            let asked = Wanted {
                form: kept.form,
                bytes: kept.was,
                ..asked
            };
            library.note_vaulted(row, &kept)?;
            progress.vaulted.fetch_add(1, Ordering::Relaxed);
            progress.was_bytes.fetch_add(asked.bytes, Ordering::Relaxed);
            if kept.deduped {
                progress.deduped.fetch_add(1, Ordering::Relaxed);
            } else {
                progress.bytes.fetch_add(kept.bytes, Ordering::Relaxed);
            }
            Ok(Outcome::Vaulted(Vaulted {
                wanted: asked,
                to: kept.path,
                bytes: kept.bytes,
                deduped: kept.deduped,
            }))
        }
    }
}

fn cover_of(
    library: &Library,
    vault: &Vault,
    album: AlbumId,
    progress: &ImportProgress,
) -> Result<()> {
    let art = match library.cover_the_vault_lacks(album) {
        Ok(Some(art)) => art,
        Ok(None) => return Ok(()),
        Err(error @ (Error::UntypedCoverArt { .. } | Error::UnknownImageFormat { .. })) => {
            tracing::warn!(album = album.get(), %error, "an album's cover names no format the vault can keep");
            progress.covers_passed.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }
        Err(error) => return Err(error),
    };

    let kept = match vault.keep_cover(&art) {
        Ok(kept) => kept,
        Err(source) => {
            tracing::warn!(album = album.get(), %source, "the vault could not keep an album's cover");
            progress.covers_passed.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }
    };
    if library.note_vaulted_cover(album, &kept, &art)? {
        progress.covers.fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}

fn passed(progress: &ImportProgress, asked: &Wanted, why: Passing) -> Outcome {
    progress.passed.fetch_add(1, Ordering::Relaxed);
    Outcome::Passed(Passed {
        from: asked.from.clone(),
        why,
    })
}

fn costliest_first(rows: &[TrackToVault]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by_key(|&at| Reverse(cost_of(&rows[at])));
    order
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Effort {
    Copied,
    Encoded,
    Packed,
}

fn cost_of(row: &TrackToVault) -> (Effort, u64) {
    let effort = match form_of(row) {
        Form::Kept => Effort::Copied,
        Form::Flac => Effort::Encoded,
        Form::Wave => Effort::Packed,
    };
    (effort, row.file_size)
}

fn worth_weighing(row: &TrackToVault) -> bool {
    !row.renewing || (row.path.is_file() && (form_of(row) != Form::Kept || sheds_tags_kept(row)))
}

fn sheds_tags_kept(row: &TrackToVault) -> bool {
    matches!(
        row.codec,
        Codec::Mp3 | Codec::Aac | Codec::Dsd | Codec::Vorbis | Codec::Opus
    )
}

fn form_of(row: &TrackToVault) -> Form {
    Form::of(row.codec, row.spec, row.spec.format.valid_bits())
}

fn wanted(row: &TrackToVault) -> Wanted {
    Wanted {
        track: row.id,
        from: row.path.clone(),
        form: form_of(row),
        bytes: row.file_size,
        codec: row.codec,
        renewing: row.renewing,
    }
}

#[cfg(test)]
mod tests {
    use std::{env, fs, io::Cursor, path::Path, process};

    use resonate_codec::{CoverArt, ImageFormat};
    use resonate_vault::VaultKey;
    use rusqlite::params;

    use super::*;
    use crate::{ScanOptions, store};

    const RATE: u32 = 44_100;
    const FRAMES: u32 = 44_100;
    const SHEET: &str = "FILE \"noise.wav\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"One\"\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    TITLE \"Two\"\n    INDEX 01 00:00:20\n";

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(named: &str) -> Self {
            let path = env::temp_dir().join(format!("resonate-import-{}-{named}", process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(path.join("music")).expect("a scratch folder");
            Self(path)
        }

        fn music(&self) -> PathBuf {
            self.0.join("music")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn noise(seed: u32) -> Vec<u8> {
        let mut state = seed;
        let mut data = Vec::with_capacity(FRAMES as usize * 4);
        for _ in 0..FRAMES * 2 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            data.extend_from_slice(&((state >> 16) as u16).to_le_bytes());
        }
        let mut wave = b"RIFF".to_vec();
        wave.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        wave.extend_from_slice(b"WAVEfmt ");
        wave.extend_from_slice(&16_u32.to_le_bytes());
        wave.extend_from_slice(&1_u16.to_le_bytes());
        wave.extend_from_slice(&2_u16.to_le_bytes());
        wave.extend_from_slice(&RATE.to_le_bytes());
        wave.extend_from_slice(&(RATE * 4).to_le_bytes());
        wave.extend_from_slice(&4_u16.to_le_bytes());
        wave.extend_from_slice(&16_u16.to_le_bytes());
        wave.extend_from_slice(b"data");
        wave.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wave.extend_from_slice(&data);
        wave
    }

    fn opened(scratch: &Scratch) -> Library {
        let vault = Arc::new(Vault::make(scratch.0.join("vault")).expect("a vault"));
        Library::open_in_memory_with_vault(vault).expect("a library")
    }

    fn scanned(library: &Library, root: &Path) {
        library
            .scan(ScanOptions {
                roots: vec![root.to_path_buf()],
                incremental: true,
                follow_symlinks: false,
                extract_cover_art: true,
                workers: NonZeroUsize::MIN,
            })
            .expect("a scan")
            .join()
            .expect("a finished scan");
    }

    fn imported(library: &Library) -> ImportSummary {
        library
            .import(
                Arc::new(Sources::local()),
                ImportOptions {
                    apply: true,
                    workers: NonZeroUsize::MIN,
                    ..ImportOptions::default()
                },
            )
            .expect("an import")
            .join()
            .expect("a finished import")
    }

    fn picture(shade: u8) -> CoverArt {
        let pixels: Vec<u8> = (0..16 * 16)
            .flat_map(|at: u32| [shade, at as u8, 40, 255])
            .collect();
        let drawn = image::RgbaImage::from_raw(16, 16, pixels).expect("a drawn picture");
        let mut written = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(drawn)
            .write_to(&mut written, image::ImageFormat::Png)
            .expect("a written picture");
        CoverArt {
            format: ImageFormat::Png,
            bytes: written.into_inner(),
        }
    }

    fn hummed(step: u32) -> Vec<u8> {
        let mut wave = noise(1);
        let data = wave.len() - FRAMES as usize * 4;
        for (at, frame) in wave[data..].as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let level = ((at as u32 * step) % 512) as u16;
            frame[..2].copy_from_slice(&level.to_le_bytes());
            frame[2..].copy_from_slice(&level.to_le_bytes());
        }
        wave
    }

    fn foretold_for(library: &Library, path: &Path) -> Option<VaultKey> {
        library
            .tracks_to_vault(&[])
            .expect("the rows to vault")
            .into_iter()
            .find(|row| row.path == path)
            .expect("a row still to vault")
            .foretold
    }

    fn keys_held(library: &Library) -> Vec<VaultKey> {
        library
            .vault_objects()
            .expect("the objects noted")
            .into_iter()
            .map(|object| object.key)
            .collect()
    }

    #[test]
    fn a_row_whose_sound_one_object_alone_has_is_foretold_that_objects_key() {
        let scratch = Scratch::new("foretold");
        let library = opened(&scratch);
        let music = scratch.music().canonicalize().expect("a scratch folder");
        fs::create_dir_all(music.join("a")).expect("a folder");
        fs::write(music.join("a/hum.wav"), hummed(3)).expect("a written source");
        scanned(&library, &music);
        let first = imported(&library);
        let landed = keys_held(&library);

        for folder in ["b", "c"] {
            fs::create_dir_all(music.join(folder)).expect("a folder");
        }
        fs::write(music.join("b/hum.wav"), hummed(3)).expect("a written source");
        fs::write(music.join("c/other.wav"), hummed(5)).expect("a written source");
        scanned(&library, &music);
        let copy = foretold_for(&library, &music.join("b/hum.wav"));
        let other = foretold_for(&library, &music.join("c/other.wav"));
        let second = imported(&library);

        fs::create_dir_all(music.join("d")).expect("a folder");
        fs::write(music.join("d/third.wav"), hummed(7)).expect("a written source");
        scanned(&library, &music);
        let shared = foretold_for(&library, &music.join("d/third.wav"));

        assert_eq!(first.stats.vaulted, 1);
        assert_eq!(landed.len(), 1);
        assert_eq!(copy, Some(landed[0]));
        assert_eq!(other, Some(landed[0]));
        assert_eq!(second.stats.deduped, 1);
        assert_eq!(second.stats.vaulted, 2);
        assert_eq!(keys_held(&library).len(), 2);
        assert_eq!(
            shared, None,
            "a sound two objects share foretold one of them"
        );
    }

    #[test]
    fn a_refused_row_is_not_weighed_again_until_its_file_or_the_encoder_moves() {
        let scratch = Scratch::new("refused");
        let library = opened(&scratch);
        let audio = scratch.music().join("noise.wav");
        fs::write(&audio, noise(0x9e37_79b9)).expect("a written source");
        fs::write(scratch.music().join("noise.cue"), SHEET).expect("a written sheet");
        scanned(&library, &scratch.music());

        let first = imported(&library);
        let again = imported(&library);
        library
            .inner()
            .write(|transaction| {
                transaction
                    .execute("UPDATE vault_refused SET under = under - 1", [])
                    .map(drop)
                    .map_err(|source| Error::store(crate::error::StoreOp::Update, source))
            })
            .expect("an older stamp");
        let behind = imported(&library);
        fs::write(&audio, noise(0x2545_f491)).expect("a rewritten source");
        scanned(&library, &scratch.music());
        let rewritten = imported(&library);

        assert_eq!(first.stats.walked, 2);
        assert!(
            first
                .plan
                .passed
                .iter()
                .all(|passed| passed.why == Passing::Refused(Refusal::NoSmaller)),
            "{:?}",
            first.plan.passed
        );
        assert_eq!(
            again.stats.walked, 0,
            "a refusal was weighed again at full cost"
        );
        assert_eq!(behind.stats.walked, 2);
        assert_eq!(rewritten.stats.walked, 2);
    }

    #[test]
    fn a_cover_is_moved_into_the_vault_only_where_the_album_still_holds_the_picture_encoded() {
        let scratch = Scratch::new("cover");
        let library = opened(&scratch);
        let vault = Arc::clone(library.vault().expect("a vault"));
        let first = picture(10);
        let better = picture(200);
        let albums: Vec<AlbumId> = ["Meddle", "Animals"]
            .into_iter()
            .map(|title| {
                library
                    .inner()
                    .write(|transaction| {
                        transaction
                            .query_row(
                                "INSERT INTO albums (title, cover_art, cover_format)
                                 VALUES (?1, ?2, ?3) RETURNING id",
                                params![
                                    title,
                                    first.bytes,
                                    store::image_format_code(ImageFormat::Png)
                                ],
                                |row| row.get::<_, i64>(0),
                            )
                            .map_err(|source| Error::store(crate::error::StoreOp::Insert, source))
                    })
                    .map(|id| AlbumId::new(id as u64).expect("an album id"))
                    .expect("an album")
            })
            .collect();
        let [raced, alone] = albums[..] else {
            panic!("two albums");
        };

        let asked = library
            .cover_the_vault_lacks(raced)
            .expect("a read")
            .expect("a cover");
        let kept = vault.keep_cover(&asked).expect("a kept cover");
        library
            .inner()
            .write(|transaction| {
                transaction
                    .execute(
                        "UPDATE albums SET cover_art = ?1 WHERE id = ?2",
                        params![better.bytes, raced.get() as i64],
                    )
                    .map(drop)
                    .map_err(|source| Error::store(crate::error::StoreOp::Update, source))
            })
            .expect("a better cover landed");
        let moved_over_the_better = library
            .note_vaulted_cover(raced, &kept, &asked)
            .expect("a noting");
        let moved_alone = library
            .note_vaulted_cover(alone, &kept, &asked)
            .expect("a noting");

        assert!(!moved_over_the_better);
        assert_eq!(
            library
                .cover_the_vault_lacks(raced)
                .expect("a read")
                .map(|held| held.bytes),
            Some(better.bytes)
        );
        assert!(moved_alone);
        assert_eq!(library.cover_the_vault_lacks(alone).expect("a read"), None);
    }
}
