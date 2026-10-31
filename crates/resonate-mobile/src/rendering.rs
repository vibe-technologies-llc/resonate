use std::sync::Arc;

use resonate_engine::{Command, NodeName};
use resonate_upnp::{FirstSearch, Located, Network, Searching};

use crate::{
    Error, MobilePlayer, Result,
    zoning::{KeptRenderer, zone_named},
};

#[derive(Clone, Default)]
pub(crate) struct Rendering(Option<Arc<Network>>);

impl Rendering {
    pub(crate) fn resting(searching: Searching) -> Self {
        match Network::resting(searching) {
            Ok(network) => Self(Some(Arc::new(network))),
            Err(error) => {
                tracing::warn!(%error, "renderers on the network cannot be played to");
                Self(None)
            }
        }
    }

    pub(crate) fn on_the_network() -> Self {
        Self::resting(Searching::on_the_network())
    }

    pub(crate) fn network(&self) -> Option<Arc<Network>> {
        self.0.clone()
    }

    pub(crate) fn look_behind(&self) {
        let Some(network) = &self.0 else {
            return;
        };
        if let Err(error) = network.look(FirstSearch::Behind) {
            tracing::warn!(%error, "the network could not be searched for renderers");
        }
    }

    fn listed(&self) -> Vec<NetworkRenderer> {
        self.0.as_ref().map_or_else(Vec::new, |network| {
            network
                .renderers()
                .into_iter()
                .map(|(_, renderer)| NetworkRenderer {
                    node: renderer.node_name().as_str().to_owned(),
                    name: renderer.name().to_owned(),
                    host: renderer.location().host().to_owned(),
                })
                .collect()
        })
    }

    pub(crate) fn named(&self, node: &str) -> Option<String> {
        self.listed()
            .into_iter()
            .find(|renderer| renderer.node == node)
            .map(|renderer| renderer.name)
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

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct NetworkRenderer {
    pub node: String,
    pub name: String,
    pub host: String,
}

#[uniffi::export]
impl MobilePlayer {
    pub fn look_for_renderers(&self, on: bool) -> Result<()> {
        let network = self
            .rendering
            .network()
            .ok_or(Error::RenderersUnreachable)?;
        if !on {
            network.stop_multicasting();
            return Ok(());
        }
        network
            .look(FirstSearch::Behind)
            .map_err(|source| Error::RendererSearch {
                source: Box::new(source),
            })
    }

    pub fn add_renderer_address(&self, address: String) -> Result<()> {
        let network = self
            .rendering
            .network()
            .ok_or(Error::RenderersUnreachable)?;
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
            .network()
            .ok_or(Error::RenderersUnreachable)?;
        network.unlocate(&Located::parse(&address).map_err(renderer_address)?);
        if network.located().is_empty() && !network.is_looking() {
            network.rest();
        }
        Ok(())
    }

    pub fn renderer_addresses(&self) -> Vec<String> {
        self.rendering.network().map_or_else(Vec::new, |network| {
            network.located().iter().map(ToString::to_string).collect()
        })
    }

    pub fn looking_for_renderers(&self) -> bool {
        self.rendering
            .network()
            .is_some_and(|network| network.is_looking())
    }

    pub fn renderers(&self) -> Vec<NetworkRenderer> {
        self.rendering.listed()
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
                let name = self.rendering.named(&node).ok_or(Error::UnknownRenderer)?;
                Some(KeptRenderer { node, name })
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
