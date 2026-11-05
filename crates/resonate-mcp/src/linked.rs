use resonate_library::{
    FromTheLink, Library, PlaylistLink, PlaylistName, ProfileLink, Reference, Upkeep,
};
use serde_json::{Value, json};

use crate::{Error, Result, catalog, written};

pub(crate) fn import(
    library: &Library,
    reference: &dyn Reference,
    link: &PlaylistLink,
    name: Option<&str>,
    upkeep: Upkeep,
) -> Result<Value> {
    let read = library
        .import_playlist_link(reference, link, name, upkeep)?
        .ok_or(Error::NoPlaylistAtTheLink)?;
    answered(library, reference, &read)
}

pub(crate) fn refresh(library: &Library, reference: &dyn Reference, name: &str) -> Result<Value> {
    let found = catalog::playlist_called(library, name)?;
    let read = library
        .refresh_playlist(reference, found.id)?
        .ok_or(Error::NoPlaylistAtTheLink)?;
    answered(library, reference, &read)
}

pub(crate) fn follow_profile(
    library: &Library,
    reference: &dyn Reference,
    link: &ProfileLink,
    upkeep: Upkeep,
) -> Result<Value> {
    let followed = library
        .follow_profile(reference, link, upkeep)?
        .ok_or(match link {
            ProfileLink::SpotifyAccount => Error::NotSignedInToSpotify,
            ProfileLink::Deezer(_) => Error::NoProfileAtTheLink,
        })?;
    let mut playlists = Vec::with_capacity(followed.playlists.len());
    for read in &followed.playlists {
        playlists.push(answered(library, reference, read)?);
    }

    Ok(json!({
        "profile": followed.name,
        "service": link.service(),
        "followed": upkeep == Upkeep::InStep,
        "playlists": playlists,
        "let_go": followed.let_go,
        "gone": followed.gone,
        "unread": followed.unread,
    }))
}

pub(crate) fn stop_following(library: &Library, link: &ProfileLink) -> Result<Value> {
    let followed = library
        .profiles_followed()?
        .iter()
        .any(|(held, _)| held == link);
    library.stop_following(link)?;

    Ok(json!({ "stopped": followed }))
}

pub(crate) fn keep(library: &Library, name: &str, upkeep: Upkeep) -> Result<Value> {
    let found = catalog::playlist_called(library, name)?;
    let changed = library.keep_playlist_from_its_link(found.id, upkeep)?;
    let kept = library.playlist(found.id)?.unwrap_or(found);

    Ok(json!({ "playlist": written::playlist(&kept), "changed": changed }))
}

fn answered(library: &Library, reference: &dyn Reference, read: &FromTheLink) -> Result<Value> {
    let wanted = read
        .found
        .iter()
        .filter(|song| match library.want_found(reference, song) {
            Ok(_) => true,
            Err(error) => {
                tracing::warn!(%error, recording = %song.recording, "a song of a linked playlist was not wanted");
                false
            }
        })
        .count();
    let playlist = library
        .playlist(read.playlist)?
        .ok_or_else(|| Error::NoSuchPlaylist(PlaylistName::new(read.name.as_str())))?;

    Ok(json!({
        "playlist": written::playlist(&playlist),
        "started": read.started,
        "added": read.added,
        "removed": read.removed,
        "wanted": wanted,
        "unnamed": read.unnamed,
    }))
}
