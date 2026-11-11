use std::{
    sync::{Arc, Weak},
    time::Duration,
};

use resonate_core::{Frames, Span, ZoneId, ZoneName};
use resonate_engine::{Command, Error, PlaybackState, Player, QueueItem, Zones};
use resonate_sync::{
    Controlled, Permille, ROWS_MIRRORED_AT_MOST, RemoteCommand, RemotePlayback, RemoteRow,
    RemoteZone, Resumed, Uncontrolled, ZONES_MIRRORED_AT_MOST,
};

const AN_EDIT_BY_ROW_ANSWERED_WITHIN: Duration = Duration::from_secs(2);

pub trait RowNames: Send + Sync + 'static {
    fn rows(&self, player: &Player, items: &[Arc<QueueItem>]) -> Vec<RemoteRow>;
}

pub struct ByTheirTags;

impl RowNames for ByTheirTags {
    fn rows(&self, player: &Player, items: &[Arc<QueueItem>]) -> Vec<RemoteRow> {
        items.iter().map(|item| row_by_tags(player, item)).collect()
    }
}

pub fn unnamed(item: &QueueItem) -> String {
    item.location
        .as_path()
        .and_then(|path| path.file_stem())
        .map_or_else(
            || item.location.to_uri(),
            |stem| stem.to_string_lossy().into_owned(),
        )
}

fn row_by_tags(player: &Player, item: &QueueItem) -> RemoteRow {
    let media = player.media(&item.location, item.span);
    let tags = media.as_ref().map(|media| &media.tags);

    RemoteRow {
        title: tags
            .and_then(|tags| tags.title.clone())
            .unwrap_or_else(|| unnamed(item)),
        artist: tags
            .and_then(|tags| tags.artist.clone())
            .unwrap_or_default(),
        length: media.as_ref().and_then(|media| {
            media
                .duration
                .map(|frames| frames.to_duration(media.spec.rate))
        }),
    }
}

enum Driving {
    Zones(Weak<Zones>),
    Player(Weak<Player>),
}

pub struct Driven {
    driving: Driving,
    names: Box<dyn RowNames>,
}

impl Driven {
    pub fn zones(zones: &Arc<Zones>, names: impl RowNames) -> Self {
        Self {
            driving: Driving::Zones(Arc::downgrade(zones)),
            names: Box::new(names),
        }
    }

    pub fn player(player: &Arc<Player>) -> Self {
        Self {
            driving: Driving::Player(Arc::downgrade(player)),
            names: Box::new(ByTheirTags),
        }
    }

    fn players(&self) -> Vec<(ZoneId, String, Arc<Player>)> {
        match &self.driving {
            Driving::Zones(zones) => zones.upgrade().map_or_else(Vec::new, |zones| {
                zones
                    .listed()
                    .into_iter()
                    .map(|zone| (zone.id, zone.name.as_str().to_owned(), zone.player))
                    .collect()
            }),
            Driving::Player(player) => player
                .upgrade()
                .map(|player| {
                    let name = ZoneName::first_given_to(ZoneId::FIRST);
                    (ZoneId::FIRST, name.as_str().to_owned(), player)
                })
                .into_iter()
                .collect(),
        }
    }

    fn sent(&self, zone: ZoneId, player: &Player, command: Command) -> Result<(), Uncontrolled> {
        let sent = match &self.driving {
            Driving::Zones(zones) => zones
                .upgrade()
                .ok_or(Uncontrolled::NoSuchZone)?
                .send(zone, command),
            Driving::Player(_) => player.send(command),
        };
        sent.map_err(|error| {
            tracing::debug!(%error, %zone, "a command from another device was not taken");
            Uncontrolled::Failed
        })
    }

    fn zone_of(&self, zone: ZoneId, name: String, player: &Player) -> RemoteZone {
        let state = player.state();
        let queued = player.queued();
        let queue = queued.rows;
        let window = Resumed::window_around(state.queue_position.unwrap_or(0), queue.len());
        let first_row = window.start;
        let held: Vec<Arc<QueueItem>> = queue
            .iter()
            .skip(first_row)
            .take(window.len().min(ROWS_MIRRORED_AT_MOST))
            .cloned()
            .collect();

        RemoteZone {
            zone,
            name,
            playback: playback_of(state.playback),
            playing: state.queue_position,
            position: state.current.map_or_else(Default::default, |track| {
                track.position.to_duration(track.source.rate)
            }),
            length: state.current.and_then(|track| {
                track
                    .duration
                    .map(|frames| frames.to_duration(track.source.rate))
            }),
            volume: Permille::of(state.volume),
            queue_length: queue.len(),
            first_row,
            rows: self.names.rows(player, &held),
            queue_revision: Some(queued.revision),
        }
    }

    fn player_of(&self, zone: ZoneId) -> Result<Arc<Player>, Uncontrolled> {
        self.players()
            .into_iter()
            .find(|(id, _, _)| *id == zone)
            .map(|(_, _, player)| player)
            .ok_or(Uncontrolled::NoSuchZone)
    }
}

const fn playback_of(state: PlaybackState) -> RemotePlayback {
    match state {
        PlaybackState::Idle => RemotePlayback::Idle,
        PlaybackState::Buffering | PlaybackState::Playing => RemotePlayback::Playing,
        PlaybackState::Paused => RemotePlayback::Paused,
        PlaybackState::Stopped => RemotePlayback::Stopped,
    }
}

const fn a_row(row: usize, rows: usize) -> Result<usize, Uncontrolled> {
    if row < rows {
        Ok(row)
    } else {
        Err(Uncontrolled::NoSuchRow)
    }
}

fn command_for(player: &Player, command: RemoteCommand) -> Result<Command, Uncontrolled> {
    let rows = player.queue().len();
    Ok(match command {
        RemoteCommand::Play => Command::Play,
        RemoteCommand::Pause => Command::Pause,
        RemoteCommand::Toggle => Command::TogglePlayPause,
        RemoteCommand::Next => Command::Next,
        RemoteCommand::Previous => Command::Previous,
        RemoteCommand::Seek(at) => {
            let track = player.state().current.ok_or(Uncontrolled::NoSuchRow)?;
            Command::Seek(Frames::from_duration(at, track.source.rate))
        }
        RemoteCommand::Volume(volume) => Command::SetVolume(volume.volume()),
        RemoteCommand::Move { from, to } => Command::Move {
            rows: Span::one(a_row(from, rows)?),
            to: a_row(to, rows)?,
        },
        RemoteCommand::Remove(row) => Command::Remove(Span::one(a_row(row, rows)?)),
        RemoteCommand::Jump(row) => Command::JumpTo(a_row(row, rows)?),
    })
}

impl Controlled for Driven {
    fn zones(&self) -> Vec<RemoteZone> {
        self.players()
            .into_iter()
            .take(ZONES_MIRRORED_AT_MOST)
            .map(|(zone, name, player)| self.zone_of(zone, name, &player))
            .collect()
    }

    fn control(&self, zone: ZoneId, command: RemoteCommand) -> Result<(), Uncontrolled> {
        let player = self.player_of(zone)?;
        let command = command_for(&player, command)?;
        self.sent(zone, &player, command)
    }

    fn control_if_the_queue_is_still(
        &self,
        zone: ZoneId,
        revision: u64,
        command: RemoteCommand,
    ) -> Result<(), Uncontrolled> {
        if !command.names_a_row() {
            return self.control(zone, command);
        }
        let player = self.player_of(zone)?;
        let command = command_for(&player, command)?;
        let answered = player
            .request_if_the_queue_is_still(revision, command)
            .and_then(|outcome| outcome.wait_for(AN_EDIT_BY_ROW_ANSWERED_WITHIN));
        match answered {
            Ok(()) | Err(Error::CommandPending { .. }) => Ok(()),
            Err(Error::QueueChanged { .. }) => Err(Uncontrolled::QueueMoved),
            Err(Error::NoSuchRow { .. }) => Err(Uncontrolled::NoSuchRow),
            Err(error) => {
                tracing::debug!(%error, %zone, "an edit from another device was not taken");
                Err(Uncontrolled::Failed)
            }
        }
    }
}
