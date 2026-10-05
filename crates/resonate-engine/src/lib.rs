mod backend;
mod catalog;
mod command;
mod engine;
mod error;
mod heard;
mod impulse;
mod kept;
mod lending;
mod measure;
mod pipeline;
mod player;
mod queue;
mod ring;
mod seed;
mod sleep;
mod state;
mod surveying;
mod tap;

pub use resonate_analysis::{
    Analysis, AnalysisOp, Cutoff, ENVELOPE_LANES, Envelope, Error as AnalysisError, Examined,
    FLOOR_DB, Finding, Judgement, KeptAnalyses, Levels, LossyGuess, Loudness, Ramp, Reach,
    SPECTROGRAM_ROWS, Spectrogram, Spectrum, Stereo, Study, Verdict, Watch, Watching, analyse,
    study,
};
pub use resonate_codec::{
    BitRate, BoxKind, BoxLayout, Codec, Container, CoverArt, Credits, Drawing, DsdRate, Faststart,
    FormatHint, Hinting, ImageFormat, LocalFiles, Media, MediaInfo, MediaProvider, MediaStream,
    PacketSpan, Packing, Percentiles, Raster, Reading, ReplayGain, Sources, StreamProfile, TagSet,
    TopLevelBox, Variability, WINDOW,
};
pub use resonate_core::{
    Locator, MediaLocation, Resumable, Resumption, SourceId, Span,
    eq::{
        Band, BandGain, BandKind, Biquad, Frequency, MAX_BANDS, Preamp, Profile, Q,
        RESPONSE_POINTS, sweep,
    },
};
pub use resonate_dsp::{
    DitherKind, FilterPhase, Impulse, NoiseShaping, Quality, ReplayGainMode, Restoration,
    SincParams, Tuning,
};
pub use resonate_pipewire::{
    AudioSource, CardProfile, Error as SinkError, GraphTime, HardwareVolume, LatencyRequest,
    MediaRole, NodeName, Plugged, ProfileIndex, SinkChange, SinkFormats, SinkId, SinkInfo,
    SinkPort, SinkStream, StreamClock, StreamCommand, StreamEvent, StreamRequest, StreamState,
    Words,
};

pub use crate::{
    backend::{Backend, SinkResult, Surveyor},
    catalog::{ArtRead, TagsRead},
    command::{
        Command, CommandKind, Landing, Outcome, PreviousRestarts, RepeatMode, SkipUnderRepeat,
    },
    error::{Cause, Error, Result},
    heard::{A_SEEK, COUNTS_AS_HEARD, Counting, Listening, Played},
    impulse::{LONGEST_IMPULSE, read_impulse},
    kept::{KEPT_EVERY, Keep, Keeping},
    pipeline::{
        BluetoothWake, Decoded, EngineConfig, Equalisation, Levelling, OutputMode, OutputPlan,
        plan_for, plan_output, resolve_replay_gain,
    },
    player::Player,
    queue::{Placement, QueueItem, Queued, Unclaimed, stamp_of, unclaimed_id},
    ring::{Entering, FADED_OVER, RingConsumer, RingMonitor, RingProducer, ring},
    sleep::{Asleep, Until},
    state::{
        Event, OutputSettings, OutputStatus, PlaybackState, PlayerState, Published, Seeks,
        StreamDigest, TrackState, TransportState,
    },
    tap::{Caught, Tap, Tapped},
};
pub(crate) use crate::{
    command::{Reply, Request},
    engine::Engine,
    sleep::Sleeping,
    tap::Tapping,
};
