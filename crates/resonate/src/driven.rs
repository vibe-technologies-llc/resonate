use std::sync::{Arc, Weak};

use resonate_core::{Frames, Span, ZoneId};
#[cfg(feature = "ui")]
use resonate_engine::Zones;
use resonate_engine::{Command, PlaybackState, Player, QueueItem};
use resonate_sync::{
    Controlled, Permille, ROWS_MIRRORED_AT_MOST, RemoteCommand, RemotePlayback, RemoteRow,
    RemoteZone, Resumed, Uncontrolled, ZONES_MIRRORED_AT_MOST,
};

const FIRST_ZONE_NAME: &str = "Zone 1";

pub(crate) enum Driven {
    #[cfg(feature = "ui")]
    Zones(Weak<Zones>),
    Player(Weak<Player>),
}

impl Driven {
    fn players(&self) -> Vec<(ZoneId, String, Arc<Player>)> {
        match self {
            #[cfg(feature = "ui")]
            Self::Zones(zones) => zones.upgrade().map_or_else(Vec::new, |zones| {
                zones
                    .listed()
                    .into_iter()
                    .map(|zone| (zone.id, zone.name.as_str().to_owned(), zone.player))
                    .collect()
            }),
            Self::Player(player) => player
                .upgrade()
                .map(|player| (ZoneId::FIRST, FIRST_ZONE_NAME.to_owned(), player))
                .into_iter()
                .collect(),
        }
    }

    fn sent(&self, zone: ZoneId, player: &Player, command: Command) -> Result<(), Uncontrolled> {
        let sent = match self {
            #[cfg(feature = "ui")]
            Self::Zones(zones) => zones
                .upgrade()
                .ok_or(Uncontrolled::NoSuchZone)?
                .send(zone, command),
            Self::Player(_) => player.send(command),
        };
        sent.map_err(|error| {
            tracing::debug!(%error, %zone, "a command from another device was not taken");
            Uncontrolled::Failed
        })
    }
}

fn playback_of(state: PlaybackState) -> RemotePlayback {
    match state {
        PlaybackState::Idle => RemotePlayback::Idle,
        PlaybackState::Buffering | PlaybackState::Playing => RemotePlayback::Playing,
        PlaybackState::Paused => RemotePlayback::Paused,
        PlaybackState::Stopped => RemotePlayback::Stopped,
    }
}

fn row_of(player: &Player, item: &QueueItem) -> RemoteRow {
    let media = player.media(&item.location, item.span);
    let tags = media.as_ref().map(|media| &media.tags);
    let stem = || {
        item.location
            .as_path()
            .and_then(|path| path.file_stem())
            .map_or_else(
                || item.location.to_uri(),
                |stem| stem.to_string_lossy().into_owned(),
            )
    };
    RemoteRow {
        title: tags
            .and_then(|tags| tags.title.clone())
            .unwrap_or_else(stem),
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

fn zone_of(zone: ZoneId, name: String, player: &Player) -> RemoteZone {
    let state = player.state();
    let queue = player.queue();
    let window = Resumed::window_around(state.queue_position.unwrap_or(0), queue.len());
    let first_row = window.start;
    let rows = queue
        .iter()
        .skip(first_row)
        .take(window.len().min(ROWS_MIRRORED_AT_MOST))
        .map(|item| row_of(player, item))
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
        rows,
    }
}

fn a_row(row: usize, rows: usize) -> Result<usize, Uncontrolled> {
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
            .map(|(zone, name, player)| zone_of(zone, name, &player))
            .collect()
    }

    fn control(&self, zone: ZoneId, command: RemoteCommand) -> Result<(), Uncontrolled> {
        let (_, _, player) = self
            .players()
            .into_iter()
            .find(|(id, _, _)| *id == zone)
            .ok_or(Uncontrolled::NoSuchZone)?;
        let command = command_for(&player, command)?;
        self.sent(zone, &player, command)
    }
}
