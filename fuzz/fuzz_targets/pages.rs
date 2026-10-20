#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    resonate_online::read_a_shared_page(text);
});
