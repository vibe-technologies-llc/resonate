#![no_main]

use libfuzzer_sys::fuzz_target;
use resonate_core::text;

fuzz_target!(|bytes: &[u8]| {
    let (decoded, encoding) = text::decoded(bytes);
    let _ = text::encoded(&decoded, encoding);
    let _ = text::decoded_as(bytes, text::detected(bytes));
});
