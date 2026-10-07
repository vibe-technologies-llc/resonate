use std::{
    fs::{self, File},
    io::{self, Read, Seek, Write},
    ops::Deref,
    path::{Component, Path, PathBuf},
    process,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use parking_lot::Mutex;
use resonate_codec::{
    Codec, Container, CoverArt, DecodeStatus, Decoder, ImageFormat, MediaInfo, Sources, Speakers,
    TagSet, probe,
};
use resonate_core::{AudioBuffer, FrameSpan, Frames, MediaLocation, SampleFormat, StreamSpec};

use crate::{
    bare, blanks, chunks, cover,
    drawn::Drawings,
    error::{Error, Result, VaultOp},
    files::VaultFiles,
    flac,
    form::{Form, WIDEST_FLAC_BITS},
    key::VaultKey,
    ogg,
    pcm::{self, Digest},
    wave,
};

pub(crate) const AUDIO: &str = "audio";
pub(crate) const COVERS: &str = "covers";
pub(crate) const STAGING: &str = "staging";
pub(crate) const COVER_EXTENSION: &str = "jxl";
const PICTURE_EXTENSIONS: [&str; 6] = [COVER_EXTENSION, "jpg", "png", "webp", "gif", "bmp"];
const AS_IT_CAME: [ImageFormat; 5] = [
    ImageFormat::Jpeg,
    ImageFormat::Png,
    ImageFormat::Webp,
    ImageFormat::Gif,
    ImageFormat::Bmp,
];
pub(crate) const COMPRESSED_EXTENSION: &str = "zst";
const FLAC_EXTENSION: &str = "flac";
const WAVE_EXTENSION: &str = "wav";
const UNNAMED_EXTENSION: &str = "bin";
const LONGEST_EXTENSION: usize = 8;
const COPY_BYTES: usize = 1 << 20;
const SIXTEEN_BIT_CEILING: u8 = 16;
const LARGEST_DELIVERY: u64 = wave::LARGEST_PCM;
const PROCESSES: &str = "/proc";

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
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
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
    pub foretold: Option<VaultKey>,
    pub halt: Halt<'a>,
}

#[derive(Clone, Copy, Debug)]
struct Encoding {
    spec: StreamSpec,
    speakers: Speakers,
    bits: u8,
    codec: Codec,
}

#[derive(Clone, Copy, Debug)]
pub struct Halt<'a>(Option<&'a AtomicBool>);

impl<'a> Halt<'a> {
    pub const NEVER: Self = Self(None);

    pub const fn on(asked: &'a AtomicBool) -> Self {
        Self(Some(asked))
    }

    pub(crate) fn heard(self) -> Result<()> {
        match self.0.is_some_and(|asked| asked.load(Ordering::Relaxed)) {
            true => Err(Error::Halted),
            false => Ok(()),
        }
    }
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
    pub declared: Box<TagSet>,
    pub bits: u8,
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

enum Foretold {
    Settled(Keeping),
    Unforetold,
    Misdeclared,
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
pub struct HeldFile {
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
    pub fn make(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)
            .map_err(|source| Error::io(VaultOp::MakeFolder, &root, source))?;
        Self::open(root)
    }

    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        if !root.is_dir() {
            return Err(Error::NotThere { path: root });
        }
        for folder in [AUDIO, COVERS, STAGING] {
            let made = root.join(folder);
            fs::create_dir_all(&made)
                .map_err(|source| Error::io(VaultOp::MakeFolder, &made, source))?;
        }
        let root = root
            .canonicalize()
            .map_err(|source| Error::io(VaultOp::Resolve, &root, source))?;
        let vault = Self {
            root,
            drawings: Drawings::default(),
            landing: Mutex::new(()),
        };
        if let Err(error) = vault.swept(Sweeping::WhatCrashed, Sweep::Taking) {
            tracing::warn!(%error, "what a crashed import left in the vault's staging could not be swept");
        }
        Ok(vault)
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
        self.within(path).is_ok()
    }

    pub fn failed_itself(&self, error: &Error) -> bool {
        match error {
            Error::Io { path, source, .. } => {
                path.starts_with(&self.root)
                    || matches!(
                        source.kind(),
                        io::ErrorKind::StorageFull
                            | io::ErrorKind::QuotaExceeded
                            | io::ErrorKind::ReadOnlyFilesystem
                    )
            }
            Error::OutsideTheVault { .. } | Error::NotThere { .. } | Error::ObjectGone { .. } => {
                true
            }
            Error::Source { .. }
            | Error::NotAKey
            | Error::Halted
            | Error::Codec { .. }
            | Error::Unencodable { .. }
            | Error::Encoding { .. }
            | Error::Written { .. }
            | Error::Picture { .. }
            | Error::PictureWritten { .. }
            | Error::PictureUnsized { .. }
            | Error::PictureUnconfirmed { .. }
            | Error::PictureRead { .. }
            | Error::Domain(_) => false,
        }
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
                        foretold: None,
                        halt: Halt::NEVER,
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
        decoder.refuse_holes();

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
        let outgrows_a_wave = whole
            && info
                .duration
                .is_some_and(|frames| wave::outgrows_a_wave(frames, info.spec.channel_count()));
        let kept = match form {
            Form::Kept if !whole => return Ok(Keeping::Refused(Refusal::CutFromAnother)),
            Form::Kept => self.kept_whole(taking, codec, &info, Some(decoder))?,
            Form::Wave if outgrows_a_wave => Keeping::Refused(Refusal::TooLarge),
            Form::Wave => self.kept_as_wave(
                weighing,
                &mut decoder,
                info.spec,
                speakers,
                codec,
                taking.halt,
            )?,
            Form::Flac => match self.foretold_flac(taking, weighing, &mut decoder, &info, bits)? {
                Foretold::Settled(kept) => kept,
                Foretold::Unforetold => self.kept_as_flac(
                    weighing,
                    &mut decoder,
                    Encoding {
                        spec: info.spec,
                        speakers,
                        bits,
                        codec,
                    },
                    taking.halt,
                )?,
                Foretold::Misdeclared => {
                    let (mut again, _) = Decoder::open(taking.sources, taking.location)
                        .map_err(|source| Error::codec(VaultOp::Read, source))?;
                    again.refuse_holes();
                    self.kept_as_flac(
                        weighing,
                        &mut again,
                        Encoding {
                            spec: info.spec,
                            speakers,
                            bits,
                            codec,
                        },
                        taking.halt,
                    )?
                }
            },
        };

        let kept = match kept {
            Keeping::Refused(Refusal::NoSmaller | Refusal::TooLarge) if whole => {
                self.kept_whole(taking, codec, &info, None)?
            }
            kept => kept,
        };
        Ok(weighed(kept, held, Box::new(info.tags), bits))
    }

    pub fn keep_cover(&self, art: &CoverArt) -> Result<KeptCover> {
        let mut digest = Digest::default();
        digest.note(&art.bytes);
        let key = digest.settled();

        if let Some(target) = self.held_cover(key) {
            let (width, height) =
                cover::size_of(&self.read_inside(&target)?, kept_as_it_came(&target))?;
            return Ok(KeptCover {
                key,
                bytes: self.sized(&target)?,
                width,
                height,
                path: target,
                deduped: true,
            });
        }

        let drawn = cover::as_jxl(art)?;
        if art.bytes.len() <= drawn.bytes.len() {
            return self.kept_as_it_came(key, art, &drawn);
        }
        let target = self.cover_path(key, COVER_EXTENSION);
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

    fn kept_as_it_came(
        &self,
        key: VaultKey,
        art: &CoverArt,
        drawn: &cover::Drawn,
    ) -> Result<KeptCover> {
        let extension = art.format.extension();
        let target = self.cover_path(key, extension);
        let staging = self.staged(extension)?;
        self.written_inside(&staging, &art.bytes)?;
        if self.read_inside(&staging)? != art.bytes {
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

    fn held_cover(&self, key: VaultKey) -> Option<PathBuf> {
        PICTURE_EXTENSIONS
            .into_iter()
            .map(|extension| self.cover_path(key, extension))
            .find(|path| path.is_file())
    }

    pub fn picture(&self, path: &Path) -> Result<CoverArt> {
        self.inside(path)?;
        if let Some(format) = kept_as_it_came(path) {
            return Ok(CoverArt {
                format,
                bytes: self.read_inside(path)?,
            });
        }
        let key = named(path);
        if let Some(drawn) = key.and_then(|key| self.drawings.drawn(key)) {
            return Ok(drawn);
        }

        let drawn_after = self.drawings.forgetting();
        let drawn = cover::drawable(&self.read_inside(path)?)?;
        if let Some(key) = key {
            self.drawings.note(key, &drawn, drawn_after);
        }
        Ok(drawn)
    }

    pub fn verify(&self, path: &Path, form: Form) -> Result<bool> {
        self.inside(path)?;
        if !path.is_file() {
            return Err(Error::ObjectGone {
                path: path.to_owned(),
            });
        }
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
            Form::Wave => Ok(self.read_back_packed(path, None)?.key == key),
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

    pub fn covers(&self) -> Result<Vec<HeldFile>> {
        let mut held = Vec::new();
        self.walked(&self.root.join(COVERS), &mut held)?;
        held.sort();
        Ok(held
            .into_iter()
            .filter(|path| {
                path.extension()
                    .and_then(|held| held.to_str())
                    .is_some_and(|held| PICTURE_EXTENSIONS.contains(&held))
            })
            .filter_map(|path| named(&path).map(|key| HeldFile { key, path }))
            .collect())
    }

    pub fn objects(&self) -> Result<Vec<HeldFile>> {
        let mut held = Vec::new();
        self.walked(&self.root.join(AUDIO), &mut held)?;
        held.sort();
        Ok(held
            .into_iter()
            .filter_map(|path| named(&path).map(|key| HeldFile { key, path }))
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
        self.swept(Sweeping::AllButTheLiving, Sweep::Taking)
    }

    pub fn staging_a_sweep_would_take(&self) -> Result<u64> {
        self.swept(Sweeping::AllButTheLiving, Sweep::Counting)
    }

    fn swept(&self, sweeping: Sweeping, sweep: Sweep) -> Result<u64> {
        let folder = self.root.join(STAGING);
        let mut swept = 0;
        let reading =
            fs::read_dir(&folder).map_err(|source| Error::io(VaultOp::Walk, &folder, source))?;
        for entry in reading {
            let entry = entry.map_err(|source| Error::io(VaultOp::Walk, &folder, source))?;
            let path = entry.path();
            if !sweeping.takes(staged_by(&path)) {
                continue;
            }
            let taken = match sweep {
                Sweep::Taking => self.forget(&path)?,
                Sweep::Counting => true,
            };
            if taken {
                swept += 1;
            }
        }
        Ok(swept)
    }

    fn foretold_flac(
        &self,
        taking: &Taking<'_>,
        weighing: Weighing,
        decoder: &mut Decoder,
        info: &MediaInfo,
        bits: u8,
    ) -> Result<Foretold> {
        let (format, stored) = flac_depth(bits);
        if weighing.renewing || taking.span.is_some() {
            return Ok(Foretold::Unforetold);
        }

        let declares = bits == stored && Container::from_id(info.container) == Container::Flac;
        let declared = if declares {
            let mut media = taking
                .sources
                .open(taking.location)
                .map_err(|source| Error::codec(VaultOp::Read, source))?;
            bare::declared_digest(&mut media.stream)
        } else {
            None
        };
        let Some(declared) = declared.or(taking.foretold) else {
            return Ok(Foretold::Unforetold);
        };
        let target = self.object_path(declared, FLAC_EXTENSION);
        let Some(standing) = self.standing(&target)? else {
            return Ok(Foretold::Unforetold);
        };

        let heard = pcm_of(decoder, info, Some(format))?;
        if heard.key != declared {
            return Ok(Foretold::Misdeclared);
        }
        if !weighing.fits(standing) {
            return Ok(Foretold::Settled(Keeping::Refused(Refusal::NoSmaller)));
        }
        Ok(Foretold::Settled(Landing::Deduped(standing).kept(Kept {
            key: declared,
            form: Form::Flac,
            path: target,
            bytes: 0,
            was: 0,
            spec: StreamSpec::new(info.spec.rate, info.spec.channels, format),
            frames: heard.frames,
            codec: Codec::from_id(info.codec),
            deduped: false,
            replaced: false,
            declared: Box::default(),
            bits: 0,
        })))
    }

    fn kept_as_flac(
        &self,
        weighing: Weighing,
        decoder: &mut Decoder,
        encoding: Encoding,
        halt: Halt<'_>,
    ) -> Result<Keeping> {
        let Encoding {
            spec,
            speakers,
            bits,
            codec,
        } = encoding;
        let (format, stored) = flac_depth(bits);
        decoder.set_output_format(format);
        let spec = StreamSpec::new(spec.rate, spec.channels, format);

        let staging = self.staged(FLAC_EXTENSION)?;
        let encoded =
            match flac::encode(decoder, spec, stored, &staging, weighing.smaller_than, halt) {
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
        let target = self.object_path(encoded.key, FLAC_EXTENSION);
        if encoded.outgrew || (!weighing.renewing && target.is_file()) {
            self.discard(&staging)?;
            let Some(standing) = self.standing(&target)? else {
                return Ok(Keeping::Refused(Refusal::NoSmaller));
            };
            if !weighing.fits(standing) {
                return Ok(Keeping::Refused(Refusal::NoSmaller));
            }
            return Ok(Landing::Deduped(standing).kept(Kept {
                key: encoded.key,
                form: Form::Flac,
                path: target,
                bytes: 0,
                was: 0,
                spec,
                frames: encoded.frames,
                codec,
                deduped: false,
                replaced: false,
                declared: Box::default(),
                bits: 0,
            }));
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
        halt: Halt<'_>,
    ) -> Result<Keeping> {
        let format = if spec.format.is_float() {
            SampleFormat::F32
        } else {
            SampleFormat::S32
        };
        decoder.set_output_format(format);
        let spec = StreamSpec::new(spec.rate, spec.channels, format);

        let staging = self.staged(WAVE_EXTENSION)?;
        let written = match wave::write(decoder, spec, speakers, &staging, halt) {
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
                declared: Box::default(),
                bits: 0,
            }));
        }

        if let Some(ceiling) = weighing.smaller_than
            && wave::hopeless(&staging, ceiling)?
        {
            return Ok(Keeping::Refused(Refusal::NoSmaller));
        }
        let packed = self.staged(&compressed_name(WAVE_EXTENSION))?;
        if wave::compressed(&staging, &packed, weighing.smaller_than, halt)?
            == wave::Packed::NoSmaller
        {
            return Ok(Keeping::Refused(Refusal::NoSmaller));
        }

        self.discard(&staging)?;
        let went_in = Heard {
            key: written.key,
            frames: written.frames,
            speakers,
        };
        let holds = self
            .read_back_packed(&packed, Some(format))
            .is_ok_and(|read| went_in.held_by(read));
        if !holds {
            return Ok(Keeping::Refused(Refusal::NotValidated));
        }
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
            declared: Box::default(),
            bits: 0,
        }))
    }

    fn kept_whole(
        &self,
        taking: &Taking<'_>,
        codec: Codec,
        info: &MediaInfo,
        unread: Option<Decoder>,
    ) -> Result<Keeping> {
        let went_in = match unread {
            Some(mut decoder) => pcm_of(&mut decoder, info, None),
            None => Decoder::open(taking.sources, taking.location)
                .map_err(|source| Error::codec(VaultOp::Verify, source))
                .and_then(|(mut decoder, info)| pcm_of(&mut decoder, &info, None)),
        };
        let went_in = match went_in {
            Ok(went_in) => went_in,
            Err(Error::Codec { source, op }) if source.is_out_of_reach() => {
                return Err(Error::Codec { op, source });
            }
            Err(_) => return Ok(Keeping::Refused(Refusal::NotValidated)),
        };

        let container = Container::from_id(info.container);
        let mut copied = self.copied(taking, Stripping::Bare(container))?;
        if copied
            .as_ref()
            .is_some_and(|copied| copied.stripped && !copied.holds_what(went_in))
        {
            copied = self.copied(taking, Stripping::Whole)?;
        }
        let Some(copied) = copied else {
            return Ok(Keeping::Refused(Refusal::Empty));
        };
        if !copied.holds_what(went_in) {
            return Ok(Keeping::Refused(Refusal::NotValidated));
        }

        let extension = named_extension(taking.location);
        let key = copied.key;
        let target = self.object_path(key, &extension);
        let landing = self.landed_or_standing(&copied.staging, &target, Weighing::AS_IT_STANDS)?;

        Ok(landing.kept(Kept {
            key,
            form: Form::Kept,
            path: target,
            bytes: 0,
            was: 0,
            spec: info.spec,
            frames: went_in.frames,
            codec,
            deduped: false,
            replaced: false,
            declared: Box::default(),
            bits: 0,
        }))
    }

    fn copied(&self, taking: &Taking<'_>, stripping: Stripping) -> Result<Option<Copied>> {
        let staging = self.staged(&named_extension(taking.location))?;
        let mut media = taking
            .sources
            .open(taking.location)
            .map_err(|source| Error::codec(VaultOp::Read, source))?;

        let mut file =
            File::create(&staging).map_err(|source| Error::io(VaultOp::Stage, &staging, source))?;
        let mut digest = Digest::default();
        let mut held = 0_u64;

        let bared = match stripping {
            Stripping::Bare(container) => {
                bare::bare(&mut media.stream, container, taking.location)?
            }
            Stripping::Whole => None,
        };
        let stripped = bared.is_some();
        let (head, until, renumbering, left_out, blanks) =
            bared.map_or((Vec::new(), None, None, Vec::new(), Vec::new()), |bared| {
                (
                    bared.head,
                    bared.until,
                    bared.renumbering,
                    bared.left_out,
                    bared.blanks,
                )
            });
        let start = media
            .stream
            .stream_position()
            .map_err(|source| Error::source(taking.location, source))?;
        if !head.is_empty() {
            digest.note(&head);
            file.write_all(&head)
                .map_err(|source| Error::io(VaultOp::Write, &staging, source))?;
            held += head.len() as u64;
        }

        let mut rest: Box<dyn Read> = match until {
            Some(until) => Box::new((&mut media.stream).take(until.saturating_sub(start))),
            None if !left_out.is_empty() => {
                Box::new(chunks::Passing::over(&mut media.stream, start, left_out))
            }
            None => Box::new(&mut media.stream),
        };
        if let Some(renumbering) = renumbering {
            rest = Box::new(ogg::Renumbered::over(rest, renumbering));
        }
        if !blanks.is_empty() {
            rest = Box::new(blanks::Blanked::over(rest, start, blanks));
        }
        let mut buffer = vec![0_u8; COPY_BYTES];
        loop {
            let read = rest
                .read(&mut buffer)
                .map_err(|source| Error::source(taking.location, source))?;
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
            return Ok(None);
        }
        let came_out = read_back(&staging, None).ok();
        Ok(Some(Copied {
            staging,
            key: digest.settled(),
            came_out,
            stripped,
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
            declared: Box::default(),
            bits: 0,
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

    fn standing(&self, target: &Path) -> Result<Option<u64>> {
        let _landing = self.landing.lock();
        match target.is_file() {
            true => self.sized(target).map(Some),
            false => Ok(None),
        }
    }

    fn read_back_packed(&self, path: &Path, format: Option<SampleFormat>) -> Result<Heard> {
        let sources = Sources::local().and(Arc::new(VaultFiles::over(self)));
        read_back_through(&sources, path, format)
    }

    fn object_path(&self, key: VaultKey, extension: &str) -> PathBuf {
        self.root
            .join(AUDIO)
            .join(key.fanout())
            .join(format!("{key}.{extension}"))
    }

    fn cover_path(&self, key: VaultKey, extension: &str) -> PathBuf {
        self.root
            .join(COVERS)
            .join(key.fanout())
            .join(format!("{key}.{extension}"))
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
        let folder = target.parent().ok_or_else(|| Error::OutsideTheVault {
            path: target.to_path_buf(),
        })?;
        let made = !folder.is_dir();
        fs::create_dir_all(folder)
            .map_err(|source| Error::io(VaultOp::MakeFolder, folder, source))?;
        fs::rename(staging, target).map_err(|source| Error::io(VaultOp::Settle, target, source))?;
        settled_folder(folder)?;
        if made && let Some(above) = folder.parent() {
            settled_folder(above)?;
        }
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
enum Sweep {
    Taking,
    Counting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sweeping {
    WhatCrashed,
    AllButTheLiving,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StagedBy {
    Living,
    Gone,
    Unnamed,
}

impl Sweeping {
    const fn takes(self, staged: StagedBy) -> bool {
        match (self, staged) {
            (_, StagedBy::Living) | (Self::WhatCrashed, StagedBy::Unnamed) => false,
            (_, StagedBy::Gone) | (Self::AllButTheLiving, StagedBy::Unnamed) => true,
        }
    }
}

fn staged_by(path: &Path) -> StagedBy {
    let pid = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.split_once('-'))
        .and_then(|(pid, _)| pid.parse::<u32>().ok());
    match pid {
        None => StagedBy::Unnamed,
        Some(pid) if pid == process::id() || !Path::new(PROCESSES).is_dir() => StagedBy::Living,
        Some(pid) if Path::new(PROCESSES).join(pid.to_string()).exists() => StagedBy::Living,
        Some(_) => StagedBy::Gone,
    }
}

fn settled_folder(folder: &Path) -> Result<()> {
    File::open(folder)
        .and_then(|opened| opened.sync_all())
        .map_err(|source| Error::io(VaultOp::Settle, folder, source))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stripping {
    Bare(Container),
    Whole,
}

struct Copied {
    staging: Staged,
    key: VaultKey,
    came_out: Option<Heard>,
    stripped: bool,
}

impl Copied {
    fn holds_what(&self, went_in: Heard) -> bool {
        self.came_out.is_some_and(|came_out| {
            went_in.key == came_out.key && went_in.frames == came_out.frames
        })
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
    read_back_through(&Sources::local(), path, format)
}

fn read_back_through(
    sources: &Sources,
    path: &Path,
    format: Option<SampleFormat>,
) -> Result<Heard> {
    let location = MediaLocation::local(path);
    let (mut decoder, info) = Decoder::open(sources, &location)
        .map_err(|source| Error::codec(VaultOp::Verify, source))?;
    pcm_of(&mut decoder, &info, format)
}

const fn flac_depth(bits: u8) -> (SampleFormat, u8) {
    if bits <= SIXTEEN_BIT_CEILING {
        (SampleFormat::S16, SIXTEEN_BIT_CEILING)
    } else {
        (SampleFormat::S24, WIDEST_FLAC_BITS)
    }
}

fn pcm_of(decoder: &mut Decoder, info: &MediaInfo, format: Option<SampleFormat>) -> Result<Heard> {
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
    decoder.refuse_holes();

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

fn weighed(kept: Keeping, was: Option<u64>, declared: Box<TagSet>, bits: u8) -> Keeping {
    match kept {
        Keeping::Kept(kept) => Keeping::Kept(Kept {
            was: was.unwrap_or(kept.bytes),
            declared,
            bits,
            ..kept
        }),
        refused @ Keeping::Refused(_) => refused,
    }
}

fn kept_as_it_came(path: &Path) -> Option<ImageFormat> {
    let extension = path.extension()?.to_str()?;
    AS_IT_CAME
        .into_iter()
        .find(|format| format.extension() == extension)
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

    use super::*;

    #[test]
    fn a_delivery_past_the_cap_is_refused_and_leaves_nothing_in_staging() {
        let root = std::env::temp_dir().join(format!("resonate-vault-unit-{}", process::id()));
        let vault = Vault::make(&root).expect("a vault");
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
        let vault = Vault::make(&root).expect("a vault");

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
    fn a_crashed_imports_staging_is_swept_when_the_vault_opens_and_a_living_ones_is_not() {
        const PAST_EVERY_PID: u32 = (1 << 22) + 1;

        let root = std::env::temp_dir().join(format!("resonate-vault-crashed-{}", process::id()));
        let _ = fs::remove_dir_all(&root);
        drop(Vault::make(&root).expect("a vault"));
        let staging = root.join(STAGING);
        let crashed = staging.join(format!("{PAST_EVERY_PID}-7.flac"));
        let living = staging.join(format!("{}-7.flac", process::id()));
        let unnamed = staging.join("stray.flac");
        for path in [&crashed, &living, &unnamed] {
            fs::write(path, b"half an object").expect("a staged write");
        }

        let vault = Vault::open(&root).expect("a vault");
        let crashed_after_opening = crashed.exists();
        let living_after_opening = living.exists();
        let unnamed_after_opening = unnamed.exists();
        let pruned = vault.sweep_the_staging().expect("a sweep");
        let living_after_pruning = living.exists();
        let unnamed_after_pruning = unnamed.exists();
        let _ = fs::remove_dir_all(&root);

        assert!(!crashed_after_opening);
        assert!(living_after_opening);
        assert!(unnamed_after_opening);
        assert_eq!(pruned, 1);
        assert!(living_after_pruning);
        assert!(!unnamed_after_pruning);
    }

    fn toned(frames: u32, seed: u32) -> Vec<u8> {
        const RATE: u32 = 44_100;

        let mut state = seed;
        let mut data = Vec::with_capacity(frames as usize * 4);
        for at in 0..frames {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = ((state >> 16) & 0xff) as i32 - 0x80;
            let tone = ((f64::from(at) * 0.05).sin() * 6000.0) as i32;
            for sample in [tone + noise, tone - noise] {
                data.extend_from_slice(&(sample as i16).to_le_bytes());
            }
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

    fn padded_declaring(object: &[u8], digest: VaultKey) -> Vec<u8> {
        const PADDING_LAST: u8 = 0x81;
        const PADDING_BYTES: u32 = 8 << 10;
        const STREAM_INFO_AT: usize = 4;
        const BLOCK_HEADER_BYTES: usize = 4;
        const STREAM_INFO_BYTES: usize = 34;
        const FRAMES_AT: usize = STREAM_INFO_AT + BLOCK_HEADER_BYTES + STREAM_INFO_BYTES;
        const DIGEST_AT: usize = FRAMES_AT - crate::key::KEY_BYTES;

        let mut copy = object[..FRAMES_AT].to_vec();
        copy[STREAM_INFO_AT] = 0;
        copy[DIGEST_AT..FRAMES_AT].copy_from_slice(digest.as_bytes());
        copy.push(PADDING_LAST);
        copy.extend_from_slice(&PADDING_BYTES.to_be_bytes()[1..]);
        copy.extend_from_slice(&[0; PADDING_BYTES as usize]);
        copy.extend_from_slice(&object[FRAMES_AT..]);
        copy
    }

    fn kept_from(vault: &Vault, path: &Path) -> Kept {
        match vault
            .keep(&Taking {
                sources: &Sources::local(),
                location: &MediaLocation::local(path),
                span: None,
                renewing: false,
                foretold: None,
                halt: Halt::NEVER,
            })
            .expect("a keeping")
        {
            Keeping::Kept(kept) => kept,
            Keeping::Refused(refusal) => panic!("refused as {refusal:?}"),
        }
    }

    fn encodes_begun() -> usize {
        flac::ENCODES_BEGUN.with(std::cell::Cell::get)
    }

    #[test]
    fn a_flac_declaring_the_digest_of_an_object_standing_is_deduped_without_an_encode() {
        let root = std::env::temp_dir().join(format!("resonate-vault-foretold-{}", process::id()));
        let _ = fs::remove_dir_all(&root);
        let vault = Vault::make(root.join("vault")).expect("a vault");
        let first = root.join("first.wav");
        let second = root.join("second.wav");
        fs::write(&first, toned(40_000, 0x1234_5678)).expect("a written source");
        fs::write(&second, toned(40_000, 0x0bad_f00d)).expect("a written source");
        let landed = kept_from(&vault, &first);
        let other = kept_from(&vault, &second);
        let object = fs::read(&landed.path).expect("the landed object");
        let other_object = fs::read(&other.path).expect("the other object");
        let copy = root.join("copy.flac");
        let lying = root.join("lying.flac");
        fs::write(&copy, padded_declaring(&object, landed.key)).expect("a copy");
        fs::write(&lying, padded_declaring(&other_object, landed.key)).expect("a lying copy");

        let before = encodes_begun();
        let deduped = kept_from(&vault, &copy);
        let after_the_copy = encodes_begun();
        let weighed = kept_from(&vault, &lying);
        let after_the_lie = encodes_begun();
        let _ = fs::remove_dir_all(&root);

        assert_eq!(landed.form, Form::Flac);
        assert!(!landed.deduped);
        assert!(deduped.deduped);
        assert_eq!(deduped.key, landed.key);
        assert_eq!(deduped.path, landed.path);
        assert_eq!(deduped.frames, landed.frames);
        assert_eq!(after_the_copy, before, "a duplicate was encoded again");
        assert_eq!(
            weighed.key, other.key,
            "a declared digest was taken on trust"
        );
        assert!(weighed.deduped);
        assert_eq!(after_the_lie, after_the_copy + 1);
    }

    #[test]
    fn a_wave_whose_key_the_catalog_foretells_is_deduped_without_an_encode() {
        let root = std::env::temp_dir().join(format!("resonate-vault-told-{}", process::id()));
        let _ = fs::remove_dir_all(&root);
        let vault = Vault::make(root.join("vault")).expect("a vault");
        let first = root.join("first.wav");
        let again = root.join("again.wav");
        let other = root.join("other.wav");
        fs::write(&first, toned(40_000, 0x1234_5678)).expect("a written source");
        fs::write(&again, toned(40_000, 0x1234_5678)).expect("a written source");
        fs::write(&other, toned(40_000, 0x0bad_f00d)).expect("a written source");
        let landed = kept_from(&vault, &first);
        let foretold = |path: &Path| match vault
            .keep(&Taking {
                sources: &Sources::local(),
                location: &MediaLocation::local(path),
                span: None,
                renewing: false,
                foretold: Some(landed.key),
                halt: Halt::NEVER,
            })
            .expect("a keeping")
        {
            Keeping::Kept(kept) => kept,
            Keeping::Refused(refusal) => panic!("refused as {refusal:?}"),
        };

        let before = encodes_begun();
        let deduped = foretold(&again);
        let after_the_copy = encodes_begun();
        let weighed = foretold(&other);
        let after_the_other = encodes_begun();
        let _ = fs::remove_dir_all(&root);

        assert!(deduped.deduped);
        assert_eq!(deduped.key, landed.key);
        assert_eq!(deduped.frames, landed.frames);
        assert_eq!(after_the_copy, before, "a foretold duplicate was encoded");
        assert!(!weighed.deduped);
        assert_ne!(weighed.key, landed.key, "a foretold key was taken on trust");
        assert_eq!(after_the_other, after_the_copy + 1);
    }

    #[test]
    fn a_path_within_the_vault_never_names_one_outside_it() {
        let root = std::env::temp_dir().join(format!("resonate-vault-within-{}", process::id()));
        let vault = Vault::make(&root).expect("a vault");

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
    fn a_vault_opened_where_none_is_makes_nothing_there_and_one_made_is_opened_after() {
        let mount =
            std::env::temp_dir().join(format!("resonate-vault-unmounted-{}", process::id()));
        let _ = fs::remove_dir_all(&mount);
        fs::create_dir_all(&mount).expect("an empty mount point");
        let root = mount.join("vault");

        let opened = Vault::open(&root);
        let left_bare = fs::read_dir(&mount).map(|entries| entries.count()).ok();
        let made = Vault::make(&root).map(|vault| vault.root().to_path_buf());
        let reopened = Vault::open(&root).is_ok();
        let _ = fs::remove_dir_all(&mount);

        assert!(
            matches!(opened, Err(Error::NotThere { .. })),
            "{:?}",
            opened.err()
        );
        assert_eq!(
            left_bare,
            Some(0),
            "opening a vault that was not there made one"
        );
        assert!(made.is_ok());
        assert!(reopened);
    }

    #[test]
    fn a_failure_on_the_vaults_own_disc_is_told_from_one_of_the_source() {
        let root = std::env::temp_dir().join(format!("resonate-vault-failed-{}", process::id()));
        let vault = Vault::make(&root).expect("a vault");
        let failed = |path: PathBuf, kind: io::ErrorKind| Error::Io {
            op: VaultOp::Write,
            path,
            source: io::Error::from(kind),
        };

        assert!(vault.failed_itself(&failed(
            root.join("staging").join("x"),
            io::ErrorKind::Other
        )));
        assert!(vault.failed_itself(&failed(
            PathBuf::from("/music/a.flac"),
            io::ErrorKind::StorageFull
        )));
        assert!(!vault.failed_itself(&failed(
            PathBuf::from("/music/a.flac"),
            io::ErrorKind::Other
        )));
        assert!(!vault.failed_itself(&Error::Unencodable {
            rate: 1,
            channels: 1,
            bits: 1
        }));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_path_climbing_out_of_the_vault_is_not_held_by_it() {
        let root = std::env::temp_dir().join(format!("resonate-vault-climbing-{}", process::id()));
        let vault = Vault::make(&root).expect("a vault");

        let climbing = root.join("..").join("elsewhere.flac");
        let deeper = root.join("audio").join("..").join("..").join("etc");

        assert!(!vault.holds(&climbing));
        assert!(!vault.holds(&deeper));
        assert!(vault.read_inside(&climbing).is_err());
        assert!(vault.holds(&root.join("audio").join("ab.flac")));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_extension_that_could_name_a_path_is_written_as_its_letters_alone() {
        assert_eq!(sanitised_extension(Some("../FLAC")), "flac");
        assert_eq!(sanitised_extension(Some("/")), UNNAMED_EXTENSION);
        assert_eq!(sanitised_extension(None), UNNAMED_EXTENSION);
    }
}
