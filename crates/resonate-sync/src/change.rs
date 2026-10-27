use std::time::{Duration, SystemTime, UNIX_EPOCH};

use resonate_core::{Answer, Recording};

use crate::{DeviceId, Result, key::random_bytes};

pub const PLAYLIST_KEY_BYTES: usize = 16;
const MINTED_KEY_MARK: [u8; 4] = *b"RSK1";
const MINTED_NANOS_FROM: usize = MINTED_KEY_MARK.len();
const MINTED_NANOS_TO: usize = MINTED_NANOS_FROM + 8;

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

impl Platform {
    pub fn label(self) -> &'static str {
        match self {
            Platform::Linux => "Linux",
            Platform::Android => "Android",
            Platform::Ios => "iOS",
            Platform::Other => "Device",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceNote {
    pub device: DeviceId,
    pub name: String,
    pub platform: Platform,
    pub stamp: Stamp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceForgotten {
    pub device: DeviceId,
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
pub struct QueryBody {
    pub name: String,
    pub text: Option<String>,
    pub sort: u8,
    pub reading: u8,
    pub rows: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryNote {
    pub key: PlaylistKey,
    pub stamp: Stamp,
    pub body: Option<QueryBody>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaylistFolded {
    pub folded: PlaylistKey,
    pub into: PlaylistKey,
    pub entries: Vec<Recording>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryFolded {
    pub folded: PlaylistKey,
    pub into: PlaylistKey,
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
pub struct AnswerNote {
    pub recording: Recording,
    pub stamp: Stamp,
    pub answer: Answer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Device(DeviceNote),
    Forgotten(DeviceForgotten),
    Playlist(PlaylistNote),
    Query(QueryNote),
    Favourite(FavouriteNote),
    Listen(Listen),
    Handoff(Handoff),
    Answer(AnswerNote),
    PlaylistFolded(PlaylistFolded),
    QueryFolded(QueryFolded),
}

#[derive(Clone, Copy, Debug)]
pub struct Clock {
    last: i64,
}

impl PlaylistKey {
    pub fn minted_at(stamp: Stamp) -> Result<Self> {
        let mut key: [u8; PLAYLIST_KEY_BYTES] = random_bytes()?;
        key[..MINTED_NANOS_FROM].copy_from_slice(&MINTED_KEY_MARK);
        key[MINTED_NANOS_FROM..MINTED_NANOS_TO].copy_from_slice(&stamp.nanos.to_be_bytes());
        Ok(Self(key))
    }

    pub fn minted_nanos(&self) -> Option<i64> {
        if self.0[..MINTED_NANOS_FROM] != MINTED_KEY_MARK {
            return None;
        }
        let mut nanos = [0; 8];
        nanos.copy_from_slice(&self.0[MINTED_NANOS_FROM..MINTED_NANOS_TO]);
        Some(i64::from_be_bytes(nanos)).filter(|nanos| *nanos > 0)
    }

    pub fn minted_after(&self, other: &PlaylistKey) -> bool {
        self.minted_nanos()
            .zip(other.minted_nanos())
            .is_some_and(|(mine, theirs)| mine > theirs)
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
            Self::Forgotten(forgotten) => Some(forgotten.stamp),
            Self::Playlist(note) => Some(note.stamp),
            Self::Query(note) => Some(note.stamp),
            Self::Favourite(note) => Some(note.stamp),
            Self::Handoff(handoff) => Some(handoff.stamp),
            Self::Answer(note) => Some(note.stamp),
            Self::Listen(_) | Self::PlaylistFolded(_) | Self::QueryFolded(_) => None,
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
    fn a_key_minted_at_a_stamp_says_when_and_a_random_one_says_nothing() {
        let device = DeviceId([1; 16]);
        let mut clock = Clock::from_last(0);
        let earlier = PlaylistKey::minted_at(clock.now(device)).expect("minted");
        let later = PlaylistKey::minted_at(clock.now(device)).expect("minted");
        let random = PlaylistKey([7; PLAYLIST_KEY_BYTES]);

        assert!(later.minted_after(&earlier));
        assert!(!earlier.minted_after(&later));
        assert!(earlier < later);
        assert_eq!(random.minted_nanos(), None);
        assert!(!later.minted_after(&random));
        assert!(!random.minted_after(&earlier));
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
