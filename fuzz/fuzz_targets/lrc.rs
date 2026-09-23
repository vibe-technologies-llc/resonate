#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    resonate_lyrics::read_an_lrc_sheet(text);
});
