#![no_main]

mod held;

use libfuzzer_sys::fuzz_target;
use resonate_codec::{read_cue, read_cue_media, renamed_cue};
use resonate_core::SampleRate;

fuzz_target!(|bytes: &[u8]| {
    let sheet = read_cue(bytes);
    for file in &sheet.files {
        let _ = renamed_cue(bytes, &file.named, "renamed.flac");
        for index in 0..file.tracks.len() {
            for rate in [SampleRate::HZ_44100, SampleRate::HZ_384000] {
                let _ = file.span_of(index, rate, None);
            }
        }
    }

    let (sources, location) = held::held(bytes);
    let _ = read_cue_media(&sources, &location);
});
