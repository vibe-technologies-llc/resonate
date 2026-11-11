use std::{collections::HashMap, time::Duration};

use resonate_core::{TrackId, Volume, ZoneId};
use resonate_engine::{Placement, Until};
use resonate_library::{Library, PlaylistName};
use resonate_mpris::{PlaybackStatus, Queueing, Seeking, ZoneAsked};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    Error, Result,
    catalog::{self, Wanted},
    controlling::{Controlling, Reach},
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

pub(crate) fn player_in(
    players: &dyn Reach,
    zone: Option<&ZoneAsked>,
) -> Result<Box<dyn Controlling>> {
    match zone {
        Some(zone) => players.zoned(zone),
        None => players.player(),
    }
}

pub(crate) fn zones(players: &dyn Reach) -> Result<Value> {
    let mut read: Vec<(Option<ZoneId>, Value)> = players
        .zones()?
        .iter()
        .filter_map(|zone| match zone_standing(&**zone) {
            Ok(standing) => Some((zone.zone_number(), standing)),
            Err(error) => {
                tracing::debug!(
                    %error,
                    name = %zone.name(),
                    "a zone stopped answering while it was being listed"
                );
                None
            }
        })
        .collect();
    read.sort_by_key(|(number, _)| *number);

    Ok(json!({
        "zones": read.into_iter().map(|(_, standing)| standing).collect::<Vec<_>>(),
    }))
}

fn zone_standing(zone: &dyn Controlling) -> Result<Value> {
    let mut standing = zone_named(zone);
    let device = zone.device().unwrap_or_else(|unread| {
        tracing::debug!(%unread, "a zone too old to name its device was listed without one");
        None
    });
    let reading = compact(json!({
        "device": device,
        "status": zone.playback()?.map(PlaybackStatus::as_str),
        "track": zone.metadata()?.as_ref().map(written::row),
        "rows": zone.queued()?.len(),
    }));
    if let (Value::Object(fields), Value::Object(read)) = (&mut standing, reading) {
        fields.extend(read);
    }
    Ok(standing)
}

fn zone_named(zone: &dyn Controlling) -> Value {
    compact(json!({
        "zone": zone.zone_number().map(ZoneId::get),
        "name": zone.zone().ok(),
        "player": zone.name(),
    }))
}

pub(crate) fn move_playback(
    players: &dyn Reach,
    from: Option<&ZoneAsked>,
    to: &ZoneAsked,
) -> Result<Value> {
    let leaving = player_in(players, from)?;
    let taking = players.zoned(to)?;
    let target = taking
        .zone_number()
        .ok_or_else(|| Error::NoSuchZone { zone: to.clone() })?;
    if leaving.name() == taking.name() {
        return Err(Error::MovedToItself { zone: target });
    }

    leaving.hand_over(target)?;

    Ok(json!({
        "from": zone_named(&*leaving),
        "to": zone_named(&*taking),
    }))
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
    let from = playing_at(player)?;

    player.seek(by)?;

    let playing = player.metadata()?;
    let landed = match &playing {
        Some(described) => Some((described.track, player.position()?)),
        None => None,
    };
    let moved = match (from, landed) {
        (Some((before, was)), Some((after, now))) if before == after => Some(between(was, now)),
        _ => None,
    };

    Ok(compact(json!({
        "player": player.name(),
        "moved_seconds": moved,
        "track": playing.as_ref().map(written::row),
        "position_seconds": landed.map(|(_, now)| seconds(now)),
    })))
}

fn between(was: Duration, now: Duration) -> f64 {
    if now >= was {
        seconds(now - was)
    } else {
        -seconds(was - now)
    }
}

fn playing_at(player: &dyn Controlling) -> Result<Option<(TrackId, Duration)>> {
    match player.metadata()? {
        Some(described) => Ok(Some((described.track, player.position()?))),
        None => Ok(None),
    }
}

pub(crate) fn set_volume(player: &dyn Controlling, volume: Volume) -> Result<Value> {
    player.set_volume(volume)?;

    Ok(json!({ "player": player.name(), "volume_percent": percent(player.volume()?) }))
}

pub(crate) fn queue(player: &dyn Controlling, most: usize) -> Result<Value> {
    let queued = player.queued()?;
    let playing = player.metadata()?.map(|described| described.track);
    let shuffle = player.shuffle()?;
    let play_positions = if shuffle {
        positions_in(&player.play_order().unwrap_or_default())
    } else {
        HashMap::new()
    };
    let listed: Vec<Value> = player
        .described(&queued[..most.min(queued.len())])?
        .iter()
        .map(|described| {
            let mut row = written::row(described);
            if let Value::Object(fields) = &mut row {
                if Some(described.track) == playing {
                    fields.insert("playing".to_owned(), Value::Bool(true));
                }
                if let Some(position) = play_positions.get(&described.track) {
                    fields.insert("play_position".to_owned(), json!(position));
                }
            }
            row
        })
        .collect();

    Ok(compact(json!({
        "player": player.name(),
        "rows": queued.len(),
        "playing_row": queued.iter().position(|track| Some(*track) == playing),
        "shuffle": shuffle,
        "queue": listed,
    })))
}

fn positions_in(order: &[TrackId]) -> HashMap<TrackId, usize> {
    order
        .iter()
        .enumerate()
        .map(|(position, track)| (*track, position))
        .collect()
}

pub(crate) fn add(player: &dyn Controlling, library: &Library, adding: &Adding) -> Result<Value> {
    let rows = catalog::rows_played(library, &adding.wanted)?;
    let before = player.queued()?.len();

    let queued = player
        .queue(
            &rows,
            Queueing {
                at: if adding.next {
                    Placement::Next
                } else {
                    Placement::Queued
                },
                play: adding.play,
            },
        )
        .map_err(|source| partway(player, before, rows.len(), source))?;

    Ok(json!({
        "player": player.name(),
        "queued": queued,
        "rows": player.queued()?.len(),
    }))
}

fn partway(
    player: &dyn Controlling,
    before: usize,
    asked: usize,
    source: resonate_mpris::Error,
) -> Error {
    match player.queued() {
        Ok(now) => Error::QueuedPartway {
            landed: now.len().saturating_sub(before),
            asked,
            source,
        },
        Err(unread) => {
            tracing::debug!(%unread, "the queue could not be read back after a failed queueing");
            Error::Mpris(source)
        }
    }
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
