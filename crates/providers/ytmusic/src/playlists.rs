use std::time::Duration;

use resonate_providers::{Id, Mix, Pacing, ProviderOp, Rating, Result, Song};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::{ASKED_APART, Tool, VideoId};

pub(crate) const LIKED_SONGS_ID: &str = "LM";
pub(crate) const LIKED_SONGS: &str = "Liked songs";
const THE_ACCOUNTS_PLAYLISTS_ARE_AT: &str = "https://www.youtube.com/feed/playlists";
const A_PLAYLIST_IS_AT: &str = "https://music.youtube.com/playlist?list=";
const NOT_A_PLAYLIST_OF_SONGS: [&str; 3] = [LIKED_SONGS_ID, "LL", "WL"];
const A_TOPIC_CHANNEL_ENDS: &str = " - Topic";
const LONGEST_PLAYLIST_ID: usize = 128;
const ASKED_ALONE_AT_MOST: usize = 25;

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
    album: Option<String>,
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

    fn is_short_of_a_field(&self) -> bool {
        self.length().is_none() || self.artist().is_none()
    }

    fn song(&self) -> Option<Song> {
        Some(Song {
            title: self.title.clone(),
            artist: self.artist(),
            album: named(self.album.as_deref()).map(str::to_owned),
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

    fn filled_from(self, whole: Self) -> Self {
        let duration = if self.length().is_some() {
            self.duration
        } else {
            whole.duration
        };
        Self {
            duration,
            channel: named(self.channel.as_deref())
                .map(str::to_owned)
                .or(whole.channel),
            uploader: named(self.uploader.as_deref())
                .map(str::to_owned)
                .or(whole.uploader),
            album: self.album.or(whole.album),
            thumbnails: self.thumbnails.or(whole.thumbnails),
            ..self
        }
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

        let pacing = Pacing::new(ASKED_APART);
        let mut asked_alone = 0;
        Ok(flat
            .entries
            .into_iter()
            .map(|entry| {
                if !entry.is_short_of_a_field() || asked_alone == ASKED_ALONE_AT_MOST {
                    return entry;
                }
                asked_alone += 1;
                self.filled(entry, &pacing)
            })
            .filter_map(|entry| entry.song())
            .collect())
    }

    fn filled(&self, entry: Entry, pacing: &Pacing) -> Entry {
        let Some(video) = VideoId::read(&entry.id) else {
            return entry;
        };
        pacing.paced();
        let whole = self.listed_song(&video).and_then(|listing| {
            serde_json::from_slice::<Entry>(&listing).map_err(|error| {
                tracing::debug!(%error, "a song's listing was not the document expected");
                self.unreadable(ProviderOp::Browse)
            })
        });
        match whole {
            Ok(whole) => entry.filled_from(whole),
            Err(error) => {
                tracing::debug!(%error, video = video.as_str(), "a song's own listing was not read");
                entry
            }
        }
    }
}
