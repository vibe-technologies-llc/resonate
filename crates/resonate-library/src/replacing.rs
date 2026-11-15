use resonate_core::ReleaseTrackId;

use crate::{Deleted, Track};

#[derive(Clone, Debug)]
pub struct Replaceable {
    pub track: Track,
    pub held: Option<Track>,
    pub release_track: Option<ReleaseTrackId>,
    pub pending: bool,
}

impl Replaceable {
    pub const fn can_be_ticked(&self) -> bool {
        !self.pending && (self.held.is_some() || self.release_track.is_some())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Replacing {
    pub folded: u64,
    pub wanted: u64,
    pub unmatched: u64,
    pub trashed: Deleted,
}
