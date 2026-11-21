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
    if library.profiles_followed()?.is_empty() && library.playlists_kept_in_step()?.is_empty() {
        said!("{}", words::followed::none_in_step());
        return Ok(());
    }

    let reference = online::reference_asked_for(config, Asking::ForTheListener)?;
    refreshed_every_kept(library, reference.as_ref(), config.spotify_signed_in())
}

fn refreshed_every_kept(
    library: &Library,
    reference: &dyn Reference,
    spotify_signed_in: bool,
) -> Result<()> {
    let mut failed = 0;

    for (profile, _) in library.profiles_followed()? {
        if profile == ProfileLink::SpotifyAccount && !spotify_signed_in {
            said!("{}", words::followed::spotify_not_signed_in());
            continue;
        }
        if let Err(error) = profile_followed(library, reference, &profile, Upkeep::InStep) {
            told!(
                "{}",
                words::followed::profile_not_read_again(
                    profile.page(),
                    profile.service(),
                    with_its_causes(&error),
                )
            );
            failed += 1;
        }
    }
    for (playlist, source) in library.playlists_kept_in_step()? {
        if let Err(error) =
            refreshed(library, reference, playlist, &source).map_err(said_to_sign_in_again)
        {
            told!(
                "{}",
                words::followed::playlist_not_read_again(
                    named(library, playlist),
                    source.link.service(),
                    with_its_causes(&error),
                )
            );
            failed += 1;
        }
    }
    match failed {
        0 => Ok(()),
        failed => Err(Error::FollowedUnread { failed }),
    }
}

fn named(library: &Library, playlist: PlaylistId) -> String {
    library
        .playlist(playlist)
        .ok()
        .flatten()
        .map_or_else(|| playlist.to_string(), |held| held.name)
}

fn with_its_causes(error: &Error) -> String {
    let mut said = error.to_string();
    let mut cause = std::error::Error::source(error);

    while let Some(under) = cause {
        said.push_str(": ");
        said.push_str(&under.to_string());
        cause = under.source();
    }
    said
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use resonate_core::{Isrc, Link, Mbid, SourceId};
    use resonate_library::{
        AlbumLink, AlbumMatch, AlbumNames, ArtistLink, ArtistMatch, ArtistPressings, ArtistProfile,
        Barcode, BarcodeMatch, CoverArt, Discography, GroupAsked, GroupMatch, LinkNames,
        LinkedPlaylist, LinkedProfile, LyricText, LyricsAsked, ProfilePlaylist, Recording,
        RecordingAsked, RecordingMatch, Release, ReleaseAsked, ReleaseGroup, ReleaseMatch,
        SongLink, SongsAsked, StreamAsked,
    };

    use super::*;

    const DELETED: ProfileLink = ProfileLink::Deezer(1);
    const STILL_THERE: ProfileLink = ProfileLink::Deezer(2);

    struct Profiles {
        source: SourceId,
        deleted: AtomicBool,
        still_there_read: AtomicUsize,
    }

    impl Profiles {
        fn new() -> Self {
            Self {
                source: SourceId::new("profiles").expect("a source"),
                deleted: AtomicBool::new(false),
                still_there_read: AtomicUsize::new(0),
            }
        }
    }

    fn the_playlist_of(link: &ProfileLink) -> PlaylistLink {
        match link {
            ProfileLink::Deezer(user) => PlaylistLink::Deezer(user * 10),
            ProfileLink::SpotifyAccount => PlaylistLink::SpotifyLiked,
        }
    }

    impl Reference for Profiles {
        fn source(&self) -> &SourceId {
            &self.source
        }

        fn release(&self, _: &Mbid) -> resonate_library::Result<Option<Release>> {
            Ok(None)
        }

        fn find_release(&self, _: &ReleaseAsked) -> resonate_library::Result<Vec<ReleaseMatch>> {
            Ok(Vec::new())
        }

        fn releases_by_barcode(&self, _: &Barcode) -> resonate_library::Result<Vec<BarcodeMatch>> {
            Ok(Vec::new())
        }

        fn recording(&self, _: &Mbid) -> resonate_library::Result<Option<Recording>> {
            Ok(None)
        }

        fn recordings_of_isrc(&self, _: &Isrc) -> resonate_library::Result<Vec<Recording>> {
            Ok(Vec::new())
        }

        fn find_recording(
            &self,
            _: &RecordingAsked,
        ) -> resonate_library::Result<Vec<RecordingMatch>> {
            Ok(Vec::new())
        }

        fn find_songs(&self, _: &SongsAsked) -> resonate_library::Result<Vec<RecordingMatch>> {
            Ok(Vec::new())
        }

        fn release_group(&self, _: &Mbid) -> resonate_library::Result<Option<ReleaseGroup>> {
            Ok(None)
        }

        fn find_release_group(&self, _: &GroupAsked) -> resonate_library::Result<Vec<GroupMatch>> {
            Ok(Vec::new())
        }

        fn find_albums(&self, _: &str) -> resonate_library::Result<Vec<AlbumMatch>> {
            Ok(Vec::new())
        }

        fn group_cover(&self, _: &Mbid) -> resonate_library::Result<Option<CoverArt>> {
            Ok(None)
        }

        fn artist(&self, _: &Mbid) -> resonate_library::Result<Option<ArtistProfile>> {
            Ok(None)
        }

        fn find_artist(&self, _: &str) -> resonate_library::Result<Vec<ArtistMatch>> {
            Ok(Vec::new())
        }

        fn release_groups_of(&self, _: &Mbid, _: u32) -> resonate_library::Result<Discography> {
            Ok(Discography::default())
        }

        fn releases_of_group(&self, _: &Mbid) -> resonate_library::Result<Vec<Release>> {
            Ok(Vec::new())
        }

        fn releases_of_artist(
            &self,
            _: &Mbid,
            _: u32,
        ) -> resonate_library::Result<ArtistPressings> {
            Ok(ArtistPressings::default())
        }

        fn cover(&self, _: &Mbid, _: Option<&Mbid>) -> resonate_library::Result<Option<CoverArt>> {
            Ok(None)
        }

        fn portrait(&self, _: &[Link]) -> resonate_library::Result<Option<CoverArt>> {
            Ok(None)
        }

        fn streamed_at(&self, _: &StreamAsked) -> resonate_library::Result<Option<Link>> {
            Ok(None)
        }

        fn lyrics(&self, _: &LyricsAsked) -> resonate_library::Result<Option<LyricText>> {
            Ok(None)
        }

        fn song_linked(&self, _: &SongLink) -> resonate_library::Result<Option<LinkNames>> {
            Ok(None)
        }

        fn album_linked(&self, _: &AlbumLink) -> resonate_library::Result<Option<AlbumNames>> {
            Ok(None)
        }

        fn artist_linked(&self, _: &ArtistLink) -> resonate_library::Result<Option<String>> {
            Ok(None)
        }

        fn playlist_linked(
            &self,
            link: &PlaylistLink,
        ) -> resonate_library::Result<Option<LinkedPlaylist>> {
            Ok(Some(LinkedPlaylist {
                name: link.page(),
                songs: Vec::new(),
            }))
        }

        fn profile_linked(
            &self,
            link: &ProfileLink,
        ) -> resonate_library::Result<Option<LinkedProfile>> {
            if *link == DELETED && self.deleted.load(Ordering::Relaxed) {
                return Ok(None);
            }
            if *link == STILL_THERE {
                self.still_there_read.fetch_add(1, Ordering::Relaxed);
            }
            Ok(Some(LinkedProfile {
                name: link.page(),
                playlists: vec![ProfilePlaylist {
                    title: link.page(),
                    link: the_playlist_of(link),
                }],
            }))
        }

        fn artist_at(&self, _: &str) -> resonate_library::Result<Option<Mbid>> {
            Ok(None)
        }
    }

    #[test]
    fn a_profile_deleted_on_its_service_is_told_and_the_next_is_still_read_again() {
        let library = Library::open_in_memory().expect("a catalog");
        let profiles = Profiles::new();

        for profile in [DELETED, STILL_THERE] {
            library
                .follow_profile(&profiles, &profile, Upkeep::InStep)
                .expect("followed")
                .expect("a profile the service showed");
        }
        profiles.deleted.store(true, Ordering::Relaxed);

        let read_before = profiles.still_there_read.load(Ordering::Relaxed);

        let refreshed = refreshed_every_kept(&library, &profiles, false);
        let read_after = profiles.still_there_read.load(Ordering::Relaxed);

        assert!(matches!(
            refreshed,
            Err(Error::FollowedUnread { failed: 1 })
        ));
        assert_eq!(read_after, read_before + 1);
    }
}
