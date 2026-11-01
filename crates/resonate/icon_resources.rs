use thiserror::Error;

pub const RT_ICON: u16 = 3;
pub const RT_GROUP_ICON: u16 = 14;
pub const NULL_RECORD: [u8; HEADER_SIZE] = null_record();

const HEADER_SIZE: usize = 32;
const ICON_DIRECTORY_HEADER: usize = 6;
const ICON_DIRECTORY_ENTRY: usize = 16;
const GROUP_DIRECTORY_ENTRY: usize = 14;
const ICON_FILE_KIND: u16 = 1;
const ORDINAL_MARK: u16 = 0xFFFF;
const MOVEABLE: u16 = 0x0010;
const PURE: u16 = 0x0020;
const DISCARDABLE: u16 = 0x1000;
const NEUTRAL_LANGUAGE: u16 = 0;
const DWORD: usize = 4;

#[derive(Debug, Error)]
pub enum Error {
    #[error("the icon file holds {held} bytes where {needed} are needed")]
    Truncated { needed: usize, held: usize },
    #[error("the file is no icon: reserved {reserved}, kind {kind}")]
    NotAnIcon { reserved: u16, kind: u16 },
    #[error("image {index} lies outside the file")]
    ImageOutsideFile { index: usize },
    #[error("{count} images cannot be numbered in a resource")]
    TooManyImages { count: usize },
    #[error("the resource file does not begin with the empty record")]
    NoEmptyRecord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub width: u8,
    pub height: u8,
    pub colours: u8,
    pub reserved: u8,
    pub planes: u16,
    pub bits: u16,
    pub length: u32,
    pub offset: u32,
}

pub struct Image<'a> {
    pub entry: Entry,
    pub bytes: &'a [u8],
}

const fn null_record() -> [u8; HEADER_SIZE] {
    let mut record = [0; HEADER_SIZE];
    record[4] = HEADER_SIZE as u8;
    record[8] = 0xFF;
    record[9] = 0xFF;
    record[12] = 0xFF;
    record[13] = 0xFF;
    record
}

fn word(bytes: &[u8], at: usize) -> Result<u16, Error> {
    let end = at + size_of::<u16>();
    let slice = bytes.get(at..end).ok_or(Error::Truncated {
        needed: end,
        held: bytes.len(),
    })?;

    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn double(bytes: &[u8], at: usize) -> Result<u32, Error> {
    let end = at + size_of::<u32>();
    let slice = bytes.get(at..end).ok_or(Error::Truncated {
        needed: end,
        held: bytes.len(),
    })?;

    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn entry_at(ico: &[u8], at: usize) -> Result<Entry, Error> {
    let needed = at + ICON_DIRECTORY_ENTRY;
    let bytes = ico.get(at..needed).ok_or(Error::Truncated {
        needed,
        held: ico.len(),
    })?;

    Ok(Entry {
        width: bytes[0],
        height: bytes[1],
        colours: bytes[2],
        reserved: bytes[3],
        planes: word(bytes, 4)?,
        bits: word(bytes, 6)?,
        length: double(bytes, 8)?,
        offset: double(bytes, 12)?,
    })
}

pub fn images(ico: &[u8]) -> Result<Vec<Image<'_>>, Error> {
    let reserved = word(ico, 0)?;
    let kind = word(ico, 2)?;
    let count = usize::from(word(ico, 4)?);

    if reserved != 0 || kind != ICON_FILE_KIND {
        return Err(Error::NotAnIcon { reserved, kind });
    }

    (0..count)
        .map(|index| {
            let entry = entry_at(ico, ICON_DIRECTORY_HEADER + index * ICON_DIRECTORY_ENTRY)?;
            let start =
                usize::try_from(entry.offset).map_err(|_| Error::ImageOutsideFile { index })?;
            let length =
                usize::try_from(entry.length).map_err(|_| Error::ImageOutsideFile { index })?;
            let bytes = start
                .checked_add(length)
                .and_then(|end| ico.get(start..end))
                .ok_or(Error::ImageOutsideFile { index })?;

            Ok(Image { entry, bytes })
        })
        .collect()
}

fn image_id(index: usize) -> Result<u16, Error> {
    index
        .checked_add(1)
        .and_then(|id| u16::try_from(id).ok())
        .ok_or(Error::TooManyImages { count: index })
}

pub fn group_directory(images: &[Image<'_>]) -> Result<Vec<u8>, Error> {
    let count = u16::try_from(images.len()).map_err(|_| Error::TooManyImages {
        count: images.len(),
    })?;
    let mut directory =
        Vec::with_capacity(ICON_DIRECTORY_HEADER + images.len() * GROUP_DIRECTORY_ENTRY);

    directory.extend_from_slice(&0u16.to_le_bytes());
    directory.extend_from_slice(&ICON_FILE_KIND.to_le_bytes());
    directory.extend_from_slice(&count.to_le_bytes());
    for (index, image) in images.iter().enumerate() {
        let entry = image.entry;

        directory.extend_from_slice(&[entry.width, entry.height, entry.colours, entry.reserved]);
        directory.extend_from_slice(&entry.planes.to_le_bytes());
        directory.extend_from_slice(&entry.bits.to_le_bytes());
        directory.extend_from_slice(&entry.length.to_le_bytes());
        directory.extend_from_slice(&image_id(index)?.to_le_bytes());
    }
    Ok(directory)
}

fn record(resource_type: u16, name: u16, flags: u16, data: &[u8]) -> Result<Vec<u8>, Error> {
    let length =
        u32::try_from(data.len()).map_err(|_| Error::TooManyImages { count: data.len() })?;
    let mut record = Vec::with_capacity(HEADER_SIZE + data.len() + DWORD);

    record.extend_from_slice(&length.to_le_bytes());
    record.extend_from_slice(&(HEADER_SIZE as u32).to_le_bytes());
    record.extend_from_slice(&ORDINAL_MARK.to_le_bytes());
    record.extend_from_slice(&resource_type.to_le_bytes());
    record.extend_from_slice(&ORDINAL_MARK.to_le_bytes());
    record.extend_from_slice(&name.to_le_bytes());
    record.extend_from_slice(&0u32.to_le_bytes());
    record.extend_from_slice(&flags.to_le_bytes());
    record.extend_from_slice(&NEUTRAL_LANGUAGE.to_le_bytes());
    record.extend_from_slice(&0u32.to_le_bytes());
    record.extend_from_slice(&0u32.to_le_bytes());
    record.extend_from_slice(data);
    record.resize(record.len().next_multiple_of(DWORD), 0);
    Ok(record)
}

pub fn records(ico: &[u8], group: u16) -> Result<Vec<u8>, Error> {
    let images = images(ico)?;
    let mut records = Vec::new();

    for (index, image) in images.iter().enumerate() {
        records.extend(record(
            RT_ICON,
            image_id(index)?,
            MOVEABLE | DISCARDABLE,
            image.bytes,
        )?);
    }
    records.extend(record(
        RT_GROUP_ICON,
        group,
        MOVEABLE | PURE | DISCARDABLE,
        &group_directory(&images)?,
    )?);
    Ok(records)
}

pub fn joined(records: &[u8], resource_file: &[u8]) -> Result<Vec<u8>, Error> {
    let rest = resource_file
        .strip_prefix(&NULL_RECORD)
        .ok_or(Error::NoEmptyRecord)?;
    let mut joined = Vec::with_capacity(NULL_RECORD.len() + records.len() + rest.len());

    joined.extend_from_slice(&NULL_RECORD);
    joined.extend_from_slice(records);
    joined.extend_from_slice(rest);
    Ok(joined)
}
