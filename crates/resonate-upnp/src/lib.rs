mod beside;
mod carriage;
mod description;
mod error;
mod feed;
mod flac;
#[cfg(fuzzing)]
mod fuzzing;
mod http;
mod learning;
mod located;
mod network;
mod renderer;
mod serve;
mod soap;
mod ssdp;
mod stream;
#[cfg(test)]
mod testing;
mod wave;
mod wire;

mod words {
    include!(concat!(env!("OUT_DIR"), "/words.rs"));
}

pub use resonate_audio::RENDERER_NODES;

#[cfg(fuzzing)]
pub use crate::fuzzing::read_what_a_network_says;
pub use crate::{
    beside::{Beside, Neighbours},
    carriage::{Carriage, Wrapping},
    error::{Action, Asked, Error, FlacOp, Result, SearchOp, ServeOp},
    feed::Feed,
    learning::{
        CLOCK_MOVED_AT_LEAST, PULLED_AT_LEAST, SETTLED_BETWEEN_TRIALS, Signals, Silence,
        TRIAL_ASKED_EVERY, TRIAL_LASTS_AT_MOST,
    },
    located::Located,
    network::{FirstSearch, Network, Searching},
    renderer::{Renderer, Udn, Vouched},
    serve::{Server, Token},
    ssdp::{RENDERER_TYPE, SSDP},
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
}
