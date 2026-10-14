use std::{
    io::{Read, Seek},
    path::Path,
    time::Duration,
};

use resonate_core::Recording;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocalPlaylistId {
    pub id: i64,
    pub created: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalPlaylist {
    pub at: LocalPlaylistId,
    pub name: String,
    pub entries: Vec<Option<Recording>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalListen {
    pub id: u64,
    pub recording: Recording,
    pub at: i64,
    pub heard: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteListen {
    pub recording: Recording,
    pub at: i64,
    pub heard: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resumed {
    pub rows: Vec<Recording>,
    pub at: u32,
    pub position: Duration,
    pub playing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    pub recording: Recording,
    pub size: u64,
    pub extension: String,
}

pub trait ReadSeek: Read + Seek + Send {}

impl<T: Read + Seek + Send> ReadSeek for T {}

pub struct Opened {
    pub reader: Box<dyn ReadSeek>,
    pub size: u64,
    pub extension: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Refused {
    Unavailable,
    NotHere,
    Unwritable,
}

pub trait Catalog: Send + Sync + 'static {
    fn playlists(&self) -> Result<Vec<LocalPlaylist>, Refused>;

    fn write_playlist(
        &self,
        at: Option<LocalPlaylistId>,
        name: &str,
        entries: &[Recording],
    ) -> Result<LocalPlaylist, Refused>;

    fn remove_playlist(&self, at: LocalPlaylistId) -> Result<(), Refused>;

    fn favourites(&self) -> Result<Vec<Recording>, Refused>;

    fn favour(&self, recording: &Recording, favoured: bool) -> Result<bool, Refused>;

    fn listens_after(&self, mark: u64, most: usize) -> Result<Vec<LocalListen>, Refused>;

    fn credit(&self, listen: &RemoteListen) -> Result<bool, Refused>;

    fn resumption(&self) -> Result<Option<Resumed>, Refused>;

    fn holds(&self, recording: &Recording) -> Result<bool, Refused>;

    fn holdings(&self) -> Result<Vec<Held>, Refused>;

    fn open(&self, recording: &Recording) -> Result<Option<Opened>, Refused>;

    fn land(&self, recording: &Recording, extension: &str, staged: &Path) -> Result<bool, Refused>;

    fn revision(&self) -> u64;
}
