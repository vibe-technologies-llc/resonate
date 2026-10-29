use std::{sync::Arc, time::Duration};

use resonate_core::MediaLocation;
use resonate_providers::Result;

use crate::settings::Online;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ServedCover(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServedArtist {
    pub location: MediaLocation,
    pub name: String,
    pub albums: Option<u32>,
    pub cover: Option<ServedCover>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServedAlbum {
    pub location: MediaLocation,
    pub name: String,
    pub artist: Option<String>,
    pub year: Option<u16>,
    pub songs: Option<u32>,
    pub cover: Option<ServedCover>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServedSong {
    pub location: MediaLocation,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub track: Option<u32>,
    pub length: Option<Duration>,
    pub format: Option<String>,
    pub cover: Option<ServedCover>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServedFound {
    pub artists: Vec<ServedArtist>,
    pub albums: Vec<ServedAlbum>,
    pub songs: Vec<ServedSong>,
}

pub trait Serves: Send + Sync {
    fn names(&self, location: &MediaLocation) -> bool;

    fn artists(&self) -> Result<Vec<ServedArtist>>;

    fn albums_of(&self, artist: &MediaLocation) -> Result<Vec<ServedAlbum>>;

    fn songs_of(&self, album: &MediaLocation) -> Result<Vec<ServedSong>>;

    fn search(&self, words: &str) -> Result<ServedFound>;

    fn expand(&self, named: &MediaLocation) -> Result<Vec<MediaLocation>>;

    fn cover(&self, of: &MediaLocation, known: Option<&ServedCover>, side: u32) -> Result<Vec<u8>>;
}

type Opening = dyn Fn(&Online) -> Option<Arc<dyn Serves>> + Send + Sync;

#[derive(Clone)]
pub struct Serving {
    opens: Arc<Opening>,
}

impl Serving {
    pub fn new(opens: impl Fn(&Online) -> Option<Arc<dyn Serves>> + Send + Sync + 'static) -> Self {
        Self {
            opens: Arc::new(opens),
        }
    }

    pub fn none() -> Self {
        Self::new(|_| None)
    }

    pub fn opened(&self, online: &Online) -> Option<Arc<dyn Serves>> {
        (self.opens)(online)
    }
}

impl Default for Serving {
    fn default() -> Self {
        Self::none()
    }
}
