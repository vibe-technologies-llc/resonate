use std::{fs::File, path::Path, time::SystemTime};

use resonate_core::writer;

pub(crate) fn left_long_ago(path: &Path) {
    let long_ago = SystemTime::now() - writer::UNHELD_AND_UNTOUCHED_FOR * 2;
    File::options()
        .write(true)
        .open(path)
        .and_then(|file| file.set_modified(long_ago))
        .expect("a file left long ago");
}
