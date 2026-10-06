#![no_main]

use libfuzzer_sys::fuzz_target;
use resonate_eq::{read_profile, write_profile};

fuzz_target!(|text: &str| {
    if let Ok(profile) = read_profile(text) {
        let written = write_profile(&profile);
        assert_eq!(read_profile(&written).ok(), Some(profile), "{written}");
    }
});
