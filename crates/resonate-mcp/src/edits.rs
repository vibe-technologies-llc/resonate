use resonate_core::{ReleaseTrackId, Span};
use resonate_library::{Cut, Favoured, Library, PlaylistName, SavedQuery, SortOrder};
use serde_json::{Value, json};

use crate::{
    Error, Result,
    catalog::{self, Wanted},
    written,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marking {
    pub favoured: Vec<Favoured>,
    pub favourite: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Filling {
    Empty,
    Rows(Wanted),
    Search { query: String, most: Option<usize> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dropping {
    Rows(Span),
    Matching(String),
}

pub(crate) fn favour(library: &Library, marking: &Marking) -> Result<Value> {
    let mut changed = 0_usize;
    for what in &marking.favoured {
        if library.favour(*what, marking.favourite)? {
            changed += 1;
        }
    }

    Ok(json!({
        "favourite": marking.favourite,
        "named": marking.favoured.len(),
        "changed": changed,
    }))
}

pub(crate) fn create_playlist(library: &Library, name: &str, filling: &Filling) -> Result<Value> {
    let id = match filling {
        Filling::Empty => library.create_playlist(name)?,
        Filling::Rows(wanted) => library.start_playlist(name, &cuts(library, wanted)?)?,
        Filling::Search { query, most } => {
            let sort = SortOrder::Relevance;
            library.save_query(
                name,
                &SavedQuery {
                    text: Some(query.trim().to_owned()),
                    sort,
                    reading: sort.reads(),
                    limit: *most,
                },
            )?
        }
    };
    let made = library
        .playlist(id)?
        .ok_or_else(|| Error::NoSuchPlaylist(PlaylistName::new(name)))?;

    Ok(json!({ "playlist": written::playlist(&made) }))
}

pub(crate) fn add_to_playlist(library: &Library, name: &str, wanted: &Wanted) -> Result<Value> {
    let found = catalog::playlist_called(library, name)?;
    let added = library.add_to_playlist(found.id, &cuts(library, wanted)?)?;

    Ok(json!({
        "playlist": written::playlist(&catalog::playlist_called(library, &found.name)?),
        "added": added,
    }))
}

pub(crate) fn remove_from_playlist(
    library: &Library,
    name: &str,
    dropping: &Dropping,
) -> Result<Value> {
    let found = catalog::playlist_called(library, name)?;
    match dropping {
        Dropping::Rows(rows) => {
            if !library.remove_from_playlist(found.id, *rows)? {
                return Err(Error::NotInThePlaylist {
                    playlist: PlaylistName::new(found.name),
                    row: rows.first(),
                });
            }
        }
        Dropping::Matching(matching) => {
            library.remove_matching(found.id, matching)?;
        }
    }
    let left = catalog::playlist_called(library, &found.name)?;

    Ok(json!({
        "playlist": written::playlist(&left),
        "removed": found.entries.saturating_sub(left.entries),
    }))
}

pub(crate) fn rename_playlist(library: &Library, name: &str, to: &str) -> Result<Value> {
    let found = catalog::playlist_called(library, name)?;
    library.rename_playlist(found.id, to)?;
    let renamed = library
        .playlist(found.id)?
        .ok_or_else(|| Error::NoSuchPlaylist(PlaylistName::new(to)))?;

    Ok(json!({ "playlist": written::playlist(&renamed) }))
}

pub(crate) fn discard_playlist(library: &Library, name: &str) -> Result<Value> {
    let found = catalog::playlist_called(library, name)?;
    if !library.remove_playlist(found.id)? {
        return Err(Error::NoSuchPlaylist(PlaylistName::new(found.name)));
    }

    Ok(json!({ "discarded": written::playlist(&found) }))
}

pub(crate) fn want(library: &Library, release_tracks: &[ReleaseTrackId]) -> Result<Value> {
    for release_track in release_tracks {
        library.want(*release_track)?;
    }

    Ok(json!({ "wanted": release_tracks.len() }))
}

fn cuts(library: &Library, wanted: &Wanted) -> Result<Vec<Cut>> {
    Ok(catalog::wanted_rows(library, wanted)?
        .into_iter()
        .map(|(location, span)| Cut { location, span })
        .collect())
}
