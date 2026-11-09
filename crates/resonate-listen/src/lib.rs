mod error;
mod heard;
#[cfg(any(target_os = "linux", windows))]
mod hearing;
#[cfg(target_os = "linux")]
mod listener;
#[cfg(any(target_os = "linux", windows))]
mod recording;
#[cfg(any(target_os = "linux", windows))]
mod session;

#[cfg(any(windows, test))]
mod endpoints;
#[cfg(windows)]
mod wasapi_apartment;
#[cfg(windows)]
mod wasapi_capture;
#[cfg(windows)]
mod wasapi_listener;
#[cfg(windows)]
mod wasapi_playback;

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod words {
    include!(concat!(env!("OUT_DIR"), "/words.rs"));
}

#[cfg(windows)]
pub use crate::endpoints::Microphone;
#[cfg(any(target_os = "linux", windows))]
pub use crate::hearing::{CLIP_BY_DEFAULT, Hearing, Listening};
#[cfg(target_os = "linux")]
pub use crate::listener::Listener;
#[cfg(any(target_os = "linux", windows))]
pub use crate::recording::Recording;
#[cfg(windows)]
pub use crate::wasapi_listener::Listener;
pub use crate::{
    error::{CaptureOp, Error, Result},
    heard::{Clip, Heard, Picture, PictureFormat, Recogniser, Recognisers, Recognition},
};

#[cfg(test)]
mod said_through_the_catalog {
    use std::{fs, path::Path};

    #[test]
    fn every_word_a_recording_is_named_by_is_said_through_its_catalog() {
        let source =
            fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/listener.rs"))
                .expect("the listener's source");

        let written: Vec<String> = resonate_words_build::prose_of(&source)
            .into_iter()
            .map(|said| format!("listener.rs:{} {:?}", said.line, said.text))
            .collect();

        assert!(
            written.is_empty(),
            "words written in the code:\n{}",
            written.join("\n")
        );
    }
}
