#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    resonate_codec::rate_a_file(bytes);
});
