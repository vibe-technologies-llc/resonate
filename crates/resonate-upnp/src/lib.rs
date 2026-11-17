mod announce;
mod beside;
mod carriage;
mod catalog;
mod criteria;
mod decoded;
mod description;
mod didl;
mod directory;
mod door;
mod error;
mod events;
mod feed;
mod fetched;
mod flac;
#[cfg(fuzzing)]
mod fuzzing;
mod http;
mod learning;
mod listing;
mod located;
mod naming;
mod neighbours;
mod network;
mod offered;
mod presented;
mod renderer;
mod serve;
mod servers;
mod soap;
mod ssdp;
mod stream;
#[cfg(test)]
mod testing;
mod watched;
mod wave;
mod wire;

mod words {
    include!(concat!(env!("OUT_DIR"), "/words.rs"));
}

pub use resonate_audio::RENDERER_NODES;

#[cfg(fuzzing)]
pub use crate::fuzzing::{read_a_listing, read_what_a_network_says};
pub use crate::{
    beside::{Beside, Neighbours},
    carriage::{Carriage, Wrapping},
    catalog::{
        AudioKind, Bytes, Catalog, Content, Entry, Field, Held, Kind, Object, Opened, Page,
        Picture, Query, Searched, Shelf, Shelved, Song, Test, Within,
    },
    decoded::{AsWave, Pcm, wave_bytes, waves_alike},
    error::{Action, Asked, Error, FlacOp, Result, SearchOp, ServeOp},
    feed::Feed,
    fetched::Fetched,
    learning::{
        CLOCK_MOVED_AT_LEAST, PULLED_AT_LEAST, SETTLED_BETWEEN_TRIALS, Signals, Silence,
        TRIAL_ASKED_EVERY, TRIAL_LASTS_AT_MOST,
    },
    listing::{Container, ENTRIES_A_LISTING_AT_MOST, Item, Listing, ObjectId, Resource},
    located::Located,
    network::{FirstSearch, Network, Searching},
    offered::{MediaServer, Offering, ServerName},
    renderer::{Renderer, Udn, Vouched},
    serve::{Server, Token},
    servers::{BROWSED_A_PAGE, MediaServers, PageAsked, Paged, ServerFound},
    ssdp::{RENDERER_TYPE, SERVER_TYPE, SSDP},
};

#[cfg(test)]
mod said_through_the_catalog {
    use std::{fs, path::Path};

    #[test]
    fn every_word_a_renderer_is_shown_by_is_said_through_its_catalog() {
        let source =
            fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/network.rs"))
                .expect("the network's source");

        let written: Vec<String> = resonate_words_build::prose_of(&source)
            .into_iter()
            .map(|said| format!("network.rs:{} {:?}", said.line, said.text))
            .collect();

        assert!(
            written.is_empty(),
            "words written in the code:\n{}",
            written.join("\n")
        );
    }

    #[test]
    fn every_word_the_media_server_shows_a_control_point_is_said_through_its_catalog() {
        let written: Vec<String> = ["src/naming.rs"]
            .into_iter()
            .flat_map(|file| {
                let source = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(file))
                    .expect("the media server's source");
                resonate_words_build::prose_of(&source)
                    .into_iter()
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
