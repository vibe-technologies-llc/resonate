use resonate_core::{ReleaseTrackId, Span};
use resonate_library::{
    Cut, Edit, Favoured, Library, PlaylistName, SavedQuery, SortOrder, Undoable,
};
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
    let changed = library.favour_all(&marking.favoured, marking.favourite)?;

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
    let wanted = library.want_release_tracks(release_tracks)?;

    Ok(json!({ "wanted": wanted.len() }))
}

fn cuts(library: &Library, wanted: &Wanted) -> Result<Vec<Cut>> {
    Ok(catalog::wanted_rows(library, wanted)?
        .into_iter()
        .map(|(location, span)| Cut { location, span })
        .collect())
}

pub(crate) fn undo_edit(library: &Library, again: bool) -> Result<Value> {
    let walked = if again {
        library.redo()?
    } else {
        library.undo()?
    };

    Ok(match walked {
        Some(walked) => json!({
            "walked": if again { "redone" } else { "undone" },
            "edit": edit_called(&walked),
            "playlist": walked.name,
            "more_to_undo": walked.behind,
        }),
        None => json!({ "walked": null }),
    })
}

fn edit_called(walked: &Undoable) -> &'static str {
    match walked.edit {
        Edit::Started => "started",
        Edit::Renamed => "renamed",
        Edit::Revised => "revised",
        Edit::Discarded => "discarded",
        Edit::Added => "added",
        Edit::Copied => "copied",
        Edit::Imported => "imported",
        Edit::Removed => "removed",
        Edit::Dropped => "dropped",
        Edit::Tidied => "tidied",
        Edit::Folded => "folded",
        Edit::Moved => "moved",
        Edit::Ordered => "ordered",
        Edit::Kept => "kept",
    }
}
