use std::{
    io,
    net::{IpAddr, Ipv4Addr},
    str,
};

use resonate_core::{AlbumId, TrackId};

use crate::{
    Action, Located, Udn, announce,
    catalog::{Catalog, Held, Object, Opened, Page, Picture, Shelf, Shelved, Song},
    description,
    directory::{self, Answered, Asking, Service},
    http,
    http::HEADERS_AT_MOST,
    offered::{self, Wanted},
    serve, soap, ssdp,
};

const SERVED_FROM: &str = "http://10.0.0.2:8200";

struct Nothing;

impl Catalog for Nothing {
    type Failure = io::Error;

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

fn read_what_a_control_point_asks(bytes: &[u8], text: Option<&str>) {
    let _ = announce::searched_for(bytes);
    let _ = offered::routed(bytes);
    let Some(text) = text else {
        return;
    };

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
        let asking = Asking {
            catalog: &Nothing,
            base: SERVED_FROM,
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

pub fn read_what_a_network_says(bytes: &[u8]) {
    if let Some(location) = ssdp::location_answered(bytes, SENDER) {
        assert!(location.is_at(SENDER));
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
