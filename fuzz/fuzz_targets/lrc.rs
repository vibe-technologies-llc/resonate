#![no_main]

mod bounded;

use libfuzzer_sys::fuzz_target;

const SET_AT_MOST: usize = 1024 * 1024;

fn grown_by_spaced_brackets(input: usize) -> usize {
    input + input / 2
}

fuzz_target!(|text: &str| {
    if let Some(lyrics) = resonate_lyrics::read_an_lrc_sheet(text) {
        bounded::within(&lyrics, grown_by_spaced_brackets(text.len()), SET_AT_MOST);
    }
});
