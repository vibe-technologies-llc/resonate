use std::{
    iter,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

use ahash::AHashMap;
use parking_lot::Mutex;
use resonate_core::{Mbid, SourceId};
use resonate_providers::{
    Asking, Away, Delivered, Identity, Providers, TURNED_TO_APART, WAITED_ON_AFTER_AN_OFFER,
};

use crate::{
    Found, Library,
    supply::{ANSWERS_WITHIN, WANTS_ASKED_AT_ONCE},
};

pub const FORESEEN_FOR: Duration = Duration::from_secs(120);

const SEARCHES_FORESEEN_AT_ONCE: usize = WANTS_ASKED_AT_ONCE.get();

#[derive(Debug)]
pub(crate) struct Foreseen {
    pub(crate) heard: Vec<SourceId>,
    pub(crate) offers: Vec<Delivered>,
    at: Instant,
}

impl Foreseen {
    fn is_fresh(&self) -> bool {
        self.at.elapsed() < FORESEEN_FOR
    }
}

#[derive(Debug, Default)]
pub(crate) struct Foresight {
    held: Mutex<AHashMap<Mbid, Foreseen>>,
    searching: AtomicUsize,
}

struct Searching<'a>(&'a AtomicUsize);

impl Drop for Searching<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Foresight {
    fn begin(&self) -> Option<Searching<'_>> {
        if self.searching.fetch_add(1, Ordering::AcqRel) >= SEARCHES_FORESEEN_AT_ONCE {
            self.searching.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        Some(Searching(&self.searching))
    }

    fn keep(&self, recording: Mbid, heard: Vec<SourceId>, offers: Vec<Delivered>) {
        let mut held = self.held.lock();
        held.retain(|_, foreseen| foreseen.is_fresh());
        held.insert(
            recording,
            Foreseen {
                heard,
                offers,
                at: Instant::now(),
            },
        );
    }

    pub(crate) fn take(&self, recording: &Mbid) -> Option<Foreseen> {
        self.held
            .lock()
            .remove(recording)
            .filter(Foreseen::is_fresh)
    }
}

impl Found {
    fn identity(&self) -> Identity {
        Identity {
            recording: Some(self.recording.clone()),
            release: self.release.as_ref().map(|release| release.id.clone()),
            isrcs: self.isrcs.clone(),
            title: self.title.clone(),
            artist: Some(self.artist.clone()).filter(|artist| !artist.is_empty()),
            album: self.release.as_ref().map(|release| release.title.clone()),
            length: self.length,
            ..Identity::default()
        }
    }
}

impl Library {
    pub fn foresee(&self, providers: &Providers, found: &Found) -> bool {
        if providers.is_narrowed() || !providers.has_a_source() {
            return false;
        }
        let foresight = self.foresight();
        let Some(_searching) = foresight.begin() else {
            return false;
        };

        let answer = providers.first(
            &found.identity(),
            &Asking {
                within: ANSWERS_WITHIN,
                cancelled: &|| false,
                turning_to: &|_| {},
                declined: &|_| false,
                passing: &[],
                apart: TURNED_TO_APART,
                grace: WAITED_ON_AFTER_AN_OFFER,
            },
            &mut Away::default(),
        );
        let Some(delivered) = answer.delivered else {
            return false;
        };
        let offers = iter::once(delivered).chain(answer.held_back).collect();
        foresight.keep(found.recording.clone(), answer.heard, offers);
        true
    }

    pub fn forget_foresight(&self, recording: &Mbid) {
        self.foresight().take(recording);
    }
}
