mod asked;
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
    consulted::{Ask, Asking, CONSULTED_AGAIN_AFTER, Consultation, Consulting},
    error::{ArchiveOp, Error, Malformed, PackOp, Result, Store, StoreOp},
    kept::{KEPT_ANSWERS_TOLD_AT_ONCE, KEPT_FRESH_FOR, Kept, KeptAnswer},
    layered::{Layer, Layered, MUSICBRAINZ, Mirrored, Peers},
    mirror::{
        Archives, Building, FORMAT, MIRROR_FILE, Made, Mirror, Phase, built_from,
        held_for_building, mirror_in, settled,
    },
};

pub const KEPT_FILE: &str = "kept.sqlite";

#[cfg(fuzzing)]
pub fn read_a_copy_line(line: &[u8]) {
    mirror::read_a_copy_line(line);
}
