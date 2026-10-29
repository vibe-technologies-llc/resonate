use std::{
    fmt,
    io::{self, Read, Seek, SeekFrom},
    sync::Arc,
    time::Duration,
};

use hdf5_reader::{
    Dataset, Datatype, Hdf5File, OpenOptions, SliceInfo, SliceInfoElem,
    storage::RangeRequestStorage,
};
use parking_lot::Mutex;
use resonate_core::{Frames, MediaLocation, SampleRate, SpeakerAngle, Speakers};
use rustfft::{FftPlanner, num_complex::Complex};
use thiserror::Error;

use crate::{
    Error, MediaStream, Result, Sources,
    netcdf::{
        self, Classic, ClassicFault, FIRST_OFFSET_VERSION, Kind, Pick, SECOND_OFFSET_VERSION,
        WIDE_DATA_VERSION,
    },
};

const SIGNATURE: [u8; 8] = *b"\x89HDF\r\n\x1a\n";

pub const SOFA_BYTES_AT_MOST: u64 = 128 << 20;
pub const SOFA_STREAMED_BYTES_AT_MOST: u64 = 8 << 30;

const MEASUREMENTS_AT_MOST: u64 = 1 << 20;
const EMITTERS_AT_MOST: u64 = 1 << 16;
const RATES_AT_MOST: u64 = 1 << 20;
const EARS: u64 = 2;
const COORDINATES: u64 = 3;
const NEAREST_WITHIN_DEGREES: f64 = 45.0;
const EXACT_WITHIN_DEGREES: f64 = 0.5;
const NEIGHBOURS_WITHIN_DEGREES: f64 = 15.0;
const NEIGHBOURS_AT_MOST: usize = 3;
const TRIANGLE_WITHIN_DEGREES: f64 = 30.0;
const TRIANGLE_CANDIDATES_AT_MOST: usize = 16;
const TRIANGLE_AREA_AT_LEAST: f64 = 1e-5;
const INSIDE_BY_AT_LEAST: f64 = -1e-9;
const CAP_TOLERANCE: f64 = 1e-9;
const WEIGHT_WORTH_READING: f64 = 1e-9;
const POLE_NEAR: f64 = 0.9;
const IN_FRONT_OF_THE_TANGENT_PLANE_AT_LEAST: f64 = 0.1;
const ONSET_AT_FRACTION_OF_PEAK: f64 = 0.1;
const RING_ELEVATION_WITHIN_DEGREES: f64 = 1.0;
const RING_GAP_AT_MOST_DEGREES: f64 = 60.0;
const RING_POLE_ELEVATION_DEGREES: f64 = 80.0;
const LISTENER_AT_THE_ORIGIN_WITHIN: f64 = 1e-9;
const SPECTRUM_FLOOR: f64 = 1e-6;
const SPECTRUM_PADDING: usize = 4;
const HALF_TURN_DEGREES: f64 = 180.0;
const FULL_TURN_DEGREES: f64 = 360.0;
const CHUNK_CACHE_BYTES: usize = 16 << 20;
const CHUNK_BYTES_AT_MOST: usize = 64 << 20;
const STREAMED_READ_AT_MOST: usize = CHUNK_BYTES_AT_MOST;
const SINGLE_PRECISION_BYTES: u8 = 4;
const DOUBLE_PRECISION_BYTES: u8 = 8;
const RADIANS: &str = "rad";
const FRONT: Direction = Direction {
    front: 1.0,
    left: 0.0,
    up: 0.0,
};
const UP: Direction = Direction {
    front: 0.0,
    left: 0.0,
    up: 1.0,
};

#[derive(Clone, Debug, PartialEq)]
pub struct Hrir {
    pub rate: SampleRate,
    pub responses: Vec<Vec<f64>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Member {
    Impulses,
    SamplingRate,
    SourcePosition,
    Delay,
    PositionType,
    PositionUnits,
    EmitterPosition,
    EmitterType,
    EmitterUnits,
    ListenerView,
    ViewType,
    ViewUnits,
    ListenerPosition,
    ListenerPositionType,
    ListenerPositionUnits,
}

impl Member {
    const fn name(self) -> &'static str {
        match self {
            Self::Impulses => "Data.IR",
            Self::SamplingRate => "Data.SamplingRate",
            Self::SourcePosition => "SourcePosition",
            Self::Delay => "Data.Delay",
            Self::PositionType => "SourcePosition:Type",
            Self::PositionUnits => "SourcePosition:Units",
            Self::EmitterPosition => "EmitterPosition",
            Self::EmitterType => "EmitterPosition:Type",
            Self::EmitterUnits => "EmitterPosition:Units",
            Self::ListenerView => "ListenerView",
            Self::ViewType => "ListenerView:Type",
            Self::ViewUnits => "ListenerView:Units",
            Self::ListenerPosition => "ListenerPosition",
            Self::ListenerPositionType => "ListenerPosition:Type",
            Self::ListenerPositionUnits => "ListenerPosition:Units",
        }
    }

    const fn attribute_name(self) -> &'static str {
        match self {
            Self::PositionType
            | Self::EmitterType
            | Self::ViewType
            | Self::ListenerPositionType => "Type",
            _ => "Units",
        }
    }
}

impl fmt::Display for Member {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Locate,
    Attribute,
    Read,
}

impl fmt::Display for Access {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Locate => "locate",
            Self::Attribute => "read the attribute of",
            Self::Read => "read",
        };
        f.write_str(name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    fn azimuth_degrees(self, angle: SpeakerAngle) -> f64 {
        let degrees = f64::from(angle.degrees());
        match self {
            Self::Left => degrees,
            Self::Right => -degrees,
        }
    }
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Left => "left",
            Self::Right => "right",
        };
        f.write_str(name)
    }
}

#[derive(Debug, Error)]
pub enum Fault {
    #[error("the set holds {bytes} bytes, more than the {at_most} one is read from")]
    TooLarge { bytes: u64, at_most: u64 },

    #[error("the HDF5 container could not be opened")]
    Open {
        #[source]
        source: Box<hdf5_reader::error::Error>,
    },

    #[error("the netCDF header could not be read")]
    ClassicOpen {
        #[source]
        source: ClassicFault,
    },

    #[error("the file is netCDF version {version}, and only classic versions 1, 2 and 5 are read")]
    ClassicVersion { version: u8 },

    #[error("could not {op} {member}")]
    Hdf5 {
        member: Member,
        op: Access,
        #[source]
        source: Box<hdf5_reader::error::Error>,
    },

    #[error("could not read {member} of the netCDF file")]
    Classic {
        member: Member,
        #[source]
        source: ClassicFault,
    },

    #[error("the set has no {member}")]
    MemberMissing { member: Member },

    #[error("{member} is not made of floating point numbers")]
    NotFloating { member: Member },

    #[error("{member} has the shape {dims:?}, which a head-related set does not")]
    ShapeNotUsable { member: Member, dims: Vec<u64> },

    #[error("the set holds no measurements")]
    NoMeasurements,

    #[error("the set's responses hold no samples")]
    NoSamples,

    #[error("the set holds {measurements} measurements, more than {at_most} are read")]
    TooManyMeasurements { measurements: u64, at_most: u64 },

    #[error("the set holds {emitters} emitters, more than {at_most} are read")]
    TooManyEmitters { emitters: u64, at_most: u64 },

    #[error("the source positions are neither spherical nor cartesian")]
    PositionTypeUnknown,

    #[error("a rate of {hertz} Hz is not one a stream can run at")]
    RateNotUsable { hertz: f64 },

    #[error("a delay of {samples} samples is not a number")]
    DelayNotUsable { samples: f64 },

    #[error("no measurement lies within {within} degrees of the {side} speaker")]
    NoMeasurementNear { side: Side, within: f64 },
}

pub fn read_hrir(
    sources: &Sources,
    location: &MediaLocation,
    longest: Duration,
    speakers: Speakers,
) -> Result<Option<Hrir>> {
    let unread = |source| Error::Io {
        location: location.clone(),
        source,
    };
    let mut media = sources.open(location)?;

    let mut bytes = vec![0; SIGNATURE.len()];
    match media.stream.read_exact(&mut bytes) {
        Ok(()) => {}
        Err(source) if source.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(source) => return Err(unread(source)),
    }
    if !is_container(&bytes) {
        return Ok(None);
    }

    if bytes.starts_with(&SIGNATURE)
        && media.stream.is_seekable()
        && let Some(length) = media.stream.byte_len()
    {
        return read_hrir_streamed(media.stream, length, longest, speakers).map_err(|source| {
            Error::Sofa {
                location: location.clone(),
                source,
            }
        });
    }

    let room = SOFA_BYTES_AT_MOST + 1 - SIGNATURE.len() as u64;
    (&mut *media.stream)
        .take(room)
        .read_to_end(&mut bytes)
        .map_err(unread)?;

    read_hrir_bytes(bytes, longest, speakers).map_err(|source| Error::Sofa {
        location: location.clone(),
        source,
    })
}

fn is_container(bytes: &[u8]) -> bool {
    bytes.starts_with(&SIGNATURE) || netcdf::version_of(bytes).is_some()
}

fn read_hrir_streamed(
    stream: Box<dyn MediaStream>,
    length: u64,
    longest: Duration,
    speakers: Speakers,
) -> std::result::Result<Option<Hrir>, Fault> {
    let container = Container::streamed(stream, length)?;
    Set::of(&container)?
        .stereo_pair(longest, speakers)
        .map(Some)
}

pub fn read_hrir_bytes(
    bytes: Vec<u8>,
    longest: Duration,
    speakers: Speakers,
) -> std::result::Result<Option<Hrir>, Fault> {
    if !is_container(&bytes) {
        return Ok(None);
    }
    let held = bytes.len() as u64;
    if held > SOFA_BYTES_AT_MOST {
        return Err(Fault::TooLarge {
            bytes: held,
            at_most: SOFA_BYTES_AT_MOST,
        });
    }

    let container = Container::of(bytes)?;
    Set::of(&container)?
        .stereo_pair(longest, speakers)
        .map(Some)
}

enum Container {
    Hdf5(Hdf5File),
    Classic(Classic),
}

impl Container {
    fn streamed(stream: Box<dyn MediaStream>, length: u64) -> std::result::Result<Self, Fault> {
        if length > SOFA_STREAMED_BYTES_AT_MOST {
            return Err(Fault::TooLarge {
                bytes: length,
                at_most: SOFA_STREAMED_BYTES_AT_MOST,
            });
        }

        let stream = Mutex::new(stream);
        let storage = RangeRequestStorage::new(length, move |offset, wanted| {
            let end = u64::try_from(wanted)
                .ok()
                .and_then(|wanted| offset.checked_add(wanted))
                .filter(|end| *end <= length)
                .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))?;
            if wanted > STREAMED_READ_AT_MOST || end > length {
                return Err(io::Error::from(io::ErrorKind::OutOfMemory).into());
            }
            let mut held = vec![0; wanted];
            let mut stream = stream.lock();
            stream.seek(SeekFrom::Start(offset))?;
            stream.read_exact(&mut held)?;
            Ok(held)
        });
        let options = OpenOptions::default()
            .with_chunk_cache_bytes(CHUNK_CACHE_BYTES)
            .with_max_chunk_bytes(CHUNK_BYTES_AT_MOST);
        Hdf5File::from_storage_with_options(Arc::new(storage), options)
            .map(Self::Hdf5)
            .map_err(|source| Fault::Open {
                source: Box::new(source),
            })
    }

    fn of(bytes: Vec<u8>) -> std::result::Result<Self, Fault> {
        if bytes.starts_with(&SIGNATURE) {
            let options = OpenOptions::default()
                .with_chunk_cache_bytes(CHUNK_CACHE_BYTES)
                .with_max_chunk_bytes(CHUNK_BYTES_AT_MOST);
            return Hdf5File::from_vec_with_options(bytes, options)
                .map(Self::Hdf5)
                .map_err(|source| Fault::Open {
                    source: Box::new(source),
                });
        }
        match netcdf::version_of(&bytes) {
            Some(FIRST_OFFSET_VERSION | SECOND_OFFSET_VERSION | WIDE_DATA_VERSION) => {
                Classic::of(bytes)
                    .map(Self::Classic)
                    .map_err(|source| Fault::ClassicOpen { source })
            }
            Some(version) => Err(Fault::ClassicVersion { version }),
            None => Err(Fault::ClassicOpen {
                source: ClassicFault::Truncated { at: 0, wanted: 4 },
            }),
        }
    }

    fn find(&self, member: Member) -> std::result::Result<Option<Variable<'_>>, Fault> {
        match self {
            Self::Hdf5(file) => match file.dataset(member.name()) {
                Ok(dataset) => Ok(Some(Variable::Hdf5(Box::new(dataset)))),
                Err(source) if is_missing(&source) => Ok(None),
                Err(source) => Err(Fault::Hdf5 {
                    member,
                    op: Access::Locate,
                    source: Box::new(source),
                }),
            },
            Self::Classic(classic) => Ok(classic.variable(member.name()).map(Variable::Classic)),
        }
    }

    fn locate(&self, member: Member) -> std::result::Result<Variable<'_>, Fault> {
        self.find(member)?.ok_or(Fault::MemberMissing { member })
    }
}

enum Variable<'a> {
    Hdf5(Box<Dataset>),
    Classic(netcdf::Variable<'a>),
}

impl Variable<'_> {
    fn shape(&self) -> Vec<u64> {
        match self {
            Self::Hdf5(dataset) => dataset.shape().to_vec(),
            Self::Classic(variable) => variable.shape(),
        }
    }

    fn floating_bytes(&self) -> Option<u8> {
        match self {
            Self::Hdf5(dataset) => match dataset.dtype() {
                Datatype::FloatingPoint { size, .. } => Some(*size),
                _ => None,
            },
            Self::Classic(variable) => match variable.kind() {
                Kind::Float => Some(SINGLE_PRECISION_BYTES),
                Kind::Double => Some(DOUBLE_PRECISION_BYTES),
                _ => None,
            },
        }
    }

    fn attribute(&self, member: Member) -> std::result::Result<Option<String>, Fault> {
        let unread = |source| Fault::Hdf5 {
            member,
            op: Access::Attribute,
            source: Box::new(source),
        };
        match self {
            Self::Hdf5(dataset) => {
                let attribute = match dataset.attribute(member.attribute_name()) {
                    Ok(attribute) => attribute,
                    Err(source) if is_missing(&source) => return Ok(None),
                    Err(source) => return Err(unread(source)),
                };
                attribute.read_string().map(Some).map_err(unread)
            }
            Self::Classic(variable) => {
                Ok(variable.text(member.attribute_name()).map(str::to_owned))
            }
        }
    }

    fn numbers(&self, member: Member, picks: &[Pick]) -> std::result::Result<Vec<f64>, Fault> {
        let Some(size) = self.floating_bytes() else {
            return Err(Fault::NotFloating { member });
        };
        match self {
            Self::Hdf5(dataset) => hdf5_numbers(dataset, member, size, picks),
            Self::Classic(variable) => variable
                .numbers(picks)
                .map_err(|source| Fault::Classic { member, source }),
        }
    }

    fn is_bounded(&self, member: Member, at_most: u64) -> std::result::Result<(), Fault> {
        let shape = self.shape();
        let elements = shape
            .iter()
            .try_fold(1_u64, |held, dim| held.checked_mul(*dim));
        match elements {
            Some(elements) if elements <= at_most => Ok(()),
            _ => Err(Fault::ShapeNotUsable {
                member,
                dims: shape,
            }),
        }
    }
}

struct Set<'a> {
    rate: SampleRate,
    impulses: Variable<'a>,
    samples: u64,
    placing: Placing,
    delays: Option<Delays>,
}

enum Placing {
    Measured { directions: Vec<Option<Direction>> },
    Speakers(Room),
}

struct Room {
    views: Vec<Option<Direction>>,
    emitters: Vec<Option<Direction>>,
    points: Vec<Option<Direction>>,
    listener: Option<Listener>,
}

struct Listener {
    rows: u64,
    points: Vec<Option<Direction>>,
}

impl Room {
    fn facing(&self, yaw: f64) -> (u64, f64) {
        if self.views.len() == 1 {
            return (0, yaw);
        }
        let heading = Direction::heard_from(yaw, 0.0);
        self.views
            .iter()
            .enumerate()
            .filter_map(|(index, view)| {
                view.map(|view| (index as u64, view, view.degrees_from(heading)))
            })
            .min_by(|(.., apart), (.., other)| apart.total_cmp(other))
            .map_or((0, yaw), |(index, view, _)| {
                (index, wrapped(yaw - azimuth_of(view)))
            })
    }

    fn emitters_seen_from(&self, view: u64) -> Vec<Option<Direction>> {
        let Some(listener) = &self.listener else {
            return self.emitters.clone();
        };
        let row = if listener.rows == 1 { 0 } else { view as usize };
        let Some(standing) = listener.points.get(row).copied().flatten() else {
            return self.emitters.clone();
        };
        if standing.length() <= LISTENER_AT_THE_ORIGIN_WITHIN {
            return self.emitters.clone();
        }
        self.points
            .iter()
            .map(|point| point.and_then(|point| Direction::along_vector(point.minus(standing))))
            .collect()
    }
}

struct Delays {
    rows: u64,
    samples: Vec<f64>,
}

#[derive(Clone, Copy)]
struct Positions {
    member: Member,
    kind: Member,
    units: Member,
    kind_unsaid: Option<Coordinates>,
    trailing_one: bool,
}

impl Positions {
    const SOURCE: Self = Self {
        member: Member::SourcePosition,
        kind: Member::PositionType,
        units: Member::PositionUnits,
        kind_unsaid: None,
        trailing_one: false,
    };
    const EMITTER: Self = Self {
        member: Member::EmitterPosition,
        kind: Member::EmitterType,
        units: Member::EmitterUnits,
        kind_unsaid: None,
        trailing_one: true,
    };
    const VIEW: Self = Self {
        member: Member::ListenerView,
        kind: Member::ViewType,
        units: Member::ViewUnits,
        kind_unsaid: Some(Coordinates::Cartesian),
        trailing_one: true,
    };
    const LISTENER: Self = Self {
        member: Member::ListenerPosition,
        kind: Member::ListenerPositionType,
        units: Member::ListenerPositionUnits,
        kind_unsaid: Some(Coordinates::Cartesian),
        trailing_one: false,
    };
}

impl<'a> Set<'a> {
    fn of(container: &'a Container) -> std::result::Result<Self, Fault> {
        let rate = rate_of(container)?;

        let impulses = container.locate(Member::Impulses)?;
        let shape = impulses.shape();
        let (measurements, samples, emitters) = match *shape.as_slice() {
            [measurements, EARS, samples] => (measurements, samples, None),
            [measurements, EARS, emitters, samples] => (measurements, samples, Some(emitters)),
            _ => {
                return Err(Fault::ShapeNotUsable {
                    member: Member::Impulses,
                    dims: shape,
                });
            }
        };
        if measurements == 0 || emitters == Some(0) {
            return Err(Fault::NoMeasurements);
        }
        if samples == 0 {
            return Err(Fault::NoSamples);
        }
        if measurements > MEASUREMENTS_AT_MOST {
            return Err(Fault::TooManyMeasurements {
                measurements,
                at_most: MEASUREMENTS_AT_MOST,
            });
        }

        let placing = match emitters {
            None => Placing::Measured {
                directions: directions_of(container, Positions::SOURCE, measurements)?,
            },
            Some(emitters) if emitters > EMITTERS_AT_MOST => {
                return Err(Fault::TooManyEmitters {
                    emitters,
                    at_most: EMITTERS_AT_MOST,
                });
            }
            Some(emitters) => Placing::Speakers(Room {
                views: views_of(container, measurements)?,
                emitters: directions_of(container, Positions::EMITTER, emitters)?,
                points: points_of(container, Positions::EMITTER, emitters)?,
                listener: listener_of(container, measurements)?,
            }),
        };
        let delays = delays_of(container, measurements)?;

        Ok(Self {
            rate,
            impulses,
            samples,
            placing,
            delays,
        })
    }

    fn stereo_pair(
        &self,
        longest: Duration,
        speakers: Speakers,
    ) -> std::result::Result<Hrir, Fault> {
        let most =
            usize::try_from(Frames::from_duration(longest, self.rate).get()).unwrap_or(usize::MAX);
        let frames = usize::try_from(self.samples)
            .unwrap_or(usize::MAX)
            .min(most);
        if frames == 0 {
            return Err(Fault::NoSamples);
        }

        let mut ears = Vec::with_capacity(4);
        for side in [Side::Left, Side::Right] {
            ears.extend(self.ears_of(side, speakers, frames, most)?);
        }

        let length = ears.iter().map(Vec::len).max().unwrap_or(0);
        for ear in &mut ears {
            ear.resize(length, 0.0);
        }
        Ok(Hrir {
            rate: self.rate,
            responses: ears,
        })
    }

    fn ears_of(
        &self,
        side: Side,
        speakers: Speakers,
        frames: usize,
        most: usize,
    ) -> std::result::Result<Vec<Vec<f64>>, Fault> {
        let yaw = f64::from(speakers.yaw.degrees());
        let azimuth = side.azimuth_degrees(speakers.angle);
        let elevation = f64::from(speakers.elevation.degrees());
        match &self.placing {
            Placing::Measured { directions } => {
                let wanted = Direction::heard_from(azimuth - yaw, elevation);
                let neighbours = neighbours_of(directions, wanted, side)?;
                let contributions = self.contributions(&neighbours, None, frames, most)?;
                Ok(blended(contributions, most, Blend::MagnitudeAndDelay))
            }
            Placing::Speakers(room) => {
                let (view, residual) = room.facing(yaw);
                let wanted = Direction::heard_from(azimuth - residual, elevation);
                let emitters = room.emitters_seen_from(view);
                let neighbours = neighbours_of(&emitters, wanted, side)?;
                let contributions = self.contributions(&neighbours, Some(view), frames, most)?;
                Ok(blended(contributions, most, Blend::OnsetsAligned))
            }
        }
    }

    fn contributions(
        &self,
        neighbours: &[(u64, f64)],
        view: Option<u64>,
        frames: usize,
        most: usize,
    ) -> std::result::Result<Contributions, Fault> {
        let ears = Pick::Span {
            start: 0,
            end: EARS,
        };
        let taps = Pick::Span {
            start: 0,
            end: frames as u64,
        };
        let mut contributions = Vec::with_capacity(neighbours.len());
        for (index, weight) in neighbours {
            let (picks, row) = match view {
                None => (vec![Pick::At(*index), ears, taps], *index),
                Some(view) => (vec![Pick::At(view), ears, Pick::At(*index), taps], view),
            };
            contributions.push((*weight, self.responses(&picks, row, frames, most)?));
        }
        Ok(contributions)
    }

    fn responses(
        &self,
        picks: &[Pick],
        row: u64,
        frames: usize,
        most: usize,
    ) -> std::result::Result<Vec<Vec<f64>>, Fault> {
        let values = self.impulses.numbers(Member::Impulses, picks)?;
        if values.len() != frames * EARS as usize {
            return Err(Fault::ShapeNotUsable {
                member: Member::Impulses,
                dims: self.impulses.shape(),
            });
        }

        let delays = self.delays_at(row)?;
        Ok(values
            .chunks_exact(frames)
            .zip(delays)
            .map(|(ear, delay)| {
                let mut response = vec![0.0; delay.min(most)];
                response.extend_from_slice(ear);
                response.truncate(most);
                response
            })
            .collect())
    }

    fn delays_at(&self, measurement: u64) -> std::result::Result<[usize; 2], Fault> {
        let Some(delays) = &self.delays else {
            return Ok([0; 2]);
        };
        let row = if delays.rows == 1 { 0 } else { measurement };
        let start = (row * EARS) as usize;
        let mut samples = [0; 2];
        for (slot, delay) in samples
            .iter_mut()
            .zip(delays.samples.iter().skip(start).take(EARS as usize))
        {
            if !delay.is_finite() {
                return Err(Fault::DelayNotUsable { samples: *delay });
            }
            *slot = delay.round().max(0.0) as usize;
        }
        Ok(samples)
    }
}

fn apart_from(directions: &[Option<Direction>], wanted: Direction) -> Vec<(u64, f64)> {
    let mut apart: Vec<(u64, f64)> = directions
        .iter()
        .enumerate()
        .filter_map(|(index, direction)| {
            direction.map(|direction| (index as u64, direction.degrees_from(wanted)))
        })
        .collect();
    apart.sort_by(|(_, apart), (_, other)| apart.total_cmp(other));
    apart
}

fn nearest_of(
    directions: &[Option<Direction>],
    wanted: Direction,
    side: Side,
) -> std::result::Result<(u64, f64), Fault> {
    directions
        .iter()
        .enumerate()
        .filter_map(|(index, direction)| {
            direction.map(|direction| (index as u64, direction.degrees_from(wanted)))
        })
        .filter(|(_, apart)| *apart <= NEAREST_WITHIN_DEGREES)
        .min_by(|(_, apart), (_, other)| apart.total_cmp(other))
        .ok_or(Fault::NoMeasurementNear {
            side,
            within: NEAREST_WITHIN_DEGREES,
        })
}

fn neighbours_of(
    directions: &[Option<Direction>],
    wanted: Direction,
    side: Side,
) -> std::result::Result<Vec<(u64, f64)>, Fault> {
    let (nearest, distance) = nearest_of(directions, wanted, side)?;
    if distance <= EXACT_WITHIN_DEGREES {
        return Ok(vec![(nearest, 1.0)]);
    }
    if let Some(surrounding) = surrounding(directions, wanted) {
        return Ok(surrounding);
    }
    if let Some(along) = along_the_ring(directions, wanted, nearest) {
        return Ok(along);
    }

    let chosen: Vec<(u64, f64)> = apart_from(directions, wanted)
        .into_iter()
        .take(NEIGHBOURS_AT_MOST)
        .enumerate()
        .filter(|(place, (_, apart))| *place == 0 || *apart <= NEIGHBOURS_WITHIN_DEGREES)
        .map(|(_, held)| held)
        .collect();
    let closeness: Vec<f64> = chosen
        .iter()
        .map(|(_, apart)| 1.0 / (apart * apart))
        .collect();
    let total: f64 = closeness.iter().sum();
    Ok(chosen
        .iter()
        .zip(closeness)
        .map(|((measurement, _), closeness)| (*measurement, closeness / total))
        .collect())
}

fn azimuth_of(direction: Direction) -> f64 {
    direction.left.atan2(direction.front).to_degrees()
}

fn elevation_of(direction: Direction) -> f64 {
    (direction.up / direction.length())
        .clamp(-1.0, 1.0)
        .asin()
        .to_degrees()
}

fn wrapped(degrees: f64) -> f64 {
    (degrees + HALF_TURN_DEGREES).rem_euclid(FULL_TURN_DEGREES) - HALF_TURN_DEGREES
}

fn along_the_ring(
    directions: &[Option<Direction>],
    wanted: Direction,
    nearest: u64,
) -> Option<Vec<(u64, f64)>> {
    let ring = directions.get(usize::try_from(nearest).ok()?).copied()??;
    let ring_elevation = elevation_of(ring);
    if ring_elevation.abs() > RING_POLE_ELEVATION_DEGREES {
        return None;
    }

    let wanted_azimuth = azimuth_of(wanted);
    let mut before: Option<(u64, f64)> = None;
    let mut after: Option<(u64, f64)> = None;
    for (index, direction) in directions.iter().enumerate() {
        let Some(direction) = direction else {
            continue;
        };
        if (elevation_of(*direction) - ring_elevation).abs() > RING_ELEVATION_WITHIN_DEGREES {
            continue;
        }
        let turn = wrapped(azimuth_of(*direction) - wanted_azimuth);
        let (held, apart) = if turn < 0.0 {
            (&mut before, -turn)
        } else {
            (&mut after, turn)
        };
        if held.is_none_or(|(_, nearest)| apart < nearest) {
            *held = Some((index as u64, apart));
        }
    }

    let ((first, behind), (second, ahead)) = before.zip(after)?;
    let gap = behind + ahead;
    if gap > RING_GAP_AT_MOST_DEGREES {
        return None;
    }
    Some(
        [(first, ahead / gap), (second, behind / gap)]
            .into_iter()
            .filter(|(_, weight)| *weight > WEIGHT_WORTH_READING)
            .collect(),
    )
}

#[derive(Clone, Copy)]
struct Corner {
    measurement: u64,
    direction: Direction,
    flat: [f64; 2],
}

struct Tangent {
    first: Direction,
    second: Direction,
    toward: Direction,
}

impl Tangent {
    fn at(toward: Direction) -> Self {
        let axis = if toward.up.abs() > POLE_NEAR {
            FRONT
        } else {
            UP
        };
        let first = axis.cross(toward).unit();
        let second = toward.cross(first);
        Self {
            first,
            second,
            toward,
        }
    }

    fn flatten(&self, direction: Direction) -> Option<[f64; 2]> {
        let depth = direction.dot(self.toward);
        (depth > IN_FRONT_OF_THE_TANGENT_PLANE_AT_LEAST).then(|| {
            [
                direction.dot(self.first) / depth,
                direction.dot(self.second) / depth,
            ]
        })
    }
}

fn barycentric(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Option<[f64; 3]> {
    let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    if area.abs() < TRIANGLE_AREA_AT_LEAST {
        return None;
    }
    let turn = |from: [f64; 2], to: [f64; 2]| (from[0] * to[1] - from[1] * to[0]) / area;
    let weights = [turn(b, c), turn(c, a), turn(a, b)];
    weights
        .iter()
        .all(|weight| *weight >= INSIDE_BY_AT_LEAST)
        .then_some(weights)
}

fn cap_around(corners: [Direction; 3]) -> Option<(Direction, f64)> {
    let [a, b, c] = corners;
    let centre = b.minus(a).cross(c.minus(a));
    if centre.length() <= f64::EPSILON {
        return None;
    }
    let centre = centre.unit();
    let (centre, cosine) = match centre.dot(a) {
        cosine if cosine < 0.0 => (centre.reversed(), -cosine),
        cosine => (centre, cosine),
    };
    (cosine > 0.0).then_some((centre, cosine))
}

fn surrounding(directions: &[Option<Direction>], wanted: Direction) -> Option<Vec<(u64, f64)>> {
    let tangent = Tangent::at(wanted);
    let corners: Vec<Corner> = apart_from(directions, wanted)
        .into_iter()
        .take_while(|(_, apart)| *apart <= TRIANGLE_WITHIN_DEGREES)
        .take(TRIANGLE_CANDIDATES_AT_MOST)
        .filter_map(|(measurement, _)| {
            let direction = directions
                .get(usize::try_from(measurement).ok()?)
                .copied()??;
            Some(Corner {
                measurement,
                direction,
                flat: tangent.flatten(direction)?,
            })
        })
        .collect();

    let mut best: Option<(f64, [usize; 3], [f64; 3])> = None;
    for i in 0..corners.len() {
        for j in i + 1..corners.len() {
            for k in j + 1..corners.len() {
                let Some(weights) = barycentric(corners[i].flat, corners[j].flat, corners[k].flat)
                else {
                    continue;
                };
                let Some((centre, cosine)) = cap_around([
                    corners[i].direction,
                    corners[j].direction,
                    corners[k].direction,
                ]) else {
                    continue;
                };
                let crowded = corners.iter().enumerate().any(|(other, corner)| {
                    ![i, j, k].contains(&other)
                        && corner.direction.dot(centre) > cosine + CAP_TOLERANCE
                });
                if crowded {
                    continue;
                }
                if best.is_none_or(|(held, ..)| cosine > held) {
                    best = Some((cosine, [i, j, k], weights));
                }
            }
        }
    }

    let (_, places, weights) = best?;
    let chosen: Vec<(u64, f64)> = places
        .iter()
        .zip(weights)
        .filter(|(_, weight)| *weight > WEIGHT_WORTH_READING)
        .map(|(place, weight)| (corners[*place].measurement, weight))
        .collect();
    let total: f64 = chosen.iter().map(|(_, weight)| weight).sum();
    Some(
        chosen
            .into_iter()
            .map(|(measurement, weight)| (measurement, weight / total))
            .collect(),
    )
}

type Contributions = Vec<(f64, Vec<Vec<f64>>)>;

#[derive(Clone, Copy)]
enum Blend {
    OnsetsAligned,
    MagnitudeAndDelay,
}

fn blended(mut contributions: Contributions, most: usize, blend: Blend) -> Vec<Vec<f64>> {
    if contributions.len() == 1 {
        return contributions
            .pop()
            .map(|(_, ears)| ears)
            .unwrap_or_default();
    }
    (0..EARS as usize)
        .map(|ear| {
            let parts: Vec<(f64, &[f64])> = contributions
                .iter()
                .map(|(weight, ears)| (*weight, ears[ear].as_slice()))
                .collect();
            match blend {
                Blend::OnsetsAligned => aligned_sum(&parts, most),
                Blend::MagnitudeAndDelay => magnitude_and_delay(&parts, most),
            }
        })
        .collect()
}

fn onset_of(response: &[f64]) -> usize {
    let peak = response
        .iter()
        .fold(0.0_f64, |peak, tap| peak.max(tap.abs()));
    if peak == 0.0 {
        return 0;
    }
    let threshold = peak * ONSET_AT_FRACTION_OF_PEAK;
    response
        .iter()
        .position(|tap| tap.abs() >= threshold)
        .unwrap_or(0)
}

fn aligned_sum(parts: &[(f64, &[f64])], most: usize) -> Vec<f64> {
    let onsets: Vec<usize> = parts
        .iter()
        .map(|(_, response)| onset_of(response))
        .collect();
    let target = parts
        .iter()
        .zip(&onsets)
        .map(|((weight, _), onset)| weight * *onset as f64)
        .sum::<f64>()
        .round() as isize;
    let shifts: Vec<isize> = onsets
        .iter()
        .map(|onset| target - *onset as isize)
        .collect();

    let length = parts
        .iter()
        .zip(&shifts)
        .map(|((_, response), shift)| (response.len() as isize + shift).max(0) as usize)
        .max()
        .unwrap_or(0)
        .min(most);
    let mut sum = vec![0.0; length];
    for ((weight, response), shift) in parts.iter().zip(&shifts) {
        for (index, tap) in response.iter().enumerate() {
            let at = index as isize + shift;
            if (0..length as isize).contains(&at) {
                sum[at as usize] += weight * tap;
            }
        }
    }
    sum
}

fn magnitude_and_delay(parts: &[(f64, &[f64])], most: usize) -> Vec<f64> {
    let onsets: Vec<usize> = parts
        .iter()
        .map(|(_, response)| onset_of(response))
        .collect();
    let target = parts
        .iter()
        .zip(&onsets)
        .map(|((weight, _), onset)| weight * *onset as f64)
        .sum::<f64>()
        .round() as isize;
    let length = parts
        .iter()
        .zip(&onsets)
        .map(|((_, response), onset)| response.len().saturating_sub(*onset))
        .max()
        .unwrap_or(0)
        .saturating_add(target.max(0) as usize)
        .min(most);

    let longest = parts
        .iter()
        .map(|(_, response)| response.len())
        .max()
        .unwrap_or(0);
    let size = (longest * SPECTRUM_PADDING).next_power_of_two();
    let magnitude = blended_magnitude(parts, size);
    let Some(rebuilt) = minimum_phase(&magnitude) else {
        return vec![0.0; length];
    };

    let shift = target - onset_of(&rebuilt) as isize;
    let mut sum = vec![0.0; length];
    for (index, tap) in rebuilt.iter().enumerate() {
        let at = index as isize + shift;
        if (0..length as isize).contains(&at) {
            sum[at as usize] = *tap;
        }
    }
    sum
}

fn blended_magnitude(parts: &[(f64, &[f64])], size: usize) -> Vec<f64> {
    let forward = FftPlanner::<f64>::new().plan_fft_forward(size);
    let mut magnitude = vec![0.0; size];
    for (weight, response) in parts {
        let mut spectrum: Vec<Complex<f64>> = response
            .iter()
            .map(|tap| Complex::new(*tap, 0.0))
            .chain(std::iter::repeat(Complex::new(0.0, 0.0)))
            .take(size)
            .collect();
        forward.process(&mut spectrum);
        for (held, bin) in magnitude.iter_mut().zip(&spectrum) {
            *held += weight * bin.norm();
        }
    }
    magnitude
}

fn minimum_phase(magnitude: &[f64]) -> Option<Vec<f64>> {
    let size = magnitude.len();
    let loudest = magnitude.iter().copied().fold(0.0_f64, f64::max);
    if loudest <= 0.0 || !loudest.is_finite() {
        return None;
    }
    let floor = loudest * SPECTRUM_FLOOR;

    let mut planner = FftPlanner::<f64>::new();
    let forward = planner.plan_fft_forward(size);
    let inverse = planner.plan_fft_inverse(size);
    let scale = 1.0 / size as f64;

    let mut cepstrum: Vec<Complex<f64>> = magnitude
        .iter()
        .map(|bin| Complex::new(bin.max(floor).ln(), 0.0))
        .collect();
    inverse.process(&mut cepstrum);

    let half = size / 2;
    let mut folded = vec![Complex::new(0.0, 0.0); size];
    folded[0] = cepstrum[0] * scale;
    for place in 1..half {
        folded[place] = cepstrum[place] * (2.0 * scale);
    }
    folded[half] = cepstrum[half] * scale;
    forward.process(&mut folded);

    let mut response: Vec<Complex<f64>> = folded.iter().map(|bin| bin.exp()).collect();
    inverse.process(&mut response);
    Some(response.iter().map(|tap| tap.re * scale).collect())
}

fn rate_of(container: &Container) -> std::result::Result<SampleRate, Fault> {
    let variable = container.locate(Member::SamplingRate)?;
    variable.is_bounded(Member::SamplingRate, RATES_AT_MOST)?;
    let hertz = variable
        .numbers(Member::SamplingRate, &[])?
        .first()
        .copied()
        .ok_or_else(|| Fault::ShapeNotUsable {
            member: Member::SamplingRate,
            dims: variable.shape(),
        })?;

    let whole = hertz.round();
    if !(f64::from(SampleRate::MIN_HZ)..=f64::from(SampleRate::MAX_HZ)).contains(&whole) {
        return Err(Fault::RateNotUsable { hertz });
    }
    SampleRate::new(whole as u32).map_err(|_| Fault::RateNotUsable { hertz })
}

fn directions_of(
    container: &Container,
    positions: Positions,
    rows: u64,
) -> std::result::Result<Vec<Option<Direction>>, Fault> {
    let variable = container.locate(positions.member)?;
    directions_in(&variable, positions, rows)
}

#[derive(Clone, Copy)]
enum Reach {
    Direction,
    Point,
}

fn directions_in(
    variable: &Variable<'_>,
    positions: Positions,
    rows: u64,
) -> std::result::Result<Vec<Option<Direction>>, Fault> {
    positions_in(variable, positions, rows, Reach::Direction)
}

fn positions_in(
    variable: &Variable<'_>,
    positions: Positions,
    rows: u64,
    reach: Reach,
) -> std::result::Result<Vec<Option<Direction>>, Fault> {
    let shape = variable.shape();
    let held = match *shape.as_slice() {
        [held, COORDINATES] => held == rows,
        [held, COORDINATES, 1] => held == rows && positions.trailing_one,
        _ => false,
    };
    if !held {
        return Err(Fault::ShapeNotUsable {
            member: positions.member,
            dims: shape,
        });
    }

    let coordinates = Coordinates::of(variable, positions)?;
    let values = variable.numbers(positions.member, &[])?;
    Ok(values
        .as_chunks::<{ COORDINATES as usize }>()
        .0
        .iter()
        .map(|position| match reach {
            Reach::Direction => coordinates.direction(position),
            Reach::Point => coordinates.point(position),
        })
        .collect())
}

fn views_of(
    container: &Container,
    measurements: u64,
) -> std::result::Result<Vec<Option<Direction>>, Fault> {
    let Some(variable) = container.find(Member::ListenerView)? else {
        return Ok(vec![None]);
    };
    let rows = match *variable.shape().as_slice() {
        [rows, COORDINATES] | [rows, COORDINATES, 1] if rows == 1 || rows == measurements => rows,
        _ => {
            return Err(Fault::ShapeNotUsable {
                member: Member::ListenerView,
                dims: variable.shape(),
            });
        }
    };
    if rows == 1 {
        return Ok(vec![None]);
    }

    directions_in(&variable, Positions::VIEW, rows)
}

fn points_of(
    container: &Container,
    positions: Positions,
    rows: u64,
) -> std::result::Result<Vec<Option<Direction>>, Fault> {
    let variable = container.locate(positions.member)?;
    positions_in(&variable, positions, rows, Reach::Point)
}

fn listener_of(
    container: &Container,
    measurements: u64,
) -> std::result::Result<Option<Listener>, Fault> {
    let Some(variable) = container.find(Member::ListenerPosition)? else {
        return Ok(None);
    };
    let rows = match *variable.shape().as_slice() {
        [rows, COORDINATES] if rows == 1 || rows == measurements => rows,
        _ => {
            return Err(Fault::ShapeNotUsable {
                member: Member::ListenerPosition,
                dims: variable.shape(),
            });
        }
    };
    let points = positions_in(&variable, Positions::LISTENER, rows, Reach::Point)?;
    Ok(Some(Listener { rows, points }))
}

fn delays_of(
    container: &Container,
    measurements: u64,
) -> std::result::Result<Option<Delays>, Fault> {
    let Some(variable) = container.find(Member::Delay)? else {
        return Ok(None);
    };
    let shape = variable.shape();
    let rows = match *shape.as_slice() {
        [EARS] => 1,
        [rows, EARS] if rows == 1 || rows == measurements => rows,
        _ => {
            return Err(Fault::ShapeNotUsable {
                member: Member::Delay,
                dims: shape,
            });
        }
    };
    let samples = variable.numbers(Member::Delay, &[])?;
    Ok(Some(Delays { rows, samples }))
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Coordinates {
    Spherical { radians: bool },
    Cartesian,
}

impl Coordinates {
    fn of(variable: &Variable<'_>, positions: Positions) -> std::result::Result<Self, Fault> {
        let Some(kind) = variable.attribute(positions.kind)? else {
            return positions.kind_unsaid.ok_or(Fault::MemberMissing {
                member: positions.kind,
            });
        };
        match kind.trim().to_ascii_lowercase().as_str() {
            "spherical" => {
                let units = variable.attribute(positions.units)?.unwrap_or_default();
                let angle = units.split(',').next().unwrap_or_default();
                let radians = angle.trim().to_ascii_lowercase().starts_with(RADIANS);
                Ok(Self::Spherical { radians })
            }
            "cartesian" => Ok(Self::Cartesian),
            _ => Err(Fault::PositionTypeUnknown),
        }
    }

    fn point(self, position: &[f64; COORDINATES as usize]) -> Option<Direction> {
        let &[first, second, third] = position;
        match self {
            Self::Spherical { radians } => {
                let (azimuth, elevation) = if radians {
                    (first.to_degrees(), second.to_degrees())
                } else {
                    (first, second)
                };
                let heard = Direction::heard_from(azimuth, elevation);
                Some(Direction {
                    front: heard.front * third,
                    left: heard.left * third,
                    up: heard.up * third,
                })
            }
            Self::Cartesian => Some(Direction {
                front: first,
                left: second,
                up: third,
            }),
        }
        .filter(|point| point.is_finite())
    }

    fn direction(self, position: &[f64; COORDINATES as usize]) -> Option<Direction> {
        let &[first, second, third] = position;
        match self {
            Self::Spherical { radians: false } => Some(Direction::heard_from(first, second)),
            Self::Spherical { radians: true } => Some(Direction::heard_from(
                first.to_degrees(),
                second.to_degrees(),
            )),
            Self::Cartesian => Direction::along(first, second, third),
        }
        .filter(|direction| direction.is_finite())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Direction {
    front: f64,
    left: f64,
    up: f64,
}

impl Direction {
    fn heard_from(azimuth_degrees: f64, elevation_degrees: f64) -> Self {
        let (azimuth, elevation) = (azimuth_degrees.to_radians(), elevation_degrees.to_radians());
        Self {
            front: elevation.cos() * azimuth.cos(),
            left: elevation.cos() * azimuth.sin(),
            up: elevation.sin(),
        }
    }

    fn along_vector(vector: Self) -> Option<Self> {
        Self::along(vector.front, vector.left, vector.up)
    }

    fn along(front: f64, left: f64, up: f64) -> Option<Self> {
        let length = (front * front + left * left + up * up).sqrt();
        (length.is_finite() && length > f64::EPSILON).then(|| Self {
            front: front / length,
            left: left / length,
            up: up / length,
        })
    }

    fn dot(self, other: Self) -> f64 {
        self.front * other.front + self.left * other.left + self.up * other.up
    }

    fn cross(self, other: Self) -> Self {
        Self {
            front: self.left * other.up - self.up * other.left,
            left: self.up * other.front - self.front * other.up,
            up: self.front * other.left - self.left * other.front,
        }
    }

    fn minus(self, other: Self) -> Self {
        Self {
            front: self.front - other.front,
            left: self.left - other.left,
            up: self.up - other.up,
        }
    }

    fn reversed(self) -> Self {
        Self {
            front: -self.front,
            left: -self.left,
            up: -self.up,
        }
    }

    fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    fn unit(self) -> Self {
        let length = self.length();
        Self {
            front: self.front / length,
            left: self.left / length,
            up: self.up / length,
        }
    }

    fn is_finite(self) -> bool {
        self.front.is_finite() && self.left.is_finite() && self.up.is_finite()
    }

    fn degrees_from(self, other: Self) -> f64 {
        let alike = self.dot(other);
        alike.clamp(-1.0, 1.0).acos().to_degrees()
    }
}

fn hdf5_numbers(
    dataset: &Dataset,
    member: Member,
    size: u8,
    picks: &[Pick],
) -> std::result::Result<Vec<f64>, Fault> {
    let selection = (!picks.is_empty()).then(|| SliceInfo {
        selections: picks
            .iter()
            .map(|pick| match *pick {
                Pick::At(index) => SliceInfoElem::Index(index),
                Pick::Span { start, end } => SliceInfoElem::Slice {
                    start,
                    end,
                    step: 1,
                },
            })
            .collect(),
    });
    let unread = |source| Fault::Hdf5 {
        member,
        op: Access::Read,
        source: Box::new(source),
    };
    match size {
        SINGLE_PRECISION_BYTES => {
            let array = match &selection {
                Some(selection) => dataset.read_slice::<f32>(selection),
                None => dataset.read_array::<f32>(),
            }
            .map_err(unread)?;
            Ok(array.iter().map(|value| f64::from(*value)).collect())
        }
        DOUBLE_PRECISION_BYTES => {
            let array = match &selection {
                Some(selection) => dataset.read_slice::<f64>(selection),
                None => dataset.read_array::<f64>(),
            }
            .map_err(unread)?;
            Ok(array.iter().copied().collect())
        }
        _ => Err(Fault::NotFloating { member }),
    }
}

fn is_missing(error: &hdf5_reader::error::Error) -> bool {
    match error {
        hdf5_reader::error::Error::DatasetNotFound(_)
        | hdf5_reader::error::Error::GroupNotFound(_)
        | hdf5_reader::error::Error::AttributeNotFound(_) => true,
        hdf5_reader::error::Error::Context { source, .. } => is_missing(source),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::{env, fs, path::Path, process};

    use hdf5_writer::{
        AttributeBuilder, DatasetBuilder, FILTER_DEFLATE, FilterDescription, Hdf5Builder,
        WriteOptions,
    };

    use super::*;
    use crate::netcdf::written;

    const LONG_ENOUGH: Duration = Duration::from_secs(10);
    const STANDARD: Speakers = Speakers::STANDARD;
    const FRAMES: usize = 6;

    #[derive(Clone, Copy)]
    enum Positions {
        Spherical,
        Cartesian,
        Radians,
    }

    struct Fixture {
        rate: f64,
        directions: Vec<(f64, f64)>,
        positions: Positions,
        kind: Option<&'static str>,
        ears: u64,
        samples: usize,
        single: bool,
        compressed: bool,
        delay: Option<Vec<f64>>,
        taps: Option<fn(usize, usize, usize) -> f64>,
    }

    impl Fixture {
        fn new(directions: &[(f64, f64)]) -> Self {
            Self {
                rate: 48_000.0,
                directions: directions.to_vec(),
                positions: Positions::Spherical,
                kind: Some("spherical"),
                ears: 2,
                samples: FRAMES,
                single: false,
                compressed: false,
                delay: None,
                taps: None,
            }
        }

        fn measurements(&self) -> u64 {
            self.directions.len() as u64
        }

        fn tap(measurement: usize, ear: usize, frame: usize) -> f64 {
            (measurement * 100 + ear * 10 + frame + 1) as f64 / 1000.0
        }

        fn tap_at(&self, measurement: usize, ear: usize, frame: usize) -> f64 {
            self.taps.map_or_else(
                || Self::tap(measurement, ear, frame),
                |taps| taps(measurement, ear, frame),
            )
        }

        fn classic_bytes(&self, version: u8, records: bool) -> Vec<u8> {
            let measurements = self.measurements() as u32;
            let samples = self.samples as u32;
            let first = if records { 0 } else { measurements };
            let mut variables = vec![
                written::Column {
                    name: "Data.IR",
                    dimensions: vec![0, 1, 2],
                    attributes: Vec::new(),
                    doubles: !self.single,
                    values: self.impulses(),
                },
                written::Column {
                    name: "SourcePosition",
                    dimensions: vec![0, 3],
                    attributes: vec![
                        ("Type", self.kind.unwrap_or("spherical").to_owned()),
                        ("Units", "degree, degree, metre".to_owned()),
                    ],
                    doubles: true,
                    values: self.positions(),
                },
                written::Column {
                    name: "Data.SamplingRate",
                    dimensions: vec![4],
                    attributes: Vec::new(),
                    doubles: true,
                    values: vec![self.rate],
                },
            ];
            if let Some(delay) = &self.delay {
                variables.push(written::Column {
                    name: "Data.Delay",
                    dimensions: vec![5, 1],
                    attributes: Vec::new(),
                    doubles: true,
                    values: delay.clone(),
                });
            }
            let delays = self.delay.as_ref().map_or(1, |delay| delay.len() / 2) as u32;
            written::File {
                version,
                dimensions: vec![
                    ("M", first),
                    ("R", self.ears as u32),
                    ("N", samples),
                    ("C", 3),
                    ("I", 1),
                    ("D", delays),
                ],
                records: if records { measurements } else { 0 },
                variables,
            }
            .bytes()
        }

        fn impulses(&self) -> Vec<f64> {
            let mut values = Vec::new();
            for measurement in 0..self.directions.len() {
                for ear in 0..self.ears as usize {
                    for frame in 0..self.samples {
                        values.push(self.tap_at(measurement, ear, frame));
                    }
                }
            }
            values
        }

        fn positions(&self) -> Vec<f64> {
            let mut values = Vec::new();
            for (azimuth, elevation) in &self.directions {
                match self.positions {
                    Positions::Spherical => values.extend([*azimuth, *elevation, 1.0]),
                    Positions::Radians => {
                        values.extend([azimuth.to_radians(), elevation.to_radians(), 1.0])
                    }
                    Positions::Cartesian => {
                        let heard = Direction::heard_from(*azimuth, *elevation);
                        values.extend([heard.front * 2.0, heard.left * 2.0, heard.up * 2.0]);
                    }
                }
            }
            values
        }

        fn bytes(&self) -> Vec<u8> {
            let shape = vec![self.measurements(), self.ears, self.samples as u64];
            let mut impulses = if self.single {
                let narrowed: Vec<f32> = self.impulses().iter().map(|tap| *tap as f32).collect();
                DatasetBuilder::typed_data("Data.IR", shape, &narrowed).expect("impulses")
            } else {
                DatasetBuilder::typed_data("Data.IR", shape, &self.impulses()).expect("impulses")
            };
            if self.compressed {
                impulses = impulses
                    .chunked(vec![1, self.ears, self.samples as u64])
                    .filter(FilterDescription {
                        id: FILTER_DEFLATE,
                        name: None,
                        client_data: vec![6],
                    });
            }

            let units = match self.positions {
                Positions::Spherical => "degree, degree, metre",
                Positions::Radians => "radian, radian, metre",
                Positions::Cartesian => "metre, metre, metre",
            };
            let mut positions = DatasetBuilder::typed_data(
                "SourcePosition",
                vec![self.measurements(), 3],
                &self.positions(),
            )
            .expect("positions")
            .attribute(AttributeBuilder::fixed_string("Units", units));
            if let Some(kind) = self.kind {
                positions = positions.attribute(AttributeBuilder::fixed_string("Type", kind));
            }

            let mut plan = Hdf5Builder::new()
                .dataset(impulses)
                .dataset(positions)
                .dataset(
                    DatasetBuilder::typed_data("Data.SamplingRate", vec![1], &[self.rate])
                        .expect("rate"),
                );
            if let Some(delay) = &self.delay {
                plan = plan.dataset(
                    DatasetBuilder::typed_data(
                        "Data.Delay",
                        vec![(delay.len() / 2) as u64, 2],
                        delay,
                    )
                    .expect("delay"),
                );
            }
            plan.into_plan()
                .expect("a plan")
                .encode(WriteOptions::default())
                .expect("bytes")
        }

        fn read(&self) -> std::result::Result<Option<Hrir>, Fault> {
            read_hrir_bytes(self.bytes(), LONG_ENOUGH, STANDARD)
        }

        fn expected(&self, left: usize, right: usize) -> Vec<Vec<f64>> {
            [(left, 0), (left, 1), (right, 0), (right, 1)]
                .iter()
                .map(|(measurement, ear)| {
                    (0..self.samples)
                        .map(|frame| self.tap_at(*measurement, *ear, frame))
                        .collect()
                })
                .collect()
        }
    }

    const SURROUND: [(f64, f64); 6] = [
        (0.0, 0.0),
        (25.0, 2.0),
        (90.0, 0.0),
        (335.0, 0.0),
        (180.0, 0.0),
        (30.0, 60.0),
    ];

    #[test]
    fn the_measurement_nearest_each_speaker_is_the_one_whose_taps_are_taken() {
        let fixture = Fixture::new(&SURROUND);

        let hrir = fixture.read().expect("a set").expect("a SOFA file");

        assert_eq!(hrir.rate, SampleRate::HZ_48000);
        assert_eq!(hrir.responses, fixture.expected(1, 3));
    }

    #[test]
    fn a_measurement_exactly_at_the_speaker_beats_one_a_little_off() {
        let fixture = Fixture::new(&[(40.0, 0.0), (30.0, 0.0), (20.0, 0.0), (330.0, 0.0)]);

        let hrir = fixture.read().expect("a set").expect("a SOFA file");

        assert_eq!(hrir.responses, fixture.expected(1, 3));
    }

    #[test]
    fn cartesian_positions_choose_as_spherical_ones_do() {
        let mut fixture = Fixture::new(&SURROUND);
        fixture.positions = Positions::Cartesian;
        fixture.kind = Some("cartesian");

        let hrir = fixture.read().expect("a set").expect("a SOFA file");

        assert_eq!(hrir.responses, fixture.expected(1, 3));
    }

    #[test]
    fn angles_in_radians_choose_as_degrees_do() {
        let mut fixture = Fixture::new(&SURROUND);
        fixture.positions = Positions::Radians;

        let hrir = fixture.read().expect("a set").expect("a SOFA file");

        assert_eq!(hrir.responses, fixture.expected(1, 3));
    }

    #[test]
    fn single_precision_and_compressed_chunks_read_as_the_plain_double_set_does() {
        let plain = Fixture::new(&SURROUND);
        let mut single = Fixture::new(&SURROUND);
        single.single = true;
        let mut compressed = Fixture::new(&SURROUND);
        compressed.compressed = true;

        let wanted = plain.read().expect("a set").expect("a SOFA file");
        let narrowed = single.read().expect("a set").expect("a SOFA file");
        let inflated = compressed.read().expect("a set").expect("a SOFA file");

        assert_eq!(inflated, wanted);
        assert_eq!(narrowed.rate, wanted.rate);
        for (heard, plain) in narrowed.responses.iter().zip(&wanted.responses) {
            for (heard, plain) in heard.iter().zip(plain) {
                assert!((heard - plain).abs() < 1e-7);
            }
        }
    }

    #[test]
    fn a_rate_that_is_not_whole_is_rounded_and_one_no_stream_runs_at_is_refused() {
        let mut fixture = Fixture::new(&SURROUND);
        fixture.rate = 44_100.000_000_1;
        let rounded = fixture.read().expect("a set").expect("a SOFA file");

        let refusals: Vec<_> = [0.0, -48_000.0, f64::NAN, 4.0e9, 1.0]
            .iter()
            .map(|rate| {
                fixture.rate = *rate;
                fixture.read()
            })
            .collect();

        assert_eq!(rounded.rate, SampleRate::HZ_44100);
        for refused in refusals {
            assert!(matches!(refused, Err(Fault::RateNotUsable { .. })));
        }
    }

    #[test]
    fn a_delay_puts_silent_samples_before_an_ear_and_every_response_stays_as_long_as_the_rest() {
        let mut fixture = Fixture::new(&SURROUND);
        fixture.delay = Some(vec![0.0, 3.2]);

        let hrir = fixture.read().expect("a set").expect("a SOFA file");

        let plain = fixture.expected(1, 3);
        assert!(hrir.responses.iter().all(|ear| ear.len() == FRAMES + 3));
        for (index, ear) in hrir.responses.iter().enumerate() {
            let padding = if index % 2 == 1 { 3 } else { 0 };
            assert!(ear[..padding].iter().all(|tap| *tap == 0.0));
            assert_eq!(&ear[padding..padding + FRAMES], plain[index].as_slice());
            assert!(ear[padding + FRAMES..].iter().all(|tap| *tap == 0.0));
        }
    }

    #[test]
    fn a_delay_for_each_measurement_is_read_by_the_measurement_chosen() {
        let mut fixture = Fixture::new(&[(30.0, 0.0), (330.0, 0.0)]);
        fixture.delay = Some(vec![0.0, 0.0, 2.0, 2.0]);

        let hrir = fixture.read().expect("a set").expect("a SOFA file");

        assert!(hrir.responses[0].iter().take(2).all(|tap| *tap != 0.0));
        assert!(hrir.responses[2].iter().take(2).all(|tap| *tap == 0.0));
    }

    #[test]
    fn a_delay_that_is_not_a_number_is_refused() {
        let mut fixture = Fixture::new(&SURROUND);
        fixture.delay = Some(vec![f64::NAN, 0.0]);

        assert!(matches!(fixture.read(), Err(Fault::DelayNotUsable { .. })));
    }

    #[test]
    fn responses_are_cut_at_the_longest_asked_for() {
        let mut fixture = Fixture::new(&SURROUND);
        fixture.samples = 100;

        let hrir = read_hrir_bytes(fixture.bytes(), Duration::from_millis(1), STANDARD)
            .expect("a set")
            .expect("a SOFA file");

        assert!(hrir.responses.iter().all(|ear| ear.len() == 48));
        assert_eq!(hrir.responses[0][0], Fixture::tap(1, 0, 0));
        assert_eq!(hrir.responses[3][47], Fixture::tap(3, 1, 47));
    }

    #[test]
    fn a_set_with_nothing_near_a_speaker_is_refused_naming_the_speaker() {
        let fixture = Fixture::new(&[(180.0, 0.0), (90.0, 0.0), (300.0, 80.0)]);

        let refused = fixture.read();

        assert!(matches!(
            refused,
            Err(Fault::NoMeasurementNear {
                side: Side::Left,
                ..
            })
        ));
    }

    #[test]
    fn a_set_that_is_not_two_ears_over_samples_is_refused_with_its_shape() {
        let mut fixture = Fixture::new(&SURROUND);
        fixture.ears = 1;

        let refused = fixture.read();

        match refused {
            Err(Fault::ShapeNotUsable { member, dims }) => {
                assert_eq!(member, Member::Impulses);
                assert_eq!(dims, vec![6, 1, FRAMES as u64]);
            }
            other => panic!("expected a shape refusal, got {other:?}"),
        }
    }

    #[test]
    fn positions_of_a_kind_no_one_knows_or_none_at_all_are_refused() {
        let mut unknown = Fixture::new(&SURROUND);
        unknown.kind = Some("polar");
        let mut unsaid = Fixture::new(&SURROUND);
        unsaid.kind = None;

        assert!(matches!(unknown.read(), Err(Fault::PositionTypeUnknown)));
        assert!(matches!(
            unsaid.read(),
            Err(Fault::MemberMissing {
                member: Member::PositionType
            })
        ));
    }

    #[test]
    fn bytes_that_are_not_a_container_of_this_kind_are_not_a_set() {
        assert!(
            read_hrir_bytes(Vec::new(), LONG_ENOUGH, STANDARD)
                .expect("no fault")
                .is_none()
        );
        assert!(
            read_hrir_bytes(b"RIFF....WAVEfmt ".to_vec(), LONG_ENOUGH, STANDARD)
                .expect("no fault")
                .is_none()
        );
        assert!(matches!(
            read_hrir_bytes(SIGNATURE.to_vec(), LONG_ENOUGH, STANDARD),
            Err(Fault::Open { .. })
        ));
    }

    #[test]
    fn a_set_cut_short_or_marked_anywhere_is_refused_or_read_and_never_panics() {
        let whole = Fixture::new(&SURROUND).bytes();

        for length in 0..whole.len() {
            let _ = read_hrir_bytes(whole[..length].to_vec(), LONG_ENOUGH, STANDARD);
        }
        for index in 0..whole.len() {
            for pattern in [0x00, 0xff, 0x80] {
                let mut marked = whole.clone();
                marked[index] = pattern;
                let _ = read_hrir_bytes(marked, LONG_ENOUGH, STANDARD);
            }
        }
    }

    #[test]
    fn a_file_is_read_through_the_sources_and_one_of_another_kind_is_left_to_the_decoders() {
        let folder = env::temp_dir().join(format!("resonate-sofa-{}", process::id()));
        fs::create_dir_all(&folder).expect("a scratch folder");
        let set = folder.join("set.sofa");
        let other = folder.join("other.wav");
        fs::write(&set, Fixture::new(&SURROUND).bytes()).expect("a set");
        fs::write(&other, b"RIFF\0\0\0\0WAVE").expect("a wave");
        let sources = Sources::local();

        let read = read_hrir(&sources, &MediaLocation::local(&set), LONG_ENOUGH, STANDARD);
        let left = read_hrir(
            &sources,
            &MediaLocation::local(&other),
            LONG_ENOUGH,
            STANDARD,
        );
        let broken = {
            let mut bytes = fs::read(&set).expect("the set");
            bytes.truncate(40);
            fs::write(&set, bytes).expect("a cut set");
            read_hrir(&sources, &MediaLocation::local(&set), LONG_ENOUGH, STANDARD)
        };
        let _ = fs::remove_dir_all(&folder);

        assert_eq!(
            read.expect("a set").expect("a SOFA file").responses.len(),
            4
        );
        assert!(left.expect("no fault").is_none());
        assert!(matches!(broken, Err(Error::Sofa { .. })));
    }

    #[test]
    fn every_seed_of_the_fuzz_target_is_a_set_that_reads() {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/seeds/sofa");
        let seeds: Vec<_> = fs::read_dir(&folder)
            .expect("the seeds")
            .map(|entry| entry.expect("a seed").path())
            .collect();

        assert!(!seeds.is_empty());
        for seed in seeds {
            let read = read_hrir_bytes(fs::read(&seed).expect("a seed"), LONG_ENOUGH, STANDARD);

            let hrir = read.expect("a set").expect("a SOFA file");
            assert_eq!(hrir.rate, SampleRate::HZ_48000, "{}", seed.display());
            assert_eq!(hrir.responses.len(), 4);
            assert!(hrir.responses.iter().all(|ear| ear.len() == 10));
        }
    }

    fn impulse_at_frame_one(_: usize, _: usize, frame: usize) -> f64 {
        f64::from(u8::from(frame == 1))
    }

    fn impulse_by_measurement(measurement: usize, _: usize, frame: usize) -> f64 {
        let at = if measurement.is_multiple_of(2) { 1 } else { 3 };
        f64::from(u8::from(frame == at))
    }

    fn rounded(response: &[f64]) -> Vec<f64> {
        response
            .iter()
            .map(|tap| (tap * 1000.0).round() / 1000.0)
            .collect()
    }

    fn angle(degrees: u8) -> Speakers {
        Speakers::at(SpeakerAngle::from_degrees(degrees).expect("an angle"))
    }

    #[test]
    fn a_speaker_angle_the_listener_sets_chooses_the_measurements_at_that_angle() {
        let fixture = Fixture::new(&[(30.0, 0.0), (45.0, 0.0), (330.0, 0.0), (315.0, 0.0)]);

        let standard = read_hrir_bytes(fixture.bytes(), LONG_ENOUGH, STANDARD)
            .expect("a set")
            .expect("a SOFA file");
        let wide = read_hrir_bytes(fixture.bytes(), LONG_ENOUGH, angle(45))
            .expect("a set")
            .expect("a SOFA file");

        assert_eq!(standard.responses, fixture.expected(0, 2));
        assert_eq!(wide.responses, fixture.expected(1, 3));
    }

    #[test]
    fn a_set_with_nothing_near_the_angle_asked_is_refused_though_it_holds_the_standard_one() {
        let fixture = Fixture::new(&[(30.0, 0.0), (330.0, 0.0)]);

        let refused = read_hrir_bytes(fixture.bytes(), LONG_ENOUGH, angle(85));

        assert!(matches!(
            refused,
            Err(Fault::NoMeasurementNear {
                side: Side::Left,
                ..
            })
        ));
    }

    #[test]
    fn two_measurements_either_side_of_a_speaker_are_blended_with_their_onsets_aligned() {
        let mut fixture = Fixture::new(&[(20.0, 0.0), (40.0, 0.0), (340.0, 0.0), (320.0, 0.0)]);
        fixture.taps = Some(impulse_by_measurement);

        let hrir = fixture.read().expect("a set").expect("a SOFA file");

        for response in &hrir.responses {
            assert_eq!(rounded(response)[..5], [0.0, 0.0, 1.0, 0.0, 0.0]);
        }
    }

    #[test]
    fn a_nearer_measurement_weighs_more_and_the_onset_follows_its_weight() {
        let mut fixture = Fixture::new(&[(28.0, 0.0), (38.0, 0.0), (332.0, 0.0), (322.0, 0.0)]);
        fixture.taps = Some(impulse_by_measurement);

        let hrir = fixture.read().expect("a set").expect("a SOFA file");

        assert_eq!(rounded(&hrir.responses[0])[..4], [0.0, 1.0, 0.0, 0.0]);
        assert_eq!(rounded(&hrir.responses[3])[..4], [0.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn a_measurement_within_half_a_degree_is_taken_whole_and_a_far_one_is_not_blended_in() {
        let mut near = Fixture::new(&[(30.2, 0.0), (36.0, 0.0), (330.0, 0.0), (324.0, 0.0)]);
        near.taps = Some(impulse_by_measurement);
        let mut far = Fixture::new(&[(25.0, 0.0), (0.0, 0.0), (335.0, 0.0), (0.0, 0.0)]);
        far.taps = Some(impulse_by_measurement);

        let exact = near.read().expect("a set").expect("a SOFA file");
        let alone = far.read().expect("a set").expect("a SOFA file");

        assert_eq!(exact.responses, near.expected(0, 2));
        assert_eq!(alone.responses, far.expected(0, 2));
    }

    #[test]
    fn a_blend_keeps_the_difference_between_the_ears_that_the_delays_make() {
        let mut fixture = Fixture::new(&[(20.0, 0.0), (40.0, 0.0), (340.0, 0.0), (320.0, 0.0)]);
        fixture.taps = Some(impulse_at_frame_one);
        fixture.delay = Some(vec![0.0, 3.0, 0.0, 3.0, 0.0, 3.0, 0.0, 3.0]);

        let hrir = fixture.read().expect("a set").expect("a SOFA file");

        assert_eq!(rounded(&hrir.responses[0])[1], 1.0);
        assert_eq!(rounded(&hrir.responses[1])[4], 1.0);
    }

    #[test]
    fn a_classic_netcdf_file_reads_as_the_same_set_in_hdf5_does() {
        let fixture = Fixture::new(&SURROUND);

        let hdf5 = fixture.read().expect("a set").expect("a SOFA file");
        let narrow = read_hrir_bytes(fixture.classic_bytes(1, false), LONG_ENOUGH, STANDARD)
            .expect("a set")
            .expect("a SOFA file");
        let wide = read_hrir_bytes(fixture.classic_bytes(2, false), LONG_ENOUGH, STANDARD)
            .expect("a set")
            .expect("a SOFA file");
        let data = read_hrir_bytes(fixture.classic_bytes(5, false), LONG_ENOUGH, STANDARD)
            .expect("a set")
            .expect("a SOFA file");

        assert_eq!(narrow, hdf5);
        assert_eq!(wide, hdf5);
        assert_eq!(data, hdf5);
    }

    #[test]
    fn a_classic_file_keeping_its_measurements_as_records_single_precision_and_delayed_reads_too() {
        let mut fixture = Fixture::new(&SURROUND);
        fixture.single = true;
        fixture.delay = Some(vec![0.0, 3.0]);

        let hdf5 = fixture.read().expect("a set").expect("a SOFA file");
        let records = read_hrir_bytes(fixture.classic_bytes(1, true), LONG_ENOUGH, STANDARD)
            .expect("a set")
            .expect("a SOFA file");
        let data = read_hrir_bytes(fixture.classic_bytes(5, true), LONG_ENOUGH, STANDARD)
            .expect("a set")
            .expect("a SOFA file");

        assert_eq!(records, hdf5);
        assert_eq!(data, hdf5);
        assert!(records.responses.iter().all(|ear| ear.len() == FRAMES + 3));
    }

    #[test]
    fn a_classic_file_is_blended_like_any_other() {
        let mut fixture = Fixture::new(&[(20.0, 0.0), (40.0, 0.0), (340.0, 0.0), (320.0, 0.0)]);
        fixture.taps = Some(impulse_by_measurement);

        let hrir = read_hrir_bytes(fixture.classic_bytes(2, false), LONG_ENOUGH, STANDARD)
            .expect("a set")
            .expect("a SOFA file");

        assert_eq!(rounded(&hrir.responses[2])[..5], [0.0, 0.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn a_netcdf_version_this_reader_does_not_know_and_a_header_that_lies_are_refused() {
        let mut other = Fixture::new(&SURROUND).classic_bytes(1, false);
        other[3] = 3;
        let mut lying = Fixture::new(&SURROUND).classic_bytes(1, false);
        lying[8..12].copy_from_slice(&0x0000_00ff_u32.to_be_bytes());

        let version = read_hrir_bytes(other, LONG_ENOUGH, STANDARD);
        let header = read_hrir_bytes(lying, LONG_ENOUGH, STANDARD);

        assert!(matches!(version, Err(Fault::ClassicVersion { version: 3 })));
        assert!(matches!(header, Err(Fault::ClassicOpen { .. })));
    }

    #[test]
    fn a_classic_file_cut_short_or_marked_anywhere_is_refused_or_read_and_never_panics() {
        let whole = Fixture::new(&SURROUND).classic_bytes(1, false);

        for length in 0..whole.len() {
            let _ = read_hrir_bytes(whole[..length].to_vec(), LONG_ENOUGH, STANDARD);
        }
        for index in 0..whole.len() {
            for pattern in [0x00, 0xff, 0x80] {
                let mut marked = whole.clone();
                marked[index] = pattern;
                let _ = read_hrir_bytes(marked, LONG_ENOUGH, STANDARD);
            }
        }
    }

    struct Brir {
        views: Vec<(f64, f64, f64)>,
        emitters: Vec<(f64, f64)>,
        view_kind: Option<&'static str>,
        listener: Option<(f64, f64, f64)>,
        ranges: Vec<f64>,
    }

    impl Brir {
        fn tap(view: usize, emitter: usize, ear: usize, frame: usize) -> f64 {
            (view * 1000 + emitter * 100 + ear * 10 + frame + 1) as f64 / 10_000.0
        }

        fn bytes(&self) -> Vec<u8> {
            let (views, emitters) = (self.views.len(), self.emitters.len());
            let mut taps = Vec::new();
            for view in 0..views {
                for ear in 0..2 {
                    for emitter in 0..emitters {
                        for frame in 0..FRAMES {
                            taps.push(Self::tap(view, emitter, ear, frame));
                        }
                    }
                }
            }
            let positions: Vec<f64> = self
                .emitters
                .iter()
                .enumerate()
                .flat_map(|(at, (azimuth, elevation))| {
                    [
                        *azimuth,
                        *elevation,
                        self.ranges.get(at).copied().unwrap_or(1.0),
                    ]
                })
                .collect();
            let looking: Vec<f64> = self
                .views
                .iter()
                .flat_map(|(front, left, up)| [*front, *left, *up])
                .collect();

            let impulses = DatasetBuilder::typed_data(
                "Data.IR",
                vec![views as u64, 2, emitters as u64, FRAMES as u64],
                &taps,
            )
            .expect("impulses");
            let emitting = DatasetBuilder::typed_data(
                "EmitterPosition",
                vec![emitters as u64, 3, 1],
                &positions,
            )
            .expect("emitters")
            .attribute(AttributeBuilder::fixed_string("Type", "spherical"))
            .attribute(AttributeBuilder::fixed_string(
                "Units",
                "degree, degree, metre",
            ));
            let rate = DatasetBuilder::typed_data("Data.SamplingRate", vec![1], &[48_000.0])
                .expect("rate");
            let mut view =
                DatasetBuilder::typed_data("ListenerView", vec![views as u64, 3], &looking)
                    .expect("views");
            if let Some(kind) = self.view_kind {
                view = view.attribute(AttributeBuilder::fixed_string("Type", kind));
            }

            let mut plan = Hdf5Builder::new()
                .dataset(impulses)
                .dataset(emitting)
                .dataset(rate)
                .dataset(view);
            if let Some((front, left, up)) = self.listener {
                plan = plan.dataset(
                    DatasetBuilder::typed_data("ListenerPosition", vec![1, 3], &[front, left, up])
                        .expect("listener"),
                );
            }
            plan.into_plan()
                .expect("a plan")
                .encode(WriteOptions::default())
                .expect("bytes")
        }

        fn expected(&self, view: usize, left: usize, right: usize) -> Vec<Vec<f64>> {
            [(left, 0), (left, 1), (right, 0), (right, 1)]
                .iter()
                .map(|(emitter, ear)| {
                    (0..FRAMES)
                        .map(|frame| Self::tap(view, *emitter, *ear, frame))
                        .collect()
                })
                .collect()
        }
    }

    #[test]
    fn a_set_of_speakers_is_read_at_the_view_facing_the_front_and_the_emitters_at_the_angle() {
        let set = Brir {
            views: vec![(0.0, 1.0, 0.0), (1.0, 0.0, 0.0), (0.0, -1.0, 0.0)],
            emitters: vec![(110.0, 0.0), (30.0, 0.0), (330.0, 0.0), (250.0, 0.0)],
            view_kind: None,
            listener: None,
            ranges: Vec::new(),
        };

        let hrir = read_hrir_bytes(set.bytes(), LONG_ENOUGH, STANDARD)
            .expect("a set")
            .expect("a SOFA file");

        assert_eq!(hrir.rate, SampleRate::HZ_48000);
        assert_eq!(hrir.responses, set.expected(1, 1, 2));
    }

    #[test]
    fn a_set_of_speakers_is_read_at_the_angle_asked_and_its_views_may_say_their_kind() {
        let set = Brir {
            views: vec![(0.0, 1.0, 0.0), (1.0, 0.0, 0.0)],
            emitters: vec![(45.0, 0.0), (315.0, 0.0), (30.0, 0.0), (330.0, 0.0)],
            view_kind: Some("cartesian"),
            listener: None,
            ranges: Vec::new(),
        };

        let hrir = read_hrir_bytes(set.bytes(), LONG_ENOUGH, angle(45))
            .expect("a set")
            .expect("a SOFA file");

        assert_eq!(hrir.responses, set.expected(1, 0, 1));
    }

    #[test]
    fn a_set_of_speakers_with_no_emitter_near_the_speaker_is_refused() {
        let set = Brir {
            views: vec![(1.0, 0.0, 0.0)],
            emitters: vec![(180.0, 0.0), (120.0, 0.0)],
            view_kind: None,
            listener: None,
            ranges: Vec::new(),
        };

        let refused = read_hrir_bytes(set.bytes(), LONG_ENOUGH, STANDARD);

        assert!(matches!(
            refused,
            Err(Fault::NoMeasurementNear {
                side: Side::Left,
                ..
            })
        ));
    }

    fn raised(degrees: i16) -> Speakers {
        Speakers::STANDARD.raised_to(
            resonate_core::SpeakerElevation::from_degrees(degrees).expect("an elevation"),
        )
    }

    fn around(azimuth_degrees: f64, radius_degrees: f64, bearing_degrees: f64) -> (f64, f64) {
        let (azimuth, radius, bearing) = (
            azimuth_degrees.to_radians(),
            radius_degrees.to_radians(),
            bearing_degrees.to_radians(),
        );
        let (along, up) = (radius.sin() * bearing.cos(), radius.sin() * bearing.sin());
        let front = radius.cos() * azimuth.cos() - along * azimuth.sin();
        let left = radius.cos() * azimuth.sin() + along * azimuth.cos();
        (
            left.atan2(front).to_degrees().rem_euclid(360.0),
            up.asin().to_degrees(),
        )
    }

    fn level_by_measurement(measurement: usize, _: usize, frame: usize) -> f64 {
        let level = [1.0, 2.0, 6.0][measurement % 3];
        if frame == 1 { level } else { 0.0 }
    }

    #[test]
    fn a_speaker_inside_a_triangle_of_measurements_takes_all_three_by_where_it_stands() {
        let mut directions = Vec::new();
        for speaker in [30.0, -30.0] {
            for bearing in [90.0, 210.0, 330.0] {
                directions.push(around(speaker, 20.0, bearing));
            }
        }
        let mut fixture = Fixture::new(&directions);
        fixture.taps = Some(level_by_measurement);

        let hrir = fixture.read().expect("a set").expect("a SOFA file");

        for response in &hrir.responses {
            assert_eq!(rounded(response)[..3], [0.0, 3.0, 0.0]);
        }
    }

    #[test]
    fn a_speaker_standing_nearer_one_corner_of_its_triangle_takes_more_of_that_corner() {
        let mut centred = Vec::new();
        let mut shifted = Vec::new();
        for speaker in [30.0, -30.0] {
            for bearing in [90.0, 210.0, 330.0] {
                centred.push(around(speaker, 20.0, bearing));
                shifted.push(around(speaker, 20.0, bearing));
            }
        }
        let mut near_the_first = Fixture::new(&shifted);
        near_the_first.directions[0] = around(30.0, 12.0, 90.0);
        near_the_first.taps = Some(level_by_measurement);
        let mut level = Fixture::new(&centred);
        level.taps = Some(level_by_measurement);

        let near = near_the_first.read().expect("a set").expect("a SOFA file");
        let even = level.read().expect("a set").expect("a SOFA file");

        assert!(near.responses[0][1] < even.responses[0][1]);
        assert_eq!(rounded(&near.responses[2])[..3], [0.0, 3.0, 0.0]);
    }

    #[test]
    fn a_speaker_raised_is_read_from_the_measurements_at_that_elevation() {
        let mut fixture = Fixture::new(&[(30.0, 0.0), (30.0, 30.0), (330.0, 0.0), (330.0, 30.0)]);
        fixture.taps = Some(impulse_by_measurement);

        let level = read_hrir_bytes(fixture.bytes(), LONG_ENOUGH, STANDARD)
            .expect("a set")
            .expect("a SOFA file");
        let high = read_hrir_bytes(fixture.bytes(), LONG_ENOUGH, raised(30))
            .expect("a set")
            .expect("a SOFA file");
        let level_only = Fixture::new(&[(30.0, 0.0), (330.0, 0.0)]);
        let too_high = read_hrir_bytes(level_only.bytes(), LONG_ENOUGH, raised(60));

        assert_eq!(level.responses, fixture.expected(0, 2));
        assert_eq!(high.responses, fixture.expected(1, 3));
        assert!(matches!(too_high, Err(Fault::NoMeasurementNear { .. })));
    }

    #[test]
    fn a_speaker_between_a_row_of_measurements_and_the_next_row_above_is_blended_of_both_rows() {
        let mut fixture = Fixture::new(&[
            (20.0, 0.0),
            (40.0, 0.0),
            (30.0, 25.0),
            (340.0, 0.0),
            (320.0, 0.0),
            (330.0, 25.0),
        ]);
        fixture.taps = Some(level_by_measurement);

        let hrir = read_hrir_bytes(fixture.bytes(), LONG_ENOUGH, raised(8))
            .expect("a set")
            .expect("a SOFA file");

        let level = rounded(&hrir.responses[0])[1];
        assert!(level > 1.0 && level < 6.0, "{level}");
        assert!((level - 2.0).abs() > 0.2, "{level}");
    }

    fn turned(degrees: i16) -> Speakers {
        Speakers::STANDARD.turned_to(resonate_core::HeadYaw::from_degrees(degrees).expect("a yaw"))
    }

    fn every_ten_degrees() -> Vec<(f64, f64)> {
        (0..36).map(|step| (f64::from(step) * 10.0, 0.0)).collect()
    }

    #[test]
    fn a_head_turned_moves_both_speakers_round_by_the_turn_before_the_set_is_read() {
        let fixture = Fixture::new(&every_ten_degrees());

        let ahead = read_hrir_bytes(fixture.bytes(), LONG_ENOUGH, STANDARD)
            .expect("a set")
            .expect("a SOFA file");
        let left = read_hrir_bytes(fixture.bytes(), LONG_ENOUGH, turned(20))
            .expect("a set")
            .expect("a SOFA file");
        let right = read_hrir_bytes(fixture.bytes(), LONG_ENOUGH, turned(-20))
            .expect("a set")
            .expect("a SOFA file");

        assert_eq!(ahead.responses, fixture.expected(3, 33));
        assert_eq!(left.responses, fixture.expected(1, 31));
        assert_eq!(right.responses, fixture.expected(5, 35));
    }

    #[test]
    fn a_head_turned_past_the_set_s_reach_is_refused_naming_the_speaker() {
        let fixture = Fixture::new(&[(30.0, 0.0), (330.0, 0.0)]);

        let refused = read_hrir_bytes(fixture.bytes(), LONG_ENOUGH, turned(90));

        assert!(matches!(
            refused,
            Err(Fault::NoMeasurementNear {
                side: Side::Right,
                ..
            })
        ));
    }

    #[test]
    fn a_set_at_one_elevation_is_interpolated_between_its_two_neighbours_along_the_ring() {
        let mut fixture = Fixture::new(&[(20.0, 0.0), (50.0, 0.0), (340.0, 0.0), (310.0, 0.0)]);
        fixture.taps = Some(level_by_measurement);

        let hrir = fixture.read().expect("a set").expect("a SOFA file");

        assert!((hrir.responses[0][1] - (1.0 * 2.0 + 2.0) / 3.0).abs() < 1e-6);
        assert!((hrir.responses[2][1] - (6.0 * 2.0 + 1.0) / 3.0).abs() < 1e-6);
    }

    #[test]
    fn a_ring_with_a_gap_wider_than_sixty_degrees_is_not_interpolated_across() {
        let mut fixture = Fixture::new(&[(10.0, 0.0), (80.0, 0.0), (350.0, 0.0), (280.0, 0.0)]);
        fixture.taps = Some(level_by_measurement);

        let hrir = fixture.read().expect("a set").expect("a SOFA file");

        assert_eq!(hrir.responses, fixture.expected(0, 2));
    }

    #[test]
    fn a_speaker_past_the_outermost_row_is_interpolated_along_that_row() {
        let mut fixture = Fixture::new(&[
            (20.0, 0.0),
            (40.0, 0.0),
            (20.0, 20.0),
            (40.0, 20.0),
            (340.0, 0.0),
            (320.0, 0.0),
            (340.0, 20.0),
            (320.0, 20.0),
        ]);
        fixture.taps = Some(level_by_measurement);

        let hrir = read_hrir_bytes(fixture.bytes(), LONG_ENOUGH, raised(35))
            .expect("a set")
            .expect("a SOFA file");

        assert!(
            (hrir.responses[0][1] - 3.5).abs() < 1e-6,
            "{:?}",
            hrir.responses[0]
        );
        assert!(
            (hrir.responses[2][1] - 1.5).abs() < 1e-6,
            "{:?}",
            hrir.responses[2]
        );
    }

    fn room(
        views: Vec<(f64, f64, f64)>,
        emitters: Vec<(f64, f64)>,
        listener: Option<(f64, f64, f64)>,
        ranges: Vec<f64>,
    ) -> Brir {
        Brir {
            views,
            emitters,
            view_kind: None,
            listener,
            ranges,
        }
    }

    #[test]
    fn a_speaker_between_two_emitters_takes_both_by_where_it_stands() {
        let set = room(
            vec![(1.0, 0.0, 0.0)],
            vec![(20.0, 0.0), (40.0, 0.0), (340.0, 0.0), (320.0, 0.0)],
            None,
            Vec::new(),
        );

        let hrir = read_hrir_bytes(set.bytes(), LONG_ENOUGH, STANDARD)
            .expect("a set")
            .expect("a SOFA file");

        let first = set.expected(0, 0, 2);
        let second = set.expected(0, 1, 3);
        for (response, (one, other)) in hrir.responses.iter().zip(first.iter().zip(&second)) {
            for (tap, (one, other)) in response.iter().zip(one.iter().zip(other)) {
                assert!((tap - (one + other) / 2.0).abs() < 1e-9);
            }
        }
    }

    fn standing_at(listener: (f64, f64, f64), azimuth: f64, range: f64) -> (f64, f64, f64) {
        let heard = Direction::heard_from(azimuth, 0.0);
        let (front, left, up) = (
            listener.0 + range * heard.front,
            listener.1 + range * heard.left,
            listener.2 + range * heard.up,
        );
        let radius = (front * front + left * left + up * up).sqrt();
        (
            left.atan2(front).to_degrees().rem_euclid(360.0),
            (up / radius).asin().to_degrees(),
            radius,
        )
    }

    fn room_with_the_listener_at(listener: Option<(f64, f64, f64)>) -> Brir {
        let standing = (1.0, 0.0, 0.0);
        let placed = [
            standing_at(standing, 30.0, 2.0),
            standing_at(standing, -30.0, 2.0),
            (30.0, 0.0, 2.0),
            (330.0, 0.0, 2.0),
        ];
        room(
            vec![(1.0, 0.0, 0.0)],
            placed
                .iter()
                .map(|(azimuth, elevation, _)| (*azimuth, *elevation))
                .collect(),
            listener,
            placed.iter().map(|(.., range)| *range).collect(),
        )
    }

    #[test]
    fn a_listener_off_the_origin_hears_each_emitter_from_where_it_stands() {
        let off = room_with_the_listener_at(Some((1.0, 0.0, 0.0)));
        let origin = room_with_the_listener_at(None);
        let at_zero = room_with_the_listener_at(Some((0.0, 0.0, 0.0)));

        let moved = read_hrir_bytes(off.bytes(), LONG_ENOUGH, STANDARD)
            .expect("a set")
            .expect("a SOFA file");
        let ignored = read_hrir_bytes(origin.bytes(), LONG_ENOUGH, STANDARD)
            .expect("a set")
            .expect("a SOFA file");
        let centred = read_hrir_bytes(at_zero.bytes(), LONG_ENOUGH, STANDARD)
            .expect("a set")
            .expect("a SOFA file");

        assert_eq!(moved.responses, off.expected(0, 0, 1));
        assert_eq!(ignored.responses, origin.expected(0, 2, 3));
        assert_eq!(centred.responses, ignored.responses);
    }

    #[test]
    fn a_head_turned_in_a_room_is_read_at_the_view_it_faces_and_what_is_left_of_the_turn_moves_the_lookup()
     {
        let facing = |degrees: f64| (degrees.to_radians().cos(), degrees.to_radians().sin(), 0.0);
        let set = room(
            vec![facing(0.0), facing(20.0)],
            vec![(30.0, 0.0), (330.0, 0.0), (10.0, 0.0), (310.0, 0.0)],
            None,
            Vec::new(),
        );
        let alone = room(
            vec![(1.0, 0.0, 0.0)],
            vec![(10.0, 0.0), (310.0, 0.0)],
            None,
            Vec::new(),
        );

        let ahead = read_hrir_bytes(set.bytes(), LONG_ENOUGH, STANDARD)
            .expect("a set")
            .expect("a SOFA file");
        let turned_to_the_view = read_hrir_bytes(set.bytes(), LONG_ENOUGH, turned(20))
            .expect("a set")
            .expect("a SOFA file");
        let turned_in_a_room_of_one_view = read_hrir_bytes(alone.bytes(), LONG_ENOUGH, turned(20))
            .expect("a set")
            .expect("a SOFA file");

        assert_eq!(ahead.responses, set.expected(0, 0, 1));
        assert_eq!(turned_to_the_view.responses, set.expected(1, 0, 1));
        assert_eq!(
            turned_in_a_room_of_one_view.responses,
            alone.expected(0, 0, 1)
        );
    }

    struct Counting {
        inner: io::Cursor<Vec<u8>>,
        read: Arc<std::sync::atomic::AtomicU64>,
        seekable: bool,
    }

    impl Read for Counting {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let taken = self.inner.read(buf)?;
            self.read
                .fetch_add(taken as u64, std::sync::atomic::Ordering::Relaxed);
            Ok(taken)
        }
    }

    impl Seek for Counting {
        fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
            self.inner.seek(to)
        }
    }

    impl MediaStream for Counting {
        fn is_seekable(&self) -> bool {
            self.seekable
        }

        fn byte_len(&self) -> Option<u64> {
            Some(self.inner.get_ref().len() as u64)
        }
    }

    struct Counted {
        source: resonate_core::SourceId,
        bytes: Vec<u8>,
        read: Arc<std::sync::atomic::AtomicU64>,
        seekable: bool,
    }

    impl crate::MediaProvider for Counted {
        fn source(&self) -> &resonate_core::SourceId {
            &self.source
        }

        fn open(&self, _: &MediaLocation) -> Result<crate::Media> {
            Ok(crate::Media {
                stream: Box::new(Counting {
                    inner: io::Cursor::new(self.bytes.clone()),
                    read: Arc::clone(&self.read),
                    seekable: self.seekable,
                }),
                hint: None,
            })
        }
    }

    fn read_counting(fixture: &Fixture, seekable: bool, speakers: Speakers) -> (Hrir, u64, usize) {
        let bytes = fixture.bytes();
        let size = bytes.len();
        let read = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let source = resonate_core::SourceId::new("counted").expect("a name");
        let sources = Sources::local().and(Arc::new(Counted {
            source: source.clone(),
            bytes,
            read: Arc::clone(&read),
            seekable,
        }));
        let location = MediaLocation::new(source, "set.sofa");

        let hrir = read_hrir(&sources, &location, LONG_ENOUGH, speakers)
            .expect("a set")
            .expect("a SOFA file");

        (hrir, read.load(std::sync::atomic::Ordering::Relaxed), size)
    }

    #[test]
    fn a_seekable_set_is_read_from_the_measurements_chosen_alone_and_a_stream_that_cannot_seek_is_read_whole()
     {
        let mut fixture = Fixture::new(&every_ten_degrees());
        fixture.samples = 4096;

        let (streamed, bytes_streamed, size) = read_counting(&fixture, true, STANDARD);
        let (whole, bytes_whole, _) = read_counting(&fixture, false, STANDARD);

        assert_eq!(streamed, whole);
        assert_eq!(streamed.responses, fixture.expected(3, 33));
        assert!(bytes_whole >= size as u64);
        assert!(
            bytes_streamed * 4 < size as u64,
            "{bytes_streamed} of {size}"
        );
    }

    #[test]
    fn a_seekable_set_declared_longer_than_the_streamed_limit_is_refused_before_a_byte_is_read() {
        struct Endless;

        impl Read for Endless {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                buf.fill(0);
                Ok(buf.len())
            }
        }

        impl Seek for Endless {
            fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
                Ok(0)
            }
        }

        impl MediaStream for Endless {
            fn is_seekable(&self) -> bool {
                true
            }

            fn byte_len(&self) -> Option<u64> {
                Some(SOFA_STREAMED_BYTES_AT_MOST + 1)
            }
        }

        let refused = read_hrir_streamed(
            Box::new(Endless),
            SOFA_STREAMED_BYTES_AT_MOST + 1,
            LONG_ENOUGH,
            STANDARD,
        );

        assert!(matches!(refused, Err(Fault::TooLarge { .. })));
    }
}

#[cfg(test)]
mod blending {
    use std::f64::consts::PI;

    use super::*;

    const RATE_HZ: f64 = 48_000.0;
    const TAPS: usize = 128;
    const SPAN_DEGREES: f64 = 30.0;
    const MOST: usize = 4096;
    const MAGNITUDE_ERROR_AT_MOST_DB: f64 = 0.5;

    fn windowed_sinc(offset: f64) -> f64 {
        const HALF: f64 = 16.0;
        if offset.abs() >= HALF {
            return 0.0;
        }
        let sinc = if offset.abs() < 1e-12 {
            1.0
        } else {
            (PI * offset).sin() / (PI * offset)
        };
        sinc * 0.5 * (1.0 + (PI * offset / HALF).cos())
    }

    fn head(degrees: f64) -> Vec<f64> {
        let pole = 0.3 + 0.5 * degrees / 60.0;
        let delay = 20.0 + 0.12 * degrees;
        let gain = 1.0 - 0.3 * degrees / 60.0;
        (0..TAPS)
            .map(|at| {
                let filtered: f64 = (0..TAPS)
                    .map(|lag| {
                        (1.0 - pole)
                            * pole.powi(lag as i32)
                            * windowed_sinc(at as f64 - lag as f64 - delay)
                    })
                    .sum();
                gain * filtered
            })
            .collect()
    }

    fn decibels(response: &[f64]) -> Vec<f64> {
        let size = 1024;
        let forward = FftPlanner::<f64>::new().plan_fft_forward(size);
        let mut spectrum: Vec<Complex<f64>> = response
            .iter()
            .map(|tap| Complex::new(*tap, 0.0))
            .chain(std::iter::repeat(Complex::new(0.0, 0.0)))
            .take(size)
            .collect();
        forward.process(&mut spectrum);
        let top = (16_000.0 / RATE_HZ * size as f64) as usize;
        spectrum[1..top]
            .iter()
            .map(|bin| 20.0 * bin.norm().max(1e-9).log10())
            .collect()
    }

    fn distance(heard: &[f64], truth: &[f64]) -> f64 {
        let (heard, truth) = (decibels(heard), decibels(truth));
        (heard
            .iter()
            .zip(&truth)
            .map(|(a, b)| (a - b) * (a - b))
            .sum::<f64>()
            / heard.len() as f64)
            .sqrt()
    }

    #[test]
    fn a_coarse_set_blended_by_magnitude_and_delay_lands_nearer_the_head_between_than_a_sum_does() {
        let (near, far) = (head(0.0), head(SPAN_DEGREES));
        let halves = [(0.5, near.as_slice()), (0.5, far.as_slice())];
        let fifths = [(0.8, near.as_slice()), (0.2, far.as_slice())];

        let midway = head(SPAN_DEGREES * 0.5);
        let nearer = head(SPAN_DEGREES * 0.2);

        let summed = distance(&aligned_sum(&halves, MOST), &midway);
        let rebuilt = distance(&magnitude_and_delay(&halves, MOST), &midway);
        let summed_unevenly = distance(&aligned_sum(&fifths, MOST), &nearer);
        let rebuilt_unevenly = distance(&magnitude_and_delay(&fifths, MOST), &nearer);

        assert!(rebuilt < summed / 10.0, "{rebuilt} dB against {summed} dB");
        assert!(
            rebuilt_unevenly < summed_unevenly / 5.0,
            "{rebuilt_unevenly} dB against {summed_unevenly} dB"
        );
        assert!(rebuilt < MAGNITUDE_ERROR_AT_MOST_DB, "{rebuilt} dB");
    }

    #[test]
    fn the_onset_of_a_blend_by_magnitude_and_delay_follows_the_weights_as_a_sums_does() {
        let (near, far) = (head(0.0), head(SPAN_DEGREES));
        let parts = [(0.75, near.as_slice()), (0.25, far.as_slice())];

        let summed = onset_of(&aligned_sum(&parts, MOST));
        let rebuilt = onset_of(&magnitude_and_delay(&parts, MOST));

        assert!(summed.abs_diff(rebuilt) <= 1, "{summed} against {rebuilt}");
    }

    #[test]
    fn a_blend_by_magnitude_and_delay_of_silence_is_silence_and_of_one_response_is_that_response() {
        let silent = vec![0.0; TAPS];
        let alone = head(10.0);

        let nothing = magnitude_and_delay(&[(0.5, &silent), (0.5, &silent)], MOST);
        let same = magnitude_and_delay(&[(0.5, &alone), (0.5, &alone)], MOST);

        assert!(nothing.iter().all(|tap| *tap == 0.0));
        assert!(distance(&same, &alone) < MAGNITUDE_ERROR_AT_MOST_DB);
    }
}
