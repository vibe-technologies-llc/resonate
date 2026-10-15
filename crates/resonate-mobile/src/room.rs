use std::{
    fmt, fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use resonate_codec::Sources;
use resonate_core::{AUDIO_EXTENSIONS, MediaLocation};
use resonate_engine::{Command, Impulse, read_impulse};

use crate::{Error, MobilePlayer, Result};

pub const IMPULSE_BYTES_AT_MOST: usize = 64 * 1024 * 1024;
const KEPT_STEM: &str = "room-correction";

#[derive(Clone, Copy, Debug)]
pub enum ImpulseOp {
    Keep,
    Find,
    Remove,
}

impl fmt::Display for ImpulseOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Keep => "kept",
            Self::Find => "found",
            Self::Remove => "removed",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RoomCorrection {
    pub channels: u32,
    pub rate: u32,
    pub length_ms: u64,
}

impl RoomCorrection {
    fn of(impulse: &Impulse) -> Self {
        let rate = impulse.rate().hz();
        Self {
            channels: u32::try_from(impulse.channels()).unwrap_or(u32::MAX),
            rate,
            length_ms: Duration::from_secs_f64(impulse.frames() as f64 / f64::from(rate))
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX),
        }
    }
}

pub(crate) struct Room {
    folder: PathBuf,
}

impl Room {
    pub(crate) const fn under(folder: PathBuf) -> Self {
        Self { folder }
    }

    fn kept(&self) -> Result<Option<PathBuf>> {
        let listed = fs::read_dir(&self.folder).map_err(|source| Error::ImpulseFile {
            op: ImpulseOp::Find,
            source,
        })?;
        for entry in listed {
            let path = entry
                .map_err(|source| Error::ImpulseFile {
                    op: ImpulseOp::Find,
                    source,
                })?
                .path();
            if path.file_stem().and_then(|stem| stem.to_str()) == Some(KEPT_STEM) {
                return Ok(Some(path));
            }
        }
        Ok(None)
    }

    fn forget(&self) -> Result<()> {
        while let Some(path) = self.kept()? {
            fs::remove_file(&path).map_err(|source| Error::ImpulseFile {
                op: ImpulseOp::Remove,
                source,
            })?;
        }
        Ok(())
    }

    fn keep(&self, bytes: &[u8], extension: &str) -> Result<PathBuf> {
        let extension = extension.to_ascii_lowercase();
        if !AUDIO_EXTENSIONS.contains(&extension.as_str()) {
            return Err(Error::ImpulseExtension);
        }
        if bytes.len() > IMPULSE_BYTES_AT_MOST {
            return Err(Error::ImpulseTooLarge);
        }
        self.forget()?;
        let path = self.folder.join(format!("{KEPT_STEM}.{extension}"));
        fs::write(&path, bytes).map_err(|source| Error::ImpulseFile {
            op: ImpulseOp::Keep,
            source,
        })?;
        Ok(path)
    }
}

fn read(sources: &Sources, path: &Path) -> Result<Impulse> {
    read_impulse(sources, &MediaLocation::local(path))?.ok_or(Error::NoImpulse)
}

#[uniffi::export]
impl MobilePlayer {
    pub fn keep_room_correction(
        &self,
        bytes: Vec<u8>,
        extension: String,
    ) -> Result<RoomCorrection> {
        let path = self.room.keep(&bytes, &extension)?;
        match read(&self.sources, &path) {
            Ok(impulse) => Ok(RoomCorrection::of(&impulse)),
            Err(error) => {
                self.room.forget()?;
                Err(error)
            }
        }
    }

    pub fn apply_room_correction(&self, on: bool) -> Result<Option<RoomCorrection>> {
        let impulse = match (on, self.room.kept()?) {
            (true, Some(path)) => Some(Arc::new(read(&self.sources, &path)?)),
            _ => None,
        };
        let described = impulse.as_deref().map(RoomCorrection::of);
        self.command(Command::SetConvolution(impulse))?;
        Ok(described)
    }

    pub fn forget_room_correction(&self) -> Result<()> {
        self.command(Command::SetConvolution(None))?;
        self.room.forget()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("resonate-room-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).expect("a folder");
        folder
    }

    fn wave(samples: &[i16]) -> Vec<u8> {
        let rate: u32 = 48_000;
        let data = u32::try_from(samples.len() * 2).expect("a short wave");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * 2).to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn a_kept_response_replaces_the_last_and_reads_back_as_an_impulse() {
        let folder = folder("kept");
        let room = Room::under(folder.clone());
        let sources = Sources::local();

        room.keep(b"an older response", "flac")
            .expect("a first file");
        let path = room
            .keep(&wave(&[i16::MAX, 0, 0, 0, 0, 0, 0, 0]), "WAV")
            .expect("a kept response");
        let impulse = read(&sources, &path).expect("an impulse");

        assert_eq!(room.kept().expect("a listing"), Some(path));
        assert_eq!(fs::read_dir(&folder).expect("a listing").count(), 1);
        assert_eq!(RoomCorrection::of(&impulse).channels, 1);
        assert_eq!(RoomCorrection::of(&impulse).rate, 48_000);

        room.forget().expect("forgotten");

        assert_eq!(room.kept().expect("a listing"), None);
    }

    #[test]
    fn a_file_that_is_not_audio_or_too_large_is_refused_before_it_is_kept() {
        let folder = folder("refused");
        let room = Room::under(folder.clone());

        let named = room.keep(b"text", "txt");
        let large = room.keep(&vec![0; IMPULSE_BYTES_AT_MOST + 1], "wav");

        assert!(matches!(named, Err(Error::ImpulseExtension)));
        assert!(matches!(large, Err(Error::ImpulseTooLarge)));
        assert_eq!(room.kept().expect("a listing"), None);
    }
}
