mod bonjour;
mod casting;
mod device;
mod error;
mod frame;
#[cfg(fuzzing)]
mod fuzzing;
mod given;
mod json;
mod learning;
mod link;
mod stream;
mod tls;

mod words {
    include!(concat!(env!("OUT_DIR"), "/words.rs"));
}

pub use resonate_audio::CAST_NODES;

#[cfg(fuzzing)]
pub use crate::fuzzing::read_what_a_receiver_says;
pub use crate::{
    bonjour::CAST_SERVICE,
    casting::{Casting, Finding, Searching},
    device::{CAST_PORT, CastId, Device, Found, MEDIA_RECEIVER},
    error::{Asked, Error, LinkOp, Refusal, Result},
    given::{Given, Host},
};

#[cfg(test)]
mod said_through_the_catalog {
    use std::{fs, path::Path};

    #[test]
    fn every_word_a_receiver_is_shown_by_is_said_through_its_catalog() {
        let source =
            fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/casting.rs"))
                .expect("the casting's source");

        let written: Vec<String> = resonate_words_build::prose_of(&source)
            .into_iter()
            .map(|said| format!("casting.rs:{} {:?}", said.line, said.text))
            .collect();

        assert!(
            written.is_empty(),
            "words written in the code:\n{}",
            written.join("\n")
        );
    }
}
