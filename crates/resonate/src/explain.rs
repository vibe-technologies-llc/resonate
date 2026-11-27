use std::path::Path;

use resonate_codec::Packing;
use resonate_core::{MediaLocation, StreamSpec};
use resonate_engine::{
    Decoded, EngineConfig, FilterPhase, NoiseShaping, OutputPlan, Quality, SinkInfo, Tuning, Words,
    plan_output, resolve_replay_gain,
};

use crate::{
    cli::Cli,
    config::Config,
    engine_settings::engine_config,
    error::Result,
    sinks::{select_sink, wanted_sink},
    sources_over, surveyed, vault_already_kept, words,
};

pub(crate) fn explain(cli: &Cli, config: &Config, path: &Path) -> Result<()> {
    let sources = sources_over(vault_already_kept(cli, config).as_ref());
    let info = resonate_codec::probe(&sources, &MediaLocation::local(path))?;

    let wanted = wanted_sink(cli, config);
    let sink = select_sink(surveyed::sinks()?, wanted.as_ref())?;
    let config = engine_config(cli, config);
    let replay_gain =
        resolve_replay_gain(config.replay_gain, config.levelling, &info.tags.replay_gain);
    let decoded = Decoded::of(&info, None);

    let plan = plan_output(decoded, &sink, &config, replay_gain);

    for line in lines(info.spec, info.packing, &sink, &config, &plan) {
        said!("{line}");
    }
    Ok(())
}

fn lines(
    source: StreamSpec,
    packing: Packing,
    sink: &SinkInfo,
    config: &EngineConfig,
    plan: &OutputPlan,
) -> Vec<String> {
    let mut lines = vec![words::main::explain_source(source.to_string())];

    if let Packing::DopMarked(rate) = packing {
        lines.push(
            words::main::ExplainDsd {
                hertz: rate.hz(),
                rate: rate.to_string(),
            }
            .said(),
        );
    }
    lines.push(
        words::main::ExplainSink {
            description: &sink.description,
            name: sink.name.as_str(),
        }
        .said(),
    );
    lines.push(
        words::main::ExplainOutput {
            mode: crate::readout::output_mode(plan.mode),
            stream: plan.stream.to_string(),
        }
        .said(),
    );
    if let Some(framing) = sink.words_for(plan.stream.format)
        && framing != Words::Whole
    {
        lines.push(
            words::main::ExplainWords {
                named: framing.spelled(plan.stream.format),
                offered: Words::offered_for(plan.stream.format).spelled(plan.stream.format),
            }
            .said(),
        );
    }
    if let Some((from, to)) = plan.remix {
        lines.push(
            words::main::ExplainRemix {
                from: from.to_string(),
                to: to.to_string(),
            }
            .said(),
        );
    }
    if let Some((from, to)) = plan.resample {
        lines.push(
            words::main::ExplainResample {
                from: from.to_string(),
                phase: phase_words(config.filter_phase),
                quality: quality_words(config.quality),
                to: to.to_string(),
            }
            .said(),
        );
    }
    if let Some(profile) = plan.equalisation.as_ref() {
        lines.push(format!(
            "{}{}",
            words::main::ExplainEq {
                bands: profile.applied(plan.stream.rate),
                peak: format!("{:+.2}", profile.peak_db(plan.stream.rate)),
                preamp: profile.preamp().to_string()
            }
            .said(),
            above_nyquist(profile.passed_over(plan.stream.rate)),
        ));
    } else if config.equaliser.enabled {
        lines.push(words::main::explain_eq_unbound().to_owned());
    }
    if let Some(impulse) = plan.convolution.as_ref() {
        lines.push(
            words::main::ExplainRoom {
                channels: impulse.channels(),
                rate: impulse.rate().to_string(),
                taps: impulse.frames(),
            }
            .said(),
        );
    }
    if let Some(restoring) = plan.restoration {
        let wall = restoring.wall.map_or_else(
            || words::main::found_as_it_plays().to_owned(),
            |wall| {
                words::main::studied_at(format!("{:.1}", f64::from(wall.centihertz()) / 100_000.0))
            },
        );

        lines.push(
            words::main::ExplainRestore {
                restoration: restoring.restoration.as_str(),
                tuning: tuning_words(restoring.tuning),
                wall,
            }
            .said(),
        );
    }
    if let Some(crossfeed) = plan.crossfeed {
        lines.push(words::main::explain_crossfeed(crossfeed.as_str()));
    }
    if plan.true_peak {
        lines.push(words::main::explain_guard().to_owned());
    }
    if let Some(depth) = plan.dither_to {
        lines.push(
            words::main::ExplainDither {
                depth: depth.to_string(),
                shaping: shaping_words(plan.shaping),
            }
            .said(),
        );
    }
    if let Packing::DopMarked(_) = packing {
        lines.push(words::main::explain_dop(dop_reading(
            plan, sink, config, source,
        )));
    }
    lines
}

fn quality_words(quality: Quality) -> &'static str {
    match quality {
        Quality::LowLatency => words::main::quality_low_latency(),
        Quality::Balanced => words::main::quality_balanced(),
        Quality::High => words::main::quality_high(),
        Quality::VeryHigh => words::main::quality_very_high(),
    }
}

fn phase_words(phase: FilterPhase) -> &'static str {
    match phase {
        FilterPhase::Linear => words::main::phase_linear(),
        FilterPhase::Intermediate => words::main::phase_intermediate(),
        FilterPhase::Minimum => words::main::phase_minimum(),
    }
}

fn tuning_words(tuning: Tuning) -> &'static str {
    match tuning {
        Tuning::Mp3 => words::main::tuning_mp3(),
        Tuning::Aac => words::main::tuning_aac(),
        Tuning::Vorbis => words::main::tuning_vorbis(),
    }
}

fn shaping_words(shaping: NoiseShaping) -> &'static str {
    match shaping {
        NoiseShaping::None => words::main::shaping_flat(),
        NoiseShaping::Lipshitz => words::main::shaping_lipshitz(),
        NoiseShaping::Threshold => words::main::shaping_threshold(),
    }
}

fn above_nyquist(bands: usize) -> String {
    match bands {
        0 => String::new(),
        bands => format!(" ({})", words::main::above_nyquist(bands)),
    }
}

fn dop_reading(
    plan: &OutputPlan,
    sink: &SinkInfo,
    config: &EngineConfig,
    source: StreamSpec,
) -> &'static str {
    if matches!(plan.packing, Packing::DopMarked(_)) {
        return words::main::dop_packed();
    }
    if !config.dop {
        return words::main::dop_off();
    }
    if !sink.supports(source) {
        return words::main::dop_inexact();
    }
    words::main::dop_gain()
}

#[cfg(test)]
mod tests {
    use resonate_core::{ChannelLayout, SampleFormat, SampleRate, StreamSpec};
    use resonate_engine::{DitherKind, NodeName, Restoration, SinkFormats, SinkId, plan_output};

    use super::*;

    fn sink_taking_sixteen_bits_at_48k() -> SinkInfo {
        SinkInfo {
            id: SinkId::new(1),
            name: NodeName::new("test_dac"),
            aliases: Vec::new(),
            description: "Test DAC".to_owned(),
            is_default: true,
            is_hardware: true,
            port: None,
            profile: None,
            formats: vec![SinkFormats {
                format: SampleFormat::S16,
                words: Words::offered_for(SampleFormat::S16),
                rates: vec![SampleRate::HZ_48000],
                channels: vec![ChannelLayout::Stereo],
            }],
            allowed_rates: vec![SampleRate::HZ_48000],
            current_rate: Some(SampleRate::HZ_48000),
        }
    }

    fn explained(
        quality: Quality,
        phase: FilterPhase,
        tuning: Tuning,
        shaping: NoiseShaping,
    ) -> String {
        let sink = sink_taking_sixteen_bits_at_48k();
        let source = StreamSpec::new(
            SampleRate::HZ_96000,
            ChannelLayout::Stereo,
            SampleFormat::S24,
        );
        let config = EngineConfig {
            quality,
            filter_phase: phase,
            noise_shaping: shaping,
            dither: DitherKind::Triangular,
            restoration: Restoration::Repair,
            ..EngineConfig::default()
        };
        let decoded = Decoded {
            tuning: Some(tuning),
            ..Decoded::samples(source)
        };

        let plan = plan_output(decoded, &sink, &config, Default::default());

        lines(source, Packing::Samples, &sink, &config, &plan).join("\n")
    }

    fn assert_said_in_words(text: &str, fragment: &str, word: &str, debug: String) {
        assert!(text.contains(fragment), "{fragment} missing from {text}");
        assert!(
            word == debug || !text.contains(&debug),
            "{debug} printed raw in {text}"
        );
    }

    #[test]
    fn explain_says_every_setting_in_words_and_never_as_a_debug_spelling() {
        let qualities = [
            (Quality::LowLatency, "low latency"),
            (Quality::Balanced, "balanced"),
            (Quality::High, "high"),
            (Quality::VeryHigh, "very high"),
        ];
        let phases = [
            (FilterPhase::Linear, "linear"),
            (FilterPhase::Intermediate, "intermediate"),
            (FilterPhase::Minimum, "minimum"),
        ];
        let tunings = [
            (Tuning::Mp3, "MP3"),
            (Tuning::Aac, "AAC"),
            (Tuning::Vorbis, "Vorbis"),
        ];
        let shapings = [
            (NoiseShaping::None, "flat"),
            (NoiseShaping::Lipshitz, "lipshitz"),
            (NoiseShaping::Threshold, "threshold"),
        ];

        for (quality, quality_said) in qualities {
            for (phase, phase_said) in phases {
                for (tuning, tuning_said) in tunings {
                    for (shaping, shaping_said) in shapings {
                        let text = explained(quality, phase, tuning, shaping);

                        assert_said_in_words(
                            &text,
                            &format!("[{quality_said}, {phase_said} phase]"),
                            quality_said,
                            format!("{quality:?}"),
                        );
                        assert_said_in_words(
                            &text,
                            &format!("encoded as {tuning_said},"),
                            tuning_said,
                            format!("{tuning:?}"),
                        );
                        assert_said_in_words(
                            &text,
                            &format!("[{shaping_said}]"),
                            shaping_said,
                            format!("{shaping:?}"),
                        );

                        assert!(!text.contains(&format!("{phase:?}")), "{text}");
                    }
                }
            }
        }
    }
}
