use resonate_core::TrackId;
use resonate_library::{
    AlbumOrder, AlbumQuery, ArtistOrder, ArtistQuery, Direction, Library, PlaylistName,
    PlaylistOrder, SortOrder, TrackQuery, Window,
};
use resonate_mpris::utc_stamp;
use serde_json::{Value, json};

use crate::{
    Error, Result,
    controlling::Row,
    written::{self, AlbumTitles, compact, seconds},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Wanted {
    Tracks(Vec<TrackId>),
    Matching { query: String, most: usize },
}

pub(crate) fn wanted_rows(library: &Library, wanted: &Wanted) -> Result<Vec<Row>> {
    match wanted {
        Wanted::Tracks(ids) => ids
            .iter()
            .map(|id| {
                library
                    .track(*id)?
                    .map(|track| (track.location, track.span))
                    .ok_or(Error::NoSuchTrack { track: *id })
            })
            .collect(),
        Wanted::Matching { query, most } => {
            let sort = SortOrder::AlbumThenTrack;
            let matched = library.tracks(&TrackQuery {
                text: Some(query.clone()),
                sort,
                reading: sort.reads(),
                limit: Some(*most),
                ..TrackQuery::default()
            })?;
            Ok(matched
                .into_iter()
                .map(|track| (track.location, track.span))
                .collect())
        }
    }
}

pub(crate) fn playlist_called(library: &Library, name: &str) -> Result<resonate_library::Playlist> {
    library
        .playlist_named(name)?
        .ok_or_else(|| Error::NoSuchPlaylist(PlaylistName::new(name)))
}

pub(crate) fn search(library: &Library, query: &str, most: usize) -> Result<Value> {
    let found = library.search(query, most)?;
    let mut titles = AlbumTitles::over(library);
    let tracks = found
        .tracks
        .iter()
        .map(|track| titles.track(track))
        .collect::<Result<Vec<_>>>()?;
    let nothing = found.tracks.is_empty() && found.albums.is_empty() && found.artists.is_empty();
    let did_you_mean = if nothing {
        library.did_you_mean(query)?
    } else {
        None
    };

    Ok(compact(json!({
        "tracks": tracks,
        "albums": found.albums.iter().map(written::album).collect::<Vec<_>>(),
        "artists": found.artists.iter().map(written::artist).collect::<Vec<_>>(),
        "did_you_mean": did_you_mean,
    })))
}

pub(crate) fn playlists(library: &Library, named: Option<&str>) -> Result<Value> {
    let held = library.playlists(PlaylistOrder::Name, Direction::Ascending, named)?;

    Ok(json!({
        "playlists": held.iter().map(written::playlist).collect::<Vec<_>>(),
    }))
}

pub(crate) fn playlist_tracks(
    library: &Library,
    name: &str,
    matching: Option<&str>,
    most: usize,
) -> Result<Value> {
    let found = playlist_called(library, name)?;
    let entries = library.playlist_entries(found.id, matching)?;
    let mut titles = AlbumTitles::over(library);
    let rows = entries
        .iter()
        .take(most)
        .map(|entry| {
            let mut row = match &entry.track {
                Some(track) => titles.track(track)?,
                None => json!({ "uri": entry.cut.location.to_uri_within(entry.cut.span) }),
            };
            if let Value::Object(fields) = &mut row {
                fields.insert("row".to_owned(), Value::from(entry.position));
            }
            Ok(row)
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(json!({
        "playlist": written::playlist(&found),
        "matched": entries.len(),
        "tracks": rows,
    }))
}

pub(crate) fn favourites(library: &Library, most: usize) -> Result<Value> {
    let tracks = library.favourite_tracks(&TrackQuery {
        sort: SortOrder::Favourited,
        reading: SortOrder::Favourited.reads(),
        limit: Some(most),
        ..TrackQuery::default()
    })?;
    let albums = library.favourite_albums(&AlbumQuery {
        sort: AlbumOrder::Favourited,
        reading: AlbumOrder::Favourited.reads(),
        limit: Some(most),
        ..AlbumQuery::default()
    })?;
    let artists = library.favourite_artists(&ArtistQuery {
        sort: ArtistOrder::Favourited,
        reading: ArtistOrder::Favourited.reads(),
        limit: Some(most),
        ..ArtistQuery::default()
    })?;
    let mut titles = AlbumTitles::over(library);

    Ok(json!({
        "tracks": tracks
            .iter()
            .map(|track| titles.track(track))
            .collect::<Result<Vec<_>>>()?,
        "albums": albums.iter().map(written::album).collect::<Vec<_>>(),
        "artists": artists.iter().map(written::artist).collect::<Vec<_>>(),
    }))
}

pub(crate) fn statistics(library: &Library, window: Window, top: usize) -> Result<Value> {
    let counts = library.statistics(window)?;
    let most = library.most_listened(window, top)?;

    Ok(compact(json!({
        "window": window.name(),
        "since": window.since().map(utc_stamp),
        "plays": counts.plays,
        "listened_seconds": seconds(counts.listened),
        "tracks_heard": counts.tracks,
        "albums_heard": counts.albums,
        "artists_heard": counts.artists,
        "tracks": most.tracks.iter().map(written::listened).collect::<Vec<_>>(),
        "albums": most.albums.iter().map(written::listened).collect::<Vec<_>>(),
        "artists": most.artists.iter().map(written::listened).collect::<Vec<_>>(),
    })))
}

pub(crate) fn suggestions(library: &Library) -> Result<Value> {
    let suggested = library.suggestions()?;

    Ok(json!({
        "suggestions": suggested.iter().map(written::suggestion).collect::<Vec<_>>(),
    }))
}

pub(crate) fn missing(library: &Library, matching: Option<&str>, most: usize) -> Result<Value> {
    let counted = library.missing_counted(matching)?;
    let missing = library.missing_tracks(matching, Some(most))?;

    Ok(json!({
        "missing": counted.tracks,
        "tracks": missing.iter().map(written::missing).collect::<Vec<_>>(),
    }))
}
