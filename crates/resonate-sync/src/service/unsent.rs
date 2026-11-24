use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use super::{RELAY_DIAL_WITHIN, SERVER_ANSWERS_WITHIN, Work};
use crate::{DeviceId, peer::DIAL_WITHIN, remote::CONTROL_ANSWERED_WITHIN};

pub(super) const UNREACHED_AFTER: Duration = SERVER_ANSWERS_WITHIN
    .saturating_add(DIAL_WITHIN)
    .saturating_add(RELAY_DIAL_WITHIN);

pub(super) struct Awaited {
    sent_by: Instant,
    waiting: Arc<AtomicBool>,
}

pub(super) struct Awaiting {
    answered_by: Instant,
    waiting: Arc<AtomicBool>,
}

pub(super) fn awaited(asked_at: Instant) -> (Awaited, Awaiting) {
    let waiting = Arc::new(AtomicBool::new(true));
    let sent_by = asked_at + DIAL_WITHIN;

    let awaited = Awaited {
        sent_by,
        waiting: Arc::clone(&waiting),
    };
    let awaiting = Awaiting {
        answered_by: sent_by + CONTROL_ANSWERED_WITHIN,
        waiting,
    };
    (awaited, awaiting)
}

impl Awaited {
    fn lapsed(&self, now: Instant) -> bool {
        now >= self.sent_by || !self.waiting.load(Ordering::Acquire)
    }
}

impl Awaiting {
    pub(super) fn left(&self) -> Duration {
        self.answered_by.saturating_duration_since(Instant::now())
    }
}

impl Drop for Awaiting {
    fn drop(&mut self) {
        self.waiting.store(false, Ordering::Release);
    }
}

impl Work {
    pub(super) fn unawaited(&self, now: Instant, watched: bool) -> bool {
        match self {
            Self::Control { awaited, .. } => awaited.lapsed(now),
            Self::Watch => !watched,
            Self::Fetch { .. } | Self::Follow | Self::Consult { .. } | Self::Export(_) => false,
        }
    }

    fn unreached_for_long(&self, tried_since: Instant, now: Instant) -> bool {
        matches!(self, Self::Fetch { asked: true, .. })
            && now.saturating_duration_since(tried_since) >= UNREACHED_AFTER
    }
}

struct Queued {
    work: Work,
    tried_since: Instant,
}

#[derive(Default)]
pub(super) struct Unsent(BTreeMap<DeviceId, Vec<Queued>>);

impl Unsent {
    pub(super) fn holds(&self, device: DeviceId) -> bool {
        self.0.contains_key(&device)
    }

    pub(super) fn devices(&self) -> Vec<DeviceId> {
        self.0.keys().copied().collect()
    }

    pub(super) fn queue(&mut self, device: DeviceId, work: Work, now: Instant) -> bool {
        let first = !self.holds(device);
        let queued = self.0.entry(device).or_default();
        let followed_already = matches!(work, Work::Follow)
            && queued
                .iter()
                .any(|queued| matches!(queued.work, Work::Follow));

        if !followed_already {
            queued.push(Queued {
                work,
                tried_since: now,
            });
        }
        first
    }

    pub(super) fn tried(&mut self, device: DeviceId, now: Instant) {
        for queued in self.0.get_mut(&device).into_iter().flatten() {
            queued.tried_since = now;
        }
    }

    pub(super) fn taken(&mut self, device: DeviceId) -> Vec<Work> {
        self.0
            .remove(&device)
            .unwrap_or_default()
            .into_iter()
            .map(|queued| queued.work)
            .collect()
    }

    pub(super) fn lapsed(
        &mut self,
        now: Instant,
        watched: impl Fn(DeviceId) -> bool,
    ) -> Vec<(DeviceId, Work)> {
        let mut lapsed = Vec::new();

        self.0.retain(|device, queued| {
            let watched = watched(*device);
            let (gone, kept) = queued.drain(..).partition::<Vec<_>, _>(|queued| {
                queued.work.unawaited(now, watched)
                    || queued.work.unreached_for_long(queued.tried_since, now)
            });

            lapsed.extend(gone.into_iter().map(|queued| (*device, queued.work)));
            *queued = kept;
            !queued.is_empty()
        });
        lapsed
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use resonate_core::{Recording, ZoneId};

    use super::*;
    use crate::{
        remote::RemoteCommand,
        service::{Shown, TransferState},
    };

    fn song(title: &str) -> Recording {
        Recording {
            mbid: None,
            title: title.into(),
            artist: "Pink Floyd".into(),
            album: "Meddle".into(),
            length: None,
        }
    }

    fn asked_to_play(asked_at: Instant) -> (Work, Awaiting) {
        let (awaited, awaiting) = awaited(asked_at);
        let (answer, _) = mpsc::channel();

        let work = Work::Control {
            zone: ZoneId::FIRST,
            command: RemoteCommand::Play,
            queue_seen: None,
            answer,
            awaited,
        };
        (work, awaiting)
    }

    fn kinds(lapsed: &[(DeviceId, Work)]) -> Vec<&'static str> {
        lapsed
            .iter()
            .map(|(_, work)| match work {
                Work::Control { .. } => "control",
                Work::Watch => "watch",
                Work::Fetch { asked: true, .. } => "asked fetch",
                Work::Fetch { asked: false, .. } => "followed fetch",
                Work::Follow => "follow",
                Work::Consult { .. } => "consult",
                Work::Export(_) => "export",
            })
            .collect()
    }

    #[test]
    fn a_command_lapses_once_its_asker_stops_waiting_or_it_could_no_longer_be_answered_in_time() {
        let device = DeviceId([7; 16]);
        let asked_at = Instant::now();
        let mut unsent = Unsent::default();

        let (waited_on, awaiting) = asked_to_play(asked_at);
        let (given_up, gave_up) = asked_to_play(asked_at);

        unsent.queue(device, waited_on, asked_at);
        unsent.queue(device, given_up, asked_at);
        drop(gave_up);

        let at_once = unsent.lapsed(asked_at, |_| true);
        let before_the_dial_ran_out = unsent.lapsed(asked_at + DIAL_WITHIN / 2, |_| true);
        let once_it_ran_out = unsent.lapsed(asked_at + DIAL_WITHIN, |_| true);

        assert_eq!(kinds(&at_once), ["control"]);
        assert!(before_the_dial_ran_out.is_empty());
        assert_eq!(kinds(&once_it_ran_out), ["control"]);
        assert!(!unsent.holds(device));
        assert!(awaiting.answered_by >= asked_at + DIAL_WITHIN + CONTROL_ANSWERED_WITHIN);
    }

    #[test]
    fn an_asked_fetch_lapses_once_its_device_goes_unreached_and_its_rows_stop_waiting() {
        let device = DeviceId([7; 16]);
        let queued_at = Instant::now();
        let mut unsent = Unsent::default();
        let mut shown = Shown::default();

        for title in ["Echoes", "Time"] {
            shown.queued(&song(title), device);
        }
        unsent.queue(
            device,
            Work::Fetch {
                recordings: vec![song("Echoes")],
                asked: true,
            },
            queued_at,
        );
        unsent.queue(
            device,
            Work::Fetch {
                recordings: vec![song("Time")],
                asked: false,
            },
            queued_at,
        );
        unsent.queue(device, Work::Follow, queued_at);
        unsent.queue(device, Work::Watch, queued_at);

        let retried_at = queued_at + UNREACHED_AFTER / 2;

        unsent.tried(device, retried_at);

        let unwatched = unsent.lapsed(queued_at + UNREACHED_AFTER, |_| false);
        let lapsed = unsent.lapsed(retried_at + UNREACHED_AFTER, |_| false);

        for (from, work) in &lapsed {
            if let Work::Fetch { recordings, .. } = work {
                shown.unreached(recordings, *from);
            }
        }

        assert_eq!(kinds(&unwatched), ["watch"]);
        assert_eq!(kinds(&lapsed), ["asked fetch"]);
        assert!(matches!(
            unsent.taken(device).as_slice(),
            [Work::Fetch { asked: false, .. }, Work::Follow]
        ));
        assert_eq!(shown.transfers[0].state, TransferState::Failed);
        assert_eq!(shown.transfers[1].state, TransferState::Waiting);
    }
}
