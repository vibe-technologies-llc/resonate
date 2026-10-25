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
mod shared;
mod shazam;
mod soundcloud;
mod spotify;
mod tidal;
mod wikidata;
mod wikipedia;
mod youtube;

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
    lastfm::{Application, LastFm, Session, signed_in},
    listenbrainz::ListenBrainz,
    lyrics::{OnlineLyrics, source_of as lyric_source_of},
    reference::Online,
    shazam::Shazam,
};
