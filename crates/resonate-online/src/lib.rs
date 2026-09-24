mod acoustid;
mod audd;
mod autoeq;
mod client;
mod commons;
mod coverart;
mod deezer;
mod error;
mod lrclib;
mod musicbrainz;
mod query;
mod reference;
mod shazam;
mod wikidata;

pub use crate::{
    acoustid::AcoustId,
    audd::Audd,
    autoeq::AutoEq,
    client::{Client, Identity},
    error::{Error, Host, Result},
    lrclib::Lrclib,
    reference::Online,
    shazam::Shazam,
};
