#![no_main]

mod held;

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use resonate_codec::{
    CueNaming, read_cue, read_cue_media, renamed_cue, the_best_a_cue_names, the_folder_a_cue_names,
    the_one_a_cue_names,
};
use resonate_core::SampleRate;

const NOWHERE: &str = "/nonexistent-resonate-fuzz";

fn listed_beside(named: &str) -> Vec<String> {
    let alone = named.rsplit(['/', '\\']).next().unwrap_or(named);
    let stem = alone.rsplit_once('.').map_or(alone, |(stem, _)| stem);

    vec![
        alone.to_owned(),
        alone.to_uppercase(),
        format!("{stem}.flac"),
        format!("{stem}.WAV"),
        format!("{alone}.flac"),
    ]
}

fuzz_target!(|bytes: &[u8]| {
    let sheet = read_cue(bytes);
    for file in &sheet.files {
        let _ = renamed_cue(bytes, &file.named, "renamed.flac");
        for index in 0..file.tracks.len() {
            for rate in [SampleRate::HZ_44100, SampleRate::HZ_384000] {
                let _ = file.span_of(index, rate, None);
            }
        }

        let listed = listed_beside(&file.named);
        let _ = the_one_a_cue_names(
            &file.named,
            listed
                .iter()
                .enumerate()
                .map(|(at, name)| (at, name.as_str())),
        );
        let _ = the_best_a_cue_names(
            listed
                .iter()
                .map(|name| (name, CueNaming::of(&file.named, name))),
        );
        let _ = the_folder_a_cue_names(Path::new(NOWHERE), &file.named);
    }

    let (sources, location) = held::held(bytes);
    let _ = read_cue_media(&sources, &location);
});
