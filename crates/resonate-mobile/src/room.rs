use std::{
    fmt, fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use parking_lot::Mutex;
use resonate_codec::Sources;
use resonate_core::{
    AUDIO_EXTENSIONS, HRIR_EXTENSIONS, MediaLocation, SpeakerAngle, SpeakerElevation, Speakers,
};
use resonate_engine::{
    Command, Impulse, PairedReadError, binaural_wave, read_binaural_pair, read_impulse,
};

use crate::{Error, MobilePlayer, Result};

pub const IMPULSE_BYTES_AT_MOST: usize = 64 * 1024 * 1024;
const KEPT_STEM: &str = "room-correction";
const CANDIDATE_STEM: &str = "room-correction-candidate";
const LEFT_STEM: &str = "room-correction-candidate-left";
const RIGHT_STEM: &str = "room-correction-candidate-right";

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

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SpeakerLimits {
    pub angle_least_degrees: u8,
    pub angle_most_degrees: u8,
    pub angle_standard_degrees: u8,
    pub elevation_least_degrees: i16,
    pub elevation_most_degrees: i16,
}

#[uniffi::export]
pub fn speaker_limits() -> SpeakerLimits {
    SpeakerLimits {
        angle_least_degrees: SpeakerAngle::NARROWEST_DEGREES,
        angle_most_degrees: SpeakerAngle::WIDEST_DEGREES,
        angle_standard_degrees: SpeakerAngle::STANDARD.degrees(),
        elevation_least_degrees: i16::from(SpeakerElevation::LOWEST_DEGREES),
        elevation_most_degrees: i16::from(SpeakerElevation::HIGHEST_DEGREES),
    }
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
    applied: Mutex<Option<Arc<Impulse>>>,
    speakers: Mutex<Speakers>,
}

impl Room {
    pub(crate) const fn under(folder: PathBuf) -> Self {
        Self {
            folder,
            applied: Mutex::new(None),
            speakers: Mutex::new(Speakers::STANDARD),
        }
    }

    pub(crate) fn applied(&self) -> Option<Arc<Impulse>> {
        self.applied.lock().clone()
    }

    fn speakers(&self) -> Speakers {
        *self.speakers.lock()
    }

    fn speak_at(&self, speakers: Speakers) {
        *self.speakers.lock() = speakers;
    }

    fn apply(&self, impulse: Option<Arc<Impulse>>) {
        *self.applied.lock() = impulse;
    }

    fn kept(&self) -> Result<Option<PathBuf>> {
        self.stemmed(KEPT_STEM)
    }

    fn stemmed(&self, wanted: &str) -> Result<Option<PathBuf>> {
        Ok(self.every(wanted)?.into_iter().next())
    }

    fn every(&self, wanted: &str) -> Result<Vec<PathBuf>> {
        let listed = fs::read_dir(&self.folder).map_err(|source| Error::ImpulseFile {
            op: ImpulseOp::Find,
            source,
        })?;
        let mut found = Vec::new();
        for entry in listed {
            let path = entry
                .map_err(|source| Error::ImpulseFile {
                    op: ImpulseOp::Find,
                    source,
                })?
                .path();
            if path.file_stem().and_then(|stem| stem.to_str()) == Some(wanted) {
                found.push(path);
            }
        }
        Ok(found)
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

    fn drop_candidates(&self) -> Result<()> {
        self.drop_stemmed(CANDIDATE_STEM)
    }

    fn drop_stemmed(&self, stem: &str) -> Result<()> {
        while let Some(path) = self.stemmed(stem)? {
            fs::remove_file(&path).map_err(|source| Error::ImpulseFile {
                op: ImpulseOp::Remove,
                source,
            })?;
        }
        Ok(())
    }

    fn stage(&self, bytes: &[u8], extension: &str) -> Result<PathBuf> {
        self.drop_candidates()?;
        self.stage_as(CANDIDATE_STEM, bytes, extension)
    }

    fn stage_as(&self, stem: &str, bytes: &[u8], extension: &str) -> Result<PathBuf> {
        let extension = extension.to_ascii_lowercase();
        if !AUDIO_EXTENSIONS.contains(&extension.as_str())
            && !HRIR_EXTENSIONS.contains(&extension.as_str())
        {
            return Err(Error::ImpulseExtension);
        }
        if bytes.len() > IMPULSE_BYTES_AT_MOST {
            return Err(Error::ImpulseTooLarge);
        }
        let path = self.folder.join(format!("{stem}.{extension}"));
        fs::write(&path, bytes).map_err(|source| Error::ImpulseFile {
            op: ImpulseOp::Keep,
            source,
        })?;
        Ok(path)
    }

    fn keep_pair(
        &self,
        left: Speaking<'_>,
        right: Speaking<'_>,
        sources: &Sources,
    ) -> Result<Impulse> {
        self.drop_stemmed(LEFT_STEM)?;
        self.drop_stemmed(RIGHT_STEM)?;
        let left_path = self.stage_as(LEFT_STEM, left.bytes, left.extension)?;
        let right_path = match self.stage_as(RIGHT_STEM, right.bytes, right.extension) {
            Ok(path) => path,
            Err(error) => {
                let _ = fs::remove_file(&left_path);
                return Err(error);
            }
        };
        let paired = read_binaural_pair(
            sources,
            &MediaLocation::local(&left_path),
            &MediaLocation::local(&right_path),
            self.speakers(),
        );
        let _ = fs::remove_file(&left_path);
        let _ = fs::remove_file(&right_path);
        let whole = paired.map_err(pairing_refused)?;
        let wave = binaural_wave(&whole).map_err(pairing_refused)?;
        self.keep(&wave, "wav", sources)
    }

    fn promote(&self, candidate: &Path) -> Result<PathBuf> {
        let extension = candidate
            .extension()
            .and_then(|extension| extension.to_str())
            .ok_or(Error::ImpulseExtension)?;
        let target = self.folder.join(format!("{KEPT_STEM}.{extension}"));
        fs::rename(candidate, &target).map_err(|source| Error::ImpulseFile {
            op: ImpulseOp::Keep,
            source,
        })?;
        for older in self
            .every(KEPT_STEM)?
            .into_iter()
            .filter(|path| *path != target)
        {
            fs::remove_file(&older).map_err(|source| Error::ImpulseFile {
                op: ImpulseOp::Remove,
                source,
            })?;
        }
        Ok(target)
    }

    fn keep(&self, bytes: &[u8], extension: &str, sources: &Sources) -> Result<Impulse> {
        let candidate = self.stage(bytes, extension)?;
        let impulse = match read(sources, &candidate, self.speakers()) {
            Ok(impulse) => impulse,
            Err(error) => {
                let _ = fs::remove_file(&candidate);
                return Err(error);
            }
        };
        self.promote(&candidate)?;
        Ok(impulse)
    }
}

pub(crate) struct Speaking<'a> {
    pub bytes: &'a [u8],
    pub extension: &'a str,
}

fn pairing_refused(source: PairedReadError) -> Error {
    Error::ImpulsePair {
        source: Box::new(source),
    }
}

fn read(sources: &Sources, path: &Path, speakers: Speakers) -> Result<Impulse> {
    read_impulse(sources, &MediaLocation::local(path), speakers)?.ok_or(Error::NoImpulse)
}

impl MobilePlayer {
    fn speak_at(&self, speakers: Speakers) -> Result<Option<RoomCorrection>> {
        let described = match (self.room.applied(), self.room.kept()?) {
            (Some(_), Some(path)) => {
                let impulse = Arc::new(read(&self.sources, &path, speakers)?);
                let described = RoomCorrection::of(&impulse);
                self.command(Command::SetConvolution(Some(impulse.clone())))?;
                self.room.apply(Some(impulse));
                Some(described)
            }
            _ => None,
        };
        self.room.speak_at(speakers);
        Ok(described)
    }
}

#[uniffi::export]
impl MobilePlayer {
    pub fn keep_room_correction(
        &self,
        bytes: Vec<u8>,
        extension: String,
    ) -> Result<RoomCorrection> {
        let impulse = self.room.keep(&bytes, &extension, &self.sources)?;
        Ok(RoomCorrection::of(&impulse))
    }

    pub fn keep_binaural_room_correction(
        &self,
        left_bytes: Vec<u8>,
        left_extension: String,
        right_bytes: Vec<u8>,
        right_extension: String,
    ) -> Result<RoomCorrection> {
        let impulse = self.room.keep_pair(
            Speaking {
                bytes: &left_bytes,
                extension: &left_extension,
            },
            Speaking {
                bytes: &right_bytes,
                extension: &right_extension,
            },
            &self.sources,
        )?;
        Ok(RoomCorrection::of(&impulse))
    }

    pub fn kept_room_correction(&self) -> Result<Option<RoomCorrection>> {
        let Some(path) = self.room.kept()? else {
            return Ok(None);
        };
        let speakers = self.room.speakers();
        Ok(Some(RoomCorrection::of(&read(
            &self.sources,
            &path,
            speakers,
        )?)))
    }

    pub fn apply_room_correction(&self, on: bool) -> Result<Option<RoomCorrection>> {
        let impulse = match (on, self.room.kept()?) {
            (true, Some(path)) => Some(Arc::new(read(&self.sources, &path, self.room.speakers())?)),
            _ => None,
        };
        let described = impulse.as_deref().map(RoomCorrection::of);
        self.command(Command::SetConvolution(impulse.clone()))?;
        self.room.apply(impulse);
        Ok(described)
    }

    pub fn speaker_angle(&self) -> u8 {
        self.room.speakers().angle.degrees()
    }

    pub fn speaker_elevation(&self) -> i16 {
        i16::from(self.room.speakers().elevation.degrees())
    }

    pub fn set_speaker_angle(&self, degrees: u8) -> Result<Option<RoomCorrection>> {
        let angle = SpeakerAngle::from_degrees(degrees)?;
        self.speak_at(Speakers {
            angle,
            ..self.room.speakers()
        })
    }

    pub fn set_speaker_elevation(&self, degrees: i16) -> Result<Option<RoomCorrection>> {
        let elevation = SpeakerElevation::from_degrees(degrees)?;
        self.speak_at(Speakers {
            elevation,
            ..self.room.speakers()
        })
    }

    pub fn forget_room_correction(&self) -> Result<()> {
        self.command(Command::SetConvolution(None))?;
        self.room.apply(None);
        self.room.forget()
    }
}

#[cfg(test)]
pub(crate) fn wave(samples: &[i16]) -> Vec<u8> {
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

#[cfg(test)]
mod tests {
    use hdf5_writer::{AttributeBuilder, DatasetBuilder, Hdf5Builder, WriteOptions};

    use super::*;

    fn folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("resonate-room-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).expect("a folder");
        folder
    }

    #[test]
    fn a_kept_response_replaces_the_last_and_reads_back_as_an_impulse() {
        let folder = folder("kept");
        let room = Room::under(folder.clone());
        let sources = Sources::local();

        let older = room
            .stage(b"an older response", "flac")
            .expect("a first file");
        room.promote(&older).expect("an older response kept");
        let impulse = room
            .keep(&wave(&[i16::MAX, 0, 0, 0, 0, 0, 0, 0]), "WAV", &sources)
            .expect("a kept response");

        assert_eq!(
            room.kept().expect("a listing"),
            Some(folder.join("room-correction.wav"))
        );
        assert_eq!(fs::read_dir(&folder).expect("a listing").count(), 1);
        assert_eq!(RoomCorrection::of(&impulse).channels, 1);
        assert_eq!(RoomCorrection::of(&impulse).rate, 48_000);

        room.forget().expect("forgotten");

        assert_eq!(room.kept().expect("a listing"), None);
    }

    fn stereo_wave(frames: &[[i16; 2]], rate: u32) -> Vec<u8> {
        let data = u32::try_from(frames.len() * 4).expect("a short wave");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * 4).to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data.to_le_bytes());
        for frame in frames {
            for sample in frame {
                bytes.extend_from_slice(&sample.to_le_bytes());
            }
        }
        bytes
    }

    #[test]
    fn two_stereo_responses_are_kept_as_one_two_by_two_response_that_reads_back() {
        let folder = folder("pair");
        let room = Room::under(folder.clone());
        let sources = Sources::local();
        let left = stereo_wave(&[[16_384, 4_096], [0, 0]], 48_000);
        let right = stereo_wave(&[[2_048, 16_384], [0, 0]], 48_000);

        let kept = room
            .keep_pair(
                Speaking {
                    bytes: &left,
                    extension: "wav",
                },
                Speaking {
                    bytes: &right,
                    extension: "WAV",
                },
                &sources,
            )
            .expect("a pair kept");

        assert!(kept.is_crossed());
        assert_eq!(RoomCorrection::of(&kept).channels, 4);
        assert_eq!(fs::read_dir(&folder).expect("a listing").count(), 1);
        let again = read(
            &sources,
            &room.kept().expect("a listing").expect("a file"),
            room.speakers(),
        )
        .expect("read back");
        assert!(again.is_crossed());
    }

    #[test]
    fn a_pair_at_two_rates_or_lengths_is_refused_and_the_kept_response_stands() {
        let folder = folder("pair-refused");
        let room = Room::under(folder.clone());
        let sources = Sources::local();
        room.keep(&wave(&[i16::MAX, 0, 0, 0]), "wav", &sources)
            .expect("a kept response");
        let left = stereo_wave(&[[16_384, 4_096], [0, 0]], 48_000);
        let slower = stereo_wave(&[[2_048, 16_384], [0, 0]], 44_100);
        let longer = stereo_wave(&[[2_048, 16_384], [0, 0], [0, 0]], 48_000);
        let at = |bytes| Speaking {
            bytes,
            extension: "wav",
        };

        let rates = room.keep_pair(at(&left), at(&slower), &sources);
        let lengths = room.keep_pair(at(&left), at(&longer), &sources);

        assert!(matches!(
            rates,
            Err(Error::ImpulsePair { source })
                if matches!(*source, PairedReadError::Pair(resonate_engine::DspError::SpeakerRatesDiffer { .. }))
        ));
        assert!(matches!(
            lengths,
            Err(Error::ImpulsePair { source })
                if matches!(*source, PairedReadError::Pair(resonate_engine::DspError::SpeakerLengthsDiffer { .. }))
        ));
        assert_eq!(
            room.kept().expect("a listing"),
            Some(folder.join("room-correction.wav"))
        );
        assert_eq!(fs::read_dir(&folder).expect("a listing").count(), 1);
    }

    fn head_related_set() -> Vec<u8> {
        let mut impulses = Vec::new();
        for measurement in 0..2 {
            for ear in 0..2 {
                for frame in 0..8 {
                    impulses
                        .push(f64::from(measurement * 2 + ear + 1) / 10.0 * f64::from(frame == 0));
                }
            }
        }
        Hdf5Builder::new()
            .dataset(DatasetBuilder::typed_data("Data.IR", vec![2, 2, 8], &impulses).expect("taps"))
            .dataset(
                DatasetBuilder::typed_data(
                    "SourcePosition",
                    vec![2, 3],
                    &[30.0, 0.0, 1.0, 330.0, 0.0, 1.0],
                )
                .expect("positions")
                .attribute(AttributeBuilder::fixed_string("Type", "spherical")),
            )
            .dataset(
                DatasetBuilder::typed_data("Data.SamplingRate", vec![1], &[48_000.0])
                    .expect("rate"),
            )
            .into_plan()
            .expect("a plan")
            .encode(WriteOptions::default())
            .expect("bytes")
    }

    #[test]
    fn a_set_of_head_related_responses_is_kept_as_the_two_by_two_response_it_reads_as() {
        let folder = folder("sofa");
        let room = Room::under(folder.clone());
        let sources = Sources::local();

        let impulse = room
            .keep(&head_related_set(), "SOFA", &sources)
            .expect("a kept set");

        assert_eq!(
            room.kept().expect("a listing"),
            Some(folder.join("room-correction.sofa"))
        );
        assert_eq!(RoomCorrection::of(&impulse).channels, 4);
        assert_eq!(RoomCorrection::of(&impulse).rate, 48_000);
        assert_eq!(fs::read_dir(&folder).expect("a listing").count(), 1);
    }

    #[test]
    fn the_speaker_angle_a_listener_sets_is_the_one_a_set_is_read_at() {
        let folder = folder("angle");
        let room = Room::under(folder.clone());
        let sources = Sources::local();
        room.keep(&head_related_set(), "sofa", &sources)
            .expect("a kept set");
        room.speak_at(Speakers::at(
            SpeakerAngle::from_degrees(90).expect("an angle"),
        ));

        let refused = room.keep(&head_related_set(), "sofa", &sources);

        assert!(matches!(refused, Err(Error::Decode(_))));
        assert_eq!(
            room.kept().expect("a listing"),
            Some(folder.join("room-correction.sofa"))
        );
        assert_eq!(fs::read_dir(&folder).expect("a listing").count(), 1);
    }

    #[test]
    fn the_speaker_elevation_a_listener_sets_is_the_one_a_set_is_read_at() {
        let folder = folder("elevation");
        let room = Room::under(folder.clone());
        let sources = Sources::local();
        room.keep(&head_related_set(), "sofa", &sources)
            .expect("a kept set");
        room.speak_at(
            Speakers::STANDARD.raised_to(SpeakerElevation::from_degrees(60).expect("an elevation")),
        );

        let refused = room.keep(&head_related_set(), "sofa", &sources);

        assert!(matches!(refused, Err(Error::Decode(_))));
        assert_eq!(fs::read_dir(&folder).expect("a listing").count(), 1);
    }

    #[test]
    fn the_limits_a_phone_offers_are_the_ones_the_boundary_refuses_beyond() {
        let limits = speaker_limits();
        let player_angle = |degrees: u8| SpeakerAngle::from_degrees(degrees).is_ok();
        let player_elevation = |degrees: i16| SpeakerElevation::from_degrees(degrees).is_ok();

        assert!(player_angle(limits.angle_least_degrees));
        assert!(player_angle(limits.angle_most_degrees));
        assert!(!player_angle(limits.angle_least_degrees - 1));
        assert!(!player_angle(limits.angle_most_degrees + 1));
        assert!(player_elevation(limits.elevation_least_degrees));
        assert!(player_elevation(limits.elevation_most_degrees));
        assert!(!player_elevation(limits.elevation_least_degrees - 1));
        assert!(!player_elevation(limits.elevation_most_degrees + 1));
        assert_eq!(limits.angle_standard_degrees, 30);
    }

    #[test]
    fn a_file_that_is_not_audio_or_too_large_is_refused_before_it_is_kept() {
        let folder = folder("refused");
        let room = Room::under(folder.clone());

        let named = room.stage(b"text", "txt");
        let large = room.stage(&vec![0; IMPULSE_BYTES_AT_MOST + 1], "wav");

        assert!(matches!(named, Err(Error::ImpulseExtension)));
        assert!(matches!(large, Err(Error::ImpulseTooLarge)));
        assert_eq!(room.kept().expect("a listing"), None);
    }

    #[test]
    fn a_response_that_does_not_read_leaves_the_kept_one_standing() {
        let folder = folder("unreadable");
        let room = Room::under(folder.clone());
        let sources = Sources::local();

        room.keep(&wave(&[i16::MAX, 0, 0, 0]), "wav", &sources)
            .expect("a kept response");
        let refused = room.keep(b"not a wave at all", "wav", &sources);

        assert!(refused.is_err());
        assert_eq!(
            room.kept().expect("a listing"),
            Some(folder.join("room-correction.wav"))
        );
        assert_eq!(fs::read_dir(&folder).expect("a listing").count(), 1);
    }
}
