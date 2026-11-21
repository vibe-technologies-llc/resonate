use std::{
    io,
    net::{IpAddr, Ipv4Addr},
    str,
};

use resonate_core::{AlbumId, TrackId};

use crate::{
    Action, Located, Udn, announce,
    catalog::{
        Catalog, Held, Kind, Object, Opened, Page, Picture, Query, Searched, Shelf, Shelved, Song,
        Within,
    },
    criteria::{self, Described, RELATIONS_AT_MOST},
    description,
    directory::{self, Answered, Asking, Service},
    events::{self, Subscribing},
    fetched, http,
    http::HEADERS_AT_MOST,
    listing::{self, ENTRIES_A_LISTING_AT_MOST, TEXT_CHARS_AT_MOST},
    naming,
    offered::{self, Wanted},
    serve, soap, ssdp,
    watched::Watched,
};

const SERVED_FROM: &str = "http://10.0.0.2:8200";

struct Nothing;

impl Catalog for Nothing {
    type Failure = io::Error;

    fn searched(&self, _: Kind, _: Within, query: &Query, page: Page) -> io::Result<Searched> {
        Ok(match query {
            Query::Present { .. } => Searched::Unanswered,
            _ => Searched::Found(Shelved {
                held: Vec::new(),
                total: page.from,
            }),
        })
    }

    fn shelved(&self, shelf: Shelf, page: Page) -> io::Result<Option<Shelved>> {
        Ok(shelf.is_top().then(|| Shelved {
            held: Vec::new(),
            total: page.from,
        }))
    }

    fn shelf(&self, _: Shelf) -> io::Result<Option<Held>> {
        Ok(None)
    }

    fn song(&self, _: TrackId) -> io::Result<Option<Song>> {
        Ok(None)
    }

    fn bytes_of(&self, _: TrackId) -> io::Result<Option<Opened>> {
        Ok(None)
    }

    fn cover(&self, _: AlbumId) -> io::Result<Option<Picture>> {
        Ok(None)
    }

    fn revision(&self) -> u32 {
        0
    }
}

fn relations_in(query: &Query) -> usize {
    match query {
        Query::Everything => 0,
        Query::All(all) | Query::Any(all) => all.iter().map(relations_in).sum(),
        Query::Text { .. } | Query::Present { .. } => 1,
    }
}

fn read_what_a_control_point_asks(bytes: &[u8], text: Option<&str>) {
    let _ = announce::searched_for(bytes);
    let _ = offered::routed(bytes);

    for method in [events::Method::Subscribe, events::Method::Unsubscribe] {
        if let Subscribing::New { callback, kept } = events::subscribing(bytes, method, SENDER) {
            assert!(callback.is_at(SENDER));
            assert!(kept >= events::KEPT_AT_LEAST && kept <= events::KEPT_AT_MOST);
        }
    }

    let Some(text) = text else {
        return;
    };

    if let Ok(criterion) = criteria::read(text) {
        for kind in [Kind::Artist, Kind::Album, Kind::Song, Kind::Playlist] {
            if let criteria::Reduced::Asks(query) =
                criteria::reduced(&criterion, &Described::of(kind))
            {
                assert!(relations_in(&query) <= RELATIONS_AT_MOST);
            }
        }
    }

    let _ = offered::route_of(text);

    if let Some(object) = Object::read(text) {
        assert_eq!(object.to_string(), text);
    }

    let _ = Udn::read(text);
    let length = bytes.len() as u64;

    if let Wanted::Part { from, to } = offered::wanted(Some(text), length) {
        assert!(from <= to && to < length);
    }

    for service in [Service::ContentDirectory, Service::ConnectionManager] {
        let Some(called) = directory::called(text, service) else {
            continue;
        };
        let watched = Watched::default();
        let asking = Asking {
            catalog: &Nothing,
            base: SERVED_FROM,
            name: naming::server_name(),
            watched: &watched,
        };

        let envelope = match asking.answer(service, &called) {
            Answered::Said(envelope) => envelope,
            Answered::Faulted(fault) => directory::fault_envelope(fault),
        };
        let answered = roxmltree::Document::parse(&envelope).expect("an answer reads as XML");
        if let Some(didl) = answered
            .descendants()
            .find(|node| node.tag_name().name() == "Result")
            .and_then(|node| node.text())
        {
            assert!(roxmltree::Document::parse(didl).is_ok());
        }
    }
}

const SENDER: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 9));

const DESCRIBED_AT: &str = "http://10.0.0.9:49152/desc.xml";

const SERVED_FROM_AT: &str = "http://10.0.0.9:8200/rootDesc.xml";

const ALSO_SERVING_AT: &str = "http://10.0.0.10/";

pub fn read_a_listing(bytes: &[u8]) {
    let served_from = Located::parse(SERVED_FROM_AT).expect("a fixed address reads");
    let also = Located::parse(ALSO_SERVING_AT).expect("a fixed address reads");
    let also_serving = |at: &Located| at.is_on_host_of(&also);

    let origin = listing::Origin {
        served_from: &served_from,
        also_serving: &also_serving,
    };
    let on_the_server = |at: &Located| at.is_on_host_of(&served_from) || at.is_on_host_of(&also);
    let Ok(text) = str::from_utf8(bytes) else {
        return;
    };
    let didl = match soap::answered(text, Action::Browse) {
        Ok(answered) => answered.named("Result").map(str::to_owned),
        Err(_) => None,
    };

    for text in [Some(text.to_owned()), didl].into_iter().flatten() {
        let Ok(listing) = listing::read_from(&text, &origin) else {
            continue;
        };

        assert!(listing.len() <= ENTRIES_A_LISTING_AT_MOST);

        for container in &listing.containers {
            assert!(!container.id.as_str().is_empty());
            assert!(!container.title.is_empty());
            assert!(container.title.chars().count() <= TEXT_CHARS_AT_MOST);
            assert!(container.cover.as_ref().is_none_or(on_the_server));
        }
        for item in &listing.items {
            assert!(!item.id.as_str().is_empty());
            assert!(!item.title.chars().any(char::is_control));
            assert!(item.played_from().is_some());
            for resource in &item.resources {
                assert!(on_the_server(&resource.at));
                assert!(Located::parse(&resource.at.to_string()).is_ok());
            }
        }
    }

    let _ = listing::length_of(text);
    let _ = fetched::range_of(text);
}

pub fn read_what_a_network_says(bytes: &[u8]) {
    for sought in [ssdp::Sought::Renderers, ssdp::Sought::Servers] {
        if let Some(answered) = ssdp::location_answered(bytes, SENDER, sought) {
            assert!(answered.location.ip().is_some());
            assert!(!answered.location.ip().is_some_and(|ip| ip.is_loopback()));
            assert!(sought == ssdp::Sought::Servers || answered.location.is_at(SENDER));
        }
    }

    serve::read_a_request_head(bytes);

    let _ = http::unchunked(bytes);
    let mut headers = [httparse::EMPTY_HEADER; HEADERS_AT_MOST];

    let mut response = httparse::Response::new(&mut headers);

    if response.parse(bytes).is_ok() {
        let _ = http::length_of(response.headers);
    }

    let Ok(text) = str::from_utf8(bytes) else {
        read_what_a_control_point_asks(bytes, None);
        return;
    };

    read_what_a_control_point_asks(bytes, Some(text));

    let described_at = Located::parse(DESCRIBED_AT).expect("a fixed address reads");

    if let Ok(server) = description::read_server(&described_at, text) {
        assert!(server.directory.control.is_on_host_of(&described_at));
    }
    if let Ok(described) = description::read(&described_at, text) {
        assert!(described.transport.control.is_on_host_of(&described_at));
        for service in [
            described.rendering,
            described.openhome_volume,
            described.connection,
        ]
        .into_iter()
        .flatten()
        {
            assert!(service.control.is_on_host_of(&described_at));
        }
    }

    let _ = soap::answered(text, Action::GetPositionInfo);
    let _ = soap::fault_code(text, Action::Play);

    if let Ok(located) = Located::parse(text) {
        let _ = located.to_string();
    }

    let _ = described_at.join(text);
}
