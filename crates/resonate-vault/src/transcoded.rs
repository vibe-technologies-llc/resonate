use std::ops::RangeInclusive;

use jxl_encoder::jpeg::{JpegData, encode_jpeg_to_jxl_container, read_jpeg};
use jxl_oxide::{JpegReconstructionStatus, JxlImage};

use crate::{
    cover,
    error::{Error, PictureOp, Result},
    key::KEY_BYTES,
    pcm::Digest,
};

const FIRST_MARKER: u8 = 0xc0;
const COMPONENT_COUNTS: [usize; 2] = [1, 3];
const TABLE_AND_SCAN_COUNTS: RangeInclusive<usize> = 1..=4;
const SEAL_TAG: [u8; 4] = *b"rsck";
const SEAL_BYTES: usize = 4 + SEAL_TAG.len() + KEY_BYTES;
const HUFFMAN_TABLE_COUNTS: RangeInclusive<usize> = 2..=8;

pub(crate) struct Transcoded {
    pub(crate) bytes: Vec<u8>,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

pub(crate) fn reconstructs(jxl: &[u8], jpeg: &[u8]) -> bool {
    cover::opened_jxl(jxl)
        .and_then(|picture| reconstructed(jxl, &picture))
        .is_ok_and(|held| held.as_deref() == Some(jpeg))
}

fn sealed(mut jxl: Vec<u8>) -> Vec<u8> {
    let mut digest = Digest::default();
    digest.note(&jxl);

    jxl.extend_from_slice(&(SEAL_BYTES as u32).to_be_bytes());
    jxl.extend_from_slice(&SEAL_TAG);
    jxl.extend_from_slice(digest.settled().as_bytes());
    jxl
}

fn is_sealed(jxl: &[u8]) -> bool {
    let Some(sealed_from) = jxl.len().checked_sub(SEAL_BYTES) else {
        return false;
    };
    let (body, seal) = jxl.split_at(sealed_from);
    let mut digest = Digest::default();
    digest.note(body);

    seal[..4] == (SEAL_BYTES as u32).to_be_bytes()
        && seal[4..8] == SEAL_TAG
        && seal[8..] == *digest.settled().as_bytes()
}

pub(crate) fn of(jpeg: &[u8]) -> Option<Transcoded> {
    let data = read_jpeg(jpeg)
        .inspect_err(|source| tracing::debug!(%source, "a JPEG cover is not transcoded"))
        .ok()?;
    if !encodable(&data) {
        tracing::debug!("a JPEG cover is shaped otherwise than the transcoder takes");
        return None;
    }
    let bytes = encode_jpeg_to_jxl_container(&data)
        .inspect_err(|source| tracing::debug!(%source, "a JPEG cover could not be transcoded"))
        .ok()?;
    Some(Transcoded {
        bytes: sealed(bytes),
        width: data.width,
        height: data.height,
    })
}

fn encodable(data: &JpegData) -> bool {
    COMPONENT_COUNTS.contains(&data.components.len())
        && data
            .components
            .iter()
            .all(|component| (component.quant_idx as usize) < data.quant.len())
        && data
            .marker_order
            .iter()
            .all(|marker| *marker >= FIRST_MARKER)
        && data.app_data.len() == data.app_marker_type.len()
        && data.app_data.iter().all(|segment| !segment.is_empty())
        && TABLE_AND_SCAN_COUNTS.contains(&data.quant.len())
        && HUFFMAN_TABLE_COUNTS.contains(&data.huffman_code.len())
        && data
            .scan_info
            .iter()
            .all(|scan| TABLE_AND_SCAN_COUNTS.contains(&(scan.num_components as usize)))
}

pub(crate) fn reconstructed(jxl: &[u8], picture: &JxlImage) -> Result<Option<Vec<u8>>> {
    match picture.jpeg_reconstruction_status() {
        JpegReconstructionStatus::Unavailable => return Ok(None),
        _ if !is_sealed(jxl) => {
            tracing::warn!("a transcoded JPEG cover is damaged or not this build's own");
            return Err(Error::PictureRead {
                op: PictureOp::Reconstruct,
            });
        }
        JpegReconstructionStatus::Available => {}
        JpegReconstructionStatus::Invalid | JpegReconstructionStatus::NeedMoreData => {
            return Err(Error::PictureRead {
                op: PictureOp::Reconstruct,
            });
        }
    }

    let mut jpeg = Vec::new();
    picture.reconstruct_jpeg(&mut jpeg).map_err(|source| {
        tracing::warn!(%source, "a JPEG could not be reconstructed from a JXL picture");
        Error::PictureRead {
            op: PictureOp::Reconstruct,
        }
    })?;
    Ok(Some(jpeg))
}

#[cfg(test)]
mod tests;
