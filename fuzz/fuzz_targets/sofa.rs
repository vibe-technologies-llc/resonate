#![no_main]

use std::time::Duration;

use libfuzzer_sys::fuzz_target;

const LONGEST: Duration = Duration::from_secs(1);

fuzz_target!(|bytes: &[u8]| {
    let _ = resonate_codec::read_hrir_bytes(bytes.to_vec(), LONGEST);
});
