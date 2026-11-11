mod cache;
mod error;
mod gateway;
mod guard;
mod hub;
mod keys;
mod pages;
mod ranges;
mod reading;
mod relay;
mod rooms;
mod searches;
mod serve;
mod store;
mod web;

mod words {
    include!(concat!(env!("OUT_DIR"), "/words.rs"));
}

#[cfg(fuzzing)]
pub fn read_a_request(request: &[u8], total: u64) {
    if let Some(asked) = pages::asked_of(request) {
        let _ = pages::answer(asked, &[]);
    }
    gateway::read_a_head(request, total);
    if let Ok(text) = std::str::from_utf8(request) {
        for line in text.lines() {
            let wanted = ranges::Wanted::parse(line);
            if let Ok(Some(span)) = ranges::windowed(wanted, Some(total)) {
                assert!(span.first <= span.last);
                assert!(span.last < total);
            }
        }
    }
}

pub use crate::{
    cache::{Cache, Fill, Hit},
    error::{CacheOp, ConfigField, Error, PemOp, Result, StoreOp},
    gateway::{Crowd, Gateway, Keeping, gateway_tls},
    guard::{Admitted, Allowance, Guard},
    keys::{Digest, Hifi, Keys, Minted},
    searches::{Recalled, Searches},
    serve::{Limits, Server, listening, pem, serving, tls},
    store::{Capacity, Store},
    web::Web,
};

#[cfg(test)]
mod said_through_the_catalog {
    use std::{fs, path::Path};

    const PROPER: [&str; 4] = [
        "200 OK",
        "404 Not Found",
        "text/html; charset=utf-8",
        "text/plain; charset=utf-8",
    ];

    #[test]
    fn every_word_an_invitation_page_says_is_said_through_its_catalog() {
        let source = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/pages.rs"))
            .expect("the pages' source");

        let written: Vec<String> = resonate_words_build::prose_of(&source)
            .into_iter()
            .filter(|said| !PROPER.contains(&said.text.as_str()))
            .filter(|said| {
                !said.text.starts_with("<!doctype") && !said.text.starts_with("HTTP/1.1 ")
            })
            .map(|said| format!("pages.rs:{} {:?}", said.line, said.text))
            .collect();

        assert!(
            written.is_empty(),
            "words written in the code:\n{}",
            written.join("\n")
        );
    }
}
