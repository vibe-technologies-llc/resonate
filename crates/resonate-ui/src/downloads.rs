use std::time::{Duration, Instant, SystemTime};

use ahash::AHashMap;
use gpui::SharedString;
use resonate_core::{AlbumId, Calendar, SourceId, TrackId, WantId};
use resonate_library::{Found, Issued, Mbid, RecordingRelease, Want};

const READING_THE_RELEASE: &str = "Reading the release…";

const WAITING_ITS_TURN: &str = "Waiting its turn…";

const SEARCHING: &str = "Searching…";

const STARTING: &str = "Starting the download…";

const DOWNLOADING: &str = "Downloading…";

const UNREACHED: &str = "Can't reach the providers · trying again";

const NOT_FOUND_YET: &str = "Not found yet";

pub(crate) const PLAYED_IF_LANDED_WITHIN: Duration = Duration::from_secs(30);

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

    pub(crate) fn while_polling(self, want: Option<WantId>, polling: &Polling) -> Self {
        let asked = want.is_some_and(|want| polling.asking.contains(&want));
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
        self.saying_at(SystemTime::now(), &Calendar::local())
    }

    fn saying_at(self, now: SystemTime, calendar: &Calendar) -> SharedString {
        match self {
            Self::Landing => SharedString::new_static(READING_THE_RELEASE),
            Self::Queued => SharedString::new_static(WAITING_ITS_TURN),
            Self::Downloading { .. } => SharedString::new_static(SEARCHING),
            Self::Unreached { .. } => SharedString::new_static(UNREACHED),
            Self::Retrying { at, .. } if at > now => {
                let minute = calendar.minute_of_the_day(at);
                SharedString::from(format!(
                    "{NOT_FOUND_YET} · next try {:02}:{:02}",
                    minute / 60,
                    minute % 60
                ))
            }
            Self::Retrying { .. } => SharedString::from(format!("{NOT_FOUND_YET} · trying again")),
            Self::Downloaded => SharedString::new_static("Downloaded"),
            Self::GaveUp => SharedString::new_static("Not found"),
            Self::NoProvider => SharedString::new_static("No provider is set up"),
            Self::Unwanted => SharedString::new_static("Couldn't add it"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Fetched {
    pub provider: SourceId,
    pub opening: bool,
    pub received: u64,
}

impl Fetched {
    pub(crate) const fn is_arriving(&self) -> bool {
        self.received > 0
    }
}

pub(crate) fn saying_while(fetching: Fetching, fetched: Option<&Fetched>) -> SharedString {
    match (fetching, fetched) {
        (Fetching::Downloading { .. }, Some(fetched)) if fetched.is_arriving() => {
            SharedString::new_static(DOWNLOADING)
        }
        (Fetching::Downloading { .. }, Some(fetched)) if fetched.opening => {
            SharedString::new_static(STARTING)
        }
        (Fetching::Downloading { .. }, Some(fetched)) => {
            SharedString::from(format!("Searching {}…", fetched.provider))
        }
        (fetching, _) => fetching.saying(),
    }
}

pub(crate) fn summed_up(fetching: &[Fetching], arriving: bool) -> SharedString {
    let underway = fetching.iter().filter(|each| each.is_underway()).count();

    match underway {
        0 => finished(fetching),
        1 => SharedString::new_static(if arriving { DOWNLOADING } else { SEARCHING }),
        more => SharedString::from(format!(
            "{} · {more} left",
            if arriving { DOWNLOADING } else { SEARCHING }
        )),
    }
}

fn finished(fetching: &[Fetching]) -> SharedString {
    let downloaded = fetching
        .iter()
        .filter(|each| **each == Fetching::Downloaded)
        .count();
    let unfinished = fetching.len() - downloaded;
    let said: Vec<String> = [(downloaded, "downloaded"), (unfinished, "not downloaded")]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, what)| format!("{count} {what}"))
        .collect();

    if said.is_empty() {
        SharedString::new_static("Downloads")
    } else {
        SharedString::from(said.join(" · "))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Polling {
    pub asking: Vec<WantId>,
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
    held: Option<TrackId>,
    album: Option<AlbumId>,
}

impl WantStanding {
    pub(crate) fn of(want: &Want) -> Self {
        Self {
            tried: want.tried,
            delivered: want.held.is_some() || want.offered.is_some(),
            misses: want.misses,
            due_at: want.due_at(),
            gave_up: want.gave_up(),
            held: want.held,
            album: Some(want.album),
        }
    }

    pub(crate) const fn held(self) -> Option<TrackId> {
        self.held
    }

    pub(crate) const fn album(self) -> Option<AlbumId> {
        self.album
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

    pub(crate) fn fetching_while(&self, polling: &Polling) -> Fetching {
        self.fetching.while_polling(self.want, polling)
    }
}

#[derive(Clone, Debug)]
struct Awaited {
    recording: Mbid,
    pressed: Instant,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Downloads {
    held: Vec<Download>,
    awaited: Option<Awaited>,
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

    pub(crate) fn play_once_landed(&mut self, recording: Mbid, pressed: Instant) {
        self.awaited = Some(Awaited { recording, pressed });
    }

    pub(crate) fn is_awaited(&self, recording: &Mbid) -> bool {
        self.awaited
            .as_ref()
            .is_some_and(|awaited| &awaited.recording == recording)
    }

    pub(crate) fn landed_to_play(
        &mut self,
        standings: &AHashMap<WantId, WantStanding>,
        now: Instant,
    ) -> Option<TrackId> {
        let awaited = self.awaited.as_ref()?;
        let overdue = now.saturating_duration_since(awaited.pressed) > PLAYED_IF_LANDED_WITHIN;
        let download = self.of(&awaited.recording);
        let held = download
            .filter(|download| download.fetching == Fetching::Downloaded)
            .and_then(Download::want)
            .and_then(|want| standings.get(&want))
            .and_then(|standing| standing.held());
        let still_coming = download.is_some_and(|download| {
            download.fetching.is_underway() || download.fetching == Fetching::Downloaded
        });

        if held.is_some() || overdue || !still_coming {
            self.awaited = None;
        }
        held.filter(|_| !overdue)
    }

    pub(crate) fn restored(&mut self, unfinished: Vec<Unfinished>) {
        for each in unfinished {
            if self.of(&each.found.recording).is_some() || !each.fetching.is_underway() {
                continue;
            }
            self.held.push(Download {
                found: each.found,
                want: Some(each.want),
                queued: each.queued,
                fetching: each.fetching,
            });
        }
    }

    pub(crate) fn provided(&mut self) -> bool {
        let mut moved = false;
        for download in &mut self.held {
            if download.fetching == Fetching::NoProvider && download.want.is_some() {
                download.fetching = Fetching::Queued;
                moved = true;
            }
        }
        moved
    }

    pub(crate) fn dismiss(&mut self, recording: &Mbid) {
        if self.is_awaited(recording) {
            self.awaited = None;
        }
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

#[derive(Clone, Debug)]
pub(crate) struct Unfinished {
    found: Found,
    want: WantId,
    queued: SystemTime,
    fetching: Fetching,
}

impl Unfinished {
    pub(crate) fn of(want: &Want) -> Option<Self> {
        let recording = want.recording.clone()?;
        let release = want.release.clone().map(|id| RecordingRelease {
            id,
            title: want.album_title.clone(),
            date: None,
            disc: Some(want.disc),
            position: Some(want.position),
            issued: Issued::default(),
        });

        Some(Self {
            found: Found {
                recording,
                title: want.title.clone(),
                artist: want.artist.clone().unwrap_or_default(),
                length: want.length,
                release,
                releases: Vec::new(),
                performer: None,
                isrcs: Vec::new(),
            },
            want: want.id,
            queued: want.wanted,
            fetching: WantStanding::of(want).fetching(),
        })
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

    use resonate_library::TRIES_BEFORE_GIVING_UP;

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
            performer: None,
            isrcs: Vec::new(),
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
            asking: asking.into_iter().collect(),
            unheard,
            now,
        }
    }

    fn fetching(downloads: &Downloads, asking: Option<WantId>) -> Vec<Fetching> {
        downloads
            .all()
            .iter()
            .map(|download| download.fetching_while(&polling(asking, false, SystemTime::now())))
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
            held: None,
            album: None,
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

    fn delivered_as(track: Option<TrackId>) -> WantStanding {
        WantStanding {
            held: track,
            ..standing(SystemTime::now(), 0, true)
        }
    }

    #[test]
    fn a_song_pressed_plays_once_it_lands_unless_it_took_longer_than_half_a_minute() {
        let now = SystemTime::now();
        let pressed = Instant::now();
        let track = TrackId::new(9).expect("a track id");
        let mut quick = queued(ECHOES, want(1), now);
        let mut slow = queued(ECHOES, want(1), now);
        let mut unscanned = queued(ECHOES, want(1), now);
        let mut unpressed = queued(ECHOES, want(1), now);
        let landed = AHashMap::from_iter([(want(1), delivered_as(Some(track)))]);
        let offered = AHashMap::from_iter([(want(1), delivered_as(None))]);
        let late = pressed + PLAYED_IF_LANDED_WITHIN + Duration::from_secs(1);

        for downloads in [&mut quick, &mut slow, &mut unscanned] {
            downloads.play_once_landed(found(ECHOES).recording, pressed);
        }
        quick.followed(&landed);
        slow.followed(&landed);
        unscanned.followed(&offered);
        unpressed.followed(&landed);

        assert_eq!(quick.landed_to_play(&landed, pressed), Some(track));
        assert_eq!(quick.landed_to_play(&landed, pressed), None, "played once");
        assert_eq!(slow.landed_to_play(&landed, late), None);
        assert!(!slow.is_awaited(&found(ECHOES).recording));
        assert_eq!(unscanned.landed_to_play(&offered, pressed), None);
        assert!(unscanned.is_awaited(&found(ECHOES).recording));
        assert_eq!(unscanned.landed_to_play(&landed, pressed), Some(track));
        assert_eq!(unpressed.landed_to_play(&landed, pressed), None);
    }

    #[test]
    fn a_song_given_up_or_dismissed_is_no_longer_awaited() {
        let now = SystemTime::now();
        let pressed = Instant::now();
        let mut given_up = queued(ECHOES, want(1), now);
        let mut dismissed = queued(ECHOES, want(1), now);
        let gave_up =
            AHashMap::from_iter([(want(1), standing(now, TRIES_BEFORE_GIVING_UP, false))]);

        given_up.play_once_landed(found(ECHOES).recording, pressed);
        dismissed.play_once_landed(found(ECHOES).recording, pressed);
        given_up.followed(&gave_up);
        dismissed.dismiss(&found(ECHOES).recording);

        assert_eq!(given_up.landed_to_play(&gave_up, pressed), None);
        assert!(!given_up.is_awaited(&found(ECHOES).recording));
        assert!(!dismissed.is_awaited(&found(ECHOES).recording));
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
            due.while_polling(Some(want(1)), &polling(None, true, later)),
            Fetching::Unreached { attempt: 2 }
        );
        assert_eq!(
            Fetching::Queued.while_polling(Some(want(1)), &polling(None, true, now)),
            Fetching::Unreached { attempt: 1 }
        );
        assert_eq!(
            waiting.while_polling(Some(want(1)), &polling(None, true, now)),
            waiting
        );
        assert_eq!(
            due.while_polling(Some(want(1)), &polling(None, false, later)),
            due
        );
        assert_eq!(
            due.while_polling(Some(want(1)), &polling(Some(want(1)), true, later)),
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
    fn a_download_says_which_step_it_is_on_from_the_release_to_the_bytes() {
        let searching = Fetched {
            provider: SourceId::new("resonate").expect("a source name"),
            opening: false,
            received: 0,
        };
        let starting = Fetched {
            opening: true,
            ..searching.clone()
        };
        let arriving = Fetched {
            received: 3 * 1024 * 1024,
            ..starting.clone()
        };
        let downloading = Fetching::Downloading { attempt: 2 };

        assert_eq!(Fetching::Landing.saying(), "Reading the release…");
        assert_eq!(Fetching::Queued.saying(), "Waiting its turn…");
        assert_eq!(saying_while(downloading, None), "Searching…");
        assert_eq!(
            saying_while(downloading, Some(&searching)),
            "Searching resonate…"
        );
        assert_eq!(
            saying_while(downloading, Some(&starting)),
            "Starting the download…"
        );
        assert_eq!(saying_while(downloading, Some(&arriving)), "Downloading…");
        assert_eq!(
            saying_while(Fetching::Queued, Some(&arriving)),
            "Waiting its turn…"
        );
        assert_eq!(
            Fetching::Unreached { attempt: 3 }.saying(),
            "Can't reach the providers · trying again"
        );
    }

    #[test]
    fn a_song_not_found_yet_says_when_it_is_next_tried_on_the_listeners_clock() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(20_000 * 86_400);
        let later = now + Duration::from_secs(14 * 3_600 + 5 * 60);
        let due = Fetching::Retrying { tries: 1, at: now };
        let waiting = Fetching::Retrying {
            tries: 1,
            at: later,
        };

        assert_eq!(
            waiting.saying_at(now, &Calendar::utc()),
            "Not found yet · next try 14:05"
        );
        assert_eq!(
            due.saying_at(now, &Calendar::utc()),
            "Not found yet · trying again"
        );
    }

    #[test]
    fn the_sidebar_says_searching_or_downloading_and_how_many_songs_are_left() {
        let retrying = Fetching::Retrying {
            tries: 1,
            at: SystemTime::UNIX_EPOCH,
        };

        assert_eq!(
            summed_up(
                &[
                    Fetching::Downloading { attempt: 1 },
                    Fetching::Queued,
                    retrying
                ],
                false
            ),
            "Searching… · 3 left"
        );
        assert_eq!(
            summed_up(&[Fetching::Downloaded, retrying, Fetching::Queued], true),
            "Downloading… · 2 left"
        );
        assert_eq!(summed_up(&[Fetching::Queued], false), "Searching…");
        assert_eq!(summed_up(&[Fetching::Landing], false), "Searching…");
        assert_eq!(
            summed_up(
                &[Fetching::Downloaded, Fetching::Downloaded, Fetching::GaveUp],
                false
            ),
            "2 downloaded · 1 not downloaded"
        );
        assert_eq!(summed_up(&[Fetching::GaveUp], false), "1 not downloaded");
    }

    #[test]
    fn what_was_still_underway_comes_back_after_a_restart_and_nothing_finished_does() {
        let now = SystemTime::now();
        let underway = Unfinished {
            found: found(ECHOES),
            want: want(1),
            queued: now,
            fetching: Fetching::Retrying { tries: 2, at: now },
        };
        let given_up = Unfinished {
            found: found(SEAMUS),
            want: want(2),
            queued: now,
            fetching: Fetching::GaveUp,
        };
        let mut downloads = queued(ECHOES, want(1), now);
        downloads.dismiss(&found(ECHOES).recording);

        downloads.restored(vec![underway.clone(), given_up]);
        downloads.restored(vec![underway]);

        assert_eq!(
            fetching(&downloads, None),
            vec![Fetching::Retrying { tries: 2, at: now }]
        );
    }

    #[test]
    fn a_song_nobody_could_fetch_is_queued_once_a_provider_is_set_up() {
        let now = SystemTime::now();
        let mut downloads = Downloads::default();
        downloads.landing(found(ECHOES), now);
        downloads.wanted(&found(ECHOES).recording, want(1), Fetcher::Nobody);

        let unprovided = fetching(&downloads, None);
        let moved = downloads.provided();

        assert_eq!(unprovided, vec![Fetching::NoProvider]);
        assert!(moved);
        assert_eq!(fetching(&downloads, None), vec![Fetching::Queued]);
        assert!(!downloads.provided());
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
