use std::io::Read;

use crate::{Error, Malformed, PackOp, Result};

pub const KEPT_PACKED_AT: i32 = 9;
pub const LARGEST_UNPACKED: usize = 16 * 1024 * 1024;

pub fn packed(bytes: &[u8], level: i32) -> Result<Vec<u8>> {
    zstd::bulk::compress(bytes, level).map_err(|source| Error::Pack {
        op: PackOp::Pack,
        source,
    })
}

pub fn unpacked(packed: &[u8]) -> Result<Vec<u8>> {
    let decoder = zstd::stream::read::Decoder::new(packed).map_err(|source| Error::Pack {
        op: PackOp::Unpack,
        source,
    })?;
    bounded(decoder)
}

pub fn bounded(reader: impl Read) -> Result<Vec<u8>> {
    let mut unpacked = Vec::new();
    reader
        .take(LARGEST_UNPACKED as u64 + 1)
        .read_to_end(&mut unpacked)
        .map_err(|source| Error::Pack {
            op: PackOp::Unpack,
            source,
        })?;
    if unpacked.len() > LARGEST_UNPACKED {
        return Err(Error::Malformed(Malformed::Unpacked));
    }
    Ok(unpacked)
}
