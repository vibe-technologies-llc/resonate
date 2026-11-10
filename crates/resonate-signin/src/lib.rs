mod error;
mod jar;
mod outcome;
mod place;
#[cfg(feature = "window")]
mod window;
#[cfg(feature = "window")]
mod words {
    include!(concat!(env!("OUT_DIR"), "/words.rs"));
}

#[cfg(feature = "window")]
pub use crate::{error::EngineOp, window::sign_in};
pub use crate::{
    error::{Error, JarOp, Result},
    jar::{Cookie, domain_cookie_of, kept, netscape, signed_in, written},
    outcome::{CLOSED, FAILED, Outcome, SIGNED_IN},
    place::{SIGN_IN_PAGE, is_youtube_music},
};

#[cfg(test)]
mod said_through_the_catalog {
    use std::{fs, path::Path};

    #[test]
    fn every_word_the_sign_in_window_shows_is_said_through_its_catalog() {
        let source =
            fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/window.rs"))
                .expect("the window's source");

        let written: Vec<String> = resonate_words_build::prose_of(&source)
            .into_iter()
            .map(|said| format!("window.rs:{} {:?}", said.line, said.text))
            .collect();

        assert!(
            written.is_empty(),
            "words written in the code:\n{}",
            written.join("\n")
        );
    }
}
