use std::sync::Arc;

use resonate_codec::Sources;
use resonate_engine::{EngineConfig, NodeName, Player, SinkError, SinkInfo};
use resonate_pipewire::PipeWire;
use resonate_upnp::{Beside, FirstSearch, Network, RENDERER_NODES, Searching};

use crate::config::Config;

#[derive(Clone, Default)]
pub(crate) struct Rendering(Option<Arc<Network>>);

impl Rendering {
    pub(crate) fn of(config: &Config, wanted: Option<&NodeName>, asked: bool) -> Self {
        let names_one = wanted.is_some_and(names_a_renderer);
        if !(asked || names_one || config.looks_for_renderers()) {
            return Self(None);
        }
        let first = if asked || names_one {
            FirstSearch::Awaited
        } else {
            FirstSearch::Behind
        };
        match Network::start(Searching::on_the_network(), first) {
            Ok(network) => Self(Some(Arc::new(network))),
            Err(error) => {
                tracing::warn!(%error, "the network could not be searched for renderers");
                Self(None)
            }
        }
    }

    pub(crate) fn sinks(&self) -> Vec<SinkInfo> {
        self.0
            .as_ref()
            .map_or_else(Vec::new, |network| network.sinks())
    }

    pub(crate) fn player(
        &self,
        engine: EngineConfig,
        sources: Arc<Sources>,
    ) -> resonate_engine::Result<Player> {
        let Some(network) = self.0.as_ref().map(Arc::clone) else {
            return Player::with_sources(engine, sources);
        };
        Player::with_sources_and_backend(engine, sources, move |config| {
            let graph = PipeWire::start(&config.app_name).map_err(SinkError::from)?;
            Ok(Box::new(Beside::over(Box::new(graph), network)))
        })
    }
}

pub(crate) fn names_a_renderer(sink: &NodeName) -> bool {
    sink.as_str().starts_with(RENDERER_NODES)
}
