use std::{sync::Arc, thread, time::Duration};

use crossbeam_channel::bounded;
use resonate_core::{FrameSpan, MediaLocation, Recording, Resumable, Span};
use resonate_engine::{Command, Moment, Player, Seat, SeatHeard as ZoneHeard, Sounding};
use resonate_library::Library;
use resonate_providers::{PlaysInPlace, found_exactly};
use resonate_sync::{
    party::{
        Going, Mine, Party, PartyFinder, PartyReach, PartySeat, PartySettings, PartySong,
        SeatHeard, Standing,
    },
    shared::invitation_in,
};

use crate::{
    cli::PartyWanted,
    config::Config,
    error::{Error, Result},
    sync::server_of,
    words,
};

const SONGS_TOLD_AHEAD: usize = 8;

pub(crate) const LOOK_AT_THE_PARTY_EVERY: Duration = Duration::from_millis(500);

pub(crate) struct ZoneSeat {
    pub(crate) seat: Seat,
    pub(crate) library: Option<Arc<Library>>,
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
            position_nanos: heard.position.map(Seat::nanos_of),
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

    fn keep_ahead(&self, rows: &[Resumable]) -> bool {
        self.seat
            .keep_ahead(rows)
            .inspect_err(|error| tracing::debug!(%error, "a party's queue ahead was not kept"))
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

pub(crate) struct Finder {
    pub(crate) library: Option<Arc<Library>>,
    pub(crate) services: Vec<Arc<dyn PlaysInPlace>>,
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

pub(crate) fn settings_for(
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

fn said_of(standing: Standing) -> &'static str {
    use resonate_sync::party::SittingOut;

    match standing {
        Standing::WaitingForTheHost => words::party::waiting_for_the_host(),
        Standing::Finding => words::party::finding(),
        Standing::SittingOut(SittingOut::NotHeld) => words::party::not_held(),
        Standing::SittingOut(SittingOut::CannotStartMidSong) => {
            words::party::cannot_start_mid_song()
        }
        Standing::SittingOut(SittingOut::AskedTo) => words::party::asked_to(),
        Standing::HostPaused => words::party::host_paused(),
        Standing::InStep => words::party::in_step(),
    }
}

fn reach_said(reach: PartyReach) -> Option<&'static str> {
    match reach {
        PartyReach::Here => None,
        PartyReach::Connecting => Some(words::party::connecting()),
        PartyReach::Reconnecting => Some(words::party::reconnecting()),
        PartyReach::Ended => Some(words::party::ended()),
        PartyReach::NoSuchParty => Some(words::party::no_such_party()),
        PartyReach::TurnedAway => Some(words::party::turned_away()),
        PartyReach::ServerTooOld => Some(words::party::server_too_old()),
    }
}

pub(crate) fn run(cli: &crate::cli::Cli, config: &Config, wanted: &PartyWanted) -> Result<()> {
    if !config.syncs() {
        return Err(Error::SyncOff);
    }

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
        said!("{}", words::party::invite(shown.page.unwrap_or(shown.link)));
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
                    words::party::here(shown.people.len())
                } else {
                    said_of(going.followed().standing).to_owned()
                }
            },
            str::to_owned,
        );
        if now != said {
            said!("{now}");
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
