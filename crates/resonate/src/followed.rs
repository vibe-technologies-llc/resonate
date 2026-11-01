use std::path::Path;

use resonate_core::PlaylistId;
use resonate_library::{
    Found, FromTheLink, Library, Playlist, PlaylistLink, PlaylistSource, ProfileLink, Reference,
    Upkeep,
};

use crate::{
    Error, Result,
    cli::UpkeepArg,
    config::Config,
    counted,
    online::{self, Asking},
};

impl From<UpkeepArg> for Upkeep {
    fn from(upkeep: UpkeepArg) -> Self {
        match upkeep {
            UpkeepArg::Once => Self::Once,
            UpkeepArg::InStep => Self::InStep,
        }
    }
}

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

fn profile_followed(
    library: &Library,
    reference: &dyn Reference,
    link: &ProfileLink,
    upkeep: Upkeep,
) -> Result<()> {
    let followed = library
        .follow_profile(reference, link, upkeep)?
        .ok_or(Error::NoProfileAtTheLink)?;
    for read in &followed.playlists {
        let wanted = want_the_rest(library, reference, &read.found);
        said!(
            "{}: {} {}, {}{}{}",
            read.name,
            if read.started {
                "started from"
            } else {
                "read from"
            },
            link.service(),
            kept_as(upkeep),
            rows_moved(read),
            beyond_the_library(wanted, read.found.len(), read.unnamed)
        );
    }
    said!(
        "{}: {} on {}{}{}{}",
        followed.name,
        match upkeep {
            Upkeep::InStep => "followed",
            Upkeep::Once => "moved to Resonate once, and no longer followed",
        },
        link.service(),
        counted_beside(followed.let_go, "playlist let go", "playlists let go"),
        counted_beside(
            followed.gone,
            "playlist deleted there",
            "playlists deleted there"
        ),
        counted_beside(followed.unread, "playlist unread", "playlists unread"),
    );
    Ok(())
}

fn counted_beside(count: usize, one: &str, many: &str) -> String {
    match count {
        0 => String::new(),
        count => format!(" · {}", counted(count as u64, one, many)),
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
        "{}: {} {}, {}{}{}",
        read.name,
        if read.started {
            "started from"
        } else {
            "read from"
        },
        link.service(),
        kept_as(upkeep),
        rows_moved(&read),
        beyond_the_library(wanted, read.found.len(), read.unnamed)
    );
    Ok(())
}

pub fn refresh_every_kept(library: &Library, config: &Config) -> Result<()> {
    let profiles = library.profiles_followed()?;
    if profiles.is_empty() && library.playlists_kept_in_step()?.is_empty() {
        said!(
            "no playlist is kept in step with a link; `resonate import <LINK> --upkeep in-step` keeps one"
        );
        return Ok(());
    }

    let reference = online::reference_asked_for(config, Asking::ForTheListener)?;
    for (profile, _) in profiles {
        profile_followed(library, reference.as_ref(), &profile, Upkeep::InStep)?;
    }
    for (playlist, source) in library.playlists_kept_in_step()? {
        refreshed(library, reference.as_ref(), playlist, &source)?;
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
        .map_or("its link", |source| source.link.service());

    match (changed, upkeep) {
        (false, _) => said!("{} was already {}", playlist.name, kept_as(upkeep)),
        (true, Upkeep::InStep) => said!("{} is kept in step with {link} now", playlist.name),
        (true, Upkeep::Once) => said!(
            "{} is left as it stands, and {link} is not read again on its own",
            playlist.name
        ),
    }
    Ok(())
}

pub fn forget_link(library: &Library, playlist: &Playlist) -> Result<()> {
    match library.forget_playlist_source(playlist.id)? {
        true => said!("{} no longer follows a link; its rows stay", playlist.name),
        false => said!("{} came from no link", playlist.name),
    }
    Ok(())
}

pub fn came_from(source: Option<&PlaylistSource>) -> String {
    match source {
        None => String::new(),
        Some(source) => format!(
            "{}, {}",
            source.link.service(),
            match source.upkeep {
                Upkeep::Once => "once",
                Upkeep::InStep => "in step",
            }
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
            "{}: {} names no playlist now; its rows stay as they were",
            library
                .playlist(playlist)?
                .map_or_else(|| playlist.to_string(), |held| held.name),
            source.link.service()
        );
        return Ok(());
    };
    let wanted = want_the_rest(library, reference, &read.found);

    said!(
        "{}: read again from {}, {}{}",
        read.name,
        source.link.service(),
        rows_moved(&read).trim_start_matches(", "),
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

const fn kept_as(upkeep: Upkeep) -> &'static str {
    match upkeep {
        Upkeep::Once => "imported once",
        Upkeep::InStep => "kept in step",
    }
}

fn rows_moved(read: &FromTheLink) -> String {
    match (read.added, read.removed) {
        (0, 0) => ", nothing to add".to_owned(),
        (added, 0) => format!(", added {}", counted(added as u64, "song", "songs")),
        (0, removed) => format!(", took out {}", counted(removed as u64, "song", "songs")),
        (added, removed) => format!(
            ", added {} and took out {}",
            counted(added as u64, "song", "songs"),
            counted(removed as u64, "song", "songs")
        ),
    }
}

fn beyond_the_library(wanted: usize, found: usize, unnamed: usize) -> String {
    let mut said = String::new();
    if wanted > 0 {
        said.push_str(&format!(
            " · wanted {} the library lacks, which `resonate poll` fetches",
            counted(wanted as u64, "song", "songs")
        ));
    }
    if found > wanted {
        said.push_str(&format!(
            " · {} could not be wanted",
            counted((found - wanted) as u64, "song", "songs")
        ));
    }
    if unnamed > 0 {
        said.push_str(&format!(
            " · {} named nothing MusicBrainz knows",
            counted(unnamed as u64, "song", "songs")
        ));
    }
    said
}
