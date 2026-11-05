use std::{
    mem,
    sync::{Arc, LazyLock},
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::Sender;
use parking_lot::Mutex;
use resonate_engine::{Command, Placement, Player, QueueItem, Seat, ZoneId, Zones};
use resonate_library::Library;
use resonate_sync::{
    DeviceId, Server,
    party::{Going, Party, PartyFinder as _, PartyFollowed, PartyShown},
    shared::{Invited, invitation_in},
};
use resonate_ui::Followed;

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

static OFFERING: Mutex<Offering> = Mutex::new(Offering::Waiting(Vec::new()));

fn tell_of(link: String, told: &Sender<Followed>) {
    if told.send(Followed::PartyOffered(link)).is_err() {
        tracing::warn!("a party's link came while no window was left to ask about it");
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

pub(crate) fn offer(text: &str) -> bool {
    if !invitation_in(text).is_some_and(|one| one.to == Invited::Party) {
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

pub(crate) fn begin(zone: ZoneId) -> Result<()> {
    let seats = SEATS.lock().clone().ok_or(Error::PartyNeedsAServer)?;
    let player = seats
        .zones
        .zone(zone)
        .map(|zone| zone.player)
        .ok_or(Error::PartyNeedsAServer)?;
    let settings = settings_for(
        seats.server.clone().address(),
        seats.trusted.clone(),
        seats.name.clone(),
    )?;
    let party = Party::host(settings).map_err(Error::Party)?;
    leave();
    let room = opened(party, player, zone, &seats)?;

    *ROOM.lock() = Some(room);

    Ok(())
}

pub(crate) fn join(code: &str, server: Option<String>, zone: ZoneId) -> Result<()> {
    let seats = SEATS.lock().clone().ok_or(Error::PartyNeedsAServer)?;
    let player = seats
        .zones
        .zone(zone)
        .map(|zone| zone.player)
        .ok_or(Error::PartyNeedsAServer)?;
    let settings = settings_for(
        server.or_else(|| seats.server.clone().address()),
        seats.trusted.clone(),
        seats.name.clone(),
    )?;
    let party = Party::join(settings, code).map_err(Error::Party)?;
    leave();
    let room = opened(party, player, zone, &seats)?;

    *ROOM.lock() = Some(room);

    Ok(())
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
    let left = ROOM.lock().take();

    drop(left);
}

pub(crate) fn turn_out(member: [u8; 16]) {
    if let Some(room) = ROOM.lock().as_ref() {
        room.going.party().turn_out(DeviceId(member));
    }
}

pub(crate) fn dismiss_a_request(at: usize) {
    if let Some(room) = ROOM.lock().as_ref() {
        room.going.party().take_a_request(at);
    }
}

pub(crate) fn play_a_request_next(at: usize) -> bool {
    let Some((party, player, finder, wanted)) = ROOM.lock().as_ref().and_then(|room| {
        let wanted = room.going.shown().asked_for.get(at).cloned()?;

        Some((
            Arc::clone(room.going.party()),
            Arc::clone(&room.player),
            Arc::clone(&room.finder),
            wanted,
        ))
    }) else {
        return false;
    };
    let Some(row) = finder.found(&wanted.song) else {
        return false;
    };
    let queued = player.queued();
    let id = resonate_engine::unclaimed_id(queued.rows.iter().map(AsRef::as_ref));
    let placed = player.send(Command::Insert {
        items: vec![QueueItem {
            id,
            location: row.location,
            span: row.span,
        }],
        at: Placement::Next,
        play: false,
    });
    party.take_a_request(at);

    placed.is_ok()
}

pub(crate) fn following(zone: ZoneId) -> bool {
    ROOM.lock()
        .as_ref()
        .is_some_and(|room| room.zone == zone && !room.going.shown().hosting)
}

#[cfg(test)]
mod tests {
    use crossbeam_channel::{Receiver, unbounded};

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
    fn only_a_party_link_is_offered() {
        assert!(!offer("resonate://shared/E-ABCDE"));
        assert!(!offer("resonate://pair/ABCDE"));
        assert!(!offer("a song.flac"));
        assert!(offer(A_PARTY));
        assert!(offer("https://resonate.tecnio.dev/p/#ABCDE"));
    }
}
