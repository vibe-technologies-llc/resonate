use std::time::Instant;

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

const NAME_BYTES_AT_MOST: usize = 256;

const UDN_BYTES_AT_MOST: usize = 256;

const SERVER_KIND: &str = ":device:MediaServer:";

const DIRECTORY_KIND: &str = ":service:ContentDirectory:";

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ServerDescribed {
    pub udn: Udn,
    pub name: String,
    pub directory: Service,
}

pub(crate) fn described_before(location: &Located, deadline: Instant) -> Result<Described> {
    read(location, &fetched(location, deadline)?)
}

pub(crate) fn server_described(location: &Located) -> Result<ServerDescribed> {
    server_described_before(location, Instant::now() + http::ANSWERED_WITHIN)
}

pub(crate) fn server_described_before(
    location: &Located,
    deadline: Instant,
) -> Result<ServerDescribed> {
    read_server(location, &fetched(location, deadline)?)
}

fn fetched(location: &Located, deadline: Instant) -> Result<String> {
    let answer = http::exchange_before(
        &Asking {
            method: "GET",
            to: location,
            headers: &[],
            body: &[],
        },
        Asked::Description,
        Instant::now() + http::answering_by(deadline),
    )?;

    if answer.status != 200 {
        return Err(Error::Status {
            asked: Asked::Description,
            status: answer.status,
        });
    }
    String::from_utf8(answer.body).map_err(|_| Error::NotText {
        asked: Asked::Description,
    })
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

fn base_of(root: Node<'_, '_>, location: &Located) -> Located {
    text_of(root, "URLBase")
        .and_then(|base| Located::parse(base).ok())
        .filter(|base| base.is_on_host_of(location))
        .unwrap_or_else(|| location.clone())
}

fn devices_in<'a, 'input>(root: Node<'a, 'input>) -> Vec<Node<'a, 'input>> {
    root.descendants()
        .filter(|node| node.tag_name().name() == "device")
        .collect()
}

pub(crate) fn read_server(location: &Located, text: &str) -> Result<ServerDescribed> {
    let document = document(text, Asked::Description)?;
    let root = document.root_element();
    let base = base_of(root, location);

    let devices = devices_in(root);
    let device = devices
        .iter()
        .find(|device| {
            child_text(**device, "deviceType").is_some_and(|kind| kind.contains(SERVER_KIND))
                && service_of(**device, location, &base, DIRECTORY_KIND).is_some()
        })
        .or_else(|| {
            devices
                .iter()
                .find(|device| service_of(**device, location, &base, DIRECTORY_KIND).is_some())
        })
        .ok_or(Error::NotAServer)?;

    let directory =
        service_of(*device, location, &base, DIRECTORY_KIND).ok_or(Error::NotAServer)?;
    let udn = udn_of(*device).ok_or(Error::NotAServer)?;
    let name = name_of(*device, &udn);

    Ok(ServerDescribed {
        udn: Udn::new(udn),
        name,
        directory,
    })
}

pub(crate) fn read(location: &Located, text: &str) -> Result<Described> {
    let document = document(text, Asked::Description)?;
    let root = document.root_element();
    let base = base_of(root, location);

    let devices = devices_in(root);
    let device = devices
        .iter()
        .find(|device| {
            child_text(**device, "deviceType").is_some_and(|kind| kind.contains(RENDERER_KIND))
        })
        .or_else(|| {
            devices
                .iter()
                .find(|device| service_of(**device, location, &base, TRANSPORT_KIND).is_some())
        })
        .ok_or(Error::NotARenderer)?;

    let transport =
        service_of(*device, location, &base, TRANSPORT_KIND).ok_or(Error::NotARenderer)?;
    let udn = udn_of(*device).ok_or(Error::NotARenderer)?;
    let name = name_of(*device, &udn);

    Ok(Described {
        udn: Udn::new(udn),
        name,
        transport,
        rendering: service_of(*device, location, &base, RENDERING_KIND),
        openhome_volume: service_of(*device, location, &base, OPENHOME_VOLUME_KIND),
        connection: service_of(*device, location, &base, CONNECTION_KIND),
    })
}

fn cleaned(text: &str, at_most: usize) -> String {
    let shown: String = text.chars().filter(|letter| !letter.is_control()).collect();
    let (kept, _) = shown.split_at(shown.floor_char_boundary(at_most));

    kept.trim().to_owned()
}

fn udn_of(device: Node<'_, '_>) -> Option<String> {
    child_text(device, "UDN")
        .map(|udn| cleaned(udn, UDN_BYTES_AT_MOST))
        .filter(|udn| !udn.is_empty())
}

fn name_of(device: Node<'_, '_>, udn: &str) -> String {
    child_text(device, "friendlyName")
        .map(|name| cleaned(name, NAME_BYTES_AT_MOST))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| udn.to_owned())
}

fn service_of(
    device: Node<'_, '_>,
    described_at: &Located,
    base: &Located,
    kind: &str,
) -> Option<Service> {
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

            if !control.is_on_host_of(described_at) {
                tracing::debug!(%control, %described_at, "a device's description names a control address on another host");
                return None;
            }
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
    fn a_control_address_on_another_host_than_the_description_is_not_taken() {
        let location = Located::parse("http://10.0.0.4:1400/d.xml").expect("reads");
        let device = |base: &str, control: &str| {
            format!(
                r#"<root>{base}<device><deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType><UDN>uuid:x</UDN><serviceList>
                <service><serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType><controlURL>{control}</controlURL></service>
                <service><serviceType>urn:schemas-upnp-org:service:RenderingControl:1</serviceType><controlURL>/rc</controlURL></service>
                </serviceList></device></root>"#
            )
        };

        assert!(matches!(
            read(&location, &device("", "http://127.0.0.1:8080/av")),
            Err(Error::NotARenderer)
        ));
        assert!(matches!(
            read(&location, &device("", "http://10.0.0.5/av")),
            Err(Error::NotARenderer)
        ));

        let same_host = read(&location, &device("", "http://10.0.0.4:9000/av")).expect("describes");

        assert_eq!(
            same_host.transport.control.to_string(),
            "http://10.0.0.4:9000/av"
        );

        let elsewhere = read(
            &location,
            &device("<URLBase>http://10.9.9.9:80/</URLBase>", "/av"),
        )
        .expect("describes");

        assert_eq!(
            elsewhere.transport.control.to_string(),
            "http://10.0.0.4:1400/av"
        );
        assert_eq!(
            elsewhere
                .rendering
                .map(|service| service.control.to_string()),
            Some("http://10.0.0.4:1400/rc".to_owned())
        );
    }

    const MINIDLNA: &str = r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0"><specVersion><major>1</major><minor>0</minor></specVersion>
<device><deviceType>urn:schemas-upnp-org:device:MediaServer:1</deviceType><friendlyName>nas: minidlna</friendlyName>
<manufacturer>Justin Maggard</manufacturer><UDN>uuid:4d696e69-444c-164e-9d41-001132a1b2c3</UDN>
<serviceList>
<service><serviceType>urn:schemas-upnp-org:service:ContentDirectory:1</serviceType><serviceId>urn:upnp-org:serviceId:ContentDirectory</serviceId><controlURL>/ctl/ContentDir</controlURL><eventSubURL>/evt/ContentDir</eventSubURL><SCPDURL>/ContentDir.xml</SCPDURL></service>
<service><serviceType>urn:schemas-upnp-org:service:ConnectionManager:1</serviceType><serviceId>urn:upnp-org:serviceId:ConnectionManager</serviceId><controlURL>/ctl/ConnectionMgr</controlURL><eventSubURL>/evt/ConnectionMgr</eventSubURL><SCPDURL>/ConnectionMgr.xml</SCPDURL></service>
</serviceList></device></root>"#;

    #[test]
    fn a_media_server_is_read_by_its_name_and_the_directory_it_is_browsed_through() {
        let location = Located::parse("http://192.168.1.4:8200/rootDesc.xml").expect("reads");

        let described = read_server(&location, MINIDLNA).expect("describes a server");

        assert_eq!(
            described.udn.as_str(),
            "uuid:4d696e69-444c-164e-9d41-001132a1b2c3"
        );
        assert_eq!(described.name, "nas: minidlna");
        assert_eq!(
            described.directory.control.to_string(),
            "http://192.168.1.4:8200/ctl/ContentDir"
        );
        assert!(matches!(
            read(&location, MINIDLNA),
            Err(Error::NotARenderer)
        ));
        assert!(matches!(
            read_server(&location, EMBEDDED),
            Err(Error::NotAServer)
        ));
    }

    #[test]
    fn a_server_whose_directory_is_on_another_host_is_no_server_here() {
        let location = Located::parse("http://192.168.1.4:8200/rootDesc.xml").expect("reads");
        let elsewhere = MINIDLNA.replace("/ctl/ContentDir", "http://192.168.1.5/ctl/ContentDir");

        assert!(matches!(
            read_server(&location, &elsewhere),
            Err(Error::NotAServer)
        ));
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

    #[test]
    fn a_name_and_a_udn_are_cleaned_of_control_characters_and_cut_on_a_letter() {
        let location = Located::parse("http://10.0.0.4:80/d.xml").expect("reads");
        let sprawling = format!("Living\nroom\r\t{}", "é\n".repeat(1 << 19));
        let udn = format!("uuid:\u{85}a{}", "b".repeat(1 << 20));
        let text = format!(
            r#"<root><device><deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType><friendlyName>{sprawling}</friendlyName><UDN>{udn}</UDN><serviceList>
            <service><serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType><controlURL>/av</controlURL></service>
            </serviceList></device></root>"#
        );

        let described = read(&location, &text).expect("describes a renderer");

        assert!(described.name.starts_with("Livingroom"));
        assert!(described.name.len() <= NAME_BYTES_AT_MOST);
        assert!(described.name.chars().all(|letter| !letter.is_control()));
        assert!(described.udn.as_str().starts_with("uuid:ab"));
        assert!(described.udn.as_str().len() <= UDN_BYTES_AT_MOST);
        assert!(
            described
                .udn
                .as_str()
                .chars()
                .all(|letter| !letter.is_control())
        );
    }

    #[test]
    fn a_name_of_nothing_but_controls_is_the_udn_instead() {
        let location = Located::parse("http://10.0.0.4:80/d.xml").expect("reads");
        let text = "<root><device><deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType><friendlyName>&#10;&#10;</friendlyName><UDN>uuid:x</UDN><serviceList><service><serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType><controlURL>/av</controlURL></service></serviceList></device></root>";

        let described = read(&location, text).expect("describes a renderer");

        assert_eq!(described.name, "uuid:x");
    }
}
