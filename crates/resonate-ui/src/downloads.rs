use std::time::SystemTime;

use ahash::AHashMap;
use gpui::SharedString;
use resonate_core::WantId;
use resonate_library::{Found, Mbid, TRIES_BEFORE_GIVING_UP, Want};

use crate::format;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fetching {
    Landing,
    Queued,
    Downloading { attempt: u32 },
    Unreached { attempt: u32 },
    Downloaded,
    Retrying { tries: u32, at: SystemTime },
    GaveUp,
    NoProvider,
    Unwanted,
}

impl Fetching {
    pub const fn is_underway(self) -> bool {
        matches!(
            self,
            Self::Landing
                | Self::Queued
                | Self::Downloading { .. }
                | Self::Unreached { .. }
                | Self::Retrying { .. }
        )
    }

    pub const fn can_be_asked_again(self) -> bool {
        matches!(
            self,
            Self::Unreached { .. }
                | Self::Retrying { .. }
                | Self::GaveUp
                | Self::NoProvider
                | Self::Unwanted
        )
    }

    pub const fn can_be_cancelled(self) -> bool {
        matches!(
            self,
            Self::Queued
                | Self::Downloading { .. }
                | Self::Unreached { .. }
                | Self::Retrying { .. }
        )
    }

    pub(crate) fn while_polling(self, want: Option<WantId>, polling: Polling) -> Self {
        let asked = want.is_some() && polling.asking == want;
        match self {
            Self::Queued if asked => Self::Downloading { attempt: 1 },
            Self::Retrying { tries, .. } if asked => Self::Downloading {
                attempt: tries.saturating_add(1),
            },
            Self::Queued if polling.unheard => Self::Unreached { attempt: 1 },
            Self::Retrying { tries, at } if polling.unheard && at <= polling.now => {
                Self::Unreached {
                    attempt: tries.saturating_add(1),
                }
            }
            held => held,
        }
    }

    pub fn saying(self) -> SharedString {
        match self {
            Self::Landing => SharedString::new_static("Adding to the catalog…"),
            Self::Queued => {
                SharedString::from(format!("Queued · attempt 1 of {TRIES_BEFORE_GIVING_UP}"))
            }
            Self::Downloading { attempt } => SharedString::from(format!(
                "Attempt {attempt} of {TRIES_BEFORE_GIVING_UP} · asking providers…"
            )),
            Self::Unreached { attempt } => SharedString::from(format!(
                "Attempt {attempt} of {TRIES_BEFORE_GIVING_UP} · a provider didn't answer, asking again shortly"
            )),
            Self::Downloaded => SharedString::new_static("Downloaded"),
            Self::Retrying { tries, at } => SharedString::from(format!(
                "No match on attempt {tries} of {TRIES_BEFORE_GIVING_UP} · trying again at {}",
                format::time_of_day(at)
            )),
            Self::GaveUp => {
                SharedString::from(format!("No match after {TRIES_BEFORE_GIVING_UP} attempts"))
            }
            Self::NoProvider => SharedString::new_static("No provider is set up"),
            Self::Unwanted => SharedString::new_static("Couldn't add it"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Polling {
    pub asking: Option<WantId>,
    pub unheard: bool,
    pub now: SystemTime,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WantStanding {
    tried: Option<SystemTime>,
    delivered: bool,
    misses: u32,
    due_at: Option<SystemTime>,
    gave_up: bool,
}

impl WantStanding {
    pub(crate) fn of(want: &Want) -> Self {
        Self {
            tried: want.tried,
            delivered: want.held.is_some() || want.offered.is_some(),
            misses: want.misses,
            due_at: want.due_at(),
            gave_up: want.gave_up(),
        }
    }

    pub(crate) fn fetching(self) -> Fetching {
        if self.delivered {
            Fetching::Downloaded
        } else if self.gave_up {
            Fetching::GaveUp
        } else if self.misses > 0 {
            Fetching::Retrying {
                tries: self.misses,
                at: self.due_at.or(self.tried).unwrap_or(SystemTime::UNIX_EPOCH),
            }
        } else {
            Fetching::Queued
        }
    }

    fn fetching_since(self, queued: SystemTime) -> Option<Fetching> {
        if self.delivered {
            return Some(Fetching::Downloaded);
        }
        let tried = self.tried.filter(|tried| *tried >= queued)?;
        if self.gave_up {
            return Some(Fetching::GaveUp);
        }

        (self.misses > 0).then(|| Fetching::Retrying {
            tries: self.misses,
            at: self.due_at.unwrap_or(tried),
        })
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
    pub const fn want(&self) -> Option<WantId> {
        self.want
    }

    pub(crate) fn fetching_while(&self, polling: Polling) -> Fetching {
        self.fetching.while_polling(self.want, polling)
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
            let Some(want) = download.want else {
                continue;
            };
            if !matches!(
                download.fetching,
                Fetching::Queued | Fetching::Retrying { .. }
            ) {
                continue;
            }
            let Some(fetching) = standings
                .get(&want)
                .and_then(|standing| standing.fetching_since(download.queued))
            else {
                continue;
            };
            if fetching != download.fetching {
                download.fetching = fetching;
                moved = true;
            }
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

    fn polling(asking: Option<WantId>, unheard: bool, now: SystemTime) -> Polling {
        Polling {
            asking,
            unheard,
            now,
        }
    }

    fn fetching(downloads: &Downloads, asking: Option<WantId>) -> Vec<Fetching> {
        downloads
            .all()
            .iter()
            .map(|download| download.fetching_while(polling(asking, false, SystemTime::now())))
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
            vec![Fetching::Downloading { attempt: 1 }]
        );
    }

    fn standing(tried: SystemTime, misses: u32, delivered: bool) -> WantStanding {
        WantStanding {
            tried: Some(tried),
            delivered,
            misses,
            due_at: Some(tried + Duration::from_secs(60)),
            gave_up: misses >= TRIES_BEFORE_GIVING_UP,
        }
    }

    #[test]
    fn a_want_delivered_is_downloaded_one_tried_in_vain_is_retried_and_the_last_try_gives_up() {
        let now = SystemTime::now();
        let later = now + Duration::from_secs(1);
        let mut delivered = queued(ECHOES, want(1), now);
        let mut unanswered = queued(SEAMUS, want(2), now);
        let mut tried_before = queued(SEAMUS, want(3), now);
        let mut given_up = queued(SEAMUS, want(4), now);

        let standings = AHashMap::from_iter([
            (want(1), standing(now, 0, true)),
            (want(2), standing(later, 1, false)),
            (want(3), standing(now - Duration::from_secs(60), 2, false)),
            (want(4), standing(later, TRIES_BEFORE_GIVING_UP, false)),
        ]);

        assert!(delivered.followed(&standings));
        assert!(unanswered.followed(&standings));
        assert!(!tried_before.followed(&standings));
        assert!(given_up.followed(&standings));
        assert!(!unanswered.followed(&standings));
        assert_eq!(fetching(&delivered, None), vec![Fetching::Downloaded]);
        assert_eq!(
            fetching(&unanswered, None),
            vec![Fetching::Retrying {
                tries: 1,
                at: later + Duration::from_secs(60)
            }]
        );
        assert_eq!(
            fetching(&unanswered, Some(want(2))),
            vec![Fetching::Downloading { attempt: 2 }],
            "a retry the poll is asking for is downloading"
        );
        assert_eq!(fetching(&tried_before, None), vec![Fetching::Queued]);
        assert_eq!(fetching(&given_up, None), vec![Fetching::GaveUp]);
        assert!(Fetching::GaveUp.can_be_asked_again());
        assert!(!Fetching::GaveUp.is_underway());
    }

    #[test]
    fn a_want_no_provider_answered_is_unreached_rather_than_a_stale_no_match() {
        let now = SystemTime::now();
        let later = now + Duration::from_secs(60);
        let due = Fetching::Retrying { tries: 1, at: now };
        let waiting = Fetching::Retrying {
            tries: 1,
            at: later,
        };

        assert_eq!(
            due.while_polling(Some(want(1)), polling(None, true, later)),
            Fetching::Unreached { attempt: 2 }
        );
        assert_eq!(
            Fetching::Queued.while_polling(Some(want(1)), polling(None, true, now)),
            Fetching::Unreached { attempt: 1 }
        );
        assert_eq!(
            waiting.while_polling(Some(want(1)), polling(None, true, now)),
            waiting
        );
        assert_eq!(
            due.while_polling(Some(want(1)), polling(None, false, later)),
            due
        );
        assert_eq!(
            due.while_polling(Some(want(1)), polling(Some(want(1)), true, later)),
            Fetching::Downloading { attempt: 2 }
        );
        assert!(Fetching::Unreached { attempt: 2 }.is_underway());
        assert!(Fetching::Unreached { attempt: 2 }.can_be_asked_again());
        assert!(Fetching::Queued.can_be_cancelled());
        assert!(due.can_be_cancelled());
        assert!(!Fetching::Landing.can_be_cancelled());
        assert!(!Fetching::Downloaded.can_be_cancelled());
        assert!(!Fetching::GaveUp.can_be_cancelled());
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
