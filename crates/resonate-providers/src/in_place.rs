use std::{
    fmt,
    io::{self, ErrorKind, Read, Seek, SeekFrom},
    time::Duration,
};

use parking_lot::Mutex;
use resonate_core::{MediaLocation, SourceId};

use crate::error::{Error, ProviderOp, Result};

const SONG_KEY: &str = "track/";
const ALBUM_KEY: &str = "album/";
const ARTIST_KEY: &str = "artist/";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(Box<str>);

impl Id {
    pub fn new(id: &str) -> Option<Self> {
        let id = id.trim();
        (!id.is_empty()).then(|| Self(Box::from(id)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Named {
    Song(Id),
    Album(Id),
    Artist(Id),
}

impl Named {
    pub fn read(location: &MediaLocation, source: &SourceId) -> Option<Self> {
        if location.source() != source {
            return None;
        }
        let key = location.locator().as_key()?;
        [
            (SONG_KEY, Self::Song as fn(Id) -> Self),
            (ALBUM_KEY, Self::Album),
            (ARTIST_KEY, Self::Artist),
        ]
        .into_iter()
        .find_map(|(prefix, named)| key.strip_prefix(prefix).and_then(Id::new).map(named))
    }

    pub fn location(&self, source: &SourceId) -> MediaLocation {
        let (prefix, id) = match self {
            Self::Song(id) => (SONG_KEY, id),
            Self::Album(id) => (ALBUM_KEY, id),
            Self::Artist(id) => (ARTIST_KEY, id),
        };
        MediaLocation::new(source.clone(), format!("{prefix}{id}"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artist {
    pub id: Id,
    pub name: String,
    pub albums: Option<u32>,
    pub cover: Option<Id>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Album {
    pub id: Id,
    pub name: String,
    pub artist: Option<String>,
    pub year: Option<u16>,
    pub songs: Option<u32>,
    pub cover: Option<Id>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Song {
    pub id: Id,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub disc: Option<u32>,
    pub track: Option<u32>,
    pub length: Option<Duration>,
    pub suffix: Option<String>,
    pub cover: Option<Id>,
}

impl Song {
    pub fn only(id: Id) -> Self {
        Self {
            id,
            title: String::new(),
            artist: None,
            album: None,
            disc: None,
            track: None,
            length: None,
            suffix: None,
            cover: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtistAlbums {
    pub artist: Artist,
    pub albums: Vec<Album>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlbumSongs {
    pub album: Album,
    pub songs: Vec<Song>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub songs: Vec<Song>,
}

pub trait SeekingBytes: Read + Seek + Send {}

impl<T: Read + Seek + Send> SeekingBytes for T {}

enum Bytes {
    Seeking(Mutex<Box<dyn SeekingBytes>>),
    Onward(Mutex<Box<dyn Read + Send>>),
}

pub struct Streamed {
    bytes: Bytes,
    length: Option<u64>,
    pub media_type: Option<String>,
}

impl Streamed {
    pub fn seeking(
        reader: impl Read + Seek + Send + 'static,
        length: Option<u64>,
        media_type: Option<String>,
    ) -> Self {
        Self {
            bytes: Bytes::Seeking(Mutex::new(Box::new(reader))),
            length,
            media_type,
        }
    }

    pub fn onward(reader: impl Read + Send + 'static, media_type: Option<String>) -> Self {
        Self {
            bytes: Bytes::Onward(Mutex::new(Box::new(reader))),
            length: None,
            media_type,
        }
    }

    pub const fn length(&self) -> Option<u64> {
        self.length
    }

    pub const fn is_seekable(&self) -> bool {
        matches!(self.bytes, Bytes::Seeking(_)) && self.length.is_some()
    }
}

impl Read for Streamed {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match &mut self.bytes {
            Bytes::Seeking(reader) => reader.get_mut().read(buf),
            Bytes::Onward(reader) => reader.get_mut().read(buf),
        }
    }
}

impl Seek for Streamed {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        match &mut self.bytes {
            Bytes::Seeking(reader) => reader.get_mut().seek(to),
            Bytes::Onward(_) => Err(io::Error::from(ErrorKind::Unsupported)),
        }
    }
}

pub trait PlaysInPlace: Send + Sync {
    fn scheme(&self) -> &SourceId;

    fn title(&self) -> &'static str;

    fn artists(&self) -> Result<Vec<Artist>>;

    fn artist(&self, id: &Id) -> Result<ArtistAlbums>;

    fn album(&self, id: &Id) -> Result<AlbumSongs>;

    fn search(&self, words: &str) -> Result<Found>;

    fn streamed(&self, song: &Id) -> Result<Streamed>;

    fn cover(&self, cover: &Id, side: u32) -> Result<Vec<u8>>;

    fn cover_of(&self, named: &Named) -> Result<Id>;

    fn songs_of(&self, named: &Named) -> Result<Vec<Song>> {
        match named {
            Named::Song(id) => Ok(vec![Song::only(id.clone())]),
            Named::Album(id) => Ok(self.album(id)?.songs),
            Named::Artist(id) => {
                let mut songs = Vec::new();
                for album in self.artist(id)?.albums {
                    songs.extend(self.album(&album.id)?.songs);
                }
                Ok(songs)
            }
        }
    }

    fn named_by(&self, location: &MediaLocation) -> Option<Named> {
        Named::read(location, self.scheme())
    }

    fn location_of(&self, named: &Named) -> MediaLocation {
        named.location(self.scheme())
    }
}

pub fn not_offered(provider: &SourceId, op: ProviderOp) -> Error {
    Error::NotOffered {
        provider: provider.clone(),
        op,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn subsonic() -> SourceId {
        SourceId::new("subsonic").expect("a source name")
    }

    #[test]
    fn a_song_album_or_artist_is_named_by_a_location_that_reads_back_as_itself() {
        let id = Id::new("al-7").expect("an id");

        for named in [
            Named::Song(id.clone()),
            Named::Album(id.clone()),
            Named::Artist(id),
        ] {
            let location = named.location(&subsonic());
            let uri = location.to_uri();

            assert_eq!(
                MediaLocation::from_uri(&uri).and_then(|read| Named::read(&read, &subsonic())),
                Some(named)
            );
        }
        assert_eq!(
            Named::read(
                &MediaLocation::from_uri("subsonic:track/42").expect("a uri"),
                &subsonic()
            ),
            Some(Named::Song(Id::new("42").expect("an id")))
        );
        assert_eq!(
            Named::read(
                &MediaLocation::from_uri("tidal:track/42").expect("a uri"),
                &subsonic()
            ),
            None
        );
        assert_eq!(
            Named::read(&MediaLocation::local("/music/track/42"), &subsonic()),
            None
        );
        assert_eq!(
            Named::read(
                &MediaLocation::from_uri("subsonic:playlist/1").expect("a uri"),
                &subsonic()
            ),
            None
        );
    }

    #[test]
    fn a_stream_read_onward_refuses_a_seek_and_says_it_cannot_and_one_of_known_length_seeks() {
        let mut onward = Streamed::onward(Cursor::new(vec![1, 2, 3]), None);
        let mut seeking = Streamed::seeking(Cursor::new(vec![1, 2, 3]), Some(3), None);
        let mut read = [0; 1];

        let refused = onward.seek(SeekFrom::Start(1));
        let landed = seeking.seek(SeekFrom::Start(2)).expect("a seek");
        seeking.read_exact(&mut read).expect("a byte");

        assert!(!onward.is_seekable());
        assert_eq!(
            refused.map_err(|error| error.kind()),
            Err(ErrorKind::Unsupported)
        );
        assert!(seeking.is_seekable());
        assert_eq!(landed, 2);
        assert_eq!(read, [3]);
    }
}
