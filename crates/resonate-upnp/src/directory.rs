use std::fmt::Write as _;

use roxmltree::{Document, ParsingOptions};

use crate::{
    catalog::{
        AudioKind, Catalog, Entry, Held, Kind, Object, Page, Query, Searched, Shelf, Within,
    },
    criteria::{self, Criterion, Described, Reduced, SEARCH_CAPABILITIES},
    didl::{Didl, FOLDER, escaped},
    naming,
    watched::Watched,
};

pub(crate) const CONTENT_DIRECTORY: &str = "urn:schemas-upnp-org:service:ContentDirectory:1";

pub(crate) const CONNECTION_MANAGER: &str = "urn:schemas-upnp-org:service:ConnectionManager:1";

const NODES_ASKED_AT_MOST: u32 = 1_000;

const ARGUMENTS_AT_MOST: usize = 16;

pub(crate) const BROWSED_AT_MOST: u32 = 500;

pub(crate) const THE_ONE_CONNECTION: &str = "0";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Service {
    ContentDirectory,
    ConnectionManager,
}

impl Service {
    pub(crate) const ALL: [Self; 2] = [Self::ContentDirectory, Self::ConnectionManager];

    pub(crate) const fn kind(self) -> &'static str {
        match self {
            Self::ContentDirectory => CONTENT_DIRECTORY,
            Self::ConnectionManager => CONNECTION_MANAGER,
        }
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::ContentDirectory => "ContentDirectory",
            Self::ConnectionManager => "ConnectionManager",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Called {
    pub action: String,
    pub arguments: Vec<(String, String)>,
}

impl Called {
    fn argument(&self, name: &str) -> Option<&str> {
        self.arguments
            .iter()
            .find(|(named, _)| named == name)
            .map(|(_, value)| value.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fault {
    InvalidAction,
    InvalidArgs,
    ActionFailed,
    NoSuchObject,
    NoSuchContainer,
    InvalidConnection,
    UnsupportedCriteria,
}

impl Fault {
    pub(crate) const fn code(self) -> u16 {
        match self {
            Self::InvalidAction => 401,
            Self::InvalidArgs => 402,
            Self::ActionFailed => 501,
            Self::NoSuchObject => 701,
            Self::InvalidConnection => 706,
            Self::UnsupportedCriteria => 708,
            Self::NoSuchContainer => 710,
        }
    }

    const fn described(self) -> &'static str {
        match self {
            Self::InvalidAction => "Invalid Action",
            Self::InvalidArgs => "Invalid Args",
            Self::ActionFailed => "Action Failed",
            Self::NoSuchObject => "No such object",
            Self::InvalidConnection => "Invalid connection reference",
            Self::UnsupportedCriteria => "Unsupported or invalid search criteria",
            Self::NoSuchContainer => "No such container",
        }
    }
}

pub(crate) enum Answered {
    Said(String),
    Faulted(Fault),
}

pub(crate) fn called(body: &str, service: Service) -> Option<Called> {
    let options = ParsingOptions {
        allow_dtd: false,
        nodes_limit: NODES_ASKED_AT_MOST,
        ..ParsingOptions::default()
    };
    let document = Document::parse_with_options(body, options).ok()?;
    let envelope = document.root_element();
    if envelope.tag_name().name() != "Envelope" {
        return None;
    }
    let call = envelope
        .children()
        .find(|node| node.is_element() && node.tag_name().name() == "Body")?
        .children()
        .find(roxmltree::Node::is_element)?;
    if call.tag_name().namespace() != Some(service.kind()) {
        return None;
    }
    let arguments: Vec<(String, String)> = call
        .children()
        .filter(roxmltree::Node::is_element)
        .take(ARGUMENTS_AT_MOST + 1)
        .map(|argument| {
            (
                argument.tag_name().name().to_owned(),
                argument.text().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    (arguments.len() <= ARGUMENTS_AT_MOST).then(|| Called {
        action: call.tag_name().name().to_owned(),
        arguments,
    })
}

pub(crate) fn source_protocols() -> String {
    AudioKind::ALL
        .into_iter()
        .map(|kind| format!("http-get:*:{}:*", kind.mime()))
        .collect::<Vec<_>>()
        .join(",")
}

pub(crate) struct Asking<'a, C> {
    pub catalog: &'a C,
    pub base: &'a str,
    pub name: &'a str,
    pub watched: &'a Watched,
}

impl<C: Catalog> Asking<'_, C> {
    pub(crate) fn answer(&self, service: Service, called: &Called) -> Answered {
        let said = match (service, called.action.as_str()) {
            (Service::ContentDirectory, "Browse") => self.browse(called),
            (Service::ContentDirectory, "Search") => self.search(called),
            (Service::ContentDirectory, "GetSearchCapabilities") => {
                Ok(vec![("SearchCaps", SEARCH_CAPABILITIES.to_owned())])
            }
            (Service::ContentDirectory, "GetSortCapabilities") => {
                Ok(vec![("SortCaps", String::new())])
            }
            (Service::ContentDirectory, "GetSystemUpdateID") => {
                Ok(vec![("Id", self.catalog.revision().to_string())])
            }
            (Service::ConnectionManager, "GetProtocolInfo") => Ok(vec![
                ("Source", source_protocols()),
                ("Sink", String::new()),
            ]),
            (Service::ConnectionManager, "GetCurrentConnectionIDs") => {
                Ok(vec![("ConnectionIDs", THE_ONE_CONNECTION.to_owned())])
            }
            (Service::ConnectionManager, "GetCurrentConnectionInfo") => connection_info(called),
            _ => Err(Fault::InvalidAction),
        };
        match said {
            Ok(arguments) => Answered::Said(envelope(service, &called.action, &arguments)),
            Err(fault) => Answered::Faulted(fault),
        }
    }

    fn browse(&self, called: &Called) -> Result<Vec<(&'static str, String)>, Fault> {
        let object = object_named(called, "ObjectID")?;
        let Page { from, most } = page_asked(called)?;
        let read_at = self.catalog.revision();

        let mut didl = Didl::new(self.base);
        let (returned, total) = match called.argument("BrowseFlag").map(str::trim) {
            Some("BrowseMetadata") => {
                self.metadata(object, &mut didl)?;
                (1, 1)
            }
            Some("BrowseDirectChildren") => {
                let children = self.children(object, Page { from, most }, &mut didl)?;
                self.watched.watch(object, read_at);
                children
            }
            _ => return Err(Fault::InvalidArgs),
        };

        Ok(vec![
            ("Result", didl.finished()),
            ("NumberReturned", returned.to_string()),
            ("TotalMatches", total.to_string()),
            ("UpdateID", self.catalog.revision().to_string()),
        ])
    }

    fn search(&self, called: &Called) -> Result<Vec<(&'static str, String)>, Fault> {
        let container = object_named(called, "ContainerID")?;
        let criterion = criteria::read(called.argument("SearchCriteria").unwrap_or_default())
            .map_err(|refusal| {
                tracing::debug!(
                    ?refusal,
                    "a search a control point asked could not be answered"
                );
                Fault::UnsupportedCriteria
            })?;
        let page = page_asked(called)?;
        let (within, sought) = match container {
            Object::Root => (Within::Catalog, &SEARCHED_UNDER_THE_ROOT[..]),
            Object::Shelf(shelf) => {
                if !shelf.is_top() {
                    self.held(shelf)?;
                }
                (Within::of(shelf), searched_under(shelf))
            }
            Object::Song(id) => {
                return Err(match self.catalog.song(id).map_err(failed)? {
                    Some(_) => Fault::NoSuchContainer,
                    None => Fault::NoSuchObject,
                });
            }
        };

        let mut didl = Didl::new(self.base);
        let mut returned = 0;
        let mut total: u32 = 0;
        for sought in sought {
            let next = page.from.saturating_add(returned);
            let wanted = Page {
                from: next.saturating_sub(total),
                most: page.most - returned,
            };
            let (taken, held) = self.sought(*sought, within, &criterion, wanted, &mut didl)?;
            returned += taken;
            total = total.saturating_add(held);
        }

        Ok(vec![
            ("Result", didl.finished()),
            ("NumberReturned", returned.to_string()),
            ("TotalMatches", total.to_string()),
            ("UpdateID", self.catalog.revision().to_string()),
        ])
    }

    fn sought(
        &self,
        sought: Sought,
        within: Within,
        criterion: &Criterion,
        page: Page,
        didl: &mut Didl<'_>,
    ) -> Result<(u32, u32), Fault> {
        let kind = match sought {
            Sought::Folder(shelf) => {
                let described = Described {
                    class: FOLDER,
                    kind: None,
                    title: Some(naming::title_of(shelf)),
                };
                if criteria::reduced(criterion, &described) != Reduced::Always {
                    return Ok((0, 0));
                }
                if page.from > 0 || page.most == 0 {
                    return Ok((0, 1));
                }
                didl.shelf(&self.held(shelf)?, Object::Root);
                return Ok((1, 1));
            }
            Sought::Of(kind) => kind,
        };
        let query = match criteria::reduced(criterion, &Described::of(kind)) {
            Reduced::Never => return Ok((0, 0)),
            Reduced::Always => Query::Everything,
            Reduced::Asks(query) => query,
        };
        let shelved = match self
            .catalog
            .searched(kind, within, &query, page)
            .map_err(failed)?
        {
            Searched::Found(shelved) => shelved,
            Searched::Unanswered => {
                tracing::debug!(?kind, ?query, "the catalog answers no such search");
                return Err(Fault::UnsupportedCriteria);
            }
        };
        let returned = shelved.held.len().min(page.most as usize);
        for entry in shelved.held.iter().take(returned) {
            match entry {
                Entry::Shelf(held) => didl.shelf(held, held.shelf.parent()),
                Entry::Song(song) => didl.song(song, Object::Shelf(Shelf::Songs)),
            }
        }
        Ok((returned as u32, shelved.total))
    }

    fn metadata(&self, object: Object, didl: &mut Didl<'_>) -> Result<(), Fault> {
        match object {
            Object::Root => didl.root(self.name, Shelf::TOP.len() as u32),
            Object::Shelf(shelf) => didl.shelf(&self.held(shelf)?, shelf.parent()),
            Object::Song(id) => {
                let song = self
                    .catalog
                    .song(id)
                    .map_err(failed)?
                    .ok_or(Fault::NoSuchObject)?;
                didl.song(&song, Object::Shelf(Shelf::Songs));
            }
        }
        Ok(())
    }

    fn held(&self, shelf: Shelf) -> Result<Held, Fault> {
        if shelf.is_top() {
            let children = self
                .catalog
                .shelved(shelf, Page::counting())
                .map_err(failed)?
                .map_or(0, |shelved| shelved.total);
            return Ok(Held {
                shelf,
                title: naming::title_of(shelf).to_owned(),
                artist: None,
                children,
                cover: None,
            });
        }
        self.catalog
            .shelf(shelf)
            .map_err(failed)?
            .ok_or(Fault::NoSuchObject)
    }

    fn children(
        &self,
        object: Object,
        page: Page,
        didl: &mut Didl<'_>,
    ) -> Result<(u32, u32), Fault> {
        let shelf = match object {
            Object::Root => {
                let top: Vec<Held> = Shelf::TOP
                    .into_iter()
                    .skip(page.from as usize)
                    .take(page.most as usize)
                    .map(|shelf| self.held(shelf))
                    .collect::<Result<_, _>>()?;
                for held in &top {
                    didl.shelf(held, Object::Root);
                }
                return Ok((top.len() as u32, Shelf::TOP.len() as u32));
            }
            Object::Shelf(shelf) => shelf,
            Object::Song(id) => {
                return Err(match self.catalog.song(id).map_err(failed)? {
                    Some(_) => Fault::NoSuchContainer,
                    None => Fault::NoSuchObject,
                });
            }
        };
        let shelved = self
            .catalog
            .shelved(shelf, page)
            .map_err(failed)?
            .ok_or(Fault::NoSuchObject)?;
        let parent = Object::Shelf(shelf);
        let returned = shelved.held.len().min(page.most as usize);
        for entry in shelved.held.iter().take(returned) {
            match entry {
                Entry::Shelf(held) => didl.shelf(held, parent),
                Entry::Song(song) => didl.song(song, parent),
            }
        }
        Ok((returned as u32, shelved.total))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sought {
    Folder(Shelf),
    Of(Kind),
}

const SEARCHED_UNDER_THE_ROOT: [Sought; 8] = [
    Sought::Folder(Shelf::Artists),
    Sought::Folder(Shelf::Albums),
    Sought::Folder(Shelf::Songs),
    Sought::Folder(Shelf::Playlists),
    Sought::Of(Kind::Artist),
    Sought::Of(Kind::Album),
    Sought::Of(Kind::Playlist),
    Sought::Of(Kind::Song),
];

const fn searched_under(shelf: Shelf) -> &'static [Sought] {
    match shelf {
        Shelf::Artists => &[Sought::Of(Kind::Artist)],
        Shelf::Albums => &[Sought::Of(Kind::Album)],
        Shelf::Playlists => &[Sought::Of(Kind::Playlist)],
        Shelf::Artist(_) => &[Sought::Of(Kind::Album), Sought::Of(Kind::Song)],
        Shelf::Songs | Shelf::Album(_) | Shelf::Playlist(_) => &[Sought::Of(Kind::Song)],
    }
}

fn object_named(called: &Called, name: &str) -> Result<Object, Fault> {
    called
        .argument(name)
        .ok_or(Fault::InvalidArgs)
        .map(|id| Object::read(id.trim()).ok_or(Fault::NoSuchObject))?
}

fn page_asked(called: &Called) -> Result<Page, Fault> {
    let counted = |name: &str| -> Result<u32, Fault> {
        called.argument(name).map_or(Ok(0), |count| {
            count.trim().parse().map_err(|_| Fault::InvalidArgs)
        })
    };
    let from = counted("StartingIndex")?;
    let most = match counted("RequestedCount")? {
        0 => BROWSED_AT_MOST,
        most => most.min(BROWSED_AT_MOST),
    };
    Ok(Page { from, most })
}

fn failed(error: impl std::error::Error) -> Fault {
    tracing::warn!(%error, "the catalog could not be read for a control point");
    Fault::ActionFailed
}

fn connection_info(called: &Called) -> Result<Vec<(&'static str, String)>, Fault> {
    if called.argument("ConnectionID").map(str::trim) != Some(THE_ONE_CONNECTION) {
        return Err(Fault::InvalidConnection);
    }
    Ok(vec![
        ("RcsID", "-1".to_owned()),
        ("AVTransportID", "-1".to_owned()),
        ("ProtocolInfo", String::new()),
        ("PeerConnectionManager", String::new()),
        ("PeerConnectionID", "-1".to_owned()),
        ("Direction", "Output".to_owned()),
        ("Status", "OK".to_owned()),
    ])
}

const ENVELOPE_OPENS: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n",
    "<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" ",
    "s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body>"
);

const ENVELOPE_CLOSES: &str = "</s:Body></s:Envelope>";

fn envelope(service: Service, action: &str, arguments: &[(&str, String)]) -> String {
    let mut written = ENVELOPE_OPENS.to_owned();
    let _ = write!(
        written,
        "<u:{action}Response xmlns:u=\"{}\">",
        service.kind()
    );
    for (name, value) in arguments {
        let _ = write!(written, "<{name}>{}</{name}>", escaped(value));
    }
    let _ = write!(written, "</u:{action}Response>{ENVELOPE_CLOSES}");
    written
}

pub(crate) fn fault_envelope(fault: Fault) -> String {
    format!(
        "{ENVELOPE_OPENS}<s:Fault><faultcode>s:Client</faultcode><faultstring>UPnPError\
         </faultstring><detail><UPnPError xmlns=\"urn:schemas-upnp-org:control-1-0\">\
         <errorCode>{}</errorCode><errorDescription>{}</errorDescription></UPnPError></detail>\
         </s:Fault>{ENVELOPE_CLOSES}",
        fault.code(),
        fault.described()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const BROWSE: &str = r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:Browse xmlns:u="urn:schemas-upnp-org:service:ContentDirectory:1"><ObjectID>albums</ObjectID><BrowseFlag>BrowseDirectChildren</BrowseFlag><Filter>*</Filter><StartingIndex>0</StartingIndex><RequestedCount>25</RequestedCount><SortCriteria></SortCriteria></u:Browse></s:Body></s:Envelope>"#;

    #[test]
    fn a_browse_is_read_by_its_action_and_arguments_in_the_service_it_was_posted_to() {
        let read = called(BROWSE, Service::ContentDirectory).expect("a call");

        assert_eq!(read.action, "Browse");
        assert_eq!(read.argument("ObjectID"), Some("albums"));
        assert_eq!(read.argument("SortCriteria"), Some(""));
        assert_eq!(called(BROWSE, Service::ConnectionManager), None);
        assert_eq!(called("<a/>", Service::ContentDirectory), None);
        assert_eq!(
            called(
                "<!DOCTYPE x [<!ENTITY e \"e\">]><s:Envelope/>",
                Service::ContentDirectory
            ),
            None
        );
    }

    #[test]
    fn a_fault_names_its_code_inside_a_soap_envelope() {
        let written = fault_envelope(Fault::NoSuchObject);

        assert_eq!(
            crate::soap::fault_code(&written, crate::Action::Play),
            Some(701)
        );
    }
}
