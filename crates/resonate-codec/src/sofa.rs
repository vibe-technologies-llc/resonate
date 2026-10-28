use std::{
    fmt,
    io::{self, Read},
    time::Duration,
};

use hdf5_reader::{Dataset, Datatype, Hdf5File, OpenOptions, SliceInfo, SliceInfoElem};
use resonate_core::{Frames, MediaLocation, SampleRate};
use thiserror::Error;

use crate::{Error, Result, Sources};

const SIGNATURE: [u8; 8] = *b"\x89HDF\r\n\x1a\n";

pub const SOFA_BYTES_AT_MOST: u64 = 128 << 20;

const MEASUREMENTS_AT_MOST: u64 = 1 << 20;
const RATES_AT_MOST: u64 = 1 << 20;
const EARS: u64 = 2;
const COORDINATES: u64 = 3;
const NEAREST_WITHIN_DEGREES: f64 = 45.0;
const SPEAKER_AZIMUTH_DEGREES: f64 = 30.0;
const CHUNK_CACHE_BYTES: usize = 16 << 20;
const CHUNK_BYTES_AT_MOST: usize = 64 << 20;
const SINGLE_PRECISION_BYTES: u8 = 4;
const DOUBLE_PRECISION_BYTES: u8 = 8;
const RADIANS: &str = "rad";

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
    const fn azimuth_degrees(self) -> f64 {
        match self {
            Self::Left => SPEAKER_AZIMUTH_DEGREES,
            Self::Right => -SPEAKER_AZIMUTH_DEGREES,
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

    #[error("could not {op} {member}")]
    Hdf5 {
        member: Member,
        op: Access,
        #[source]
        source: Box<hdf5_reader::error::Error>,
    },

    #[error("the set has no {member}")]
    MemberMissing { member: Member },

    #[error("{member} is not made of floating point numbers")]
    NotFloating { member: Member },

    #[error("{member} has the shape {dims:?}, which a free-field HRIR set does not")]
    ShapeNotUsable { member: Member, dims: Vec<u64> },

    #[error("the set holds no measurements")]
    NoMeasurements,

    #[error("the set's responses hold no samples")]
    NoSamples,

    #[error("the set holds {measurements} measurements, more than {at_most} are read")]
    TooManyMeasurements { measurements: u64, at_most: u64 },

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
    if bytes != SIGNATURE {
        return Ok(None);
    }

    let room = SOFA_BYTES_AT_MOST + 1 - SIGNATURE.len() as u64;
    (&mut *media.stream)
        .take(room)
        .read_to_end(&mut bytes)
        .map_err(unread)?;

    read_hrir_bytes(bytes, longest).map_err(|source| Error::Sofa {
        location: location.clone(),
        source,
    })
}

pub fn read_hrir_bytes(
    bytes: Vec<u8>,
    longest: Duration,
) -> std::result::Result<Option<Hrir>, Fault> {
    if !bytes.starts_with(&SIGNATURE) {
        return Ok(None);
    }
    let held = bytes.len() as u64;
    if held > SOFA_BYTES_AT_MOST {
        return Err(Fault::TooLarge {
            bytes: held,
            at_most: SOFA_BYTES_AT_MOST,
        });
    }

    let options = OpenOptions::default()
        .with_chunk_cache_bytes(CHUNK_CACHE_BYTES)
        .with_max_chunk_bytes(CHUNK_BYTES_AT_MOST);
    let file = Hdf5File::from_vec_with_options(bytes, options).map_err(|source| Fault::Open {
        source: Box::new(source),
    })?;

    Set::of(&file)?.stereo_pair(longest).map(Some)
}

struct Set {
    rate: SampleRate,
    impulses: Dataset,
    samples: u64,
    directions: Vec<Option<Direction>>,
    delays: Option<Delays>,
}

struct Delays {
    rows: u64,
    samples: Vec<f64>,
}

impl Set {
    fn of(file: &Hdf5File) -> std::result::Result<Self, Fault> {
        let rate = rate_of(file)?;

        let impulses = locate(file, Member::Impulses)?;
        let (measurements, samples) = match *impulses.shape() {
            [measurements, EARS, samples] => (measurements, samples),
            ref dims => {
                return Err(Fault::ShapeNotUsable {
                    member: Member::Impulses,
                    dims: dims.to_vec(),
                });
            }
        };
        if measurements == 0 {
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

        let directions = directions_of(file, measurements)?;
        let delays = delays_of(file, measurements)?;

        Ok(Self {
            rate,
            impulses,
            samples,
            directions,
            delays,
        })
    }

    fn stereo_pair(&self, longest: Duration) -> std::result::Result<Hrir, Fault> {
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
            let measurement = self.nearest(side)?;
            ears.extend(self.ears_at(measurement, frames, most)?);
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

    fn nearest(&self, side: Side) -> std::result::Result<u64, Fault> {
        let wanted = Direction::heard_from(side.azimuth_degrees(), 0.0);
        self.directions
            .iter()
            .enumerate()
            .filter_map(|(index, direction)| {
                direction.map(|direction| (index, direction.degrees_from(wanted)))
            })
            .filter(|(_, apart)| *apart <= NEAREST_WITHIN_DEGREES)
            .min_by(|(_, apart), (_, other)| apart.total_cmp(other))
            .map(|(index, _)| index as u64)
            .ok_or(Fault::NoMeasurementNear {
                side,
                within: NEAREST_WITHIN_DEGREES,
            })
    }

    fn ears_at(
        &self,
        measurement: u64,
        frames: usize,
        most: usize,
    ) -> std::result::Result<Vec<Vec<f64>>, Fault> {
        let selection = SliceInfo {
            selections: vec![
                SliceInfoElem::Index(measurement),
                SliceInfoElem::Slice {
                    start: 0,
                    end: EARS,
                    step: 1,
                },
                SliceInfoElem::Slice {
                    start: 0,
                    end: frames as u64,
                    step: 1,
                },
            ],
        };
        let values = numbers(&self.impulses, Member::Impulses, Some(&selection))?;
        if values.len() != frames * EARS as usize {
            return Err(Fault::ShapeNotUsable {
                member: Member::Impulses,
                dims: self.impulses.shape().to_vec(),
            });
        }

        let delays = self.delays_at(measurement)?;
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

fn rate_of(file: &Hdf5File) -> std::result::Result<SampleRate, Fault> {
    let dataset = locate(file, Member::SamplingRate)?;
    bounded(&dataset, Member::SamplingRate, RATES_AT_MOST)?;
    let hertz = numbers(&dataset, Member::SamplingRate, None)?
        .first()
        .copied()
        .ok_or_else(|| Fault::ShapeNotUsable {
            member: Member::SamplingRate,
            dims: dataset.shape().to_vec(),
        })?;

    let whole = hertz.round();
    if !(f64::from(SampleRate::MIN_HZ)..=f64::from(SampleRate::MAX_HZ)).contains(&whole) {
        return Err(Fault::RateNotUsable { hertz });
    }
    SampleRate::new(whole as u32).map_err(|_| Fault::RateNotUsable { hertz })
}

fn directions_of(
    file: &Hdf5File,
    measurements: u64,
) -> std::result::Result<Vec<Option<Direction>>, Fault> {
    let dataset = locate(file, Member::SourcePosition)?;
    if *dataset.shape() != [measurements, COORDINATES] {
        return Err(Fault::ShapeNotUsable {
            member: Member::SourcePosition,
            dims: dataset.shape().to_vec(),
        });
    }

    let coordinates = Coordinates::of(&dataset)?;
    let values = numbers(&dataset, Member::SourcePosition, None)?;
    Ok(values
        .as_chunks::<{ COORDINATES as usize }>()
        .0
        .iter()
        .map(|position| coordinates.direction(position))
        .collect())
}

fn delays_of(file: &Hdf5File, measurements: u64) -> std::result::Result<Option<Delays>, Fault> {
    let dataset = match file.dataset(Member::Delay.name()) {
        Ok(dataset) => dataset,
        Err(source) if is_missing(&source) => return Ok(None),
        Err(source) => {
            return Err(Fault::Hdf5 {
                member: Member::Delay,
                op: Access::Locate,
                source: Box::new(source),
            });
        }
    };
    let rows = match *dataset.shape() {
        [EARS] => 1,
        [rows, EARS] if rows == 1 || rows == measurements => rows,
        ref dims => {
            return Err(Fault::ShapeNotUsable {
                member: Member::Delay,
                dims: dims.to_vec(),
            });
        }
    };
    let samples = numbers(&dataset, Member::Delay, None)?;
    Ok(Some(Delays { rows, samples }))
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Coordinates {
    Spherical { radians: bool },
    Cartesian,
}

impl Coordinates {
    fn of(dataset: &Dataset) -> std::result::Result<Self, Fault> {
        let kind = text(dataset, Member::PositionType)?.ok_or(Fault::MemberMissing {
            member: Member::PositionType,
        })?;
        match kind.trim().to_ascii_lowercase().as_str() {
            "spherical" => {
                let units = text(dataset, Member::PositionUnits)?.unwrap_or_default();
                let angle = units.split(',').next().unwrap_or_default();
                let radians = angle.trim().to_ascii_lowercase().starts_with(RADIANS);
                Ok(Self::Spherical { radians })
            }
            "cartesian" => Ok(Self::Cartesian),
            _ => Err(Fault::PositionTypeUnknown),
        }
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

    fn along(front: f64, left: f64, up: f64) -> Option<Self> {
        let length = (front * front + left * left + up * up).sqrt();
        (length.is_finite() && length > f64::EPSILON).then(|| Self {
            front: front / length,
            left: left / length,
            up: up / length,
        })
    }

    fn is_finite(self) -> bool {
        self.front.is_finite() && self.left.is_finite() && self.up.is_finite()
    }

    fn degrees_from(self, other: Self) -> f64 {
        let alike = self.front * other.front + self.left * other.left + self.up * other.up;
        alike.clamp(-1.0, 1.0).acos().to_degrees()
    }
}

fn locate(file: &Hdf5File, member: Member) -> std::result::Result<Dataset, Fault> {
    file.dataset(member.name()).map_err(|source| {
        if is_missing(&source) {
            Fault::MemberMissing { member }
        } else {
            Fault::Hdf5 {
                member,
                op: Access::Locate,
                source: Box::new(source),
            }
        }
    })
}

fn text(dataset: &Dataset, member: Member) -> std::result::Result<Option<String>, Fault> {
    let name = match member {
        Member::PositionType => "Type",
        _ => "Units",
    };
    let attribute = match dataset.attribute(name) {
        Ok(attribute) => attribute,
        Err(source) if is_missing(&source) => return Ok(None),
        Err(source) => {
            return Err(Fault::Hdf5 {
                member,
                op: Access::Attribute,
                source: Box::new(source),
            });
        }
    };
    attribute
        .read_string()
        .map(Some)
        .map_err(|source| Fault::Hdf5 {
            member,
            op: Access::Attribute,
            source: Box::new(source),
        })
}

fn bounded(dataset: &Dataset, member: Member, at_most: u64) -> std::result::Result<(), Fault> {
    let elements = dataset
        .shape()
        .iter()
        .try_fold(1_u64, |held, dim| held.checked_mul(*dim));
    match elements {
        Some(elements) if elements <= at_most => Ok(()),
        _ => Err(Fault::ShapeNotUsable {
            member,
            dims: dataset.shape().to_vec(),
        }),
    }
}

fn numbers(
    dataset: &Dataset,
    member: Member,
    selection: Option<&SliceInfo>,
) -> std::result::Result<Vec<f64>, Fault> {
    let Datatype::FloatingPoint { size, .. } = dataset.dtype() else {
        return Err(Fault::NotFloating { member });
    };
    let unread = |source| Fault::Hdf5 {
        member,
        op: Access::Read,
        source: Box::new(source),
    };
    match *size {
        SINGLE_PRECISION_BYTES => {
            let array = match selection {
                Some(selection) => dataset.read_slice::<f32>(selection),
                None => dataset.read_array::<f32>(),
            }
            .map_err(unread)?;
            Ok(array.iter().map(|value| f64::from(*value)).collect())
        }
        DOUBLE_PRECISION_BYTES => {
            let array = match selection {
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

    const LONG_ENOUGH: Duration = Duration::from_secs(10);
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
            }
        }

        fn measurements(&self) -> u64 {
            self.directions.len() as u64
        }

        fn tap(measurement: usize, ear: usize, frame: usize) -> f64 {
            (measurement * 100 + ear * 10 + frame + 1) as f64 / 1000.0
        }

        fn impulses(&self) -> Vec<f64> {
            let mut values = Vec::new();
            for measurement in 0..self.directions.len() {
                for ear in 0..self.ears as usize {
                    for frame in 0..self.samples {
                        values.push(Self::tap(measurement, ear, frame));
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
            read_hrir_bytes(self.bytes(), LONG_ENOUGH)
        }

        fn expected(&self, left: usize, right: usize) -> Vec<Vec<f64>> {
            [(left, 0), (left, 1), (right, 0), (right, 1)]
                .iter()
                .map(|(measurement, ear)| {
                    (0..self.samples)
                        .map(|frame| Self::tap(*measurement, *ear, frame))
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

        let hrir = read_hrir_bytes(fixture.bytes(), Duration::from_millis(1))
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
            read_hrir_bytes(Vec::new(), LONG_ENOUGH)
                .expect("no fault")
                .is_none()
        );
        assert!(
            read_hrir_bytes(b"RIFF....WAVEfmt ".to_vec(), LONG_ENOUGH)
                .expect("no fault")
                .is_none()
        );
        assert!(matches!(
            read_hrir_bytes(SIGNATURE.to_vec(), LONG_ENOUGH),
            Err(Fault::Open { .. })
        ));
    }

    #[test]
    fn a_set_cut_short_or_marked_anywhere_is_refused_or_read_and_never_panics() {
        let whole = Fixture::new(&SURROUND).bytes();

        for length in 0..whole.len() {
            let _ = read_hrir_bytes(whole[..length].to_vec(), LONG_ENOUGH);
        }
        for index in 0..whole.len() {
            for pattern in [0x00, 0xff, 0x80] {
                let mut marked = whole.clone();
                marked[index] = pattern;
                let _ = read_hrir_bytes(marked, LONG_ENOUGH);
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

        let read = read_hrir(&sources, &MediaLocation::local(&set), LONG_ENOUGH);
        let left = read_hrir(&sources, &MediaLocation::local(&other), LONG_ENOUGH);
        let broken = {
            let mut bytes = fs::read(&set).expect("the set");
            bytes.truncate(40);
            fs::write(&set, bytes).expect("a cut set");
            read_hrir(&sources, &MediaLocation::local(&set), LONG_ENOUGH)
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
            let read = read_hrir_bytes(fs::read(&seed).expect("a seed"), LONG_ENOUGH);

            let hrir = read.expect("a set").expect("a SOFA file");
            assert_eq!(hrir.rate, SampleRate::HZ_48000, "{}", seed.display());
            assert_eq!(hrir.responses.len(), 4);
            assert!(hrir.responses.iter().all(|ear| ear.len() == 10));
        }
    }
}
