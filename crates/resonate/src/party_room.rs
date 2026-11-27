use std::{
    mem,
    sync::{Arc, LazyLock},
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::Sender;
use parking_lot::Mutex;
use resonate_core::Resumable;
use resonate_engine::{Command, Placement, Player, QueueItem, Seat, ZoneId, Zones};
use resonate_library::Library;
use resonate_sync::{
    DeviceId, Server,
    party::{
        Going, Party, PartyChat, PartyFinder as _, PartyFollowed, PartyShown, PartyWantingASong,
        RequestId,
    },
    shared::{Invited, invitation_in},
};
use resonate_ui::{Followed, ListOffered, PartyRequestId};

use crate::{
    error::{Error, Result},
    party::{Finder, LOOK_AT_THE_PARTY_EVERY, ZoneSeat, settings_for},
};

const THE_WINDOW_COMES_WITHIN: Duration = Duration::from_secs(30);

struct Room {
    going: Going,
    zone: ZoneId,
    player: Arc<Player>,
    finder: Arc<Finder>,
}

#[derive(Clone)]
pub(crate) struct Seats {
    pub(crate) zones: Arc<Zones>,
    pub(crate) library: Arc<Library>,
    pub(crate) served: crate::Served,
    pub(crate) server: Server,
    pub(crate) trusted: Option<std::path::PathBuf>,
    pub(crate) name: String,
    pub(crate) time_server: Option<String>,
    pub(crate) told: Sender<Followed>,
}

enum Offering {
    Waiting(Vec<String>),
    Told(Sender<Followed>),
}

impl Offering {
    fn offer(&mut self, link: String) {
        match self {
            Self::Waiting(links) => links.push(link),
            Self::Told(told) => tell_of(link, told),
        }
    }

    fn opened_to(&mut self, told: Sender<Followed>) {
        let waiting = mem::replace(self, Self::Told(told.clone()));
        if let Self::Waiting(links) = waiting {
            for link in links {
                tell_of(link, &told);
            }
        }
    }

    fn closed(&mut self) {
        *self = Self::Waiting(Vec::new());
    }
}

static SEATS: LazyLock<Mutex<Option<Seats>>> = LazyLock::new(|| Mutex::new(None));

static ROOM: LazyLock<Mutex<Option<Room>>> = LazyLock::new(|| Mutex::new(None));

static CHANGING: Mutex<()> = Mutex::new(());

static OFFERING: Mutex<Offering> = Mutex::new(Offering::Waiting(Vec::new()));

fn tell_of(link: String, told: &Sender<Followed>) {
    let followed = match invitation_in(&link) {
        Some(invitation) if invitation.to == Invited::Playlist => {
            let own_server = crate::together::names_the_server_in_use(&invitation);
            let server = invitation
                .server
                .unwrap_or_else(crate::together::server_in_use);

            Followed::ListOffered(ListOffered {
                link,
                server,
                own_server,
            })
        }
        _ => Followed::PartyOffered(link),
    };
    if told.send(followed).is_err() {
        tracing::warn!("a link came while no window was left to ask about it");
    }
}

pub(crate) fn in_the_window(seats: Seats) {
    let told = seats.told.clone();

    *SEATS.lock() = Some(seats);

    OFFERING.lock().opened_to(told);
}

pub(crate) fn out_of_the_window() {
    leave();
    *SEATS.lock() = None;
    OFFERING.lock().closed();
}

#[cfg(test)]
pub(crate) fn offers_waiting() -> Vec<String> {
    match &*OFFERING.lock() {
        Offering::Waiting(links) => links.clone(),
        Offering::Told(_) => Vec::new(),
    }
}

pub(crate) fn offer(text: &str) -> bool {
    if invitation_in(text).is_none() {
        return false;
    }

    OFFERING.lock().offer(text.to_owned());

    true
}

pub(crate) fn join_in(text: &str, zone: ZoneId) -> bool {
    let Some(invitation) = invitation_in(text).filter(|one| one.to == Invited::Party) else {
        return false;
    };

    join(&invitation.code, invitation.server, zone)
        .inspect_err(|error| tracing::warn!(%error, "a party could not be joined"))
        .is_ok()
}

fn opened(party: Party, player: Arc<Player>, zone: ZoneId, seats: &Seats) -> Result<Room> {
    let finder = Arc::new(Finder {
        library: Some(Arc::clone(&seats.library)),
        services: seats.served.services(),
    });
    let seat = ZoneSeat {
        seat: Seat::on(Arc::clone(&player)),
        library: Some(Arc::clone(&seats.library)),
    };

    Ok(Room {
        going: Going::begin(party, seat, Arc::clone(&finder)).map_err(Error::Party)?,
        zone,
        player,
        finder,
    })
}

fn replaced<Held, Prepared>(
    slot: &Mutex<Option<Held>>,
    changing: &Mutex<()>,
    prepare: impl FnOnce() -> Result<Prepared>,
    open: impl FnOnce(Prepared) -> Result<Held>,
) -> Result<()> {
    let _changing = changing.lock();
    let prepared = prepare()?;

    let left = slot.lock().take();

    drop(left);

    let held = open(prepared)?;
    let replaced = slot.lock().replace(held);

    drop(replaced);

    Ok(())
}

fn seated_on(zone: ZoneId) -> Result<(Seats, Arc<Player>)> {
    let seats = SEATS.lock().clone().ok_or(Error::PartyNeedsAServer)?;
    let player = seats
        .zones
        .zone(zone)
        .map(|zone| zone.player)
        .ok_or(Error::PartyNeedsAServer)?;

    Ok((seats, player))
}

pub(crate) fn begin(zone: ZoneId) -> Result<()> {
    replaced(
        &ROOM,
        &CHANGING,
        || {
            let (seats, player) = seated_on(zone)?;
            let settings = settings_for(
                seats.server.clone().address(),
                seats.trusted.clone(),
                seats.name.clone(),
                seats.time_server.clone(),
            )?;
            let party = Party::host(settings).map_err(Error::Party)?;

            Ok((party, player, seats))
        },
        |(party, player, seats)| opened(party, player, zone, &seats),
    )
}

pub(crate) fn join(code: &str, server: Option<String>, zone: ZoneId) -> Result<()> {
    replaced(
        &ROOM,
        &CHANGING,
        || {
            let (seats, player) = seated_on(zone)?;
            let settings = settings_for(
                server.or_else(|| seats.server.clone().address()),
                seats.trusted.clone(),
                seats.name.clone(),
                seats.time_server.clone(),
            )?;
            let party = Party::join(settings, code).map_err(Error::Party)?;

            Ok((party, player, seats))
        },
        |(party, player, seats)| opened(party, player, zone, &seats),
    )
}

pub(crate) fn accept(text: &str) -> bool {
    let Some(invitation) = invitation_in(text).filter(|one| one.to == Invited::Party) else {
        return false;
    };

    thread::spawn(move || {
        let deadline = Instant::now() + THE_WINDOW_COMES_WITHIN;
        loop {
            let zone = SEATS
                .lock()
                .as_ref()
                .and_then(|seats| seats.zones.listed().first().map(|zone| zone.id));

            if let Some(zone) = zone {
                if let Err(error) = join(&invitation.code, invitation.server.clone(), zone) {
                    tracing::warn!(%error, "a party could not be joined");
                }
                return;
            }
            if Instant::now() >= deadline {
                tracing::warn!("a party's link came while no window was open to join it from");
                return;
            }
            thread::sleep(LOOK_AT_THE_PARTY_EVERY);
        }
    });

    true
}

pub(crate) fn named(name: &str) {
    if let Some(seats) = SEATS.lock().as_mut() {
        name.clone_into(&mut seats.name);
    }
    if let Some(room) = ROOM.lock().as_ref() {
        room.going.party().named(name.to_owned());
    }
}

pub(crate) fn shown() -> Option<(PartyShown, PartyFollowed, ZoneId)> {
    ROOM.lock()
        .as_ref()
        .map(|room| (room.going.shown(), room.going.followed(), room.zone))
}

pub(crate) fn leave() {
    let _changing = CHANGING.lock();
    let left = ROOM.lock().take();

    drop(left);
}

pub(crate) fn say(chat: PartyChat) {
    if let Some(room) = ROOM.lock().as_ref() {
        room.going.party().say(chat);
    }
}

pub(crate) fn hand_over(member: [u8; 16]) {
    if let Some(room) = ROOM.lock().as_ref() {
        room.going.party().hand_over(DeviceId(member));
    }
}

pub(crate) fn turn_out(member: [u8; 16]) {
    if let Some(room) = ROOM.lock().as_ref() {
        room.going.party().turn_out(DeviceId(member));
    }
}

fn request_named(
    asked_for: &[PartyWantingASong],
    request: PartyRequestId,
) -> Option<PartyWantingASong> {
    let request = RequestId::from_bytes(&request.0)?;

    asked_for
        .iter()
        .find(|asked| asked.id() == request)
        .cloned()
}

pub(crate) fn dismiss_a_request(request: PartyRequestId) {
    if let Some(room) = ROOM.lock().as_ref()
        && let Some(request) = request_named(&room.going.shown().asked_for, request)
    {
        room.going.party().take_a_request(request);
    }
}

pub(crate) fn play_a_request_next(request: PartyRequestId) -> bool {
    let Some((party, player, finder, wanted)) = ROOM.lock().as_ref().and_then(|room| {
        let wanted = request_named(&room.going.shown().asked_for, request)?;

        Some((
            Arc::clone(room.going.party()),
            Arc::clone(&room.player),
            Arc::clone(&room.finder),
            wanted,
        ))
    }) else {
        return false;
    };

    let Some(_claim) = party.claim_a_request(&wanted) else {
        return true;
    };
    let Some(row) = finder.found(&wanted.song) else {
        return false;
    };
    let queued = queued_next(&player, row);

    taken_once(queued, || party.take_a_request(wanted))
}

fn queued_next(player: &Player, row: Resumable) -> bool {
    let queued = player.queued();
    let id = resonate_engine::unclaimed_id(queued.rows.iter().map(AsRef::as_ref));

    player
        .send(Command::Insert {
            items: vec![QueueItem {
                id,
                location: row.location,
                span: row.span,
            }],
            at: Placement::Next,
            play: false,
        })
        .is_ok()
}

fn taken_once(queued: bool, take: impl FnOnce()) -> bool {
    if queued {
        take();
    }

    queued
}

pub(crate) fn following(zone: ZoneId) -> bool {
    ROOM.lock()
        .as_ref()
        .is_some_and(|room| room.zone == zone && !room.going.shown().hosting)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crossbeam_channel::{Receiver, unbounded};
    use resonate_core::Recording;
    use resonate_sync::party::PartySong;

    use super::*;

    const A_PARTY: &str = "resonate://party/ABCDE";

    fn heard(told: &Receiver<Followed>) -> Vec<Followed> {
        told.try_iter().collect()
    }

    #[test]
    fn links_that_came_before_the_window_are_told_to_it_once_it_opens_in_the_order_they_came() {
        let (told, listening) = unbounded();
        let mut offering = Offering::Waiting(Vec::new());

        offering.offer("resonate://party/AAAAA".to_owned());
        offering.offer("resonate://party/BBBBB".to_owned());

        assert!(heard(&listening).is_empty());

        offering.opened_to(told);

        assert_eq!(
            heard(&listening),
            vec![
                Followed::PartyOffered("resonate://party/AAAAA".to_owned()),
                Followed::PartyOffered("resonate://party/BBBBB".to_owned()),
            ]
        );
    }

    #[test]
    fn a_link_that_comes_while_the_window_is_open_is_told_at_once() {
        let (told, listening) = unbounded();
        let mut offering = Offering::Waiting(Vec::new());

        offering.opened_to(told);

        offering.offer(A_PARTY.to_owned());

        assert_eq!(
            heard(&listening),
            vec![Followed::PartyOffered(A_PARTY.to_owned())]
        );
    }

    #[test]
    fn a_window_that_closed_holds_the_next_links_for_the_next_window() {
        let (told, _closed) = unbounded();
        let (next, listening) = unbounded();
        let mut offering = Offering::Waiting(Vec::new());

        offering.opened_to(told);

        offering.closed();
        offering.offer(A_PARTY.to_owned());
        offering.opened_to(next);

        assert_eq!(
            heard(&listening),
            vec![Followed::PartyOffered(A_PARTY.to_owned())]
        );
    }

    #[test]
    fn only_a_link_that_asks_first_is_offered() {
        assert!(!offer("resonate://pair/ABCDE"));
        assert!(!offer("a song.flac"));
        assert!(offer("resonate://shared/E-ABCDE"));
        assert!(offer(A_PARTY));
        assert!(offer("https://resonate.tecnio.dev/p/#ABCDE"));
        assert!(offer(
            "resonate://shared/E-ABCDE?server=wss%3A%2F%2Fstranger.example%2F"
        ));
    }

    #[test]
    fn a_shared_list_naming_a_server_is_told_to_the_window_with_the_server_it_names() {
        let (told, listening) = unbounded();
        let mut offering = Offering::Waiting(Vec::new());

        offering.opened_to(told);

        let link = "resonate://shared/E-ABCDE?server=wss%3A%2F%2Fstranger.example%2F";

        offering.offer(link.to_owned());

        assert_eq!(
            heard(&listening),
            vec![Followed::ListOffered(ListOffered {
                link: link.to_owned(),
                server: "wss://stranger.example/".to_owned(),
                own_server: false,
            })]
        );
    }

    #[test]
    fn a_shared_list_from_the_bus_is_held_for_an_offer_whatever_server_it_names() {
        use crate::together::{Arrival, accepted};

        let own = "resonate://shared/E-ABCDE";
        let hosted = "https://resonate.tecnio.dev/s/#E-FGHIJ";
        let stranger = "resonate://shared/E-KLMNO?server=wss%3A%2F%2Fstranger.example%2F";

        for link in [own, hosted, stranger] {
            assert!(accepted(link, Arrival::FromTheBus));
        }

        let Offering::Waiting(links) = &*OFFERING.lock() else {
            panic!("no window was opened, so the offers wait for one");
        };

        assert_eq!(
            links,
            &[own.to_owned(), hosted.to_owned(), stranger.to_owned()]
        );
    }

    #[test]
    fn a_shared_list_naming_no_server_is_told_with_the_one_in_use_as_its_own() {
        let (told, listening) = unbounded();
        let mut offering = Offering::Waiting(Vec::new());

        offering.opened_to(told);

        let link = "resonate://shared/E-ABCDE";

        offering.offer(link.to_owned());

        assert_eq!(
            heard(&listening),
            vec![Followed::ListOffered(ListOffered {
                link: link.to_owned(),
                server: crate::together::server_in_use(),
                own_server: true,
            })]
        );
    }

    fn asked_by(guest: u8, title: &str) -> PartyWantingASong {
        PartyWantingASong {
            from: DeviceId([guest; 16]),
            song: PartySong {
                row: 1,
                recording: Recording {
                    mbid: None,
                    title: title.to_owned(),
                    artist: "Artist".to_owned(),
                    album: "Album".to_owned(),
                    length: None,
                },
                isrc: None,
            },
        }
    }

    fn drawn(asked: &PartyWantingASong) -> PartyRequestId {
        PartyRequestId(asked.id().to_bytes())
    }

    #[test]
    fn a_request_pressed_is_the_one_drawn_whatever_has_come_and_gone_before_it_is_settled() {
        let first = asked_by(1, "Slow Water");
        let second = asked_by(2, "Low Tide");
        let third = asked_by(3, "Time");
        let pressed = drawn(&second);
        let gone = drawn(&first);

        let after_the_first_was_taken = vec![second.clone(), third.clone()];
        let after_another_came_first = vec![third, first, second.clone()];

        assert_eq!(
            request_named(&after_the_first_was_taken, pressed),
            Some(second.clone())
        );
        assert_eq!(
            request_named(&after_another_came_first, pressed),
            Some(second)
        );
        assert_eq!(request_named(&after_the_first_was_taken, gone), None);
    }

    #[test]
    fn a_request_named_by_bytes_that_name_no_request_settles_nothing() {
        let shown = vec![asked_by(1, "Slow Water")];

        assert_eq!(request_named(&shown, PartyRequestId([0; 32])), None);
    }

    #[test]
    fn beginning_and_joining_at_once_leave_one_room_and_never_two_at_a_time() {
        static LIVE: AtomicUsize = AtomicUsize::new(0);
        static MOST: AtomicUsize = AtomicUsize::new(0);

        struct Held;

        impl Held {
            fn made() -> Self {
                let now = LIVE.fetch_add(1, Ordering::SeqCst) + 1;

                MOST.fetch_max(now, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(3));

                Self
            }
        }

        impl Drop for Held {
            fn drop(&mut self) {
                LIVE.fetch_sub(1, Ordering::SeqCst);
            }
        }

        let slot = Mutex::new(None);
        let changing = Mutex::new(());

        thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    replaced(&slot, &changing, || Ok(()), |()| Ok(Held::made()))
                        .expect("the room changed");
                });
            }
        });

        assert_eq!(MOST.load(Ordering::SeqCst), 1);
        assert_eq!(LIVE.load(Ordering::SeqCst), 1);
        assert!(slot.lock().is_some());
    }

    #[test]
    fn a_room_that_cannot_be_made_leaves_the_one_there_is() {
        let slot = Mutex::new(Some(7_u8));
        let changing = Mutex::new(());

        let prepared = replaced(
            &slot,
            &changing,
            || Err::<(), _>(Error::PartyNeedsAServer),
            |()| Ok(8_u8),
        );

        assert!(matches!(prepared, Err(Error::PartyNeedsAServer)));
        assert_eq!(*slot.lock(), Some(7));
    }

    #[test]
    fn a_request_is_taken_only_once_it_was_queued() {
        let taken = std::cell::Cell::new(0);

        let failed = taken_once(false, || taken.set(taken.get() + 1));

        assert!(!failed);
        assert_eq!(taken.get(), 0, "a request that could not be queued waits");

        let queued = taken_once(true, || taken.set(taken.get() + 1));

        assert!(queued);
        assert_eq!(taken.get(), 1);
    }
}
