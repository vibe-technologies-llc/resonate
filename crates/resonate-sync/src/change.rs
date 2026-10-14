use std::time::{Duration, SystemTime, UNIX_EPOCH};

use resonate_core::Recording;

use crate::{DeviceId, Result, key::random_bytes};

pub const PLAYLIST_KEY_BYTES: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Stamp {
    pub nanos: i64,
    pub device: DeviceId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlaylistKey(pub [u8; PLAYLIST_KEY_BYTES]);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Platform {
    Linux,
    Android,
    Ios,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceNote {
    pub device: DeviceId,
    pub name: String,
    pub platform: Platform,
    pub stamp: Stamp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaylistBody {
    pub name: String,
    pub entries: Vec<Recording>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaylistNote {
    pub key: PlaylistKey,
    pub stamp: Stamp,
    pub body: Option<PlaylistBody>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FavouriteNote {
    pub recording: Recording,
    pub favoured: bool,
    pub stamp: Stamp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listen {
    pub device: DeviceId,
    pub id: u64,
    pub recording: Recording,
    pub at: i64,
    pub heard: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handoff {
    pub device: DeviceId,
    pub stamp: Stamp,
    pub rows: Vec<Recording>,
    pub at: u32,
    pub position: Duration,
    pub playing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Device(DeviceNote),
    Playlist(PlaylistNote),
    Favourite(FavouriteNote),
    Listen(Listen),
    Handoff(Handoff),
}

#[derive(Clone, Copy, Debug)]
pub struct Clock {
    last: i64,
}

impl PlaylistKey {
    pub fn minted() -> Result<Self> {
        random_bytes().map(Self)
    }
}

impl Clock {
    pub const fn from_last(last: i64) -> Self {
        Self { last }
    }

    pub const fn last(&self) -> i64 {
        self.last
    }

    pub fn now(&mut self, device: DeviceId) -> Stamp {
        let nanos = wall_nanos().max(self.last.saturating_add(1));
        self.last = nanos;
        Stamp { nanos, device }
    }

    pub fn after(&mut self, nanos: i64, device: DeviceId) -> Stamp {
        let nanos = nanos.max(self.last.saturating_add(1));
        self.last = nanos;
        Stamp { nanos, device }
    }

    pub fn saw(&mut self, stamp: Stamp) {
        self.last = self.last.max(stamp.nanos);
    }
}

pub fn wall_nanos() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|since| i64::try_from(since.as_nanos()).ok())
        .unwrap_or_default()
}

impl Change {
    pub fn stamp(&self) -> Option<Stamp> {
        match self {
            Self::Device(note) => Some(note.stamp),
            Self::Playlist(note) => Some(note.stamp),
            Self::Favourite(note) => Some(note.stamp),
            Self::Handoff(handoff) => Some(handoff.stamp),
            Self::Listen(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_never_steps_back_past_what_it_has_seen() {
        let device = DeviceId([1; 16]);
        let mut clock = Clock::from_last(0);
        let ahead = Stamp {
            nanos: wall_nanos() + 3_600_000_000_000,
            device: DeviceId([2; 16]),
        };

        let first = clock.now(device);
        clock.saw(ahead);
        let second = clock.now(device);

        assert!(second.nanos > ahead.nanos);
        assert!(second > first);
        assert!(clock.after(0, device) > second);
    }

    #[test]
    fn stamps_at_one_instant_are_ordered_by_device() {
        let earlier = Stamp {
            nanos: 5,
            device: DeviceId([1; 16]),
        };
        let later = Stamp {
            nanos: 5,
            device: DeviceId([2; 16]),
        };

        assert!(later > earlier);
    }
}
