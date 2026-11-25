use std::{sync::Arc, thread};

use resonate_cast::{CAST_NODES, Casting, Searching as CastSearching};
use resonate_codec::Sources;
use resonate_core::Switch;
use resonate_engine::{
    Backend, EngineConfig, Learning, NodeName, Player, SinkInfo, desktop_backend,
};
use resonate_upnp::{Beside, FirstSearch, Network, RENDERER_NODES, Searching};

use crate::{
    config::Config,
    error::{Error, Result},
};

pub(crate) struct Heard {
    pub sink: SinkInfo,
    pub learning: Learning,
}

#[derive(Clone, Default)]
pub(crate) struct Rendering(Option<Kinds>);

#[derive(Clone)]
struct Kinds {
    upnp: Option<Arc<Network>>,
    cast: Option<Arc<Casting>>,
}

impl Kinds {
    fn resting(config: &Config) -> Option<Self> {
        Self::started(
            config,
            Network::resting(Searching::on_the_network()),
            Casting::resting(CastSearching::on_the_network()),
        )
    }

    fn started(
        config: &Config,
        upnp: resonate_upnp::Result<Network>,
        cast: resonate_cast::Result<Casting>,
    ) -> Option<Self> {
        let upnp = upnp
            .inspect_err(|error| {
                tracing::warn!(%error, "UPnP renderers on the network cannot be played to");
            })
            .ok()
            .map(|upnp| {
                upnp.vouch(config.renderers_vouched_for());
                upnp.teach(config.formats_learnt_for().clone());
                for address in config.renderers_at() {
                    if let Err(error) = upnp.locate(address.clone()) {
                        tracing::warn!(%error, %address, "a renderer given by address could not be held");
                    }
                }
                Arc::new(upnp)
            });
        let cast = cast
            .inspect_err(|error| {
                tracing::warn!(%error, "Cast receivers on the network cannot be played to");
            })
            .ok()
            .map(|cast| {
                cast.teach(config.formats_learnt_for().clone());
                for address in config.cast_at() {
                    if let Err(error) = cast.locate(address.clone()) {
                        tracing::warn!(%error, %address, "a Cast receiver given by address could not be held");
                    }
                }
                Arc::new(cast)
            });

        (upnp.is_some() || cast.is_some()).then_some(Self { upnp, cast })
    }

    fn look(&self, first: FirstSearch) {
        let (upnp, cast) = thread::scope(|scope| {
            let cast = scope.spawn(|| self.cast.as_ref().map(|cast| cast.look(first)));
            (self.upnp.as_ref().map(|upnp| upnp.look(first)), cast.join())
        });

        if let Some(Err(error)) = upnp {
            tracing::warn!(%error, "the network could not be searched for UPnP renderers");
        }
        match cast {
            Ok(None | Some(Ok(()))) => {}
            Ok(Some(Err(error))) => {
                tracing::warn!(%error, "the network could not be browsed for Cast receivers");
            }
            Err(_) => tracing::warn!("browsing for Cast receivers panicked"),
        }
    }

    fn reach_addresses(&self, first: FirstSearch) {
        let (upnp, cast) = thread::scope(|scope| {
            let cast = scope.spawn(|| self.cast.as_ref().map(|cast| cast.look_at_addresses(first)));
            (
                self.upnp.as_ref().map(|upnp| upnp.look_at_addresses(first)),
                cast.join(),
            )
        });

        if let Some(Err(error)) = upnp {
            tracing::warn!(%error, "the renderers given by address could not be reached");
        }
        match cast {
            Ok(None | Some(Ok(()))) => {}
            Ok(Some(Err(error))) => {
                tracing::warn!(%error, "the Cast receivers given by address could not be reached");
            }
            Err(_) => tracing::warn!("reaching Cast receivers by address panicked"),
        }
    }

    fn sinks(&self) -> Vec<SinkInfo> {
        let mut sinks = self
            .upnp
            .as_ref()
            .map_or_else(Vec::new, |upnp| upnp.sinks());

        sinks.extend(self.cast.iter().flat_map(|cast| cast.sinks()));
        sinks
    }

    fn beside(&self, graph: Box<dyn Backend>) -> Box<dyn Backend> {
        match (&self.upnp, &self.cast) {
            (Some(upnp), Some(cast)) => {
                Box::new(Beside::over(graph, upnp.clone()).and(cast.clone()))
            }
            (Some(upnp), None) => Box::new(Beside::over(graph, upnp.clone())),
            (None, Some(cast)) => Box::new(Beside::over(graph, cast.clone())),
            (None, None) => graph,
        }
    }

    fn learn(&self, wanted: &NodeName) -> Result<Heard> {
        if let Some(upnp) = &self.upnp
            && let Some(sink) = upnp.sinks().into_iter().find(|sink| sink.is_named(wanted))
        {
            let learning = upnp
                .learn(sink.id)
                .map_err(|source| Error::RendererUnlearnt {
                    sink: sink.name.clone(),
                    source: Box::new(source),
                })?;

            return Ok(Heard { sink, learning });
        }
        if let Some(cast) = &self.cast
            && let Some(sink) = cast.sinks().into_iter().find(|sink| sink.is_named(wanted))
        {
            let learning = cast
                .learn(sink.id)
                .map_err(|source| Error::ReceiverUnlearnt {
                    sink: sink.name.clone(),
                    source: Box::new(source),
                })?;

            return Ok(Heard { sink, learning });
        }
        Err(Error::NotANetworkDevice(wanted.clone()))
    }

    #[cfg(feature = "ui")]
    fn rest(&self) {
        if let Some(upnp) = &self.upnp {
            upnp.stop_multicasting();
        }
        if let Some(cast) = &self.cast {
            cast.stop_browsing();
        }
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
        let searches = asked == Asked::Network || names_one || config.is_on(Switch::Renderers);
        let reaches = asked == Asked::Addresses || config.holds_receiver_addresses();

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
            if names_one || config.is_on(Switch::Renderers) {
                kinds.look(first);
            } else if config.holds_receiver_addresses() {
                kinds.reach_addresses(first);
            }
        }
        Self(kinds)
    }

    #[cfg(feature = "ui")]
    pub(crate) fn switch(&self) -> Option<Arc<dyn resonate_ui::Renderers>> {
        self.0
            .as_ref()
            .map(|kinds| Arc::new(Switched(kinds.clone())) as Arc<dyn resonate_ui::Renderers>)
    }

    pub(crate) fn learn(&self, wanted: &NodeName) -> Result<Heard> {
        self.0
            .as_ref()
            .ok_or_else(|| Error::NotANetworkDevice(wanted.clone()))?
            .learn(wanted)
    }

    pub(crate) fn sinks(&self) -> Vec<SinkInfo> {
        self.0.as_ref().map_or_else(Vec::new, Kinds::sinks)
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
            let graph = desktop_backend(&config.app_name)?;
            Ok(kinds.beside(graph))
        })
    }
}

pub(crate) fn names_a_renderer(sink: &NodeName) -> bool {
    let name = sink.as_str();
    name.starts_with(RENDERER_NODES) || name.starts_with(CAST_NODES)
}

#[cfg(feature = "ui")]
use std::collections::BTreeSet;

#[cfg(feature = "ui")]
use resonate_engine::{Learnt, Unlearnt};
#[cfg(feature = "ui")]
use resonate_upnp::{Udn, Vouched};

#[cfg(feature = "ui")]
struct Switched(Kinds);

#[cfg(feature = "ui")]
impl resonate_ui::Renderers for Switched {
    fn looking(&self) -> bool {
        self.0.upnp.as_ref().is_some_and(|upnp| upnp.is_looking())
            || self.0.cast.as_ref().is_some_and(|cast| cast.is_looking())
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
        let Some(upnp) = self.0.upnp.clone() else {
            return;
        };

        thread::spawn(move || upnp.vouch(vouched));
    }

    fn learn(&self, sink: &NodeName) -> std::result::Result<Learning, Unlearnt> {
        self.0
            .learn(sink)
            .map(|heard| heard.learning)
            .map_err(|error| {
                tracing::warn!(%error, %sink, "what a device plays was not learnt");
                unlearnt(&error)
            })
    }

    fn teach(&self, learnt: &Learnt) {
        let upnp = self.0.upnp.clone();
        let cast = self.0.cast.clone();
        let learnt = learnt.clone();

        thread::spawn(move || {
            if let Some(upnp) = upnp {
                upnp.teach(learnt.clone());
            }
            if let Some(cast) = cast {
                cast.teach(learnt);
            }
        });
    }
}

#[cfg(feature = "ui")]
fn unlearnt(error: &Error) -> Unlearnt {
    match error {
        Error::NotANetworkDevice(_) => Unlearnt::NotListed,
        Error::RendererUnlearnt { source, .. }
            if matches!(**source, resonate_upnp::Error::InUse(_)) =>
        {
            Unlearnt::InUse
        }
        Error::ReceiverUnlearnt { source, .. }
            if matches!(**source, resonate_cast::Error::InUse(_)) =>
        {
            Unlearnt::InUse
        }
        _ => Unlearnt::Unreached,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
        time::Duration,
    };

    use resonate_cast::{Error as CastError, LinkOp};
    use resonate_upnp::Located;

    use super::*;

    const DESCRIPTION: &str = r#"<?xml version="1.0"?><root xmlns="urn:schemas-upnp-org:device-1-0"><device><deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType><friendlyName>Kitchen</friendlyName><UDN>uuid:kitchen</UDN><serviceList><service><serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType><controlURL>/av</controlURL></service></serviceList></device></root>"#;

    fn a_renderer_describing_itself() -> Located {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let at = listener.local_addr().expect("its address");

        thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut asked = [0; 1024];

                let _ = stream.read(&mut asked);
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{DESCRIPTION}",
                    DESCRIPTION.len()
                );
            }
        });
        Located::parse(&format!("http://{at}/description.xml")).expect("reads")
    }

    fn searching_nowhere() -> Searching {
        Searching {
            to: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9),
            within: Duration::from_millis(100),
            every: Duration::from_secs(3_600),
            serving_on: IpAddr::V4(Ipv4Addr::LOCALHOST),
        }
    }

    fn cast_refused() -> resonate_cast::Result<Casting> {
        Err(CastError::Link {
            at: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
            op: LinkOp::Spawn,
            source: std::io::ErrorKind::Other.into(),
        })
    }

    #[test]
    fn upnp_renderers_are_still_listed_when_cast_cannot_start() {
        let renderer = a_renderer_describing_itself();
        let upnp = Network::resting(searching_nowhere()).expect("rests");

        upnp.locate(renderer).expect("holds the address");

        let kinds = Kinds::started(&Config::default(), Ok(upnp), cast_refused())
            .expect("one kind started is enough");

        kinds.reach_addresses(FirstSearch::Awaited);

        let rendering = Rendering(Some(kinds));
        let sinks = rendering.sinks();

        assert_eq!(sinks.len(), 1);
        assert_eq!(sinks[0].description, "Kitchen");
    }

    #[test]
    fn cast_receivers_are_kept_when_upnp_cannot_start_and_nothing_started_is_no_rendering() {
        let upnp_refused = || {
            Err(resonate_upnp::Error::Serve {
                op: resonate_upnp::ServeOp::Listen,
                source: std::io::ErrorKind::Other.into(),
            })
        };

        let cast = Casting::resting(CastSearching::on_the_network()).expect("rests");
        let kept = Kinds::started(&Config::default(), upnp_refused(), Ok(cast))
            .expect("one kind started is enough");

        assert!(kept.upnp.is_none());
        assert!(kept.cast.is_some());
        assert!(Kinds::started(&Config::default(), upnp_refused(), cast_refused()).is_none());
    }
}
