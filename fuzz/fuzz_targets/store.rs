#![no_main]

use std::{
    fs,
    path::{Path, PathBuf},
    process,
};

use libfuzzer_sys::fuzz_target;
use resonate_core::eq::{BOOST_AT_MOST_DB, FITTED_AT};
use resonate_eq::{Error, Store};

mod stage;

fn file_of_this_run() -> PathBuf {
    std::env::temp_dir().join(format!("resonate-fuzz-store-{}.txt", process::id()))
}

fn read_back(path: &Path, bytes: &[u8]) {
    match Store::read_in(path) {
        Ok(kept) => {
            assert_eq!(kept.bytes, bytes);
            assert!(kept.profile.loudest_lift_db(FITTED_AT) <= f64::from(BOOST_AT_MOST_DB));
            assert!(!kept.profile.is_transparent() || kept.profile.target().is_some());

            stage::holds_under_full_scale(&kept.profile);
        }
        Err(Error::PeakTooHigh { peak_db, .. }) => {
            assert!(peak_db > f64::from(BOOST_AT_MOST_DB));
        }
        Err(_) => {}
    }
}

fuzz_target!(|bytes: &[u8]| {
    let path = file_of_this_run();

    fs::write(&path, bytes).expect("a file in the temporary folder");
    read_back(&path, bytes);
    fs::remove_file(&path).expect("the file just written");
});
