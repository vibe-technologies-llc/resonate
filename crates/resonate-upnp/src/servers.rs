use std::{collections::BTreeMap, net::SocketAddr, time::Duration};

use parking_lot::Mutex;

use crate::{
    Action, Asked, Error, Located, Result, Udn,
    description::{self, Service},
    fetched::Fetched,
    http::{self, Asking},
    listing::{self, ENTRIES_A_LISTING_AT_MOST, Listing, ObjectId},
    soap, ssdp,
};

pub const BROWSED_A_PAGE: u32 = 200;

pub const PAGES_AT_MOST: usize = 25;

pub const BROWSE_ANSWERED_WITHIN: Duration = Duration::from_secs(10);

const SERVERS_AT_MOST: usize = 64;

const DESCRIBED_FROM_A_SEARCH_AT_MOST: usize = 32;

const SEARCHES_MISSED_BEFORE_GONE: u8 = 2;

const DIRECT_CHILDREN: &str = "BrowseDirectChildren";

const METADATA: &str = "BrowseMetadata";

const EVERYTHING: &str = "*";

const AUDIO_CLASS: &str = "object.item.audioItem";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerFound {
    udn: Udn,
    name: String,
    located: Located,
    directory: Service,
    a_page: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageAsked {
    pub from: u32,
    pub most: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Paged {
    pub listing: Listing,
    pub returned: u32,
    pub total: Option<u32>,
}

impl ServerFound {
    pub(crate) fn reached(located: Located) -> Result<Self> {
        let described = description::server_described(&located)?;
        Ok(Self {
            udn: described.udn,
            name: described.name,
            located,
            directory: described.directory,
            a_page: BROWSED_A_PAGE,
        })
    }

    pub fn paged_by(self, a_page: u32) -> Self {
        Self {
            a_page: a_page.clamp(1, BROWSED_A_PAGE),
            ..self
        }
    }

    pub const fn udn(&self) -> &Udn {
        &self.udn
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn located(&self) -> &Located {
        &self.located
    }

    pub fn page_of_children(&self, container: &ObjectId, page: PageAsked) -> Result<Paged> {
        let from = page.from.to_string();
        let most = page.most.clamp(1, self.a_page).to_string();
        self.asked(
            Action::Browse,
            &[
                ("ObjectID", container.as_str()),
                ("BrowseFlag", DIRECT_CHILDREN),
                ("Filter", EVERYTHING),
                ("StartingIndex", &from),
                ("RequestedCount", &most),
                ("SortCriteria", ""),
            ],
        )
    }

    pub fn children(&self, container: &ObjectId) -> Result<Listing> {
        self.paged_through(|page| self.page_of_children(container, page))
    }

    pub fn metadata(&self, object: &ObjectId) -> Result<Listing> {
        Ok(self
            .asked(
                Action::Browse,
                &[
                    ("ObjectID", object.as_str()),
                    ("BrowseFlag", METADATA),
                    ("Filter", EVERYTHING),
                    ("StartingIndex", "0"),
                    ("RequestedCount", "0"),
                    ("SortCriteria", ""),
                ],
            )?
            .listing)
    }

    pub fn songs_named(&self, words: &str) -> Result<Listing> {
        let criteria = criteria_for(words);
        self.paged_through(|page| {
            let from = page.from.to_string();
            let most = page.most.clamp(1, self.a_page).to_string();
            self.asked(
                Action::Search,
                &[
                    ("ContainerID", ObjectId::root().as_str()),
                    ("SearchCriteria", &criteria),
                    ("Filter", EVERYTHING),
                    ("StartingIndex", &from),
                    ("RequestedCount", &most),
                    ("SortCriteria", ""),
                ],
            )
        })
    }

    pub fn fetched(&self, at: &Located) -> Result<Fetched> {
        self.on_this_server(at)?;
        Fetched::open(at.clone())
    }

    pub fn picture(&self, at: &Located) -> Result<Vec<u8>> {
        self.on_this_server(at)?;
        let answer = http::exchange(
            &Asking {
                method: "GET",
                to: at,
                headers: &[],
                body: &[],
            },
            Asked::Picture,
        )?;
        match answer.status {
            200 => Ok(answer.body),
            status => Err(Error::Status {
                asked: Asked::Picture,
                status,
            }),
        }
    }

    fn on_this_server(&self, at: &Located) -> Result<()> {
        if at.is_on_host_of(&self.located) {
            Ok(())
        } else {
            Err(Error::UnreadableAddress)
        }
    }

    fn paged_through(&self, mut asked: impl FnMut(PageAsked) -> Result<Paged>) -> Result<Listing> {
        let mut listing = Listing::default();
        let mut from = 0_u32;
        for _ in 0..PAGES_AT_MOST {
            let paged = asked(PageAsked {
                from,
                most: self.a_page,
            })?;
            let returned = paged.returned;
            listing.extend(paged.listing);
            from = from.saturating_add(returned);
            let whole = returned == 0
                || paged.total.is_some_and(|total| from >= total)
                || listing.len() >= ENTRIES_A_LISTING_AT_MOST;
            if whole {
                break;
            }
        }
        Ok(listing)
    }

    fn asked(&self, action: Action, arguments: &[(&str, &str)]) -> Result<Paged> {
        let answered =
            soap::call_within(&self.directory, action, arguments, BROWSE_ANSWERED_WITHIN)?;
        let didl = answered
            .named("Result")
            .ok_or(Error::Unanswered { action })?;
        let listing = listing::read(didl, &self.located)?;
        let count = |name: &str| {
            answered
                .named(name)
                .and_then(|count| count.trim().parse::<u32>().ok())
        };
        let read = u32::try_from(listing.len()).unwrap_or(u32::MAX);
        Ok(Paged {
            returned: count("NumberReturned").unwrap_or(read),
            total: count("TotalMatches").filter(|total| *total > 0),
            listing,
        })
    }
}

fn criteria_for(words: &str) -> String {
    let quoted: String = words
        .trim()
        .chars()
        .filter(|letter| !letter.is_control())
        .flat_map(|letter| match letter {
            '"' | '\\' => vec!['\\', letter],
            letter => vec![letter],
        })
        .collect();
    format!(
        "upnp:class derivedfrom \"{AUDIO_CLASS}\" and (dc:title contains \"{quoted}\" or \
         upnp:artist contains \"{quoted}\" or upnp:album contains \"{quoted}\")"
    )
}

pub struct MediaServers {
    to: SocketAddr,
    within: Duration,
    known: Mutex<BTreeMap<Udn, Held>>,
    round: Mutex<()>,
}

struct Held {
    server: ServerFound,
    missed: u8,
}

impl MediaServers {
    pub fn on_the_network() -> Self {
        Self::searching(ssdp::SSDP, Duration::from_secs(2))
    }

    pub fn searching(to: SocketAddr, within: Duration) -> Self {
        Self {
            to,
            within,
            known: Mutex::new(BTreeMap::new()),
            round: Mutex::new(()),
        }
    }

    pub fn found(&self) -> Result<Vec<ServerFound>> {
        let _one_at_a_time = self.round.lock();
        let heard = ssdp::search(self.to, self.within, ssdp::Sought::Servers)?;

        let mut seen = Vec::new();
        let mut reached = Vec::new();
        for location in heard {
            if let Some(udn) = self.held_at(&location) {
                seen.push(udn);
                continue;
            }
            let held = self.known.lock().len() + reached.len();
            if held >= SERVERS_AT_MOST || reached.len() >= DESCRIBED_FROM_A_SEARCH_AT_MOST {
                tracing::debug!(%location, "a media server answering the search was not described: enough are held");
                continue;
            }
            match ServerFound::reached(location.clone()) {
                Ok(server) => reached.push(server),
                Err(error) => {
                    tracing::debug!(%error, %location, "a device answering the search for media servers is not one");
                }
            }
        }

        let mut known = self.known.lock();
        for held in known.values_mut() {
            held.missed = held.missed.saturating_add(1);
        }
        for udn in &seen {
            if let Some(held) = known.get_mut(udn) {
                held.missed = 0;
            }
        }
        for server in reached {
            known.insert(server.udn.clone(), Held { server, missed: 0 });
        }
        known.retain(|udn, held| {
            let kept = held.missed < SEARCHES_MISSED_BEFORE_GONE;
            if !kept {
                tracing::debug!(%udn, "a media server has missed enough searches to be gone");
            }
            kept
        });
        drop(known);
        Ok(self.held())
    }

    pub fn held(&self) -> Vec<ServerFound> {
        let mut servers: Vec<ServerFound> = self
            .known
            .lock()
            .values()
            .map(|held| held.server.clone())
            .collect();
        servers.sort_by_cached_key(|server| (server.name.to_lowercase(), server.udn.clone()));
        servers
    }

    pub fn server(&self, udn: &Udn) -> Result<ServerFound> {
        if let Some(held) = self.known.lock().get(udn) {
            return Ok(held.server.clone());
        }
        self.found()?
            .into_iter()
            .find(|server| server.udn == *udn)
            .ok_or_else(|| Error::NoSuchServer(udn.clone()))
    }

    fn held_at(&self, location: &Located) -> Option<Udn> {
        self.known
            .lock()
            .values()
            .find(|held| held.server.located == *location)
            .map(|held| held.server.udn.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_searched_for_are_quoted_as_the_criteria_grammar_asks() {
        let criteria = criteria_for(" say \"hi\" \\ there\n");

        assert_eq!(
            criteria,
            "upnp:class derivedfrom \"object.item.audioItem\" and (dc:title contains \"say \\\"hi\\\" \\\\ there\" or upnp:artist contains \"say \\\"hi\\\" \\\\ there\" or upnp:album contains \"say \\\"hi\\\" \\\\ there\")"
        );
    }
}
