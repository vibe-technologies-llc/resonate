use resonate_core::Recording;

use crate::{LocalPlaylistId, Refused};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalShared {
    pub at: LocalPlaylistId,
    pub name: String,
    pub entries: Vec<Option<Recording>>,
}

pub trait SharedCatalog: Send + Sync + 'static {
    fn shared_read(&self, at: LocalPlaylistId) -> Result<Option<LocalShared>, Refused>;

    fn shared_write(
        &self,
        at: Option<LocalPlaylistId>,
        name: &str,
        entries: &[Recording],
        editable: bool,
    ) -> Result<LocalShared, Refused>;

    fn shared_flag(
        &self,
        at: LocalPlaylistId,
        editable: bool,
    ) -> Result<Option<LocalShared>, Refused>;

    fn shared_unflag(&self, at: LocalPlaylistId) -> Result<(), Refused>;

    fn shared_held_as(&self, recording: &Recording) -> Result<Option<Recording>, Refused>;

    fn shared_revision(&self) -> u64;
}
