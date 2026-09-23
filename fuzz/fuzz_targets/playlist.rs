#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    resonate_library::read_a_playlist_sheet(text);
});
