use std::{
    collections::BTreeMap,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use parking_lot::Mutex;
use resonate_sync::{DeviceId, SpaceId};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    Appended,
    Arrived(DeviceId),
    Left(DeviceId),
    Punched {
        device: DeviceId,
        address: SocketAddr,
        sealed: Vec<u8>,
    },
    Compacted(u64),
    Closed(Vec<u8>),
    Replaced,
    FellBehind,
    Relayed {
        from: DeviceId,
        datagram: Vec<u8>,
    },
}

impl Notice {
    const fn may_be_dropped(&self) -> bool {
        matches!(self, Self::Punched { .. } | Self::Relayed { .. })
    }
}

pub const RELAY_BACKLOG_BYTES_AT_MOST: usize = 256 * 1024;
pub const NOTICE_BACKLOG_BYTES_AT_MOST: usize = 1024 * 1024;
pub const A_NOTICE_WEIGHS_AT_LEAST: usize = A_RELAYED_DATAGRAM_WEIGHS_AT_LEAST;

pub fn notice_weight(notice: &Notice) -> usize {
    match notice {
        Notice::Replaced | Notice::FellBehind => 0,
        Notice::Relayed { datagram, .. } => relayed_weight(datagram.len()),
        Notice::Punched { sealed, .. } | Notice::Closed(sealed) => {
            sealed.len().max(A_NOTICE_WEIGHS_AT_LEAST)
        }
        Notice::Appended | Notice::Arrived(_) | Notice::Left(_) | Notice::Compacted(_) => {
            A_NOTICE_WEIGHS_AT_LEAST
        }
    }
}

struct Present {
    notices: UnboundedSender<Notice>,
    address: SocketAddr,
    session: u64,
    relaying: bool,
    behind: bool,
    backlog: Arc<AtomicUsize>,
}

impl Present {
    fn queue(&mut self, notice: Notice) -> bool {
        let weight = notice_weight(&notice);
        let droppable = notice.may_be_dropped();
        let most = if droppable {
            RELAY_BACKLOG_BYTES_AT_MOST
        } else {
            NOTICE_BACKLOG_BYTES_AT_MOST
        };
        if self.backlog.load(Ordering::Relaxed) + weight > most {
            if !droppable && !self.behind {
                self.behind = true;
                let _ = self.notices.send(Notice::FellBehind);
            }
            return false;
        }

        self.backlog.fetch_add(weight, Ordering::Relaxed);
        let sent = self.notices.send(notice).is_ok();
        if !sent {
            self.backlog.fetch_sub(weight, Ordering::Relaxed);
        }
        sent
    }
}

#[derive(Default)]
pub struct Hub {
    spaces: Mutex<BTreeMap<SpaceId, BTreeMap<DeviceId, Present>>>,
    sessions: Mutex<u64>,
}

pub struct Joined {
    pub notices: UnboundedReceiver<Notice>,
    pub session: u64,
    pub others: Vec<DeviceId>,
    pub backlog: Arc<AtomicUsize>,
}

impl Hub {
    pub fn join(
        &self,
        space: SpaceId,
        device: DeviceId,
        address: SocketAddr,
        most: usize,
    ) -> Option<Joined> {
        let session = {
            let mut sessions = self.sessions.lock();
            *sessions += 1;
            *sessions
        };
        let mut spaces = self.spaces.lock();
        let present = spaces.entry(space).or_default();
        if !present.contains_key(&device) && present.len() >= most {
            return None;
        }
        let (notices, received) = unbounded_channel();
        let backlog = Arc::new(AtomicUsize::new(0));
        if let Some(replaced) = present.insert(
            device,
            Present {
                notices,
                address,
                session,
                relaying: false,
                behind: false,
                backlog: Arc::clone(&backlog),
            },
        ) {
            let _ = replaced.notices.send(Notice::Replaced);
        }
        let others = present
            .keys()
            .copied()
            .filter(|other| *other != device)
            .collect::<Vec<_>>();
        for other in &others {
            if let Some(present) = present.get_mut(other) {
                present.queue(Notice::Arrived(device));
            }
        }
        Some(Joined {
            notices: received,
            session,
            others,
            backlog,
        })
    }

    pub fn leave(&self, space: SpaceId, device: DeviceId, session: u64) {
        let mut spaces = self.spaces.lock();
        let Some(present) = spaces.get_mut(&space) else {
            return;
        };
        if present
            .get(&device)
            .is_none_or(|present| present.session != session)
        {
            return;
        }
        present.remove(&device);
        for other in present.values_mut() {
            other.queue(Notice::Left(device));
        }
        if present.is_empty() {
            spaces.remove(&space);
        }
    }

    pub fn tell_all(&self, space: SpaceId, notice: &Notice) {
        if let Some(present) = self.spaces.lock().get_mut(&space) {
            for device in present.values_mut() {
                device.queue(notice.clone());
            }
        }
    }

    pub fn punch(&self, space: SpaceId, from: DeviceId, to: DeviceId, sealed: Vec<u8>) -> bool {
        let mut spaces = self.spaces.lock();
        let Some(present) = spaces.get_mut(&space) else {
            return false;
        };
        let Some(address) = present.get(&from).map(|asking| asking.address) else {
            return false;
        };
        let Some(asked) = present.get_mut(&to) else {
            return false;
        };
        asked.queue(Notice::Punched {
            device: from,
            address,
            sealed,
        })
    }
}

impl Hub {
    pub fn relaying(&self, space: SpaceId, device: DeviceId, session: u64) {
        if let Some(present) = self
            .spaces
            .lock()
            .get_mut(&space)
            .and_then(|present| present.get_mut(&device))
            .filter(|present| present.session == session)
        {
            present.relaying = true;
        }
    }

    pub fn relay(&self, space: SpaceId, from: DeviceId, to: DeviceId, datagram: Vec<u8>) -> bool {
        let mut spaces = self.spaces.lock();
        let Some(asked) = spaces
            .get_mut(&space)
            .and_then(|present| present.get_mut(&to))
        else {
            return false;
        };
        asked.relaying && asked.queue(Notice::Relayed { from, datagram })
    }
}

pub const A_RELAYED_DATAGRAM_WEIGHS_AT_LEAST: usize = 64;

pub fn relayed_weight(length: usize) -> usize {
    length.max(A_RELAYED_DATAGRAM_WEIGHS_AT_LEAST)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPACE: SpaceId = SpaceId([1; 16]);
    const DESK: DeviceId = DeviceId([2; 16]);
    const PHONE: DeviceId = DeviceId([3; 16]);

    fn address(port: u16) -> SocketAddr {
        SocketAddr::from(([203, 0, 113, 1], port))
    }

    #[test]
    fn a_device_joining_is_told_who_is_here_and_they_are_told_of_it() {
        let hub = Hub::default();
        let mut desk = hub.join(SPACE, DESK, address(1), 4).expect("joined");
        let phone = hub.join(SPACE, PHONE, address(2), 4).expect("joined");

        assert_eq!(phone.others, [DESK]);
        assert_eq!(desk.notices.try_recv(), Ok(Notice::Arrived(PHONE)));

        hub.leave(SPACE, PHONE, phone.session);
        assert_eq!(desk.notices.try_recv(), Ok(Notice::Left(PHONE)));
    }

    #[test]
    fn a_full_space_turns_another_device_away_but_not_one_coming_back() {
        let hub = Hub::default();
        let mut first = hub.join(SPACE, DESK, address(1), 1).expect("joined");

        assert!(hub.join(SPACE, PHONE, address(2), 1).is_none());
        let again = hub.join(SPACE, DESK, address(3), 1).expect("back");
        assert_eq!(first.notices.try_recv(), Ok(Notice::Replaced));

        hub.leave(SPACE, DESK, first.session);
        assert!(hub.join(SPACE, PHONE, address(2), 1).is_none());
        hub.leave(SPACE, DESK, again.session);
        assert!(hub.join(SPACE, PHONE, address(2), 1).is_some());
    }

    #[test]
    fn a_punch_carries_the_askers_address_to_the_asked() {
        let hub = Hub::default();
        let _desk = hub.join(SPACE, DESK, address(1), 4).expect("joined");
        let mut phone = hub.join(SPACE, PHONE, address(2), 4).expect("joined");

        assert!(hub.punch(SPACE, DESK, PHONE, vec![7]));
        assert_eq!(
            phone.notices.try_recv(),
            Ok(Notice::Punched {
                device: DESK,
                address: address(1),
                sealed: vec![7]
            })
        );
        assert!(!hub.punch(SPACE, DESK, DeviceId([9; 16]), vec![]));
    }

    #[test]
    fn a_datagram_is_relayed_only_to_a_device_that_said_it_relays() {
        let hub = Hub::default();
        let desk = hub.join(SPACE, DESK, address(1), 4).expect("joined");
        let mut phone = hub.join(SPACE, PHONE, address(2), 4).expect("joined");

        let before = hub.relay(SPACE, DESK, PHONE, vec![1, 2]);
        hub.relaying(SPACE, PHONE, phone.session);
        let after = hub.relay(SPACE, DESK, PHONE, vec![1, 2]);
        let nowhere = hub.relay(SPACE, DESK, DeviceId([9; 16]), vec![1]);

        assert!(!before);
        assert!(after);
        assert!(!nowhere);
        assert_eq!(
            phone.notices.try_recv(),
            Ok(Notice::Relayed {
                from: DESK,
                datagram: vec![1, 2]
            })
        );
        assert_eq!(
            desk.backlog.load(Ordering::Relaxed),
            A_NOTICE_WEIGHS_AT_LEAST,
            "only the arrival of the phone was queued for the desk"
        );
        assert_eq!(
            phone.backlog.load(Ordering::Relaxed),
            A_RELAYED_DATAGRAM_WEIGHS_AT_LEAST
        );
    }

    #[test]
    fn empty_datagrams_fill_a_backlog_as_surely_as_full_ones() {
        let hub = Hub::default();
        let _desk = hub.join(SPACE, DESK, address(1), 4).expect("joined");
        let phone = hub.join(SPACE, PHONE, address(2), 4).expect("joined");
        hub.relaying(SPACE, PHONE, phone.session);

        let fitting = RELAY_BACKLOG_BYTES_AT_MOST / A_RELAYED_DATAGRAM_WEIGHS_AT_LEAST;
        let taken = (0..fitting)
            .filter(|_| hub.relay(SPACE, DESK, PHONE, Vec::new()))
            .count();

        assert_eq!(taken, fitting);
        assert!(!hub.relay(SPACE, DESK, PHONE, Vec::new()));
    }

    #[test]
    fn a_device_that_has_not_read_what_was_relayed_is_sent_no_more_than_a_backlog() {
        let hub = Hub::default();
        let _desk = hub.join(SPACE, DESK, address(1), 4).expect("joined");
        let phone = hub.join(SPACE, PHONE, address(2), 4).expect("joined");
        hub.relaying(SPACE, PHONE, phone.session);

        let whole = hub.relay(SPACE, DESK, PHONE, vec![0; RELAY_BACKLOG_BYTES_AT_MOST]);
        let over = hub.relay(SPACE, DESK, PHONE, vec![0; 1]);
        phone
            .backlog
            .fetch_sub(RELAY_BACKLOG_BYTES_AT_MOST, Ordering::Relaxed);
        let after_reading = hub.relay(SPACE, DESK, PHONE, vec![0; 1]);

        assert!(whole);
        assert!(!over);
        assert!(after_reading);
    }

    #[test]
    fn a_device_that_reads_nothing_is_queued_no_more_than_a_backlog_of_punches() {
        let hub = Hub::default();
        let _desk = hub.join(SPACE, DESK, address(1), 4).expect("joined");
        let mut phone = hub.join(SPACE, PHONE, address(2), 4).expect("joined");
        let sealed = vec![0; 4096];

        let taken = (0..1000)
            .filter(|_| hub.punch(SPACE, DESK, PHONE, sealed.clone()))
            .count();

        assert_eq!(taken, RELAY_BACKLOG_BYTES_AT_MOST / sealed.len());
        let mut queued = 0;
        while let Ok(notice) = phone.notices.try_recv() {
            queued += notice_weight(&notice);
        }
        assert!(queued <= RELAY_BACKLOG_BYTES_AT_MOST);
    }

    #[test]
    fn a_punch_is_charged_to_the_backlog_until_it_is_read() {
        let hub = Hub::default();
        let _desk = hub.join(SPACE, DESK, address(1), 4).expect("joined");
        let phone = hub.join(SPACE, PHONE, address(2), 4).expect("joined");

        hub.punch(SPACE, DESK, PHONE, vec![7; 1000]);

        assert_eq!(phone.backlog.load(Ordering::Relaxed), 1000);
    }

    #[test]
    fn a_device_that_reads_nothing_is_told_it_fell_behind_rather_than_queued_everything() {
        let hub = Hub::default();
        let _desk = hub.join(SPACE, DESK, address(1), 4).expect("joined");
        let mut phone = hub.join(SPACE, PHONE, address(2), 4).expect("joined");
        let closing = Notice::Closed(vec![0; 256 * 1024]);

        for _ in 0..100 {
            hub.tell_all(SPACE, &closing);
        }

        let mut queued = 0;
        let mut behind = 0;
        while let Ok(notice) = phone.notices.try_recv() {
            queued += notice_weight(&notice);
            behind += usize::from(notice == Notice::FellBehind);
        }
        assert!(queued <= NOTICE_BACKLOG_BYTES_AT_MOST);
        assert_eq!(behind, 1, "told once, however much more came");
    }

    #[test]
    fn a_device_that_reads_what_it_is_sent_is_never_told_it_fell_behind() {
        let hub = Hub::default();
        let _desk = hub.join(SPACE, DESK, address(1), 4).expect("joined");
        let mut phone = hub.join(SPACE, PHONE, address(2), 4).expect("joined");

        for _ in 0..10_000 {
            hub.tell_all(SPACE, &Notice::Appended);
            let notice = phone.notices.try_recv().expect("a notice");
            phone
                .backlog
                .fetch_sub(notice_weight(&notice), Ordering::Relaxed);
            assert_eq!(notice, Notice::Appended);
        }
    }

    #[test]
    fn a_relay_declared_by_a_session_that_was_replaced_does_not_count() {
        let hub = Hub::default();
        let _desk = hub.join(SPACE, DESK, address(1), 4).expect("joined");
        let old = hub.join(SPACE, PHONE, address(2), 4).expect("joined");
        let _new = hub.join(SPACE, PHONE, address(3), 4).expect("joined");

        hub.relaying(SPACE, PHONE, old.session);

        assert!(!hub.relay(SPACE, DESK, PHONE, vec![1]));
    }
}
