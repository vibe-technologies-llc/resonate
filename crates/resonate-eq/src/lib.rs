mod apo;
mod catalogue;
mod error;
mod fitting;
mod graphic;
mod measurement;
mod provider;
mod store;
mod targets;

mod words {
    include!(concat!(env!("OUT_DIR"), "/words.rs"));
}

pub use crate::{
    apo::{LARGEST_PROFILE, Reading, read as reading, read_number, read_profile, write_profile},
    catalogue::{
        Catalogue, Device, DeviceId, Found, MEASURERS_BEST_FIRST, search, search_preferring,
        suggest, suggest_preferring,
    },
    error::{EqOp, Error, Result, StoreOp},
    fitting::{Fitted, Fitting, fit},
    measurement::{
        LARGEST_MEASUREMENT, Layout, POINTS_AT_LEAST, Point, Reading as ResponseReading, Response,
        read_response,
    },
    provider::{Corrected, Corrections, Fetched, Uncorrected},
    store::{Binding, Kept, Measured, OWN_FOLDER, ProfileName, Store},
    targets::BuiltIn,
};

#[cfg(test)]
mod said_through_the_catalog {
    use std::{fs, path::Path};

    const SAYS_WORDS: [&str; 3] = ["catalogue.rs", "measurement.rs", "targets.rs"];

    const PROPER: [&str; 3] = ["super review", ") by ", " on "];

    #[test]
    fn every_word_the_equaliser_names_for_a_listener_is_said_through_its_catalog() {
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
