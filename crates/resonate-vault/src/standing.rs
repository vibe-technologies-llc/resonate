use std::{fs::File, io::Read, path::Path};

use resonate_core::Frames;

use crate::{
    bare::FLAC_MAGIC,
    key::{KEY_BYTES, VaultKey},
    unpacking::Unpacking,
};

const FLAC_HEAD_BYTES: usize = 42;
const STREAM_INFO_AT: usize = 8;
const SAMPLES_AT: usize = STREAM_INFO_AT + 10;
const DIGEST_AT: usize = FLAC_HEAD_BYTES - KEY_BYTES;
const STREAM_INFO: u8 = 0;
const BLOCK_KIND: u8 = 0x7f;

const WAVE_HEAD_BYTES: usize = 68;
const CHANNELS_AT: usize = 22;
const RATE_AT: usize = 24;
const BLOCK_ALIGN_AT: usize = 32;
const FORMAT_LENGTH_AT: usize = 16;
const FORMAT_AT: usize = 20;
const CHUNK_HEADER_BYTES: usize = 8;
const DATA: &[u8; 4] = b"data";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Declared {
    pub(crate) rate: u32,
    pub(crate) channels: u8,
    pub(crate) frames: Frames,
    pub(crate) digest: Option<VaultKey>,
}

pub(crate) fn of_a_flac(path: &Path) -> Option<Declared> {
    let mut head = [0_u8; FLAC_HEAD_BYTES];
    File::open(path).ok()?.read_exact(&mut head).ok()?;
    if head[..FLAC_MAGIC.len()] != FLAC_MAGIC || head[FLAC_MAGIC.len()] & BLOCK_KIND != STREAM_INFO
    {
        return None;
    }

    let packed = u64::from_be_bytes(head[SAMPLES_AT..SAMPLES_AT + 8].try_into().ok()?);
    let rate = (packed >> 44) as u32;
    let channels = ((packed >> 41) & 0b111) as u8 + 1;
    let frames = packed & ((1 << 36) - 1);
    let digest: [u8; KEY_BYTES] = head[DIGEST_AT..].try_into().ok()?;

    Some(Declared {
        rate,
        channels,
        frames: Frames(frames),
        digest: Some(VaultKey::of(digest)),
    })
}

pub(crate) fn of_a_packed_wave(path: &Path) -> Option<Declared> {
    let mut head = [0_u8; WAVE_HEAD_BYTES];
    Unpacking::open(path).ok()?.read_exact(&mut head).ok()?;
    let little = |at: usize| u16::from_le_bytes([head[at], head[at + 1]]);
    let long = |at: usize| u32::from_le_bytes(head[at..at + 4].try_into().unwrap_or_default());

    let format_ends = FORMAT_AT + long(FORMAT_LENGTH_AT) as usize;
    let data_named = head.get(format_ends..format_ends + DATA.len())?;
    if data_named != DATA {
        return None;
    }
    let data_bytes = u64::from(u32::from_le_bytes(
        head.get(format_ends + DATA.len()..format_ends + CHUNK_HEADER_BYTES)?
            .try_into()
            .ok()?,
    ));
    let block_align = u64::from(little(BLOCK_ALIGN_AT));
    if block_align == 0 {
        return None;
    }

    Some(Declared {
        rate: long(RATE_AT),
        channels: u8::try_from(little(CHANNELS_AT)).ok()?,
        frames: Frames(data_bytes / block_align),
        digest: None,
    })
}
