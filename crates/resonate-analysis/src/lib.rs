mod analysis;
mod applied;
mod bands;
mod celt;
mod coinciding;
mod conform;
mod crew;
mod envelope;
mod error;
mod granules;
mod headroom;
mod kept;
mod levels;
mod loudness;
mod peaks;
mod pitch;
mod print;
mod reduce;
mod relay;
mod signature;
mod spectrogram;
mod spectrum;
mod split;
mod steps;
mod subbands;
mod tones;
mod verdict;
mod watch;
mod weighing;

mod words {
    include!(concat!(env!("OUT_DIR"), "/words.rs"));
}

#[cfg(fuzzing)]
pub fn read_a_kept_analysis(bytes: &[u8]) {
    let _ = crate::kept::read(bytes);
}

pub use crate::{
    analysis::{Analysis, Examined, Excerpt, Study, analyse, excerpt, print, study},
    applied::Applied,
    bands::{BANDS, BandFigures, BandGains, BandLevels, band_figures, span_of_band},
    envelope::{ENVELOPE_COLUMNS, ENVELOPE_LANES, Envelope, Reach},
    error::{AnalysisOp, Error, Result},
    headroom::{CUTS_AT_MOST, Cut, Loudest, loudest_through, peak_through},
    kept::{KEPT_BYTES_AT_MOST, KeptAnalyses},
    levels::{Levels, Stereo},
    loudness::{Loudness, LoudnessMeter},
    peaks::{BAND_PEAKS_BYTES, BAND_PEAKS_BYTES_BEFORE_TOGETHER, BandPeaks},
    print::PRINTED_FOR,
    signature::{Signature, signature_of},
    spectrogram::{Ramp, SPECTROGRAM_COLUMNS, SPECTROGRAM_FLOOR_DB, SPECTROGRAM_ROWS, Spectrogram},
    spectrum::{FLOOR_DB, Spectrum},
    steps::{Coding, Steps, Tell},
    verdict::{Cutoff, Finding, JUDGED_UNDER, Judgement, LossyGuess, Verdict},
    watch::{Watch, Watching},
};

#[cfg(test)]
mod said_through_the_catalog {
    use std::path::Path;

    const PROPER: &[&str] = &["Vorbis", "Opus", "quantiser steps", "intensity stereo"];

    #[test]
    fn every_word_a_study_tells_a_listener_is_said_through_its_catalog() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let found = resonate_words_build::prose_in(&root, PROPER).expect("the sources read");

        let written: Vec<String> = found
            .iter()
            .flat_map(|(path, prose)| {
                prose
                    .iter()
                    .map(move |said| format!("{}:{} {:?}", path.display(), said.line, said.text))
            })
            .collect();

        assert!(
            written.is_empty(),
            "words written in the code:\n{}",
            written.join("\n")
        );
    }
}
