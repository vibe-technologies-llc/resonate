mod catalog;
mod change;
mod consulted;
mod error;
#[cfg(fuzzing)]
mod fuzzing;
mod key;
mod ledger;
mod merge;
mod nearby;
pub mod party;
mod peer;
mod quic;
mod relay;
mod remote;
mod seal;
mod service;
pub mod shared;
mod stun;
mod web;
pub mod wire;

mod words {
    include!(concat!(env!("OUT_DIR"), "/words.rs"));
}

#[cfg(fuzzing)]
pub use crate::fuzzing::read_a_stun_answer;
pub use crate::{
    catalog::{
        Catalog, Held, HeldShare, LocalListen, LocalPlaylist, LocalPlaylistId, LocalQuery, Opened,
        ReadSeek, Refused, RemoteListen, Resumed, SongTags,
    },
    change::{
        AnswerNote, Change, Clock, DeviceForgotten, DeviceNote, FavouriteNote, Handoff, Listen,
        Platform, PlaylistBody, PlaylistFolded, PlaylistKey, PlaylistNote, QueryBody, QueryFolded,
        QueryNote, ShareNote, Stamp,
    },
    consulted::{AskedToConsult, CONSULT_ANSWERED_WITHIN, Consultable, Consulting, KeptAnswer},
    error::{CodeRefused, DeflateOp, Error, FileOp, Malformed, Result, StoreOp, StreamOp, TlsOp},
    key::{DEVICE_BYTES, Derived, DeviceId, PAIRING_LINK, SpaceId, SyncKey, Token},
    ledger::{Applied, Follow, Ledger, Looked, Me},
    nearby::{BONJOUR_SERVICE, Broadcast, NEARBY_GROUP, NEARBY_PORT, Nearby, RECORD_KEY},
    quic::{read_frame, send_frame, transport},
    relay::{RELAYED_PORT, Relay},
    remote::{
        CONTROL_ANSWERED_WITHIN, Controlled, Controlling, MIRROR_GOES_STALE_AFTER, Mirror,
        Permille, ROWS_MIRRORED_AT_MOST, RemoteCommand, RemotePlayback, RemoteRow, RemoteZone,
        Revisions, Uncontrolled, Watched, ZONES_MIRRORED_AT_MOST,
    },
    seal::{opened, sealed},
    service::{
        AskedToControl, HOSTED_SERVER, SERVER_PORT, Server, Settings, Shown, Standing, Sync,
        SyncDevice, Transfer, TransferState,
    },
    stun::{Reflector, STUN_PORT},
    web::{
        Accepted, Duplex, Incoming, Outgoing, WEB_SERVER_PORT, WebAddress, accepted, connect,
        to_the_web, web_address,
    },
};

#[cfg(test)]
mod said_through_the_catalog {
    use std::{fs, path::Path};

    const SAYS_WORDS: [&str; 2] = ["change.rs", "shared/service.rs"];

    const PROPER: [&str; 5] = ["Linux", "Android", "iOS", "Windows", "macOS"];

    #[test]
    fn every_word_sync_names_for_a_listener_is_said_through_its_catalog() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

        let written: Vec<String> = SAYS_WORDS
            .iter()
            .flat_map(|file| {
                let source = fs::read_to_string(root.join(file)).expect("a file that says words");
                resonate_words_build::prose_of(&source)
                    .into_iter()
                    .filter(|said| !PROPER.contains(&said.text.as_str()))
                    .map(move |said| format!("{file}:{} {:?}", said.line, said.text))
            })
            .collect();

        assert!(
            written.is_empty(),
            "words written in the code:\n{}",
            written.join("\n")
        );
    }
}
