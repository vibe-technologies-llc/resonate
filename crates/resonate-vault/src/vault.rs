use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    ops::Deref,
    path::{Component, Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
};

use parking_lot::Mutex;
use resonate_codec::{
    Codec, CoverArt, DecodeStatus, Decoder, MediaInfo, MediaStream, Sources, Speakers, probe,
};
use resonate_core::{AudioBuffer, FrameSpan, Frames, MediaLocation, SampleFormat, StreamSpec};

use crate::{
    cover,
    drawn::Drawings,
    error::{Error, Result, VaultOp},
    flac,
    form::{Form, WIDEST_FLAC_BITS},
    key::VaultKey,
    pcm::{self, Digest},
    wave,
};

pub(crate) const AUDIO: &str = "audio";
pub(crate) const COVERS: &str = "covers";
pub(crate) const STAGING: &str = "staging";
pub(crate) const COVER_EXTENSION: &str = "jxl";
pub(crate) const COMPRESSED_EXTENSION: &str = "zst";
const FLAC_EXTENSION: &str = "flac";
const WAVE_EXTENSION: &str = "wav";
const UNNAMED_EXTENSION: &str = "bin";
const LONGEST_EXTENSION: usize = 8;
const COPY_BYTES: usize = 1 << 20;
const SIXTEEN_BIT_CEILING: u8 = 16;
const FLAC_MAGIC: [u8; 4] = *b"fLaC";
const METADATA_HEADER_BYTES: usize = 4;
const LAST_BLOCK: u8 = 0x80;
const BLOCK_KIND: u8 = 0x7f;
const STREAM_INFO: u8 = 0;
const MOST_METADATA_BYTES: u64 = 64 << 20;
const LARGEST_DELIVERY: u64 = wave::LARGEST_PCM;

static STAGED: AtomicU64 = AtomicU64::new(0);

pub struct Vault {
    root: PathBuf,
    drawings: Drawings,
    landing: Mutex<()>,
}

struct Staged(PathBuf);

impl Deref for Staged {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for Staged {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        match fs::remove_file(&self.0) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(path = %self.0.display(), %error, "a staging file could not be taken away");
            }
        }
    }
}

pub struct Taking<'a> {
    pub sources: &'a Sources,
    pub location: &'a MediaLocation,
    pub span: Option<FrameSpan>,
    pub renewing: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Kept {
    pub key: VaultKey,
    pub form: Form,
    pub path: PathBuf,
    pub bytes: u64,
    pub was: u64,
    pub spec: StreamSpec,
    pub frames: Frames,
    pub codec: Codec,
    pub deduped: bool,
    pub replaced: bool,
}

#[derive(Clone, Copy)]
struct Weighing {
    renewing: bool,
    smaller_than: Option<u64>,
}

impl Weighing {
    const AS_IT_STANDS: Self = Self {
        renewing: false,
        smaller_than: None,
    };

    fn fits(self, bytes: u64) -> bool {
        self.smaller_than.is_none_or(|was| bytes < was)
    }
}

enum Landing {
    Landed(u64),
    Deduped(u64),
    Replaced(u64),
    NoSmaller,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeptCover {
    pub key: VaultKey,
    pub path: PathBuf,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    pub deduped: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Refusal {
    Empty,
    TooLarge,
    NotValidated,
    CutFromAnother,
    NoSmaller,
}

impl Refusal {
    pub const ALL: [Self; 5] = [
        Self::Empty,
        Self::TooLarge,
        Self::NotValidated,
        Self::CutFromAnother,
        Self::NoSmaller,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "holds no audio",
            Self::TooLarge => "is larger than a WAVE file holds",
            Self::NotValidated => "did not read back as what went in",
            Self::CutFromAnother => "is cut out of a file this build cannot re-encode",
            Self::NoSmaller => {
                "would come out no smaller than its share of the file it is cut from"
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Keeping {
    Kept(Kept),
    Refused(Refusal),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeldCover {
    pub key: VaultKey,
    pub path: PathBuf,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Holdings {
    pub objects: u64,
    pub bytes: u64,
    pub covers: u64,
    pub cover_bytes: u64,
}

impl Vault {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        for folder in [AUDIO, COVERS, STAGING] {
            let made = root.join(folder);
            fs::create_dir_all(&made)
                .map_err(|source| Error::io(VaultOp::MakeFolder, &made, source))?;
        }
        let root = root
            .canonicalize()
            .map_err(|source| Error::io(VaultOp::Resolve, &root, source))?;
        Ok(Self {
            root,
            drawings: Drawings::default(),
            landing: Mutex::new(()),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn within(&self, path: &Path) -> Result<PathBuf> {
        path.strip_prefix(&self.root)
            .ok()
            .filter(|within| names_a_place_inside(within))
            .map(Path::to_path_buf)
            .ok_or_else(|| Error::OutsideTheVault {
                path: path.to_path_buf(),
            })
    }

    pub fn at(&self, within: &Path) -> Result<PathBuf> {
        if names_a_place_inside(within) {
            Ok(self.root.join(within))
        } else {
            Err(Error::OutsideTheVault {
                path: within.to_path_buf(),
            })
        }
    }

    pub fn holds(&self, path: &Path) -> bool {
        path.starts_with(&self.root)
    }

    pub fn keep_delivered(&self, reader: &mut dyn Read, extension: &str) -> Result<Keeping> {
        self.keep_delivered_under(reader, extension, LARGEST_DELIVERY)
    }

    pub(crate) fn keep_delivered_under(
        &self,
        reader: &mut dyn Read,
        extension: &str,
        largest: u64,
    ) -> Result<Keeping> {
        let staging = self.staged(&sanitised_extension(Some(extension)))?;
        let kept = self
            .staged_delivery(reader, &staging, largest)
            .and_then(|whole| {
                if whole {
                    self.keep(&Taking {
                        sources: &Sources::local(),
                        location: &MediaLocation::local(staging.to_path_buf()),
                        span: None,
                        renewing: false,
                    })
                } else {
                    Ok(Keeping::Refused(Refusal::TooLarge))
                }
            });
        let discarded = self.discard(&staging);
        let kept = kept?;
        discarded?;
        Ok(kept)
    }

    fn staged_delivery(&self, reader: &mut dyn Read, staging: &Path, largest: u64) -> Result<bool> {
        self.inside(staging)?;
        let mut file =
            File::create(staging).map_err(|source| Error::io(VaultOp::Stage, staging, source))?;
        let copied = std::io::copy(&mut reader.take(largest.saturating_add(1)), &mut file)
            .map_err(|source| Error::io(VaultOp::Write, staging, source))?;
        file.sync_all()
            .map_err(|source| Error::io(VaultOp::Settle, staging, source))?;
        Ok(copied <= largest)
    }

    pub fn keep(&self, taking: &Taking<'_>) -> Result<Keeping> {
        let opened = match taking.span {
            Some(span) => Decoder::open_span(taking.sources, taking.location, span),
            None => Decoder::open(taking.sources, taking.location),
        };
        let (mut decoder, info) = opened.map_err(|source| Error::codec(VaultOp::Read, source))?;

        let codec = Codec::from_id(info.codec);
        let bits = info
            .bits_per_coded_sample
            .unwrap_or_else(|| info.spec.format.valid_bits());

        let whole = taking.span.is_none();
        let held = match taking.span {
            None => taking.location.as_path().and_then(sized_source),
            Some(span) => share_of(taking, span),
        };

        let speakers = info.speakers;
        let form = Form::of(codec, info.spec, bits).placing(speakers, info.spec.channel_count());
        let weighing = Weighing {
            renewing: taking.renewing,
            smaller_than: held,
        };
        let kept = match form {
            Form::Kept if !whole => return Ok(Keeping::Refused(Refusal::CutFromAnother)),
            Form::Kept => self.kept_whole(taking, codec, info.spec, info.duration)?,
            Form::Wave => self.kept_as_wave(weighing, &mut decoder, info.spec, speakers, codec)?,
            Form::Flac => {
                self.kept_as_flac(weighing, &mut decoder, info.spec, speakers, bits, codec)?
            }
        };

        let kept = match kept {
            Keeping::Refused(Refusal::NoSmaller) if whole => {
                self.kept_whole(taking, codec, info.spec, info.duration)?
            }
            kept => kept,
        };
        Ok(weighed(kept, held))
    }

    pub fn keep_cover(&self, art: &CoverArt) -> Result<KeptCover> {
        let mut digest = Digest::default();
        digest.note(&art.bytes);
        let key = digest.settled();
        let target = self.cover_path(key);

        if target.is_file() {
            let picture = cover::pixels_of_jxl(&self.read_inside(&target)?)?;
            return Ok(KeptCover {
                key,
                bytes: self.sized(&target)?,
                width: picture.width(),
                height: picture.height(),
                path: target,
                deduped: true,
            });
        }

        let drawn = cover::as_jxl(art)?;
        let staging = self.staged(COVER_EXTENSION)?;
        self.written_inside(&staging, &drawn.bytes)?;

        let read_back = cover::pixels_of_jxl(&drawn.bytes)?;
        if read_back != cover::read(art)? {
            self.discard(&staging)?;
            return Err(Error::PictureUnconfirmed {
                width: drawn.width,
                height: drawn.height,
            });
        }

        let bytes = self.landed(&staging, &target)?;
        Ok(KeptCover {
            key,
            path: target,
            bytes,
            width: drawn.width,
            height: drawn.height,
            deduped: false,
        })
    }

    pub fn picture(&self, path: &Path) -> Result<CoverArt> {
        self.inside(path)?;
        let key = named(path);
        if let Some(drawn) = key.and_then(|key| self.drawings.drawn(key)) {
            return Ok(drawn);
        }

        let drawn = cover::drawable(&self.read_inside(path)?)?;
        if let Some(key) = key {
            self.drawings.note(key, &drawn);
        }
        Ok(drawn)
    }

    pub fn verify(&self, path: &Path, form: Form) -> Result<bool> {
        self.inside(path)?;
        let Some(key) = named(path) else {
            return Ok(false);
        };

        match form {
            Form::Kept => {
                let mut digest = Digest::default();
                let mut file =
                    File::open(path).map_err(|source| Error::io(VaultOp::Verify, path, source))?;
                let mut buffer = vec![0_u8; COPY_BYTES];
                loop {
                    let read = file
                        .read(&mut buffer)
                        .map_err(|source| Error::io(VaultOp::Verify, path, source))?;
                    if read == 0 {
                        break;
                    }
                    digest.note(&buffer[..read]);
                }
                Ok(digest.settled() == key)
            }
            Form::Flac => Ok(read_back(path, None)?.key == key),
            Form::Wave => {
                let staging = self.staged(WAVE_EXTENSION)?;
                self.written_inside(&staging, &wave::decompressed(path)?)?;
                let read = read_back(&staging, None);
                self.discard(&staging)?;
                Ok(read?.key == key)
            }
        }
    }

    pub fn forget(&self, path: &Path) -> Result<bool> {
        self.inside(path)?;
        if let Some(key) = named(path) {
            self.drawings.forget(key);
        }
        if !path.is_file() {
            return Ok(false);
        }
        fs::remove_file(path)
            .map(|()| true)
            .map_err(|source| Error::io(VaultOp::Discard, path, source))
    }

    pub fn walk(&self) -> Result<Vec<PathBuf>> {
        let mut held = Vec::new();
        for folder in [AUDIO, COVERS] {
            self.walked(&self.root.join(folder), &mut held)?;
        }
        held.sort();
        Ok(held)
    }

    pub fn covers(&self) -> Result<Vec<HeldCover>> {
        let mut held = Vec::new();
        self.walked(&self.root.join(COVERS), &mut held)?;
        held.sort();
        Ok(held
            .into_iter()
            .filter(|path| path.extension().and_then(|held| held.to_str()) == Some(COVER_EXTENSION))
            .filter_map(|path| named(&path).map(|key| HeldCover { key, path }))
            .collect())
    }

    pub fn holding(&self) -> Result<Holdings> {
        let mut holdings = Holdings::default();
        for path in self.walk()? {
            let bytes = self.sized(&path)?;
            if path.starts_with(self.root.join(COVERS)) {
                holdings.covers += 1;
                holdings.cover_bytes += bytes;
            } else {
                holdings.objects += 1;
                holdings.bytes += bytes;
            }
        }
        Ok(holdings)
    }

    pub fn sweep_the_staging(&self) -> Result<u64> {
        let folder = self.root.join(STAGING);
        let mut swept = 0;
        let reading =
            fs::read_dir(&folder).map_err(|source| Error::io(VaultOp::Walk, &folder, source))?;
        for entry in reading {
            let entry = entry.map_err(|source| Error::io(VaultOp::Walk, &folder, source))?;
            if self.forget(&entry.path())? {
                swept += 1;
            }
        }
        Ok(swept)
    }

    fn kept_as_flac(
        &self,
        weighing: Weighing,
        decoder: &mut Decoder,
        spec: StreamSpec,
        speakers: Speakers,
        bits: u8,
        codec: Codec,
    ) -> Result<Keeping> {
        let (format, stored) = if bits <= SIXTEEN_BIT_CEILING {
            (SampleFormat::S16, SIXTEEN_BIT_CEILING)
        } else {
            (SampleFormat::S24, WIDEST_FLAC_BITS)
        };
        decoder.set_output_format(format);
        let spec = StreamSpec::new(spec.rate, spec.channels, format);

        let staging = self.staged(FLAC_EXTENSION)?;
        let encoded = match flac::encode(decoder, spec, stored, &staging) {
            Ok(encoded) => encoded,
            Err(error) => {
                self.discard(&staging)?;
                return Err(error);
            }
        };

        if encoded.frames == Frames::ZERO {
            self.discard(&staging)?;
            return Ok(Keeping::Refused(Refusal::Empty));
        }

        self.settled(
            weighing,
            &staging,
            Heard {
                key: encoded.key,
                frames: encoded.frames,
                speakers,
            },
            FLAC_EXTENSION,
            Form::Flac,
            spec,
            codec,
            format,
        )
    }

    fn kept_as_wave(
        &self,
        weighing: Weighing,
        decoder: &mut Decoder,
        spec: StreamSpec,
        speakers: Speakers,
        codec: Codec,
    ) -> Result<Keeping> {
        let format = if spec.format.is_float() {
            SampleFormat::F32
        } else {
            SampleFormat::S32
        };
        decoder.set_output_format(format);
        let spec = StreamSpec::new(spec.rate, spec.channels, format);

        let staging = self.staged(WAVE_EXTENSION)?;
        let written = match wave::write(decoder, spec, speakers, &staging) {
            Ok(written) => written,
            Err(error) => {
                self.discard(&staging)?;
                return Err(error);
            }
        };

        if written.pcm_bytes > wave::LARGEST_PCM {
            self.discard(&staging)?;
            return Ok(Keeping::Refused(Refusal::TooLarge));
        }
        if written.frames == Frames::ZERO {
            self.discard(&staging)?;
            return Ok(Keeping::Refused(Refusal::Empty));
        }

        let target = self.object_path(written.key, &compressed_name(WAVE_EXTENSION));
        if target.is_file() && !weighing.renewing {
            self.discard(&staging)?;
            let bytes = self.sized(&target)?;
            if !weighing.fits(bytes) {
                return Ok(Keeping::Refused(Refusal::NoSmaller));
            }
            return Ok(Keeping::Kept(Kept {
                key: written.key,
                form: Form::Wave,
                bytes,
                was: bytes,
                path: target,
                spec,
                frames: written.frames,
                codec,
                deduped: true,
                replaced: false,
            }));
        }

        let packed = self.staged(&compressed_name(WAVE_EXTENSION))?;
        if wave::compressed(&staging, &packed, weighing.smaller_than)? == wave::Packed::NoSmaller {
            return Ok(Keeping::Refused(Refusal::NoSmaller));
        }

        let went_in = Heard {
            key: written.key,
            frames: written.frames,
            speakers,
        };
        let holds = read_back(&staging, Some(format)).is_ok_and(|read| went_in.held_by(read));
        if !holds {
            return Ok(Keeping::Refused(Refusal::NotValidated));
        }
        self.discard(&staging)?;
        let landing = self.landed_or_standing(&packed, &target, weighing)?;

        Ok(landing.kept(Kept {
            key: written.key,
            form: Form::Wave,
            path: target,
            bytes: 0,
            was: 0,
            spec,
            frames: written.frames,
            codec,
            deduped: false,
            replaced: false,
        }))
    }

    fn kept_whole(
        &self,
        taking: &Taking<'_>,
        codec: Codec,
        spec: StreamSpec,
        duration: Option<Frames>,
    ) -> Result<Keeping> {
        let extension = named_extension(taking.location);
        let staging = self.staged(&extension)?;
        let mut media = taking
            .sources
            .open(taking.location)
            .map_err(|source| Error::codec(VaultOp::Read, source))?;

        let mut file =
            File::create(&staging).map_err(|source| Error::io(VaultOp::Stage, &staging, source))?;
        let mut buffer = vec![0_u8; COPY_BYTES];
        let mut digest = Digest::default();
        let mut held = 0_u64;

        if let Some(head) = bare_flac_head(&mut media.stream, &staging)? {
            digest.note(&head);
            file.write_all(&head)
                .map_err(|source| Error::io(VaultOp::Write, &staging, source))?;
            held += head.len() as u64;
        }

        loop {
            let read = media
                .stream
                .read(&mut buffer)
                .map_err(|source| Error::io(VaultOp::Read, &staging, source))?;
            if read == 0 {
                break;
            }
            digest.note(&buffer[..read]);
            file.write_all(&buffer[..read])
                .map_err(|source| Error::io(VaultOp::Write, &staging, source))?;
            held += read as u64;
        }
        file.sync_all()
            .map_err(|source| Error::io(VaultOp::Settle, &staging, source))?;

        if held == 0 {
            self.discard(&staging)?;
            return Ok(Keeping::Refused(Refusal::Empty));
        }

        let went_in = Decoder::open(taking.sources, taking.location)
            .map_err(|source| Error::codec(VaultOp::Verify, source))
            .and_then(|(decoder, info)| pcm_of(decoder, &info, None));
        let holds = match (went_in, read_back(&staging, None)) {
            (Ok(went_in), Ok(came_out)) => {
                went_in.key == came_out.key && went_in.frames == came_out.frames
            }
            _ => false,
        };
        if !holds {
            self.discard(&staging)?;
            return Ok(Keeping::Refused(Refusal::NotValidated));
        }

        let key = digest.settled();
        let target = self.object_path(key, &extension);
        let landing = self.landed_or_standing(&staging, &target, Weighing::AS_IT_STANDS)?;

        Ok(landing.kept(Kept {
            key,
            form: Form::Kept,
            path: target,
            bytes: 0,
            was: 0,
            spec,
            frames: duration.unwrap_or(Frames::ZERO),
            codec,
            deduped: false,
            replaced: false,
        }))
    }

    #[allow(clippy::too_many_arguments)]
    fn settled(
        &self,
        weighing: Weighing,
        staging: &Path,
        went_in: Heard,
        extension: &str,
        form: Form,
        spec: StreamSpec,
        codec: Codec,
        format: SampleFormat,
    ) -> Result<Keeping> {
        let Heard { key, frames, .. } = went_in;
        let holds = read_back(staging, Some(format)).is_ok_and(|read| went_in.held_by(read));
        if !holds {
            self.discard(staging)?;
            return Ok(Keeping::Refused(Refusal::NotValidated));
        }

        let target = self.object_path(key, extension);
        let landing = self.landed_or_standing(staging, &target, weighing)?;

        Ok(landing.kept(Kept {
            key,
            form,
            path: target,
            bytes: 0,
            was: 0,
            spec,
            frames,
            codec,
            deduped: false,
            replaced: false,
        }))
    }

    fn landed_or_standing(
        &self,
        staging: &Path,
        target: &Path,
        weighing: Weighing,
    ) -> Result<Landing> {
        let _landing = self.landing.lock();
        let staged = self.sized(staging)?;
        let landing = if target.is_file() {
            let standing = self.sized(target)?;
            if weighing.renewing && staged < standing && weighing.fits(staged) {
                Landing::Replaced(self.landed(staging, target)?)
            } else if weighing.fits(standing) {
                Landing::Deduped(standing)
            } else {
                Landing::NoSmaller
            }
        } else if weighing.fits(staged) {
            Landing::Landed(self.landed(staging, target)?)
        } else {
            Landing::NoSmaller
        };
        if !matches!(landing, Landing::Landed(_) | Landing::Replaced(_)) {
            self.discard(staging)?;
        }
        Ok(landing)
    }

    fn object_path(&self, key: VaultKey, extension: &str) -> PathBuf {
        self.root
            .join(AUDIO)
            .join(key.fanout())
            .join(format!("{key}.{extension}"))
    }

    fn cover_path(&self, key: VaultKey) -> PathBuf {
        self.root
            .join(COVERS)
            .join(key.fanout())
            .join(format!("{key}.{COVER_EXTENSION}"))
    }

    fn staged(&self, extension: &str) -> Result<Staged> {
        let nth = STAGED.fetch_add(1, Ordering::Relaxed);
        Ok(Staged(
            self.root
                .join(STAGING)
                .join(format!("{}-{nth}.{extension}", process::id())),
        ))
    }

    fn landed(&self, staging: &Path, target: &Path) -> Result<u64> {
        self.inside(staging)?;
        self.inside(target)?;
        if let Some(folder) = target.parent() {
            fs::create_dir_all(folder)
                .map_err(|source| Error::io(VaultOp::MakeFolder, folder, source))?;
        }
        fs::rename(staging, target).map_err(|source| Error::io(VaultOp::Settle, target, source))?;
        self.sized(target)
    }

    fn discard(&self, path: &Path) -> Result<()> {
        self.forget(path).map(|_| ())
    }

    fn inside(&self, path: &Path) -> Result<()> {
        if self.holds(path) {
            Ok(())
        } else {
            Err(Error::OutsideTheVault {
                path: path.to_path_buf(),
            })
        }
    }

    fn sized(&self, path: &Path) -> Result<u64> {
        fs::metadata(path)
            .map(|held| held.len())
            .map_err(|source| Error::io(VaultOp::Read, path, source))
    }

    fn read_inside(&self, path: &Path) -> Result<Vec<u8>> {
        self.inside(path)?;
        fs::read(path).map_err(|source| Error::io(VaultOp::Read, path, source))
    }

    fn written_inside(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        self.inside(path)?;
        if let Some(folder) = path.parent() {
            fs::create_dir_all(folder)
                .map_err(|source| Error::io(VaultOp::MakeFolder, folder, source))?;
        }
        let mut file =
            File::create(path).map_err(|source| Error::io(VaultOp::Stage, path, source))?;
        file.write_all(bytes)
            .map_err(|source| Error::io(VaultOp::Write, path, source))?;
        file.sync_all()
            .map_err(|source| Error::io(VaultOp::Settle, path, source))
    }

    fn walked(&self, folder: &Path, into: &mut Vec<PathBuf>) -> Result<()> {
        let reading = match fs::read_dir(folder) {
            Ok(reading) => reading,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(source) => return Err(Error::io(VaultOp::Walk, folder, source)),
        };

        for entry in reading {
            let entry = entry.map_err(|source| Error::io(VaultOp::Walk, folder, source))?;
            let path = entry.path();
            if path.is_dir() {
                self.walked(&path, into)?;
            } else if named(&path).is_some() {
                into.push(path);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Heard {
    key: VaultKey,
    frames: Frames,
    speakers: Speakers,
}

impl Heard {
    fn held_by(self, read: Self) -> bool {
        self.key == read.key && self.frames == read.frames && self.speakers.held_by(read.speakers)
    }
}

fn read_back(path: &Path, format: Option<SampleFormat>) -> Result<Heard> {
    let sources = Sources::local();
    let location = MediaLocation::local(path);
    let (decoder, info) = Decoder::open(&sources, &location)
        .map_err(|source| Error::codec(VaultOp::Verify, source))?;
    pcm_of(decoder, &info, format)
}

fn pcm_of(mut decoder: Decoder, info: &MediaInfo, format: Option<SampleFormat>) -> Result<Heard> {
    let format = format.unwrap_or_else(|| {
        let bits = info
            .bits_per_coded_sample
            .unwrap_or_else(|| info.spec.format.valid_bits());
        if info.spec.format.is_float() {
            SampleFormat::F32
        } else if bits <= SIXTEEN_BIT_CEILING {
            SampleFormat::S16
        } else if bits <= WIDEST_FLAC_BITS {
            SampleFormat::S24
        } else {
            SampleFormat::S32
        }
    });
    decoder.set_output_format(format);

    let spec = StreamSpec::new(info.spec.rate, info.spec.channels, format);
    let mut block = AudioBuffer::empty(spec);
    let mut bytes = Vec::new();
    let mut digest = Digest::default();
    let mut frames = 0_u64;

    loop {
        let status = decoder
            .next_block(&mut block)
            .map_err(|source| Error::codec(VaultOp::Verify, source))?;
        if status == DecodeStatus::EndOfStream {
            break;
        }
        pcm::little_endian(&block, &mut bytes);
        digest.note(&bytes);
        frames += block.frames() as u64;
    }

    Ok(Heard {
        key: digest.settled(),
        frames: Frames(frames),
        speakers: info.speakers,
    })
}

fn sized_source(path: &Path) -> Option<u64> {
    fs::metadata(path).ok().map(|held| held.len())
}

fn share_of(taking: &Taking<'_>, span: FrameSpan) -> Option<u64> {
    let size = taking.location.as_path().and_then(sized_source)?;
    let whole = probe(taking.sources, taking.location).ok()?.duration?;
    let held = span.within(whole).frames()?;
    if whole == Frames::ZERO {
        return None;
    }
    let share = u128::from(size) * u128::from(held.get()) / u128::from(whole.get());
    u64::try_from(share).ok()
}

impl Landing {
    fn kept(self, landed: Kept) -> Keeping {
        let (bytes, deduped, replaced) = match self {
            Self::Landed(bytes) => (bytes, false, false),
            Self::Deduped(bytes) => (bytes, true, false),
            Self::Replaced(bytes) => (bytes, false, true),
            Self::NoSmaller => return Keeping::Refused(Refusal::NoSmaller),
        };
        Keeping::Kept(Kept {
            bytes,
            was: bytes,
            deduped,
            replaced,
            ..landed
        })
    }
}

fn weighed(kept: Keeping, was: Option<u64>) -> Keeping {
    match kept {
        Keeping::Kept(kept) => Keeping::Kept(Kept {
            was: was.unwrap_or(kept.bytes),
            ..kept
        }),
        refused @ Keeping::Refused(_) => refused,
    }
}

fn bare_flac_head(stream: &mut Box<dyn MediaStream>, named: &Path) -> Result<Option<Vec<u8>>> {
    if !stream.is_seekable() {
        return Ok(None);
    }

    let mut magic = [0_u8; FLAC_MAGIC.len()];
    let read = stream
        .read(&mut magic)
        .map_err(|source| Error::io(VaultOp::Read, named, source))?;
    if read != magic.len() || magic != FLAC_MAGIC {
        stream
            .seek(SeekFrom::Start(0))
            .map_err(|source| Error::io(VaultOp::Read, named, source))?;
        return Ok(None);
    }

    let mut stream_info = None;
    let mut walked = 0_u64;
    let mut reached_the_audio = false;

    loop {
        let mut header = [0_u8; METADATA_HEADER_BYTES];
        if stream.read_exact(&mut header).is_err() {
            break;
        }
        let last = header[0] & LAST_BLOCK != 0;
        let kind = header[0] & BLOCK_KIND;
        let length = u32::from(header[1]) << 16 | u32::from(header[2]) << 8 | u32::from(header[3]);

        walked += u64::from(length);
        if walked > MOST_METADATA_BYTES {
            break;
        }

        if kind == STREAM_INFO {
            let mut held = vec![0_u8; length as usize];
            if stream.read_exact(&mut held).is_err() {
                break;
            }
            stream_info = Some(held);
        } else if stream.seek(SeekFrom::Current(i64::from(length))).is_err() {
            break;
        }

        if last {
            reached_the_audio = true;
            break;
        }
    }

    let Some(held) = stream_info.filter(|_| reached_the_audio) else {
        stream
            .seek(SeekFrom::Start(0))
            .map_err(|source| Error::io(VaultOp::Read, named, source))?;
        return Ok(None);
    };

    let length = held.len() as u32;
    let mut head = Vec::with_capacity(FLAC_MAGIC.len() + METADATA_HEADER_BYTES + held.len());
    head.extend_from_slice(&FLAC_MAGIC);
    head.push(LAST_BLOCK | STREAM_INFO);
    head.push((length >> 16) as u8);
    head.push((length >> 8) as u8);
    head.push(length as u8);
    head.extend_from_slice(&held);
    Ok(Some(head))
}

fn names_a_place_inside(within: &Path) -> bool {
    within.components().next().is_some()
        && within
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn named(path: &Path) -> Option<VaultKey> {
    let name = path.file_name()?.to_str()?;
    let stem = name.split('.').next()?;
    VaultKey::read(stem).ok()
}

fn named_extension(location: &MediaLocation) -> String {
    sanitised_extension(location.extension())
}

fn sanitised_extension(extension: Option<&str>) -> String {
    let named: String = extension
        .unwrap_or(UNNAMED_EXTENSION)
        .chars()
        .filter(|letter| letter.is_ascii_alphanumeric())
        .take(LONGEST_EXTENSION)
        .collect::<String>()
        .to_ascii_lowercase();

    if named.is_empty() {
        UNNAMED_EXTENSION.to_owned()
    } else {
        named
    }
}

fn compressed_name(extension: &str) -> String {
    format!("{extension}.{COMPRESSED_EXTENSION}")
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use resonate_codec::Reading;

    use super::*;

    fn block(kind: u8, last: bool, payload: &[u8]) -> Vec<u8> {
        let length = payload.len() as u32;
        let mut held = vec![
            kind | if last { LAST_BLOCK } else { 0 },
            (length >> 16) as u8,
            (length >> 8) as u8,
            length as u8,
        ];
        held.extend_from_slice(payload);
        held
    }

    fn streamed(bytes: Vec<u8>) -> Box<dyn MediaStream> {
        Box::new(Reading::new(Cursor::new(bytes)))
    }

    #[test]
    fn a_head_that_never_reaches_its_last_block_is_copied_whole_rather_than_cut_short() {
        const PADDING: u8 = 1;

        let mut whole = FLAC_MAGIC.to_vec();
        whole.extend(block(STREAM_INFO, false, &[7_u8; 34]));
        whole.extend(block(PADDING, false, &[0_u8; 16]));

        let mut stream = streamed(whole.clone());
        let head =
            bare_flac_head(&mut stream, Path::new("truncated.flac")).expect("a readable head");
        assert_eq!(
            head, None,
            "a STREAMINFO was marked last ahead of blocks it did not walk"
        );

        let mut rest = Vec::new();
        stream.read_to_end(&mut rest).expect("the whole file");
        assert_eq!(rest, whole, "the file was not handed back from its start");
    }

    #[test]
    fn a_flac_head_is_rewritten_as_its_stream_info_alone() {
        const VORBIS_COMMENT: u8 = 4;
        const PICTURE: u8 = 6;

        let stream_info = vec![7_u8; 34];
        let mut whole = FLAC_MAGIC.to_vec();
        whole.extend(block(STREAM_INFO, false, &stream_info));
        whole.extend(block(VORBIS_COMMENT, false, b"a title nobody asked for"));
        whole.extend(block(PICTURE, true, &vec![9_u8; 4096]));
        whole.extend_from_slice(b"the frames that follow");

        let mut stream = streamed(whole);
        let head = bare_flac_head(&mut stream, Path::new("noisy.flac"))
            .expect("a readable head")
            .expect("a FLAC head");

        let mut wanted = FLAC_MAGIC.to_vec();
        wanted.extend(block(STREAM_INFO, true, &stream_info));
        assert_eq!(head, wanted);

        let mut rest = Vec::new();
        stream.read_to_end(&mut rest).expect("the frames");
        assert_eq!(rest, b"the frames that follow");
    }

    #[test]
    fn a_delivery_past_the_cap_is_refused_and_leaves_nothing_in_staging() {
        let root = std::env::temp_dir().join(format!("resonate-vault-unit-{}", process::id()));
        let vault = Vault::open(&root).expect("a vault");
        let mut delivery = Cursor::new(vec![0_u8; 65]);

        let kept = vault
            .keep_delivered_under(&mut delivery, "wav", 64)
            .expect("a refusal rather than a failure");

        assert_eq!(kept, Keeping::Refused(Refusal::TooLarge));
        assert_eq!(
            fs::read_dir(root.join(STAGING))
                .expect("the staging folder")
                .count(),
            0
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_staging_file_is_taken_away_whatever_came_of_the_write() {
        let root = std::env::temp_dir().join(format!("resonate-vault-staging-{}", process::id()));
        let vault = Vault::open(&root).expect("a vault");

        let failed: Result<()> = (|| {
            let staging = vault.staged(FLAC_EXTENSION)?;
            fs::write(&staging, b"half an object").expect("a staged write");
            Err(Error::io(
                VaultOp::Write,
                &staging,
                std::io::Error::other("the disc filled"),
            ))
        })();

        assert!(failed.is_err());
        assert_eq!(
            fs::read_dir(vault.root().join(STAGING))
                .expect("the staging folder")
                .count(),
            0
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_path_within_the_vault_never_names_one_outside_it() {
        let root = std::env::temp_dir().join(format!("resonate-vault-within-{}", process::id()));
        let vault = Vault::open(&root).expect("a vault");

        assert!(vault.at(Path::new("../elsewhere")).is_err());
        assert!(vault.at(Path::new("/etc/passwd")).is_err());
        assert!(vault.at(Path::new("")).is_err());
        let inside = vault
            .at(Path::new("covers/ab/ab.jxl"))
            .expect("a place inside");
        assert_eq!(
            vault.within(&inside).expect("a place inside"),
            Path::new("covers/ab/ab.jxl")
        );
        assert!(vault.within(Path::new("/elsewhere/covers/ab.jxl")).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_extension_that_could_name_a_path_is_written_as_its_letters_alone() {
        assert_eq!(sanitised_extension(Some("../FLAC")), "flac");
        assert_eq!(sanitised_extension(Some("/")), UNNAMED_EXTENSION);
        assert_eq!(sanitised_extension(None), UNNAMED_EXTENSION);
    }

    #[test]
    fn a_stream_that_is_not_a_flac_is_left_where_it_was_found() {
        let mut stream = streamed(b"RIFF....WAVEfmt ".to_vec());
        assert!(
            bare_flac_head(&mut stream, Path::new("not.wav"))
                .expect("a readable head")
                .is_none()
        );

        let mut rest = Vec::new();
        stream.read_to_end(&mut rest).expect("the whole file");
        assert_eq!(rest, b"RIFF....WAVEfmt ");
    }
}
