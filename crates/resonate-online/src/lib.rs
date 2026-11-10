mod acoustid;
mod apple;
mod audd;
mod autoeq;
mod betterlyrics;
mod by_ear;
mod client;
mod commons;
mod coverart;
mod deezer;
mod error;
mod lastfm;
mod linked;
mod listenbrainz;
mod lrclib;
mod lyrics;
mod musicbrainz;
mod query;
mod reference;
mod releases;
mod shared;
mod shazam;
mod soundcloud;
mod spotify;
mod spotify_account;
mod tidal;
mod wikidata;
mod wikipedia;
mod youtube;

mod words {
    include!(concat!(env!("OUT_DIR"), "/words.rs"));
}

#[cfg(fuzzing)]
pub fn read_a_shared_page(page: &str) {
    let _ = crate::shared::shared_picture(page);
    let _ = crate::shared::next_data(page);
    let _ = crate::spotify::read_embedded(page);
    let _ = crate::youtube::read_listed(page);
    let _ = crate::tidal::read_embedded(page);
    let _ = crate::linked::read_pages(page);
}

pub use crate::{
    acoustid::AcoustId,
    audd::Audd,
    autoeq::AutoEq,
    by_ear::ByEar,
    client::{Client, Identity, Introduction},
    error::{Error, Host, Result},
    lastfm::{Application, LastFm, LastfmApproving, Session, approval_asked},
    listenbrainz::ListenBrainz,
    lyrics::{OnlineLyrics, source_of as lyric_source_of},
    reference::Online,
    releases::{Release, Releases, Repository, Version},
    shazam::Shazam,
    spotify_account::{
        SPOTIFY_REDIRECT, SpotifyAccount, SpotifyAuthorizing, SpotifyEndpoints, SpotifyGrant,
        SpotifyGrants, SpotifySignIn, SpotifySignInError, SpotifySignedIn,
    },
};

#[cfg(test)]
mod said_through_the_catalog {
    use std::{fs, path::Path};

    const PROPER: [&str; 8] = [
        "playlist-read-private playlist-read-collaborative user-library-read",
        "SpotifyGrant",
        "SpotifyAuthorizing",
        "SpotifySignedIn",
        "Bearer {token}",
        "404 Not Found",
        "400 Bad Request",
        "200 OK",
    ];

    #[test]
    fn every_word_a_spotify_sign_in_shows_a_listener_is_said_through_its_catalog() {
        let source = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("src/spotify_account.rs"),
        )
        .expect("the account's source");

        let written: Vec<String> = resonate_words_build::prose_of(&source)
            .into_iter()
            .filter(|said| !PROPER.contains(&said.text.as_str()))
            .filter(|said| {
                !said.text.starts_with("HTTP/1.1 ") && !said.text.starts_with("<!doctype")
            })
            .map(|said| format!("spotify_account.rs:{} {:?}", said.line, said.text))
            .collect();

        assert!(
            written.is_empty(),
            "words written in the code:\n{}",
            written.join("\n")
        );
    }
}
