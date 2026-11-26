#![no_main]

use libfuzzer_sys::fuzz_target;
use resonate_eq::{read_profile, write_profile};

mod stage;

fuzz_target!(|text: &str| {
    if let Ok(profile) = read_profile(text) {
        let written = write_profile(&profile);
        assert_eq!(
            read_profile(&written).ok(),
            Some(profile.clone()),
            "{written}"
        );

        stage::holds_under_full_scale(&profile);
    }
});
