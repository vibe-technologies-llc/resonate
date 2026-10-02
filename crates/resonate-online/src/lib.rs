mod acoustid;
mod apple;
mod audd;
mod autoeq;
mod by_ear;
mod client;
mod commons;
mod coverart;
mod deezer;
mod error;
mod linked;
mod listenbrainz;
mod lrclib;
mod musicbrainz;
mod query;
mod reference;
mod shared;
mod shazam;
mod soundcloud;
mod spotify;
mod wikidata;
mod wikipedia;

pub use crate::{
    acoustid::AcoustId,
    audd::Audd,
    autoeq::AutoEq,
    by_ear::ByEar,
    client::{Client, Identity, Introduction},
    error::{Error, Host, Result},
    listenbrainz::ListenBrainz,
    lrclib::Lrclib,
    reference::Online,
    shazam::Shazam,
};
