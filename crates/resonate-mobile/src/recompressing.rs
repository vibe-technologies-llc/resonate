use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
};

use parking_lot::Mutex;
use resonate_core::writer::{self, Held, Writer};
use resonate_library::{
    BlockKind, Ceiling, Halt, KeptOriginal, RecompressStage, Recompressed, TrailingTag,
    recompress_into,
};

use crate::{
    Error, MobilePlayer, Result, Track, documents::location, one_file::Spelled,
    stamp::DocumentStamp, words,
};

const STAGING: &str = "recompress-staging";
const STAGE_SUFFIX: &str = ".flac";
const STAGED_BY_AND_COUNTED: char = '-';

static STAGED: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Enum)]
pub enum RecompressCeiling {
    #[default]
    Kept,
    Studio,
    High,
    Cd,
}

impl From<RecompressCeiling> for Ceiling {
    fn from(asked: RecompressCeiling) -> Self {
        match asked {
            RecompressCeiling::Kept => Self::Kept,
            RecompressCeiling::Studio => Self::Studio,
            RecompressCeiling::High => Self::High,
            RecompressCeiling::Cd => Self::Cd,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RecompressKept {
    NoSmaller,
    Unverified,
    ChangedMeanwhile,
    Halted,
    NotAFlac,
    Vaulted,
    CutBySheet,
    OutsideTheLibrary,
    HeadUnread,
    Id3AfterTheAudio,
    ApeAfterTheAudio,
    BeyondTheEncoder,
    TiedToTheSamples,
    DisagreesWithItsDigest,
    WiderThanItsDepth,
    TurnsPartway,
    Undecodable,
    Silent,
    HeldUnderOtherNames,
    AttributesNotCarried,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RecompressStaged {
    Staged {
        stage_path: String,
        before: u64,
        after: u64,
    },
    KeptOriginal {
        reason: RecompressKept,
        said: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SizedSongAfter {
    pub bytes: u64,
    pub id: u64,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SizedSong {
    pub track: Track,
    pub bytes: u64,
}

#[derive(Default)]
pub(crate) struct Stages(Mutex<BTreeMap<PathBuf, Held>>);

#[uniffi::export]
impl MobilePlayer {
    pub fn largest_songs(
        &self,
        after: Option<SizedSongAfter>,
        limit: u32,
    ) -> Result<Vec<SizedSong>> {
        let mut sized: Vec<SizedSong> = self
            .catalog
            .held_with_their_sizes()?
            .into_iter()
            .filter_map(|(stamped, track)| {
                let bytes = match stamped {
                    0 => self.measured(&track.uri)?,
                    stamped => stamped,
                };

                Some(SizedSong { track, bytes })
            })
            .filter(|song| after.is_none_or(|after| comes_after(song, after)))
            .collect();

        sized.sort_by(|one, other| {
            other
                .bytes
                .cmp(&one.bytes)
                .then(one.track.id.cmp(&other.track.id))
        });
        sized.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        Ok(sized)
    }

    pub fn recompress_song(&self, id: u64, ceiling: RecompressCeiling) -> Result<RecompressStaged> {
        let track = self.catalog.track(id)?;
        let at = location(&track.uri)?;
        let folder = self.tagging.folder().join(STAGING);

        fs::create_dir_all(&folder).map_err(|source| Error::RecompressStaging { source })?;
        self.services.stages.sweep(&folder);

        let path = folder.join(format!(
            "{}{STAGED_BY_AND_COUNTED}{}{STAGE_SUFFIX}",
            process::id(),
            STAGED.fetch_add(1, Ordering::Relaxed)
        ));
        let held = Held::made(&path).map_err(|source| Error::RecompressStaging { source })?;
        let file = held
            .file()
            .try_clone()
            .map_err(|source| Error::RecompressStaging { source })?;

        let outcome = recompress_into(
            &self.sources,
            &at,
            RecompressStage { path: &path, file },
            ceiling.into(),
            Halt::NEVER,
        )
        .map_err(|source| Error::Recompress {
            source: Box::new(source),
        })?;

        match outcome {
            Recompressed::Smaller(shrunk) => {
                let staged = RecompressStaged::Staged {
                    stage_path: path.display().to_string(),
                    before: shrunk.before,
                    after: shrunk.after,
                };

                self.services.stages.0.lock().insert(path, held);
                Ok(staged)
            }
            Recompressed::Kept(kept) => Ok(RecompressStaged::KeptOriginal {
                reason: reason_of(kept),
                said: said_of(kept),
            }),
        }
    }

    pub fn song_recompressed(
        &self,
        id: u64,
        stage_path: String,
        modified_ms: i64,
    ) -> Result<Track> {
        let track = self.catalog.track(id)?;
        let path = PathBuf::from(&stage_path);
        let staged = fs::metadata(&path)
            .map_err(|_| Error::UnknownRecompressStage)?
            .len();

        if !self.services.stages.0.lock().contains_key(&path) {
            return Err(Error::UnknownRecompressStage);
        }

        let written = self.measured(&track.uri);

        if written != Some(staged) {
            return Err(Error::RecompressNotWritten {
                staged,
                written: written.unwrap_or(0),
            });
        }

        self.forget_recompress_stage(stage_path);
        self.services.analyses.lock().remove(&id);

        let stamp = DocumentStamp::of(staged, modified_ms);

        self.indexed(track.uri, track.name, stamp, Spelled::Anew)
    }

    pub fn forget_recompress_stage(&self, stage_path: String) {
        let path = PathBuf::from(stage_path);

        if self.services.stages.0.lock().remove(&path).is_none() {
            return;
        }
        if let Err(error) = fs::remove_file(&path) {
            tracing::debug!(%error, path = %path.display(), "a re-compressed copy could not be taken away");
        }
    }
}

fn comes_after(song: &SizedSong, after: SizedSongAfter) -> bool {
    song.bytes < after.bytes || (song.bytes == after.bytes && song.track.id > after.id)
}

impl Stages {
    fn sweep(&self, folder: &Path) {
        let held = self.0.lock();
        let Ok(entries) = fs::read_dir(folder) else {
            return;
        };

        for entry in entries.flatten() {
            let left = entry.path();

            if held.contains_key(&left) {
                continue;
            }

            let Some(writer) = staged_by(&left) else {
                continue;
            };

            if writer::writer_of(&left, writer) == Writer::Living {
                continue;
            }
            if let Err(error) = fs::remove_file(&left) {
                tracing::debug!(%error, path = %left.display(), "a re-compressed copy left behind could not be swept");
            }
        }
    }
}

fn staged_by(path: &Path) -> Option<u32> {
    let name = path.file_name()?.to_str()?.strip_suffix(STAGE_SUFFIX)?;
    let (writer, counted) = name.split_once(STAGED_BY_AND_COUNTED)?;

    counted.parse::<u64>().ok()?;
    writer.parse().ok()
}

const fn reason_of(kept: KeptOriginal) -> RecompressKept {
    match kept {
        KeptOriginal::NoSmaller { .. } => RecompressKept::NoSmaller,
        KeptOriginal::Unverified => RecompressKept::Unverified,
        KeptOriginal::ChangedMeanwhile => RecompressKept::ChangedMeanwhile,
        KeptOriginal::Halted => RecompressKept::Halted,
        KeptOriginal::NotAFlac => RecompressKept::NotAFlac,
        KeptOriginal::Vaulted => RecompressKept::Vaulted,
        KeptOriginal::CutBySheet => RecompressKept::CutBySheet,
        KeptOriginal::OutsideTheLibrary => RecompressKept::OutsideTheLibrary,
        KeptOriginal::HeadUnread(_) => RecompressKept::HeadUnread,
        KeptOriginal::TagAfterTheAudio(TrailingTag::Id3v1) => RecompressKept::Id3AfterTheAudio,
        KeptOriginal::TagAfterTheAudio(TrailingTag::Ape) => RecompressKept::ApeAfterTheAudio,
        KeptOriginal::BeyondTheEncoder(_) => RecompressKept::BeyondTheEncoder,
        KeptOriginal::TiedToTheSamples(_) => RecompressKept::TiedToTheSamples,
        KeptOriginal::DisagreesWithItsDigest => RecompressKept::DisagreesWithItsDigest,
        KeptOriginal::WiderThanItsDepth => RecompressKept::WiderThanItsDepth,
        KeptOriginal::TurnsPartway => RecompressKept::TurnsPartway,
        KeptOriginal::Undecodable => RecompressKept::Undecodable,
        KeptOriginal::Silent => RecompressKept::Silent,
        KeptOriginal::HeldUnderOtherNames => RecompressKept::HeldUnderOtherNames,
        KeptOriginal::AttributesNotCarried => RecompressKept::AttributesNotCarried,
    }
}

fn said_of(kept: KeptOriginal) -> String {
    let said = match kept {
        KeptOriginal::NoSmaller { .. } => words::recompress::no_smaller(),
        KeptOriginal::Unverified => words::recompress::unverified(),
        KeptOriginal::ChangedMeanwhile => words::recompress::changed_meanwhile(),
        KeptOriginal::Halted => words::recompress::halted(),
        KeptOriginal::NotAFlac => words::recompress::not_a_flac(),
        KeptOriginal::Vaulted => words::recompress::vaulted(),
        KeptOriginal::CutBySheet => words::recompress::cut_by_sheet(),
        KeptOriginal::OutsideTheLibrary => words::recompress::outside_the_library(),
        KeptOriginal::HeadUnread(_) => words::recompress::head_unread(),
        KeptOriginal::TagAfterTheAudio(TrailingTag::Id3v1) => {
            words::recompress::id3_after_the_audio()
        }
        KeptOriginal::TagAfterTheAudio(TrailingTag::Ape) => {
            words::recompress::ape_after_the_audio()
        }
        KeptOriginal::BeyondTheEncoder(shape) => {
            return words::recompress::BeyondTheEncoder {
                bits: shape.bits.to_string(),
                rate: shape.rate.kilohertz(),
            }
            .said();
        }
        KeptOriginal::TiedToTheSamples(block) => {
            return words::recompress::tied_to_the_samples(block_named(block));
        }
        KeptOriginal::DisagreesWithItsDigest => words::recompress::disagrees_with_its_digest(),
        KeptOriginal::WiderThanItsDepth => words::recompress::wider_than_its_depth(),
        KeptOriginal::TurnsPartway => words::recompress::turns_partway(),
        KeptOriginal::Undecodable => words::recompress::undecodable(),
        KeptOriginal::Silent => words::recompress::silent(),
        KeptOriginal::HeldUnderOtherNames => words::recompress::held_under_other_names(),
        KeptOriginal::AttributesNotCarried => words::recompress::attributes_not_carried(),
    };

    said.to_owned()
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
    use std::{f64::consts::TAU, fs::File};

    use resonate_core::{ChannelLayout, SampleFormat, SampleRate, StreamSpec};
    use resonate_vault::FlacWriter;

    use super::*;
    use crate::harness::{FOLDER, Harness, silence};

    const TONE_FRAMES: usize = 96_000;

    fn a_high_resolution_flac(harness: &Harness, name: &str) -> Track {
        let path = harness.folder.join(name);
        let file = File::create(&path).expect("a FLAC file");
        let spec = StreamSpec::new(
            SampleRate::HZ_96000,
            ChannelLayout::Stereo,
            SampleFormat::S24,
        );

        let mut writer = FlacWriter::over(file, &path, spec, 24, &[], None).expect("a writer");
        let full = f64::from((1_u32 << 23) - 1);
        let samples: Vec<i32> = (0..TONE_FRAMES)
            .flat_map(|frame| {
                let moment = frame as f64 / 96_000.0;
                let tone = ((TAU * 440.0 * moment).sin() * 0.4 * full).round() as i32;

                [tone, tone / 2]
            })
            .collect();

        writer.push(&samples).expect("the samples");
        writer.finish().expect("a written FLAC");
        harness
            .player
            .index_document(format!("{FOLDER}{name}"), name.to_owned())
            .expect("the FLAC indexed")
    }

    fn ids(songs: &[SizedSong]) -> Vec<u64> {
        songs.iter().map(|song| song.track.id).collect()
    }

    #[test]
    fn the_largest_songs_are_listed_first_a_page_at_a_time_measured_where_unstamped() {
        let harness = Harness::new();
        let larger = harness.folder.join("larger.wav");
        let mut bytes = silence();

        bytes.extend(std::iter::repeat_n(0, 4_096));
        fs::write(&larger, &bytes).expect("a larger song");

        let added = harness
            .player
            .index_document(format!("{FOLDER}larger.wav"), "larger.wav".to_owned())
            .expect("the larger song indexed");
        let whole = harness.player.largest_songs(None, 100).expect("every song");
        let mut paged = Vec::new();
        let mut after = None;

        loop {
            let page = harness.player.largest_songs(after, 2).expect("a page");

            let Some(last) = page.last() else {
                break;
            };

            after = Some(SizedSongAfter {
                bytes: last.bytes,
                id: last.track.id,
            });
            paged.extend(page);
        }

        assert_eq!(whole[0].track.id, added.id);
        assert_eq!(whole[0].bytes, bytes.len() as u64);
        assert_eq!(whole.len(), harness.songs.len() + 1);
        assert!(whole.windows(2).all(|pair| pair[0].bytes >= pair[1].bytes));
        assert_eq!(ids(&paged), ids(&whole));
    }

    #[test]
    fn a_song_recompressed_is_staged_written_over_its_document_and_indexed_anew() {
        let harness = Harness::new();
        let song = a_high_resolution_flac(&harness, "harbour.flac");
        let document = harness.folder.join("harbour.flac");
        let before = fs::metadata(&document).expect("the song").len();

        let staged = harness
            .player
            .recompress_song(song.id, RecompressCeiling::Cd)
            .expect("a re-compression");

        let RecompressStaged::Staged {
            stage_path,
            before: said_before,
            after,
        } = staged
        else {
            panic!("a high resolution song was kept under the CD ceiling: {staged:?}");
        };
        let unwritten = harness
            .player
            .song_recompressed(song.id, stage_path.clone(), 1);

        assert_eq!(said_before, before);
        assert_eq!(fs::metadata(&document).expect("the song").len(), before);
        assert!(
            matches!(unwritten, Err(Error::RecompressNotWritten { .. })),
            "{unwritten:?}"
        );

        fs::copy(&stage_path, &document).expect("the copy written over the song");

        let indexed = harness
            .player
            .song_recompressed(song.id, stage_path.clone(), 1)
            .expect("the song indexed again");
        let again = harness
            .player
            .song_recompressed(song.id, stage_path.clone(), 1);

        assert_eq!(indexed.id, song.id);
        assert_eq!(indexed.rate, 44_100);
        assert_eq!(indexed.bits, Some(16));
        assert_eq!(fs::metadata(&document).expect("the song").len(), after);
        assert!(!Path::new(&stage_path).exists());
        assert!(
            matches!(again, Err(Error::UnknownRecompressStage)),
            "{again:?}"
        );
    }

    #[test]
    fn a_song_that_is_no_flac_is_kept_and_says_why_and_a_stage_forgotten_is_gone() {
        let harness = Harness::new();
        let wave = harness.songs[0].clone();
        let flac = a_high_resolution_flac(&harness, "lantern.flac");

        let kept = harness
            .player
            .recompress_song(wave.id, RecompressCeiling::Kept)
            .expect("an answer");
        let staged = harness
            .player
            .recompress_song(flac.id, RecompressCeiling::High)
            .expect("a re-compression");

        assert_eq!(
            kept,
            RecompressStaged::KeptOriginal {
                reason: RecompressKept::NotAFlac,
                said: words::recompress::not_a_flac().to_owned(),
            }
        );

        let RecompressStaged::Staged { stage_path, .. } = staged else {
            panic!("a 96 kHz song was kept under the 48 kHz ceiling: {staged:?}");
        };

        harness.player.forget_recompress_stage(stage_path.clone());

        assert!(!Path::new(&stage_path).exists());
    }
}
