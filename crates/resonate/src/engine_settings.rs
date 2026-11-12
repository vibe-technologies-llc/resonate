use std::{path::Path, sync::Arc};

use resonate_codec::Sources;
use resonate_core::{MediaLocation, Speakers};
use resonate_engine::{BluetoothWake, EngineConfig, Impulse, Levelling, read_impulse};

use crate::{
    cli::Cli,
    config::Config,
    equaliser,
    error::{Error, Result},
    sinks::wanted_sink,
};

fn impulse_at(path: &Path, speakers: Speakers) -> Option<Arc<Impulse>> {
    match read_impulse(&Sources::local(), &MediaLocation::local(path), speakers) {
        Ok(Some(impulse)) => Some(Arc::new(impulse)),
        Ok(None) => {
            tracing::warn!(path = %path.display(), "the room's response holds no taps, so nothing is corrected");
            None
        }
        Err(error) => {
            tracing::warn!(%error, "the room's response could not be read, so nothing is corrected");
            None
        }
    }
}

pub(crate) const fn one_way_on_bit_perfect(cli: &Cli) -> Result<()> {
    if cli.bit_perfect && cli.no_bit_perfect {
        Err(Error::BitPerfectBothWays)
    } else {
        Ok(())
    }
}

const fn bit_perfect_asked(cli: &Cli) -> Option<bool> {
    match (cli.bit_perfect, cli.no_bit_perfect) {
        (true, _) => Some(true),
        (_, true) => Some(false),
        (false, false) => None,
    }
}

pub(crate) fn engine_config(cli: &Cli, config: &Config) -> EngineConfig {
    let defaults = EngineConfig::default();
    EngineConfig {
        sink: wanted_sink(cli, config),
        quality: cli
            .quality
            .map(Into::into)
            .or(config.quality)
            .unwrap_or(defaults.quality),
        filter_phase: cli
            .filter_phase
            .map(Into::into)
            .or(config.filter_phase)
            .unwrap_or(defaults.filter_phase),
        dither: cli
            .dither
            .map(Into::into)
            .or(config.dither)
            .unwrap_or(defaults.dither),
        noise_shaping: cli
            .noise_shaping
            .map(Into::into)
            .or(config.noise_shaping)
            .unwrap_or(defaults.noise_shaping),
        replay_gain: config.replay_gain.unwrap_or(defaults.replay_gain),
        levelling: Levelling {
            pre_amp: config.pre_amp.unwrap_or(defaults.levelling.pre_amp),
            untagged: config.untagged.unwrap_or(defaults.levelling.untagged),
        },
        prefer_bit_perfect: bit_perfect_asked(cli)
            .or(config.bit_perfect)
            .unwrap_or(defaults.prefer_bit_perfect),
        dop: config.dop.unwrap_or(defaults.dop),
        dsd_like_pcm: config.dsd_like_pcm.unwrap_or(defaults.dsd_like_pcm),
        device_volume: config.device_volume.unwrap_or(defaults.device_volume),
        true_peak: config.true_peak.unwrap_or(defaults.true_peak),
        restoration: config.restoration.unwrap_or(defaults.restoration),
        crossfeed: config.crossfeed.unwrap_or(defaults.crossfeed),
        force_graph_rate: config.force_graph_rate.unwrap_or(defaults.force_graph_rate),
        bluetooth: BluetoothWake {
            on: config.bluetooth_wake.unwrap_or(defaults.bluetooth.on),
            lead: config.bluetooth_lead.unwrap_or(defaults.bluetooth.lead),
            awake_for: config
                .bluetooth_awake
                .unwrap_or(defaults.bluetooth.awake_for),
        },
        volume: config.volume.unwrap_or(defaults.volume),
        buffer: config.buffer.unwrap_or(defaults.buffer),
        equaliser: Arc::new(equaliser::resolved(config)),
        convolution: config
            .convolution
            .as_deref()
            .and_then(|path| impulse_at(path, config.speakers())),
        skip_under_repeat: config.skip_under_repeat(),
        previous_restarts: config.previous_restarts(),
        smart_shuffle: config.smart_shuffle(),
        ..defaults
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::fixtures::parsed;

    #[test]
    fn bit_perfect_is_asked_for_or_refused_for_one_run_and_never_both() {
        let asked = |arguments: &[&str]| bit_perfect_asked(&parsed(arguments));
        let refused = |arguments: &[&str]| {
            matches!(
                one_way_on_bit_perfect(&parsed(arguments)),
                Err(Error::BitPerfectBothWays)
            )
        };

        assert_eq!(asked(&["resonate", "sinks"]), None);
        assert_eq!(asked(&["resonate", "--bit-perfect", "sinks"]), Some(true));
        assert_eq!(asked(&["resonate", "sinks", "--bit-perfect"]), Some(true));
        assert_eq!(
            asked(&["resonate", "sinks", "--no-bit-perfect"]),
            Some(false)
        );
        assert!(!refused(&["resonate", "sinks", "--bit-perfect"]));
        assert!(refused(&[
            "resonate",
            "--no-bit-perfect",
            "--bit-perfect",
            "sinks"
        ]));
        assert!(refused(&[
            "resonate",
            "--no-bit-perfect",
            "sinks",
            "--bit-perfect"
        ]));
        assert!(refused(&[
            "resonate",
            "--bit-perfect",
            "sinks",
            "--no-bit-perfect"
        ]));
    }
}
