#![no_main]

use libfuzzer_sys::fuzz_target;
use resonate_musicbrainz::{Answer, Asked};

fuzz_target!(|bytes: &[u8]| {
    if let Ok(asked) = Asked::decoded(bytes) {
        assert_eq!(Asked::decoded(&asked.encoded()).ok(), Some(asked));
    }
    if let Ok(answer) = Answer::decoded(bytes) {
        assert_eq!(Answer::decoded(&answer.encoded()).ok(), Some(answer));
    }
    resonate_musicbrainz::read_a_copy_line(bytes);
});
