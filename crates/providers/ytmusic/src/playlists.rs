use std::time::Duration;

use resonate_providers::{Id, Mix, ProviderOp, Rating, Result, Song};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::{Tool, VideoId};

pub(crate) const LIKED_SONGS_ID: &str = "LM";
pub(crate) const LIKED_SONGS: &str = "Liked songs";
const THE_ACCOUNTS_PLAYLISTS_ARE_AT: &str = "https://www.youtube.com/feed/playlists";
const A_PLAYLIST_IS_AT: &str = "https://music.youtube.com/playlist?list=";
const NOT_A_PLAYLIST_OF_SONGS: [&str; 3] = [LIKED_SONGS_ID, "LL", "WL"];
const A_TOPIC_CHANNEL_ENDS: &str = " - Topic";
const LONGEST_PLAYLIST_ID: usize = 128;

fn entries_readable<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Vec<Entry>, D::Error> {
    let listed = Option::<Vec<Value>>::deserialize(deserializer)?.unwrap_or_default();
    Ok(listed
        .into_iter()
        .filter_map(|entry| {
            serde_json::from_value(entry)
                .inspect_err(|error| {
                    tracing::debug!(%error, "an entry that does not read is passed over");
                })
                .ok()
        })
        .collect())
}

#[derive(Default, Deserialize)]
struct Flat {
    #[serde(default)]
    title: Option<String>,
    #[serde(default, deserialize_with = "entries_readable")]
    entries: Vec<Entry>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Playlist {
    pub name: Option<String>,
    pub songs: Vec<Song>,
}

#[derive(Deserialize)]
struct Video {
    #[serde(default)]
    track: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    artist: Option<String>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    album: Option<String>,
    #[serde(default)]
    duration: Option<Value>,
    #[serde(default)]
    thumbnails: Option<Value>,
}

#[derive(Deserialize)]
struct Entry {
    id: String,
    title: String,
    #[serde(default)]
    duration: Option<Value>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    uploader: Option<String>,
    #[serde(default)]
    thumbnails: Option<Value>,
}

fn named(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|text| !text.is_empty())
}

fn length_of(duration: Option<&Value>) -> Option<Duration> {
    Duration::try_from_secs_f64(duration?.as_f64()?).ok()
}

fn widest_picture_read(thumbnails: Option<&Value>) -> Option<Id> {
    thumbnails?
        .as_array()?
        .iter()
        .filter_map(|thumbnail| {
            let address = thumbnail.get("url")?.as_str()?;
            let width = thumbnail.get("width").and_then(Value::as_u64).unwrap_or(0);
            (!is_webp(address)).then_some((width, address))
        })
        .max_by_key(|(width, _)| *width)
        .and_then(|(_, address)| Id::new(address))
}

fn is_webp(address: &str) -> bool {
    let path = address.split(['?', '#']).next().unwrap_or(address);
    path.ends_with(".webp") || path.contains("/vi_webp/")
}

fn artist_of(channel: &str) -> String {
    channel
        .strip_suffix(A_TOPIC_CHANNEL_ENDS)
        .unwrap_or(channel)
        .to_owned()
}

impl Video {
    fn song(&self, video: &VideoId) -> Option<Song> {
        let title = named(self.track.as_deref())
            .or(named(self.title.as_deref()))?
            .to_owned();
        Some(Song {
            rating: Rating::of(None, &title, None),
            artist: named(self.artist.as_deref())
                .map(str::to_owned)
                .or_else(|| named(self.channel.as_deref()).map(artist_of)),
            album: named(self.album.as_deref()).map(str::to_owned),
            length: length_of(self.duration.as_ref()),
            cover: widest_picture_read(self.thumbnails.as_ref()),
            title,
            ..Song::only(Id::new(video.as_str())?)
        })
    }
}

impl Entry {
    fn length(&self) -> Option<Duration> {
        length_of(self.duration.as_ref())
    }

    fn artist(&self) -> Option<String> {
        named(self.channel.as_deref())
            .or(named(self.uploader.as_deref()))
            .map(artist_of)
    }

    fn thumbnail(&self) -> Option<Id> {
        let largest = self.thumbnails.as_ref()?.as_array()?.last()?;
        Id::new(largest.get("url")?.as_str()?)
    }

    fn song(&self) -> Option<Song> {
        Some(Song {
            title: self.title.clone(),
            artist: self.artist(),
            length: self.length(),
            cover: self.thumbnail(),
            rating: Rating::of(None, &self.title, None),
            ..Song::only(Id::new(VideoId::read(&self.id)?.as_str())?)
        })
    }

    fn mix(self) -> Option<Mix> {
        Some(Mix {
            id: playlist_id(&self.id)?,
            name: named(Some(&self.title))?.to_owned(),
            subtitle: None,
        })
    }
}

pub fn playlist_id(id: &str) -> Option<Id> {
    let fits = id.len() <= LONGEST_PLAYLIST_ID
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    fits.then(|| Id::new(id)).flatten()
}

impl Tool {
    pub fn playlists(&self) -> Result<Vec<Mix>> {
        let op = ProviderOp::Browse;
        self.refused_while_lapsed(op)?;
        let listing = self.listed_at(THE_ACCOUNTS_PLAYLISTS_ARE_AT)?;
        let flat: Flat = serde_json::from_slice(&listing).map_err(|error| {
            tracing::debug!(%error, "the account's playlists were not the document expected");
            self.unreadable(op)
        })?;
        Ok(flat
            .entries
            .into_iter()
            .filter(|entry| !NOT_A_PLAYLIST_OF_SONGS.contains(&entry.id.as_str()))
            .filter_map(Entry::mix)
            .collect())
    }

    pub fn song(&self, video: &VideoId) -> Result<Option<Song>> {
        let op = ProviderOp::Search;
        self.refused_while_lapsed(op)?;
        let Some(listing) = self.video_listed(video)? else {
            return Ok(None);
        };
        let read: Video = serde_json::from_slice(&listing).map_err(|error| {
            tracing::debug!(%error, "a video was not the document expected");
            self.unreadable(op)
        })?;
        read.song(video)
            .map(Some)
            .ok_or_else(|| self.unreadable(op))
    }

    pub fn playlist(&self, id: &Id) -> Result<Playlist> {
        let op = ProviderOp::Browse;
        let id = playlist_id(id.as_str()).ok_or_else(|| self.unreadable(op))?;
        self.refused_while_lapsed(op)?;
        let listing = self.listed_at(&format!("{A_PLAYLIST_IS_AT}{id}"))?;
        let flat: Flat = serde_json::from_slice(&listing).map_err(|error| {
            tracing::debug!(%error, "a playlist was not the document expected");
            self.unreadable(op)
        })?;

        Ok(Playlist {
            name: named(flat.title.as_deref()).map(str::to_owned),
            songs: flat.entries.iter().filter_map(Entry::song).collect(),
        })
    }
}
