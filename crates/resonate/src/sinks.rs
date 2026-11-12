use std::time::Duration;
#[cfg(feature = "ui")]
use std::{sync::Arc, thread};

use resonate_cast::Given;
use resonate_core::{SampleRate, Volume};
use resonate_engine::{
    HardwareVolume, NodeName, Player, Plugged, SinkError, SinkFormats, SinkInfo,
};
use resonate_library::{Failure, Failures};
use resonate_upnp::Located;

use crate::{
    cli::Cli,
    config::Config,
    error::{Error, Result},
    rendering,
    rendering::{Asked, Rendering},
    surveyed,
    table::Table,
    words,
    words::columns,
};

pub(crate) const SINK_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) fn wanted_sink(cli: &Cli, config: &Config) -> Option<NodeName> {
    cli.sink
        .as_deref()
        .map(NodeName::new)
        .or_else(|| config.sink.clone())
}

pub(crate) fn rates(rates: &[SampleRate]) -> String {
    if rates.is_empty() {
        return "none".to_owned();
    }
    rates
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" / ")
}

pub(crate) fn select_sink(sinks: Vec<SinkInfo>, wanted: Option<&NodeName>) -> Result<SinkInfo> {
    let Some(wanted) = wanted else {
        return sinks
            .into_iter()
            .find(|sink| sink.is_default)
            .ok_or(Error::Sink(SinkError::NoSink));
    };
    sinks
        .into_iter()
        .find(|sink| sink.is_named(wanted))
        .ok_or_else(|| Error::SinkNotFound(wanted.clone()))
}

pub(crate) fn confirm_sink(player: &Player, wanted: Option<&NodeName>) {
    let Some(wanted) = wanted else {
        return;
    };
    if player
        .sinks_once_surveyed(SINK_TIMEOUT)
        .iter()
        .any(|sink| sink.is_named(wanted))
    {
        return;
    }
    tracing::warn!(
        %wanted,
        "the chosen sink is not in the graph; the default device plays until it appears"
    );
}

#[cfg(feature = "ui")]
pub(crate) fn confirm_sink_once_the_window_is_open(player: &Arc<Player>, wanted: Option<NodeName>) {
    let Some(wanted) = wanted else {
        return;
    };
    let player = Arc::downgrade(player);
    let spawned = thread::Builder::new()
        .name("resonate-sink-survey".to_owned())
        .spawn(move || {
            if let Some(player) = player.upgrade() {
                confirm_sink(&player, Some(&wanted));
            }
        });
    if let Err(error) = spawned {
        tracing::debug!(%error, "the chosen sink was not looked for in the graph");
    }
}

pub(crate) fn broken_down(failed: Failures) -> String {
    let named: Vec<String> = Failure::ALL
        .into_iter()
        .filter(|failure| failed.counted(*failure) > 0)
        .map(|failure| format!("{} {}", failed.counted(failure), failure.as_str()))
        .collect();

    if named.is_empty() {
        String::new()
    } else {
        format!(" ({})", named.join(", "))
    }
}

fn with_receiver_addresses(
    config: &Config,
    renderers_at: &[String],
    cast_at: &[String],
) -> Result<Config> {
    let mut config = config.clone();
    for address in renderers_at {
        let located = Located::parse(address).map_err(|source| Error::RendererAddress {
            address: address.clone(),
            source: Box::new(source),
        })?;
        config.renderers_at.insert(located);
    }
    for address in cast_at {
        let receiver = Given::parse(address).ok_or_else(|| Error::CastAddress {
            address: address.clone(),
        })?;
        config.cast_at.insert(receiver);
    }
    Ok(config)
}

pub(crate) fn list_sinks(
    cli: &Cli,
    config: &Config,
    renderers: bool,
    renderers_at: &[String],
    cast_at: &[String],
) -> Result<()> {
    let mut sinks = surveyed::sinks()?;
    let config = with_receiver_addresses(config, renderers_at, cast_at)?;
    let asked = if renderers {
        Asked::Network
    } else if renderers_at.is_empty() && cast_at.is_empty() {
        Asked::Nothing
    } else {
        Asked::Addresses
    };
    sinks.extend(Rendering::of(&config, wanted_sink(cli, &config).as_ref(), asked).sinks());

    let mut table = Table::new(vec![
        "",
        columns::device(),
        columns::node_name(),
        columns::driven_by(),
        columns::profile(),
        columns::port(),
        columns::format(),
        columns::advertises(),
        columns::graph(),
        columns::now(),
    ]);

    for sink in &sinks {
        let marker = if sink.is_default { "*" } else { "" };
        let current = sink.current_rate.map_or_else(
            || words::main::unknown().to_owned(),
            |rate: SampleRate| rate.to_string(),
        );

        let mut formats = sink.formats.iter();
        let first = formats.next();
        table.push(vec![
            marker.to_owned(),
            sink.description.clone(),
            sink.name.to_string(),
            driven_by(sink).to_owned(),
            sink.profile.as_ref().map_or_else(
                || words::main::none().to_owned(),
                |profile| profile.description.clone(),
            ),
            comes_out_of(sink),
            first.map_or_else(|| words::main::none().to_owned(), SinkFormats::spelled),
            first.map_or_else(
                || words::main::none().to_owned(),
                |entry| rates(&entry.rates),
            ),
            rates(&sink.allowed_rates),
            current,
        ]);

        for entry in formats {
            table.push(vec![
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                entry.spelled(),
                rates(&entry.rates),
            ]);
        }
    }

    said_on!("{}", table.render());
    Ok(())
}

fn driven_by(sink: &SinkInfo) -> &'static str {
    if rendering::names_a_renderer(&sink.name) {
        words::main::the_network()
    } else if sink.is_hardware {
        words::main::a_device()
    } else {
        words::main::the_graph()
    }
}

fn comes_out_of(sink: &SinkInfo) -> String {
    let Some(port) = sink.port.as_ref() else {
        return words::main::none().to_owned();
    };
    let plugged = match port.plugged {
        Plugged::Unsaid => port.description.clone(),
        Plugged::No => words::main::nothing_plugged_in(&port.description),
        Plugged::Yes => words::main::plugged_in(&port.description),
    };
    let volume = match port.hardware_volume {
        HardwareVolume::Unsaid => plugged,
        HardwareVolume::No => words::main::volume_in_software(plugged),
        HardwareVolume::Yes => words::main::volume_on_the_hardware(plugged),
        HardwareVolume::Always => words::main::volume_always_on_the_device(plugged),
    };
    let level = match port.volume {
        Some(gain) => words::main::heard_at(Volume::heard_at(gain).to_string(), volume),
        None => volume,
    };
    if port.muted {
        words::main::muted(level)
    } else {
        level
    }
}
