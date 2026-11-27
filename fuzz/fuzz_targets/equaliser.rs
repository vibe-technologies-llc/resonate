#![no_main]

use libfuzzer_sys::fuzz_target;
use resonate_core::eq::{BOOST_AT_MOST_DB, FITTED_AT};
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

        assert!(profile.loudest_lift_db(FITTED_AT) <= f64::from(BOOST_AT_MOST_DB));

        stage::holds_under_full_scale(&profile);
    }
});
