#[cfg(target_os = "macos")]
mod backend;
mod clock;
#[cfg(target_os = "macos")]
mod device;
mod drain;
mod fill;
mod formats;
#[cfg(target_os = "macos")]
mod opening;
mod remembered;
mod serving;
mod survey;
mod ways;

mod words {
    include!(concat!(env!("OUT_DIR"), "/words.rs"));
}

#[cfg(target_os = "macos")]
pub use crate::backend::CoreAudio;
pub use crate::{
    clock::Latencies,
    drain::Drain,
    fill::{HostNanos, Laid, Presented, Rendering, Shaped},
    formats::{
        ClientWord, Depth, Physical, RATES_A_RANGE_IS_CROSSED_WITH, frame_of, hog_formats, laid_of,
        layout_of, physical_for, rate_of, shared_formats, words_of,
    },
    remembered::Remembered,
    serving::{Hardware, Looks, Standing, launch},
    survey::{Level, Seen, sinks_of},
    ways::{Way, device_of, node_of},
};

#[cfg(test)]
mod said_through_the_catalog {
    use std::{fs, path::Path};

    #[test]
    fn every_word_a_device_is_shown_by_is_said_through_its_catalog() {
        let source =
            fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/survey.rs"))
                .expect("the survey's source");

        let written: Vec<String> = resonate_words_build::prose_of(&source)
            .into_iter()
            .map(|said| format!("survey.rs:{} {:?}", said.line, said.text))
            .collect();

        assert!(
            written.is_empty(),
            "words written in the code:\n{}",
            written.join("\n")
        );
    }
}
