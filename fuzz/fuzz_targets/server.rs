#![no_main]

use libfuzzer_sys::fuzz_target;

const TOTAL_BYTES: usize = 8;

fuzz_target!(|bytes: &[u8]| {
    let Some((total, request)) = bytes.split_first_chunk::<TOTAL_BYTES>() else {
        return;
    };
    resonate_server::read_a_request(request, u64::from_le_bytes(*total));
});
