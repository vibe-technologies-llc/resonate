#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    resonate_mpris::read_what_a_peer_says(bytes);
});
