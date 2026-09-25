use resonate_core::{TrackId, Volume};
use resonate_engine::{Placement, Until};
use resonate_library::{Library, PlaylistName};
use resonate_mpris::{PlaybackStatus, Queueing, Seeking};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    Error, Result,
    catalog::{self, Wanted},
    controlling::Controlling,
    written::{self, compact, percent, seconds},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Play,
    Pause,
    Toggle,
    Stop,
    Next,
    Previous,
}

impl Action {
    pub const ALL: [Self; 6] = [
        Self::Play,
        Self::Pause,
        Self::Toggle,
        Self::Stop,
        Self::Next,
        Self::Previous,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Pause => "pause",
            Self::Toggle => "toggle",
            Self::Stop => "stop",
            Self::Next => "next",
            Self::Previous => "previous",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Adding {
    pub wanted: Wanted,
    pub next: bool,
    pub play: bool,
}

pub(crate) fn now_playing(player: &dyn Controlling) -> Result<Value> {
    let playing = player.metadata()?;
    let position = match playing {
        Some(_) => Some(player.position()?),
        None => None,
    };

    Ok(compact(json!({
        "player": player.name(),
        "status": player.playback()?.map(PlaybackStatus::as_str),
        "track": playing.as_ref().map(written::row),
        "position_seconds": position.map(seconds),
        "volume_percent": percent(player.volume()?),
        "sleep_timer": player.sleep()?.map(written::asleep),
    })))
}

pub(crate) fn control(player: &dyn Controlling, action: Action) -> Result<Value> {
    match action {
        Action::Play => player.play(),
        Action::Pause => player.pause(),
        Action::Toggle => player.play_pause(),
        Action::Stop => player.stop(),
        Action::Next => player.next(),
        Action::Previous => player.previous(),
    }?;

    Ok(compact(json!({
        "player": player.name(),
        "done": action.name(),
        "status": player.playback()?.map(PlaybackStatus::as_str),
        "track": player.metadata()?.as_ref().map(written::row),
    })))
}

pub(crate) fn seek(player: &dyn Controlling, by: Seeking) -> Result<Value> {
    player.seek(by)?;
    let moved = match by {
        Seeking::Forward(span) => seconds(span),
        Seeking::Backward(span) => -seconds(span),
    };
    let playing = player.metadata()?;
    let position = match playing {
        Some(_) => Some(seconds(player.position()?)),
        None => None,
    };

    Ok(compact(json!({
        "player": player.name(),
        "moved_seconds": moved,
        "track": playing.as_ref().map(written::row),
        "position_seconds": position,
    })))
}

pub(crate) fn set_volume(player: &dyn Controlling, volume: Volume) -> Result<Value> {
    player.set_volume(volume)?;

    Ok(json!({ "player": player.name(), "volume_percent": percent(player.volume()?) }))
}

pub(crate) fn queue(player: &dyn Controlling, most: usize) -> Result<Value> {
    let queued = player.queued()?;
    let playing = player.metadata()?.map(|described| described.track);
    let listed: Vec<Value> = player
        .described(&queued[..most.min(queued.len())])?
        .iter()
        .map(|described| {
            let mut row = written::row(described);
            if Some(described.track) == playing
                && let Value::Object(fields) = &mut row
            {
                fields.insert("playing".to_owned(), Value::Bool(true));
            }
            row
        })
        .collect();

    Ok(compact(json!({
        "player": player.name(),
        "rows": queued.len(),
        "playing_row": queued.iter().position(|track| Some(*track) == playing),
        "queue": listed,
    })))
}

pub(crate) fn add(player: &dyn Controlling, library: &Library, adding: &Adding) -> Result<Value> {
    let rows = catalog::wanted_rows(library, &adding.wanted)?;
    let queued = player.queue(
        &rows,
        Queueing {
            at: if adding.next {
                Placement::Next
            } else {
                Placement::Queued
            },
            play: adding.play,
        },
    )?;

    Ok(json!({
        "player": player.name(),
        "queued": queued,
        "rows": player.queued()?.len(),
    }))
}

pub(crate) fn remove(player: &dyn Controlling, track: TrackId) -> Result<Value> {
    if !player.queued()?.contains(&track) {
        return Err(Error::NotInTheQueue { track });
    }
    player.remove_track(track)?;

    Ok(json!({
        "player": player.name(),
        "removed": track.to_string(),
        "rows": player.queued()?.len(),
    }))
}

pub(crate) fn play_playlist(player: &dyn Controlling, name: &str) -> Result<Value> {
    let offered = player.playlists()?;
    let wanted = name.trim().to_lowercase();
    let found = offered
        .iter()
        .find(|playlist| playlist.name == name)
        .or_else(|| {
            offered
                .iter()
                .find(|playlist| playlist.name.to_lowercase() == wanted)
        })
        .ok_or_else(|| Error::PlaylistNotOffered(PlaylistName::new(name)))?;
    player.activate_playlist(found.id)?;

    Ok(json!({ "player": player.name(), "playing": found.name }))
}

pub(crate) fn set_sleep(player: &dyn Controlling, until: Option<Until>) -> Result<Value> {
    player.set_sleep(until)?;

    Ok(compact(json!({
        "player": player.name(),
        "sleep_timer": player.sleep()?.map(written::asleep),
    })))
}
