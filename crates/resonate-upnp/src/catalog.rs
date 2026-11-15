use std::{
    error::Error as StdError,
    fmt,
    io::{Read, Seek},
    time::Duration,
};

use resonate_core::{AlbumId, ArtistId, PlaylistId, TrackId};

const ROOT: &str = "0";

const ARTISTS: &str = "artists";

const ALBUMS: &str = "albums";

const SONGS: &str = "songs";

const PLAYLISTS: &str = "playlists";

const ARTIST: &str = "artist-";

const ALBUM: &str = "album-";

const PLAYLIST: &str = "playlist-";

const SONG: &str = "song-";

const DIGITS_AT_MOST: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shelf {
    Artists,
    Artist(ArtistId),
    Albums,
    Album(AlbumId),
    Songs,
    Playlists,
    Playlist(PlaylistId),
}

impl Shelf {
    pub const TOP: [Self; 4] = [Self::Artists, Self::Albums, Self::Songs, Self::Playlists];

    pub(crate) const fn parent(self) -> Object {
        match self {
            Self::Artists | Self::Albums | Self::Songs | Self::Playlists => Object::Root,
            Self::Artist(_) => Object::Shelf(Self::Artists),
            Self::Album(_) => Object::Shelf(Self::Albums),
            Self::Playlist(_) => Object::Shelf(Self::Playlists),
        }
    }

    pub(crate) const fn is_top(self) -> bool {
        matches!(
            self,
            Self::Artists | Self::Albums | Self::Songs | Self::Playlists
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Object {
    Root,
    Shelf(Shelf),
    Song(TrackId),
}

impl Object {
    pub fn read(id: &str) -> Option<Self> {
        let numbered = |prefix: &str| -> Option<u64> {
            let digits = id.strip_prefix(prefix)?;
            let well_formed = !digits.is_empty()
                && digits.len() <= DIGITS_AT_MOST
                && !digits.starts_with('0')
                && digits.bytes().all(|byte| byte.is_ascii_digit());
            well_formed.then(|| digits.parse().ok()).flatten()
        };

        match id {
            ROOT => Some(Self::Root),
            ARTISTS => Some(Self::Shelf(Shelf::Artists)),
            ALBUMS => Some(Self::Shelf(Shelf::Albums)),
            SONGS => Some(Self::Shelf(Shelf::Songs)),
            PLAYLISTS => Some(Self::Shelf(Shelf::Playlists)),
            _ => {
                if let Some(raw) = numbered(ARTIST) {
                    return ArtistId::new(raw)
                        .ok()
                        .map(|id| Self::Shelf(Shelf::Artist(id)));
                }
                if let Some(raw) = numbered(ALBUM) {
                    return AlbumId::new(raw)
                        .ok()
                        .map(|id| Self::Shelf(Shelf::Album(id)));
                }
                if let Some(raw) = numbered(PLAYLIST) {
                    return PlaylistId::new(raw)
                        .ok()
                        .map(|id| Self::Shelf(Shelf::Playlist(id)));
                }
                numbered(SONG)
                    .and_then(|raw| TrackId::new(raw).ok())
                    .map(Self::Song)
            }
        }
    }
}

impl fmt::Display for Object {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Root => f.write_str(ROOT),
            Self::Shelf(Shelf::Artists) => f.write_str(ARTISTS),
            Self::Shelf(Shelf::Albums) => f.write_str(ALBUMS),
            Self::Shelf(Shelf::Songs) => f.write_str(SONGS),
            Self::Shelf(Shelf::Playlists) => f.write_str(PLAYLISTS),
            Self::Shelf(Shelf::Artist(id)) => write!(f, "{ARTIST}{id}"),
            Self::Shelf(Shelf::Album(id)) => write!(f, "{ALBUM}{id}"),
            Self::Shelf(Shelf::Playlist(id)) => write!(f, "{PLAYLIST}{id}"),
            Self::Song(id) => write!(f, "{SONG}{id}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Page {
    pub from: u32,
    pub most: u32,
}

impl Page {
    pub const fn first(most: u32) -> Self {
        Self { from: 0, most }
    }

    pub const fn counting() -> Self {
        Self::first(0)
    }

    pub const fn until(self) -> u32 {
        self.from.saturating_add(self.most)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    pub shelf: Shelf,
    pub title: String,
    pub artist: Option<String>,
    pub children: u32,
    pub cover: Option<AlbumId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Song {
    pub id: TrackId,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub cover: Option<AlbumId>,
    pub genre: Option<String>,
    pub track_number: Option<u32>,
    pub duration: Option<Duration>,
    pub kind: AudioKind,
    pub size: u64,
    pub rate: u32,
    pub channels: u16,
    pub bits: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Shelf(Held),
    Song(Song),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shelved {
    pub held: Vec<Entry>,
    pub total: u32,
}

pub trait Bytes: Read + Seek + Send {}

impl<T: Read + Seek + Send> Bytes for T {}

pub struct Opened {
    pub bytes: Box<dyn Bytes>,
    pub kind: AudioKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
}

pub trait Catalog: Send + Sync + 'static {
    type Failure: StdError + Send + Sync + 'static;

    fn shelved(&self, shelf: Shelf, page: Page) -> Result<Option<Shelved>, Self::Failure>;

    fn shelf(&self, shelf: Shelf) -> Result<Option<Held>, Self::Failure>;

    fn song(&self, id: TrackId) -> Result<Option<Song>, Self::Failure>;

    fn bytes_of(&self, id: TrackId) -> Result<Option<Opened>, Self::Failure>;

    fn cover(&self, album: AlbumId) -> Result<Option<Picture>, Self::Failure>;

    fn revision(&self) -> u32;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AudioKind {
    Flac,
    Mp3,
    Mp4,
    Aac,
    Ogg,
    Opus,
    Wave,
    Aiff,
    Matroska,
    Caf,
    Dsf,
    Dff,
    WavPack,
    Ape,
}

impl AudioKind {
    pub const ALL: [Self; 14] = [
        Self::Flac,
        Self::Mp3,
        Self::Mp4,
        Self::Aac,
        Self::Ogg,
        Self::Opus,
        Self::Wave,
        Self::Aiff,
        Self::Matroska,
        Self::Caf,
        Self::Dsf,
        Self::Dff,
        Self::WavPack,
        Self::Ape,
    ];

    pub fn of_extension(extension: &str) -> Option<Self> {
        let extension = extension.to_ascii_lowercase();
        Some(match extension.as_str() {
            "flac" => Self::Flac,
            "mp3" => Self::Mp3,
            "m4a" | "m4b" | "mp4" => Self::Mp4,
            "aac" => Self::Aac,
            "ogg" | "oga" => Self::Ogg,
            "opus" => Self::Opus,
            "wav" | "wave" | "rf64" | "w64" => Self::Wave,
            "aif" | "aiff" => Self::Aiff,
            "mka" => Self::Matroska,
            "caf" => Self::Caf,
            "dsf" => Self::Dsf,
            "dff" => Self::Dff,
            "wv" => Self::WavPack,
            "ape" => Self::Ape,
            _ => return None,
        })
    }

    pub const fn mime(self) -> &'static str {
        match self {
            Self::Flac => "audio/flac",
            Self::Mp3 => "audio/mpeg",
            Self::Mp4 => "audio/mp4",
            Self::Aac => "audio/aac",
            Self::Ogg => "audio/ogg",
            Self::Opus => "audio/opus",
            Self::Wave => "audio/wav",
            Self::Aiff => "audio/aiff",
            Self::Matroska => "audio/x-matroska",
            Self::Caf => "audio/x-caf",
            Self::Dsf => "audio/x-dsf",
            Self::Dff => "audio/x-dff",
            Self::WavPack => "audio/x-wavpack",
            Self::Ape => "audio/x-ape",
        }
    }

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Flac => "flac",
            Self::Mp3 => "mp3",
            Self::Mp4 => "m4a",
            Self::Aac => "aac",
            Self::Ogg => "ogg",
            Self::Opus => "opus",
            Self::Wave => "wav",
            Self::Aiff => "aiff",
            Self::Matroska => "mka",
            Self::Caf => "caf",
            Self::Dsf => "dsf",
            Self::Dff => "dff",
            Self::WavPack => "wv",
            Self::Ape => "ape",
        }
    }
}

#[cfg(test)]
mod tests {
    use resonate_core::AUDIO_EXTENSIONS;

    use super::*;

    #[test]
    fn every_extension_a_scan_admits_is_served_under_a_kind() {
        for extension in AUDIO_EXTENSIONS {
            assert!(
                AudioKind::of_extension(extension).is_some(),
                "{extension} names no kind"
            );
        }
        for kind in AudioKind::ALL {
            assert_eq!(AudioKind::of_extension(kind.extension()), Some(kind));
        }
    }

    #[test]
    fn an_object_id_reads_back_as_it_was_written_and_nothing_else_reads() {
        let objects = [
            Object::Root,
            Object::Shelf(Shelf::Artists),
            Object::Shelf(Shelf::Albums),
            Object::Shelf(Shelf::Songs),
            Object::Shelf(Shelf::Playlists),
            Object::Shelf(Shelf::Artist(ArtistId::new(7).expect("an id"))),
            Object::Shelf(Shelf::Album(AlbumId::new(42).expect("an id"))),
            Object::Shelf(Shelf::Playlist(PlaylistId::new(3).expect("an id"))),
            Object::Song(TrackId::new(u64::MAX).expect("an id")),
        ];

        for object in objects {
            assert_eq!(Object::read(&object.to_string()), Some(object));
        }
        for unread in [
            "",
            "1",
            "song-",
            "song-0",
            "song-007",
            "song-+7",
            "song--7",
            "song-7 ",
            "song-18446744073709551616",
            "album-1/../../etc/passwd",
            "/home/listener/music/a.flac",
            "Artists",
        ] {
            assert_eq!(Object::read(unread), None, "{unread:?} read");
        }
    }
}
