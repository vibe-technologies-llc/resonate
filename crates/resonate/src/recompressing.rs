use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use resonate_core::StreamSpec;
use resonate_library::{
    BlockKind, Ceiling, Codec, KeptOriginal, Library, PassKind, RecompressSummary, Recompressed,
    SoundShape, TrailingTag,
};

use crate::{
    arguments::local_path,
    cli::CeilingArg,
    error::Result,
    info::bytes_text,
    organising::under_its_root,
    passes::{finished, none_failed, until_told},
    table::Table,
    words::{self, columns},
};

#[derive(Clone, Copy)]
pub(crate) struct Recompressing<'a> {
    pub(crate) files: &'a [PathBuf],
    pub(crate) all: bool,
    pub(crate) to: CeilingArg,
    pub(crate) apply: bool,
}

pub(crate) const fn ceiling_of(asked: CeilingArg) -> Ceiling {
    match asked {
        CeilingArg::Kept => Ceiling::Kept,
        CeilingArg::Studio => Ceiling::Studio,
        CeilingArg::High => Ceiling::High,
        CeilingArg::Cd => Ceiling::Cd,
    }
}

struct Listed {
    path: PathBuf,
    bytes: u64,
    codec: Codec,
    spec: StreamSpec,
}

pub(crate) fn recompress(library: &Library, asked: Recompressing<'_>) -> Result<()> {
    let ceiling = ceiling_of(asked.to);
    let roots = library.roots()?;

    if asked.all && asked.apply {
        let summary = until_told(library.recompress_all(ceiling)?)?;

        said_on!("{}", told_of(&summary, &roots));
        finished(PassKind::Recompress, summary.cancelled)?;
        return none_failed(PassKind::Recompress, summary.stats.failed);
    }
    if asked.all {
        let listed: Vec<Listed> = library
            .recompress_candidates()?
            .into_iter()
            .map(|candidate| Listed {
                path: candidate.path,
                bytes: candidate.bytes,
                codec: Codec::Flac,
                spec: candidate.spec,
            })
            .collect();

        said_on!("{}", candidates(&listed, &roots));
        return Ok(());
    }

    let mut held = Vec::with_capacity(asked.files.len());

    for file in asked.files {
        let path = local_path(file);
        let path = path.canonicalize().unwrap_or(path);

        match library.track_at(&path, None)? {
            Some(track) => held.push((path, track)),
            None => said!(
                "{}",
                words::recompress::not_held(path.display().to_string())
            ),
        }
    }
    if !asked.apply {
        let listed: Vec<Listed> = held
            .into_iter()
            .map(|(path, track)| Listed {
                path,
                bytes: track.file_size,
                codec: track.codec,
                spec: track.spec,
            })
            .collect();

        said_on!("{}", candidates(&listed, &roots));
        return Ok(());
    }

    let halt = Arc::new(AtomicBool::new(false));
    let asking = Arc::clone(&halt);
    let _interrupting =
        crate::signals::cancel_when_told(move || asking.store(true, Ordering::Relaxed));
    let mut saved = Saved::default();

    for (path, track) in held {
        if halt.load(Ordering::Relaxed) {
            break;
        }

        let file = under_its_root(&path, &roots);

        match library.recompress(track.id, ceiling, &halt) {
            Ok(outcome) => {
                saved.heard(outcome);
                said!("{}", said_of(file, outcome));
            }
            Err(error) => {
                saved.failed += 1;
                said!(
                    "{}",
                    words::recompress::Failed {
                        error: error.to_string(),
                        file
                    }
                    .said()
                );
            }
        }
    }
    said!("{}", saved.said());
    finished(PassKind::Recompress, halt.load(Ordering::Relaxed))?;
    none_failed(PassKind::Recompress, saved.failed)
}

pub(crate) fn storage(library: &Library, limit: usize) -> Result<()> {
    let roots = library.roots()?;
    let largest = library.largest_files(None, limit)?;

    if largest.is_empty() {
        said!("{}", words::recompress::nothing_stored());
        return Ok(());
    }

    let mut table = Table::new(vec![columns::size(), columns::format(), columns::file()]);

    for file in &largest {
        table.push(vec![
            bytes_text(file.bytes),
            format_of(file.codec, file.spec),
            under_its_root(&file.path, &roots),
        ]);
    }
    said_on!("{}", table.render());
    Ok(())
}

#[derive(Default)]
struct Saved {
    smaller: u64,
    before: u64,
    after: u64,
    failed: u64,
}

impl Saved {
    fn heard(&mut self, outcome: Recompressed) {
        if let Recompressed::Smaller(shrunk) = outcome {
            self.smaller += 1;
            self.before += shrunk.before;
            self.after += shrunk.after;
        }
    }

    fn said(&self) -> String {
        words::recompress::Saved {
            count: self.smaller,
            saved: bytes_text(self.before.saturating_sub(self.after)),
        }
        .said()
    }
}

fn told_of(summary: &RecompressSummary, roots: &[PathBuf]) -> String {
    let mut told = String::new();

    for file in &summary.files {
        told.push_str(&said_of(under_its_root(&file.path, roots), file.outcome));
        told.push('\n');
    }
    told.push_str(
        &words::recompress::Saved {
            count: summary.stats.smaller,
            saved: bytes_text(summary.stats.saved()),
        }
        .said(),
    );
    told.push('\n');
    if summary.cancelled {
        told.push_str(words::main::cancelled());
        told.push('\n');
    }
    told
}

fn said_of(file: String, outcome: Recompressed) -> String {
    match outcome {
        Recompressed::Smaller(shrunk) => words::recompress::Shrunk {
            after: bytes_text(shrunk.after),
            before: bytes_text(shrunk.before),
            file,
        }
        .said(),
        Recompressed::Kept(kept) => words::recompress::Kept {
            file,
            why: kept_because(kept),
        }
        .said(),
    }
}

fn kept_because(kept: KeptOriginal) -> String {
    let why = match kept {
        KeptOriginal::NoSmaller { .. } => words::recompress::why_no_smaller(),
        KeptOriginal::Unverified => words::recompress::why_unverified(),
        KeptOriginal::ChangedMeanwhile => words::recompress::why_changed_meanwhile(),
        KeptOriginal::Halted => words::recompress::why_halted(),
        KeptOriginal::NotAFlac => words::recompress::why_not_a_flac(),
        KeptOriginal::Vaulted => words::recompress::why_vaulted(),
        KeptOriginal::CutBySheet => words::recompress::why_cut_by_sheet(),
        KeptOriginal::OutsideTheLibrary => words::recompress::why_outside_the_library(),
        KeptOriginal::HeadUnread(_) => words::recompress::why_head_unread(),
        KeptOriginal::TagAfterTheAudio(TrailingTag::Id3v1) => {
            words::recompress::why_id3_after_the_audio()
        }
        KeptOriginal::TagAfterTheAudio(TrailingTag::Ape) => {
            words::recompress::why_ape_after_the_audio()
        }
        KeptOriginal::BeyondTheEncoder(shape) => return beyond_the_encoder(shape),
        KeptOriginal::TiedToTheSamples(block) => {
            return words::recompress::why_tied_to_the_samples(block_named(block));
        }
        KeptOriginal::DisagreesWithItsDigest => words::recompress::why_disagrees_with_its_digest(),
        KeptOriginal::WiderThanItsDepth => words::recompress::why_wider_than_its_depth(),
        KeptOriginal::TurnsPartway => words::recompress::why_turns_partway(),
        KeptOriginal::Undecodable => words::recompress::why_undecodable(),
        KeptOriginal::Silent => words::recompress::why_silent(),
        KeptOriginal::HeldUnderOtherNames => words::recompress::why_held_under_other_names(),
        KeptOriginal::AttributesNotCarried => words::recompress::why_attributes_not_carried(),
    };

    why.to_owned()
}

fn beyond_the_encoder(shape: SoundShape) -> String {
    words::recompress::WhyBeyondTheEncoder {
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

fn candidates(listed: &[Listed], roots: &[PathBuf]) -> String {
    if listed.is_empty() {
        return format!("{}\n", words::recompress::nothing_held());
    }

    let mut table = Table::new(vec![columns::size(), columns::format(), columns::file()]);

    for file in listed {
        table.push(vec![
            bytes_text(file.bytes),
            format_of(file.codec, file.spec),
            under_its_root(&file.path, roots),
        ]);
    }

    let mut told = table.render();

    told.push_str(
        &words::recompress::Candidates {
            bytes: bytes_text(listed.iter().map(|file| file.bytes).sum()),
            count: listed.len() as u64,
        }
        .said(),
    );
    told.push('\n');
    told
}

fn format_of(codec: Codec, spec: StreamSpec) -> String {
    words::recompress::FormatOf {
        bits: spec.format.valid_bits().to_string(),
        codec: codec.as_str(),
        rate: spec.rate.kilohertz(),
    }
    .said()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn every_ceiling_the_grammar_names_is_the_librarys_of_the_same_meaning() {
        assert_eq!(ceiling_of(CeilingArg::Kept), Ceiling::Kept);
        assert_eq!(ceiling_of(CeilingArg::Studio), Ceiling::Studio);
        assert_eq!(ceiling_of(CeilingArg::High), Ceiling::High);
        assert_eq!(ceiling_of(CeilingArg::Cd), Ceiling::Cd);
    }

    #[test]
    fn the_settings_file_spells_each_ceiling_as_the_grammar_does() {
        use clap::ValueEnum as _;

        for asked in CeilingArg::value_variants() {
            let spelled = asked
                .to_possible_value()
                .expect("every ceiling is offered")
                .get_name()
                .to_owned();

            assert_eq!(ceiling_of(*asked).as_str(), spelled);
        }
    }

    #[test]
    fn a_file_kept_says_why_in_the_listeners_words() {
        let said = said_of(
            "Lantern.flac".to_owned(),
            Recompressed::Kept(KeptOriginal::Vaulted),
        );

        assert!(said.starts_with("Lantern.flac"), "{said}");
        assert!(said.contains(words::recompress::why_vaulted()), "{said}");
    }

    #[test]
    fn a_path_under_no_root_is_written_whole() {
        let path = Path::new("/elsewhere/Lantern.flac");

        assert_eq!(under_its_root(path, &[]), path.display().to_string());
    }
}
