use std::{
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use parking_lot::RwLock;
use resonate_codec::{BoxLayout, MediaInfo, PacketSpan, StreamProfile};
use resonate_core::{
    AppliedGain, FrameSpan, Frames, MediaLocation, QueueStamp, StreamSpec, TrackId, Volume,
};
use resonate_pipewire::{NodeName, SinkId, SinkInfo, Words};

use crate::{
    Asleep, BluetoothWake, CommandKind, DitherKind, EngineConfig, Equalisation, Error, FilterPhase,
    Levelling, NoiseShaping, OutputMode, PreviousRestarts, Quality, Queued, RepeatMode,
    ReplayGainMode, Restoration, SkipUnderRepeat, Tapped,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlaybackState {
    #[default]
    Idle,
    Buffering,
    Playing,
    Paused,
    Stopped,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TransportState {
    #[default]
    Idle,
    Loading,
    Playing,
    Paused,
    Draining,
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackState {
    pub id: TrackId,
    pub source: StreamSpec,
    pub position: Frames,
    pub duration: Option<Frames>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Seeks(u64);

impl Seeks {
    pub const fn stepped(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutputStatus {
    pub sink: SinkId,
    pub negotiated: StreamSpec,
    pub words: Option<Words>,
    pub mode: OutputMode,
    pub latency: Frames,
    pub underruns: u64,
    pub went_without: Frames,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayerState {
    pub playback: PlaybackState,
    pub current: Option<TrackState>,
    pub volume: Volume,
    pub repeat: RepeatMode,
    pub skip_under_repeat: SkipUnderRepeat,
    pub previous_restarts: PreviousRestarts,
    pub shuffle: bool,
    pub queue_position: Option<usize>,
    pub loaded_position: Option<usize>,
    pub queue_len: usize,
    pub queue_stamp: QueueStamp,
    pub seeks: Seeks,
    pub sleeping: Option<Asleep>,
    pub output: Option<OutputStatus>,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            playback: PlaybackState::Idle,
            current: None,
            volume: Volume::MAX,
            repeat: RepeatMode::Off,
            skip_under_repeat: SkipUnderRepeat::default(),
            previous_restarts: PreviousRestarts::default(),
            shuffle: false,
            queue_position: None,
            loaded_position: None,
            queue_len: 0,
            queue_stamp: QueueStamp::default(),
            seeks: Seeks::default(),
            sleeping: None,
            output: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputSettings {
    pub sink: Option<NodeName>,
    pub quality: Quality,
    pub filter_phase: FilterPhase,
    pub true_peak: bool,
    pub restoration: Restoration,
    pub dither: DitherKind,
    pub noise_shaping: NoiseShaping,
    pub replay_gain: ReplayGainMode,
    pub levelling: Levelling,
    pub prefer_bit_perfect: bool,
    pub dop: bool,
    pub force_graph_rate: bool,
    pub bluetooth: BluetoothWake,
    pub buffer: Duration,
    pub equaliser: Arc<Equalisation>,
}

impl OutputSettings {
    pub fn of(config: &EngineConfig) -> Self {
        Self {
            sink: config.sink.clone(),
            quality: config.quality,
            filter_phase: config.filter_phase,
            true_peak: config.true_peak,
            restoration: config.restoration,
            dither: config.dither,
            noise_shaping: config.noise_shaping,
            replay_gain: config.replay_gain,
            levelling: config.levelling,
            prefer_bit_perfect: config.prefer_bit_perfect,
            dop: config.dop,
            force_graph_rate: config.force_graph_rate,
            bluetooth: config.bluetooth,
            buffer: config.buffer,
            equaliser: Arc::clone(&config.equaliser),
        }
    }

    pub fn already_says(&self, config: &EngineConfig) -> bool {
        self.sink == config.sink
            && self.quality == config.quality
            && self.filter_phase == config.filter_phase
            && self.true_peak == config.true_peak
            && self.restoration == config.restoration
            && self.dither == config.dither
            && self.noise_shaping == config.noise_shaping
            && self.replay_gain == config.replay_gain
            && self.levelling == config.levelling
            && self.prefer_bit_perfect == config.prefer_bit_perfect
            && self.dop == config.dop
            && self.force_graph_rate == config.force_graph_rate
            && self.bluetooth == config.bluetooth
            && self.buffer == config.buffer
            && self.equaliser == config.equaliser
    }
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self::of(&EngineConfig::default())
    }
}

#[derive(Clone, Default)]
pub struct Published {
    pub state: Arc<RwLock<PlayerState>>,
    pub settings: Arc<RwLock<Arc<OutputSettings>>>,
    pub digest: Arc<RwLock<Option<Arc<StreamDigest>>>>,
    pub queue: Arc<RwLock<Queued>>,
    pub sinks: Arc<RwLock<Arc<[SinkInfo]>>>,
    pub tap: Arc<RwLock<Tapped>>,
    pub listening: Arc<AtomicBool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StreamDigest {
    pub track: TrackId,
    pub location: MediaLocation,
    pub span: Option<FrameSpan>,
    pub info: Arc<MediaInfo>,
    pub layout: Option<Arc<BoxLayout>>,
    pub replay_gain_mode: ReplayGainMode,
    pub replay_gain: AppliedGain,
    pub profiled_from: Frames,
    pub decoded: Frames,
    pub packet: Option<PacketSpan>,
    pub profile: Option<StreamProfile>,
}

#[derive(Debug)]
pub enum Event {
    TrackStarted(TrackId),
    TrackFinished(TrackId),
    QueueFinished,
    OutputChanged(OutputStatus),
    Underrun { missing: Frames },
    Failed { track: TrackId, error: Error },
    CommandFailed { command: CommandKind, error: Error },
}

#[cfg(test)]
mod tests {
    use resonate_core::eq::{Band, BandGain, BandKind, Frequency, Preamp, Profile, Q};
    use resonate_dsp::Quality;
    use resonate_pipewire::NodeName;

    use super::*;

    fn every_field_moved(config: &mut EngineConfig) -> Vec<&'static str> {
        let mut moved = Vec::new();
        let mut note = |named, config: &EngineConfig| {
            if !OutputSettings::of(&EngineConfig::default()).already_says(config) {
                moved.push(named);
            }
        };

        config.sink = Some(NodeName::new("a sink of another name"));
        note("sink", config);
        *config = EngineConfig::default();

        config.quality = Quality::Fast;
        note("quality", config);
        *config = EngineConfig::default();

        config.filter_phase = FilterPhase::Minimum;
        note("filter_phase", config);
        *config = EngineConfig::default();

        config.true_peak = !EngineConfig::default().true_peak;
        note("true_peak", config);
        *config = EngineConfig::default();

        config.restoration = Restoration::Extend;
        note("restoration", config);
        *config = EngineConfig::default();

        config.dither = DitherKind::None;
        note("dither", config);
        *config = EngineConfig::default();

        config.noise_shaping = NoiseShaping::None;
        note("noise_shaping", config);
        *config = EngineConfig::default();

        config.replay_gain = ReplayGainMode::Track;
        note("replay_gain", config);
        *config = EngineConfig::default();

        config.prefer_bit_perfect = !EngineConfig::default().prefer_bit_perfect;
        note("prefer_bit_perfect", config);
        *config = EngineConfig::default();

        config.dop = !EngineConfig::default().dop;
        note("dop", config);
        *config = EngineConfig::default();

        config.bluetooth.on = !EngineConfig::default().bluetooth.on;
        note("bluetooth", config);
        *config = EngineConfig::default();

        config.force_graph_rate = !EngineConfig::default().force_graph_rate;
        note("force_graph_rate", config);
        *config = EngineConfig::default();

        config.buffer = EngineConfig::default().buffer + Duration::from_millis(1);
        note("buffer", config);
        *config = EngineConfig::default();

        config.equaliser = Arc::new(Equalisation {
            enabled: true,
            fallback: Some(Arc::new(
                Profile::new(
                    Preamp::NONE,
                    vec![Band::new(
                        BandKind::Peaking,
                        Frequency::from_centihertz(100_000).expect("a valid frequency"),
                        BandGain::from_millibels(300).expect("a valid gain"),
                        Q::BUTTERWORTH,
                    )],
                )
                .expect("a valid profile"),
            )),
            ..Equalisation::default()
        });
        note("equaliser", config);
        *config = EngineConfig::default();

        moved
    }

    #[test]
    fn every_published_setting_is_one_already_says_compares() {
        let mut config = EngineConfig::default();
        assert!(
            OutputSettings::of(&config).already_says(&config),
            "a settings value built from a config must agree with it"
        );

        let moved = every_field_moved(&mut config);
        assert_eq!(
            moved.len(),
            14,
            "a field OutputSettings publishes but already_says does not compare would stop the \
             engine publishing it; noticed {moved:?}"
        );
    }
}
