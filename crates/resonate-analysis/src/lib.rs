mod analysis;
mod bands;
mod conform;
mod envelope;
mod error;
mod granules;
mod headroom;
mod kept;
mod levels;
mod loudness;
mod print;
mod reduce;
mod signature;
mod spectrogram;
mod spectrum;
mod split;
mod steps;
mod verdict;
mod watch;
mod weighing;

#[cfg(fuzzing)]
pub fn read_a_kept_analysis(bytes: &[u8]) {
    let _ = crate::kept::read(bytes);
}

pub use crate::{
    analysis::{Analysis, Examined, Excerpt, Study, analyse, excerpt, print, study},
    bands::{
        BAND_PEAKS_BYTES, BANDS, BandFigures, BandGains, BandLevels, BandPeaks, band_figures,
        span_of_band,
    },
    envelope::{ENVELOPE_COLUMNS, ENVELOPE_LANES, Envelope, Reach},
    error::{AnalysisOp, Error, Result},
    headroom::{CUTS_AT_MOST, Cut, Loudest, loudest_through, peak_through},
    kept::{KEPT_BYTES_AT_MOST, KeptAnalyses},
    levels::{Levels, Stereo},
    loudness::Loudness,
    print::PRINTED_FOR,
    signature::{Signature, signature_of},
    spectrogram::{Ramp, SPECTROGRAM_COLUMNS, SPECTROGRAM_FLOOR_DB, SPECTROGRAM_ROWS, Spectrogram},
    spectrum::{FLOOR_DB, Spectrum},
    steps::{Coding, Steps},
    verdict::{Cutoff, Finding, JUDGED_UNDER, Judgement, LossyGuess, Verdict},
    watch::{Watch, Watching},
};
