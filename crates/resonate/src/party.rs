use std::{
    sync::{Arc, LazyLock},
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::bounded;
use parking_lot::Mutex;
use resonate_core::{FrameSpan, MediaLocation, Recording, Resumable, Span};
use resonate_engine::{
    Command, Moment, Placement, Player, QueueItem, Seat, SeatHeard as ZoneHeard, Sounding, ZoneId,
    Zones,
};
use resonate_library::Library;
use resonate_providers::{PlaysInPlace, found_exactly};
use resonate_sync::{
    Server,
    party::{
        Going, Mine, Party, PartyFinder, PartyFollowed, PartyReach, PartySeat, PartySettings,
        PartyShown, PartySong, SeatHeard, Standing,
    },
    shared::{Invited, invitation_in},
};

use crate::{
    cli::PartyWanted,
    config::Config,
    error::{Error, Result},
    sync::server_of,
};

const SONGS_TOLD_AHEAD: usize = 8;

const LOOK_AT_THE_PARTY_EVERY: Duration = Duration::from_millis(500);

const THE_WINDOW_COMES_WITHIN: Duration = Duration::from_secs(30);

struct ZoneSeat {
    seat: Seat,
    library: Option<Arc<Library>>,
}

fn nanos(position: Duration) -> u64 {
    u64::try_from(position.as_nanos()).unwrap_or(u64::MAX)
}

fn song_of(
    player: &Player,
    library: Option<&Library>,
    location: &MediaLocation,
    span: Option<FrameSpan>,
) -> Option<PartySong> {
    let row = resonate_core::QueueStamp::of([(location, span)]).value();
    let media = player.media(location, span);
    let isrc = media.as_ref().and_then(|info| info.tags.isrc.clone());

    if let Some(library) = library
        && let Ok(Some(recording)) = library.recording_at(location, span)
    {
        return Some(PartySong {
            row,
            recording,
            isrc,
        });
    }

    let info = media?;

    Some(PartySong {
        row,
        recording: Recording {
            mbid: None,
            title: info.tags.title.clone()?,
            artist: info.tags.artist.clone().unwrap_or_default(),
            album: info.tags.album.clone().unwrap_or_default(),
            length: info
                .duration
                .map(|frames| frames.to_duration(info.spec.rate)),
        },
        isrc,
    })
}

fn mine_of(heard: &ZoneHeard) -> Mine {
    match heard.sounding {
        Sounding::Playing => Mine::Playing,
        Sounding::Paused => Mine::Paused,
        Sounding::Stopped => Mine::Stopped,
    }
}

impl PartySeat for ZoneSeat {
    type Row = Resumable;

    fn heard(&self) -> SeatHeard {
        let heard = self.seat.heard();

        SeatHeard {
            mine: mine_of(&heard),
            row: heard.row.map(resonate_core::QueueStamp::value),
            position_nanos: heard.position.map(nanos),
            at_nanos: heard.at.nanos(),
            coarse: heard.holds_seconds_downstream,
        }
    }

    fn stamp_of(&self, row: &Resumable) -> u64 {
        Seat::stamp_of(row).value()
    }

    fn songs_up_next(&self) -> (Option<PartySong>, Vec<PartySong>) {
        let player = self.seat.player();
        let state = player.state();
        let queued = player.queued();
        let Some(at) = state.queue_position else {
            return (None, Vec::new());
        };
        let library = self.library.as_deref();
        let song = queued
            .rows
            .get(at)
            .and_then(|item| song_of(player, library, &item.location, item.span));
        let next = (at + 1..at + 1 + SONGS_TOLD_AHEAD)
            .filter_map(|row| queued.rows.get(row))
            .filter_map(|item| song_of(player, library, &item.location, item.span))
            .collect();

        (song, next)
    }

    fn added_here(&self, known: &[u64]) -> Vec<PartySong> {
        let player = self.seat.player();
        let queued = player.queued();
        let library = self.library.as_deref();
        let mut added = Vec::new();

        for (row, item) in queued.rows.iter().enumerate().rev() {
            let stamp = resonate_core::QueueStamp::of([(&item.location, item.span)]).value();
            if known.contains(&stamp) {
                continue;
            }
            if let Some(song) = song_of(player, library, &item.location, item.span) {
                added.push(song);
            }
            if let Err(error) = player.send(Command::Remove(Span::one(row))) {
                tracing::warn!(%error, "a song added during a party was left in the queue");
            }
        }

        added
    }

    fn take_up(
        &self,
        rows: &[Resumable],
        position_nanos: u64,
        at_nanos: u64,
        sounding: bool,
    ) -> bool {
        self.seat
            .take_up(
                rows,
                Duration::from_nanos(position_nanos),
                Moment::from_nanos(at_nanos),
                sounding,
            )
            .inspect_err(|error| tracing::debug!(%error, "a party's song did not start here"))
            .unwrap_or(false)
    }

    fn nudge(&self, position_nanos: u64, at_nanos: u64) {
        let nudged = self.seat.nudge(
            Duration::from_nanos(position_nanos),
            Moment::from_nanos(at_nanos),
        );
        if let Err(error) = nudged {
            tracing::debug!(%error, "a party could not move back into step");
        }
    }

    fn seek(&self, to_nanos: u64) {
        if let Err(error) = self.seat.seek(Duration::from_nanos(to_nanos)) {
            tracing::debug!(%error, "a party could not seek to its host");
        }
    }

    fn pause(&self) {
        if let Err(error) = self.seat.pause() {
            tracing::debug!(%error, "a party could not pause");
        }
    }

    fn play(&self) {
        if let Err(error) = self.seat.play() {
            tracing::debug!(%error, "a party could not play");
        }
    }

    fn put_back(&self) {
        if let Err(error) = self.seat.put_back() {
            tracing::warn!(%error, "the queue from before a party was not put back");
        }
    }
}

struct Finder {
    library: Option<Arc<Library>>,
    services: Vec<Arc<dyn PlaysInPlace>>,
}

impl Finder {
    fn held_here(&self, song: &PartySong) -> Option<Resumable> {
        let library = self.library.as_deref()?;
        let anywhere = Recording {
            album: String::new(),
            ..song.recording.clone()
        };
        let by_the_tags = |recording: &Recording| {
            library
                .resolve_recording(recording)
                .inspect_err(|error| tracing::debug!(%error, "a party's song was not looked up"))
                .ok()
                .flatten()
        };
        let held = by_the_tags(&song.recording)
            .or_else(|| by_the_tags(&anywhere))
            .or_else(|| {
                song.isrc
                    .as_deref()
                    .and_then(|isrc| library.cut_with_isrc(isrc).ok().flatten())
            })?;

        Some(Resumable {
            location: held.location,
            span: held.span,
            held: None,
        })
    }
}

impl PartyFinder for Finder {
    type Row = Resumable;

    fn found(&self, song: &PartySong) -> Option<Resumable> {
        self.held_here(song).or_else(|| {
            found_exactly(&self.services, &song.recording).map(|location| Resumable {
                location,
                span: None,
                held: None,
            })
        })
    }
}

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
    pub(crate) services: Vec<Arc<dyn PlaysInPlace>>,
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

fn settings_for(
    server: Option<String>,
    trusted: Option<std::path::PathBuf>,
    name: String,
) -> Result<PartySettings> {
    let server = server.ok_or(Error::PartyNeedsAServer)?;

    Ok(PartySettings {
        server,
        trusted,
        name,
        now_nanos: Arc::new(|| Moment::now().nanos()),
    })
}

fn opened(party: Party, player: Arc<Player>, zone: ZoneId, seats: &Seats) -> Result<Room> {
    let finder = Arc::new(Finder {
        library: Some(Arc::clone(&seats.library)),
        services: seats.services.clone(),
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
        room.going.party().turn_out(resonate_sync::DeviceId(member));
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

fn said_of(standing: Standing) -> &'static str {
    use resonate_sync::party::SittingOut;

    match standing {
        Standing::WaitingForTheHost => "Waiting for the host to play something",
        Standing::Finding => "Looking for this song",
        Standing::SittingOut(SittingOut::NotHeld) => "You don't have this song, so you sit it out",
        Standing::SittingOut(SittingOut::CannotStartMidSong) => {
            "This song can't start part way through here, so you join at the next"
        }
        Standing::SittingOut(SittingOut::AskedTo) => "Paused here; press play to rejoin",
        Standing::HostPaused => "The host has paused",
        Standing::InStep => "In step with the host",
    }
}

fn reach_said(reach: PartyReach) -> Option<&'static str> {
    match reach {
        PartyReach::Here => None,
        PartyReach::Connecting => Some("Reaching the server…"),
        PartyReach::Reconnecting => Some("Lost the server; trying again…"),
        PartyReach::Ended => Some("The party is over"),
        PartyReach::NoSuchParty => Some("That party has ended, or was never begun"),
        PartyReach::TurnedAway => Some("The server turned that link away"),
        PartyReach::ServerTooOld => Some("The server is too old to hold parties"),
    }
}

pub(crate) fn run(cli: &crate::cli::Cli, config: &Config, wanted: &PartyWanted) -> Result<()> {
    let library = crate::catalog(cli, config);
    let served = crate::Served::of(config, crate::settings_path(cli).ok());
    let sources = crate::playing_from(library.as_deref(), &served);
    let sink = crate::wanted_sink(cli, config);
    let player = Arc::new(
        crate::Rendering::of(config, sink.as_ref(), crate::Asked::Nothing)
            .player(crate::engine_config(cli, config), Arc::clone(&sources))?,
    );
    crate::confirm_sink(&player, sink.as_ref());

    let name = config.shared_name.clone().unwrap_or_default();
    let trusted = config.sync_server_certificate.clone();
    let party = match wanted {
        PartyWanted::Start { files } => {
            let items = crate::queue_items_served(files, &served);
            if items.is_empty() {
                return Err(Error::NoFileNamed);
            }
            player.send(Command::Load {
                items,
                start_at: 0,
                autoplay: true,
            })?;

            Party::host(settings_for(server_of(config).address(), trusted, name)?)
                .map_err(Error::Party)?
        }
        PartyWanted::Join { link } => {
            let invitation = invitation_in(link);
            let code = invitation
                .as_ref()
                .map_or(link.as_str(), |one| one.code.as_str());
            let server = invitation
                .as_ref()
                .and_then(|one| one.server.clone())
                .or_else(|| server_of(config).address());

            Party::join(settings_for(server, trusted, name)?, code).map_err(Error::Party)?
        }
    };
    let finder = Arc::new(Finder {
        library: library.clone(),
        services: served.services(),
    });
    let seat = ZoneSeat {
        seat: Seat::on(Arc::clone(&player)),
        library,
    };
    let mut going = Going::begin(party, seat, finder).map_err(Error::Party)?;

    let (asked_to_quit, quitting) = bounded(1);
    let silenced = Arc::clone(&player);
    crate::signals::quit_when_told(
        asked_to_quit,
        crate::signals::silencing(move || vec![Arc::clone(&silenced)]),
    );

    let shown = going.shown();
    if shown.hosting {
        println!("Invite people with {}", shown.page.unwrap_or(shown.link));
    }

    let mut said = String::new();
    loop {
        if quitting.recv_timeout(LOOK_AT_THE_PARTY_EVERY).is_ok() {
            break;
        }

        let shown = going.shown();
        let now = reach_said(shown.reach).map_or_else(
            || {
                if shown.hosting {
                    format!("{} here", shown.people.len())
                } else {
                    said_of(going.followed().standing).to_owned()
                }
            },
            str::to_owned,
        );
        if now != said {
            println!("{now}");
            said = now;
        }
        if matches!(
            shown.reach,
            PartyReach::Ended
                | PartyReach::NoSuchParty
                | PartyReach::TurnedAway
                | PartyReach::ServerTooOld
        ) {
            break;
        }
    }

    going.leave();
    thread::sleep(Duration::from_millis(50));

    Ok(())
}
