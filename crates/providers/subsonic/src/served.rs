use std::time::Duration;

use resonate_providers::{Album, AlbumSongs, Artist, ArtistAlbums, Found, Id, Song};
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use serde_json::Value;

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

fn cover_spelt<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    Ok(match Option::<Value>::deserialize(deserializer)? {
        Some(Value::String(id)) => Some(id),
        Some(Value::Number(id)) => Some(id.to_string()),
        _ => None,
    })
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
    #[serde(default, rename = "coverArt", deserialize_with = "cover_spelt")]
    cover: Option<String>,
    #[serde(default, deserialize_with = "readable")]
    album: Vec<AlbumDoc>,
}

impl ArtistDoc {
    pub(crate) fn artist(self) -> Option<Artist> {
        Some(Artist {
            id: Id::new(&self.id)?,
            name: self.name.unwrap_or_default(),
            albums: small(self.albums),
            cover: self.cover.as_deref().and_then(Id::new),
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
    #[serde(default, rename = "coverArt", deserialize_with = "cover_spelt")]
    cover: Option<String>,
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
            cover: self.cover.as_deref().and_then(Id::new),
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
    #[serde(default, rename = "coverArt", deserialize_with = "cover_spelt")]
    cover: Option<String>,
    #[serde(default)]
    starred: Option<Value>,
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
            cover: self.cover.as_deref().and_then(Id::new),
            favourite: Some(self.starred.is_some_and(|starred| !starred.is_null())),
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
            mixes: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
