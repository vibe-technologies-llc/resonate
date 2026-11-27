#![no_main]

mod bounded;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    if let Some(lyrics) = resonate_lyrics::read_a_lyricsfile(text) {
        bounded::within(&lyrics, text.len(), text.len());
    }
});
