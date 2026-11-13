use std::{
    ffi::OsString,
    io::{self, IsTerminal as _},
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, SystemTime},
};

use crossbeam_channel::{bounded, never, select, tick};
use resonate_core::{FrameSpan, Frames, MediaLocation, PlaylistId, SampleRate, Switch, Volume};
use resonate_engine::{
    Command, Counting, Event, Keeping, Listening, Placement, Player, QueueItem, RepeatMode, Until,
    stamp_of,
};
use resonate_library::{Library, Listen, Playing, PlaylistName};
use resonate_mpris::{PlayerName, Queueing, Running, Standing, ZoneAsked};

use crate::{
    arguments::queue_items_served,
    catalog,
    cli::{Cli, QueueArgs, TransportArgs},
    config::Config,
    discord,
    engine_settings::engine_config,
    error::{Error, Result},
    input,
    input::{Action, Pressed},
    kinship, mpris, open_library, playing_from, playlists, readout,
    readout::Readout,
    rendering::{Asked, Rendering},
    report,
    resuming::{keep_the_queue, keep_the_queue_on_leaving, resuming},
    served::Served,
    settings_path,
    sinks::{confirm_sink, wanted_sink},
    sleep, submitting, sync,
    table::Table,
    versions,
    vocabulary::Meaning,
    words,
    words::columns,
};

const HEARD_SAMPLE: Duration = Duration::from_millis(500);

const COMMAND_TIMEOUT: Duration = Duration::from_secs(3);

pub(crate) fn play(
    cli: &Cli,
    config: &Config,
    arguments: &[OsString],
    spec: Option<&str>,
    transport: &TransportArgs,
) -> Result<()> {
    let asleep = spec.map(sleep::Sleep::read).transpose()?;
    let library = catalog(cli, config);

    let items = versions::played_as_chosen(
        library.as_deref(),
        mpris::claimed_by(
            library.as_deref(),
            queue_items_served(arguments, &Served::of(config, settings_path(cli).ok())),
        ),
    );
    if items.is_empty() {
        return Err(if arguments.is_empty() {
            Error::NoFileNamed
        } else {
            Error::NothingPlayableNamed
        });
    }
    play_queue(
        cli,
        config,
        items,
        library,
        Opening {
            asleep: asleep.and_then(sleep::Sleep::until),
            ..Opening::default()
        },
        transport,
    )
}

pub(crate) fn queue_onto_a_running_player(
    cli: &Cli,
    config: &Config,
    wanted: &QueueArgs,
) -> Result<()> {
    let running = reached(wanted.player.as_deref(), wanted.zone.as_deref())?;
    if let Some(to) = wanted.move_to.as_deref().map(ZoneAsked::read) {
        return move_the_playback(&running, to);
    }
    let rows = match wanted.playlist.as_deref() {
        Some(name) => playlist_rows(cli, config, name)?,
        None => queue_items_served(&wanted.files, &Served::of(config, settings_path(cli).ok()))
            .into_iter()
            .map(|item| (item.location, item.span))
            .collect(),
    };
    let queued = running.queue(
        &rows,
        Queueing {
            at: if wanted.next {
                Placement::Next
            } else {
                Placement::Queued
            },
            play: wanted.play,
        },
    )?;

    said!(
        "{}",
        words::main::queued_onto(running.name().to_string(), queued)
    );
    Ok(())
}

fn move_the_playback(running: &Running, to: ZoneAsked) -> Result<()> {
    let target = Running::zoned(&to)?
        .and_then(|target| target.zone_number())
        .ok_or(Error::NoSuchZone { zone: to })?;
    running.hand_over(target)?;

    said!(
        "{}",
        words::main::moving_to(running.name().to_string(), target.to_string())
    );
    Ok(())
}

pub(crate) fn reached(player: Option<&str>, zone: Option<&str>) -> Result<Running> {
    if let Some(zone) = zone.map(ZoneAsked::read) {
        return Running::zoned(&zone)?.ok_or(Error::NoSuchZone { zone });
    }
    let Some(name) = player.map(PlayerName::new) else {
        return Running::found()?.ok_or(Error::NothingRunning);
    };
    Running::named(&name)?.ok_or(Error::NoSuchPlayer { name })
}

fn playlist_rows(
    cli: &Cli,
    config: &Config,
    name: &str,
) -> Result<Vec<(MediaLocation, Option<FrameSpan>)>> {
    let library = open_library(cli, config)?;
    let found = library
        .playlist_named(name)?
        .ok_or_else(|| Error::NoSuchPlaylist(PlaylistName::new(name)))?;

    let entries = library.playlist_entries(found.id, None)?;

    Ok(
        versions::played_as_chosen(Some(&library), playlists::queue_items(&entries))
            .into_iter()
            .map(|item| (item.location, item.span))
            .collect(),
    )
}

pub(crate) fn players() -> Result<()> {
    let standing: Vec<Standing> = Running::listed()?
        .iter()
        .filter_map(|player| match player.standing() {
            Ok(standing) => Some(standing),
            Err(error) => {
                tracing::debug!(
                    %error,
                    name = %player.name(),
                    "a player stopped answering while it was being listed"
                );
                None
            }
        })
        .collect();

    if standing.is_empty() {
        said!("{}", words::main::no_player());
        return Ok(());
    }

    let mut table = Table::new(vec![
        columns::player(),
        columns::zone(),
        columns::status(),
        columns::queued(),
        columns::title(),
        columns::artist(),
    ]);
    for player in &standing {
        table.push(vec![
            player.name.to_string(),
            player.zone.clone().unwrap_or_default(),
            player
                .playback
                .map_or_else(String::new, |status| status.as_str().to_owned()),
            player.queued.to_string(),
            player.title.clone().unwrap_or_default(),
            player.artist.clone().unwrap_or_default(),
        ]);
    }

    said_on!("{}", table.render());
    Ok(())
}

#[derive(Default)]
pub(crate) struct Opening {
    pub(crate) playing: Option<PlaylistId>,
    pub(crate) asleep: Option<Until>,
    pub(crate) place: Frames,
}

pub(crate) fn play_queue(
    cli: &Cli,
    config: &Config,
    items: Vec<QueueItem>,
    library: Option<Arc<Library>>,
    opening: Opening,
    transport: &TransportArgs,
) -> Result<()> {
    resonate_mpris::while_media_keys_are_heard(|| {
        play_queue_here(cli, config, items, library, opening, transport)
    })
}

fn play_queue_here(
    cli: &Cli,
    config: &Config,
    items: Vec<QueueItem>,
    library: Option<Arc<Library>>,
    opening: Opening,
    transport: &TransportArgs,
) -> Result<()> {
    let Opening {
        playing,
        asleep,
        place,
    } = opening;
    let sources = playing_from(
        library.as_deref(),
        &Served::of(config, settings_path(cli).ok()),
    );
    let mut engine = kinship::kin_from(engine_config(cli, config), library.as_ref());
    if let Some(percent) = transport.volume {
        engine.volume = at_percent(percent)?;
    }
    let wanted = wanted_sink(cli, config);
    let player = Arc::new(
        Rendering::of(config, wanted.as_ref(), Asked::Nothing)
            .player(engine, Arc::clone(&sources))?,
    );
    confirm_sink(&player, wanted.as_ref());
    let queue = stamp_of(&items);
    let start_at = if transport.shuffle {
        anywhere_in(items.len())
    } else {
        0
    };
    for command in set_out(transport, items, start_at) {
        player.send(command)?;
    }
    if place != Frames::default()
        && let Err(error) = player.send(Command::Seek(place))
    {
        tracing::warn!(%error, "the place the other device was at could not be reached");
    }
    if let Some(library) = library.as_ref() {
        library.set_playing_playlist(playing.map(|playlist| Playing { playlist, queue }));
    }
    if asleep.is_some() {
        player.send(Command::SleepUntil(asleep))?;
    }
    let keeps = resuming(config, library.as_deref());

    let (asked_to_quit, quitting) = bounded(1);
    let silenced = Arc::clone(&player);
    crate::signals::quit_when_told(
        asked_to_quit.clone(),
        crate::signals::silencing(move || vec![Arc::clone(&silenced)]),
    );
    let mpris = mpris::start(
        &mpris::Serving::new(
            &sources,
            library.as_ref(),
            settings_path(cli).ok(),
            Some(asked_to_quit),
            None,
            &Arc::new(AtomicBool::new(config.is_on(Switch::Notify))),
        )
        .heard_on_the_main_loop(),
        &player,
    );
    let presenter = discord::start(&player, library.as_ref(), config);
    #[cfg(any(windows, target_os = "macos"))]
    let _awake = {
        let playing = Arc::clone(&player);
        crate::awake::start(move || vec![Arc::clone(&playing)])
    };
    let submitting = library
        .as_ref()
        .map(|library| submitting::start(config, library, &player));
    let syncing = library
        .as_ref()
        .map(|library| sync::start(config, settings_path(cli).ok().as_deref(), library));
    #[cfg(feature = "sync")]
    if let Some(syncing) = syncing.as_ref() {
        syncing.drive(resonate_driven::Driven::player(&player));
    }

    let behind = input::played_in_the_background();
    let keyed = (!behind)
        .then(input::KeyAtATime::where_a_terminal)
        .flatten();
    let help = if keyed.is_some() {
        input::keys_said()
    } else {
        input::help_said()
    };
    let mut readout = Readout::over(keyed.is_some() && io::stdout().is_terminal());
    if behind {
        told!("{}", words::main::in_the_background());
    } else {
        said!("{help}");
    }
    let events = player.events().clone();
    let mut keys = if behind {
        never()
    } else if keyed.is_some() {
        input::keys()
    } else {
        input::lines()
    };
    let sampled = tick(HEARD_SAMPLE);
    let mut listening = Listening::default();
    let mut counted: Option<Listen> = None;
    let mut keeping = Keeping::default();
    let mut outcome = Played::default();
    let mut acted = Ok(());

    loop {
        select! {
            recv(events) -> event => {
                let Ok(event) = event else { break };
                readout.clear();
                outcome.hears(&event);
                if announce(event, &player) {
                    break;
                }
                readout.draw(&player);
            }
            recv(sampled) -> _ => {
                count_a_play(&player, library.as_deref(), &mut listening, &mut counted);
                if keeps {
                    keep_the_queue(&player, library.as_deref(), &mut keeping);
                }
                readout.draw(&player);
            }
            recv(keys) -> pressed => {
                let Ok(pressed) = pressed else {
                    keys = never();
                    continue;
                };
                match pressed {
                    Pressed::Acted(Action::Quit) => break,
                    Pressed::Acted(action) => {
                        readout.clear();
                        acted = act(&player, action, &help);
                        if acted.is_err() {
                            break;
                        }
                    }
                    Pressed::Typing(typed) => readout.typing(typed),
                    Pressed::Unknown(line) => {
                        readout.clear();
                        told!("{}", words::main::unknown_key(format!("{line:?}")));
                    }
                }
                readout.draw(&player);
            }
            recv(quitting) -> _ => break,
        }
    }
    readout.clear();
    drop(keyed);

    if keeps {
        keep_the_queue_on_leaving(&player, library.as_deref(), &mut keeping);
    }
    count_a_play(&player, library.as_deref(), &mut listening, &mut counted);
    if let Some(library) = library.as_deref() {
        record_a_play(library, listening.leaves(), &mut counted);
    }
    presenter.leave();
    if let Some(syncing) = syncing {
        syncing.leave();
    }
    if let Some(submitting) = submitting {
        submitting.leave();
    }
    drop(mpris);
    drop(player);
    acted?;
    outcome.played_anything()
}

#[derive(Default)]
pub(crate) struct Played {
    started: bool,
    failed: usize,
    finished: bool,
}

impl Played {
    const fn hears(&mut self, event: &Event) {
        match event {
            Event::TrackStarted(_) => self.started = true,
            Event::Failed { .. } => self.failed += 1,
            Event::QueueFinished => self.finished = true,
            _ => {}
        }
    }

    const fn played_anything(&self) -> Result<()> {
        if !self.finished || self.started || self.failed == 0 {
            Ok(())
        } else {
            Err(Error::NothingPlayed {
                failed: self.failed,
            })
        }
    }
}

fn at_percent(percent: u8) -> Result<Volume> {
    Ok(Volume::new(f32::from(percent) / 100.0)?)
}

fn anywhere_in(rows: usize) -> usize {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|since| since.subsec_nanos() as usize)
        .unwrap_or_default();

    now.checked_rem(rows).unwrap_or_default()
}

fn set_out(transport: &TransportArgs, items: Vec<QueueItem>, start_at: usize) -> Vec<Command> {
    let shuffle = transport.shuffle.then_some(Command::SetShuffle(true));
    let repeat = transport
        .repeat
        .map(|repeat| Command::SetRepeat(repeat.meaning()));
    let load = Command::Load {
        items,
        start_at,
        autoplay: true,
    };

    shuffle.into_iter().chain(repeat).chain([load]).collect()
}

fn count_a_play(
    player: &Player,
    library: Option<&Library>,
    listening: &mut Listening,
    counted: &mut Option<Listen>,
) {
    let Some(library) = library else {
        return;
    };
    record_a_play(
        library,
        listening.heard(&player.state(), &player.queue()),
        counted,
    );
}

fn record_a_play(library: &Library, counting: Option<Counting>, counted: &mut Option<Listen>) {
    let Some(counting) = counting else {
        return;
    };

    match counting {
        Counting::Counts(played) => {
            *counted = match library.track_played(&played.location, played.span, played.heard) {
                Ok(row) => row.map(|row| row.listen),
                Err(error) => {
                    tracing::warn!(%error, "a play was not counted");
                    None
                }
            };
        }
        Counting::Hears(played) => {
            if let Some(listen) = *counted
                && let Err(error) = library.listened(listen, played.heard)
            {
                tracing::warn!(%error, "how long a play has been heard for was not kept");
            }
        }
        Counting::Settles(played) => {
            if let Some(listen) = counted.take()
                && let Err(error) = library.listened(listen, played.heard)
            {
                tracing::warn!(%error, "how long a play was heard for was not kept");
            }
        }
        Counting::Passes(heard) => {
            if let Err(error) = library.passed(heard) {
                tracing::warn!(%error, "how long a passing listen lasted was not kept");
            }
        }
    }
}

fn announce(event: Event, player: &Player) -> bool {
    match event {
        Event::TrackStarted(track) => {
            said!("{}", words::main::started(readout::billing(player, track)))
        }
        Event::TrackFinished(track) => {
            said!("{}", words::main::finished(readout::billing(player, track)))
        }
        Event::OutputChanged(status) => said!(
            "{}",
            words::main::output(
                format!("{:?}", status.mode),
                status.sink.to_string(),
                status.negotiated.to_string(),
            )
        ),
        Event::Underrun { missing } => told!("{}", words::main::underrun(missing.to_string())),
        Event::Failed { track, error } => {
            told!("{}", words::main::failed(readout::billing(player, track)));
            report(&error);
        }
        Event::Waiting { track, error } => {
            told!("{}", words::main::waits(readout::billing(player, track)));
            report(&error);
        }
        Event::CommandFailed { command, error } => {
            told!("{}", words::main::rejected(format!("{command:?}")));
            report(&error);
        }
        Event::QueueFinished => return true,
    }
    false
}

fn act(player: &Player, action: Action, help: &str) -> Result<()> {
    let state = player.state();
    let rate = state.current.map(|track| track.source.rate);

    let command = match action {
        Action::Toggle => Command::TogglePlayPause,
        Action::Next => Command::Next,
        Action::Previous => Command::Previous,
        Action::Stop => Command::Stop,
        Action::SeekTo(moment) => {
            let Some(rate) = rate else { return Ok(()) };
            Command::Seek(Frames::from_duration(moment, rate))
        }
        Action::SeekForward(span) => {
            let Some(rate) = rate else { return Ok(()) };
            Command::SeekBy(frames_in(span, rate))
        }
        Action::SeekBack(span) => {
            let Some(rate) = rate else { return Ok(()) };
            Command::SeekBy(-frames_in(span, rate))
        }
        Action::VolumeBy(step) => {
            let percent = (state.volume.get() * 100.0).round() as i32;
            let wanted = percent.saturating_add(step).clamp(0, 100);
            Command::SetVolume(Volume::new(wanted as f32 / 100.0)?)
        }
        Action::ToggleShuffle => Command::SetShuffle(!state.shuffle),
        Action::CycleRepeat => Command::SetRepeat(match state.repeat {
            RepeatMode::Off => RepeatMode::Queue,
            RepeatMode::Queue => RepeatMode::Track,
            RepeatMode::Track => RepeatMode::Off,
        }),
        Action::Sleep(wanted) => Command::SleepUntil(wanted.until()),
        Action::Help => {
            said!("{help}");
            return Ok(());
        }
        Action::Quit => return Ok(()),
    };

    let kind = command.kind();
    if let Err(error) = player.request(command)?.wait_for(COMMAND_TIMEOUT) {
        told!("{}", words::main::rejected(format!("{kind:?}")));
        report(&error);
    }
    Ok(())
}

fn frames_in(span: Duration, rate: SampleRate) -> i64 {
    i64::try_from(Frames::from_duration(span, rate).get()).unwrap_or(i64::MAX)
}

#[cfg(not(feature = "ui"))]
pub(crate) fn launch(cli: Cli, config: Config, _library: Arc<Library>) -> Result<()> {
    if !cli.files.is_empty() {
        return play(&cli, &config, &cli.files, None, &TransportArgs::default());
    }
    let player = Player::new(engine_config(&cli, &config))?;
    confirm_sink(&player, wanted_sink(&cli, &config).as_ref());
    player.shutdown()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::*;
    use crate::{
        cli::{RepeatArg, Sub},
        fixtures::parsed,
    };

    #[test]
    fn a_queue_that_finished_with_every_track_failed_is_no_success() {
        let track = resonate_core::TrackId::new(1).expect("an id");
        let failed = || Event::Failed {
            track,
            error: resonate_engine::Error::QueueEmpty,
        };

        let mut every_one_failed = Played::default();
        for event in [failed(), failed(), Event::QueueFinished] {
            every_one_failed.hears(&event);
        }
        let mut one_played = Played::default();
        for event in [failed(), Event::TrackStarted(track), Event::QueueFinished] {
            one_played.hears(&event);
        }
        let mut left_early = Played::default();
        left_early.hears(&failed());

        assert!(matches!(
            every_one_failed.played_anything(),
            Err(Error::NothingPlayed { failed: 2 })
        ));
        assert!(one_played.played_anything().is_ok());
        assert!(left_early.played_anything().is_ok());
    }

    fn transport_of(arguments: &[&str]) -> TransportArgs {
        match parsed(arguments).command {
            Some(Sub::Play { transport, .. }) => transport,
            other => panic!("{arguments:?} is not play: {other:?}"),
        }
    }

    #[test]
    fn play_takes_shuffle_repeat_and_a_volume_for_the_run() {
        let bare = transport_of(&["resonate", "play", "a.flac"]);
        let set = transport_of(&[
            "resonate",
            "play",
            "a.flac",
            "--shuffle",
            "--repeat",
            "track",
            "--volume",
            "40",
        ]);
        let refused = |volume: &str| {
            Cli::try_parse_from(["resonate", "play", "a.flac", "--volume", volume]).is_err()
        };

        assert!(!bare.shuffle);
        assert_eq!(bare.repeat, None);
        assert_eq!(bare.volume, None);
        assert!(set.shuffle);
        assert_eq!(set.repeat, Some(RepeatArg::Track));
        assert_eq!(set.volume, Some(40));
        assert!(refused("101"));
        assert!(refused("-1"));
        assert!(!refused("0"));
        assert!(!refused("100"));
        assert_eq!(at_percent(40).ok(), Volume::new(0.4).ok());
        assert_eq!(at_percent(100).ok(), Some(Volume::MAX));
    }

    #[test]
    fn the_transport_is_set_before_the_queue_loads_so_the_first_track_is_already_shuffled() {
        let set = transport_of(&[
            "resonate",
            "play",
            "a.flac",
            "--shuffle",
            "--repeat",
            "queue",
        ]);
        let bare = transport_of(&["resonate", "play", "a.flac"]);

        let commands = set_out(&set, Vec::new(), 3);
        let loaded = set_out(&bare, Vec::new(), 0);

        assert_eq!(
            commands,
            vec![
                Command::SetShuffle(true),
                Command::SetRepeat(RepeatMode::Queue),
                Command::Load {
                    items: Vec::new(),
                    start_at: 3,
                    autoplay: true,
                },
            ]
        );
        assert_eq!(
            loaded,
            vec![Command::Load {
                items: Vec::new(),
                start_at: 0,
                autoplay: true,
            }]
        );
        assert_eq!(anywhere_in(0), 0);
        assert!(anywhere_in(7) < 7);
    }
}
