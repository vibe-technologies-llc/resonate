use thiserror::Error;

const MAGIC: &[u8; 3] = b"CDF";
const ABSENT: u32 = 0;
const DIMENSIONS_TAG: u32 = 0x0A;
const VARIABLES_TAG: u32 = 0x0B;
const ATTRIBUTES_TAG: u32 = 0x0C;
const STREAMING_NARROW_RECORDS: u64 = u32::MAX as u64;
const STREAMING_WIDE_RECORDS: u64 = u64::MAX;
const PADDED_TO: usize = 4;
const WORD_BYTES: u64 = 4;
const DIMENSION_BYTES_AT_LEAST: u64 = 2 * WORD_BYTES;
const ATTRIBUTE_BYTES_AT_LEAST: u64 = 3 * WORD_BYTES;
const VARIABLE_BYTES_AT_LEAST: u64 = 7 * WORD_BYTES;
const RECORD_DIMENSION_LENGTH: u64 = 0;

pub(crate) const FIRST_OFFSET_VERSION: u8 = 1;
pub(crate) const SECOND_OFFSET_VERSION: u8 = 2;
pub(crate) const WIDE_DATA_VERSION: u8 = 5;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ClassicFault {
    #[error("the file ends before the {wanted} bytes wanted at byte {at}")]
    Truncated { at: u64, wanted: u64 },

    #[error("a list is tagged {found:#x}, which no list of a classic file is")]
    ListTag { found: u32 },

    #[error("a type code of {code} is not one a classic file has")]
    TypeUnknown { code: u32 },

    #[error("a name is not text")]
    NameNotText,

    #[error("a list declares {count} entries, more than the {left} bytes left can hold")]
    CountPastFile { count: u64, left: u64 },

    #[error("a variable names dimension {id} where the file declares {declared}")]
    DimensionUnknown { id: u32, declared: usize },

    #[error("the record dimension is not the first of a variable")]
    RecordNotFirst,

    #[error("a variable holds {length} bytes from byte {begin}, past the file's {size}")]
    DataPastFile { begin: u64, length: u64, size: u64 },

    #[error("a variable's size cannot be counted")]
    SizeOverflow,

    #[error("a selection reaches past the shape of what it reads")]
    SelectionOutside,

    #[error("a variable is not made of floating point numbers")]
    NotFloating,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Byte,
    Char,
    Short,
    Int,
    Float,
    Double,
    UnsignedByte,
    UnsignedShort,
    UnsignedInt,
    Int64,
    UnsignedInt64,
}

impl Kind {
    const fn of(code: u32, extended: bool) -> Result<Self, ClassicFault> {
        match code {
            1 => Ok(Self::Byte),
            2 => Ok(Self::Char),
            3 => Ok(Self::Short),
            4 => Ok(Self::Int),
            5 => Ok(Self::Float),
            6 => Ok(Self::Double),
            7 if extended => Ok(Self::UnsignedByte),
            8 if extended => Ok(Self::UnsignedShort),
            9 if extended => Ok(Self::UnsignedInt),
            10 if extended => Ok(Self::Int64),
            11 if extended => Ok(Self::UnsignedInt64),
            code => Err(ClassicFault::TypeUnknown { code }),
        }
    }

    pub(crate) const fn bytes(self) -> u64 {
        match self {
            Self::Byte | Self::Char | Self::UnsignedByte => 1,
            Self::Short | Self::UnsignedShort => 2,
            Self::Int | Self::Float | Self::UnsignedInt => 4,
            Self::Double | Self::Int64 | Self::UnsignedInt64 => 8,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pick {
    At(u64),
    Span { start: u64, end: u64 },
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
    wide_counts: bool,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8], at: usize) -> Self {
        Self {
            bytes,
            at,
            wide_counts: false,
        }
    }

    fn count(&mut self) -> Result<u64, ClassicFault> {
        if self.wide_counts {
            self.wide()
        } else {
            self.word().map(u64::from)
        }
    }

    fn left(&self) -> u64 {
        (self.bytes.len() - self.at) as u64
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], ClassicFault> {
        let end = self
            .at
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or(ClassicFault::Truncated {
                at: self.at as u64,
                wanted: count as u64,
            })?;
        let taken = &self.bytes[self.at..end];
        self.at = end;
        Ok(taken)
    }

    fn word(&mut self) -> Result<u32, ClassicFault> {
        let taken = self.take(WORD_BYTES as usize)?;
        Ok(u32::from_be_bytes([taken[0], taken[1], taken[2], taken[3]]))
    }

    fn wide(&mut self) -> Result<u64, ClassicFault> {
        let taken = self.take(8)?;
        let mut whole = [0; 8];
        whole.copy_from_slice(taken);
        Ok(u64::from_be_bytes(whole))
    }

    fn padded(&mut self, count: usize) -> Result<&'a [u8], ClassicFault> {
        let taken = self.take(count)?;
        let slack = (PADDED_TO - count % PADDED_TO) % PADDED_TO;
        self.take(slack)?;
        Ok(taken)
    }

    fn name(&mut self) -> Result<String, ClassicFault> {
        let length = self.count()?;
        let length = usize::try_from(length).map_err(|_| ClassicFault::CountPastFile {
            count: length,
            left: self.left(),
        })?;
        let raw = self.padded(length)?;
        String::from_utf8(raw.to_vec()).map_err(|_| ClassicFault::NameNotText)
    }

    fn list(&mut self, tag: u32, at_least: u64) -> Result<usize, ClassicFault> {
        let found = self.word()?;
        let count = self.count()?;
        if found == ABSENT && count == 0 {
            return Ok(0);
        }
        if found != tag {
            return Err(ClassicFault::ListTag { found });
        }
        let left = self.left();
        if count.saturating_mul(at_least) > left {
            return Err(ClassicFault::CountPastFile { count, left });
        }
        Ok(count as usize)
    }
}

#[derive(Debug)]
struct Dimension {
    length: u64,
}

impl Dimension {
    const fn is_record(&self) -> bool {
        self.length == RECORD_DIMENSION_LENGTH
    }
}

#[derive(Debug)]
struct Attribute {
    name: String,
    text: Option<String>,
}

#[derive(Debug)]
struct Header {
    name: String,
    dimensions: Vec<usize>,
    attributes: Vec<Attribute>,
    kind: Kind,
    begin: u64,
}

#[derive(Debug)]
pub(crate) struct Classic {
    bytes: Vec<u8>,
    dimensions: Vec<Dimension>,
    variables: Vec<Header>,
    records: u64,
    record_bytes: u64,
}

pub(crate) fn version_of(bytes: &[u8]) -> Option<u8> {
    if bytes.len() > MAGIC.len() && bytes.starts_with(MAGIC) {
        Some(bytes[MAGIC.len()])
    } else {
        None
    }
}

impl Classic {
    pub(crate) fn of(bytes: Vec<u8>) -> Result<Self, ClassicFault> {
        let mut cursor = Cursor::new(&bytes, MAGIC.len());
        let version = cursor.take(1)?[0];
        let wide_offsets = matches!(version, SECOND_OFFSET_VERSION | WIDE_DATA_VERSION);
        let extended = version == WIDE_DATA_VERSION;
        cursor.wide_counts = extended;
        let streaming = if extended {
            STREAMING_WIDE_RECORDS
        } else {
            STREAMING_NARROW_RECORDS
        };
        let records = match cursor.count()? {
            held if held == streaming => 0,
            records => records,
        };

        let dimensions = dimensions_of(&mut cursor)?;
        attributes_of(&mut cursor, ATTRIBUTES_TAG, extended)?;
        let variables = variables_of(&mut cursor, &dimensions, wide_offsets, extended)?;
        let record_bytes = record_bytes_of(&dimensions, &variables)?;

        Ok(Self {
            bytes,
            dimensions,
            variables,
            records,
            record_bytes,
        })
    }

    pub(crate) fn variable(&self, name: &str) -> Option<Variable<'_>> {
        self.variables
            .iter()
            .find(|header| header.name == name)
            .map(|header| Variable {
                classic: self,
                header,
            })
    }
}

fn dimensions_of(cursor: &mut Cursor<'_>) -> Result<Vec<Dimension>, ClassicFault> {
    let count = cursor.list(DIMENSIONS_TAG, DIMENSION_BYTES_AT_LEAST)?;
    let mut dimensions = Vec::with_capacity(count);
    for _ in 0..count {
        cursor.name()?;
        let length = cursor.count()?;
        dimensions.push(Dimension { length });
    }
    Ok(dimensions)
}

fn attributes_of(
    cursor: &mut Cursor<'_>,
    tag: u32,
    extended: bool,
) -> Result<Vec<Attribute>, ClassicFault> {
    let count = cursor.list(tag, ATTRIBUTE_BYTES_AT_LEAST)?;
    let mut attributes = Vec::with_capacity(count);
    for _ in 0..count {
        let name = cursor.name()?;
        let kind = Kind::of(cursor.word()?, extended)?;
        let elements = cursor.count()?;
        let length = elements
            .checked_mul(kind.bytes())
            .filter(|length| *length <= cursor.left())
            .ok_or(ClassicFault::CountPastFile {
                count: elements,
                left: cursor.left(),
            })?;
        let raw = cursor.padded(length as usize)?;
        let text = (kind == Kind::Char).then(|| {
            String::from_utf8_lossy(raw)
                .trim_end_matches('\0')
                .to_owned()
        });
        attributes.push(Attribute { name, text });
    }
    Ok(attributes)
}

fn variables_of(
    cursor: &mut Cursor<'_>,
    dimensions: &[Dimension],
    wide_offsets: bool,
    extended: bool,
) -> Result<Vec<Header>, ClassicFault> {
    let count = cursor.list(VARIABLES_TAG, VARIABLE_BYTES_AT_LEAST)?;
    let mut variables = Vec::with_capacity(count);
    for _ in 0..count {
        let name = cursor.name()?;
        let rank = cursor.count()?;
        if rank.saturating_mul(WORD_BYTES) > cursor.left() {
            return Err(ClassicFault::CountPastFile {
                count: rank,
                left: cursor.left(),
            });
        }
        let mut shape = Vec::with_capacity(rank as usize);
        for place in 0..rank {
            let id = cursor.word()?;
            let dimension = dimensions
                .get(id as usize)
                .ok_or(ClassicFault::DimensionUnknown {
                    id,
                    declared: dimensions.len(),
                })?;
            if dimension.is_record() && place != 0 {
                return Err(ClassicFault::RecordNotFirst);
            }
            shape.push(id as usize);
        }
        let attributes = attributes_of(cursor, ATTRIBUTES_TAG, extended)?;
        let kind = Kind::of(cursor.word()?, extended)?;
        cursor.count()?;
        let begin = if wide_offsets {
            cursor.wide()?
        } else {
            u64::from(cursor.word()?)
        };
        variables.push(Header {
            name,
            dimensions: shape,
            attributes,
            kind,
            begin,
        });
    }
    Ok(variables)
}

fn padded_to_word(bytes: u64) -> Result<u64, ClassicFault> {
    bytes
        .checked_add(PADDED_TO as u64 - 1)
        .map(|bytes| bytes / PADDED_TO as u64 * PADDED_TO as u64)
        .ok_or(ClassicFault::SizeOverflow)
}

fn inner_elements(dimensions: &[Dimension], header: &Header) -> Result<u64, ClassicFault> {
    header
        .dimensions
        .iter()
        .skip(1)
        .try_fold(1_u64, |held, id| held.checked_mul(dimensions[*id].length))
        .ok_or(ClassicFault::SizeOverflow)
}

fn is_record(dimensions: &[Dimension], header: &Header) -> bool {
    header
        .dimensions
        .first()
        .is_some_and(|id| dimensions[*id].is_record())
}

fn record_bytes_of(dimensions: &[Dimension], variables: &[Header]) -> Result<u64, ClassicFault> {
    let mut total = 0_u64;
    for header in variables
        .iter()
        .filter(|header| is_record(dimensions, header))
    {
        let each = inner_elements(dimensions, header)?
            .checked_mul(header.kind.bytes())
            .ok_or(ClassicFault::SizeOverflow)?;
        total = total
            .checked_add(padded_to_word(each)?)
            .ok_or(ClassicFault::SizeOverflow)?;
    }
    Ok(total)
}

#[derive(Clone, Copy)]
pub(crate) struct Variable<'a> {
    classic: &'a Classic,
    header: &'a Header,
}

impl Variable<'_> {
    pub(crate) const fn kind(&self) -> Kind {
        self.header.kind
    }

    pub(crate) fn shape(&self) -> Vec<u64> {
        self.header
            .dimensions
            .iter()
            .map(|id| {
                let dimension = &self.classic.dimensions[*id];
                if dimension.is_record() {
                    self.classic.records
                } else {
                    dimension.length
                }
            })
            .collect()
    }

    pub(crate) fn text(&self, attribute: &str) -> Option<&str> {
        self.header
            .attributes
            .iter()
            .find(|held| held.name == attribute)
            .and_then(|held| held.text.as_deref())
    }

    pub(crate) fn numbers(&self, picks: &[Pick]) -> Result<Vec<f64>, ClassicFault> {
        let kind = self.header.kind;
        if !matches!(kind, Kind::Float | Kind::Double) {
            return Err(ClassicFault::NotFloating);
        }

        let shape = self.shape();
        let ranges = ranges_of(&shape, picks)?;
        let size = kind.bytes();
        let file = self.classic.bytes.len() as u64;
        let elements = ranges
            .iter()
            .try_fold(1_u64, |held, (start, end)| held.checked_mul(end - start))
            .ok_or(ClassicFault::SizeOverflow)?;
        let wanted = elements
            .checked_mul(size)
            .ok_or(ClassicFault::SizeOverflow)?;
        if wanted > file {
            return Err(ClassicFault::DataPastFile {
                begin: self.header.begin,
                length: wanted,
                size: file,
            });
        }

        let strides = self.strides(&shape, size)?;
        let mut values = Vec::with_capacity(elements as usize);
        if elements == 0 {
            return Ok(values);
        }

        let Some(last) = ranges.len().checked_sub(1) else {
            self.append(&mut values, self.header.begin, 1)?;
            return Ok(values);
        };
        let run = ranges[last].1 - ranges[last].0;
        let mut at: Vec<u64> = ranges.iter().map(|(start, _)| *start).collect();
        loop {
            let mut offset = self.header.begin;
            for (place, index) in at.iter().enumerate() {
                offset = index
                    .checked_mul(strides[place])
                    .and_then(|bytes| offset.checked_add(bytes))
                    .ok_or(ClassicFault::SizeOverflow)?;
            }
            self.append(&mut values, offset, run)?;

            let mut place = last;
            loop {
                if place == 0 {
                    return Ok(values);
                }
                place -= 1;
                at[place] += 1;
                if at[place] < ranges[place].1 {
                    break;
                }
                at[place] = ranges[place].0;
            }
        }
    }

    fn strides(&self, shape: &[u64], size: u64) -> Result<Vec<u64>, ClassicFault> {
        let mut strides = vec![0; shape.len()];
        let mut inner = size;
        for place in (0..shape.len()).rev() {
            strides[place] = inner;
            let length = if place == 0 && is_record(&self.classic.dimensions, self.header) {
                1
            } else {
                shape[place]
            };
            inner = inner
                .checked_mul(length)
                .ok_or(ClassicFault::SizeOverflow)?;
        }
        if is_record(&self.classic.dimensions, self.header) {
            strides[0] = self.classic.record_bytes;
        }
        Ok(strides)
    }

    fn append(&self, values: &mut Vec<f64>, begin: u64, count: u64) -> Result<(), ClassicFault> {
        let size = self.header.kind.bytes();
        let length = count.checked_mul(size).ok_or(ClassicFault::SizeOverflow)?;
        let file = self.classic.bytes.len() as u64;
        let past = ClassicFault::DataPastFile {
            begin,
            length,
            size: file,
        };
        let end = begin
            .checked_add(length)
            .ok_or(ClassicFault::SizeOverflow)?;
        if end > file {
            return Err(past);
        }
        let raw = &self.classic.bytes[begin as usize..end as usize];
        match self.header.kind {
            Kind::Float => values.extend(
                raw.as_chunks::<4>()
                    .0
                    .iter()
                    .map(|chunk| f64::from(f32::from_be_bytes(*chunk))),
            ),
            _ => values.extend(
                raw.as_chunks::<8>()
                    .0
                    .iter()
                    .map(|chunk| f64::from_be_bytes(*chunk)),
            ),
        }
        Ok(())
    }
}

fn ranges_of(shape: &[u64], picks: &[Pick]) -> Result<Vec<(u64, u64)>, ClassicFault> {
    shape
        .iter()
        .enumerate()
        .map(|(place, length)| {
            let (start, end) = match picks.get(place) {
                Some(Pick::At(index)) => (*index, index.saturating_add(1)),
                Some(Pick::Span { start, end }) => (*start, *end),
                None => (0, *length),
            };
            if start > end || end > *length {
                Err(ClassicFault::SelectionOutside)
            } else {
                Ok((start, end))
            }
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod written {
    use super::{
        ATTRIBUTES_TAG, DIMENSIONS_TAG, MAGIC, PADDED_TO, SECOND_OFFSET_VERSION, VARIABLES_TAG,
        WIDE_DATA_VERSION,
    };

    pub(crate) struct Column {
        pub name: &'static str,
        pub dimensions: Vec<usize>,
        pub attributes: Vec<(&'static str, String)>,
        pub doubles: bool,
        pub values: Vec<f64>,
    }

    pub(crate) struct File {
        pub version: u8,
        pub dimensions: Vec<(&'static str, u32)>,
        pub records: u32,
        pub variables: Vec<Column>,
    }

    fn padded(bytes: &mut Vec<u8>) {
        while !bytes.len().is_multiple_of(PADDED_TO) {
            bytes.push(0);
        }
    }

    fn count(bytes: &mut Vec<u8>, wide: bool, value: usize) {
        if wide {
            bytes.extend_from_slice(&(value as u64).to_be_bytes());
        } else {
            bytes.extend_from_slice(&(value as u32).to_be_bytes());
        }
    }

    fn name(bytes: &mut Vec<u8>, wide: bool, text: &str) {
        count(bytes, wide, text.len());
        bytes.extend_from_slice(text.as_bytes());
        padded(bytes);
    }

    impl File {
        fn is_record(&self, variable: &Column) -> bool {
            variable
                .dimensions
                .first()
                .is_some_and(|id| self.dimensions[*id].1 == 0)
        }

        fn per_record(&self, variable: &Column) -> usize {
            let size = if variable.doubles { 8 } else { 4 };
            let inner: usize = variable
                .dimensions
                .iter()
                .skip(1)
                .map(|id| self.dimensions[*id].1 as usize)
                .product();
            (inner * size).div_ceil(PADDED_TO) * PADDED_TO
        }

        fn header(&self, begins: &[u64]) -> Vec<u8> {
            let wide = self.version == WIDE_DATA_VERSION;
            let wide_offsets = self.version >= SECOND_OFFSET_VERSION;
            let absent = |bytes: &mut Vec<u8>| {
                bytes.extend_from_slice(&[0; 4]);
                count(bytes, wide, 0);
            };
            let mut bytes = MAGIC.to_vec();
            bytes.push(self.version);
            count(&mut bytes, wide, self.records as usize);

            bytes.extend_from_slice(&DIMENSIONS_TAG.to_be_bytes());
            count(&mut bytes, wide, self.dimensions.len());
            for (label, length) in &self.dimensions {
                name(&mut bytes, wide, label);
                count(&mut bytes, wide, *length as usize);
            }

            absent(&mut bytes);

            bytes.extend_from_slice(&VARIABLES_TAG.to_be_bytes());
            count(&mut bytes, wide, self.variables.len());
            for (variable, begin) in self.variables.iter().zip(begins) {
                name(&mut bytes, wide, variable.name);
                count(&mut bytes, wide, variable.dimensions.len());
                for id in &variable.dimensions {
                    bytes.extend_from_slice(&(*id as u32).to_be_bytes());
                }
                if variable.attributes.is_empty() {
                    absent(&mut bytes);
                } else {
                    bytes.extend_from_slice(&ATTRIBUTES_TAG.to_be_bytes());
                    count(&mut bytes, wide, variable.attributes.len());
                    for (label, text) in &variable.attributes {
                        name(&mut bytes, wide, label);
                        bytes.extend_from_slice(&2_u32.to_be_bytes());
                        count(&mut bytes, wide, text.len());
                        bytes.extend_from_slice(text.as_bytes());
                        padded(&mut bytes);
                    }
                }
                bytes.extend_from_slice(&(if variable.doubles { 6_u32 } else { 5 }).to_be_bytes());
                count(&mut bytes, wide, 0);
                if wide_offsets {
                    bytes.extend_from_slice(&begin.to_be_bytes());
                } else {
                    bytes.extend_from_slice(&(*begin as u32).to_be_bytes());
                }
            }
            bytes
        }

        fn push(bytes: &mut Vec<u8>, doubles: bool, value: f64) {
            if doubles {
                bytes.extend_from_slice(&value.to_be_bytes());
            } else {
                bytes.extend_from_slice(&(value as f32).to_be_bytes());
            }
        }

        pub(crate) fn bytes(&self) -> Vec<u8> {
            let header_length = self.header(&vec![0; self.variables.len()]).len() as u64;

            let mut begins = Vec::new();
            let mut at = header_length;
            for variable in self.variables.iter().filter(|held| !self.is_record(held)) {
                begins.push((variable.name, at));
                let size = if variable.doubles { 8 } else { 4 };
                at += (variable.values.len() * size).div_ceil(PADDED_TO) as u64 * PADDED_TO as u64;
            }
            let records_at = at;
            let mut within = 0_u64;
            for variable in self.variables.iter().filter(|held| self.is_record(held)) {
                begins.push((variable.name, records_at + within));
                within += self.per_record(variable) as u64;
            }
            let ordered: Vec<u64> = self
                .variables
                .iter()
                .map(|variable| {
                    begins
                        .iter()
                        .find(|(held, _)| *held == variable.name)
                        .map_or(0, |(_, begin)| *begin)
                })
                .collect();

            let mut bytes = self.header(&ordered);
            for variable in self.variables.iter().filter(|held| !self.is_record(held)) {
                for value in &variable.values {
                    Self::push(&mut bytes, variable.doubles, *value);
                }
                padded(&mut bytes);
            }
            let records = self.records as usize;
            for record in 0..records {
                for variable in self.variables.iter().filter(|held| self.is_record(held)) {
                    let inner = variable.values.len() / records.max(1);
                    for value in &variable.values[record * inner..(record + 1) * inner] {
                        Self::push(&mut bytes, variable.doubles, *value);
                    }
                    padded(&mut bytes);
                }
            }
            bytes
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{written::*, *};

    fn grid() -> File {
        File {
            version: 1,
            dimensions: vec![("rows", 2), ("columns", 3)],
            records: 0,
            variables: vec![
                Column {
                    name: "single",
                    dimensions: vec![0, 1],
                    attributes: vec![("Type", "spherical".to_owned())],
                    doubles: false,
                    values: vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
                },
                Column {
                    name: "double",
                    dimensions: vec![1],
                    attributes: Vec::new(),
                    doubles: true,
                    values: vec![0.25, 0.5, 0.75],
                },
            ],
        }
    }

    #[test]
    fn a_grid_of_singles_and_a_row_of_doubles_read_back_whole_and_in_part() {
        let classic = Classic::of(grid().bytes()).expect("a classic file");

        let single = classic.variable("single").expect("a variable");
        let double = classic.variable("double").expect("a variable");

        assert_eq!(single.shape(), vec![2, 3]);
        assert_eq!(single.kind(), Kind::Float);
        assert_eq!(single.text("Type"), Some("spherical"));
        assert_eq!(single.text("Units"), None);
        assert_eq!(
            single.numbers(&[]).expect("all"),
            vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
        );
        assert_eq!(
            single
                .numbers(&[Pick::At(1), Pick::Span { start: 1, end: 3 }])
                .expect("a row's tail"),
            vec![5.0, 6.0]
        );
        assert_eq!(double.numbers(&[]).expect("all"), vec![0.25, 0.5, 0.75]);
        assert!(classic.variable("absent").is_none());
    }

    #[test]
    fn wide_offsets_and_wide_data_read_as_narrow_ones_do() {
        let versioned = |version| {
            let mut file = grid();
            file.version = version;
            Classic::of(file.bytes()).expect("a classic file")
        };

        let narrow = versioned(1);
        let wide = versioned(2);
        let data = versioned(5);

        let heard = |classic: &Classic| classic.variable("single").unwrap().numbers(&[]).unwrap();
        assert_eq!(heard(&narrow), heard(&wide));
        assert_eq!(heard(&narrow), heard(&data));
        assert_eq!(
            data.variable("single").unwrap().text("Type"),
            Some("spherical")
        );
        assert_eq!(data.variable("double").unwrap().shape(), vec![3]);
    }

    #[test]
    fn records_of_a_wide_data_file_read_as_those_of_a_narrow_one_do() {
        let records = |version| File {
            version,
            dimensions: vec![("rows", 0), ("columns", 2)],
            records: 3,
            variables: vec![
                Column {
                    name: "first",
                    dimensions: vec![0, 1],
                    attributes: Vec::new(),
                    doubles: false,
                    values: vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
                },
                Column {
                    name: "second",
                    dimensions: vec![0, 1],
                    attributes: Vec::new(),
                    doubles: true,
                    values: vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0],
                },
            ],
        };

        let classic = Classic::of(records(5).bytes()).expect("a classic file");

        assert_eq!(classic.variable("first").unwrap().shape(), vec![3, 2]);
        assert_eq!(
            classic.variable("second").unwrap().numbers(&[]).unwrap(),
            vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0]
        );
        assert_eq!(
            classic
                .variable("first")
                .unwrap()
                .numbers(&[Pick::At(2)])
                .unwrap(),
            vec![5.0, 6.0]
        );
    }

    #[test]
    fn the_integer_types_only_a_wide_data_file_has_are_known_to_it_alone() {
        let mut extended = grid().bytes();
        extended[3] = WIDE_DATA_VERSION;
        assert_eq!(Kind::of(10, true), Ok(Kind::Int64));
        assert_eq!(Kind::of(7, true).map(Kind::bytes), Ok(1));
        assert_eq!(Kind::of(8, true).map(Kind::bytes), Ok(2));
        assert_eq!(Kind::of(9, true).map(Kind::bytes), Ok(4));
        assert_eq!(Kind::of(11, true).map(Kind::bytes), Ok(8));
        assert_eq!(
            Kind::of(10, false),
            Err(ClassicFault::TypeUnknown { code: 10 })
        );
        assert!(Classic::of(extended).is_err());
    }

    #[test]
    fn a_wide_data_file_cut_short_or_marked_never_panics() {
        let mut file = grid();
        file.version = 5;
        let whole = file.bytes();

        for length in 0..whole.len() {
            let _ = Classic::of(whole[..length].to_vec());
        }
        for index in 0..whole.len() {
            for pattern in [0x00, 0xff, 0x80, 0x7f] {
                let mut marked = whole.clone();
                marked[index] = pattern;
                if let Ok(classic) = Classic::of(marked) {
                    for held in ["single", "double"] {
                        if let Some(variable) = classic.variable(held) {
                            let _ = variable.numbers(&[]);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn records_interleave_and_a_variable_of_them_reads_its_own_rows() {
        let file = File {
            version: 1,
            dimensions: vec![("rows", 0), ("columns", 2)],
            records: 3,
            variables: vec![
                Column {
                    name: "first",
                    dimensions: vec![0, 1],
                    attributes: Vec::new(),
                    doubles: false,
                    values: vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
                },
                Column {
                    name: "second",
                    dimensions: vec![0, 1],
                    attributes: Vec::new(),
                    doubles: true,
                    values: vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0],
                },
            ],
        };

        let classic = Classic::of(file.bytes()).expect("a classic file");
        let first = classic.variable("first").unwrap();
        let second = classic.variable("second").unwrap();

        assert_eq!(first.shape(), vec![3, 2]);
        assert_eq!(
            first.numbers(&[]).unwrap(),
            vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
        );
        assert_eq!(second.numbers(&[Pick::At(2)]).unwrap(), vec![50.0, 60.0]);
        assert_eq!(
            second
                .numbers(&[Pick::Span { start: 1, end: 3 }, Pick::At(1)])
                .unwrap(),
            vec![40.0, 60.0]
        );
    }

    #[test]
    fn a_selection_past_the_shape_or_a_variable_that_is_not_floating_is_refused() {
        let classic = Classic::of(grid().bytes()).expect("a classic file");
        let single = classic.variable("single").unwrap();

        assert_eq!(
            single.numbers(&[Pick::At(2)]),
            Err(ClassicFault::SelectionOutside)
        );
        assert_eq!(
            single.numbers(&[
                Pick::Span { start: 0, end: 1 },
                Pick::Span { start: 2, end: 9 }
            ]),
            Err(ClassicFault::SelectionOutside)
        );
    }

    #[test]
    fn a_header_that_lies_is_refused_and_a_file_cut_short_or_marked_never_panics() {
        let whole = grid().bytes();

        assert!(matches!(
            Classic::of(b"CDF\x01".to_vec()),
            Err(ClassicFault::Truncated { .. })
        ));
        for length in 0..whole.len() {
            let cut = Classic::of(whole[..length].to_vec());
            if let Ok(classic) = cut
                && let Some(single) = classic.variable("single")
            {
                let _ = single.numbers(&[]);
            }
        }
        for index in 0..whole.len() {
            for pattern in [0x00, 0xff, 0x80, 0x7f] {
                let mut marked = whole.clone();
                marked[index] = pattern;
                if let Ok(classic) = Classic::of(marked) {
                    for held in ["single", "double"] {
                        if let Some(variable) = classic.variable(held) {
                            let _ = variable.numbers(&[]);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_magic_and_its_version_are_read_from_the_first_bytes() {
        assert_eq!(version_of(b"CDF\x02rest"), Some(2));
        assert_eq!(version_of(b"CDF"), None);
        assert_eq!(version_of(b"\x89HDF\r\n\x1a\n"), None);
    }
}
