mod backend;
mod catalog;
mod command;
mod engine;
mod error;
mod heard;
mod kept;
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
    FLOOR_DB, Finding, Judgement, Levels, LossyGuess, Loudness, Ramp, Reach, SPECTROGRAM_ROWS,
    Spectrogram, Spectrum, Stereo, Study, Verdict, Watch, Watching, analyse, study,
};
pub use resonate_codec::{
    BitRate, BoxKind, BoxLayout, Codec, Container, CoverArt, Credits, Drawing, DsdRate, Faststart,
    FormatHint, Hinting, ImageFormat, LocalFiles, Media, MediaInfo, MediaProvider, MediaStream,
    PacketSpan, Packing, Percentiles, Reading, ReplayGain, Sources, StreamProfile, TagSet,
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
    DitherKind, FilterPhase, NoiseShaping, Quality, ReplayGainMode, Restoration, SincParams, Tuning,
};
pub use resonate_pipewire::{
    AudioSource, Error as SinkError, HardwareVolume, LatencyRequest, MediaRole, NodeName, Plugged,
    SinkChange, SinkFormats, SinkId, SinkInfo, SinkPort, SinkStream, StreamCommand, StreamEvent,
    StreamRequest, StreamState, Words,
};

pub use crate::{
    backend::{Backend, SinkResult, Surveyor},
    catalog::{ArtRead, TagsRead},
    command::{Command, CommandKind, Landing, Outcome, RepeatMode, SkipUnderRepeat},
    error::{Error, Result},
    heard::{A_SEEK, COUNTS_AS_HEARD, Counting, Listening, Played},
    kept::{KEPT_EVERY, Keep, Keeping},
    pipeline::{
        BluetoothWake, Decoded, EngineConfig, Equalisation, Levelling, OutputMode, OutputPlan,
        plan_for, plan_output, resolve_replay_gain,
    },
    player::Player,
    queue::{Placement, QueueItem, Queued, Unclaimed, stamp_of, unclaimed_id},
    ring::{RingConsumer, RingMonitor, RingProducer, ring},
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
