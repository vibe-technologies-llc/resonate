use std::{
    net::{IpAddr, Ipv4Addr},
    str,
};

use crate::{Action, Located, description, http, http::HEADERS_AT_MOST, soap, ssdp};

const SENDER: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 9));

const DESCRIBED_AT: &str = "http://10.0.0.9:49152/desc.xml";

pub fn read_what_a_network_says(bytes: &[u8]) {
    if let Some(location) = ssdp::location_answered(bytes, SENDER) {
        assert!(location.is_at(SENDER));
    }

    let _ = http::unchunked(bytes);
    let mut headers = [httparse::EMPTY_HEADER; HEADERS_AT_MOST];
    let mut response = httparse::Response::new(&mut headers);
    if response.parse(bytes).is_ok() {
        let _ = http::length_of(response.headers);
    }

    let Ok(text) = str::from_utf8(bytes) else {
        return;
    };
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
