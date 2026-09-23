#![no_main]

mod held;

use libfuzzer_sys::fuzz_target;
use resonate_codec::probe_boxes;

fuzz_target!(|bytes: &[u8]| {
    let (sources, location) = held::held(bytes);
    let _ = probe_boxes(&sources, &location);
});
