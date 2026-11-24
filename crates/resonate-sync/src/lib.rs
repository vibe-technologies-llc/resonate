mod catalog;
mod change;
mod consulted;
mod error;
#[cfg(all(feature = "device", feature = "faults"))]
#[doc(hidden)]
pub mod faults;
#[cfg(feature = "device")]
mod fresh;
#[cfg(all(fuzzing, feature = "device"))]
mod fuzzing;
mod key;
mod ledger;
mod merge;
#[cfg(feature = "device")]
mod nearby;
pub mod party;
#[cfg(feature = "device")]
mod peer;
mod quic;
#[cfg(feature = "device")]
mod relay;
mod remote;
mod revision;
#[cfg(feature = "device")]
mod routes;
mod seal;
#[cfg(feature = "device")]
mod service;
pub mod shared;
#[cfg(feature = "device")]
mod stun;
mod web;
pub mod wire;

#[cfg_attr(not(feature = "device"), allow(dead_code))]
mod words {
    include!(concat!(env!("OUT_DIR"), "/words.rs"));
}

#[cfg(all(fuzzing, feature = "device"))]
pub use crate::fuzzing::read_a_stun_answer;
pub use crate::{
    catalog::{
        Catalog, DeletedEverywhere, Held, HeldMark, HeldShare, HeldSince, LocalListen,
        LocalPlaylist, LocalPlaylistId, LocalQuery, Opened, ReadSeek, Refused, RemoteListen,
        Resumed, SongTags, SongsHeldAfter,
    },
    change::{
        AnswerNote, Change, Clock, DeletionNote, DeviceForgotten, DeviceNote, FavouriteNote,
        Handoff, LayoutNote, Listen, Platform, PlaylistBody, PlaylistFolded, PlaylistKey,
        PlaylistNote, QueryBody, QueryFolded, QueryNote, STAMP_AHEAD_OF_THE_WALL_AT_MOST_NANOS,
        ShareNote, Stamp, wall_nanos,
    },
    consulted::{
        AskedForExport, AskedToConsult, CONSULT_ANSWERED_WITHIN, Consultable, ConsultedAnswer,
        Consulting, Dates, ExportDeclined, ExportFetched, ExportHeld, ExportPart, KeptAnswer,
        OfferedExport, ReadsExport,
    },
    error::{CodeRefused, DeflateOp, Error, FileOp, Malformed, Result, StoreOp, StreamOp, TlsOp},
    key::{DEVICE_BYTES, Derived, DeviceId, PAIRING_LINK, SpaceId, SyncKey, Token},
    ledger::{Applied, Follow, Ledger, Looked, Me, Welcomed},
    quic::{read_frame, send_frame, server_transport},
    remote::{
        CONTROL_ANSWERED_WITHIN, Controlled, Controlling, MIRROR_EXPIRES_AFTER,
        MIRROR_GOES_STALE_AFTER, Mirror, Permille, ROWS_MIRRORED_AT_MOST, RemoteCommand,
        RemotePlayback, RemoteRow, RemoteZone, Revisions, Uncontrolled, Watched,
        ZONES_MIRRORED_AT_MOST,
    },
    revision::{Bound, Revised, Revision},
    seal::{opened, sealed},
    web::{Accepted, Duplex, Heard, Holds, Incoming, Outgoing, accepted},
    wire::HOSTED_SERVER,
};
#[cfg(feature = "device")]
pub use crate::{
    consulted::ExportReceiving,
    nearby::{BONJOUR_SERVICE, Broadcast, NEARBY_GROUP, NEARBY_PORT, Nearby, RECORD_KEY},
    quic::check_trusted_certificate,
    relay::{RELAYED_PORT, Relay},
    service::{
        AskedToControl, AskedToFetch, ExportWanted, FetchOutcome, SERVER_PORT, Server, Settings,
        Shown, Standing, Sync, SyncDevice, Transfer, TransferState,
    },
    stun::{Reflector, STUN_PORT},
    web::{WEB_SERVER_PORT, WebAddress, connect, to_the_web, web_address},
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
