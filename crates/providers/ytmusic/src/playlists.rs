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
    #[serde(default, deserialize_with = "entries_readable")]
    entries: Vec<Entry>,
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

impl Entry {
    fn length(&self) -> Option<Duration> {
        let seconds = self.duration.as_ref()?.as_f64()?;
        Duration::try_from_secs_f64(seconds).ok()
    }

    fn artist(&self) -> Option<String> {
        let channel = named(self.channel.as_deref()).or(named(self.uploader.as_deref()))?;
        let artist = channel
            .strip_suffix(A_TOPIC_CHANNEL_ENDS)
            .unwrap_or(channel);
        Some(artist.to_owned())
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

fn playlist_id(id: &str) -> Option<Id> {
    let fits = id.len() <= LONGEST_PLAYLIST_ID
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    fits.then(|| Id::new(id)).flatten()
}

impl Tool {
    pub fn playlists(&self) -> Result<Vec<Mix>> {
        let listing = self.listed_at(THE_ACCOUNTS_PLAYLISTS_ARE_AT)?;
        let flat: Flat = serde_json::from_slice(&listing).unwrap_or_else(|error| {
            tracing::debug!(%error, "the account's playlists were not the document expected");
            Flat::default()
        });
        Ok(flat
            .entries
            .into_iter()
            .filter(|entry| !NOT_A_PLAYLIST_OF_SONGS.contains(&entry.id.as_str()))
            .filter_map(Entry::mix)
            .collect())
    }

    pub fn playlist(&self, id: &Id) -> Result<Vec<Song>> {
        let op = ProviderOp::Browse;
        let id = playlist_id(id.as_str()).ok_or_else(|| self.unreadable(op))?;
        let listing = self.listed_at(&format!("{A_PLAYLIST_IS_AT}{id}"))?;
        let flat: Flat = serde_json::from_slice(&listing).map_err(|error| {
            tracing::debug!(%error, "a playlist was not the document expected");
            self.unreadable(op)
        })?;

        Ok(flat.entries.iter().filter_map(Entry::song).collect())
    }
}
