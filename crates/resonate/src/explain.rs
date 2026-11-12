use std::path::Path;

use resonate_codec::Packing;
use resonate_core::{MediaLocation, StreamSpec};
use resonate_engine::{
    Decoded, EngineConfig, OutputPlan, SinkInfo, Words, plan_output, resolve_replay_gain,
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

    said!("{}", words::main::explain_source(info.spec.to_string()));
    if let Packing::DopMarked(rate) = info.packing {
        said!("{}", words::main::explain_dsd(rate.hz(), rate.to_string()));
    }
    said!(
        "{}",
        words::main::explain_sink(&sink.description, sink.name.as_str())
    );
    said!(
        "{}",
        words::main::explain_output(format!("{:?}", plan.mode), plan.stream.to_string())
    );
    if let Some(framing) = sink.words_for(plan.stream.format)
        && framing != Words::Whole
    {
        said!(
            "{}",
            words::main::explain_words(
                framing.spelled(plan.stream.format),
                Words::offered_for(plan.stream.format).spelled(plan.stream.format),
            )
        );
    }
    if let Some((from, to)) = plan.remix {
        said!(
            "{}",
            words::main::explain_remix(from.to_string(), to.to_string())
        );
    }
    if let Some((from, to)) = plan.resample {
        said!(
            "{}",
            words::main::explain_resample(
                from.to_string(),
                format!("{:?}", config.filter_phase),
                format!("{:?}", config.quality),
                to.to_string(),
            )
        );
    }
    if let Some(profile) = plan.equalisation.as_ref() {
        said!(
            "{}{}",
            words::main::explain_eq(
                profile.applied(plan.stream.rate),
                format!("{:+.2}", profile.peak_db(plan.stream.rate)),
                profile.preamp().to_string(),
            ),
            above_nyquist(profile.passed_over(plan.stream.rate)),
        );
    } else if config.equaliser.enabled {
        said!("{}", words::main::explain_eq_unbound());
    }
    if let Some(impulse) = plan.convolution.as_ref() {
        said!(
            "{}",
            words::main::explain_room(
                impulse.channels(),
                impulse.rate().to_string(),
                impulse.frames(),
            )
        );
    }
    if let Some(restoring) = plan.restoration {
        let wall = restoring.wall.map_or_else(
            || words::main::found_as_it_plays().to_owned(),
            |wall| {
                words::main::studied_at(format!("{:.1}", f64::from(wall.centihertz()) / 100_000.0))
            },
        );
        said!(
            "{}",
            words::main::explain_restore(
                restoring.restoration.as_str(),
                format!("{:?}", restoring.tuning),
                wall,
            )
        );
    }
    if let Some(crossfeed) = plan.crossfeed {
        said!("{}", words::main::explain_crossfeed(crossfeed.as_str()));
    }
    if plan.true_peak {
        said!("{}", words::main::explain_guard());
    }
    if let Some(depth) = plan.dither_to {
        said!(
            "{}",
            words::main::explain_dither(depth.to_string(), format!("{:?}", plan.shaping))
        );
    }
    if let Packing::DopMarked(_) = info.packing {
        said!(
            "{}",
            words::main::explain_dop(dop_reading(&plan, &sink, &config, info.spec))
        );
    }
    Ok(())
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
