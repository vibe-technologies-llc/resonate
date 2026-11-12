use std::path::Path;

use resonate_core::PlaylistId;
use resonate_library::{
    Found, FromTheLink, Library, LookupOp, Playlist, PlaylistLink, PlaylistSource, ProfileLink,
    Reference, Upkeep,
};

use crate::{
    Error, Result,
    config::Config,
    online::{self, Asking},
    words,
};

pub fn link_named_by(file: &Path) -> Option<PlaylistLink> {
    file.to_str().and_then(PlaylistLink::read)
}

pub fn profile_named_by(file: &Path) -> Option<ProfileLink> {
    file.to_str().and_then(ProfileLink::read)
}

pub fn follow_profile(
    library: &Library,
    config: &Config,
    link: &ProfileLink,
    upkeep: Upkeep,
) -> Result<()> {
    let reference = online::reference_asked_for(config, Asking::ForTheListener)?;
    profile_followed(library, reference.as_ref(), link, upkeep)
}

fn said_to_sign_in_again(error: Error) -> Error {
    match error {
        Error::Library(resonate_library::Error::Refused {
            op: LookupOp::SpotifyAccount,
            ..
        }) => Error::SpotifySignInLapsed,
        error => error,
    }
}

fn profile_followed(
    library: &Library,
    reference: &dyn Reference,
    link: &ProfileLink,
    upkeep: Upkeep,
) -> Result<()> {
    let followed = library
        .follow_profile(reference, link, upkeep)
        .map_err(|error| said_to_sign_in_again(error.into()))?
        .ok_or(match link {
            ProfileLink::SpotifyAccount => Error::NotSignedInToSpotify,
            ProfileLink::Deezer(_) => Error::NoProfileAtTheLink,
        })?;
    for read in &followed.playlists {
        let wanted = want_the_rest(library, reference, &read.found);
        said!(
            "{}{}{}",
            whence(read, link.service(), upkeep),
            rows_moved(read),
            beyond_the_library(wanted, read.found.len(), read.unnamed)
        );
    }
    said!(
        "{}{}{}{}",
        match upkeep {
            Upkeep::InStep => words::followed::followed_on(&followed.name, link.service()),
            Upkeep::Once => words::followed::moved_once_on(&followed.name, link.service()),
        },
        counted_beside(followed.let_go, words::followed::let_go),
        counted_beside(followed.gone, words::followed::deleted_there),
        counted_beside(followed.unread, words::followed::unread),
    );
    Ok(())
}

fn counted_beside(count: usize, said: fn(usize) -> String) -> String {
    match count {
        0 => String::new(),
        count => format!(" · {}", said(count)),
    }
}

fn whence(read: &FromTheLink, service: &str, upkeep: Upkeep) -> String {
    if read.started {
        words::followed::started_from(kept_as(upkeep), &read.name, service)
    } else {
        words::followed::read_from(kept_as(upkeep), &read.name, service)
    }
}

pub fn import(
    library: &Library,
    config: &Config,
    link: &PlaylistLink,
    name: Option<&str>,
    upkeep: Upkeep,
) -> Result<()> {
    let reference = online::reference_asked_for(config, Asking::ForTheListener)?;
    let read = library
        .import_playlist_link(reference.as_ref(), link, name, upkeep)?
        .ok_or(Error::NoPlaylistAtTheLink)?;
    let wanted = want_the_rest(library, reference.as_ref(), &read.found);

    said!(
        "{}{}{}",
        whence(&read, link.service(), upkeep),
        rows_moved(&read),
        beyond_the_library(wanted, read.found.len(), read.unnamed)
    );
    Ok(())
}

pub fn refresh_every_kept(library: &Library, config: &Config) -> Result<()> {
    let profiles = library.profiles_followed()?;
    if profiles.is_empty() && library.playlists_kept_in_step()?.is_empty() {
        said!("{}", words::followed::none_in_step());
        return Ok(());
    }

    let reference = online::reference_asked_for(config, Asking::ForTheListener)?;
    for (profile, _) in profiles {
        if profile == ProfileLink::SpotifyAccount && !config.spotify_signed_in() {
            said!("{}", words::followed::spotify_not_signed_in());
            continue;
        }
        profile_followed(library, reference.as_ref(), &profile, Upkeep::InStep)?;
    }
    for (playlist, source) in library.playlists_kept_in_step()? {
        refreshed(library, reference.as_ref(), playlist, &source).map_err(said_to_sign_in_again)?;
    }
    Ok(())
}

pub fn refresh(library: &Library, config: &Config, playlist: &Playlist) -> Result<()> {
    let source = playlist
        .source
        .as_ref()
        .ok_or(resonate_library::Error::NoPlaylistSource {
            playlist: playlist.id,
        })?;
    let reference = online::reference_asked_for(config, Asking::ForTheListener)?;
    refreshed(library, reference.as_ref(), playlist.id, source)
}

pub fn set_upkeep(library: &Library, playlist: &Playlist, upkeep: Upkeep) -> Result<()> {
    let changed = library.keep_playlist_from_its_link(playlist.id, upkeep)?;
    let link = playlist
        .source
        .as_ref()
        .map_or(words::followed::its_link(), |source| source.link.service());

    match (changed, upkeep) {
        (false, _) => said!(
            "{}",
            words::followed::was_already(kept_as(upkeep), &playlist.name)
        ),
        (true, Upkeep::InStep) => said!("{}", words::followed::in_step_now(link, &playlist.name)),
        (true, Upkeep::Once) => said!(
            "{}",
            words::followed::left_as_it_stands(link, &playlist.name)
        ),
    }
    Ok(())
}

pub fn forget_link(library: &Library, playlist: &Playlist) -> Result<()> {
    match library.forget_playlist_source(playlist.id)? {
        true => said!("{}", words::followed::no_longer_follows(&playlist.name)),
        false => said!("{}", words::followed::came_from_no_link(&playlist.name)),
    }
    Ok(())
}

pub fn came_from(source: Option<&PlaylistSource>) -> String {
    match source {
        None => String::new(),
        Some(source) => words::followed::came_from(
            source.link.service(),
            match source.upkeep {
                Upkeep::Once => words::followed::once(),
                Upkeep::InStep => words::followed::in_step(),
            },
        ),
    }
}

fn refreshed(
    library: &Library,
    reference: &dyn Reference,
    playlist: PlaylistId,
    source: &PlaylistSource,
) -> Result<()> {
    let Some(read) = library.refresh_playlist(reference, playlist)? else {
        said!(
            "{}",
            words::followed::names_no_playlist(
                library
                    .playlist(playlist)?
                    .map_or_else(|| playlist.to_string(), |held| held.name),
                source.link.service(),
            )
        );
        return Ok(());
    };
    let wanted = want_the_rest(library, reference, &read.found);

    said!(
        "{}{}",
        words::followed::read_again(
            rows_moved(&read).trim_start_matches(", "),
            &read.name,
            source.link.service(),
        ),
        beyond_the_library(wanted, read.found.len(), read.unnamed)
    );
    Ok(())
}

fn want_the_rest(library: &Library, reference: &dyn Reference, found: &[Found]) -> usize {
    found
        .iter()
        .filter(|song| match library.want_found(reference, song) {
            Ok(_) => true,
            Err(error) => {
                tracing::warn!(%error, recording = %song.recording, title = %song.title, "a song of a linked playlist was not wanted");
                false
            }
        })
        .count()
}

fn kept_as(upkeep: Upkeep) -> &'static str {
    match upkeep {
        Upkeep::Once => words::followed::imported_once(),
        Upkeep::InStep => words::followed::kept_in_step(),
    }
}

fn rows_moved(read: &FromTheLink) -> String {
    match (read.added, read.removed) {
        (0, 0) => format!(", {}", words::followed::nothing_to_add()),
        (added, 0) => format!(", {}", words::followed::added(added)),
        (0, removed) => format!(", {}", words::followed::took_out(removed)),
        (added, removed) => format!(", {}", words::followed::added_and_took_out(added, removed)),
    }
}

fn beyond_the_library(wanted: usize, found: usize, unnamed: usize) -> String {
    let mut said = String::new();
    if wanted > 0 {
        said.push_str(&format!(" · {}", words::followed::wanted(wanted)));
    }
    if found > wanted {
        said.push_str(&format!(
            " · {}",
            words::followed::not_wanted(found - wanted)
        ));
    }
    if unnamed > 0 {
        said.push_str(&format!(" · {}", words::followed::unnamed(unnamed)));
    }
    said
}
