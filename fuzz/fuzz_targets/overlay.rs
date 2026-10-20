#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    resonate_codec::edit_through_an_overlay(bytes);
});
