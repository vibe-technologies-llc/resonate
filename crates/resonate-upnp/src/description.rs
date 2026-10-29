use roxmltree::{Document, Node, ParsingOptions};

use crate::{
    Asked, Error, Located, Result, Udn,
    http::{self, Asking},
};

pub(crate) const NODES_AT_MOST: u32 = 100_000;

const RENDERER_KIND: &str = ":device:MediaRenderer:";

const TRANSPORT_KIND: &str = ":service:AVTransport:";

const RENDERING_KIND: &str = ":service:RenderingControl:";

const OPENHOME_VOLUME_KIND: &str = "urn:av-openhome-org:service:Volume:";

const CONNECTION_KIND: &str = ":service:ConnectionManager:";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Service {
    pub kind: Box<str>,
    pub control: Located,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Described {
    pub udn: Udn,
    pub name: String,
    pub transport: Service,
    pub rendering: Option<Service>,
    pub openhome_volume: Option<Service>,
    pub connection: Option<Service>,
}

pub(crate) fn described(location: &Located) -> Result<Described> {
    let answer = http::exchange(
        &Asking {
            method: "GET",
            to: location,
            headers: &[],
            body: &[],
        },
        Asked::Description,
    )?;
    if answer.status != 200 {
        return Err(Error::Status {
            asked: Asked::Description,
            status: answer.status,
        });
    }
    let text = std::str::from_utf8(&answer.body).map_err(|_| Error::NotText {
        asked: Asked::Description,
    })?;
    read(location, text)
}

pub(crate) fn document(text: &str, asked: Asked) -> Result<Document<'_>> {
    let options = ParsingOptions {
        allow_dtd: false,
        nodes_limit: NODES_AT_MOST,
        ..ParsingOptions::default()
    };
    Document::parse_with_options(text, options).map_err(|source| Error::NotADocument {
        asked,
        source: Box::new(source),
    })
}

pub(crate) fn read(location: &Located, text: &str) -> Result<Described> {
    let document = document(text, Asked::Description)?;
    let root = document.root_element();
    let base = match text_of(root, "URLBase") {
        Some(base) => Located::parse(base).unwrap_or_else(|_| location.clone()),
        None => location.clone(),
    };

    let devices: Vec<Node<'_, '_>> = root
        .descendants()
        .filter(|node| node.tag_name().name() == "device")
        .collect();
    let device = devices
        .iter()
        .find(|device| {
            child_text(**device, "deviceType").is_some_and(|kind| kind.contains(RENDERER_KIND))
        })
        .or_else(|| {
            devices
                .iter()
                .find(|device| service_of(**device, &base, TRANSPORT_KIND).is_some())
        })
        .ok_or(Error::NotARenderer)?;

    let transport = service_of(*device, &base, TRANSPORT_KIND).ok_or(Error::NotARenderer)?;
    let udn = child_text(*device, "UDN").ok_or(Error::NotARenderer)?;
    let name = child_text(*device, "friendlyName").unwrap_or(udn);

    Ok(Described {
        udn: Udn::new(udn),
        name: name.to_owned(),
        transport,
        rendering: service_of(*device, &base, RENDERING_KIND),
        openhome_volume: service_of(*device, &base, OPENHOME_VOLUME_KIND),
        connection: service_of(*device, &base, CONNECTION_KIND),
    })
}

fn service_of(device: Node<'_, '_>, base: &Located, kind: &str) -> Option<Service> {
    device
        .children()
        .filter(|node| node.tag_name().name() == "serviceList")
        .flat_map(|list| list.children())
        .filter(|node| node.tag_name().name() == "service")
        .find_map(|service| {
            let named = child_text(service, "serviceType")?;
            if !named.contains(kind) {
                return None;
            }
            let control = base.join(child_text(service, "controlURL")?).ok()?;
            Some(Service {
                kind: named.into(),
                control,
            })
        })
}

fn child_text<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.children()
        .find(|child| child.tag_name().name() == name)
        .and_then(|child| child.text())
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

fn text_of<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.descendants()
        .find(|child| child.tag_name().name() == name)
        .and_then(|child| child.text())
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMBEDDED: &str = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
  <specVersion><major>1</major><minor>0</minor></specVersion>
  <device>
    <deviceType>urn:schemas-upnp-org:device:ZonePlayer:1</deviceType>
    <friendlyName>Living room</friendlyName>
    <UDN>uuid:RINCON_1</UDN>
    <deviceList>
      <device>
        <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
        <friendlyName>Living room - renderer</friendlyName>
        <UDN> uuid:RINCON_1_MR </UDN>
        <serviceList>
          <service>
            <serviceType>urn:schemas-upnp-org:service:RenderingControl:1</serviceType>
            <controlURL>/MediaRenderer/RenderingControl/Control</controlURL>
          </service>
          <service>
            <serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType>
            <controlURL>MediaRenderer/AVTransport/Control</controlURL>
          </service>
          <service>
            <serviceType>urn:schemas-upnp-org:service:ConnectionManager:1</serviceType>
            <controlURL>/MediaRenderer/ConnectionManager/Control</controlURL>
          </service>
        </serviceList>
      </device>
    </deviceList>
  </device>
</root>"#;

    #[test]
    fn a_renderer_inside_another_device_is_found_with_its_services_read_against_the_description() {
        let location =
            Located::parse("http://10.0.0.4:1400/xml/device_description.xml").expect("reads");

        let described = read(&location, EMBEDDED).expect("describes a renderer");

        assert_eq!(described.udn.as_str(), "uuid:RINCON_1_MR");
        assert_eq!(described.name, "Living room - renderer");
        assert_eq!(
            described.transport.control.to_string(),
            "http://10.0.0.4:1400/xml/MediaRenderer/AVTransport/Control"
        );
        assert_eq!(
            described.transport.kind.as_ref(),
            "urn:schemas-upnp-org:service:AVTransport:1"
        );
        assert_eq!(
            described
                .rendering
                .map(|service| service.control.to_string()),
            Some("http://10.0.0.4:1400/MediaRenderer/RenderingControl/Control".to_owned())
        );
        assert!(described.connection.is_some());
    }

    #[test]
    fn an_openhome_volume_service_is_read_beside_the_upnp_ones_and_a_playlist_is_not_looked_for() {
        let location = Located::parse("http://10.0.0.4:80/d.xml").expect("reads");
        let text = r#"<root><device><deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType><UDN>uuid:oh</UDN><serviceList>
            <service><serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType><controlURL>/av</controlURL></service>
            <service><serviceType>urn:av-openhome-org:service:Playlist:1</serviceType><controlURL>/playlist</controlURL></service>
            <service><serviceType>urn:av-openhome-org:service:Volume:1</serviceType><controlURL>/volume</controlURL></service>
            </serviceList></device></root>"#;

        let described = read(&location, text).expect("describes a renderer");

        assert_eq!(
            described
                .openhome_volume
                .map(|service| service.control.to_string()),
            Some("http://10.0.0.4:80/volume".to_owned())
        );
        assert!(described.rendering.is_none());
    }

    #[test]
    fn a_device_with_no_transport_is_no_renderer_and_a_dtd_is_refused() {
        let location = Located::parse("http://10.0.0.4/d.xml").expect("reads");
        let server = r#"<root><device><deviceType>urn:schemas-upnp-org:device:MediaServer:1</deviceType><UDN>uuid:s</UDN></device></root>"#;
        let dtd = r#"<!DOCTYPE root [<!ENTITY a "aaaa">]><root>&a;</root>"#;

        assert!(matches!(read(&location, server), Err(Error::NotARenderer)));
        assert!(matches!(
            read(&location, dtd),
            Err(Error::NotADocument { .. })
        ));
    }
}
