use std::time::Duration;

use resonate_providers::{
    Album, AlbumSongs, Artist, ArtistAlbums, Found, Id, Song, rating_in_the_title,
};
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use serde_json::Value;

const NANOSECONDS_A_TICK: u64 = 100;
const CONTAINERS_IN_ORDER_OF_TRUST: [&str; 12] = [
    "flac", "mp3", "m4a", "ogg", "opus", "wav", "aac", "wma", "aiff", "ape", "wv", "dsf",
];

pub(crate) fn readable<'de, D: Deserializer<'de>, T: DeserializeOwned>(
    deserializer: D,
) -> Result<Vec<T>, D::Error> {
    let listed = Option::<Vec<Value>>::deserialize(deserializer)?.unwrap_or_default();
    Ok(listed
        .into_iter()
        .filter_map(|item| {
            serde_json::from_value(item)
                .inspect_err(|error| {
                    tracing::debug!(%error, "a Jellyfin listing that does not read is passed over");
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

fn optional_id_spelt<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
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

fn names<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<String>, D::Error> {
    Ok(match Option::<Value>::deserialize(deserializer)? {
        Some(Value::Array(named)) => named
            .into_iter()
            .filter_map(|name| match name {
                Value::String(name) if !name.trim().is_empty() => Some(name),
                _ => None,
            })
            .collect(),
        Some(Value::String(name)) if !name.trim().is_empty() => vec![name],
        _ => Vec::new(),
    })
}

fn small<T: TryFrom<u64>>(number: Option<u64>) -> Option<T> {
    number.and_then(|number| T::try_from(number).ok())
}

fn suffix_of(container: &str) -> Option<String> {
    let kinds: Vec<String> = container
        .split(',')
        .map(|kind| kind.trim().to_ascii_lowercase())
        .filter(|kind| !kind.is_empty())
        .collect();
    CONTAINERS_IN_ORDER_OF_TRUST
        .iter()
        .find(|trusted| kinds.iter().any(|kind| kind == *trusted))
        .map(|trusted| (*trusted).to_owned())
        .or_else(|| kinds.into_iter().next())
}

#[derive(Default, Deserialize)]
pub(crate) struct ItemsDoc {
    #[serde(default, rename = "Items", deserialize_with = "readable")]
    items: Vec<ItemDoc>,
    #[serde(
        default,
        rename = "TotalRecordCount",
        deserialize_with = "number_spelt"
    )]
    total: Option<u64>,
}

impl ItemsDoc {
    pub(crate) const fn listed(&self) -> usize {
        self.items.len()
    }

    pub(crate) fn total(&self) -> Option<usize> {
        small(self.total)
    }

    pub(crate) fn artists(self) -> Vec<Artist> {
        self.items.into_iter().filter_map(ItemDoc::artist).collect()
    }

    pub(crate) fn albums(self) -> Vec<Album> {
        self.items.into_iter().filter_map(ItemDoc::album).collect()
    }

    pub(crate) fn songs(self) -> Vec<Song> {
        self.items.into_iter().filter_map(ItemDoc::song).collect()
    }
}

#[derive(Deserialize)]
struct UserDataDoc {
    #[serde(default, rename = "IsFavorite")]
    favourite: Option<bool>,
}

#[derive(Deserialize)]
pub(crate) struct ItemDoc {
    #[serde(rename = "Id", deserialize_with = "id_spelt")]
    id: String,
    #[serde(default, rename = "Name")]
    name: Option<String>,
    #[serde(default, rename = "AlbumArtist")]
    album_artist: Option<String>,
    #[serde(default, rename = "Artists", deserialize_with = "names")]
    artists: Vec<String>,
    #[serde(default, rename = "Album")]
    album: Option<String>,
    #[serde(default, rename = "AlbumId", deserialize_with = "optional_id_spelt")]
    album_id: Option<String>,
    #[serde(default, rename = "AlbumPrimaryImageTag")]
    album_image: Option<String>,
    #[serde(default, rename = "ProductionYear", deserialize_with = "number_spelt")]
    year: Option<u64>,
    #[serde(default, rename = "IndexNumber", deserialize_with = "number_spelt")]
    track: Option<u64>,
    #[serde(
        default,
        rename = "ParentIndexNumber",
        deserialize_with = "number_spelt"
    )]
    disc: Option<u64>,
    #[serde(default, rename = "RunTimeTicks", deserialize_with = "number_spelt")]
    ticks: Option<u64>,
    #[serde(default, rename = "Container")]
    container: Option<String>,
    #[serde(default, rename = "ChildCount", deserialize_with = "number_spelt")]
    children: Option<u64>,
    #[serde(default, rename = "AlbumCount", deserialize_with = "number_spelt")]
    albums: Option<u64>,
    #[serde(default, rename = "SongCount", deserialize_with = "number_spelt")]
    songs: Option<u64>,
    #[serde(default, rename = "ImageTags")]
    images: Option<Value>,
    #[serde(default, rename = "UserData")]
    user: Option<UserDataDoc>,
}

impl ItemDoc {
    fn has_a_picture(&self) -> bool {
        self.images
            .as_ref()
            .and_then(|images| images.get("Primary"))
            .is_some_and(|tag| !tag.is_null())
    }

    fn own_cover(&self) -> Option<Id> {
        self.has_a_picture().then(|| Id::new(&self.id)).flatten()
    }

    fn cover(&self) -> Option<Id> {
        self.own_cover().or_else(|| {
            self.album_image
                .as_deref()
                .filter(|tag| !tag.is_empty())
                .and(self.album_id.as_deref())
                .and_then(Id::new)
        })
    }

    fn lead_artist(&self) -> Option<String> {
        if self.artists.is_empty() {
            return self.album_artist.clone().filter(|name| !name.is_empty());
        }
        Some(self.artists.join(", "))
    }

    pub(crate) fn artist(self) -> Option<Artist> {
        Some(Artist {
            cover: self.own_cover(),
            id: Id::new(&self.id)?,
            name: self.name.unwrap_or_default(),
            albums: small(self.albums),
        })
    }

    pub(crate) fn album(self) -> Option<Album> {
        Some(Album {
            cover: self.own_cover(),
            id: Id::new(&self.id)?,
            rating: rating_in_the_title(self.name.as_deref().unwrap_or_default()),
            name: self.name.unwrap_or_default(),
            artist: self
                .album_artist
                .clone()
                .filter(|name| !name.is_empty())
                .or_else(|| self.artists.first().cloned()),
            year: small(self.year),
            songs: small(self.children.or(self.songs)),
        })
    }

    pub(crate) fn song(self) -> Option<Song> {
        Some(Song {
            cover: self.cover(),
            artist: self.lead_artist(),
            id: Id::new(&self.id)?,
            rating: rating_in_the_title(self.name.as_deref().unwrap_or_default()),
            title: self.name.unwrap_or_default(),
            album: self.album,
            disc: small(self.disc),
            track: small(self.track),
            length: self
                .ticks
                .map(|ticks| Duration::from_nanos(ticks.saturating_mul(NANOSECONDS_A_TICK))),
            suffix: self.container.as_deref().and_then(suffix_of),
            favourite: self.user.and_then(|user| user.favourite),
        })
    }

    pub(crate) fn with_albums(self, albums: Vec<Album>) -> Option<ArtistAlbums> {
        Some(ArtistAlbums {
            artist: self.artist()?,
            albums,
        })
    }

    pub(crate) fn with_songs(self, songs: Vec<Song>) -> Option<AlbumSongs> {
        Some(AlbumSongs {
            album: self.album()?,
            songs,
        })
    }
}

pub(crate) fn found(artists: ItemsDoc, albums: ItemsDoc, songs: ItemsDoc) -> Found {
    Found {
        artists: artists.artists(),
        albums: albums.albums(),
        songs: songs.songs(),
        mixes: Vec::new(),
    }
}

#[derive(Deserialize)]
pub(crate) struct Authenticated {
    #[serde(rename = "AccessToken", deserialize_with = "id_spelt")]
    pub(crate) token: String,
    #[serde(rename = "User")]
    pub(crate) user: Account,
}

#[derive(Deserialize)]
pub(crate) struct Account {
    #[serde(rename = "Id", deserialize_with = "id_spelt")]
    pub(crate) id: String,
    #[serde(rename = "Name", default)]
    pub(crate) name: String,
}

#[derive(Deserialize)]
pub(crate) struct Initiated {
    #[serde(rename = "Secret")]
    pub(crate) secret: String,
    #[serde(rename = "Code")]
    pub(crate) code: String,
}

#[derive(Deserialize)]
pub(crate) struct Polled {
    #[serde(rename = "Authenticated", default)]
    pub(crate) authenticated: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_numeric_id_a_number_spelt_as_text_and_an_odd_listing_are_read_and_passed_over() {
        let items: ItemsDoc = serde_json::from_str(
            r#"{"Items":[{"Id":3,"Name":"Pink Floyd","AlbumCount":"2","ImageTags":{"Primary":"x"}},
                         {"Id":null},
                         {"Id":"a1","Name":"Meddle","ProductionYear":1971,"ChildCount":6}],
                "TotalRecordCount":3}"#,
        )
        .expect("an items answer");

        assert_eq!(items.listed(), 2);
        assert_eq!(items.total(), Some(3));

        let mut listed = items.items.into_iter();
        let artist = listed.next().and_then(ItemDoc::artist).expect("an artist");
        let album = listed.next().and_then(ItemDoc::album).expect("an album");

        assert_eq!(artist.id.as_str(), "3");
        assert_eq!(artist.albums, Some(2));
        assert_eq!(artist.cover.as_ref().map(Id::as_str), Some("3"));
        assert_eq!(album.name, "Meddle");
        assert_eq!(album.year, Some(1971));
        assert_eq!(album.songs, Some(6));
        assert_eq!(album.cover, None);
    }

    #[test]
    fn a_song_is_read_with_its_ticks_container_artists_cover_and_favourite_mark() {
        let song: ItemDoc = serde_json::from_str(
            r#"{"Id":"s1","Name":"Echoes","Artists":["Pink Floyd","Guest"],"AlbumArtist":"Pink Floyd",
                "Album":"Meddle","AlbumId":"al1","AlbumPrimaryImageTag":"t","IndexNumber":6,
                "ParentIndexNumber":1,"RunTimeTicks":14120000000,
                "Container":"mov,mp4,m4a,3gp,3g2,mj2","UserData":{"IsFavorite":true}}"#,
        )
        .expect("a song");
        let song = song.song().expect("a readable song");

        assert_eq!(song.length, Some(Duration::from_secs(1412)));
        assert_eq!(song.suffix.as_deref(), Some("m4a"));
        assert_eq!(song.artist.as_deref(), Some("Pink Floyd, Guest"));
        assert_eq!(song.cover.as_ref().map(Id::as_str), Some("al1"));
        assert_eq!((song.disc, song.track), (Some(1), Some(6)));
        assert_eq!(song.favourite, Some(true));
    }

    #[test]
    fn a_song_with_no_user_data_has_no_known_favourite_mark() {
        let song: ItemDoc = serde_json::from_str(r#"{"Id":"s1","Name":"Echoes"}"#).expect("a song");

        assert_eq!(song.song().and_then(|song| song.favourite), None);
    }

    #[test]
    fn a_container_none_of_the_trusted_names_is_taken_as_its_first() {
        assert_eq!(suffix_of("matroska,webm").as_deref(), Some("matroska"));
        assert_eq!(suffix_of("FLAC").as_deref(), Some("flac"));
        assert_eq!(suffix_of(" "), None);
    }
}
