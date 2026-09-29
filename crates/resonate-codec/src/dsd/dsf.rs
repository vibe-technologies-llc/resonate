use std::io::SeekFrom;

use resonate_core::MediaLocation;
use symphonia::core::audio::Position;

use crate::{
    Error, Result,
    dsd::{
        BitOrder, Container, DsdChunk, DsdField, Edited, Interleave, Layout, placed, rate::DsdRate,
    },
    prescan::read_exact,
    source::MediaStream,
};

const DSD_HEADER_BYTES: u64 = 28;
const FMT_BODY_BYTES: u64 = 52;
const DATA_HEADER_BYTES: u64 = 12;
const BLOCK_BYTES: u64 = 4_096;

const FORMAT_VERSION: u32 = 1;
const FORMAT_ID_RAW: u32 = 0;
const BITS_LSB_FIRST: u32 = 1;
const BITS_MSB_FIRST: u32 = 8;

const MONO: u32 = 1;
const STEREO: u32 = 2;
const THREE: u32 = 3;
const QUAD: u32 = 4;
const FOUR: u32 = 5;
const FIVE: u32 = 6;
const FIVE_ONE: u32 = 7;

pub(crate) fn read(bytes: &mut dyn MediaStream, location: &MediaLocation) -> Result<Layout> {
    bytes.seek(SeekFrom::Start(0)).map_err(|source| Error::Io {
        location: location.clone(),
        source,
    })?;

    let head = read_exact::<28, _>(bytes).ok_or_else(|| missing(location, DsdChunk::Format))?;
    if !head.starts_with(b"DSD ") || u64::from_le_bytes(take8(&head, 4)) != DSD_HEADER_BYTES {
        return Err(missing(location, DsdChunk::Format));
    }
    let metadata_at = u64::from_le_bytes(take8(&head, 20));

    let fmt = read_exact::<64, _>(bytes).ok_or_else(|| missing(location, DsdChunk::Format))?;
    if !fmt.starts_with(b"fmt ") || u64::from_le_bytes(take8(&fmt, 4)) != FMT_BODY_BYTES {
        return Err(missing(location, DsdChunk::Format));
    }

    let version = u32::from_le_bytes(take4(&fmt, 12));
    if version != FORMAT_VERSION {
        return Err(unusable(location, DsdField::FormatVersion, version.into()));
    }
    let format_id = u32::from_le_bytes(take4(&fmt, 16));
    if format_id != FORMAT_ID_RAW {
        return Err(unusable(location, DsdField::FormatId, format_id.into()));
    }

    let kind = u32::from_le_bytes(take4(&fmt, 20));
    let declared_channels = u32::from_le_bytes(take4(&fmt, 24));
    let placed = placed(declared_channels, positions_of(kind), location)?;
    let channels = placed.channels;

    let hz = u32::from_le_bytes(take4(&fmt, 28));
    let rate = DsdRate::new(hz).ok_or_else(|| Error::RateNotRepresentable {
        location: location.clone(),
        track: crate::StreamTrackId(0),
        rate: hz,
    })?;

    let bits = match u32::from_le_bytes(take4(&fmt, 32)) {
        BITS_LSB_FIRST => BitOrder::LeastSignificantFirst,
        BITS_MSB_FIRST => BitOrder::MostSignificantFirst,
        other => return Err(unusable(location, DsdField::BitOrder, other.into())),
    };

    let samples = u64::from_le_bytes(take8(&fmt, 36));
    let block = u64::from(u32::from_le_bytes(take4(&fmt, 44)));
    if block != BLOCK_BYTES {
        return Err(unusable(location, DsdField::BlockSize, block));
    }

    if !fmt.get(52..56).is_some_and(|id| id == b"data") {
        return Err(missing(location, DsdChunk::Data));
    }
    let declared = u64::from_le_bytes(take8(&fmt, 56));
    let data_at = DSD_HEADER_BYTES + FMT_BODY_BYTES + DATA_HEADER_BYTES;
    let payload = declared.saturating_sub(DATA_HEADER_BYTES);

    let lanes = u64::from(channels.count().get());
    let held = bytes
        .byte_len()
        .map_or(payload, |len| payload.min(len.saturating_sub(data_at)));
    let data_bytes = held - held % (BLOCK_BYTES * lanes).max(1);

    Ok(Layout {
        container: Container::Dsf,
        rate,
        channels,
        speakers: placed.speakers,
        samples: samples.min(data_bytes / lanes * 8),
        data_at,
        data_bytes,
        order: Interleave::Blocked(BLOCK_BYTES),
        bits,
        metadata_at: (metadata_at > 0).then_some(metadata_at),
        edited: Edited::default(),
        packed: None,
    })
}

fn positions_of(kind: u32) -> Option<Position> {
    let front = Position::FRONT_LEFT | Position::FRONT_RIGHT;
    let rear = Position::REAR_LEFT | Position::REAR_RIGHT;
    Some(match kind {
        MONO => Position::FRONT_CENTER,
        STEREO => front,
        THREE => front | Position::FRONT_CENTER,
        QUAD => front | rear,
        FOUR => front | Position::FRONT_CENTER | Position::LFE1,
        FIVE => front | Position::FRONT_CENTER | rear,
        FIVE_ONE => front | Position::FRONT_CENTER | Position::LFE1 | rear,
        _ => return None,
    })
}

fn take4(held: &[u8], at: usize) -> [u8; 4] {
    held.get(at..at + 4)
        .and_then(|slice| slice.try_into().ok())
        .unwrap_or([0; 4])
}

fn take8(held: &[u8], at: usize) -> [u8; 8] {
    held.get(at..at + 8)
        .and_then(|slice| slice.try_into().ok())
        .unwrap_or([0; 8])
}

fn missing(location: &MediaLocation, chunk: DsdChunk) -> Error {
    Error::DsdChunkMissing {
        location: location.clone(),
        chunk,
    }
}

fn unusable(location: &MediaLocation, field: DsdField, value: u64) -> Error {
    Error::DsdFieldNotUsable {
        location: location.clone(),
        field,
        value,
    }
}
