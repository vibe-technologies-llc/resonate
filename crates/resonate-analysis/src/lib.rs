mod analysis;
mod envelope;
mod error;
mod kept;
mod levels;
mod loudness;
mod print;
mod reduce;
mod signature;
mod spectrogram;
mod spectrum;
mod verdict;
mod watch;

pub use crate::{
    analysis::{Analysis, Examined, Excerpt, Study, analyse, excerpt, print, study},
    envelope::{ENVELOPE_COLUMNS, ENVELOPE_LANES, Envelope, Reach},
    error::{AnalysisOp, Error, Result},
    kept::{KEPT_BYTES_AT_MOST, KeptAnalyses},
    levels::{Levels, Stereo},
    loudness::Loudness,
    print::PRINTED_FOR,
    signature::{Signature, signature_of},
    spectrogram::{Ramp, SPECTROGRAM_COLUMNS, SPECTROGRAM_FLOOR_DB, SPECTROGRAM_ROWS, Spectrogram},
    spectrum::{FLOOR_DB, Spectrum},
    verdict::{Cutoff, Finding, JUDGED_UNDER, Judgement, LossyGuess, Verdict},
    watch::{Watch, Watching},
};
