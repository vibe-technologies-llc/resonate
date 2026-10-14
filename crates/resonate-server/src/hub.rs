use std::{collections::BTreeMap, net::SocketAddr};

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
    Replaced,
}

struct Present {
    notices: UnboundedSender<Notice>,
    address: SocketAddr,
    session: u64,
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
        if let Some(replaced) = present.insert(
            device,
            Present {
                notices,
                address,
                session,
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
            if let Some(present) = present.get(other) {
                let _ = present.notices.send(Notice::Arrived(device));
            }
        }
        Some(Joined {
            notices: received,
            session,
            others,
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
        for other in present.values() {
            let _ = other.notices.send(Notice::Left(device));
        }
        if present.is_empty() {
            spaces.remove(&space);
        }
    }

    pub fn tell_all(&self, space: SpaceId, notice: &Notice) {
        if let Some(present) = self.spaces.lock().get(&space) {
            for device in present.values() {
                let _ = device.notices.send(notice.clone());
            }
        }
    }

    pub fn punch(&self, space: SpaceId, from: DeviceId, to: DeviceId, sealed: Vec<u8>) -> bool {
        let spaces = self.spaces.lock();
        let Some(present) = spaces.get(&space) else {
            return false;
        };
        let (Some(asking), Some(asked)) = (present.get(&from), present.get(&to)) else {
            return false;
        };
        asked
            .notices
            .send(Notice::Punched {
                device: from,
                address: asking.address,
                sealed,
            })
            .is_ok()
    }
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
}
