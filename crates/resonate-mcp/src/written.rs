use std::time::Duration;

use ahash::AHashMap;
use resonate_core::{AlbumId, Volume};
use resonate_engine::{Asleep, Until};
use resonate_library::{
    Album, Artist, Library, Listened, MissingTrack, Playlist, Suggestion, Track,
};
use resonate_mpris::{Described, utc_stamp};
use serde_json::{Value, json};

use crate::Result;

const MILLIS_A_SECOND: f64 = 1_000.0;
const WHOLE: f64 = 100.0;

pub(crate) fn seconds(span: Duration) -> f64 {
    span.as_millis() as f64 / MILLIS_A_SECOND
}

pub(crate) fn percent(volume: Volume) -> f64 {
    (f64::from(volume.get()) * WHOLE).round()
}

pub(crate) fn compact(value: Value) -> Value {
    match value {
        Value::Object(mut fields) => {
            fields.retain(|_, field| !field.is_null());
            Value::Object(fields)
        }
        other => other,
    }
}

pub(crate) struct AlbumTitles<'l> {
    library: &'l Library,
    held: AHashMap<AlbumId, Option<String>>,
}

impl<'l> AlbumTitles<'l> {
    pub(crate) fn over(library: &'l Library) -> Self {
        Self {
            library,
            held: AHashMap::new(),
        }
    }

    pub(crate) fn track(&mut self, track: &Track) -> Result<Value> {
        let album = match track.album_id {
            Some(id) => self.title_of(id)?,
            None => None,
        };
        Ok(held_track(track, album))
    }

    fn title_of(&mut self, id: AlbumId) -> Result<Option<String>> {
        if let Some(title) = self.held.get(&id) {
            return Ok(title.clone());
        }
        let title = self.library.album(id)?.map(|album| album.title);
        self.held.insert(id, title.clone());
        Ok(title)
    }
}

fn held_track(track: &Track, album: Option<String>) -> Value {
    compact(json!({
        "track_id": track.id.get(),
        "title": track.title,
        "artist": track.artist,
        "album": album,
        "disc": track.disc_number,
        "number": track.track_number,
        "length_seconds": track
            .duration
            .map(|frames| seconds(frames.to_duration(track.spec.rate))),
        "genre": track.genre,
        "plays": track.plays,
        "last_played": track.played.map(utc_stamp),
        "favourite": track.favourite.is_some(),
        "uri": track.location.to_uri_within(track.span),
    }))
}

pub(crate) fn album(album: &Album) -> Value {
    compact(json!({
        "album_id": album.id.get(),
        "title": album.title,
        "artist": album.artist,
        "year": album.year,
        "tracks": album.track_count,
        "missing": album.missing,
        "favourite": album.favourite.is_some(),
    }))
}

pub(crate) fn artist(artist: &Artist) -> Value {
    compact(json!({
        "artist_id": artist.id.get(),
        "name": artist.name,
        "albums": artist.album_count,
        "tracks": artist.track_count,
        "favourite": artist.favourite.is_some(),
    }))
}

pub(crate) fn playlist(playlist: &Playlist) -> Value {
    compact(json!({
        "name": playlist.name,
        "tracks": playlist.entries,
        "length_seconds": playlist.duration.map(seconds),
        "plays": playlist.plays,
        "last_played": playlist.played.map(utc_stamp),
        "fills_from": playlist.query.as_ref().and_then(|query| query.text.clone()),
        "pinned": playlist.pinned.is_some(),
    }))
}

pub(crate) fn listened<Id>(listened: &Listened<Id>) -> Value {
    json!({
        "name": listened.name,
        "plays": listened.plays,
        "listened_seconds": seconds(listened.listened),
    })
}

pub(crate) fn suggestion(suggestion: &Suggestion) -> Value {
    compact(json!({
        "name": suggestion.name,
        "why": suggestion.reason.says(),
        "tracks": suggestion.rows,
        "length_seconds": suggestion.length.map(seconds),
        "search": suggestion.query.text,
    }))
}

pub(crate) fn missing(missing: &MissingTrack) -> Value {
    compact(json!({
        "release_track_id": missing.release_track.get(),
        "title": missing.title,
        "artist": missing.artist,
        "album": missing.album_title,
        "album_artist": missing.owner,
        "disc": missing.disc,
        "number": missing.number.clone().unwrap_or_else(|| missing.position.to_string()),
        "length_seconds": missing.length.map(seconds),
        "wanted": missing.want.is_some(),
    }))
}

pub(crate) fn row(described: &Described) -> Value {
    compact(json!({
        "queue_id": described.track.to_string(),
        "title": described.title,
        "artist": described.artist,
        "album": described.album,
        "length_seconds": described.length.map(seconds),
        "uri": described
            .location
            .as_ref()
            .map(|location| location.to_uri_within(described.span)),
    }))
}

pub(crate) fn asleep(asleep: Asleep) -> Value {
    let until = match asleep.until {
        Until::After(_) => "after_a_while",
        Until::EndOfTrack => "end_of_track",
        Until::EndOfQueue => "end_of_queue",
    };
    compact(json!({
        "until": until,
        "left_seconds": asleep.left.map(seconds),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_nothing_answered_is_left_out_rather_than_written_as_null() {
        let written = compact(json!({ "title": "Echoes", "artist": null, "plays": 0 }));

        assert_eq!(written, json!({ "title": "Echoes", "plays": 0 }));
    }

    #[test]
    fn a_length_reads_in_seconds_to_the_millisecond() {
        assert_eq!(seconds(Duration::from_millis(1_401_234)), 1_401.234);
        assert_eq!(seconds(Duration::from_micros(999)), 0.0);
    }

    #[test]
    fn a_volume_reads_as_a_whole_percentage() {
        let volume = Volume::new(0.456).expect("a volume inside the range");

        assert_eq!(percent(volume), 46.0);
        assert_eq!(percent(Volume::MAX), 100.0);
    }
}
