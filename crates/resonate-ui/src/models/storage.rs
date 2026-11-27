use std::{
    mem,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashMap;
use gpui::{Context, Task};
use resonate_core::TrackId;
use resonate_library::{
    BlockKind, Ceiling, KeptOriginal, Library, RecompressProgress, RecompressStats,
    RecompressSummary, Recompressed, SizedAfter, SizedFile, SoundShape, Track, TrailingTag,
};

use super::{LibraryModel, Notice, SCAN_POLL, Work};
use crate::{format, toast, words};

pub(crate) const FILES_A_PAGE: usize = 40;

#[derive(Clone, Debug, PartialEq)]
pub struct StoredFile {
    pub file: SizedFile,
    pub songs: Vec<Track>,
}

impl StoredFile {
    pub fn first(&self) -> Option<TrackId> {
        self.file.tracks.first().copied()
    }

    pub fn is_a_sheets(&self) -> bool {
        self.file.tracks.len() > 1 || self.songs.iter().any(|song| song.span.is_some())
    }

    pub fn recompresses(&self) -> bool {
        self.file.codec == resonate_library::Codec::Flac
            && !self.file.vaulted
            && !self.is_a_sheets()
    }
}

#[derive(Clone)]
pub(super) enum Recompressing {
    One {
        track: TrackId,
        halt: Arc<AtomicBool>,
    },
    Every(Arc<RecompressProgress>),
}

pub(super) struct Storage {
    files: Arc<[StoredFile]>,
    more: bool,
    asked: bool,
    reading: bool,
    _reading: Task<()>,
}

impl Default for Storage {
    fn default() -> Self {
        Self {
            files: Arc::default(),
            more: false,
            asked: false,
            reading: false,
            _reading: Task::ready(()),
        }
    }
}

struct Page {
    files: Vec<StoredFile>,
    more: bool,
}

fn page_of(
    library: &Library,
    after: Option<&SizedAfter>,
    limit: usize,
) -> resonate_library::Result<Page> {
    let mut files = library.largest_files(after, limit.saturating_add(1))?;

    let more = files.len() > limit;

    files.truncate(limit);

    let ids: Vec<TrackId> = files
        .iter()
        .flat_map(|file| file.tracks.iter().copied())
        .collect();
    let mut by_id: AHashMap<TrackId, Track> = library
        .tracks_with_ids(&ids)?
        .into_iter()
        .map(|track| (track.id, track))
        .collect();

    let files = files
        .into_iter()
        .map(|file| StoredFile {
            songs: file
                .tracks
                .iter()
                .filter_map(|id| by_id.remove(id))
                .collect(),
            file,
        })
        .collect();

    Ok(Page { files, more })
}

impl LibraryModel {
    pub fn largest_files(&self) -> Arc<[StoredFile]> {
        Arc::clone(&self.storage.files)
    }

    pub const fn holds_more_files(&self) -> bool {
        self.storage.more
    }

    pub const fn is_reading_the_files(&self) -> bool {
        self.storage.reading
    }

    pub const fn has_read_the_files(&self) -> bool {
        self.storage.asked && !self.storage.reading
    }

    pub fn read_the_largest_files(&mut self, cx: &mut Context<Self>) {
        let shown = self.storage.files.len().max(FILES_A_PAGE);

        self.read_the_files(None, shown, cx);
    }

    pub fn ask_for_the_largest_files(&mut self, cx: &mut Context<Self>) {
        if self.storage.asked {
            return;
        }
        self.read_the_largest_files(cx);
    }

    pub fn show_more_files(&mut self, cx: &mut Context<Self>) {
        let Some(last) = self.storage.files.last().map(|stored| stored.file.key()) else {
            return;
        };

        self.read_the_files(Some(last), FILES_A_PAGE, cx);
    }

    pub(super) fn read_the_files_again(&mut self, cx: &mut Context<Self>) {
        if self.storage.asked {
            self.read_the_largest_files(cx);
        }
    }

    fn read_the_files(&mut self, after: Option<SizedAfter>, limit: usize, cx: &mut Context<Self>) {
        let library = Arc::clone(&self.library);
        let appended = after.is_some();

        self.storage.asked = true;
        self.storage.reading = true;
        cx.notify();

        self.storage._reading = cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { page_of(&library, after.as_ref(), limit) })
                .await;

            let landed = this.update(cx, |this, cx| {
                this.storage.reading = false;
                match read {
                    Ok(page) => {
                        this.storage.files = if appended {
                            this.storage
                                .files
                                .iter()
                                .cloned()
                                .chain(page.files)
                                .collect()
                        } else {
                            page.files.into()
                        };
                        this.storage.more = page.more;
                    }
                    Err(error) => {
                        tracing::error!(%error, "the largest files could not be read");
                        toast::tell(
                            toast::could_not(words::models::list_the_largest_files(), &error),
                            cx,
                        );
                    }
                }
                cx.notify();
            });
            let _ = landed;
        });
    }

    pub fn recompressing_track(&self) -> Option<TrackId> {
        match self.running.work.recompressing() {
            Some(Recompressing::One { track, .. }) => Some(*track),
            Some(Recompressing::Every(_)) | None => None,
        }
    }

    pub const fn is_recompressing_every_flac(&self) -> bool {
        matches!(
            self.running.work.recompressing(),
            Some(Recompressing::Every(_))
        )
    }

    pub fn is_stopping_recompress(&self) -> bool {
        match self.running.work.recompressing() {
            Some(Recompressing::Every(progress)) => progress.is_cancelled(),
            Some(Recompressing::One { halt, .. }) => halt.load(Ordering::Relaxed),
            None => false,
        }
    }

    pub fn recompress_stats(&self) -> Option<RecompressStats> {
        match self.running.work.recompressing() {
            Some(Recompressing::Every(progress)) => Some(progress.snapshot()),
            Some(Recompressing::One { .. }) | None => None,
        }
    }

    pub fn stop_recompress(&mut self, cx: &mut Context<Self>) {
        match self.running.work.recompressing() {
            Some(Recompressing::Every(progress)) => progress.cancel(),
            Some(Recompressing::One { halt, .. }) => halt.store(true, Ordering::Relaxed),
            None => return,
        }
        cx.notify();
    }

    fn refused_while_busy(&self, cx: &mut Context<Self>) -> bool {
        if !self.running.work.is_busy() {
            return false;
        }
        toast::tell(
            Notice::Trouble(words::models::already_walking().to_owned()),
            cx,
        );
        cx.notify();
        true
    }

    pub fn recompress_the_file(
        &mut self,
        track: TrackId,
        title: String,
        ceiling: Ceiling,
        cx: &mut Context<Self>,
    ) {
        if self.refused_while_busy(cx) {
            return;
        }

        let library = Arc::clone(&self.library);
        let halt = Arc::new(AtomicBool::new(false));
        let halted = Arc::clone(&halt);
        let before = mem::replace(&mut self._edit, Task::ready(()));

        self.running.work = Work::Recompressing(Recompressing::One { track, halt });
        cx.notify();

        self.running._recompress = cx.spawn(async move |this, cx| {
            before.await;

            let outcome = cx
                .background_executor()
                .spawn(async move { library.recompress(track, ceiling, &halted) })
                .await;

            let finished = this.update(cx, |this, cx| {
                this.running.work = Work::Nothing;
                this.take_up_what_waited(cx);
                toast::tell(recompressed_told(outcome, &title), cx);
                this.reload(cx);
            });
            let _ = finished;
        });
    }

    pub fn recompress_every_flac(&mut self, ceiling: Ceiling, cx: &mut Context<Self>) {
        if self.refused_while_busy(cx) {
            return;
        }

        let handle = match self.library.recompress_all(ceiling) {
            Ok(handle) => handle,
            Err(error) => {
                tracing::error!(%error, "the library's FLAC files could not be re-compressed");
                toast::tell(
                    toast::could_not(words::models::recompress_every_flac(), &error),
                    cx,
                );
                cx.notify();
                return;
            }
        };

        self.running.work =
            Work::Recompressing(Recompressing::Every(Arc::clone(handle.progress())));
        cx.notify();

        self.running._recompress = cx.spawn(async move |this, cx| {
            while !handle.is_finished() {
                cx.background_executor().timer(SCAN_POLL).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
            }

            let finished = this.update(cx, |this, cx| {
                this.running.work = Work::Nothing;
                this.take_up_what_waited(cx);
                match handle.join() {
                    Ok(summary) => toast::tell(every_flac_told(&summary), cx),
                    Err(error) => {
                        tracing::error!(%error, "the library's FLAC files were not re-compressed");
                        toast::tell(
                            toast::could_not(words::models::recompress_every_flac(), &error),
                            cx,
                        );
                    }
                }
                this.reload(cx);
            });
            let _ = finished;
        });
    }
}

fn recompressed_told(outcome: resonate_library::Result<Recompressed>, title: &str) -> Notice {
    match outcome {
        Ok(Recompressed::Smaller(shrunk)) => Notice::Done(
            words::models::RecompressedSmaller {
                after: format::bytes(shrunk.after),
                before: format::bytes(shrunk.before),
                title,
            }
            .said(),
        ),
        Ok(Recompressed::Kept(kept)) => {
            let said = words::models::KeptAsItWas {
                title,
                why: kept_because(kept),
            }
            .said();

            match kept {
                KeptOriginal::NoSmaller { .. } | KeptOriginal::Halted => Notice::Noted(said),
                _ => Notice::Trouble(said),
            }
        }
        Err(error) => {
            tracing::error!(%error, "a song could not be re-compressed");
            toast::could_not(words::models::recompress_that_song(), &error)
        }
    }
}

fn every_flac_told(summary: &RecompressSummary) -> Notice {
    let stats = summary.stats;
    let said = words::models::RecompressedEveryFlac {
        failed: stats.failed,
        kept: stats.kept,
        saved: format::bytes(stats.saved()),
        smaller: stats.smaller,
    }
    .said();

    if summary.cancelled {
        return Notice::Noted(words::models::stopped_recompressing(said));
    }
    if stats.failed > 0 {
        return Notice::Trouble(said);
    }
    Notice::Done(said)
}

pub(crate) fn kept_because(kept: KeptOriginal) -> String {
    let why = match kept {
        KeptOriginal::NoSmaller { .. } => words::models::kept_no_smaller(),
        KeptOriginal::Unverified => words::models::kept_unverified(),
        KeptOriginal::ChangedMeanwhile => words::models::kept_changed_meanwhile(),
        KeptOriginal::Halted => words::models::kept_halted(),
        KeptOriginal::NotAFlac => words::models::kept_not_a_flac(),
        KeptOriginal::Vaulted => words::models::kept_vaulted(),
        KeptOriginal::CutBySheet => words::models::kept_cut_by_sheet(),
        KeptOriginal::OutsideTheLibrary => words::models::kept_outside_the_library(),
        KeptOriginal::HeadUnread(_) => words::models::kept_head_unread(),
        KeptOriginal::TagAfterTheAudio(TrailingTag::Id3v1) => {
            words::models::kept_id3_after_the_audio()
        }
        KeptOriginal::TagAfterTheAudio(TrailingTag::Ape) => {
            words::models::kept_ape_after_the_audio()
        }
        KeptOriginal::BeyondTheEncoder(shape) => return beyond_the_encoder(shape),
        KeptOriginal::TiedToTheSamples(block) => {
            return words::models::kept_tied_to_the_samples(block_named(block));
        }
        KeptOriginal::DisagreesWithItsDigest => words::models::kept_disagrees_with_its_digest(),
        KeptOriginal::WiderThanItsDepth => words::models::kept_wider_than_its_depth(),
        KeptOriginal::TurnsPartway => words::models::kept_turns_partway(),
        KeptOriginal::Undecodable => words::models::kept_undecodable(),
        KeptOriginal::Silent => words::models::kept_silent(),
        KeptOriginal::HeldUnderOtherNames => words::models::kept_held_under_other_names(),
        KeptOriginal::AttributesNotCarried => words::models::kept_attributes_not_carried(),
    };

    why.to_owned()
}

fn beyond_the_encoder(shape: SoundShape) -> String {
    words::models::KeptBeyondTheEncoder {
        bits: shape.bits.to_string(),
        rate: shape.rate.kilohertz(),
    }
    .said()
}

const fn block_named(block: BlockKind) -> &'static str {
    match block {
        BlockKind::Padding => "PADDING",
        BlockKind::Application => "APPLICATION",
        BlockKind::SeekTable => "SEEKTABLE",
        BlockKind::VorbisComment => "VORBIS_COMMENT",
        BlockKind::CueSheet => "CUESHEET",
        BlockKind::Picture => "PICTURE",
        BlockKind::Reserved(_) => "RESERVED",
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui::TestAppContext;
    use resonate_library::{Ceiling, Library, ScanProgress};

    use super::{super::Work, *};
    use crate::driven::{Driven, Folder};

    #[gpui::test]
    fn every_flac_is_not_re_compressed_while_a_scan_runs_and_the_scan_is_left_to_run(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));
        let scanning = Arc::new(ScanProgress::default());

        let mut driven = Driven::opened_in(cx, library, &folder);

        driven.root.update(&mut driven.cx, |root, cx| {
            root.library.update(cx, |library, cx| {
                library.running.work = Work::Scanning(Arc::clone(&scanning));
                library.recompress_every_flac(Ceiling::Cd, cx);
            });
        });
        driven.until_said(|said| said == words::models::already_walking());

        let left = driven.read(|root, cx| {
            let library = root.library.read(cx);
            (
                library.is_scanning(),
                library.is_recompressing_every_flac(),
                library.recompress_stats(),
            )
        });

        assert_eq!(left, (true, false, None));
    }
}
