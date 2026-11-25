use std::{sync::Arc, thread, time::Duration};

use crossbeam_channel::bounded;
use resonate_core::{FrameSpan, MediaLocation, QueueStamp, Recording, Resumable, Span, TrackId};
use resonate_engine::{Command, Moment, Player, Seat, SeatHeard as ZoneHeard, Sounding, TakenUp};
use resonate_library::Library;
use resonate_providers::{PlaysInPlace, found_exactly};
use resonate_sync::{
    party::{
        Going, Mine, Party, PartyApart, PartyChat, PartyFinder, PartyLine, PartyReach,
        PartyReaction, PartySeat, PartySettings, PartySong, SeatHeard, SeatTakenUp, Standing,
        known_where_they_moved,
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
    let row = QueueStamp::of([(location, span)]).value();
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

fn stamps_of(moved: &[(QueueStamp, QueueStamp)]) -> Vec<(u64, u64)> {
    moved
        .iter()
        .map(|(from, to)| (from.value(), to.value()))
        .collect()
}

const fn as_the_party_reads(taken: TakenUp) -> SeatTakenUp {
    match taken {
        TakenUp::InStep => SeatTakenUp::InStep,
        TakenUp::Queued => SeatTakenUp::Queued,
        TakenUp::Untouched => SeatTakenUp::Untouched,
    }
}

fn remove_by_identity(player: &Player, id: TrackId) -> resonate_engine::Result<()> {
    let row = player.queued().rows.iter().position(|item| item.id == id);

    match row {
        Some(row) => player.send(Command::Remove(Span::one(row))),
        None => Ok(()),
    }
}

impl PartySeat for ZoneSeat {
    type Row = Resumable;

    fn heard(&self) -> SeatHeard {
        let heard = self.seat.heard();

        SeatHeard {
            mine: mine_of(&heard),
            row: heard.row.map(QueueStamp::value),
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

    fn moved(&self) -> Vec<(u64, u64)> {
        stamps_of(&self.seat.take_the_moves())
    }

    fn added_here(&self, known: &[u64]) -> Vec<PartySong> {
        let player = self.seat.player();
        let queued = player.queued();
        let known = known_where_they_moved(known, &stamps_of(&self.seat.moves_seen()));
        let library = self.library.as_deref();
        let mut added = Vec::new();

        for item in queued.rows.iter().rev() {
            let stamp = QueueStamp::of([(&item.location, item.span)]).value();

            if known.contains(&stamp) {
                continue;
            }

            let Some(song) = song_of(player, library, &item.location, item.span) else {
                continue;
            };

            if let Err(error) = remove_by_identity(player, item.id) {
                tracing::warn!(%error, "a song added during a party was left in the queue");
            }
            added.push(song);
        }

        added
    }

    fn take_up(
        &self,
        rows: &[Resumable],
        position_nanos: u64,
        at_nanos: u64,
        sounding: bool,
    ) -> SeatTakenUp {
        self.seat
            .take_up(
                rows,
                Duration::from_nanos(position_nanos),
                Moment::from_nanos(at_nanos),
                sounding,
            )
            .map_or_else(
                |error| {
                    tracing::debug!(%error, "a party's song did not start here");
                    SeatTakenUp::Untouched
                },
                as_the_party_reads,
            )
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
    time_server: Option<String>,
) -> Result<PartySettings> {
    let server = server.ok_or(Error::PartyNeedsAServer)?;

    Ok(PartySettings {
        server,
        trusted,
        name,
        now_nanos: Arc::new(|| Moment::now().nanos()),
        time_server,
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

const APART_SAID_AGAIN_PAST_NANOS: u64 = 5_000_000;

fn moved_enough_to_say(before: PartyApart, now: PartyApart) -> bool {
    before.corrected != now.corrected
        || before.nanos.abs_diff(now.nanos) >= APART_SAID_AGAIN_PAST_NANOS
}

fn apart_of(apart: PartyApart) -> String {
    let millis = format!("{:+.1}", apart.nanos as f64 / 1_000_000.0);
    if apart.corrected {
        words::party::apart_corrected(millis)
    } else {
        words::party::apart(millis)
    }
}

fn reach_said(reach: PartyReach) -> Option<&'static str> {
    match reach {
        PartyReach::Here => None,
        PartyReach::Connecting => Some(words::party::connecting()),
        PartyReach::Reconnecting => Some(words::party::reconnecting()),
        PartyReach::Ended => Some(words::party::ended()),
        PartyReach::HandedOver => Some(words::party::handed_over()),
        PartyReach::NoSuchParty => Some(words::party::no_such_party()),
        PartyReach::TurnedAway => Some(words::party::turned_away()),
        PartyReach::ServerTooOld => Some(words::party::server_too_old()),
    }
}

const fn reaction_drawn(reaction: PartyReaction) -> &'static str {
    match reaction {
        PartyReaction::Love => "\u{2764}\u{fe0f}",
        PartyReaction::Fire => "\u{1f525}",
        PartyReaction::Laugh => "\u{1f602}",
        PartyReaction::Clap => "\u{1f44f}",
        PartyReaction::Dance => "\u{1f483}",
        PartyReaction::Wow => "\u{1f62e}",
    }
}

fn line_said(line: &PartyLine, mine: bool) -> String {
    let person = if mine {
        words::party::you().to_owned()
    } else if line.name.is_empty() {
        words::party::someone().to_owned()
    } else {
        line.name.clone()
    };

    match &line.chat {
        PartyChat::Words(words) => words::party::said_by(person, words.clone()),
        PartyChat::Reaction(reaction) => words::party::said_by(person, reaction_drawn(*reaction)),
    }
}

fn settings_wanted(config: &Config, wanted: &PartyWanted) -> Result<PartySettings> {
    let server = match wanted {
        PartyWanted::Start { .. } => server_of(config).address(),
        PartyWanted::Join { link } => invitation_in(link)
            .and_then(|invitation| invitation.server)
            .or_else(|| server_of(config).address()),
    };

    settings_for(
        server,
        config.sync_server_certificate.clone(),
        config.shared_name.clone().unwrap_or_default(),
        config.party_time_server(),
    )
}

pub(crate) fn run(cli: &crate::cli::Cli, config: &Config, wanted: &PartyWanted) -> Result<()> {
    if !config.syncs() {
        return Err(Error::SyncOff);
    }

    let settings = settings_wanted(config, wanted)?;
    let library = crate::catalog(cli, config);
    let served = crate::Served::of(config, crate::settings_path(cli).ok());

    let sources = crate::playing_from(library.as_deref(), &served);
    let sink = crate::sinks::wanted_sink(cli, config);

    let player = Arc::new(
        crate::rendering::Rendering::of(config, sink.as_ref(), crate::rendering::Asked::Nothing)
            .player(
                crate::engine_settings::engine_config(cli, config),
                Arc::clone(&sources),
            )?,
    );

    crate::sinks::confirm_sink(&player, sink.as_ref());

    let party = match wanted {
        PartyWanted::Start { files } => {
            let items = crate::arguments::queue_items_served(files, &served);

            if items.is_empty() {
                return Err(Error::NoFileNamed);
            }
            player.send(Command::Load {
                items,
                start_at: 0,
                autoplay: true,
            })?;

            Party::host(settings).map_err(Error::Party)?
        }
        PartyWanted::Join { link } => {
            let invitation = invitation_in(link);
            let code = invitation
                .as_ref()
                .map_or(link.as_str(), |one| one.code.as_str());

            Party::join(settings, code).map_err(Error::Party)?
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

    let typed = if crate::input::played_in_the_background() {
        None
    } else {
        said!("{}", words::party::type_to_chat());
        Some(crate::input::typed_lines())
    };

    let mut said = String::new();
    let mut apart_said: Option<PartyApart> = None;
    let mut lines_heard = Vec::new();

    loop {
        if quitting.recv_timeout(LOOK_AT_THE_PARTY_EVERY).is_ok() {
            break;
        }
        if let Some(typed) = &typed {
            for line in typed.try_iter() {
                if let Some(chat) = PartyChat::words(&line) {
                    going.party().say(chat);
                }
            }
        }

        let shown = going.shown();

        for line in &shown.chat {
            if !lines_heard.contains(&line.id) {
                said!("{}", line_said(line, line.from == shown.me));
                lines_heard.push(line.id);
            }
        }

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
        if let Some(apart) = shown.apart
            && apart_said.is_none_or(|before| moved_enough_to_say(before, apart))
        {
            said!("{}", apart_of(apart));
            apart_said = Some(apart);
        }
        if matches!(
            shown.reach,
            PartyReach::Ended
                | PartyReach::HandedOver
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crossbeam_channel::{Receiver, unbounded};
    use resonate_core::{Gain, Switch};
    use resonate_engine::{
        AudioSource, Backend, EngineConfig, Opener, Placement, QueueItem, RepeatMode, SinkChange,
        SinkError, SinkId, SinkInfo, SinkResult, SinkStream, StreamRequest, Surveyor,
    };

    use super::*;

    struct NoSinks {
        changes: Receiver<SinkChange>,
    }

    struct Nothing;

    impl Opener for Nothing {
        fn open(
            &self,
            _request: &StreamRequest,
            _source: Box<dyn AudioSource>,
        ) -> SinkResult<SinkStream> {
            Err(SinkError::NoSink)
        }
    }

    impl Surveyor for Nothing {
        fn enumerate_sinks(&self, _timeout: Duration) -> SinkResult<Vec<SinkInfo>> {
            Ok(Vec::new())
        }
    }

    impl Backend for NoSinks {
        fn subscribe_sinks(&self) -> Receiver<SinkChange> {
            self.changes.clone()
        }

        fn surveyor(&self) -> Arc<dyn Surveyor> {
            Arc::new(Nothing)
        }

        fn opener(&self) -> Arc<dyn Opener> {
            Arc::new(Nothing)
        }

        fn set_device_volume(&self, _sink: SinkId, _gain: Gain) -> SinkResult<()> {
            Err(SinkError::NoSink)
        }

        fn set_device_mute(&self, _sink: SinkId, _muted: bool) -> SinkResult<()> {
            Err(SinkError::NoSink)
        }

        fn shutdown(self: Box<Self>) -> SinkResult<()> {
            Ok(())
        }
    }

    fn a_zone() -> Arc<Player> {
        Arc::new(
            Player::with_backend(EngineConfig::default(), |_| {
                let (announce, changes) = unbounded();

                drop(announce);
                Ok(Box::new(NoSinks { changes }))
            })
            .expect("the engine starts behind no sink"),
        )
    }

    fn a_row(name: &str) -> Resumable {
        Resumable {
            location: MediaLocation::local(PathBuf::from("/music").join(name)),
            span: None,
            held: None,
        }
    }

    #[test]
    fn a_guest_whose_song_moves_keeps_it_where_it_went_and_asks_the_host_nothing() {
        let player = a_zone();
        let seat = ZoneSeat {
            seat: Seat::on(Arc::clone(&player)),
            library: None,
        };
        let rows = [a_row("first.flac"), a_row("second.flac")];
        let moved_to = a_row("filed/second.flac");

        seat.take_up(&rows, 0, Moment::now().nanos(), false);

        let known: Vec<u64> = rows.iter().map(|row| seat.stamp_of(row)).collect();

        player
            .request(Command::Relocate(vec![(
                rows[1].location.clone(),
                moved_to.location.clone(),
            )]))
            .expect("the move sent")
            .wait()
            .expect("the move followed");

        let asked_before_the_seat_looked = seat.added_here(&known);
        let moved = seat.moved();
        let known_now = [known[0], seat.stamp_of(&moved_to)];
        let asked = seat.added_here(&known_now);

        player
            .request(Command::SetRepeat(RepeatMode::Off))
            .expect("a command after any removal")
            .wait()
            .expect("every command before it settled");

        let queued: Vec<MediaLocation> = player
            .queued()
            .rows
            .iter()
            .map(|item| item.location.clone())
            .collect();

        assert!(asked_before_the_seat_looked.is_empty());
        assert_eq!(moved, vec![(known[1], known_now[1])]);
        assert!(asked.is_empty());
        assert_eq!(queued, [rows[0].location.clone(), moved_to.location]);
    }

    #[test]
    fn a_song_added_in_a_party_that_cannot_be_told_stays_in_the_queue() {
        let player = a_zone();
        let seat = ZoneSeat {
            seat: Seat::on(Arc::clone(&player)),
            library: None,
        };
        let row = a_row("untold.flac");
        let id = resonate_engine::unclaimed_id(std::iter::empty());

        player
            .request(Command::Insert {
                items: vec![QueueItem::whole(id, row.location.clone())],
                at: Placement::Next,
                play: false,
            })
            .expect("the song sent")
            .wait()
            .expect("the song queued");

        let asked = seat.added_here(&[]);

        player
            .request(Command::SetRepeat(RepeatMode::Off))
            .expect("a command after any removal")
            .wait()
            .expect("every command before it settled");

        let queued: Vec<TrackId> = player.queued().rows.iter().map(|item| item.id).collect();

        assert!(asked.is_empty());
        assert_eq!(queued, [id]);
    }

    #[test]
    fn a_song_is_removed_by_the_identity_it_was_queued_under_and_never_by_a_row_that_moved() {
        let player = a_zone();
        let first = resonate_engine::unclaimed_id(std::iter::empty());
        let second = TrackId::new(first.get() - 1).expect("another identity");

        player
            .request(Command::Insert {
                items: vec![
                    QueueItem::whole(first, a_row("one.flac").location),
                    QueueItem::whole(second, a_row("two.flac").location),
                ],
                at: Placement::Queued,
                play: false,
            })
            .expect("the songs sent")
            .wait()
            .expect("the songs queued");

        remove_by_identity(&player, first).expect("the first removed");

        player
            .request(Command::SetRepeat(RepeatMode::Off))
            .expect("a command after any removal")
            .wait()
            .expect("every command before it settled");

        remove_by_identity(&player, first).expect("a song already gone is no failure");

        player
            .request(Command::SetRepeat(RepeatMode::Off))
            .expect("a command after any removal")
            .wait()
            .expect("every command before it settled");

        let queued: Vec<TrackId> = player.queued().rows.iter().map(|item| item.id).collect();

        assert_eq!(queued, [second]);
    }

    #[test]
    fn a_party_started_with_no_server_asks_for_none_of_the_player_and_plays_nothing() {
        let mut config = Config::default();

        config.switches.say(Switch::SyncNearbyOnly, true);

        let cli = crate::fixtures::parsed(&["resonate", "party", "start", "a.flac"]);
        let wanted = PartyWanted::Start {
            files: vec!["a.flac".into()],
        };

        let outcome = run(&cli, &config, &wanted);

        assert!(matches!(outcome, Err(Error::PartyNeedsAServer)));
    }
}
