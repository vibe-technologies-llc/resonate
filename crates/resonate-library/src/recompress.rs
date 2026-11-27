use std::{
    ffi::OsStr,
    fs::{self, File},
    io::{self, Seek, SeekFrom},
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
};

use resonate_codec::{Codec, DecodeStatus, Decoder, Sources, carries_what_the_file_did};
use resonate_core::{
    AudioBuffer, BitDepth, ChannelLayout, MediaLocation, RateFamily, SampleFormat, SampleRate,
    StreamSpec,
    file_facts::{self, FileStamp},
    naming, os_bytes,
    writer::{self, Held, Writer},
};
use resonate_dsp::{
    Dither, DitherKind, FilterPhase, NoiseShaping, Processor, Quality, Resampler, ResamplerConfig,
};
use resonate_vault::{
    BlockKind, FlacHead, FlacWriter, Halt, HeadBlock, HeadFault, SampleDigest, TrailingTag,
    heard_at_depth, narrowed_to_depth,
};

use crate::{Error, RecompressOp, Result};

const ENCODER_RATE_AT_MOST: u32 = 96_000;
const ENCODER_BITS_AT_MOST: u8 = 24;
const DECODED_AS: SampleFormat = SampleFormat::S32;
const FRAMES_AT_A_TIME: usize = 4_096;
const DITHER_SEED: u64 = 0x5eed_f1ac;
const SHAPED_AT_OR_UNDER_BITS: u8 = 16;
const STAGED_BY_AND_COUNTED: char = '-';
const STAGE_SUFFIX: &str = ".resonate-recompress";
const LONGEST_LETTER_BYTES: usize = 4;

static STAGED: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub enum Ceiling {
    #[default]
    Kept,
    Studio,
    High,
    Cd,
}

struct Bound {
    rate: SampleRate,
    bits: u8,
}

impl Ceiling {
    pub const ALL: [Self; 4] = [Self::Kept, Self::Studio, Self::High, Self::Cd];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Kept => "kept",
            Self::Studio => "96k",
            Self::High => "48k",
            Self::Cd => "cd",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|ceiling| ceiling.as_str().eq_ignore_ascii_case(text.trim()))
    }

    const fn bound(self) -> Option<Bound> {
        match self {
            Self::Kept => None,
            Self::Studio => Some(Bound {
                rate: SampleRate::HZ_96000,
                bits: 24,
            }),
            Self::High => Some(Bound {
                rate: SampleRate::HZ_48000,
                bits: 24,
            }),
            Self::Cd => Some(Bound {
                rate: SampleRate::HZ_44100,
                bits: 16,
            }),
        }
    }

    pub fn shape_for(self, from: SoundShape) -> Option<SoundShape> {
        let Some(bound) = self.bound() else {
            let encodable =
                from.rate.hz() <= ENCODER_RATE_AT_MOST && from.bits <= ENCODER_BITS_AT_MOST;

            return encodable.then_some(from);
        };

        Some(SoundShape {
            rate: rate_within(from.rate, bound.rate),
            bits: from.bits.min(bound.bits),
            channels: from.channels,
        })
    }
}

fn rate_within(from: SampleRate, at_most: SampleRate) -> SampleRate {
    if from <= at_most {
        return from;
    }

    let base = match from.family() {
        RateFamily::Base44100 => SampleRate::HZ_44100,
        RateFamily::Base48000 | RateFamily::Other => SampleRate::HZ_48000,
    };

    if base > at_most {
        return at_most;
    }

    let mut rate = base;

    while let Ok(doubled) = SampleRate::new(rate.hz() * 2) {
        if doubled > at_most {
            break;
        }
        rate = doubled;
    }
    rate
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct SoundShape {
    pub rate: SampleRate,
    pub bits: u8,
    pub channels: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shrunk {
    pub before: u64,
    pub after: u64,
    pub from: SoundShape,
    pub to: SoundShape,
}

impl Shrunk {
    pub const fn saved(self) -> u64 {
        self.before.saturating_sub(self.after)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeptOriginal {
    NoSmaller { before: u64 },
    Unverified,
    ChangedMeanwhile,
    Halted,
    NotAFlac,
    Vaulted,
    CutBySheet,
    OutsideTheLibrary,
    HeadUnread(HeadFault),
    TagAfterTheAudio(TrailingTag),
    BeyondTheEncoder(SoundShape),
    TiedToTheSamples(BlockKind),
    DisagreesWithItsDigest,
    WiderThanItsDepth,
    TurnsPartway,
    Undecodable,
    Silent,
    HeldUnderOtherNames,
    AttributesNotCarried,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recompressed {
    Smaller(Shrunk),
    Kept(KeptOriginal),
}

pub struct RecompressStage<'a> {
    pub path: &'a Path,
    pub file: File,
}

pub fn recompress_into(
    sources: &Sources,
    location: &MediaLocation,
    stage: RecompressStage<'_>,
    ceiling: Ceiling,
    halt: Halt<'_>,
) -> Result<Recompressed> {
    let path = stage.path.to_path_buf();
    let outcome = written_into(sources, location, stage, ceiling, halt);

    if !matches!(outcome, Ok(Recompressed::Smaller(_))) {
        let _ = fs::remove_file(&path);
    }
    outcome
}

fn written_into(
    sources: &Sources,
    location: &MediaLocation,
    stage: RecompressStage<'_>,
    ceiling: Ceiling,
    halt: Halt<'_>,
) -> Result<Recompressed> {
    let read_failed = |source| Error::RecompressRead {
        location: location.clone(),
        source: Box::new(source),
    };
    let mut media = sources.open(location).map_err(read_failed)?;

    let before = media
        .stream
        .seek(SeekFrom::End(0))
        .map_err(|source| Error::RecompressStage {
            op: RecompressOp::Read,
            path: stage.path.to_path_buf(),
            source,
        })?;
    let head = match FlacHead::read(media.stream.as_mut()) {
        Ok(head) => head,
        Err(HeadFault::NotAFlac) => return Ok(Recompressed::Kept(KeptOriginal::NotAFlac)),
        Err(fault) => return Ok(Recompressed::Kept(KeptOriginal::HeadUnread(fault))),
    };

    drop(media);
    if let Some(tag) = head.trailing {
        return Ok(Recompressed::Kept(KeptOriginal::TagAfterTheAudio(tag)));
    }

    let (mut decoder, info) = Decoder::open(sources, location).map_err(read_failed)?;

    if Codec::from_id(info.codec) != Codec::Flac {
        return Ok(Recompressed::Kept(KeptOriginal::NotAFlac));
    }

    let from = SoundShape {
        rate: info.spec.rate,
        bits: head.info.bits,
        channels: info.spec.channel_count().get(),
    };
    let Some(to) = ceiling.shape_for(from) else {
        return Ok(Recompressed::Kept(KeptOriginal::BeyondTheEncoder(from)));
    };

    if let Some(kind) = tied_to_the_samples(&head.blocks, from, to) {
        return Ok(Recompressed::Kept(KeptOriginal::TiedToTheSamples(kind)));
    }

    let spec = StreamSpec::new(to.rate, info.spec.channels, DECODED_AS);
    let carried = head.carried();

    let encoded = encoded(
        &mut decoder,
        Encoding {
            stage,
            spec,
            from,
            to,
            carried: &carried,
            before,
            halt,
        },
        info.spec.channels,
    );
    let heard = match encoded {
        Ok(Ended::Heard(heard)) => heard,
        Ok(Ended::Kept(kept)) => return Ok(Recompressed::Kept(kept)),
        Err(error) => return Err(error.located(location)),
    };

    if head
        .info
        .digest
        .is_some_and(|declared| declared != heard.source)
        || head
            .info
            .frames
            .is_some_and(|declared| declared != heard.source_frames)
    {
        return Ok(Recompressed::Kept(KeptOriginal::DisagreesWithItsDigest));
    }
    if from == to && heard.written.digest != heard.source {
        return Ok(Recompressed::Kept(KeptOriginal::Unverified));
    }

    #[cfg(feature = "faults")]
    if let Some(local) = location.as_path() {
        crate::faults::spoil_if_asked(local, &heard.stage);
    }

    match heard_at_depth(
        &Sources::local(),
        &MediaLocation::local(&heard.stage),
        to.bits,
        halt,
    ) {
        Ok(read)
            if read.digest == heard.written.digest
                && read.frames == heard.written.frames
                && !read.wider_than_its_depth => {}
        Ok(_) => return Ok(Recompressed::Kept(KeptOriginal::Unverified)),
        Err(resonate_vault::Error::Halted) => return Ok(Recompressed::Kept(KeptOriginal::Halted)),
        Err(error) => {
            tracing::debug!(%error, stage = %heard.stage.display(), "a re-compressed copy did not read back");
            return Ok(Recompressed::Kept(KeptOriginal::Unverified));
        }
    }

    Ok(Recompressed::Smaller(Shrunk {
        before,
        after: heard.written.bytes,
        from,
        to,
    }))
}

fn tied_to_the_samples(
    blocks: &[HeadBlock],
    from: SoundShape,
    to: SoundShape,
) -> Option<BlockKind> {
    let resampled = from.rate != to.rate;
    let reshaped = resampled || from.bits != to.bits;

    blocks
        .iter()
        .map(|block| block.kind)
        .find(|kind| match kind {
            BlockKind::CueSheet => resampled,
            BlockKind::Application => reshaped,
            _ => false,
        })
}

struct Encoding<'a> {
    stage: RecompressStage<'a>,
    spec: StreamSpec,
    from: SoundShape,
    to: SoundShape,
    carried: &'a [HeadBlock],
    before: u64,
    halt: Halt<'a>,
}

struct Heard {
    stage: PathBuf,
    source: resonate_vault::VaultKey,
    source_frames: resonate_core::Frames,
    written: resonate_vault::FlacWritten,
}

enum Ended {
    Heard(Heard),
    Kept(KeptOriginal),
}

enum Failed {
    Vault(RecompressOp, resonate_vault::Error),
    Read(resonate_codec::Error),
}

impl Failed {
    fn located(self, location: &MediaLocation) -> Error {
        match self {
            Self::Vault(op, source) => Error::Recompress {
                location: location.clone(),
                op,
                source: Box::new(source),
            },
            Self::Read(source) => Error::RecompressRead {
                location: location.clone(),
                source: Box::new(source),
            },
        }
    }
}

fn encoded(
    decoder: &mut Decoder,
    encoding: Encoding<'_>,
    channels: ChannelLayout,
) -> std::result::Result<Ended, Failed> {
    let Encoding {
        stage,
        spec,
        from,
        to,
        carried,
        before,
        halt,
    } = encoding;
    let stage_path = stage.path.to_path_buf();

    let mut writer =
        match FlacWriter::over(stage.file, stage.path, spec, to.bits, carried, Some(before)) {
            Ok(writer) => writer,
            Err(resonate_vault::Error::Unencodable { .. }) => {
                return Ok(Ended::Kept(KeptOriginal::BeyondTheEncoder(from)));
            }
            Err(error) => return Err(Failed::Vault(RecompressOp::Stage, error)),
        };
    let mut converting = match from == to {
        true => None,
        false => match Converting::between(from, to, channels) {
            Some(converting) => Some(converting),
            None => return Ok(Ended::Kept(KeptOriginal::BeyondTheEncoder(from))),
        },
    };

    decoder.refuse_holes();
    decoder.set_output_format(DECODED_AS);

    let mut block = AudioBuffer::empty(StreamSpec::new(from.rate, channels, DECODED_AS));
    let mut samples = Vec::new();
    let mut source = SampleDigest::default();
    let mut source_frames = 0_u64;

    loop {
        if halt.is_asked() {
            return Ok(Ended::Kept(KeptOriginal::Halted));
        }
        if writer.outgrew() {
            return Ok(Ended::Kept(KeptOriginal::NoSmaller { before }));
        }

        let status = match decoder.next_block(&mut block) {
            Ok(status) => status,
            Err(error) => return heard_failing(error),
        };

        match status {
            DecodeStatus::EndOfStream => break,
            DecodeStatus::Reformatted(_) => return Ok(Ended::Kept(KeptOriginal::TurnsPartway)),
            DecodeStatus::Decoded => {}
        }

        widened(&block, &mut samples);
        if !narrowed_to_depth(&mut samples, from.bits) {
            return Ok(Ended::Kept(KeptOriginal::WiderThanItsDepth));
        }
        source.note(&samples, from.bits);
        source_frames += block.frames() as u64;

        let pushed = match &mut converting {
            Some(converting) => converting.through(&samples, &mut writer),
            None => writer.push(&samples),
        };

        pushed.map_err(|error| Failed::Vault(RecompressOp::Encode, error))?;
    }

    if let Some(converting) = &mut converting {
        converting
            .finish(&mut writer)
            .map_err(|error| Failed::Vault(RecompressOp::Encode, error))?;
    }
    if source_frames == 0 {
        return Ok(Ended::Kept(KeptOriginal::Silent));
    }

    let written = writer
        .finish()
        .map_err(|error| Failed::Vault(RecompressOp::Encode, error))?;

    if written.outgrew {
        return Ok(Ended::Kept(KeptOriginal::NoSmaller { before }));
    }
    Ok(Ended::Heard(Heard {
        stage: stage_path,
        source: source.settled(),
        source_frames: resonate_core::Frames(source_frames),
        written,
    }))
}

fn heard_failing(error: resonate_codec::Error) -> std::result::Result<Ended, Failed> {
    if error.is_out_of_reach() {
        return Err(Failed::Read(error));
    }
    match error {
        resonate_codec::Error::FormatChanged { .. } => Ok(Ended::Kept(KeptOriginal::TurnsPartway)),
        error => {
            tracing::debug!(%error, "a FLAC to re-compress would not decode whole");
            Ok(Ended::Kept(KeptOriginal::Undecodable))
        }
    }
}

fn widened(block: &AudioBuffer, into: &mut Vec<i32>) {
    into.clear();
    if let resonate_core::SampleData::S32(samples) = block.data() {
        into.extend_from_slice(samples);
    }
}

struct Converting {
    from_scale: f64,
    to_scale: f64,
    channels: usize,
    resampler: Option<Resampler>,
    dither: Dither,
    widened: Vec<f64>,
    resampled: Vec<f64>,
    dithered: Vec<f64>,
    narrowed: Vec<i32>,
}

impl Converting {
    fn between(from: SoundShape, to: SoundShape, channels: ChannelLayout) -> Option<Self> {
        let depth = BitDepth::from_bits(to.bits).ok()?;
        let shaping = match to.bits <= SHAPED_AT_OR_UNDER_BITS {
            true => NoiseShaping::Threshold,
            false => NoiseShaping::None,
        };

        let mut dither = Dither::new(depth, DitherKind::Triangular, shaping, DITHER_SEED);
        let resampler = match from.rate == to.rate {
            true => None,
            false => Some(
                Resampler::new(ResamplerConfig {
                    input_rate: from.rate,
                    output_rate: to.rate,
                    channels,
                    quality: Quality::VeryHigh,
                    phase: FilterPhase::Linear,
                    max_frames_in: FRAMES_AT_A_TIME,
                })
                .ok()?,
            ),
        };

        dither
            .prepare(
                StreamSpec::new(to.rate, channels, SampleFormat::F32),
                FRAMES_AT_A_TIME,
            )
            .ok()?;
        Some(Self {
            from_scale: full_scale_of(from.bits),
            to_scale: full_scale_of(to.bits),
            channels: usize::from(channels.count().get()),
            resampler,
            dither,
            widened: Vec::new(),
            resampled: Vec::new(),
            dithered: Vec::new(),
            narrowed: Vec::new(),
        })
    }

    fn through(&mut self, samples: &[i32], writer: &mut FlacWriter) -> resonate_vault::Result<()> {
        for chunk in samples.chunks(FRAMES_AT_A_TIME * self.channels) {
            self.widened.clear();
            self.widened.extend(
                chunk
                    .iter()
                    .map(|sample| f64::from(*sample) / self.from_scale),
            );

            let made = match &mut self.resampler {
                Some(resampler) => {
                    let frames = chunk.len() / self.channels;

                    self.resampled.clear();
                    self.resampled
                        .resize(resampler.max_output_frames(frames) * self.channels, 0.0);
                    resampler
                        .process(&self.widened, &mut self.resampled)
                        .frames_out
                        * self.channels
                }
                None => {
                    self.resampled.clone_from(&self.widened);
                    self.resampled.len()
                }
            };

            self.settle(made, writer)?;
        }
        Ok(())
    }

    fn finish(&mut self, writer: &mut FlacWriter) -> resonate_vault::Result<()> {
        let Some(resampler) = &mut self.resampler else {
            return Ok(());
        };

        self.resampled.clear();
        self.resampled
            .resize(resampler.max_flush_frames() * self.channels, 0.0);

        let made = resampler.flush(&mut self.resampled) * self.channels;

        self.settle(made, writer)
    }

    fn settle(&mut self, made: usize, writer: &mut FlacWriter) -> resonate_vault::Result<()> {
        let made = &self.resampled[..made.min(self.resampled.len())];

        for piece in made.chunks(FRAMES_AT_A_TIME * self.channels) {
            self.dithered.clear();
            self.dithered.resize(piece.len(), 0.0);
            self.dither.process(piece, &mut self.dithered);

            let lowest = -self.to_scale;
            let highest = self.to_scale - 1.0;

            self.narrowed.clear();
            self.narrowed.extend(
                self.dithered
                    .iter()
                    .map(|sample| (sample * self.to_scale).round().clamp(lowest, highest) as i32),
            );
            writer.push(&self.narrowed)?;
        }
        Ok(())
    }
}

fn full_scale_of(bits: u8) -> f64 {
    f64::from(1_u32 << (bits.clamp(1, 32) - 1))
}

pub(crate) struct InPlace<'a> {
    pub(crate) path: &'a Path,
    pub(crate) ceiling: Ceiling,
    pub(crate) halt: Halt<'a>,
}

pub(crate) fn recompress_in_place(asked: InPlace<'_>) -> Result<Recompressed> {
    let InPlace {
        path,
        ceiling,
        halt,
    } = asked;
    let staging_failed = |op, source| Error::RecompressStage {
        op,
        path: path.to_path_buf(),
        source,
    };

    sweep_what_a_dead_writer_staged(path);

    let read = FileStamp::at(path).map_err(|source| staging_failed(RecompressOp::Read, source))?;
    let stage = staged_beside(path);
    let held = Held::made(&stage).map_err(|source| staging_failed(RecompressOp::Stage, source))?;
    let file = held
        .file()
        .try_clone()
        .map_err(|source| staging_failed(RecompressOp::Stage, source))?;
    let location = MediaLocation::local(path);

    let outcome = recompress_into(
        &Sources::local(),
        &location,
        RecompressStage { path: &stage, file },
        ceiling,
        halt,
    )?;
    let Recompressed::Smaller(shrunk) = outcome else {
        return Ok(outcome);
    };

    let placed = placed_over(path, &stage, read);

    if !matches!(placed, Ok(Placed::Over)) {
        let _ = fs::remove_file(&stage);
    }
    drop(held);
    match placed {
        Ok(Placed::Over) => Ok(Recompressed::Smaller(shrunk)),
        Ok(Placed::Kept(kept)) => Ok(Recompressed::Kept(kept)),
        Err(source) => Err(staging_failed(RecompressOp::Place, source)),
    }
}

enum Placed {
    Over,
    Kept(KeptOriginal),
}

fn placed_over(path: &Path, stage: &Path, read: FileStamp) -> io::Result<Placed> {
    let standing = fs::metadata(path)?;

    if FileStamp::of(&standing) != read {
        return Ok(Placed::Kept(KeptOriginal::ChangedMeanwhile));
    }
    if has_other_names(&standing) {
        return Ok(Placed::Kept(KeptOriginal::HeldUnderOtherNames));
    }
    if !carries_what_the_file_did(&standing, path, stage) {
        return Ok(Placed::Kept(KeptOriginal::AttributesNotCarried));
    }
    file_facts::sync_file(stage)?;
    if FileStamp::at(path)? != read {
        return Ok(Placed::Kept(KeptOriginal::ChangedMeanwhile));
    }
    file_facts::replace(stage, path)?.noted(path);
    Ok(Placed::Over)
}

#[cfg(unix)]
fn has_other_names(standing: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;

    standing.nlink() > 1
}

#[cfg(not(unix))]
fn has_other_names(_: &fs::Metadata) -> bool {
    false
}

fn staged_beside(path: &Path) -> PathBuf {
    let stamp = format!(
        ".{}{STAGED_BY_AND_COUNTED}{}{STAGE_SUFFIX}",
        process::id(),
        STAGED.fetch_add(1, Ordering::Relaxed)
    );

    path.with_file_name(naming::named_within(
        ".",
        path.file_name().unwrap_or_default(),
        stamp,
    ))
}

fn staged_by(path: &Path, staged: &OsStr) -> Option<u32> {
    let name = os_bytes::bytes_of(path.file_name()?);
    let staged_bytes = os_bytes::bytes_of(staged);
    let unsuffixed = staged_bytes
        .strip_prefix(b".")?
        .strip_suffix(STAGE_SUFFIX.as_bytes())?;
    let dot = unsuffixed.iter().rposition(|byte| *byte == b'.')?;

    let (kept, stamp) = (&unsuffixed[..dot], &unsuffixed[dot + 1..]);
    let cut = staged.len() + LONGEST_LETTER_BYTES > naming::NAME_BYTES_AT_MOST;

    if kept != name.as_ref() && !(cut && !kept.is_empty() && name.starts_with(kept)) {
        return None;
    }

    let (writer, counted) = std::str::from_utf8(stamp)
        .ok()?
        .split_once(STAGED_BY_AND_COUNTED)?;

    counted.parse::<u64>().ok()?;
    writer.parse().ok()
}

fn sweep_what_a_dead_writer_staged(path: &Path) {
    let Some(folder) = path.parent() else {
        return;
    };
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };

    for entry in entries.flatten() {
        let Some(writer) = staged_by(path, &entry.file_name()) else {
            continue;
        };
        let left = entry.path();

        if writer::writer_of(&left, writer) == Writer::Living {
            continue;
        }
        match fs::remove_file(&left) {
            Ok(()) => {
                tracing::info!(path = %left.display(), "swept a copy a re-compression cut short left behind");
            }
            Err(error) => {
                tracing::debug!(%error, path = %left.display(), "a copy a re-compression cut short left behind could not be swept");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(rate: SampleRate, bits: u8) -> SoundShape {
        SoundShape {
            rate,
            bits,
            channels: 2,
        }
    }

    #[test]
    fn a_ceiling_reads_back_from_its_spelling_and_no_other() {
        for ceiling in Ceiling::ALL {
            assert_eq!(Ceiling::parse(ceiling.as_str()), Some(ceiling));
        }

        assert_eq!(Ceiling::parse("CD"), Some(Ceiling::Cd));
        assert_eq!(Ceiling::parse("192k"), None);
        assert_eq!(Ceiling::parse(""), None);
    }

    #[test]
    fn a_ceiling_takes_the_highest_rate_of_the_sources_family_under_it_and_the_lesser_depth() {
        for (from, ceiling, to) in [
            (
                shape(SampleRate::HZ_88200, 24),
                Ceiling::High,
                shape(SampleRate::HZ_44100, 24),
            ),
            (
                shape(SampleRate::HZ_192000, 24),
                Ceiling::High,
                shape(SampleRate::HZ_48000, 24),
            ),
            (
                shape(SampleRate::HZ_96000, 24),
                Ceiling::Studio,
                shape(SampleRate::HZ_96000, 24),
            ),
            (
                shape(SampleRate::HZ_176400, 24),
                Ceiling::Studio,
                shape(SampleRate::HZ_88200, 24),
            ),
            (
                shape(SampleRate::HZ_96000, 24),
                Ceiling::Cd,
                shape(SampleRate::HZ_44100, 16),
            ),
            (
                shape(SampleRate::HZ_48000, 16),
                Ceiling::Cd,
                shape(SampleRate::HZ_44100, 16),
            ),
            (
                shape(SampleRate::HZ_22050, 16),
                Ceiling::Cd,
                shape(SampleRate::HZ_22050, 16),
            ),
            (
                shape(SampleRate::HZ_44100, 16),
                Ceiling::Kept,
                shape(SampleRate::HZ_44100, 16),
            ),
        ] {
            assert_eq!(
                ceiling.shape_for(from),
                Some(to),
                "{from:?} under {ceiling:?}"
            );
        }
    }

    #[test]
    fn a_source_past_what_the_encoder_writes_is_kept_only_under_a_ceiling() {
        assert_eq!(
            Ceiling::Kept.shape_for(shape(SampleRate::HZ_192000, 24)),
            None
        );
        assert_eq!(
            Ceiling::Studio.shape_for(shape(SampleRate::HZ_192000, 24)),
            Some(shape(SampleRate::HZ_96000, 24))
        );
    }

    #[test]
    fn a_stage_is_named_beside_its_file_and_read_back_as_this_writers() {
        let path = Path::new("/music/Album/01 Song.flac");
        let stage = staged_beside(path);

        assert_eq!(stage.parent(), path.parent());
        assert_eq!(
            staged_by(path, stage.file_name().expect("a name")),
            Some(process::id())
        );
        assert_eq!(
            staged_by(
                Path::new("/music/Album/02 Other.flac"),
                stage.file_name().expect("a name")
            ),
            None
        );
    }
}
