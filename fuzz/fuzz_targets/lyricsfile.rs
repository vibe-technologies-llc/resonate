#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    resonate_lyrics::read_a_lyricsfile(text);
});
