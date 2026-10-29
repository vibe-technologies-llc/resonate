use std::{fmt, io, time::Duration};

use resonate_core::{MediaLocation, SourceId};
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use serde_json::Value;

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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Album {
    pub id: Id,
    pub name: String,
    pub artist: Option<String>,
    pub year: Option<u16>,
    pub songs: Option<u32>,
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

pub struct Streamed {
    pub(crate) reader: resonate_fetch::Seeking,
    pub media_type: Option<String>,
}

impl Streamed {
    pub const fn length(&self) -> Option<u64> {
        self.reader.length()
    }
}

impl io::Read for Streamed {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reader.read(buf)
    }
}

impl io::Seek for Streamed {
    fn seek(&mut self, to: io::SeekFrom) -> io::Result<u64> {
        self.reader.seek(to)
    }
}

pub(crate) fn readable<'de, D: Deserializer<'de>, T: DeserializeOwned>(
    deserializer: D,
) -> Result<Vec<T>, D::Error> {
    let listed = Option::<Vec<Value>>::deserialize(deserializer)?.unwrap_or_default();
    Ok(listed
        .into_iter()
        .filter_map(|item| {
            serde_json::from_value(item)
                .inspect_err(|error| {
                    tracing::debug!(%error, "a Subsonic listing that does not read is passed over");
                })
                .ok()
        })
        .collect())
}

pub(crate) fn id_spelt<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    match Value::deserialize(deserializer)? {
        Value::String(id) => Ok(id),
        Value::Number(id) => Ok(id.to_string()),
        _ => Err(serde::de::Error::custom("an id is a string or a number")),
    }
}

fn number_spelt<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u64>, D::Error> {
    Ok(match Option::<Value>::deserialize(deserializer)? {
        Some(Value::Number(number)) => number.as_u64(),
        Some(Value::String(number)) => number.trim().parse().ok(),
        _ => None,
    })
}

fn small<T: TryFrom<u64>>(number: Option<u64>) -> Option<T> {
    number.and_then(|number| T::try_from(number).ok())
}

#[derive(Default, Deserialize)]
pub(crate) struct ArtistsDoc {
    #[serde(default, deserialize_with = "readable")]
    index: Vec<IndexDoc>,
}

impl ArtistsDoc {
    pub(crate) fn artists(self) -> Vec<Artist> {
        self.index
            .into_iter()
            .flat_map(|index| index.artist)
            .filter_map(ArtistDoc::artist)
            .collect()
    }
}

#[derive(Deserialize)]
struct IndexDoc {
    #[serde(default, deserialize_with = "readable")]
    artist: Vec<ArtistDoc>,
}

#[derive(Deserialize)]
pub(crate) struct ArtistDoc {
    #[serde(deserialize_with = "id_spelt")]
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default, rename = "albumCount", deserialize_with = "number_spelt")]
    albums: Option<u64>,
    #[serde(default, deserialize_with = "readable")]
    album: Vec<AlbumDoc>,
}

impl ArtistDoc {
    pub(crate) fn artist(self) -> Option<Artist> {
        Some(Artist {
            id: Id::new(&self.id)?,
            name: self.name.unwrap_or_default(),
            albums: small(self.albums),
        })
    }

    pub(crate) fn with_albums(mut self) -> Option<ArtistAlbums> {
        let albums = std::mem::take(&mut self.album)
            .into_iter()
            .filter_map(AlbumDoc::album)
            .collect();
        Some(ArtistAlbums {
            artist: self.artist()?,
            albums,
        })
    }
}

#[derive(Deserialize)]
pub(crate) struct AlbumDoc {
    #[serde(deserialize_with = "id_spelt")]
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    artist: Option<String>,
    #[serde(default, deserialize_with = "number_spelt")]
    year: Option<u64>,
    #[serde(default, rename = "songCount", deserialize_with = "number_spelt")]
    songs: Option<u64>,
    #[serde(default, deserialize_with = "readable")]
    song: Vec<SongDoc>,
}

impl AlbumDoc {
    pub(crate) fn album(self) -> Option<Album> {
        Some(Album {
            id: Id::new(&self.id)?,
            name: self.name.or(self.title).unwrap_or_default(),
            artist: self.artist,
            year: small(self.year),
            songs: small(self.songs),
        })
    }

    pub(crate) fn with_songs(mut self) -> Option<AlbumSongs> {
        let songs = std::mem::take(&mut self.song)
            .into_iter()
            .filter_map(SongDoc::song)
            .collect();
        Some(AlbumSongs {
            album: self.album()?,
            songs,
        })
    }
}

#[derive(Deserialize)]
pub(crate) struct SongDoc {
    #[serde(deserialize_with = "id_spelt")]
    id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    artist: Option<String>,
    #[serde(default)]
    album: Option<String>,
    #[serde(default, rename = "discNumber", deserialize_with = "number_spelt")]
    disc: Option<u64>,
    #[serde(default, deserialize_with = "number_spelt")]
    track: Option<u64>,
    #[serde(default, deserialize_with = "number_spelt")]
    duration: Option<u64>,
    #[serde(default)]
    suffix: Option<String>,
}

impl SongDoc {
    pub(crate) fn song(self) -> Option<Song> {
        Some(Song {
            id: Id::new(&self.id)?,
            title: self.title.unwrap_or_default(),
            artist: self.artist,
            album: self.album,
            disc: small(self.disc),
            track: small(self.track),
            length: self.duration.map(Duration::from_secs),
            suffix: self.suffix,
        })
    }
}

#[derive(Default, Deserialize)]
pub(crate) struct FoundDoc {
    #[serde(default, deserialize_with = "readable")]
    artist: Vec<ArtistDoc>,
    #[serde(default, deserialize_with = "readable")]
    album: Vec<AlbumDoc>,
    #[serde(default, deserialize_with = "readable")]
    song: Vec<SongDoc>,
}

impl FoundDoc {
    pub(crate) fn found(self) -> Found {
        Found {
            artists: self
                .artist
                .into_iter()
                .filter_map(ArtistDoc::artist)
                .collect(),
            albums: self.album.into_iter().filter_map(AlbumDoc::album).collect(),
            songs: self.song.into_iter().filter_map(SongDoc::song).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
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
    fn a_numeric_id_and_a_number_spelt_as_text_are_read_and_an_odd_listing_passed_over() {
        let found: FoundDoc = serde_json::from_str(
            r#"{"artist":[{"id":3,"name":"Pink Floyd","albumCount":"2"},{"id":null}],
                "album":[{"id":"a1","title":"Meddle","year":1971}],
                "song":null}"#,
        )
        .expect("a search answer");
        let found = found.found();

        assert_eq!(found.artists.len(), 1);
        assert_eq!(found.artists[0].id.as_str(), "3");
        assert_eq!(found.artists[0].albums, Some(2));
        assert_eq!(found.albums[0].name, "Meddle");
        assert_eq!(found.albums[0].year, Some(1971));
        assert!(found.songs.is_empty());
    }
}
