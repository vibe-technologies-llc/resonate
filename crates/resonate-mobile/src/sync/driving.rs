use std::time::{Duration, Instant};

use resonate_core::ZoneId;
use resonate_sync::{
    Controlling, Mirror, Permille, RemoteCommand, RemotePlayback, RemoteRow, RemoteZone, Watched,
};

use super::the_device;
use crate::{Error, MobilePlayer, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RemoteControl {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Seek { millis: u64 },
    Volume { permille: u16 },
    Move { from: u32, to: u32 },
    Remove { row: u32 },
    Jump { row: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RemoteAnswer {
    Done,
    Untrusted,
    Uncontrollable,
    NoSuchZone,
    NoSuchRow,
    Failed,
    Unanswered,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RemoteStanding {
    Waiting,
    Mirrored,
    Refused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RemotePlaying {
    Idle,
    Playing,
    Paused,
    Stopped,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RemoteSong {
    pub title: String,
    pub artist: String,
    pub length_millis: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RemoteZoneState {
    pub zone: u32,
    pub name: String,
    pub playing: RemotePlaying,
    pub playing_row: Option<u32>,
    pub position_millis: u64,
    pub length_millis: Option<u64>,
    pub volume_permille: u16,
    pub queue_length: u32,
    pub first_row: u32,
    pub rows: Vec<RemoteSong>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RemoteDevice {
    pub standing: RemoteStanding,
    pub zones: Vec<RemoteZoneState>,
}

fn millis(length: Duration) -> u64 {
    u64::try_from(length.as_millis()).unwrap_or(u64::MAX)
}

fn counted(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

fn row(given: u32) -> usize {
    usize::try_from(given).unwrap_or(usize::MAX)
}

const fn playing_of(playback: RemotePlayback) -> RemotePlaying {
    match playback {
        RemotePlayback::Idle => RemotePlaying::Idle,
        RemotePlayback::Playing => RemotePlaying::Playing,
        RemotePlayback::Paused => RemotePlaying::Paused,
        RemotePlayback::Stopped => RemotePlaying::Stopped,
    }
}

const fn answer_of(controlling: Controlling) -> RemoteAnswer {
    match controlling {
        Controlling::Done => RemoteAnswer::Done,
        Controlling::Untrusted => RemoteAnswer::Untrusted,
        Controlling::Uncontrollable => RemoteAnswer::Uncontrollable,
        Controlling::NoSuchZone => RemoteAnswer::NoSuchZone,
        Controlling::NoSuchRow => RemoteAnswer::NoSuchRow,
        Controlling::Failed => RemoteAnswer::Failed,
        Controlling::Unanswered => RemoteAnswer::Unanswered,
    }
}

pub(crate) fn command_of(control: RemoteControl) -> RemoteCommand {
    match control {
        RemoteControl::Play => RemoteCommand::Play,
        RemoteControl::Pause => RemoteCommand::Pause,
        RemoteControl::Toggle => RemoteCommand::Toggle,
        RemoteControl::Next => RemoteCommand::Next,
        RemoteControl::Previous => RemoteCommand::Previous,
        RemoteControl::Seek { millis } => RemoteCommand::Seek(Duration::from_millis(millis)),
        RemoteControl::Volume { permille } => RemoteCommand::Volume(Permille::new(permille)),
        RemoteControl::Move { from, to } => RemoteCommand::Move {
            from: row(from),
            to: row(to),
        },
        RemoteControl::Remove { row: at } => RemoteCommand::Remove(row(at)),
        RemoteControl::Jump { row: at } => RemoteCommand::Jump(row(at)),
    }
}

fn song_of(row: &RemoteRow) -> RemoteSong {
    RemoteSong {
        title: row.title.clone(),
        artist: row.artist.clone(),
        length_millis: row.length.map(millis),
    }
}

fn zone_state(mirror: &Mirror, zone: &RemoteZone, now: Instant) -> RemoteZoneState {
    RemoteZoneState {
        zone: zone.zone.get(),
        name: zone.name.clone(),
        playing: playing_of(zone.playback),
        playing_row: zone.playing.map(counted),
        position_millis: millis(mirror.position_of(zone, now)),
        length_millis: zone.length.map(millis),
        volume_permille: zone.volume.get(),
        queue_length: counted(zone.queue_length),
        first_row: counted(zone.first_row),
        rows: zone.rows.iter().map(song_of).collect(),
    }
}

pub(crate) fn device_of(watched: &Watched, now: Instant) -> RemoteDevice {
    match watched {
        Watched::Waiting => RemoteDevice {
            standing: RemoteStanding::Waiting,
            zones: Vec::new(),
        },
        Watched::Refused(_) => RemoteDevice {
            standing: RemoteStanding::Refused,
            zones: Vec::new(),
        },
        Watched::Mirrored(mirror) => RemoteDevice {
            standing: RemoteStanding::Mirrored,
            zones: mirror
                .zones
                .iter()
                .map(|zone| zone_state(mirror, zone, now))
                .collect(),
        },
    }
}

#[uniffi::export]
impl MobilePlayer {
    pub fn sync_watch(&self, device: String) -> Result<RemoteDevice> {
        let running = self.syncing.running.lock();
        let running = running.as_ref().ok_or(Error::SyncOff)?;
        let device = the_device(&running.sync, &device).ok_or(Error::UnknownSyncDevice)?;
        running.sync.watch(device.id);
        Ok(device_of(&running.sync.watched(device.id), Instant::now()))
    }

    pub fn sync_unwatch(&self, device: String) {
        let running = self.syncing.running.lock();
        if let Some(running) = running.as_ref()
            && let Some(device) = the_device(&running.sync, &device)
        {
            running.sync.unwatch(device.id);
        }
    }

    pub fn sync_control(
        &self,
        device: String,
        zone: u32,
        control: RemoteControl,
    ) -> Result<RemoteAnswer> {
        let asked = {
            let running = self.syncing.running.lock();
            let running = running.as_ref().ok_or(Error::SyncOff)?;
            let device = the_device(&running.sync, &device).ok_or(Error::UnknownSyncDevice)?;
            let Some(zone) = ZoneId::new(zone) else {
                return Ok(RemoteAnswer::NoSuchZone);
            };
            running
                .sync
                .ask_to_control(device.id, zone, command_of(control))
        };
        Ok(answer_of(asked.answer()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mirrored_zone_reaches_the_phones_with_its_place_played_on_and_its_rows_numbered() {
        let heard_at = Instant::now();
        let zone = RemoteZone {
            zone: ZoneId::FIRST,
            name: "Living room".into(),
            playback: RemotePlayback::Playing,
            playing: Some(3),
            position: Duration::from_secs(10),
            length: Some(Duration::from_secs(240)),
            volume: Permille::new(700),
            queue_length: 9,
            first_row: 2,
            rows: vec![RemoteRow {
                title: "Echoes".into(),
                artist: "Pink Floyd".into(),
                length: None,
            }],
        };
        let watched = Watched::Mirrored(Mirror {
            zones: vec![zone],
            heard_at,
        });

        let shown = device_of(&watched, heard_at + Duration::from_secs(2));

        assert_eq!(shown.standing, RemoteStanding::Mirrored);
        assert_eq!(shown.zones[0].position_millis, 12_000);
        assert_eq!(shown.zones[0].playing_row, Some(3));
        assert_eq!(shown.zones[0].first_row, 2);
        assert_eq!(shown.zones[0].rows[0].title, "Echoes");
        assert_eq!(
            command_of(RemoteControl::Move { from: 4, to: 1 }),
            RemoteCommand::Move { from: 4, to: 1 }
        );
    }
}
