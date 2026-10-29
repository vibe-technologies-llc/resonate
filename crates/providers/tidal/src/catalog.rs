use std::time::Duration;

use resonate_core::SourceId;
use resonate_fetch::escaped;
use resonate_providers::{
    Album, AlbumSongs, Artist, ArtistAlbums, Error, Found, Id, Named, ProviderOp, Result, Song,
};
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use serde_json::Value;

use crate::hifi::{id_spelt, listings_readable};

pub(crate) const BROWSED_AT_MOST: usize = 20;
pub(crate) const SONGS_OF_AN_ALBUM_AT_MOST: usize = 100;
pub(crate) const ALBUMS_OF_AN_ARTIST_AT_MOST: usize = 50;
pub(crate) const FAVOURITES_AT_MOST: usize = 50;
pub(crate) const IMAGES: &str = "https://resources.tidal.com/images/";
const DELIVERED_AS: &str = "flac";
const ALBUM_COVER: &str = "album:";
const ARTIST_PICTURE: &str = "artist:";
const ALBUM_SIDES: [u32; 5] = [80, 160, 320, 640, 1280];
const ARTIST_SIDES: [u32; 4] = [160, 320, 480, 750];
const YEAR_DIGITS: usize = 4;

#[derive(Deserialize)]
#[serde(bound = "T: DeserializeOwned")]
pub(crate) struct Items<T> {
    #[serde(default, deserialize_with = "listings_readable")]
    pub(crate) items: Vec<T>,
}

impl<T> Default for Items<T> {
    fn default() -> Self {
        Self { items: Vec::new() }
    }
}

fn items_or_none<'de, D: Deserializer<'de>, T: DeserializeOwned>(
    deserializer: D,
) -> std::result::Result<Vec<T>, D::Error> {
    Ok(Option::<Value>::deserialize(deserializer)?
        .and_then(|value| serde_json::from_value::<Items<T>>(value).ok())
        .map(|items| items.items)
        .unwrap_or_default())
}

pub(crate) fn user_spelt<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<u64>, D::Error> {
    Ok(match Option::<Value>::deserialize(deserializer)? {
        Some(Value::Number(user)) => user.as_u64(),
        Some(Value::String(user)) => user.trim().parse().ok(),
        _ => None,
    })
}

#[derive(Deserialize)]
#[serde(bound = "T: DeserializeOwned")]
struct Favoured<T> {
    item: T,
}

fn favoured<T>(favoured: Items<Favoured<T>>) -> impl Iterator<Item = T> {
    favoured.items.into_iter().map(|favoured| favoured.item)
}

#[derive(Default, Deserialize)]
pub(crate) struct V1Found {
    #[serde(default, deserialize_with = "items_or_none")]
    artists: Vec<V1Artist>,
    #[serde(default, deserialize_with = "items_or_none")]
    albums: Vec<V1Album>,
    #[serde(default, deserialize_with = "items_or_none")]
    tracks: Vec<V1Track>,
}

impl V1Found {
    pub(crate) fn found(self) -> Found {
        Found {
            artists: self
                .artists
                .into_iter()
                .filter_map(V1Artist::artist)
                .collect(),
            albums: self.albums.into_iter().filter_map(V1Album::album).collect(),
            songs: self.tracks.into_iter().filter_map(V1Track::song).collect(),
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct V1Artist {
    #[serde(deserialize_with = "id_spelt")]
    id: u64,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    picture: Option<String>,
}

impl V1Artist {
    pub(crate) fn artist(self) -> Option<Artist> {
        Some(Artist {
            id: Id::new(&self.id.to_string())?,
            name: self.name.unwrap_or_default(),
            albums: None,
            cover: picture(ARTIST_PICTURE, self.picture.as_deref()),
        })
    }
}

#[derive(Deserialize)]
struct Credited {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct V1Album {
    #[serde(deserialize_with = "id_spelt")]
    id: u64,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    cover: Option<String>,
    #[serde(default, rename = "releaseDate")]
    released: Option<String>,
    #[serde(default, rename = "numberOfTracks")]
    tracks: Option<u32>,
    #[serde(default)]
    artist: Option<Credited>,
}

impl V1Album {
    pub(crate) fn album(self) -> Option<Album> {
        Some(Album {
            id: Id::new(&self.id.to_string())?,
            name: self.title.unwrap_or_default(),
            artist: self.artist.and_then(|artist| artist.name),
            year: self
                .released
                .as_deref()
                .and_then(|date| date.get(..YEAR_DIGITS))
                .and_then(|year| year.parse().ok()),
            songs: self.tracks,
            cover: picture(ALBUM_COVER, self.cover.as_deref()),
        })
    }
}

#[derive(Deserialize)]
struct OnAlbum {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    cover: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct V1Track {
    #[serde(deserialize_with = "id_spelt")]
    id: u64,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    duration: Option<u64>,
    #[serde(default, rename = "trackNumber")]
    track: Option<u32>,
    #[serde(default, rename = "volumeNumber")]
    disc: Option<u32>,
    #[serde(default)]
    artist: Option<Credited>,
    #[serde(default)]
    artists: Option<Vec<Credited>>,
    #[serde(default)]
    album: Option<OnAlbum>,
}

impl V1Track {
    pub(crate) fn song(self) -> Option<Song> {
        let title = self.title.unwrap_or_default();
        let title = match self.version.filter(|version| !version.trim().is_empty()) {
            Some(version) => format!("{title} ({version})"),
            None => title,
        };
        let artist = self.artist.and_then(|artist| artist.name).or_else(|| {
            self.artists
                .and_then(|artists| artists.into_iter().find_map(|artist| artist.name))
        });
        let (album, cover) = self
            .album
            .map(|album| (album.title, picture(ALBUM_COVER, album.cover.as_deref())))
            .unwrap_or_default();
        Some(Song {
            id: Id::new(&self.id.to_string())?,
            title,
            artist,
            album,
            disc: self.disc,
            track: self.track,
            length: self.duration.map(Duration::from_secs),
            suffix: Some(DELIVERED_AS.to_owned()),
            cover,
        })
    }

    pub(crate) fn album_cover(&self) -> Option<Id> {
        picture(
            ALBUM_COVER,
            self.album.as_ref().and_then(|album| album.cover.as_deref()),
        )
    }

    fn album_named(&self) -> Option<String> {
        self.album.as_ref().and_then(|album| album.title.clone())
    }
}

#[derive(Deserialize)]
pub(crate) struct Entry {
    #[serde(default)]
    item: Option<V1Track>,
}

fn is_a_picture(uuid: &str) -> bool {
    !uuid.is_empty()
        && uuid
            .chars()
            .all(|character| character.is_ascii_hexdigit() || character == '-')
}

fn picture(kind: &str, uuid: Option<&str>) -> Option<Id> {
    let uuid = uuid?.trim();
    is_a_picture(uuid)
        .then(|| Id::new(&format!("{kind}{uuid}")))
        .flatten()
}

pub(crate) fn picture_at(images: &str, cover: &Id, side: u32) -> Option<String> {
    let (sides, uuid) = match cover.as_str().split_once(':')? {
        ("album", uuid) => (ALBUM_SIDES.as_slice(), uuid),
        ("artist", uuid) => (ARTIST_SIDES.as_slice(), uuid),
        _ => return None,
    };
    if !is_a_picture(uuid) {
        return None;
    }
    let drawn = sides
        .iter()
        .copied()
        .find(|drawn| *drawn >= side)
        .or_else(|| sides.last().copied())?;
    Some(format!(
        "{images}{}/{drawn}x{drawn}.jpg",
        uuid.replace('-', "/")
    ))
}

pub(crate) fn track_id(provider: &SourceId, op: ProviderOp, id: &Id) -> Result<u64> {
    id.as_str().parse().map_err(|_| Error::Unreadable {
        provider: provider.clone(),
        op,
    })
}

pub(crate) trait AsksV1 {
    fn provider(&self) -> &SourceId;

    fn v1(&self, op: ProviderOp, path: &str) -> Result<Option<Vec<u8>>>;

    fn read<T: DeserializeOwned>(&self, op: ProviderOp, bytes: &[u8]) -> Result<T>;

    fn browsed<T: DeserializeOwned + Default>(&self, path: &str) -> Result<T> {
        let op = ProviderOp::Browse;
        match self.v1(op, path)? {
            Some(bytes) => self.read(op, &bytes),
            None => Ok(T::default()),
        }
    }

    fn searched(&self, words: &str) -> Result<Found> {
        let found: V1Found = self.browsed(&format!(
            "search?query={}&limit={BROWSED_AT_MOST}&offset=0&types=ARTISTS%2CALBUMS%2CTRACKS",
            escaped(words)
        ))?;
        Ok(found.found())
    }

    fn favoured_by(&self, user: u64) -> Result<Found> {
        let asked = |kind: &str| {
            format!(
                "users/{user}/favorites/{kind}?limit={FAVOURITES_AT_MOST}&offset=0&order=DATE&orderDirection=DESC"
            )
        };
        let artists: Items<Favoured<V1Artist>> = self.browsed(&asked("artists"))?;
        let albums: Items<Favoured<V1Album>> = self.browsed(&asked("albums"))?;
        let tracks: Items<Favoured<V1Track>> = self.browsed(&asked("tracks"))?;
        Ok(Found {
            artists: favoured(artists).filter_map(V1Artist::artist).collect(),
            albums: favoured(albums).filter_map(V1Album::album).collect(),
            songs: favoured(tracks).filter_map(V1Track::song).collect(),
        })
    }

    fn artist_albums(&self, id: &Id) -> Result<ArtistAlbums> {
        let artist = track_id(self.provider(), ProviderOp::Browse, id)?;
        let albums: Items<V1Album> = self.browsed(&format!(
            "artists/{artist}/albums?limit={ALBUMS_OF_AN_ARTIST_AT_MOST}&offset=0"
        ))?;
        let albums: Vec<Album> = albums
            .items
            .into_iter()
            .filter_map(V1Album::album)
            .collect();
        Ok(ArtistAlbums {
            artist: Artist {
                id: id.clone(),
                name: albums
                    .iter()
                    .find_map(|album| album.artist.clone())
                    .unwrap_or_default(),
                albums: u32::try_from(albums.len()).ok(),
                cover: None,
            },
            albums,
        })
    }

    fn album_songs(&self, id: &Id) -> Result<AlbumSongs> {
        let album = track_id(self.provider(), ProviderOp::Browse, id)?;
        let tracks: Items<V1Track> = self.browsed(&format!(
            "albums/{album}/tracks?limit={SONGS_OF_AN_ALBUM_AT_MOST}&offset=0"
        ))?;
        Ok(album_of(id, tracks.items))
    }

    fn cover_named(&self, named: &Named) -> Result<Id> {
        let op = ProviderOp::Cover;
        let unheld = || Error::Unreadable {
            provider: self.provider().clone(),
            op,
        };
        match named {
            Named::Song(id) => {
                let track = track_id(self.provider(), op, id)?;
                let bytes = self
                    .v1(op, &format!("tracks/{track}"))?
                    .ok_or_else(unheld)?;
                let track: V1Track = self.read(op, &bytes)?;
                track.album_cover().ok_or_else(unheld)
            }
            Named::Album(id) => {
                let album = track_id(self.provider(), op, id)?;
                let bytes = self
                    .v1(op, &format!("albums/{album}"))?
                    .ok_or_else(unheld)?;
                let album: V1Album = self.read(op, &bytes)?;
                album
                    .album()
                    .and_then(|album| album.cover)
                    .ok_or_else(unheld)
            }
            Named::Artist(id) => {
                let artist = track_id(self.provider(), op, id)?;
                let bytes = self
                    .v1(op, &format!("artists/{artist}"))?
                    .ok_or_else(unheld)?;
                let artist: V1Artist = self.read(op, &bytes)?;
                artist
                    .artist()
                    .and_then(|artist| artist.cover)
                    .ok_or_else(unheld)
            }
        }
    }
}

pub(crate) fn album_of(id: &Id, tracks: Vec<V1Track>) -> AlbumSongs {
    let name = tracks.iter().find_map(V1Track::album_named);
    let cover = tracks.iter().find_map(V1Track::album_cover);
    let songs: Vec<Song> = tracks.into_iter().filter_map(V1Track::song).collect();
    AlbumSongs {
        album: Album {
            id: id.clone(),
            name: name.unwrap_or_default(),
            artist: songs.iter().find_map(|song| song.artist.clone()),
            year: None,
            songs: u32::try_from(songs.len()).ok(),
            cover,
        },
        songs,
    }
}

pub(crate) fn songs_of_entries(entries: Vec<Entry>) -> Vec<V1Track> {
    entries.into_iter().filter_map(|entry| entry.item).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_asked_for_at_the_least_side_tidal_draws_that_covers_the_one_wanted() {
        let cover = Id::new("album:ab12-cd34").expect("a cover");
        let portrait = Id::new("artist:ab12-cd34").expect("a portrait");

        assert_eq!(
            picture_at(IMAGES, &cover, 300).as_deref(),
            Some("https://resources.tidal.com/images/ab12/cd34/320x320.jpg")
        );
        assert_eq!(
            picture_at(IMAGES, &cover, 4000).as_deref(),
            Some("https://resources.tidal.com/images/ab12/cd34/1280x1280.jpg")
        );
        assert_eq!(
            picture_at(IMAGES, &portrait, 500).as_deref(),
            Some("https://resources.tidal.com/images/ab12/cd34/750x750.jpg")
        );
        assert_eq!(
            picture_at(IMAGES, &Id::new("album:../x").expect("a name"), 80),
            None
        );
        assert_eq!(picture(ALBUM_COVER, Some("../etc")), None);
    }

    #[test]
    fn a_v1_search_reads_each_kind_and_passes_over_what_does_not_read() {
        let found: V1Found = serde_json::from_str(
            r#"{"artists":{"items":[{"id":3,"name":"Pink Floyd","picture":"aa-bb"},{"id":null}]},
                "albums":{"items":[{"id":"7","title":"Meddle","releaseDate":"1971-10-30","numberOfTracks":6,"artist":{"name":"Pink Floyd"},"cover":"cc-dd"}]},
                "tracks":null}"#,
        )
        .expect("a search answer");
        let found = found.found();

        assert_eq!(found.artists.len(), 1);
        assert_eq!(
            found.artists[0].cover.as_ref().map(Id::as_str),
            Some("artist:aa-bb")
        );
        assert_eq!(found.albums[0].year, Some(1971));
        assert_eq!(found.albums[0].songs, Some(6));
        assert!(found.songs.is_empty());
    }
}
