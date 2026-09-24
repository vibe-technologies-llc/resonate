mod acoustid;
mod apple;
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
    client::{Client, Identity, Introduction},
    error::{Error, Host, Result},
    lrclib::Lrclib,
    reference::Online,
    shazam::Shazam,
};
