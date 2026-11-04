use std::{
    sync::{Arc, LazyLock},
    thread,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use resonate_engine::{Command, Placement, Player, QueueItem, Seat, ZoneId, Zones};
use resonate_library::Library;
use resonate_sync::{
    DeviceId, Server,
    party::{Going, Party, PartyFinder as _, PartyFollowed, PartyShown},
    shared::{Invited, invitation_in},
};

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
}

static SEATS: LazyLock<Mutex<Option<Seats>>> = LazyLock::new(|| Mutex::new(None));

static ROOM: LazyLock<Mutex<Option<Room>>> = LazyLock::new(|| Mutex::new(None));

pub(crate) fn in_the_window(seats: Seats) {
    *SEATS.lock() = Some(seats);
}

pub(crate) fn out_of_the_window() {
    *ROOM.lock() = None;
    *SEATS.lock() = None;
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
    *ROOM.lock() = None;
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
    let guard = ROOM.lock();
    let Some(room) = guard.as_ref() else {
        return false;
    };
    let Some(wanted) = room.going.shown().asked_for.get(at).cloned() else {
        return false;
    };
    let Some(row) = room.finder.found(&wanted.song) else {
        return false;
    };
    let queued = room.player.queued();
    let id = resonate_engine::unclaimed_id(queued.rows.iter().map(AsRef::as_ref));
    let placed = room.player.send(Command::Insert {
        items: vec![QueueItem {
            id,
            location: row.location,
            span: row.span,
        }],
        at: Placement::Next,
        play: false,
    });
    room.going.party().take_a_request(at);

    placed.is_ok()
}

pub(crate) fn following(zone: ZoneId) -> bool {
    ROOM.lock()
        .as_ref()
        .is_some_and(|room| room.zone == zone && !room.going.shown().hosting)
}
