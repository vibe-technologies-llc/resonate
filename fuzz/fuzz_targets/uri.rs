#![no_main]

use libfuzzer_sys::fuzz_target;
use resonate_core::MediaLocation;

fuzz_target!(|uri: &str| {
    if let Some((location, span)) = MediaLocation::from_uri_within(uri) {
        let written = location.to_uri_within(span);
        let _ = MediaLocation::from_uri_within(&written);
    }
    let _ = MediaLocation::from_uri(uri);
});
