use std::{collections::BTreeSet, sync::Arc, thread};

use resonate_cast::{CAST_NODES, Casting, Searching as CastSearching};
use resonate_codec::Sources;
use resonate_engine::{EngineConfig, NodeName, Player, SinkError, SinkInfo};
use resonate_pipewire::PipeWire;
use resonate_upnp::{Beside, FirstSearch, Network, RENDERER_NODES, Searching, Udn, Vouched};

use crate::config::Config;

#[derive(Clone, Default)]
pub(crate) struct Rendering(Option<Kinds>);

#[derive(Clone)]
struct Kinds {
    upnp: Arc<Network>,
    cast: Arc<Casting>,
}

impl Kinds {
    fn resting(config: &Config) -> Option<Self> {
        let upnp = Network::resting(Searching::on_the_network())
            .inspect_err(|error| {
                tracing::warn!(%error, "UPnP renderers on the network cannot be played to");
            })
            .ok()?;
        upnp.vouch(config.renderers_vouched_for());
        for address in config.renderers_at() {
            if let Err(error) = upnp.locate(address.clone()) {
                tracing::warn!(%error, %address, "a renderer given by address could not be held");
            }
        }
        let cast = Casting::resting(CastSearching::on_the_network())
            .inspect_err(|error| {
                tracing::warn!(%error, "Cast receivers on the network cannot be played to");
            })
            .ok()?;
        Some(Self {
            upnp: Arc::new(upnp),
            cast: Arc::new(cast),
        })
    }

    fn look(&self, first: FirstSearch) {
        let (upnp, cast) = thread::scope(|scope| {
            let cast = scope.spawn(|| self.cast.look(first));
            (self.upnp.look(first), cast.join())
        });
        if let Err(error) = upnp {
            tracing::warn!(%error, "the network could not be searched for UPnP renderers");
        }
        match cast {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                tracing::warn!(%error, "the network could not be browsed for Cast receivers");
            }
            Err(_) => tracing::warn!("browsing for Cast receivers panicked"),
        }
    }

    fn reach_addresses(&self, first: FirstSearch) {
        if let Err(error) = self.upnp.look_at_addresses(first) {
            tracing::warn!(%error, "the renderers given by address could not be reached");
        }
    }

    #[cfg(feature = "ui")]
    fn rest(&self) {
        self.upnp.stop_multicasting();
        self.cast.rest();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    Nothing,
    Addresses,
    Network,
}

impl Rendering {
    pub(crate) fn of(config: &Config, wanted: Option<&NodeName>, asked: Asked) -> Self {
        let names_one = wanted.is_some_and(names_a_renderer);
        let searches = asked == Asked::Network || names_one || config.looks_for_renderers();
        let reaches = asked == Asked::Addresses || !config.renderers_at().is_empty();
        if !(searches || reaches) {
            return Self(None);
        }
        let first = if asked != Asked::Nothing || names_one {
            FirstSearch::Awaited
        } else {
            FirstSearch::Behind
        };
        let kinds = Kinds::resting(config);
        if let Some(kinds) = &kinds {
            if searches {
                kinds.look(first);
            } else {
                kinds.reach_addresses(first);
            }
        }
        Self(kinds)
    }

    #[cfg(feature = "ui")]
    pub(crate) fn for_the_window(config: &Config, wanted: Option<&NodeName>) -> Self {
        let names_one = wanted.is_some_and(names_a_renderer);
        let kinds = Kinds::resting(config);
        if let Some(kinds) = &kinds {
            let first = if names_one {
                FirstSearch::Awaited
            } else {
                FirstSearch::Behind
            };
            if names_one || config.looks_for_renderers() {
                kinds.look(first);
            } else if !config.renderers_at().is_empty() {
                kinds.reach_addresses(first);
            }
        }
        Self(kinds)
    }

    #[cfg(feature = "ui")]
    pub(crate) fn switch(&self) -> Option<Arc<dyn resonate_ui::Renderers>> {
        self.0
            .as_ref()
            .map(|kinds| Arc::new(Switch(kinds.clone())) as Arc<dyn resonate_ui::Renderers>)
    }

    pub(crate) fn sinks(&self) -> Vec<SinkInfo> {
        self.0.as_ref().map_or_else(Vec::new, |kinds| {
            let mut sinks = kinds.upnp.sinks();
            sinks.extend(kinds.cast.sinks());
            sinks
        })
    }

    pub(crate) fn player(
        &self,
        engine: EngineConfig,
        sources: Arc<Sources>,
    ) -> resonate_engine::Result<Player> {
        let Some(kinds) = self.0.clone() else {
            return Player::with_sources(engine, sources);
        };
        Player::with_sources_and_backend(engine, sources, move |config| {
            let graph = PipeWire::start(&config.app_name).map_err(SinkError::from)?;
            Ok(Box::new(
                Beside::over(Box::new(graph), kinds.upnp).and(kinds.cast),
            ))
        })
    }
}

pub(crate) fn names_a_renderer(sink: &NodeName) -> bool {
    let name = sink.as_str();
    name.starts_with(RENDERER_NODES) || name.starts_with(CAST_NODES)
}

#[cfg(feature = "ui")]
struct Switch(Kinds);

#[cfg(feature = "ui")]
impl resonate_ui::Renderers for Switch {
    fn looking(&self) -> bool {
        self.0.upnp.is_looking() || self.0.cast.is_looking()
    }

    fn look(&self, on: bool) {
        if on {
            self.0.look(FirstSearch::Behind);
        } else {
            self.0.rest();
        }
    }

    fn vouch(&self, renderers: &BTreeSet<NodeName>) {
        let vouched = Vouched::flac_at_any_rate(renderers.iter().filter_map(Udn::from_node));
        let upnp = Arc::clone(&self.0.upnp);
        thread::spawn(move || upnp.vouch(vouched));
    }
}
