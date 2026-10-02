use std::time::SystemTime;

use ahash::AHashMap;
use resonate_core::WantId;
use resonate_library::{Found, Mbid, Want};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fetching {
    Landing,
    Queued,
    Downloading,
    Downloaded,
    Unfound,
    NoProvider,
    Unwanted,
}

impl Fetching {
    pub const fn is_underway(self) -> bool {
        matches!(self, Self::Landing | Self::Queued | Self::Downloading)
    }

    pub const fn can_be_asked_again(self) -> bool {
        matches!(self, Self::Unfound | Self::NoProvider | Self::Unwanted)
    }

    pub const fn saying(self) -> &'static str {
        match self {
            Self::Landing => "Adding to the catalog…",
            Self::Queued => "Queued",
            Self::Downloading => "Downloading…",
            Self::Downloaded => "Downloaded",
            Self::Unfound => "No provider had it",
            Self::NoProvider => "No provider is set up",
            Self::Unwanted => "Couldn't add it",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WantStanding {
    tried: Option<SystemTime>,
    delivered: bool,
}

impl WantStanding {
    pub(crate) fn of(want: &Want) -> Self {
        Self {
            tried: want.tried,
            delivered: want.held.is_some() || want.offered.is_some(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Download {
    pub found: Found,
    want: Option<WantId>,
    queued: SystemTime,
    fetching: Fetching,
}

impl Download {
    pub fn fetching_while(&self, asking: Option<WantId>) -> Fetching {
        match self.fetching {
            Fetching::Queued if asking.is_some() && asking == self.want => Fetching::Downloading,
            held => held,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Downloads {
    held: Vec<Download>,
}

impl Downloads {
    pub(crate) fn all(&self) -> &[Download] {
        &self.held
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.held.is_empty()
    }

    pub(crate) fn of(&self, recording: &Mbid) -> Option<&Download> {
        self.held
            .iter()
            .find(|download| &download.found.recording == recording)
    }

    pub(crate) fn landing(&mut self, found: Found, now: SystemTime) {
        self.dismiss(&found.recording);
        self.held.push(Download {
            found,
            want: None,
            queued: now,
            fetching: Fetching::Landing,
        });
    }

    pub(crate) fn wanted(&mut self, recording: &Mbid, want: WantId, fetched_by: Fetcher) {
        if let Some(download) = self.find(recording) {
            download.want = Some(want);
            download.fetching = match fetched_by {
                Fetcher::AProvider => Fetching::Queued,
                Fetcher::Nobody => Fetching::NoProvider,
            };
        }
    }

    pub(crate) fn unwanted(&mut self, recording: &Mbid) {
        if let Some(download) = self.find(recording) {
            download.fetching = Fetching::Unwanted;
        }
    }

    pub(crate) fn followed(&mut self, standings: &AHashMap<WantId, WantStanding>) -> bool {
        let mut moved = false;
        for download in &mut self.held {
            let (Some(want), Fetching::Queued) = (download.want, download.fetching) else {
                continue;
            };
            let Some(standing) = standings.get(&want) else {
                continue;
            };
            let tried_since_queued = standing.tried.is_some_and(|tried| tried >= download.queued);
            download.fetching = if standing.delivered {
                Fetching::Downloaded
            } else if tried_since_queued {
                Fetching::Unfound
            } else {
                continue;
            };
            moved = true;
        }
        moved
    }

    pub(crate) fn dismiss(&mut self, recording: &Mbid) {
        self.held
            .retain(|download| &download.found.recording != recording);
    }

    pub(crate) fn clear_finished(&mut self) {
        self.held.retain(|download| download.fetching.is_underway());
    }

    fn find(&mut self, recording: &Mbid) -> Option<&mut Download> {
        self.held
            .iter_mut()
            .find(|download| &download.found.recording == recording)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fetcher {
    AProvider,
    Nobody,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const ECHOES: &str = "83d91898-7763-47d7-b03b-b92132375c47";
    const SEAMUS: &str = "b84ee12a-09ef-421b-82de-0441a926375b";

    fn found(recording: &str) -> Found {
        Found {
            recording: Mbid::new(recording).expect("an mbid"),
            title: "Echoes".to_owned(),
            artist: "Pink Floyd".to_owned(),
            length: None,
            release: None,
            releases: Vec::new(),
        }
    }

    fn want(raw: u64) -> WantId {
        WantId::new(raw).expect("a want id")
    }

    fn queued(recording: &str, id: WantId, at: SystemTime) -> Downloads {
        let mut downloads = Downloads::default();
        let found = found(recording);
        let named = found.recording.clone();
        downloads.landing(found, at);
        downloads.wanted(&named, id, Fetcher::AProvider);
        downloads
    }

    fn fetching(downloads: &Downloads, asking: Option<WantId>) -> Vec<Fetching> {
        downloads
            .all()
            .iter()
            .map(|download| download.fetching_while(asking))
            .collect()
    }

    #[test]
    fn a_song_is_landing_then_queued_then_downloading_while_the_poll_asks_for_it() {
        let now = SystemTime::now();
        let mut downloads = Downloads::default();

        downloads.landing(found(ECHOES), now);
        let landing = fetching(&downloads, None);
        downloads.wanted(&found(ECHOES).recording, want(7), Fetcher::AProvider);

        assert_eq!(landing, vec![Fetching::Landing]);
        assert_eq!(fetching(&downloads, None), vec![Fetching::Queued]);
        assert_eq!(fetching(&downloads, Some(want(8))), vec![Fetching::Queued]);
        assert_eq!(
            fetching(&downloads, Some(want(7))),
            vec![Fetching::Downloading]
        );
    }

    #[test]
    fn a_want_delivered_is_downloaded_and_one_tried_since_without_an_answer_is_unfound() {
        let now = SystemTime::now();
        let mut delivered = queued(ECHOES, want(1), now);
        let mut unanswered = queued(SEAMUS, want(2), now);
        let mut tried_before = queued(SEAMUS, want(3), now);

        let standings = AHashMap::from_iter([
            (
                want(1),
                WantStanding {
                    tried: Some(now),
                    delivered: true,
                },
            ),
            (
                want(2),
                WantStanding {
                    tried: Some(now + Duration::from_secs(1)),
                    delivered: false,
                },
            ),
            (
                want(3),
                WantStanding {
                    tried: Some(now - Duration::from_secs(60)),
                    delivered: false,
                },
            ),
        ]);

        assert!(delivered.followed(&standings));
        assert!(unanswered.followed(&standings));
        assert!(!tried_before.followed(&standings));
        assert_eq!(fetching(&delivered, None), vec![Fetching::Downloaded]);
        assert_eq!(fetching(&unanswered, None), vec![Fetching::Unfound]);
        assert_eq!(fetching(&tried_before, None), vec![Fetching::Queued]);
    }

    #[test]
    fn clearing_keeps_what_is_underway_and_a_song_landed_again_starts_over() {
        let now = SystemTime::now();
        let mut downloads = queued(ECHOES, want(1), now);
        downloads.landing(found(SEAMUS), now);
        downloads.unwanted(&found(SEAMUS).recording);

        downloads.clear_finished();
        let cleared = fetching(&downloads, None);
        downloads.landing(found(ECHOES), now);

        assert_eq!(cleared, vec![Fetching::Queued]);
        assert_eq!(fetching(&downloads, None), vec![Fetching::Landing]);
        assert_eq!(downloads.all().len(), 1);
    }
}
