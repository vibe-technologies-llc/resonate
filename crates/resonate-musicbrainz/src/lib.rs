mod asked;
mod bzipped;
mod consulted;
#[cfg(feature = "download")]
pub mod download;
mod error;
mod kept;
mod layered;
pub mod mirror;
mod packed;
pub mod wire;

pub use crate::{
    asked::{Answer, Asked, Entity},
    consulted::{Ask, Asking, CONSULTED_AGAIN_AFTER, Consultation, Consulted, Consulting, Heard},
    error::{ArchiveOp, CopyOp, Error, LandStep, Malformed, PackOp, Result, Store, StoreOp},
    kept::{Dated, KEPT_ANSWERS_TOLD_AT_ONCE, KEPT_FRESH_FOR, Kept, KeptAnswer},
    layered::{Layer, Layered, MUSICBRAINZ, Mirrored, Peers},
    mirror::{
        AMENDED_FILE, AmendedTo, Archives, Building, Built, CopyPart, FORMAT, Keeping, KeptTables,
        MIRROR_FILE, Made, Mirror, Offered, OfferedFile, Phase, TABLES_FILE, built_from,
        exported_after, held_for_building, mirror_in, received_amendments, received_copy,
        removed_beside_the_mirror, removed_copy,
        replicated::{PACKETS_A_BATCH, Packet, Replicated, packet_read, replicated},
        settled,
    },
};

pub const KEPT_FILE: &str = "kept.sqlite";

#[cfg(fuzzing)]
pub fn read_a_copy_line(line: &[u8]) {
    mirror::read_a_copy_line(line);
}

#[cfg(fuzzing)]
pub fn read_pending_changes(rows: &[u8]) {
    mirror::replicated::read_pending_changes(rows);
}

#[cfg(fuzzing)]
pub fn bzip2_decoded(packed: &[u8], at_most: u64) -> std::io::Result<Vec<u8>> {
    bzipped::read_whole(bzipped::decoded(packed), at_most)
}

#[cfg(fuzzing)]
pub fn bzip2_decoded_across(
    packed: &[u8],
    threads: usize,
    at_most: u64,
) -> std::io::Result<Vec<u8>> {
    let owned = std::io::Cursor::new(packed.to_vec());
    bzipped::read_whole(bzipped::decoded_across(owned, threads), at_most)
}
