use std::{collections::BTreeMap, sync::Arc, time::Duration};

use resonate_codec::{Codec, Delivery, DsdRate, MediaInfo, Packing, ReplayGain};
use resonate_core::{
    AppliedGain, BitDepth, ChannelLayout, Gain, SampleFormat, SampleRate, Silence, StreamSpec,
    Trim, Volume,
    eq::{Frequency, Profile},
};
use resonate_dsp::{
    Chain, Dither, DitherKind, Equaliser, FilterPhase, GainConfig, GainStage, NoiseShaping,
    Quality, Remix, ReplayGainMode, Resampler, ResamplerConfig, Restoration, Restore,
    RestoreConfig, Result, TruePeak, Tuning,
};
use resonate_pipewire::{NodeName, SinkInfo};

use crate::{SkipUnderRepeat, seed};

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Equalisation {
    pub enabled: bool,
    pub bound: BTreeMap<NodeName, Arc<Profile>>,
    pub fallback: Option<Arc<Profile>>,
}

impl Equalisation {
    pub fn for_sink(&self, sink: &NodeName) -> Option<&Arc<Profile>> {
        if !self.enabled {
            return None;
        }
        self.bound.get(sink).or(self.fallback.as_ref())
    }

    pub fn binds_anything(&self) -> bool {
        !self.bound.is_empty() || self.fallback.is_some()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EngineConfig {
    pub app_name: String,
    pub sink: Option<NodeName>,
    pub buffer: Duration,
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
    pub volume: Volume,
    pub equaliser: Arc<Equalisation>,
    pub skip_under_repeat: SkipUnderRepeat,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Levelling {
    pub pre_amp: Trim,
    pub untagged: Trim,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            app_name: "Resonate".to_owned(),
            sink: None,
            buffer: Duration::from_millis(500),
            quality: Quality::High,
            filter_phase: FilterPhase::Linear,
            true_peak: true,
            restoration: Restoration::Off,
            dither: DitherKind::Triangular,
            noise_shaping: NoiseShaping::Threshold,
            replay_gain: ReplayGainMode::Off,
            levelling: Levelling::default(),
            prefer_bit_perfect: true,
            dop: false,
            force_graph_rate: true,
            volume: Volume::MAX,
            equaliser: Arc::new(Equalisation::default()),
            skip_under_repeat: SkipUnderRepeat::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OutputMode {
    BitPerfect,
    Repacked,
    Dithered,
    Converted,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decoded {
    pub spec: StreamSpec,
    pub packing: Packing,
    pub tuning: Option<Tuning>,
    pub lowpass: Option<Frequency>,
}

impl Decoded {
    pub const fn samples(spec: StreamSpec) -> Self {
        Self {
            spec,
            packing: Packing::Samples,
            tuning: None,
            lowpass: None,
        }
    }

    pub fn of(info: &MediaInfo, lowpass: Option<Frequency>) -> Self {
        Self {
            spec: info.spec,
            packing: info.packing,
            tuning: tuning_of(Codec::from_id(info.codec)),
            lowpass,
        }
    }
}

pub const fn tuning_of(codec: Codec) -> Option<Tuning> {
    match codec {
        Codec::Mp3 => Some(Tuning::Mp3),
        Codec::Aac => Some(Tuning::Aac),
        Codec::Vorbis => Some(Tuning::Vorbis),
        Codec::Flac | Codec::Alac | Codec::Dsd | Codec::Pcm | Codec::Opus | Codec::Unknown => None,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutputPlan {
    pub stream: StreamSpec,
    pub mode: OutputMode,
    pub packing: Packing,
    pub remix: Option<(ChannelLayout, ChannelLayout)>,
    pub restoration: Option<RestoreConfig>,
    pub resample: Option<(SampleRate, SampleRate)>,
    pub equalisation: Option<Arc<Profile>>,
    pub dither_to: Option<BitDepth>,
    pub shaping: NoiseShaping,
    pub gain: Option<GainConfig>,
    pub rounds: bool,
    pub decoded_as: SampleFormat,
    pub true_peak: bool,
}

impl OutputPlan {
    pub fn is_transparent(&self) -> bool {
        self.remix.is_none()
            && self.restoration.is_none()
            && self.resample.is_none()
            && self.equalisation.is_none()
            && self.gain.is_none()
            && self.dither_to.is_none()
            && !self.rounds
            && !self.true_peak
    }

    pub fn delivery(&self) -> Delivery {
        match self.packing {
            Packing::DopMarked(rate) => Delivery {
                format: SampleFormat::S24,
                packing: Packing::DopMarked(rate),
            },
            Packing::Samples if self.is_transparent() => Delivery::samples(self.stream.format),
            Packing::Samples => Delivery::samples(self.decoded_as),
        }
    }

    pub fn silence(&self) -> Silence {
        self.packing.silence(self.stream.format)
    }

    pub fn becomes_on_the_same_stream(&self, wanted: &Self) -> bool {
        self.resample.is_none()
            && self.resample == wanted.resample
            && self.stream == wanted.stream
            && self.packing == wanted.packing
            && self.remix == wanted.remix
    }

    pub fn same_shape_as(&self, other: &Self) -> bool {
        self.stream == other.stream
            && self.mode == other.mode
            && self.packing == other.packing
            && self.remix == other.remix
            && self.restoration == other.restoration
            && self.resample == other.resample
            && self.equalisation.is_some() == other.equalisation.is_some()
            && self.dither_to == other.dither_to
            && self.shaping == other.shaping
            && self.gain.is_some() == other.gain.is_some()
            && self.rounds == other.rounds
            && self.decoded_as == other.decoded_as
            && self.true_peak == other.true_peak
    }

    pub fn build_chain(
        &self,
        source: StreamSpec,
        config: &EngineConfig,
        block: usize,
    ) -> Result<Chain> {
        let carrier = StreamSpec::new(source.rate, source.channels, SampleFormat::F32);
        let mut builder = Chain::builder(carrier).max_frames_in(block);

        if let Some((from, to)) = self.remix {
            builder = builder.push(Box::new(Remix::new(from, to)));
        }
        if let Some(restoring) = self.restoration {
            builder = builder.push(Box::new(Restore::new(restoring)));
        }
        if let Some((from, to)) = self.resample {
            builder = builder.push(Box::new(Resampler::new(ResamplerConfig {
                input_rate: from,
                output_rate: to,
                channels: self.stream.channels,
                quality: config.quality,
                phase: config.filter_phase,
                max_frames_in: block,
            })?));
        }
        if let Some(profile) = self.equalisation.as_ref() {
            builder = builder.push(Box::new(Equaliser::new(
                Arc::clone(profile),
                self.stream.rate,
            )));
        }
        if let Some(gain) = self.gain {
            builder = builder.push(Box::new(GainStage::new(gain)));
        }
        if self.true_peak {
            builder = builder.push(Box::new(TruePeak::new()));
        }
        if let Some(depth) = self.dither_to {
            builder = builder.push(Box::new(Dither::new(
                depth,
                config.dither,
                self.shaping,
                seed::from_clock(),
            )));
        }

        builder.build()
    }
}

pub fn plan_output(
    source: Decoded,
    sink: &SinkInfo,
    config: &EngineConfig,
    replay_gain: AppliedGain,
) -> OutputPlan {
    if let Packing::DopMarked(rate) = source.packing
        && dop_survives(source.spec, sink, config, replay_gain)
    {
        return untouched(source.spec, rate);
    }

    let decoded = source;
    let source = source.spec;
    let target = if config.prefer_bit_perfect {
        sink.best_spec_for(source)
    } else {
        sink.current_rate
            .and_then(|rate| sink.best_spec_on(source, rate))
            .or_else(|| sink.best_spec_for(source))
    }
    .unwrap_or(source);

    plan_for(decoded, &sink.name, target, config, replay_gain)
}

pub fn packs_again(
    source: Decoded,
    open: &OutputPlan,
    sink: &SinkInfo,
    config: &EngineConfig,
    replay_gain: AppliedGain,
) -> bool {
    matches!(source.packing, Packing::DopMarked(_))
        && open.packing == Packing::Samples
        && open.stream == source.spec
        && dop_survives(source.spec, sink, config, replay_gain)
}

fn dop_survives(
    source: StreamSpec,
    sink: &SinkInfo,
    config: &EngineConfig,
    replay_gain: AppliedGain,
) -> bool {
    config.dop
        && sink.supports(source)
        && gain_config(config, replay_gain).is_none()
        && eq_config(config, &sink.name).is_none()
}

fn untouched(stream: StreamSpec, rate: DsdRate) -> OutputPlan {
    OutputPlan {
        stream,
        mode: OutputMode::BitPerfect,
        packing: Packing::DopMarked(rate),
        remix: None,
        restoration: None,
        resample: None,
        equalisation: None,
        dither_to: None,
        shaping: NoiseShaping::None,
        gain: None,
        rounds: false,
        decoded_as: stream.format,
        true_peak: false,
    }
}

pub fn plan_for(
    source: Decoded,
    sink: &NodeName,
    target: StreamSpec,
    config: &EngineConfig,
    replay_gain: AppliedGain,
) -> OutputPlan {
    let decimates = matches!(source.packing, Packing::DopMarked(_));
    let restoration = source
        .tuning
        .filter(|_| config.restoration != Restoration::Off)
        .map(|tuning| RestoreConfig {
            restoration: config.restoration,
            tuning,
            wall: source.lowpass,
        });
    let source = source.spec;
    let stream = target;
    let resample = (stream.rate != source.rate).then_some((source.rate, stream.rate));
    let remix = (stream.channel_count() != source.channel_count())
        .then_some((source.channels, stream.channels));
    let equalisation = eq_config(config, sink);
    let converts =
        resample.is_some() || remix.is_some() || equalisation.is_some() || restoration.is_some();
    let gain =
        gain_config(config, replay_gain).or_else(|| converts.then(|| gain_of(config, replay_gain)));

    let stages = converts || gain.is_some();
    let narrows = stream != source && !stream.losslessly_holds(source);

    let dithers =
        (stages || narrows) && config.dither != DitherKind::None && stream.format.is_integer();

    let rounds = narrows && !dithers;
    let true_peak = config.true_peak && (stages || (narrows && !source.format.is_integer()));
    let decoded_as = if decimates || !source.format.is_integer() {
        SampleFormat::F32
    } else {
        source.format
    };

    let mode = if stages || decimates {
        OutputMode::Converted
    } else if stream == source {
        OutputMode::BitPerfect
    } else if !narrows {
        OutputMode::Repacked
    } else if dithers {
        OutputMode::Dithered
    } else {
        OutputMode::Converted
    };

    OutputPlan {
        stream,
        mode,
        packing: Packing::Samples,
        remix,
        restoration,
        resample,
        equalisation,
        dither_to: dithers.then(|| stream.format.bit_depth()),
        shaping: config.noise_shaping.at(stream.rate),
        gain,
        rounds,
        decoded_as,
        true_peak,
    }
}

pub fn resolve_replay_gain(
    mode: ReplayGainMode,
    levelling: Levelling,
    tags: &ReplayGain,
) -> AppliedGain {
    let (gain, peak) = match mode {
        ReplayGainMode::Off => (None, None),
        ReplayGainMode::Track => (
            tags.track_gain.or(tags.album_gain),
            tags.track_peak.or(tags.album_peak),
        ),
        ReplayGainMode::Album => (
            tags.album_gain.or(tags.track_gain),
            tags.album_peak.or(tags.track_peak),
        ),
    };

    let peak = peak.and_then(|peak| Gain::new(peak).ok());
    match (mode, gain) {
        (ReplayGainMode::Off, _) => AppliedGain::default(),
        (_, Some(gain)) => AppliedGain {
            gain: Some(levelling.pre_amp.raised(gain)),
            peak,
        },
        (_, None) => AppliedGain {
            gain: (!levelling.untagged.is_none()).then(|| levelling.untagged.decibels()),
            peak,
        },
    }
}

fn eq_config(config: &EngineConfig, sink: &NodeName) -> Option<Arc<Profile>> {
    config
        .equaliser
        .for_sink(sink)
        .filter(|profile| !profile.is_transparent())
        .map(Arc::clone)
}

fn gain_config(config: &EngineConfig, replay_gain: AppliedGain) -> Option<GainConfig> {
    let attenuated = config.volume != Volume::MAX;
    (attenuated || replay_gain.adjusts()).then(|| gain_of(config, replay_gain))
}

fn gain_of(config: &EngineConfig, replay_gain: AppliedGain) -> GainConfig {
    GainConfig {
        volume: config.volume,
        replay_gain,
        prevent_clipping: true,
        ..GainConfig::default()
    }
}

#[cfg(test)]
mod tests {
    use resonate_core::{
        ChannelLayout, Decibels, SampleFormat,
        eq::{Band, BandGain, BandKind, Frequency, Preamp, Q},
    };
    use resonate_pipewire::{SinkFormats, SinkId};

    use super::*;

    const BLOCK: usize = 1024;
    const THIS_SINK: &str = "alsa_output.test";
    const ANOTHER_SINK: &str = "alsa_output.other";

    fn this_sink() -> NodeName {
        NodeName::new(THIS_SINK)
    }

    fn sink(allowed: &[SampleRate], formats: &[SampleFormat]) -> SinkInfo {
        taking(allowed, formats, &[ChannelLayout::Stereo])
    }

    fn taking(
        allowed: &[SampleRate],
        formats: &[SampleFormat],
        channels: &[ChannelLayout],
    ) -> SinkInfo {
        named(THIS_SINK, allowed, formats, channels)
    }

    fn named(
        name: &str,
        allowed: &[SampleRate],
        formats: &[SampleFormat],
        channels: &[ChannelLayout],
    ) -> SinkInfo {
        SinkInfo {
            id: SinkId::new(1),
            name: NodeName::new(name),
            description: "Test DAC".to_owned(),
            is_default: true,
            is_hardware: true,
            port: None,
            profile: None,
            formats: formats
                .iter()
                .map(|format| SinkFormats {
                    format: *format,
                    rates: allowed.to_vec(),
                    channels: channels.to_vec(),
                })
                .collect(),
            allowed_rates: allowed.to_vec(),
            current_rate: Some(SampleRate::HZ_48000),
        }
    }

    fn spec(rate: SampleRate, format: SampleFormat) -> StreamSpec {
        StreamSpec::new(rate, ChannelLayout::Stereo, format)
    }

    fn plan(source: StreamSpec, sink: &SinkInfo, config: &EngineConfig) -> OutputPlan {
        plan_output(
            Decoded::samples(source),
            sink,
            config,
            AppliedGain::default(),
        )
    }

    fn boost(db: f32) -> AppliedGain {
        AppliedGain {
            gain: Some(Decibels::new(db).expect("a finite adjustment")),
            peak: None,
        }
    }

    fn peaking_at(db: f32, peak: f32) -> AppliedGain {
        AppliedGain {
            peak: Some(Gain::new(peak).expect("a declared peak")),
            ..boost(db)
        }
    }

    fn half_volume() -> EngineConfig {
        EngineConfig {
            volume: Volume::new(0.5).expect("in range"),
            ..EngineConfig::default()
        }
    }

    #[test]
    fn a_sink_that_takes_the_source_exactly_yields_a_bit_perfect_plan() {
        let sink = sink(
            &[
                SampleRate::HZ_44100,
                SampleRate::HZ_48000,
                SampleRate::HZ_96000,
            ],
            &[SampleFormat::S24, SampleFormat::S32],
        );
        let plan = plan(
            spec(SampleRate::HZ_96000, SampleFormat::S24),
            &sink,
            &EngineConfig::default(),
        );

        assert_eq!(plan.mode, OutputMode::BitPerfect);
        assert_eq!(plan.stream, spec(SampleRate::HZ_96000, SampleFormat::S24));
        assert!(plan.resample.is_none());
        assert!(plan.dither_to.is_none());
        assert!(plan.gain.is_none());
    }

    #[test]
    fn a_plan_keeps_a_shaping_curve_only_where_the_output_rate_was_designed_for_it() {
        for (configured, rate, expected) in [
            (
                NoiseShaping::Lipshitz,
                SampleRate::HZ_48000,
                NoiseShaping::Lipshitz,
            ),
            (
                NoiseShaping::Lipshitz,
                SampleRate::HZ_96000,
                NoiseShaping::None,
            ),
            (
                NoiseShaping::Threshold,
                SampleRate::HZ_48000,
                NoiseShaping::Threshold,
            ),
            (
                NoiseShaping::Threshold,
                SampleRate::HZ_96000,
                NoiseShaping::Threshold,
            ),
        ] {
            let config = EngineConfig {
                noise_shaping: configured,
                ..EngineConfig::default()
            };
            let sink = sink(&[rate], &[SampleFormat::S16]);
            let plan = plan(
                spec(SampleRate::HZ_44100, SampleFormat::S24),
                &sink,
                &config,
            );

            assert_eq!(plan.stream.rate, rate);
            assert_eq!(
                plan.dither_to,
                Some(BitDepth::Bits16),
                "{rate} did not dither"
            );
            assert_eq!(
                plan.shaping, expected,
                "{configured:?} at {rate} shaped its dither wrongly"
            );
        }
    }

    #[test]
    fn widening_the_format_alone_is_repacked_not_converted() {
        let sink = sink(&[SampleRate::HZ_44100], &[SampleFormat::S32]);
        let plan = plan(
            spec(SampleRate::HZ_44100, SampleFormat::S16),
            &sink,
            &EngineConfig::default(),
        );

        assert_eq!(plan.mode, OutputMode::Repacked);
        assert_eq!(plan.stream.format, SampleFormat::S32);
        assert!(plan.resample.is_none());
        assert!(plan.dither_to.is_none());
    }

    #[test]
    fn a_rate_the_graph_will_not_switch_to_forces_a_resample() {
        let sink = sink(&[SampleRate::HZ_48000], &[SampleFormat::S32]);
        let plan = plan(
            spec(SampleRate::HZ_44100, SampleFormat::S16),
            &sink,
            &EngineConfig::default(),
        );

        assert_eq!(plan.mode, OutputMode::Converted);
        assert_eq!(
            plan.resample,
            Some((SampleRate::HZ_44100, SampleRate::HZ_48000))
        );
    }

    #[test]
    fn attenuation_alone_is_enough_to_leave_the_bit_perfect_path() {
        let sink = sink(&[SampleRate::HZ_44100], &[SampleFormat::S16]);
        let plan = plan(
            spec(SampleRate::HZ_44100, SampleFormat::S16),
            &sink,
            &half_volume(),
        );

        assert_eq!(plan.mode, OutputMode::Converted);
        assert!(plan.gain.is_some());
        assert!(plan.resample.is_none());
    }

    #[test]
    fn narrowing_the_format_dithers_to_the_target_depth() {
        let sink = sink(&[SampleRate::HZ_44100], &[SampleFormat::S16]);
        let plan = plan(
            spec(SampleRate::HZ_44100, SampleFormat::S24),
            &sink,
            &EngineConfig::default(),
        );

        assert_eq!(plan.mode, OutputMode::Dithered);
        assert_eq!(plan.dither_to, Some(BitDepth::Bits16));
        assert!(plan.resample.is_none());
        assert_eq!(plan.stream.rate, SampleRate::HZ_44100);
    }

    #[test]
    fn a_word_length_that_only_narrows_is_dithered_rather_than_converted() {
        let sink = sink(&[SampleRate::HZ_48000], &[SampleFormat::S16]);
        let plan = plan(
            spec(SampleRate::HZ_48000, SampleFormat::S24),
            &sink,
            &EngineConfig::default(),
        );

        assert_eq!(plan.mode, OutputMode::Dithered);
        assert!(
            plan.resample.is_none(),
            "a rate the sink takes was resampled"
        );
        assert!(plan.remix.is_none());
        assert!(plan.gain.is_none());
        assert_eq!(plan.shaping, NoiseShaping::Threshold);
    }

    #[test]
    fn narrowing_with_the_dither_turned_off_is_a_conversion_nothing_covers() {
        let sink = sink(&[SampleRate::HZ_48000], &[SampleFormat::S16]);
        let config = EngineConfig {
            dither: DitherKind::None,
            ..EngineConfig::default()
        };
        let plan = plan(
            spec(SampleRate::HZ_48000, SampleFormat::S24),
            &sink,
            &config,
        );

        assert_eq!(plan.mode, OutputMode::Converted);
        assert_eq!(plan.dither_to, None);
        assert!(
            plan.rounds && !plan.is_transparent(),
            "the narrowing was left to the decoder's floor rather than rounded"
        );
        assert_eq!(plan.delivery().format, SampleFormat::S24);
    }

    #[test]
    fn a_float_source_reaching_an_integer_word_is_rounded_even_with_the_dither_off() {
        let sink = sink(&[SampleRate::HZ_48000], &[SampleFormat::S24]);
        let config = EngineConfig {
            dither: DitherKind::None,
            ..EngineConfig::default()
        };
        let plan = plan(
            spec(SampleRate::HZ_48000, SampleFormat::F32),
            &sink,
            &config,
        );

        assert!(
            plan.rounds,
            "a float source was handed to a truncating cast"
        );
        assert!(!plan.is_transparent());
        assert_eq!(plan.delivery().format, SampleFormat::F32);
    }

    #[test]
    fn a_converting_plan_reads_an_integer_source_in_its_own_words() {
        for format in [SampleFormat::S16, SampleFormat::S24, SampleFormat::S32] {
            let sink = sink(&[SampleRate::HZ_48000], &[SampleFormat::S32]);
            let plan = plan(
                spec(SampleRate::HZ_44100, format),
                &sink,
                &EngineConfig::default(),
            );

            assert!(plan.resample.is_some());
            assert_eq!(
                plan.delivery().format,
                format,
                "a {format} source was narrowed to f32 before the chain"
            );
        }
    }

    #[test]
    fn the_true_peak_guard_rides_on_whatever_converts_and_never_on_a_bit_perfect_stream() {
        let sink = sink(&[SampleRate::HZ_48000], &[SampleFormat::S16]);
        let resampled = plan(
            spec(SampleRate::HZ_44100, SampleFormat::S16),
            &sink,
            &EngineConfig::default(),
        );
        assert!(resampled.true_peak);
        assert!(
            resampled
                .build_chain(
                    spec(SampleRate::HZ_44100, SampleFormat::S16),
                    &EngineConfig::default(),
                    BLOCK
                )
                .expect("a converting chain builds")
                .latency_frames()
                > 0.0
        );

        let float = plan(
            spec(SampleRate::HZ_48000, SampleFormat::F32),
            &sink,
            &EngineConfig::default(),
        );
        assert!(
            float.true_peak,
            "a float source's overs were left to the conversion"
        );

        let untouched = plan(
            spec(SampleRate::HZ_48000, SampleFormat::S16),
            &sink,
            &EngineConfig::default(),
        );
        assert_eq!(untouched.mode, OutputMode::BitPerfect);
        assert!(!untouched.true_peak);

        let off = EngineConfig {
            true_peak: false,
            ..EngineConfig::default()
        };
        assert!(!plan(spec(SampleRate::HZ_44100, SampleFormat::S16), &sink, &off).true_peak);
    }

    #[test]
    fn a_lossy_source_is_restored_only_where_it_is_asked_for_and_a_lossless_one_never() {
        let sink = sink(&[SampleRate::HZ_44100], &[SampleFormat::S16]);
        let source = spec(SampleRate::HZ_44100, SampleFormat::F32);
        let extending = EngineConfig {
            restoration: Restoration::Extend,
            ..EngineConfig::default()
        };
        let lossy = Decoded {
            tuning: Some(Tuning::Mp3),
            lowpass: Frequency::from_hertz(16_000.0).ok(),
            ..Decoded::samples(source)
        };

        let restored = plan_output(lossy, &sink, &extending, AppliedGain::default());
        assert_eq!(
            restored.restoration,
            Some(RestoreConfig {
                restoration: Restoration::Extend,
                tuning: Tuning::Mp3,
                wall: Frequency::from_hertz(16_000.0).ok(),
            })
        );
        assert_eq!(restored.mode, OutputMode::Converted);
        assert!(!restored.is_transparent());
        let chain = restored
            .build_chain(source, &extending, BLOCK)
            .expect("a restoring chain builds");
        assert!(chain.latency_frames() > 0.0);

        let unasked = plan_output(
            lossy,
            &sink,
            &EngineConfig::default(),
            AppliedGain::default(),
        );
        assert_eq!(unasked.restoration, None);

        let lossless = plan_output(
            Decoded::samples(spec(SampleRate::HZ_44100, SampleFormat::S16)),
            &sink,
            &extending,
            AppliedGain::default(),
        );
        assert_eq!(lossless.restoration, None);
        assert_eq!(lossless.mode, OutputMode::BitPerfect);
    }

    #[test]
    fn only_the_codecs_that_cut_a_band_away_are_tuned_for() {
        assert_eq!(tuning_of(Codec::Mp3), Some(Tuning::Mp3));
        assert_eq!(tuning_of(Codec::Aac), Some(Tuning::Aac));
        assert_eq!(tuning_of(Codec::Vorbis), Some(Tuning::Vorbis));
        for codec in [
            Codec::Flac,
            Codec::Alac,
            Codec::Pcm,
            Codec::Dsd,
            Codec::Unknown,
        ] {
            assert_eq!(tuning_of(codec), None, "{codec:?}");
        }
    }

    #[test]
    fn a_rate_the_sink_cannot_take_is_converted_however_the_depth_lands() {
        let sink = sink(&[SampleRate::HZ_48000], &[SampleFormat::S24]);
        let plan = plan(
            spec(SampleRate::HZ_44100, SampleFormat::S24),
            &sink,
            &EngineConfig::default(),
        );

        assert_eq!(plan.mode, OutputMode::Converted);
        assert_eq!(
            plan.resample,
            Some((SampleRate::HZ_44100, SampleRate::HZ_48000))
        );
    }

    #[test]
    fn a_32_bit_target_is_dithered_to_its_own_grid_wherever_something_narrows() {
        let sink = sink(&[SampleRate::HZ_48000], &[SampleFormat::S32]);
        let resampled = plan(
            spec(SampleRate::HZ_44100, SampleFormat::S16),
            &sink,
            &EngineConfig::default(),
        );

        assert_eq!(resampled.mode, OutputMode::Converted);
        assert_eq!(resampled.dither_to, Some(BitDepth::Bits32));

        let float = plan(
            spec(SampleRate::HZ_48000, SampleFormat::F32),
            &sink,
            &EngineConfig::default(),
        );
        assert_eq!(float.mode, OutputMode::Dithered);
        assert_eq!(float.dither_to, Some(BitDepth::Bits32));

        let held = plan(
            spec(SampleRate::HZ_48000, SampleFormat::S24),
            &sink,
            &EngineConfig::default(),
        );
        assert_eq!(held.mode, OutputMode::Repacked);
        assert_eq!(held.dither_to, None);
    }

    #[test]
    fn dither_is_omitted_when_the_user_turned_it_off() {
        let sink = sink(&[SampleRate::HZ_44100], &[SampleFormat::S16]);
        let config = EngineConfig {
            dither: DitherKind::None,
            ..EngineConfig::default()
        };
        let plan = plan(
            spec(SampleRate::HZ_44100, SampleFormat::S24),
            &sink,
            &config,
        );

        assert_eq!(plan.mode, OutputMode::Converted);
        assert!(plan.dither_to.is_none());
    }

    #[test]
    fn without_prefer_bit_perfect_the_plan_stays_on_the_graphs_current_rate() {
        let sink = sink(
            &[SampleRate::HZ_44100, SampleRate::HZ_48000],
            &[SampleFormat::S32],
        );
        let config = EngineConfig {
            prefer_bit_perfect: false,
            ..EngineConfig::default()
        };
        let plan = plan(
            spec(SampleRate::HZ_44100, SampleFormat::S32),
            &sink,
            &config,
        );

        assert_eq!(plan.stream.rate, SampleRate::HZ_48000);
        assert_eq!(plan.mode, OutputMode::Converted);
    }

    #[test]
    fn prefer_bit_perfect_switches_the_graph_rate_to_match_the_source() {
        let sink = sink(
            &[SampleRate::HZ_44100, SampleRate::HZ_48000],
            &[SampleFormat::S32],
        );
        let plan = plan(
            spec(SampleRate::HZ_44100, SampleFormat::S32),
            &sink,
            &EngineConfig::default(),
        );

        assert_eq!(plan.stream.rate, SampleRate::HZ_44100);
        assert_eq!(plan.mode, OutputMode::BitPerfect);
    }

    #[test]
    fn replay_gain_leaves_the_bit_perfect_path_but_an_untagged_track_does_not() {
        let sink = sink(&[SampleRate::HZ_44100], &[SampleFormat::S16]);
        let source = spec(SampleRate::HZ_44100, SampleFormat::S16);
        let config = EngineConfig {
            replay_gain: ReplayGainMode::Track,
            ..EngineConfig::default()
        };

        let untagged = plan_output(
            Decoded::samples(source),
            &sink,
            &config,
            AppliedGain::default(),
        );
        assert_eq!(untagged.mode, OutputMode::BitPerfect);
        assert!(untagged.gain.is_none());

        let applied = boost(-7.5);
        let tagged = plan_output(Decoded::samples(source), &sink, &config, applied);
        assert_eq!(tagged.mode, OutputMode::Converted);
        assert_eq!(tagged.gain.map(|gain| gain.replay_gain), Some(applied));
    }

    #[test]
    fn the_album_mode_prefers_the_album_tag_and_falls_back_to_the_track_tag() {
        let track = Decibels::new(-9.0).expect("a finite adjustment");
        let album = Decibels::new(-6.0).expect("a finite adjustment");

        let both = ReplayGain {
            track_gain: Some(track),
            album_gain: Some(album),
            ..ReplayGain::default()
        };
        assert_eq!(
            resolve_replay_gain(ReplayGainMode::Album, Levelling::default(), &both).gain,
            Some(album)
        );
        assert_eq!(
            resolve_replay_gain(ReplayGainMode::Track, Levelling::default(), &both).gain,
            Some(track)
        );

        let track_only = ReplayGain {
            track_gain: Some(track),
            ..ReplayGain::default()
        };
        assert_eq!(
            resolve_replay_gain(ReplayGainMode::Album, Levelling::default(), &track_only).gain,
            Some(track)
        );
        assert_eq!(
            resolve_replay_gain(ReplayGainMode::Off, Levelling::default(), &both),
            AppliedGain::default()
        );
    }

    #[test]
    fn a_pre_amp_raises_a_tagged_gain_and_an_untagged_track_takes_a_gain_of_its_own() {
        let tagged = ReplayGain {
            track_gain: Some(Decibels::new(-9.0).expect("finite")),
            track_peak: Some(0.5),
            ..ReplayGain::default()
        };
        let levelling = Levelling {
            pre_amp: Trim::from_millibels(300).expect("a trim"),
            untagged: Trim::from_millibels(-600).expect("a trim"),
        };

        let raised = resolve_replay_gain(ReplayGainMode::Track, levelling, &tagged);
        assert_eq!(raised.gain, Some(Decibels::new(-6.0).expect("finite")));
        assert_eq!(raised.peak, Gain::new(0.5).ok());

        let untagged =
            resolve_replay_gain(ReplayGainMode::Album, levelling, &ReplayGain::default());
        assert_eq!(untagged.gain, Some(Decibels::new(-6.0).expect("finite")));
        assert_eq!(untagged.peak, None);

        assert_eq!(
            resolve_replay_gain(ReplayGainMode::Off, levelling, &tagged),
            AppliedGain::default(),
            "a pre-amp moved a track with ReplayGain switched off"
        );
    }

    #[test]
    fn a_pre_amp_that_would_push_a_tagged_peak_past_full_scale_is_capped_at_the_peak() {
        let tagged = ReplayGain {
            track_gain: Some(Decibels::new(-1.0).expect("finite")),
            track_peak: Some(0.9),
            ..ReplayGain::default()
        };
        let levelling = Levelling {
            pre_amp: Trim::from_millibels(600).expect("a trim"),
            ..Levelling::default()
        };

        let applied = resolve_replay_gain(ReplayGainMode::Track, levelling, &tagged);
        assert!(applied.is_capped());
        assert!((applied.applied().get() * 0.9 - 1.0).abs() < 1e-4);
    }

    #[test]
    fn the_peak_follows_the_gain_the_mode_chose() {
        let tags = ReplayGain {
            track_gain: Some(Decibels::new(-9.0).expect("finite")),
            track_peak: Some(0.99),
            album_gain: Some(Decibels::new(-6.0).expect("finite")),
            album_peak: Some(1.02),
        };

        let album = resolve_replay_gain(ReplayGainMode::Album, Levelling::default(), &tags);
        assert_eq!(album.gain, tags.album_gain);
        assert_eq!(album.peak, Gain::new(1.02).ok());

        let track = resolve_replay_gain(ReplayGainMode::Track, Levelling::default(), &tags);
        assert_eq!(track.gain, tags.track_gain);
        assert_eq!(track.peak, Gain::new(0.99).ok());

        assert_eq!(
            resolve_replay_gain(ReplayGainMode::Off, Levelling::default(), &tags).peak,
            None
        );
    }

    #[test]
    fn a_peak_that_would_clip_earns_a_gain_stage_even_at_unity() {
        let sink = sink(&[SampleRate::HZ_44100], &[SampleFormat::S16]);
        let source = spec(SampleRate::HZ_44100, SampleFormat::S16);
        let clipping = AppliedGain {
            gain: Some(Decibels::UNITY),
            peak: Gain::new(1.2).ok(),
        };

        let plan = plan_output(
            Decoded::samples(source),
            &sink,
            &EngineConfig::default(),
            clipping,
        );

        assert_eq!(plan.mode, OutputMode::Converted);
        assert!(plan.gain.is_some(), "a clipping source was left untouched");
    }

    #[test]
    fn a_boost_a_full_scale_peak_caps_at_unity_plays_bit_perfect() {
        let sink = sink(&[SampleRate::HZ_44100], &[SampleFormat::S16]);
        let source = spec(SampleRate::HZ_44100, SampleFormat::S16);

        for dither in [DitherKind::None, DitherKind::Triangular] {
            let config = EngineConfig {
                replay_gain: ReplayGainMode::Track,
                dither,
                ..EngineConfig::default()
            };
            let plan = plan_output(
                Decoded::samples(source),
                &sink,
                &config,
                peaking_at(6.0, 1.0),
            );

            assert_eq!(
                plan.mode,
                OutputMode::BitPerfect,
                "a boost capped at unity left the bit-perfect path with {dither:?} dither"
            );
            assert_eq!(plan.gain, None, "a gain stage was asked to multiply by one");
            assert_eq!(
                plan.dither_to, None,
                "{dither:?} dither ran over untouched samples"
            );
            assert_eq!(plan.delivery().format, SampleFormat::S16);
        }
    }

    #[test]
    fn a_renegotiated_stream_takes_the_channel_layout_the_graph_named() {
        let source = spec(SampleRate::HZ_44100, SampleFormat::S24);
        let target = StreamSpec::new(SampleRate::HZ_48000, ChannelLayout::Mono, SampleFormat::F32);
        let plan = plan_for(
            Decoded::samples(source),
            &this_sink(),
            target,
            &EngineConfig::default(),
            AppliedGain::default(),
        );

        assert_eq!(plan.stream, target);
        assert_eq!(
            plan.remix,
            Some((ChannelLayout::Stereo, ChannelLayout::Mono))
        );
        assert_eq!(plan.mode, OutputMode::Converted);
    }

    #[test]
    fn a_renegotiation_settles_because_the_next_plan_asks_for_what_the_graph_answered() {
        let source = spec(SampleRate::HZ_44100, SampleFormat::S24);
        let negotiated = StreamSpec::new(
            SampleRate::HZ_48000,
            ChannelLayout::Stereo,
            SampleFormat::S32,
        );

        let first = plan_for(
            Decoded::samples(source),
            &this_sink(),
            negotiated,
            &EngineConfig::default(),
            AppliedGain::default(),
        );
        let second = plan_for(
            Decoded::samples(source),
            &this_sink(),
            first.stream,
            &EngineConfig::default(),
            AppliedGain::default(),
        );

        assert_eq!(first.stream, negotiated);
        assert!(first.same_shape_as(&second));
    }

    #[test]
    fn a_stereo_sink_plans_a_downmix_for_a_surround_source() {
        let sink = taking(
            &[SampleRate::HZ_48000],
            &[SampleFormat::S32],
            &[ChannelLayout::Stereo],
        );
        let source = StreamSpec::new(
            SampleRate::HZ_48000,
            ChannelLayout::Surround51,
            SampleFormat::S32,
        );
        let plan = plan(source, &sink, &EngineConfig::default());

        assert_eq!(plan.stream.channels, ChannelLayout::Stereo);
        assert_eq!(
            plan.remix,
            Some((ChannelLayout::Surround51, ChannelLayout::Stereo))
        );
        assert_eq!(plan.mode, OutputMode::Converted);
        assert!(!plan.is_transparent());
    }

    #[test]
    fn a_surround_sink_leaves_a_surround_source_alone() {
        let sink = taking(
            &[SampleRate::HZ_48000],
            &[SampleFormat::S32],
            &[ChannelLayout::Stereo, ChannelLayout::Surround51],
        );
        let source = StreamSpec::new(
            SampleRate::HZ_48000,
            ChannelLayout::Surround51,
            SampleFormat::S32,
        );
        let plan = plan(source, &sink, &EngineConfig::default());

        assert_eq!(plan.mode, OutputMode::BitPerfect);
        assert!(plan.remix.is_none());
    }

    #[test]
    fn a_downmixing_plan_names_a_spec_the_sink_advertised() {
        let sink = taking(
            &[SampleRate::HZ_44100, SampleRate::HZ_48000],
            &[SampleFormat::S16, SampleFormat::S32],
            &[ChannelLayout::Mono, ChannelLayout::Stereo],
        );

        for channels in [
            ChannelLayout::Mono,
            ChannelLayout::Stereo,
            ChannelLayout::Quad,
            ChannelLayout::Surround51,
            ChannelLayout::Surround71,
        ] {
            let source = StreamSpec::new(SampleRate::HZ_44100, channels, SampleFormat::S24);
            let plan = plan(source, &sink, &EngineConfig::default());

            assert!(
                sink.supports(plan.stream),
                "{channels} was planned onto {stream}, which the sink never advertised",
                stream = plan.stream
            );
        }
    }

    #[test]
    fn a_downmix_and_a_resample_share_one_chain() {
        let sink = taking(
            &[SampleRate::HZ_48000],
            &[SampleFormat::S16],
            &[ChannelLayout::Stereo],
        );
        let source = StreamSpec::new(
            SampleRate::HZ_44100,
            ChannelLayout::Surround51,
            SampleFormat::S24,
        );
        let config = EngineConfig::default();
        let plan = plan(source, &sink, &config);

        assert_eq!(
            plan.remix,
            Some((ChannelLayout::Surround51, ChannelLayout::Stereo))
        );
        assert_eq!(
            plan.resample,
            Some((SampleRate::HZ_44100, SampleRate::HZ_48000))
        );

        let mut chain = plan
            .build_chain(source, &config, BLOCK)
            .expect("a downmixing chain builds");
        let fed = vec![0.25; BLOCK * 6];
        let mut produced = vec![0.0; chain.max_output_frames() * 2];
        let count = chain.process(&fed, &mut produced);

        assert_eq!(count.frames_in, BLOCK);
        assert!(count.frames_out > 0);
    }

    #[test]
    fn a_sink_that_takes_only_more_channels_than_the_source_is_fed_silence_in_the_rest() {
        let sink = taking(
            &[SampleRate::HZ_48000],
            &[SampleFormat::S32],
            &[ChannelLayout::Surround51],
        );
        let source = spec(SampleRate::HZ_48000, SampleFormat::S32);
        let config = EngineConfig::default();
        let plan = plan(source, &sink, &config);

        assert_eq!(plan.stream.channels, ChannelLayout::Surround51);

        let mut chain = plan
            .build_chain(source, &config, BLOCK)
            .expect("an upmixing chain builds");
        let fed = vec![0.5; BLOCK * 2];
        let mut produced = vec![0.0; chain.max_output_frames() * 6];
        chain.process(&fed, &mut produced);

        let frame = produced.get(..6).expect("one frame came out");
        let within_the_dither = 2.0 / f64::from(SampleFormat::S32.full_scale());
        for (made, wanted) in frame.iter().zip([0.5, 0.5, 0.0, 0.0, 0.0, 0.0]) {
            assert!(
                (made - wanted).abs() <= within_the_dither,
                "{frame:?} is not the source on the fronts and silence elsewhere"
            );
        }
    }

    fn dsd64() -> DsdRate {
        DsdRate::new(2_822_400).expect("a representable dsd rate")
    }

    fn packed(spec: StreamSpec) -> Decoded {
        Decoded {
            packing: Packing::DopMarked(dsd64()),
            ..Decoded::samples(spec)
        }
    }

    fn dop_config() -> EngineConfig {
        EngineConfig {
            dop: true,
            ..EngineConfig::default()
        }
    }

    #[test]
    fn a_dop_source_a_sink_takes_exactly_reaches_it_untouched() {
        let sink = sink(&[SampleRate::HZ_176400], &[SampleFormat::S24]);
        let source = spec(SampleRate::HZ_176400, SampleFormat::S24);

        let plan = plan_output(packed(source), &sink, &dop_config(), AppliedGain::default());

        assert_eq!(plan.mode, OutputMode::BitPerfect);
        assert_eq!(plan.packing, Packing::DopMarked(dsd64()));
        assert!(plan.is_transparent());
        assert_eq!(plan.delivery().format, SampleFormat::S24);
        assert!(
            plan.build_chain(source, &dop_config(), BLOCK)
                .expect("a transparent chain always builds")
                .is_transparent()
        );
    }

    #[test]
    fn a_marked_plan_carries_a_silence_the_graph_cannot_supply_itself() {
        let sink = sink(&[SampleRate::HZ_176400], &[SampleFormat::S24]);
        let source = spec(SampleRate::HZ_176400, SampleFormat::S24);

        let marked = plan_output(packed(source), &sink, &dop_config(), AppliedGain::default());
        let Silence::Marked {
            word,
            alternate,
            format,
        } = marked.silence()
        else {
            panic!("a DoP plan went silent as plain zeros");
        };

        assert_eq!(format, marked.stream.format);
        assert_eq!(word[2], 0x05);
        assert_eq!(alternate[2], 0xFA);

        let decimated = plan_output(
            packed(source),
            &sink,
            &EngineConfig::default(),
            AppliedGain::default(),
        );
        assert_eq!(decimated.silence(), Silence::Unmarked);
    }

    #[test]
    fn dop_is_off_until_it_is_asked_for() {
        let sink = sink(&[SampleRate::HZ_176400], &[SampleFormat::S24]);
        let source = spec(SampleRate::HZ_176400, SampleFormat::S24);

        let plan = plan_output(
            packed(source),
            &sink,
            &EngineConfig::default(),
            AppliedGain::default(),
        );

        assert_eq!(plan.packing, Packing::Samples);
        assert_eq!(plan.delivery().packing, Packing::Samples);
        assert_eq!(
            plan.mode,
            OutputMode::Converted,
            "a decimation was called bit-perfect because the carrier matched the sink"
        );
    }

    #[test]
    fn a_decimated_stream_packs_again_once_nothing_stands_in_the_way() {
        let sink = sink(&[SampleRate::HZ_176400], &[SampleFormat::S24]);
        let source = spec(SampleRate::HZ_176400, SampleFormat::S24);
        let quiet = EngineConfig {
            volume: Volume::new(0.5).expect("half volume"),
            ..dop_config()
        };

        let decimated = plan_output(packed(source), &sink, &quiet, AppliedGain::default());
        assert_eq!(decimated.packing, Packing::Samples);
        assert!(!packs_again(
            packed(source),
            &decimated,
            &sink,
            &quiet,
            AppliedGain::default()
        ));
        assert!(packs_again(
            packed(source),
            &decimated,
            &sink,
            &dop_config(),
            AppliedGain::default()
        ));
        assert!(!packs_again(
            packed(source),
            &decimated,
            &sink,
            &EngineConfig::default(),
            AppliedGain::default()
        ));
    }

    #[test]
    fn a_sink_that_does_not_take_the_carrier_exactly_never_gets_a_marker() {
        let narrow = sink(&[SampleRate::HZ_44100], &[SampleFormat::S16]);
        let wrong_rate = sink(&[SampleRate::HZ_48000], &[SampleFormat::S24]);
        let source = spec(SampleRate::HZ_176400, SampleFormat::S24);

        for sink in [narrow, wrong_rate] {
            let plan = plan_output(packed(source), &sink, &dop_config(), AppliedGain::default());
            assert_eq!(
                plan.packing,
                Packing::Samples,
                "a sink that cannot take the carrier was handed DoP anyway"
            );
        }
    }

    #[test]
    fn no_setting_the_pane_offers_can_produce_a_marked_plan_with_a_stage_in_it() {
        let source = spec(SampleRate::HZ_176400, SampleFormat::S24);
        let sinks = [
            sink(&[SampleRate::HZ_176400], &[SampleFormat::S24]),
            sink(&[SampleRate::HZ_176400], &[SampleFormat::S32]),
            sink(&[SampleRate::HZ_44100], &[SampleFormat::S16]),
            sink(
                &[SampleRate::HZ_44100, SampleRate::HZ_176400],
                &[SampleFormat::S16, SampleFormat::S24],
            ),
        ];
        let gains = [
            AppliedGain::default(),
            boost(-7.5),
            AppliedGain {
                gain: Some(Decibels::UNITY),
                peak: Gain::new(1.2).ok(),
            },
        ];

        let equalisations = [
            Equalisation::default(),
            switched_on(Equalisation {
                fallback: Some(Arc::new(Profile::flat())),
                ..Equalisation::default()
            }),
            switched_on(Equalisation {
                fallback: Some(boosting()),
                ..Equalisation::default()
            }),
            switched_on(Equalisation {
                bound: [(this_sink(), boosting())].into_iter().collect(),
                ..Equalisation::default()
            }),
            switched_on(Equalisation {
                bound: [(NodeName::new(ANOTHER_SINK), boosting())]
                    .into_iter()
                    .collect(),
                ..Equalisation::default()
            }),
        ];

        let mut marked = 0_usize;
        for sink in &sinks {
            for gain in gains {
                for equaliser in &equalisations {
                    for dither in [DitherKind::None, DitherKind::Triangular] {
                        for shaping in [
                            NoiseShaping::None,
                            NoiseShaping::Lipshitz,
                            NoiseShaping::Threshold,
                        ] {
                            for volume in [Volume::MAX, Volume::new(0.5).expect("half volume")] {
                                for replay_gain in [
                                    ReplayGainMode::Off,
                                    ReplayGainMode::Track,
                                    ReplayGainMode::Album,
                                ] {
                                    let config = EngineConfig {
                                        dop: true,
                                        dither,
                                        noise_shaping: shaping,
                                        volume,
                                        replay_gain,
                                        equaliser: Arc::new(equaliser.clone()),
                                        ..EngineConfig::default()
                                    };
                                    let plan = plan_output(packed(source), sink, &config, gain);

                                    if plan.packing == Packing::Samples {
                                        continue;
                                    }
                                    marked += 1;
                                    assert!(
                                        plan.is_transparent(),
                                        "a marked plan carries a stage that would rewrite its \
                                         markers"
                                    );
                                    assert_eq!(plan.mode, OutputMode::BitPerfect);
                                    assert_eq!(plan.delivery().format, SampleFormat::S24);
                                }
                            }
                        }
                    }
                }
            }
        }

        assert!(
            marked > 0,
            "every setting in the cross product refused DoP, so nothing was proved about a \
             marked plan"
        );
    }

    fn switched_on(equalisation: Equalisation) -> Equalisation {
        Equalisation {
            enabled: true,
            ..equalisation
        }
    }

    fn boosting() -> Arc<Profile> {
        Arc::new(
            Profile::new(
                Preamp::NONE,
                vec![Band::new(
                    BandKind::Peaking,
                    Frequency::from_hertz(1_000.0).expect("in range"),
                    BandGain::from_decibels(6.0).expect("in range"),
                    Q::from_units(1.0).expect("in range"),
                )],
            )
            .expect("one band"),
        )
    }

    #[test]
    fn a_marked_source_loses_its_markers_rather_than_its_equaliser() {
        let source = spec(SampleRate::HZ_176400, SampleFormat::S24);
        let sink = sink(&[SampleRate::HZ_176400], &[SampleFormat::S24]);
        let config = EngineConfig {
            dop: true,
            equaliser: Arc::new(switched_on(Equalisation {
                fallback: Some(boosting()),
                ..Equalisation::default()
            })),
            ..EngineConfig::default()
        };

        let plan = plan_output(packed(source), &sink, &config, AppliedGain::default());

        assert_eq!(plan.packing, Packing::Samples);
        assert!(plan.equalisation.is_some());
        assert_eq!(plan.mode, OutputMode::Converted);
    }

    #[test]
    fn a_build_as_it_left_the_workshop_carries_no_equaliser_at_all() {
        let config = EngineConfig::default();

        assert!(!config.equaliser.enabled);
        assert!(!config.equaliser.binds_anything());
        assert_eq!(config.equaliser.for_sink(&this_sink()), None);
    }

    #[test]
    fn an_equaliser_that_is_switched_off_leaves_every_plan_the_shape_it_had() {
        let source = spec(SampleRate::HZ_44100, SampleFormat::S24);
        let sinks = [
            sink(&[SampleRate::HZ_44100], &[SampleFormat::S24]),
            sink(&[SampleRate::HZ_48000], &[SampleFormat::S16]),
        ];
        let bound = Equalisation {
            enabled: false,
            bound: [
                (this_sink(), boosting()),
                (NodeName::new(ANOTHER_SINK), boosting()),
            ]
            .into_iter()
            .collect(),
            fallback: Some(boosting()),
        };

        for sink in &sinks {
            let bare = plan_output(
                Decoded::samples(source),
                sink,
                &EngineConfig::default(),
                AppliedGain::default(),
            );
            let with_bindings = plan_output(
                Decoded::samples(source),
                sink,
                &EngineConfig {
                    equaliser: Arc::new(bound.clone()),
                    ..EngineConfig::default()
                },
                AppliedGain::default(),
            );
            assert_eq!(bare, with_bindings);
        }
    }

    #[test]
    fn an_equaliser_switched_on_over_an_empty_profile_is_still_bit_perfect() {
        let source = spec(SampleRate::HZ_44100, SampleFormat::S24);
        let sink = sink(&[SampleRate::HZ_44100], &[SampleFormat::S24]);
        let config = EngineConfig {
            equaliser: Arc::new(switched_on(Equalisation {
                fallback: Some(Arc::new(Profile::flat())),
                ..Equalisation::default()
            })),
            ..EngineConfig::default()
        };

        let plan = plan_output(
            Decoded::samples(source),
            &sink,
            &config,
            AppliedGain::default(),
        );

        assert_eq!(plan.equalisation, None);
        assert_eq!(plan.mode, OutputMode::BitPerfect);
        assert!(plan.is_transparent());
    }

    #[test]
    fn a_profile_is_taken_from_the_device_that_names_one_and_otherwise_from_the_fallback() {
        let source = spec(SampleRate::HZ_44100, SampleFormat::S24);
        let mine = sink(&[SampleRate::HZ_44100], &[SampleFormat::S24]);
        let other = named(
            ANOTHER_SINK,
            &[SampleRate::HZ_44100],
            &[SampleFormat::S24],
            &[ChannelLayout::Stereo],
        );

        let only_mine = EngineConfig {
            equaliser: Arc::new(switched_on(Equalisation {
                bound: [(this_sink(), boosting())].into_iter().collect(),
                ..Equalisation::default()
            })),
            ..EngineConfig::default()
        };
        let plan = |sink: &SinkInfo, config: &EngineConfig| {
            plan_output(
                Decoded::samples(source),
                sink,
                config,
                AppliedGain::default(),
            )
        };

        assert!(plan(&mine, &only_mine).equalisation.is_some());
        assert!(plan(&other, &only_mine).equalisation.is_none());

        let with_fallback = EngineConfig {
            equaliser: Arc::new(switched_on(Equalisation {
                bound: [(this_sink(), boosting())].into_iter().collect(),
                fallback: Some(boosting()),
                ..Equalisation::default()
            })),
            ..EngineConfig::default()
        };
        assert!(plan(&other, &with_fallback).equalisation.is_some());
    }

    #[test]
    fn editing_a_band_is_not_a_change_of_shape_but_switching_the_equaliser_is() {
        let source = spec(SampleRate::HZ_44100, SampleFormat::S24);
        let sink = sink(&[SampleRate::HZ_44100], &[SampleFormat::S24]);
        let with = |equaliser: Equalisation| EngineConfig {
            equaliser: Arc::new(equaliser),
            ..EngineConfig::default()
        };
        let plan = |config: &EngineConfig| {
            plan_output(
                Decoded::samples(source),
                &sink,
                config,
                AppliedGain::default(),
            )
        };

        let quieter = Arc::new(
            Profile::new(
                Preamp::NONE,
                vec![Band::new(
                    BandKind::Peaking,
                    Frequency::from_hertz(1_000.0).expect("in range"),
                    BandGain::from_decibels(3.0).expect("in range"),
                    Q::from_units(1.0).expect("in range"),
                )],
            )
            .expect("one band"),
        );

        let louder = plan(&with(switched_on(Equalisation {
            fallback: Some(boosting()),
            ..Equalisation::default()
        })));
        let softer = plan(&with(switched_on(Equalisation {
            fallback: Some(quieter),
            ..Equalisation::default()
        })));
        let off = plan(&with(Equalisation::default()));

        assert!(
            louder.same_shape_as(&softer),
            "a band edit reopened the stream"
        );
        assert_ne!(louder.equalisation, softer.equalisation);
        assert!(!louder.same_shape_as(&off));
    }

    #[test]
    fn a_plan_that_says_it_touches_the_signal_builds_a_chain_that_does() {
        let source = spec(SampleRate::HZ_44100, SampleFormat::S24);
        let sinks = [
            sink(&[SampleRate::HZ_44100], &[SampleFormat::S24]),
            sink(&[SampleRate::HZ_48000], &[SampleFormat::S16]),
            sink(&[SampleRate::HZ_44100], &[SampleFormat::S32]),
        ];
        let equalisations = [
            Equalisation::default(),
            switched_on(Equalisation {
                fallback: Some(Arc::new(Profile::flat())),
                ..Equalisation::default()
            }),
            switched_on(Equalisation {
                fallback: Some(boosting()),
                ..Equalisation::default()
            }),
        ];
        let gains = [
            AppliedGain::default(),
            peaking_at(6.0, 1.0),
            peaking_at(6.0, 0.5),
            peaking_at(-6.0, 0.9),
        ];

        for sink in &sinks {
            for equaliser in &equalisations {
                for volume in [Volume::MAX, Volume::new(0.5).expect("half volume")] {
                    for dither in [DitherKind::None, DitherKind::Triangular] {
                        for gain in gains {
                            let config = EngineConfig {
                                volume,
                                dither,
                                replay_gain: ReplayGainMode::Track,
                                equaliser: Arc::new(equaliser.clone()),
                                ..EngineConfig::default()
                            };
                            let plan = plan_output(Decoded::samples(source), sink, &config, gain);
                            let chain = plan
                                .build_chain(source, &config, BLOCK)
                                .expect("every plan the pane can produce builds");

                            assert_eq!(
                                plan.is_transparent(),
                                chain.is_transparent(),
                                "the plan and the chain disagree about whether anything runs \
                                 under {gain:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn turning_dop_off_while_it_plays_is_a_change_of_shape() {
        let sink = sink(&[SampleRate::HZ_176400], &[SampleFormat::S24]);
        let source = spec(SampleRate::HZ_176400, SampleFormat::S24);

        let marked = plan_output(packed(source), &sink, &dop_config(), AppliedGain::default());
        let plain = plan_output(
            packed(source),
            &sink,
            &EngineConfig::default(),
            AppliedGain::default(),
        );

        assert!(
            !marked.same_shape_as(&plain),
            "dropping DoP left the stream open and the decoder still marking"
        );
    }

    #[test]
    fn a_bit_perfect_plan_builds_a_chain_with_nothing_in_it() {
        let sink = sink(&[SampleRate::HZ_96000], &[SampleFormat::S24]);
        let source = spec(SampleRate::HZ_96000, SampleFormat::S24);
        let config = EngineConfig::default();

        let chain = plan(source, &sink, &config)
            .build_chain(source, &config, BLOCK)
            .expect("a transparent chain always builds");

        assert!(chain.is_transparent());
    }

    #[test]
    fn every_plan_this_crate_can_produce_builds_a_chain() {
        let sinks = [
            sink(&[SampleRate::HZ_44100], &[SampleFormat::S16]),
            sink(&[SampleRate::HZ_48000], &[SampleFormat::S32]),
            sink(&[SampleRate::HZ_96000], &[SampleFormat::S24]),
            sink(
                &[SampleRate::HZ_44100, SampleRate::HZ_192000],
                &[SampleFormat::F32],
            ),
        ];
        let sources = [
            spec(SampleRate::HZ_44100, SampleFormat::S16),
            spec(SampleRate::HZ_48000, SampleFormat::S24),
            spec(SampleRate::HZ_88200, SampleFormat::S32),
            spec(SampleRate::HZ_192000, SampleFormat::F32),
        ];
        let configs = [EngineConfig::default(), half_volume()];

        for sink in &sinks {
            for source in sources {
                for config in &configs {
                    let plan = plan_output(
                        Decoded::samples(source),
                        sink,
                        config,
                        AppliedGain::default(),
                    );
                    assert!(
                        plan.build_chain(source, config, BLOCK).is_ok(),
                        "{source} -> {stream} did not build",
                        stream = plan.stream
                    );
                }
            }
        }
    }

    #[test]
    fn plans_that_differ_only_in_amplitude_share_a_shape() {
        let sink = sink(&[SampleRate::HZ_44100], &[SampleFormat::S16]);
        let source = spec(SampleRate::HZ_44100, SampleFormat::S16);

        let quiet = plan(source, &sink, &half_volume());
        let quieter = plan(
            source,
            &sink,
            &EngineConfig {
                volume: Volume::new(0.25).expect("in range"),
                ..EngineConfig::default()
            },
        );
        let unity = plan(source, &sink, &EngineConfig::default());

        assert!(quiet.same_shape_as(&quieter));
        assert!(!quiet.same_shape_as(&unity));
    }

    fn equalising() -> EngineConfig {
        EngineConfig {
            equaliser: Arc::new(switched_on(Equalisation {
                fallback: Some(boosting()),
                ..Equalisation::default()
            })),
            ..EngineConfig::default()
        }
    }

    #[test]
    fn every_converting_plan_carries_a_gain_stage_so_the_volume_never_changes_its_shape() {
        let surround = StreamSpec::new(
            SampleRate::HZ_48000,
            ChannelLayout::Surround51,
            SampleFormat::S32,
        );
        let converting = [
            (
                spec(SampleRate::HZ_44100, SampleFormat::S16),
                sink(&[SampleRate::HZ_48000], &[SampleFormat::S32]),
                EngineConfig::default(),
            ),
            (
                surround,
                sink(&[SampleRate::HZ_48000], &[SampleFormat::S32]),
                EngineConfig::default(),
            ),
            (
                spec(SampleRate::HZ_44100, SampleFormat::S16),
                sink(&[SampleRate::HZ_44100], &[SampleFormat::S16]),
                equalising(),
            ),
        ];

        for (source, sink, config) in converting {
            let full = plan(source, &sink, &config);
            let halved = plan(
                source,
                &sink,
                &EngineConfig {
                    volume: Volume::new(0.5).expect("in range"),
                    ..config
                },
            );

            let carried = full
                .gain
                .expect("a converting plan at full volume left its gain stage out");
            assert_eq!(carried.volume, Volume::MAX);
            assert!(!carried.replay_gain.adjusts());
            assert_eq!(full.mode, OutputMode::Converted);
            assert!(
                full.same_shape_as(&halved),
                "moving the volume off full changed the shape of {source} -> {stream}",
                stream = full.stream
            );
        }
    }

    #[test]
    fn a_plan_becomes_another_on_its_own_stream_only_where_no_resampler_holds_history() {
        let exact = sink(&[SampleRate::HZ_44100], &[SampleFormat::S16]);
        let slower = sink(&[SampleRate::HZ_48000], &[SampleFormat::S16]);
        let stereo = sink(&[SampleRate::HZ_48000], &[SampleFormat::S32]);
        let cd = spec(SampleRate::HZ_44100, SampleFormat::S16);
        let studio = spec(SampleRate::HZ_44100, SampleFormat::S24);
        let surround = StreamSpec::new(
            SampleRate::HZ_48000,
            ChannelLayout::Surround51,
            SampleFormat::S32,
        );
        let plain = EngineConfig::default();
        let quiet = half_volume();
        let equalised = equalising();

        let reshaped = [
            ("full to half volume", cd, &exact, &plain, &quiet),
            ("switching the equaliser on", cd, &exact, &plain, &equalised),
            (
                "switching the equaliser off",
                cd,
                &exact,
                &equalised,
                &plain,
            ),
            ("a narrowing turned down", studio, &exact, &plain, &quiet),
            ("a downmix equalised", surround, &stereo, &plain, &equalised),
        ];
        for (what, source, sink, from, to) in reshaped {
            let running = plan(source, sink, from);
            let wanted = plan(source, sink, to);

            assert!(
                !running.same_shape_as(&wanted),
                "{what} was a retune, so it proved nothing about a reshape"
            );
            assert!(
                running.becomes_on_the_same_stream(&wanted),
                "{what} reopened a stream it could have kept"
            );
        }

        let resampled = plan(cd, &slower, &plain);
        assert!(
            !resampled.becomes_on_the_same_stream(&plan(cd, &slower, &equalised)),
            "an equaliser switched on under a resampler kept a history the new chain cannot carry"
        );

        let renegotiated = plan_for(
            Decoded::samples(cd),
            &this_sink(),
            spec(SampleRate::HZ_44100, SampleFormat::S32),
            &plain,
            AppliedGain::default(),
        );
        assert!(!plan(cd, &exact, &plain).becomes_on_the_same_stream(&renegotiated));

        let carrier = spec(SampleRate::HZ_176400, SampleFormat::S24);
        let takes_the_carrier = sink(&[SampleRate::HZ_176400], &[SampleFormat::S24]);
        let marked = plan_output(
            packed(carrier),
            &takes_the_carrier,
            &dop_config(),
            AppliedGain::default(),
        );
        let decimated = plan_output(
            packed(carrier),
            &takes_the_carrier,
            &plain,
            AppliedGain::default(),
        );
        assert!(!marked.becomes_on_the_same_stream(&decimated));
    }
}
