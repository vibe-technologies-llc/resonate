#![no_main]

use libfuzzer_sys::fuzz_target;
use resonate_library::{Error, heard_in_lines};

fuzz_target!(|bytes: &[u8]| {
    match heard_in_lines(bytes) {
        Ok(heard) => {
            let lines = bytes.split(|byte| *byte == b'\n').count();
            assert!(heard.len() <= lines);
            for listen in &heard {
                assert!(!listen.recording.title.trim().is_empty());
                assert!(!listen.recording.artist.trim().is_empty());
            }
        }
        Err(Error::ListenLine { line, .. }) => {
            let lines = bytes.split(|byte| *byte == b'\n').count();
            assert!((1..=lines).contains(&line));
        }
        Err(other) => panic!("a reader of lines answered {other:?}"),
    }
});
