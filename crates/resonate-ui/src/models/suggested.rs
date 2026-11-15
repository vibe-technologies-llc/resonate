use std::sync::Arc;

use resonate_core::WantId;
use resonate_library::Suggestion;

use super::LibraryModel;

impl LibraryModel {
    pub(super) fn asking_for(&self) -> Vec<WantId> {
        self.running
            .work
            .polling()
            .map_or_else(Vec::new, |progress| progress.asking_all())
    }

    pub(crate) const fn take_the_offers(&mut self) -> Option<Arc<[Suggestion]>> {
        self.landed.offered.take()
    }
}
