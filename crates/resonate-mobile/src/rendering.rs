use std::sync::Arc;

use resonate_cast::{Casting, Given, Searching as CastSearching};
use resonate_engine::{Command, NodeName};
use resonate_upnp::{FirstSearch, Located, Network, Searching};

use crate::{
    Error, MobilePlayer, Result,
    zoning::{KeptRenderer, zone_named},
};

#[derive(Clone)]
pub(crate) struct Kinds {
    pub(crate) upnp: Arc<Network>,
    pub(crate) cast: Arc<Casting>,
}

#[derive(Clone, Default)]
pub(crate) struct Rendering(Option<Kinds>);

impl Rendering {
    pub(crate) fn resting(searching: Searching, browsing: CastSearching) -> Self {
        let upnp = Network::resting(searching)
            .inspect_err(|error| {
                tracing::warn!(%error, "renderers on the network cannot be played to");
            })
            .ok();
        let cast = Casting::resting(browsing)
            .inspect_err(|error| {
                tracing::warn!(%error, "Cast receivers on the network cannot be played to");
            })
            .ok();
        match (upnp, cast) {
            (Some(upnp), Some(cast)) => Self(Some(Kinds {
                upnp: Arc::new(upnp),
                cast: Arc::new(cast),
            })),
            _ => Self(None),
        }
    }

    pub(crate) fn on_the_network() -> Self {
        Self::resting(Searching::on_the_network(), CastSearching::on_the_network())
    }

    pub(crate) fn kinds(&self) -> Option<Kinds> {
        self.0.clone()
    }

    pub(crate) fn look_behind(&self) {
        let Some(kinds) = &self.0 else {
            return;
        };
        if let Err(error) = kinds.upnp.look(FirstSearch::Behind) {
            tracing::warn!(%error, "the network could not be searched for renderers");
        }
        if let Err(error) = kinds.cast.look(FirstSearch::Behind) {
            tracing::warn!(%error, "the network could not be browsed for Cast receivers");
        }
    }

    fn listed(&self) -> Vec<Listed> {
        let Some(kinds) = &self.0 else {
            return Vec::new();
        };
        let renderers = kinds
            .upnp
            .renderers()
            .into_iter()
            .map(|(_, renderer)| Listed {
                renderer: NetworkRenderer {
                    node: renderer.node_name().as_str().to_owned(),
                    name: renderer.name().to_owned(),
                    host: renderer.location().host().to_owned(),
                    kind: RendererKind::Upnp,
                },
                aliases: Vec::new(),
            });
        let sinks = kinds.cast.sinks();
        let receivers = kinds
            .cast
            .receivers()
            .into_iter()
            .filter_map(|(id, device)| {
                let sink = sinks.iter().find(|sink| sink.id == id)?;
                Some(Listed {
                    renderer: NetworkRenderer {
                        node: sink.name.as_str().to_owned(),
                        name: device.name().to_owned(),
                        host: device.at().ip().to_string(),
                        kind: RendererKind::Cast,
                    },
                    aliases: sink
                        .aliases
                        .iter()
                        .map(|alias| alias.as_str().to_owned())
                        .collect(),
                })
            });
        renderers.chain(receivers).collect()
    }

    pub(crate) fn named(&self, node: &str) -> Option<NetworkRenderer> {
        self.listed()
            .into_iter()
            .find(|listed| listed.answers_to(node))
            .map(|listed| listed.renderer)
    }
}

struct Listed {
    renderer: NetworkRenderer,
    aliases: Vec<String>,
}

impl Listed {
    fn answers_to(&self, node: &str) -> bool {
        self.renderer.node == node || self.aliases.iter().any(|alias| alias == node)
    }
}

pub(crate) fn sink_of(renderer: Option<&KeptRenderer>) -> Option<NodeName> {
    renderer.map(|renderer| NodeName::new(renderer.node.as_str()))
}

fn renderer_address(source: resonate_upnp::Error) -> Error {
    Error::RendererAddress {
        source: Box::new(source),
    }
}

fn receiver_unreached(source: resonate_cast::Error) -> Error {
    Error::ReceiverUnreached {
        source: Box::new(source),
    }
}

fn receiver_search(source: resonate_cast::Error) -> Error {
    Error::ReceiverSearch {
        source: Box::new(source),
    }
}

fn receiver_given(address: &str) -> Result<Given> {
    Given::parse(address).ok_or(Error::ReceiverAddress)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RendererKind {
    Upnp,
    Cast,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NetworkRenderer {
    pub node: String,
    pub name: String,
    pub host: String,
    pub kind: RendererKind,
}

#[uniffi::export]
impl MobilePlayer {
    pub fn look_for_renderers(&self, on: bool) -> Result<()> {
        let kinds = self.rendering.kinds().ok_or(Error::RenderersUnreachable)?;
        if !on {
            kinds.upnp.stop_multicasting();
            kinds.cast.stop_browsing();
            return Ok(());
        }
        let renderers =
            kinds
                .upnp
                .look(FirstSearch::Behind)
                .map_err(|source| Error::RendererSearch {
                    source: Box::new(source),
                });
        let receivers = kinds
            .cast
            .look(FirstSearch::Behind)
            .map_err(receiver_search);
        renderers.and(receivers)
    }

    pub fn add_renderer_address(&self, address: String) -> Result<()> {
        let network = self
            .rendering
            .kinds()
            .ok_or(Error::RenderersUnreachable)?
            .upnp;
        let located = Located::parse(&address).map_err(renderer_address)?;
        let reaching = network.is_reaching();
        let located = network.locate(located);
        if !reaching {
            network
                .look_at_addresses(FirstSearch::Behind)
                .map_err(|source| Error::RendererSearch {
                    source: Box::new(source),
                })?;
        }
        located.map_err(renderer_address)
    }

    pub fn remove_renderer_address(&self, address: String) -> Result<()> {
        let network = self
            .rendering
            .kinds()
            .ok_or(Error::RenderersUnreachable)?
            .upnp;
        network.unlocate(&Located::parse(&address).map_err(renderer_address)?);
        if network.located().is_empty() && !network.is_looking() {
            network.rest();
        }
        Ok(())
    }

    pub fn renderer_addresses(&self) -> Vec<String> {
        self.rendering.kinds().map_or_else(Vec::new, |kinds| {
            kinds
                .upnp
                .located()
                .iter()
                .map(ToString::to_string)
                .collect()
        })
    }

    pub fn add_receiver_address(&self, address: String) -> Result<()> {
        let casting = self
            .rendering
            .kinds()
            .ok_or(Error::RenderersUnreachable)?
            .cast;
        let given = receiver_given(&address)?;
        let reaching = casting.is_reaching();
        let located = casting.locate(given);
        if !reaching {
            casting
                .look_at_addresses(FirstSearch::Behind)
                .map_err(receiver_search)?;
        }
        located.map_err(receiver_unreached)
    }

    pub fn remove_receiver_address(&self, address: String) -> Result<()> {
        let casting = self
            .rendering
            .kinds()
            .ok_or(Error::RenderersUnreachable)?
            .cast;
        casting.unlocate(receiver_given(&address)?);
        if casting.located().is_empty() && !casting.is_looking() {
            casting.rest();
        }
        Ok(())
    }

    pub fn receiver_addresses(&self) -> Vec<String> {
        self.rendering.kinds().map_or_else(Vec::new, |kinds| {
            kinds
                .cast
                .located()
                .iter()
                .map(ToString::to_string)
                .collect()
        })
    }

    pub fn looking_for_renderers(&self) -> bool {
        self.rendering
            .kinds()
            .is_some_and(|kinds| kinds.upnp.is_looking() || kinds.cast.is_looking())
    }

    pub fn renderers(&self) -> Vec<NetworkRenderer> {
        self.rendering
            .listed()
            .into_iter()
            .map(|listed| listed.renderer)
            .collect()
    }

    pub fn select_zone_renderer(&self, zone: u32, renderer: Option<String>) -> Result<()> {
        let id = zone_named(zone)?;
        let zoned = self.playing.zoned(id)?;
        let name = self
            .playing
            .zones()?
            .zone(id)
            .ok_or(Error::NoZone { zone })?
            .name;
        let chosen = match renderer {
            None => None,
            Some(node) => {
                let listed = self.rendering.named(&node).ok_or(Error::UnknownRenderer)?;
                Some(KeptRenderer {
                    node: listed.node,
                    name: listed.name,
                })
            }
        };

        self.playing
            .send_to(id, Command::SetSink(sink_of(chosen.as_ref())))?;
        *zoned.renderer.lock() = chosen;
        self.catalog.keep_zone(&zoned.kept(id, &name))?;
        self.services.watching.tell();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket},
        sync::atomic::{AtomicUsize, Ordering},
        thread,
        time::{Duration, Instant},
    };

    use parking_lot::Mutex;
    use resonate_core::{Volume, ZoneId};

    use super::*;
    use crate::{
        Transport,
        harness::{Harness, LOOKED_AGAIN_AFTER, SETTLED_WITHIN},
        zoning::KeptZone,
    };

    const UDN: &str = "uuid:phone-renderer";
    const NAME: &str = "Fake streamer";

    struct Fake {
        search_at: SocketAddr,
        described_at: SocketAddr,
        actions: Arc<Mutex<Vec<String>>>,
        searched: Arc<AtomicUsize>,
    }

    impl Fake {
        fn start() -> Self {
            let ssdp = UdpSocket::bind("127.0.0.1:0").expect("a loopback socket for the search");
            let http = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
            let search_at = ssdp.local_addr().expect("the search address");
            let described_at = http.local_addr().expect("the description address");
            let actions = Arc::new(Mutex::new(Vec::new()));

            let searched = Arc::new(AtomicUsize::new(0));
            let counted = Arc::clone(&searched);
            thread::spawn(move || answer_searches(&ssdp, described_at, &counted));
            let heard = Arc::clone(&actions);
            thread::spawn(move || {
                for stream in http.incoming().flatten() {
                    let heard = Arc::clone(&heard);
                    thread::spawn(move || answer(stream, &heard));
                }
            });

            Self {
                search_at,
                described_at,
                actions,
                searched,
            }
        }

        fn searching(&self) -> Searching {
            Searching {
                to: self.search_at,
                within: Duration::from_millis(300),
                every: Duration::from_secs(3_600),
                serving_on: IpAddr::V4(Ipv4Addr::LOCALHOST),
            }
        }

        fn description_address(&self) -> String {
            format!("http://{}/description.xml", self.described_at)
        }

        fn searches(&self) -> usize {
            self.searched.load(Ordering::SeqCst)
        }

        fn heard(&self, action: &str) -> usize {
            self.actions
                .lock()
                .iter()
                .filter(|heard| *heard == action)
                .count()
        }

        fn until_heard(&self, action: &str, times: usize) {
            let began = Instant::now();
            while self.heard(action) < times {
                assert!(
                    began.elapsed() < SETTLED_WITHIN,
                    "the renderer never heard {action}: {:?}",
                    self.actions.lock()
                );
                thread::sleep(LOOKED_AGAIN_AFTER);
            }
        }
    }

    fn answer_searches(ssdp: &UdpSocket, described_at: SocketAddr, searched: &AtomicUsize) {
        let mut buffer = [0; 2048];
        while let Ok((read, from)) = ssdp.recv_from(&mut buffer) {
            let asked = String::from_utf8_lossy(&buffer[..read]).into_owned();
            if !asked.starts_with("M-SEARCH") {
                continue;
            }
            searched.fetch_add(1, Ordering::SeqCst);
            let answer = format!(
                "HTTP/1.1 200 OK\r\nLOCATION: http://{described_at}/description.xml\r\nST: \
                 urn:schemas-upnp-org:device:MediaRenderer:1\r\nUSN: {UDN}\r\n\r\n"
            );
            let _ = ssdp.send_to(answer.as_bytes(), from);
        }
    }

    fn answer(mut stream: TcpStream, actions: &Mutex<Vec<String>>) {
        let mut held = Vec::new();
        let mut buffer = [0; 4096];
        let head_ends = loop {
            if let Some(at) = held.windows(4).position(|window| window == b"\r\n\r\n") {
                break at + 4;
            }
            match stream.read(&mut buffer) {
                Ok(0) | Err(_) => return,
                Ok(read) => held.extend_from_slice(&buffer[..read]),
            }
        };
        let head = String::from_utf8_lossy(&held[..head_ends]).into_owned();
        let header = |name: &str| {
            head.lines().find_map(|line| {
                let (named, value) = line.split_once(':')?;
                named
                    .trim()
                    .eq_ignore_ascii_case(name)
                    .then(|| value.trim().to_owned())
            })
        };
        let length: usize = header("Content-Length")
            .and_then(|length| length.parse().ok())
            .unwrap_or(0);
        let mut body = held.len() - head_ends;
        while body < length {
            match stream.read(&mut buffer) {
                Ok(0) | Err(_) => return,
                Ok(read) => body += read,
            }
        }

        let answered = if head.starts_with("GET /description.xml") {
            format!(
                r#"<?xml version="1.0"?><root xmlns="urn:schemas-upnp-org:device-1-0"><device><deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType><friendlyName>{NAME}</friendlyName><UDN>{UDN}</UDN><serviceList><service><serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType><controlURL>/av</controlURL></service></serviceList></device></root>"#
            )
        } else {
            let action = header("SOAPAction")
                .and_then(|named| {
                    named
                        .trim_matches('"')
                        .split_once('#')
                        .map(|(_, action)| action.to_owned())
                })
                .unwrap_or_default();
            actions.lock().push(action.clone());
            format!(
                "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body><u:{action}Response xmlns:u=\"urn:schemas-upnp-org:service:AVTransport:1\"></u:{action}Response></s:Body></s:Envelope>"
            )
        };
        let _ = stream.write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answered}",
                answered.len()
            )
            .as_bytes(),
        );
    }

    fn found(harness: &Harness) -> NetworkRenderer {
        let began = Instant::now();
        loop {
            if let Some(renderer) = harness.player.renderers().into_iter().next() {
                return renderer;
            }
            assert!(
                began.elapsed() < SETTLED_WITHIN,
                "the renderer on the loopback was never found"
            );
            thread::sleep(LOOKED_AGAIN_AFTER);
        }
    }

    fn first_kept(harness: &Harness) -> Option<KeptRenderer> {
        harness
            .player
            .catalog
            .kept_zones()
            .expect("read")
            .into_iter()
            .find(|kept| kept.id.is_first())
            .and_then(|kept| kept.renderer)
    }

    #[test]
    fn a_renderer_found_on_the_network_is_listed_and_a_zone_bound_to_it_plays_there_and_is_kept() {
        let fake = Fake::start();
        let harness = Harness::beside_renderers(fake.searching());
        let resting = harness.player.looking_for_renderers();
        let unlisted = harness.player.renderers();

        harness.player.look_for_renderers(true).expect("looking");
        let renderer = found(&harness);

        assert!(!resting);
        assert!(unlisted.is_empty());
        assert!(harness.player.looking_for_renderers());
        assert_eq!(
            renderer,
            NetworkRenderer {
                node: format!("upnp:{UDN}"),
                name: NAME.to_owned(),
                host: "127.0.0.1".to_owned(),
                kind: RendererKind::Upnp,
            }
        );

        harness.resumed("abc", 0, 0);
        harness
            .player
            .select_zone_renderer(1, Some(renderer.node.clone()))
            .expect("the first zone on the renderer");
        harness.player.play().expect("played");
        fake.until_heard("SetAVTransportURI", 1);
        fake.until_heard("Play", 1);
        let zone = harness.player.zones().remove(0);

        assert_eq!(zone.renderer.as_deref(), Some(renderer.node.as_str()));
        assert_eq!(zone.renderer_name.as_deref(), Some(NAME));
        assert_eq!(
            first_kept(&harness),
            Some(KeptRenderer {
                node: renderer.node.clone(),
                name: NAME.to_owned(),
            })
        );

        harness
            .player
            .output_route_changed(48_000, 10.0, true)
            .expect("a route reset");
        harness.settled(|snapshot| snapshot.transport == Transport::Playing);

        assert_eq!(fake.heard("Stop"), 0);
        assert_eq!(
            harness.player.zones()[0].renderer.as_deref(),
            Some(renderer.node.as_str())
        );

        harness
            .player
            .select_zone_renderer(1, None)
            .expect("back on the phone");
        fake.until_heard("Stop", 1);

        assert_eq!(harness.player.zones()[0].renderer, None);
        assert_eq!(first_kept(&harness), None);
        assert!(matches!(
            harness
                .player
                .select_zone_renderer(1, Some("upnp:uuid:nobody".to_owned())),
            Err(Error::UnknownRenderer)
        ));

        harness.player.look_for_renderers(false).expect("resting");

        assert!(!harness.player.looking_for_renderers());
        assert!(harness.player.renderers().is_empty());
    }

    #[test]
    fn a_renderer_added_by_address_is_listed_without_a_search_and_forgotten_with_it() {
        let fake = Fake::start();
        let harness = Harness::beside_renderers(fake.searching());

        harness
            .player
            .add_renderer_address(fake.description_address())
            .expect("the address is reached");
        let renderer = found(&harness);

        assert_eq!(
            harness.player.renderer_addresses(),
            [fake.description_address()]
        );
        assert_eq!(renderer.node, format!("upnp:{UDN}"));
        assert_eq!(renderer.host, "127.0.0.1");
        assert!(!harness.player.looking_for_renderers());
        assert_eq!(fake.searches(), 0);

        harness
            .player
            .remove_renderer_address(fake.description_address())
            .expect("forgotten");

        assert!(harness.player.renderers().is_empty());
        assert!(harness.player.renderer_addresses().is_empty());
        assert!(matches!(
            harness
                .player
                .add_renderer_address("https://not-http/".to_owned()),
            Err(Error::RendererAddress { .. })
        ));
    }

    #[test]
    fn turning_the_search_off_keeps_the_renderer_given_by_address() {
        let fake = Fake::start();
        let harness = Harness::beside_renderers(fake.searching());
        harness
            .player
            .add_renderer_address(fake.description_address())
            .expect("the address is reached");
        harness.player.look_for_renderers(true).expect("looking");
        found(&harness);
        assert!(harness.player.looking_for_renderers());

        harness.player.look_for_renderers(false).expect("stopped");

        assert!(!harness.player.looking_for_renderers());
        assert_eq!(harness.player.renderers().len(), 1);
        assert_eq!(harness.player.renderer_addresses().len(), 1);
    }

    fn nothing_listening() -> String {
        let closed = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let at = closed.local_addr().expect("the address");
        drop(closed);
        at.to_string()
    }

    #[test]
    fn a_receiver_given_by_address_is_held_with_no_search_and_forgotten() {
        let harness = Harness::beside_renderers(Fake::start().searching());
        let silent = nothing_listening();
        let also_silent = nothing_listening();

        let first = harness.player.add_receiver_address(silent.clone());
        let reaching = harness.player.add_receiver_address(also_silent.clone());

        assert!(first.is_ok());
        assert!(matches!(reaching, Err(Error::ReceiverUnreached { .. })));
        let mut held = harness.player.receiver_addresses();
        held.sort();
        let mut given = vec![silent.clone(), also_silent.clone()];
        given.sort();
        assert_eq!(held, given);
        assert!(harness.player.renderers().is_empty());
        assert!(!harness.player.looking_for_renderers());
        assert!(harness.player.renderer_addresses().is_empty());

        harness
            .player
            .remove_receiver_address(silent)
            .expect("forgotten");
        harness
            .player
            .remove_receiver_address(also_silent.clone())
            .expect("forgotten");

        assert!(harness.player.receiver_addresses().is_empty());
    }

    #[test]
    fn a_receiver_address_that_is_neither_an_address_nor_a_name_is_refused_and_a_name_is_held() {
        let harness = Harness::beside_renderers(Fake::start().searching());

        for refused in [
            "",
            "not an address!",
            "192.168.1.300",
            "http://192.168.1.30/",
        ] {
            assert!(
                matches!(
                    harness.player.add_receiver_address(refused.to_owned()),
                    Err(Error::ReceiverAddress)
                ),
                "{refused:?} was taken for a receiver"
            );
        }
        assert!(harness.player.receiver_addresses().is_empty());

        harness
            .player
            .add_receiver_address("living-room.invalid".to_owned())
            .expect("a name is held until it is reached");

        assert_eq!(
            harness.player.receiver_addresses(),
            ["living-room.invalid:8009"]
        );
    }

    #[test]
    fn one_switch_looks_for_renderers_and_receivers_and_turning_it_off_keeps_the_addresses() {
        let fake = Fake::start();
        let harness = Harness::beside_renderers(fake.searching());
        let _ = harness.player.add_receiver_address(nothing_listening());
        let casting = harness.player.rendering.kinds().expect("both kinds").cast;
        assert!(!casting.is_looking());

        harness.player.look_for_renderers(true).expect("looking");
        found(&harness);

        assert!(harness.player.looking_for_renderers());
        assert!(casting.is_looking());

        harness.player.look_for_renderers(false).expect("stopped");

        assert!(!harness.player.looking_for_renderers());
        assert!(!casting.is_looking());
        assert_eq!(harness.player.receiver_addresses().len(), 1);
    }

    #[test]
    fn a_zone_kept_on_a_renderer_reopens_wanting_it_and_names_it_as_kept_until_it_is_found() {
        let harness = Harness::new();
        let kept = KeptZone {
            id: ZoneId::new(4).expect("a zone"),
            name: "Study".into(),
            route: String::new(),
            volume: Volume::MAX,
            renderer: Some(KeptRenderer {
                node: format!("upnp:{UDN}"),
                name: NAME.to_owned(),
            }),
        };
        let zones = harness.player.playing.zones().expect("the zones");

        let study = harness
            .player
            .zone_opened(&zones, &kept, 0)
            .expect("a zone opened from the kept row");
        let listed = harness
            .player
            .zones()
            .into_iter()
            .find(|zone| zone.id == study.id.get())
            .expect("the study listed");

        assert_eq!(
            harness
                .player
                .playing
                .wanted_sink(study.id)
                .expect("a zone"),
            Some(NodeName::new(format!("upnp:{UDN}")))
        );
        assert_eq!(listed.renderer_name.as_deref(), Some(NAME));
        assert_eq!(
            crate::zoning::first_zones_sink(&[KeptZone {
                id: ZoneId::FIRST,
                ..kept.clone()
            }]),
            Some(NodeName::new(format!("upnp:{UDN}")))
        );
        assert_eq!(crate::zoning::first_zones_sink(&[kept]), None);
        assert!(matches!(
            harness.player.look_for_renderers(true),
            Err(Error::RenderersUnreachable)
        ));
    }
}
