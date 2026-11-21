use std::fmt::Write as _;

use crate::{Udn, didl::escaped, directory::Service};

pub(crate) const SERVER_TYPE: &str = "urn:schemas-upnp-org:device:MediaServer:1";

pub(crate) const DESCRIBED_AT: &str = "/description.xml";

pub(crate) const CONTROLLED_UNDER: &str = "/control/";

pub(crate) const EVENTED_UNDER: &str = "/event/";

const DESCRIBED_UNDER: &str = "/";

const MAKER: &str = "Resonate";

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub(crate) fn description_path(service: Service) -> String {
    format!("{DESCRIBED_UNDER}{}.xml", service.name())
}

pub(crate) fn control_path(service: Service) -> String {
    format!("{CONTROLLED_UNDER}{}", service.name())
}

pub(crate) fn event_path(service: Service) -> String {
    format!("{EVENTED_UNDER}{}", service.name())
}

pub(crate) fn device(udn: &Udn, name: &str) -> String {
    let mut services = String::new();

    for service in Service::ALL {
        let _ = write!(
            services,
            "<service><serviceType>{}</serviceType><serviceId>urn:upnp-org:serviceId:{name}\
             </serviceId><SCPDURL>{}</SCPDURL><controlURL>{}</controlURL><eventSubURL>\
             {}</eventSubURL></service>",
            service.kind(),
            description_path(service),
            control_path(service),
            event_path(service),
            name = service.name(),
        );
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<root \
         xmlns=\"urn:schemas-upnp-org:device-1-0\" xmlns:dlna=\"urn:schemas-dlna-org:device-1-0\">\
         <specVersion><major>1</major><minor>0</minor></specVersion><device><deviceType>\
         {SERVER_TYPE}</deviceType><friendlyName>{}</friendlyName><manufacturer>{MAKER}\
         </manufacturer><modelName>{MAKER}</modelName><modelNumber>{VERSION}</modelNumber><UDN>{}\
         </UDN><dlna:X_DLNADOC>DMS-1.50</dlna:X_DLNADOC><serviceList>{services}</serviceList>\
         </device></root>",
        escaped(name),
        escaped(udn.as_str()),
    )
}

struct Argument {
    name: &'static str,
    out: bool,
    related: &'static str,
}

const fn given(name: &'static str, related: &'static str) -> Argument {
    Argument {
        name,
        out: false,
        related,
    }
}

const fn answered(name: &'static str, related: &'static str) -> Argument {
    Argument {
        name,
        out: true,
        related,
    }
}

struct Variable {
    name: &'static str,
    kind: &'static str,
    evented: bool,
    allowed: &'static [&'static str],
}

const fn variable(name: &'static str, kind: &'static str) -> Variable {
    Variable {
        name,
        kind,
        evented: false,
        allowed: &[],
    }
}

const fn evented(name: &'static str, kind: &'static str) -> Variable {
    Variable {
        name,
        kind,
        evented: true,
        allowed: &[],
    }
}

const fn one_of(name: &'static str, allowed: &'static [&'static str]) -> Variable {
    Variable {
        name,
        kind: "string",
        evented: false,
        allowed,
    }
}

const DIRECTORY_ACTIONS: &[(&str, &[Argument])] = &[
    (
        "GetSearchCapabilities",
        &[answered("SearchCaps", "SearchCapabilities")],
    ),
    (
        "GetSortCapabilities",
        &[answered("SortCaps", "SortCapabilities")],
    ),
    ("GetSystemUpdateID", &[answered("Id", "SystemUpdateID")]),
    (
        "Browse",
        &[
            given("ObjectID", "A_ARG_TYPE_ObjectID"),
            given("BrowseFlag", "A_ARG_TYPE_BrowseFlag"),
            given("Filter", "A_ARG_TYPE_Filter"),
            given("StartingIndex", "A_ARG_TYPE_Index"),
            given("RequestedCount", "A_ARG_TYPE_Count"),
            given("SortCriteria", "A_ARG_TYPE_SortCriteria"),
            answered("Result", "A_ARG_TYPE_Result"),
            answered("NumberReturned", "A_ARG_TYPE_Count"),
            answered("TotalMatches", "A_ARG_TYPE_Count"),
            answered("UpdateID", "A_ARG_TYPE_UpdateID"),
        ],
    ),
    (
        "Search",
        &[
            given("ContainerID", "A_ARG_TYPE_ObjectID"),
            given("SearchCriteria", "A_ARG_TYPE_SearchCriteria"),
            given("Filter", "A_ARG_TYPE_Filter"),
            given("StartingIndex", "A_ARG_TYPE_Index"),
            given("RequestedCount", "A_ARG_TYPE_Count"),
            given("SortCriteria", "A_ARG_TYPE_SortCriteria"),
            answered("Result", "A_ARG_TYPE_Result"),
            answered("NumberReturned", "A_ARG_TYPE_Count"),
            answered("TotalMatches", "A_ARG_TYPE_Count"),
            answered("UpdateID", "A_ARG_TYPE_UpdateID"),
        ],
    ),
];

const DIRECTORY_VARIABLES: &[Variable] = &[
    variable("SearchCapabilities", "string"),
    variable("SortCapabilities", "string"),
    evented("SystemUpdateID", "ui4"),
    evented("ContainerUpdateIDs", "string"),
    variable("A_ARG_TYPE_ObjectID", "string"),
    variable("A_ARG_TYPE_Result", "string"),
    one_of(
        "A_ARG_TYPE_BrowseFlag",
        &["BrowseMetadata", "BrowseDirectChildren"],
    ),
    variable("A_ARG_TYPE_Filter", "string"),
    variable("A_ARG_TYPE_SortCriteria", "string"),
    variable("A_ARG_TYPE_SearchCriteria", "string"),
    variable("A_ARG_TYPE_Index", "ui4"),
    variable("A_ARG_TYPE_Count", "ui4"),
    variable("A_ARG_TYPE_UpdateID", "ui4"),
];

const CONNECTION_ACTIONS: &[(&str, &[Argument])] = &[
    (
        "GetProtocolInfo",
        &[
            answered("Source", "SourceProtocolInfo"),
            answered("Sink", "SinkProtocolInfo"),
        ],
    ),
    (
        "GetCurrentConnectionIDs",
        &[answered("ConnectionIDs", "CurrentConnectionIDs")],
    ),
    (
        "GetCurrentConnectionInfo",
        &[
            given("ConnectionID", "A_ARG_TYPE_ConnectionID"),
            answered("RcsID", "A_ARG_TYPE_RcsID"),
            answered("AVTransportID", "A_ARG_TYPE_AVTransportID"),
            answered("ProtocolInfo", "A_ARG_TYPE_ProtocolInfo"),
            answered("PeerConnectionManager", "A_ARG_TYPE_ConnectionManager"),
            answered("PeerConnectionID", "A_ARG_TYPE_ConnectionID"),
            answered("Direction", "A_ARG_TYPE_Direction"),
            answered("Status", "A_ARG_TYPE_ConnectionStatus"),
        ],
    ),
];

const CONNECTION_VARIABLES: &[Variable] = &[
    evented("SourceProtocolInfo", "string"),
    evented("SinkProtocolInfo", "string"),
    evented("CurrentConnectionIDs", "string"),
    one_of(
        "A_ARG_TYPE_ConnectionStatus",
        &[
            "OK",
            "ContentFormatMismatch",
            "InsufficientBandwidth",
            "UnreliableChannel",
            "Unknown",
        ],
    ),
    variable("A_ARG_TYPE_ConnectionManager", "string"),
    one_of("A_ARG_TYPE_Direction", &["Input", "Output"]),
    variable("A_ARG_TYPE_ProtocolInfo", "string"),
    variable("A_ARG_TYPE_ConnectionID", "i4"),
    variable("A_ARG_TYPE_AVTransportID", "i4"),
    variable("A_ARG_TYPE_RcsID", "i4"),
];

pub(crate) fn service(service: Service) -> String {
    let (actions, variables) = match service {
        Service::ContentDirectory => (DIRECTORY_ACTIONS, DIRECTORY_VARIABLES),
        Service::ConnectionManager => (CONNECTION_ACTIONS, CONNECTION_VARIABLES),
    };
    let mut written = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<scpd \
         xmlns=\"urn:schemas-upnp-org:service-1-0\"><specVersion><major>1</major><minor>0\
         </minor></specVersion><actionList>",
    );

    for (action, arguments) in actions {
        let _ = write!(written, "<action><name>{action}</name><argumentList>");

        for argument in *arguments {
            let _ = write!(
                written,
                "<argument><name>{}</name><direction>{}</direction>\
                 <relatedStateVariable>{}</relatedStateVariable></argument>",
                argument.name,
                if argument.out { "out" } else { "in" },
                argument.related,
            );
        }
        written.push_str("</argumentList></action>");
    }
    written.push_str("</actionList><serviceStateTable>");
    for variable in variables {
        let _ = write!(
            written,
            "<stateVariable sendEvents=\"{}\"><name>{}</name><dataType>{}</dataType>",
            if variable.evented { "yes" } else { "no" },
            variable.name,
            variable.kind,
        );

        if !variable.allowed.is_empty() {
            written.push_str("<allowedValueList>");
            for allowed in variable.allowed {
                let _ = write!(written, "<allowedValue>{allowed}</allowedValue>");
            }
            written.push_str("</allowedValueList>");
        }
        written.push_str("</stateVariable>");
    }
    written.push_str("</serviceStateTable></scpd>");
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Asked, description::document};

    #[test]
    fn every_argument_names_a_variable_its_service_declares() {
        for (actions, variables) in [
            (DIRECTORY_ACTIONS, DIRECTORY_VARIABLES),
            (CONNECTION_ACTIONS, CONNECTION_VARIABLES),
        ] {
            for (action, arguments) in actions {
                for argument in *arguments {
                    assert!(
                        variables
                            .iter()
                            .any(|variable| variable.name == argument.related),
                        "{action}'s {} names {}",
                        argument.name,
                        argument.related
                    );
                }
            }
        }
    }

    #[test]
    fn the_description_and_both_services_read_as_documents() {
        let udn = Udn::new("uuid:4c1b3a27-0000-4000-8000-000000000001");
        let described = device(&udn, "Name & <more>");

        let read = document(&described, Asked::Description).expect("a document");

        assert!(
            read.descendants()
                .any(|node| node.tag_name().name() == "friendlyName"
                    && node.text() == Some("Name & <more>"))
        );
        for kind in Service::ALL {
            assert!(document(&service(kind), Asked::Description).is_ok());
        }
    }
}
