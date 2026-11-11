use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use parking_lot::Mutex;
use resonate_sync::{DeviceId, SpaceId, wire::Refusal};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

pub const ROOM_BACKLOG_BYTES_AT_MOST: usize = 1024 * 1024;
pub const SEATS_BOUND_AT_MOST: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoomNotice {
    Heard { from: DeviceId, sealed: Vec<u8> },
    Arrived(DeviceId),
    Left(DeviceId),
    Replaced,
    Ended,
}

struct Member {
    notices: UnboundedSender<RoomNotice>,
    session: u64,
    backlog: Arc<AtomicUsize>,
}

struct Room {
    token: [u8; 32],
    host_token: [u8; 32],
    host: DeviceId,
    host_session: Option<u64>,
    members: BTreeMap<DeviceId, Member>,
    seats: BTreeMap<DeviceId, [u8; 32]>,
}

#[derive(Default)]
struct Held {
    rooms: BTreeMap<SpaceId, Room>,
    sessions: u64,
}

#[derive(Default)]
pub struct Rooms {
    held: Mutex<Held>,
}

pub struct JoinedRoom {
    pub notices: UnboundedReceiver<RoomNotice>,
    pub session: u64,
    pub members: Vec<DeviceId>,
    pub host: DeviceId,
    pub backlog: Arc<AtomicUsize>,
}

pub struct Asked<'a> {
    pub room: SpaceId,
    pub token: &'a [u8; 32],
    pub host_token: Option<&'a [u8; 32]>,
    pub member: DeviceId,
    pub seat: Option<&'a [u8; 32]>,
}

pub struct RoomLimits {
    pub rooms: usize,
    pub members: usize,
}

impl Rooms {
    pub fn join(
        &self,
        asked: &Asked<'_>,
        limits: &RoomLimits,
        may_host: impl FnOnce() -> bool,
    ) -> Result<JoinedRoom, Refusal> {
        let mut held = self.held.lock();
        held.sessions += 1;
        let session = held.sessions;
        let rooms_held = held.rooms.len();

        let room = match held.rooms.entry(asked.room) {
            std::collections::btree_map::Entry::Occupied(found) => found.into_mut(),
            std::collections::btree_map::Entry::Vacant(free) => {
                let Some(host_token) = asked.host_token else {
                    return Err(Refusal::NoSuchRoom);
                };
                if rooms_held >= limits.rooms {
                    return Err(Refusal::Busy);
                }
                if !may_host() {
                    return Err(Refusal::TooFast);
                }
                free.insert(Room {
                    token: *asked.token,
                    host_token: *host_token,
                    host: asked.member,
                    host_session: None,
                    members: BTreeMap::new(),
                    seats: BTreeMap::new(),
                })
            }
        };

        if room.token != *asked.token {
            return Err(Refusal::Token);
        }
        let hosting = match asked.host_token {
            Some(host_token) if room.host_token == *host_token => true,
            Some(_) => return Err(Refusal::Token),
            None if asked.member == room.host => return Err(Refusal::Token),
            None => false,
        };
        if !hosting {
            match (room.seats.get(&asked.member), asked.seat) {
                (Some(bound), Some(given)) if bound == given => {}
                (Some(_), _) => return Err(Refusal::Token),
                (None, Some(_)) if room.seats.len() >= SEATS_BOUND_AT_MOST => {
                    return Err(Refusal::Busy);
                }
                (None, _) => {}
            }
        }
        if !room.members.contains_key(&asked.member) && room.members.len() >= limits.members {
            return Err(Refusal::Busy);
        }
        if let (false, Some(seat)) = (hosting, asked.seat) {
            room.seats.insert(asked.member, *seat);
        }

        let (notices, received) = unbounded_channel();
        let backlog = Arc::new(AtomicUsize::new(0));
        if let Some(replaced) = room.members.insert(
            asked.member,
            Member {
                notices,
                session,
                backlog: Arc::clone(&backlog),
            },
        ) {
            let _ = replaced.notices.send(RoomNotice::Replaced);
        }
        if hosting {
            room.host = asked.member;
            room.host_session = Some(session);
        }
        let members: Vec<DeviceId> = room
            .members
            .keys()
            .copied()
            .filter(|member| *member != asked.member)
            .collect();
        for other in &members {
            if let Some(other) = room.members.get(other) {
                let _ = other.notices.send(RoomNotice::Arrived(asked.member));
            }
        }

        Ok(JoinedRoom {
            notices: received,
            session,
            members,
            host: room.host,
            backlog,
        })
    }

    pub fn leave(&self, room: SpaceId, member: DeviceId, session: u64) -> Leaving {
        let mut held = self.held.lock();
        let Some(found) = held.rooms.get_mut(&room) else {
            return Leaving::Plainly;
        };
        if found
            .members
            .get(&member)
            .is_none_or(|present| present.session != session)
        {
            return Leaving::Plainly;
        }

        found.members.remove(&member);
        for other in found.members.values() {
            let _ = other.notices.send(RoomNotice::Left(member));
        }
        let host_left = found.host_session == Some(session);
        if host_left {
            found.host_session = None;
        }
        if found.members.is_empty() && found.host_session.is_none() {
            held.rooms.remove(&room);
            return Leaving::Plainly;
        }

        if host_left {
            Leaving::TheHostLeft
        } else {
            Leaving::Plainly
        }
    }

    pub fn end_unless_the_host_came_back(&self, room: SpaceId) -> bool {
        let mut held = self.held.lock();
        let Some(found) = held.rooms.get(&room) else {
            return false;
        };
        if found.host_session.is_some() {
            return false;
        }

        for member in found.members.values() {
            let _ = member.notices.send(RoomNotice::Ended);
        }
        held.rooms.remove(&room);

        true
    }

    pub fn end(&self, room: SpaceId, session: u64) {
        let mut held = self.held.lock();
        let Some(found) = held.rooms.get(&room) else {
            return;
        };
        if found.host_session != Some(session) {
            return;
        }

        for member in found.members.values() {
            let _ = member.notices.send(RoomNotice::Ended);
        }
        held.rooms.remove(&room);
    }

    pub fn say(
        &self,
        room: SpaceId,
        from: DeviceId,
        session: u64,
        to: Option<DeviceId>,
        sealed: &[u8],
    ) {
        let held = self.held.lock();
        let Some(found) = held.rooms.get(&room) else {
            return;
        };

        if found.host_session == Some(session) {
            match to {
                Some(to) => {
                    if let Some(member) = found.members.get(&to) {
                        told(member, from, sealed);
                    }
                }
                None => {
                    for (id, member) in &found.members {
                        if *id != from {
                            told(member, from, sealed);
                        }
                    }
                }
            }
        } else if let Some(host) = found.members.get(&found.host) {
            told(host, from, sealed);
        }
    }

    pub fn kick(&self, room: SpaceId, session: u64, member: DeviceId) -> bool {
        let held = self.held.lock();
        let Some(found) = held.rooms.get(&room) else {
            return false;
        };
        if found.host_session != Some(session) || member == found.host {
            return false;
        }

        found
            .members
            .get(&member)
            .is_some_and(|kicked| kicked.notices.send(RoomNotice::Ended).is_ok())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leaving {
    Plainly,
    TheHostLeft,
}

fn told(member: &Member, from: DeviceId, sealed: &[u8]) {
    let held = member.backlog.load(Ordering::Relaxed);
    if held + sealed.len() > ROOM_BACKLOG_BYTES_AT_MOST {
        return;
    }

    member.backlog.fetch_add(sealed.len(), Ordering::Relaxed);
    let sent = member
        .notices
        .send(RoomNotice::Heard {
            from,
            sealed: sealed.to_vec(),
        })
        .is_ok();
    if !sent {
        member.backlog.fetch_sub(sealed.len(), Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOM: SpaceId = SpaceId([1; 16]);
    const HOST: DeviceId = DeviceId([2; 16]);
    const GUEST: DeviceId = DeviceId([3; 16]);
    const OTHER: DeviceId = DeviceId([4; 16]);
    const TOKEN: [u8; 32] = [5; 32];
    const HOST_TOKEN: [u8; 32] = [6; 32];
    const LIMITS: RoomLimits = RoomLimits {
        rooms: 2,
        members: 3,
    };

    fn host(rooms: &Rooms) -> JoinedRoom {
        rooms
            .join(
                &Asked {
                    room: ROOM,
                    token: &TOKEN,
                    host_token: Some(&HOST_TOKEN),
                    member: HOST,
                    seat: None,
                },
                &LIMITS,
                || true,
            )
            .expect("a room is made")
    }

    fn guest(rooms: &Rooms, member: DeviceId) -> Result<JoinedRoom, Refusal> {
        seated(rooms, member, None)
    }

    fn seated(
        rooms: &Rooms,
        member: DeviceId,
        seat: Option<&[u8; 32]>,
    ) -> Result<JoinedRoom, Refusal> {
        rooms.join(
            &Asked {
                room: ROOM,
                token: &TOKEN,
                host_token: None,
                member,
                seat,
            },
            &LIMITS,
            || true,
        )
    }

    #[test]
    fn a_guest_finds_no_room_until_its_host_made_it_and_a_wrong_token_is_turned_away() {
        let rooms = Rooms::default();

        assert!(matches!(guest(&rooms, GUEST), Err(Refusal::NoSuchRoom)));

        let _host = host(&rooms);
        let wrong = rooms.join(
            &Asked {
                room: ROOM,
                token: &[9; 32],
                host_token: None,
                member: GUEST,
                seat: None,
            },
            &LIMITS,
            || true,
        );
        let joined = guest(&rooms, GUEST).expect("the token fits");

        assert!(matches!(wrong, Err(Refusal::Token)));
        assert_eq!(joined.members, [HOST]);
        assert_eq!(joined.host, HOST);
    }

    #[test]
    fn no_guest_can_claim_the_host_without_its_token() {
        let rooms = Rooms::default();
        let _host = host(&rooms);

        let forged = rooms.join(
            &Asked {
                room: ROOM,
                token: &TOKEN,
                host_token: Some(&TOKEN),
                member: GUEST,
                seat: None,
            },
            &LIMITS,
            || true,
        );

        assert!(matches!(forged, Err(Refusal::Token)));
    }

    #[test]
    fn no_guest_can_take_the_hosts_seat_by_joining_under_its_id() {
        let rooms = Rooms::default();
        let mut hosting = host(&rooms);
        let _guest = guest(&rooms, GUEST).expect("a guest joins");
        let _arrived = hosting.notices.try_recv();

        let seated = guest(&rooms, HOST);

        assert!(matches!(seated, Err(Refusal::Token)));
        assert!(hosting.notices.try_recv().is_err());
        assert_eq!(
            rooms.leave(ROOM, HOST, hosting.session),
            Leaving::TheHostLeft
        );
        assert!(matches!(guest(&rooms, HOST), Err(Refusal::Token)));
    }

    #[test]
    fn no_guest_can_take_another_guests_place_by_joining_under_its_id() {
        let rooms = Rooms::default();
        let _hosting = host(&rooms);
        let mut victim = seated(&rooms, GUEST, Some(&[10; 32])).expect("a guest joins");

        let wrong = seated(&rooms, GUEST, Some(&[11; 32]));
        let none = guest(&rooms, GUEST);

        assert!(matches!(wrong, Err(Refusal::Token)));
        assert!(matches!(none, Err(Refusal::Token)));
        assert!(victim.notices.try_recv().is_err(), "the guest was replaced");
    }

    #[test]
    fn a_guests_place_stays_its_own_after_it_leaves_and_it_takes_it_back_with_its_secret() {
        let rooms = Rooms::default();
        let _hosting = host(&rooms);
        let first = seated(&rooms, GUEST, Some(&[10; 32])).expect("a guest joins");
        rooms.leave(ROOM, GUEST, first.session);

        let stranger = seated(&rooms, GUEST, Some(&[11; 32]));
        let back = seated(&rooms, GUEST, Some(&[10; 32]));

        assert!(matches!(stranger, Err(Refusal::Token)));
        assert!(back.is_ok());
    }

    #[test]
    fn a_guest_connecting_again_with_its_secret_replaces_its_stale_session() {
        let rooms = Rooms::default();
        let _hosting = host(&rooms);
        let mut stale = seated(&rooms, GUEST, Some(&[10; 32])).expect("a guest joins");

        let _again = seated(&rooms, GUEST, Some(&[10; 32])).expect("back");

        assert_eq!(stale.notices.try_recv(), Ok(RoomNotice::Replaced));
    }

    #[test]
    fn a_guest_from_a_build_without_secrets_still_joins_and_comes_back() {
        let rooms = Rooms::default();
        let _hosting = host(&rooms);

        let first = guest(&rooms, GUEST).expect("a guest joins");
        rooms.leave(ROOM, GUEST, first.session);

        assert!(guest(&rooms, GUEST).is_ok());
    }

    #[test]
    fn a_room_binds_only_so_many_places() {
        let rooms = Rooms::default();
        let _hosting = host(&rooms);
        let big = RoomLimits {
            rooms: 2,
            members: 3,
        };
        let join = |index: u32| {
            let mut member = [0; 16];
            member[..4].copy_from_slice(&index.to_be_bytes());
            let joined = rooms.join(
                &Asked {
                    room: ROOM,
                    token: &TOKEN,
                    host_token: None,
                    member: DeviceId(member),
                    seat: Some(&[12; 32]),
                },
                &big,
                || true,
            );
            if let Ok(joined) = &joined {
                rooms.leave(ROOM, DeviceId(member), joined.session);
            }
            joined.map(|_| ())
        };

        for index in 0..SEATS_BOUND_AT_MOST {
            join(u32::try_from(index).expect("a small number")).expect("a place is bound");
        }

        assert!(matches!(join(u32::MAX), Err(Refusal::Busy)));
    }

    #[test]
    fn a_hosts_word_reaches_everyone_and_a_guests_only_the_host() {
        let rooms = Rooms::default();
        let mut hosting = host(&rooms);
        let mut one = guest(&rooms, GUEST).expect("joined");
        let mut other = guest(&rooms, OTHER).expect("joined");
        while hosting.notices.try_recv().is_ok() {}
        while one.notices.try_recv().is_ok() {}

        rooms.say(ROOM, HOST, hosting.session, None, &[1]);
        rooms.say(ROOM, GUEST, one.session, Some(OTHER), &[2]);

        assert_eq!(
            one.notices.try_recv(),
            Ok(RoomNotice::Heard {
                from: HOST,
                sealed: vec![1]
            })
        );
        assert_eq!(
            other.notices.try_recv(),
            Ok(RoomNotice::Heard {
                from: HOST,
                sealed: vec![1]
            })
        );
        assert!(other.notices.try_recv().is_err(), "a guest reached a guest");
        assert_eq!(
            hosting.notices.try_recv(),
            Ok(RoomNotice::Heard {
                from: GUEST,
                sealed: vec![2]
            })
        );
    }

    #[test]
    fn a_host_gone_may_come_back_and_a_room_left_without_one_ends() {
        let rooms = Rooms::default();
        let hosting = host(&rooms);
        let mut one = guest(&rooms, GUEST).expect("joined");

        assert_eq!(
            rooms.leave(ROOM, HOST, hosting.session),
            Leaving::TheHostLeft
        );

        let back = host(&rooms);
        assert!(!rooms.end_unless_the_host_came_back(ROOM));

        assert_eq!(rooms.leave(ROOM, HOST, back.session), Leaving::TheHostLeft);
        assert!(rooms.end_unless_the_host_came_back(ROOM));

        let mut ended = false;
        while let Ok(notice) = one.notices.try_recv() {
            ended |= notice == RoomNotice::Ended;
        }
        assert!(ended);
        assert!(matches!(guest(&rooms, GUEST), Err(Refusal::NoSuchRoom)));
    }

    #[test]
    fn a_full_room_and_a_full_server_turn_away_all_but_one_coming_back() {
        let rooms = Rooms::default();
        let _host = host(&rooms);
        let _one = guest(&rooms, GUEST).expect("joined");
        let _other = guest(&rooms, OTHER).expect("joined");

        assert!(matches!(
            guest(&rooms, DeviceId([9; 16])),
            Err(Refusal::Busy)
        ));
        assert!(guest(&rooms, GUEST).is_ok(), "coming back takes no seat");

        let full = RoomLimits {
            rooms: 1,
            members: 3,
        };
        let another = rooms.join(
            &Asked {
                room: SpaceId([8; 16]),
                token: &TOKEN,
                host_token: Some(&HOST_TOKEN),
                member: HOST,
                seat: None,
            },
            &full,
            || true,
        );
        assert!(matches!(another, Err(Refusal::Busy)));
    }

    #[test]
    fn only_the_host_can_turn_a_guest_out_and_the_host_cannot_be() {
        let rooms = Rooms::default();
        let hosting = host(&rooms);
        let one = guest(&rooms, GUEST).expect("joined");
        let mut other = guest(&rooms, OTHER).expect("joined");

        assert!(!rooms.kick(ROOM, one.session, OTHER));
        assert!(!rooms.kick(ROOM, hosting.session, HOST));
        assert!(rooms.kick(ROOM, hosting.session, OTHER));

        let mut ended = false;
        while let Ok(notice) = other.notices.try_recv() {
            ended |= notice == RoomNotice::Ended;
        }
        assert!(ended);
    }

    #[test]
    fn a_guest_who_reads_nothing_costs_a_bounded_backlog() {
        let rooms = Rooms::default();
        let hosting = host(&rooms);
        let one = guest(&rooms, GUEST).expect("joined");

        rooms.say(
            ROOM,
            HOST,
            hosting.session,
            None,
            &vec![0; ROOM_BACKLOG_BYTES_AT_MOST],
        );
        rooms.say(ROOM, HOST, hosting.session, None, &[0]);

        assert_eq!(
            one.backlog.load(Ordering::Relaxed),
            ROOM_BACKLOG_BYTES_AT_MOST
        );
    }
}
