use std::{
    collections::BTreeMap,
    fmt::Write as _,
    net::IpAddr,
    time::{Duration, Instant},
};

use parking_lot::Mutex;

use crate::{
    Asked, Located, Udn,
    catalog::Object,
    didl::escaped,
    directory::{self, Service},
    http::{self, HEADERS_AT_MOST},
    neighbours,
};

pub(crate) const SUBSCRIBERS_AT_MOST: usize = 32;

pub(crate) const KEPT_AT_MOST: Duration = Duration::from_secs(1800);

pub(crate) const KEPT_AT_LEAST: Duration = Duration::from_secs(60);

pub(crate) const TOLD_AT_MOST_EVERY: Duration = Duration::from_secs(2);

pub(crate) const TOLD_WITHIN: Duration = Duration::from_secs(2);

const CONTAINERS_PENDING_AT_MOST: usize = 2 * crate::watched::WATCHED_AT_MOST;

const CALLBACKS_AT_MOST: usize = 4;

const EVENTED: &str = "upnp:event";

const CHANGED: &str = "upnp:propchange";

const SECONDS: &str = "second-";

const FOREVER: &str = "infinite";

const PROPERTY_SET_OPENS: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n",
    "<e:propertyset xmlns:e=\"urn:schemas-upnp-org:event-1-0\">"
);

const PROPERTY_SET_CLOSES: &str = "</e:propertyset>";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Method {
    Subscribe,
    Unsubscribe,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Subscribing {
    New { callback: Located, kept: Duration },
    Renewed { sid: String, kept: Duration },
    Ended { sid: String },
    Malformed,
    Unmet,
}

fn kept_for(timeout: Option<&str>) -> Duration {
    let asked = timeout.and_then(|timeout| {
        let seconds = timeout
            .get(..SECONDS.len())
            .filter(|named| named.eq_ignore_ascii_case(SECONDS))
            .and_then(|_| timeout.get(SECONDS.len()..))?;

        if seconds.eq_ignore_ascii_case(FOREVER) {
            return Some(KEPT_AT_MOST);
        }
        seconds
            .bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| seconds.parse::<u64>().ok())
            .flatten()
            .map(Duration::from_secs)
    });
    asked
        .unwrap_or(KEPT_AT_MOST)
        .clamp(KEPT_AT_LEAST, KEPT_AT_MOST)
}

fn callback_for(callbacks: &str, peer: IpAddr) -> Option<Located> {
    if !neighbours::is_a_neighbour(peer) {
        return None;
    }
    callbacks
        .split('<')
        .skip(1)
        .take(CALLBACKS_AT_MOST)
        .filter_map(|named| named.split_once('>').map(|(url, _)| url))
        .filter_map(|url| Located::parse(url).ok())
        .find(|callback| callback.is_at(peer))
}

pub(crate) fn subscribing(held: &[u8], method: Method, peer: IpAddr) -> Subscribing {
    let mut headers = [httparse::EMPTY_HEADER; HEADERS_AT_MOST];

    let mut request = httparse::Request::new(&mut headers);

    match request.parse(held) {
        Ok(httparse::Status::Complete(_)) => {}
        Ok(httparse::Status::Partial) | Err(_) => return Subscribing::Malformed,
    }

    let named = |name: &str| {
        request
            .headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case(name))
            .and_then(|header| std::str::from_utf8(header.value).ok())
            .map(str::trim)
    };
    let sid = named("SID").map(str::to_owned);
    let nt = named("NT");
    let callbacks = named("CALLBACK");
    let kept = kept_for(named("TIMEOUT"));

    match (method, sid) {
        (_, Some(_)) if nt.is_some() || callbacks.is_some() => Subscribing::Malformed,
        (Method::Subscribe, Some(sid)) => Subscribing::Renewed { sid, kept },
        (Method::Unsubscribe, Some(sid)) => Subscribing::Ended { sid },
        (Method::Unsubscribe, None) => Subscribing::Unmet,
        (Method::Subscribe, None) => {
            if nt != Some(EVENTED) {
                return Subscribing::Unmet;
            }
            callbacks
                .and_then(|callbacks| callback_for(callbacks, peer))
                .map_or(Subscribing::Unmet, |callback| Subscribing::New {
                    callback,
                    kept,
                })
        }
    }
}

struct Subscription {
    service: Service,
    callback: Located,
    until: Instant,
    seq: u32,
    answered: bool,
    told: Option<u32>,
    told_at: Instant,
    containers: Vec<(Object, u32)>,
}

impl Subscription {
    fn awaits(&self, revision: u32) -> bool {
        match self.told {
            None => self.answered,
            Some(told) => {
                self.service == Service::ContentDirectory
                    && (told != revision || !self.containers.is_empty())
                    && self.told_at.elapsed() >= TOLD_AT_MOST_EVERY
            }
        }
    }
}

pub(crate) struct Telling {
    sid: String,
    callback: Located,
    seq: u32,
    revision: u32,
    body: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kept {
    Granted(Duration),
    Unknown,
}

#[derive(Default)]
pub(crate) struct Subscribers {
    held: Mutex<BTreeMap<String, Subscription>>,
}

const fn next_seq(seq: u32) -> u32 {
    match seq.checked_add(1) {
        Some(next) => next,
        None => 1,
    }
}

impl Subscribers {
    pub(crate) fn subscribe(
        &self,
        service: Service,
        callback: Located,
        kept: Duration,
    ) -> Option<String> {
        let now = Instant::now();
        let mut held = self.held.lock();

        held.retain(|_, subscription| subscription.until > now);
        if held.len() >= SUBSCRIBERS_AT_MOST {
            return None;
        }

        let sid = Udn::minted().as_str().to_owned();

        held.insert(
            sid.clone(),
            Subscription {
                service,
                callback,
                until: now + kept,
                seq: 0,
                answered: false,
                told: None,
                told_at: now,
                containers: Vec::new(),
            },
        );
        Some(sid)
    }

    pub(crate) fn renew(&self, service: Service, sid: &str, kept: Duration) -> Kept {
        let now = Instant::now();
        let mut held = self.held.lock();
        match held.get_mut(sid) {
            Some(subscription) if subscription.service == service && subscription.until > now => {
                subscription.until = now + kept;
                Kept::Granted(kept)
            }
            _ => Kept::Unknown,
        }
    }

    pub(crate) fn answered(&self, sid: &str) {
        if let Some(subscription) = self.held.lock().get_mut(sid) {
            subscription.answered = true;
        }
    }

    pub(crate) fn end(&self, service: Service, sid: &str) -> bool {
        let mut held = self.held.lock();

        if held
            .get(sid)
            .is_some_and(|subscription| subscription.service == service)
        {
            held.remove(sid);
            return true;
        }
        false
    }

    #[cfg(test)]
    fn held(&self) -> usize {
        self.held.lock().len()
    }

    pub(crate) fn directory_heard(&self) -> bool {
        self.held
            .lock()
            .values()
            .any(|subscription| subscription.service == Service::ContentDirectory)
    }

    pub(crate) fn awaits_a_change(&self, revision: u32) -> bool {
        self.held.lock().values().any(|subscription| {
            subscription.told.is_some_and(|told| told != revision) && subscription.awaits(revision)
        })
    }

    pub(crate) fn note(&self, changed: &[(Object, u32)]) {
        if changed.is_empty() {
            return;
        }

        let mut held = self.held.lock();
        for subscription in held
            .values_mut()
            .filter(|subscription| subscription.service == Service::ContentDirectory)
        {
            let containers = &mut subscription.containers;
            for (object, update) in changed {
                match containers.iter().position(|(pending, _)| pending == object) {
                    Some(at) => containers[at].1 = *update,
                    None if containers.len() < CONTAINERS_PENDING_AT_MOST => {
                        containers.push((*object, *update));
                    }
                    None => {}
                }
            }
        }
    }

    pub(crate) fn due(&self, revision: u32) -> Vec<Telling> {
        let now = Instant::now();
        let mut held = self.held.lock();

        held.retain(|_, subscription| subscription.until > now);
        held.iter()
            .filter(|(_, subscription)| subscription.awaits(revision))
            .map(|(sid, subscription)| Telling {
                sid: sid.clone(),
                callback: subscription.callback.clone(),
                seq: subscription.seq,
                revision,
                body: property_set(
                    subscription.service,
                    revision,
                    &subscription.containers,
                    subscription.told.is_none(),
                ),
            })
            .collect()
    }

    pub(crate) fn settle(&self, telling: &Telling, told: bool) {
        let mut held = self.held.lock();

        if !told {
            held.remove(&telling.sid);
            return;
        }
        if let Some(subscription) = held.get_mut(&telling.sid) {
            subscription.seq = next_seq(telling.seq);
            subscription.told = Some(telling.revision);
            subscription.told_at = Instant::now();
            subscription.containers.clear();
        }
    }
}

pub(crate) fn property_set(
    service: Service,
    revision: u32,
    containers: &[(Object, u32)],
    initial: bool,
) -> String {
    let properties: Vec<(&str, String)> = match service {
        Service::ContentDirectory => {
            let mut properties = vec![("SystemUpdateID", revision.to_string())];

            if initial || !containers.is_empty() {
                properties.push(("ContainerUpdateIDs", container_update_ids(containers)));
            }
            properties
        }
        Service::ConnectionManager => vec![
            ("SourceProtocolInfo", directory::source_protocols()),
            ("SinkProtocolInfo", String::new()),
            (
                "CurrentConnectionIDs",
                directory::THE_ONE_CONNECTION.to_owned(),
            ),
        ],
    };
    let mut written = PROPERTY_SET_OPENS.to_owned();

    for (name, value) in properties {
        let _ = write!(
            written,
            "<e:property><{name}>{}</{name}></e:property>",
            escaped(&value)
        );
    }
    written.push_str(PROPERTY_SET_CLOSES);
    written
}

fn container_update_ids(containers: &[(Object, u32)]) -> String {
    containers
        .iter()
        .map(|(object, update)| format!("{object},{update}"))
        .collect::<Vec<_>>()
        .join(",")
}

pub(crate) fn tell(telling: &Telling) -> bool {
    let seq = telling.seq.to_string();
    let told = http::exchange_before(
        &http::Asking {
            method: "NOTIFY",
            to: &telling.callback,
            headers: &[
                ("Content-Type", "text/xml; charset=\"utf-8\""),
                ("NT", EVENTED),
                ("NTS", CHANGED),
                ("SID", &telling.sid),
                ("SEQ", &seq),
            ],
            body: telling.body.as_bytes(),
        },
        Asked::Event,
        Instant::now() + TOLD_WITHIN,
    );
    match told {
        Ok(answer) if (200..300).contains(&answer.status) => true,
        Ok(answer) => {
            tracing::debug!(status = answer.status, callback = %telling.callback, "a subscriber refused an event, so it is dropped");
            false
        }
        Err(error) => {
            tracing::debug!(%error, callback = %telling.callback, "a subscriber could not be told of a change, so it is dropped");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use resonate_core::PlaylistId;

    use super::*;
    use crate::catalog::Shelf;

    const PEER: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20));

    fn asked(head: &str, method: Method) -> Subscribing {
        subscribing(head.as_bytes(), method, PEER)
    }

    #[test]
    fn a_subscription_is_read_from_its_head_and_its_callback_must_be_the_subscribers_own() {
        let new = asked(
            "SUBSCRIBE /event/ContentDirectory HTTP/1.1\r\nHOST: 192.168.1.2:8200\r\nCALLBACK: \
             <http://8.8.8.8/x><http://192.168.1.20:49152/notify>\r\nNT: upnp:event\r\nTIMEOUT: \
             Second-300\r\n\r\n",
            Method::Subscribe,
        );
        let far = asked(
            "SUBSCRIBE /event/ContentDirectory HTTP/1.1\r\nCALLBACK: <http://8.8.8.8/x>\r\nNT: \
             upnp:event\r\n\r\n",
            Method::Subscribe,
        );
        let elsewhere = asked(
            "SUBSCRIBE /event/ContentDirectory HTTP/1.1\r\nCALLBACK: \
             <http://192.168.1.21/x>\r\nNT: upnp:event\r\n\r\n",
            Method::Subscribe,
        );
        let named = asked(
            "SUBSCRIBE /event/ContentDirectory HTTP/1.1\r\nCALLBACK: \
             <http://listener.local/x>\r\nNT: upnp:event\r\n\r\n",
            Method::Subscribe,
        );

        assert_eq!(
            new,
            Subscribing::New {
                callback: Located::parse("http://192.168.1.20:49152/notify").expect("an address"),
                kept: Duration::from_secs(300),
            }
        );
        assert_eq!(far, Subscribing::Unmet);
        assert_eq!(elsewhere, Subscribing::Unmet);
        assert_eq!(named, Subscribing::Unmet);
        assert_eq!(
            asked(
                "SUBSCRIBE /e HTTP/1.1\r\nCALLBACK: <http://192.168.1.20/x>\r\n\r\n",
                Method::Subscribe
            ),
            Subscribing::Unmet
        );
        assert_eq!(
            asked(
                "SUBSCRIBE /e HTTP/1.1\r\nSID: uuid:a\r\nNT: upnp:event\r\n\r\n",
                Method::Subscribe
            ),
            Subscribing::Malformed
        );
        assert_eq!(
            asked(
                "SUBSCRIBE /e HTTP/1.1\r\nSID: uuid:a\r\nTIMEOUT: Second-infinite\r\n\r\n",
                Method::Subscribe
            ),
            Subscribing::Renewed {
                sid: "uuid:a".to_owned(),
                kept: KEPT_AT_MOST
            }
        );
        assert_eq!(
            asked(
                "UNSUBSCRIBE /e HTTP/1.1\r\nSID: uuid:a\r\n\r\n",
                Method::Unsubscribe
            ),
            Subscribing::Ended {
                sid: "uuid:a".to_owned()
            }
        );
        assert_eq!(
            asked("UNSUBSCRIBE /e HTTP/1.1\r\n\r\n", Method::Unsubscribe),
            Subscribing::Unmet
        );
    }

    #[test]
    fn a_timeout_asked_is_held_between_the_least_and_the_most_kept() {
        assert_eq!(kept_for(Some("Second-300")), Duration::from_secs(300));
        assert_eq!(kept_for(Some("second-5")), KEPT_AT_LEAST);
        assert_eq!(kept_for(Some("Second-99999999")), KEPT_AT_MOST);
        assert_eq!(kept_for(Some("Second-+5")), KEPT_AT_MOST);
        assert_eq!(kept_for(Some("Minute-5")), KEPT_AT_MOST);
        assert_eq!(kept_for(None), KEPT_AT_MOST);
    }

    #[test]
    fn subscribers_are_held_to_a_number_told_once_a_change_and_dropped_when_they_do_not_answer() {
        let subscribers = Subscribers::default();
        let callback = Located::parse("http://192.168.1.20/x").expect("an address");

        let sids: Vec<String> = (0..SUBSCRIBERS_AT_MOST)
            .filter_map(|_| {
                subscribers.subscribe(Service::ContentDirectory, callback.clone(), KEPT_AT_LEAST)
            })
            .collect();
        let refused = subscribers.subscribe(Service::ContentDirectory, callback, KEPT_AT_LEAST);
        let unanswered = subscribers.due(3);

        for sid in &sids {
            subscribers.answered(sid);
        }

        let first = subscribers.due(3);

        for telling in &first {
            subscribers.settle(telling, telling.sid != sids[0]);
        }

        let again = subscribers.due(3);
        let changed_at_once = subscribers.due(4);

        assert_eq!(sids.len(), SUBSCRIBERS_AT_MOST);
        assert_eq!(refused, None);
        assert!(unanswered.is_empty());
        assert_eq!(first.len(), SUBSCRIBERS_AT_MOST);
        assert!(first.iter().all(|telling| telling.seq == 0));
        assert_eq!(subscribers.held(), SUBSCRIBERS_AT_MOST - 1);
        assert!(again.is_empty());
        assert!(changed_at_once.is_empty());
        assert_eq!(
            subscribers.renew(Service::ContentDirectory, &sids[0], KEPT_AT_MOST),
            Kept::Unknown
        );
        assert_eq!(
            subscribers.renew(Service::ConnectionManager, &sids[1], KEPT_AT_MOST),
            Kept::Unknown
        );
        assert_eq!(
            subscribers.renew(Service::ContentDirectory, &sids[1], KEPT_AT_MOST),
            Kept::Granted(KEPT_AT_MOST)
        );
        assert!(subscribers.end(Service::ContentDirectory, &sids[1]));
        assert!(!subscribers.end(Service::ContentDirectory, &sids[1]));
        assert_eq!(next_seq(u32::MAX), 1);
    }

    #[test]
    fn a_property_set_names_each_evented_variable_and_reads_as_a_document() {
        let initial = property_set(Service::ContentDirectory, 42, &[], true);
        let told = property_set(
            Service::ContentDirectory,
            43,
            &[
                (
                    Object::Shelf(Shelf::Playlist(PlaylistId::new(3).expect("an id"))),
                    43,
                ),
                (Object::Root, 43),
            ],
            false,
        );
        let unnamed = property_set(Service::ContentDirectory, 44, &[], false);
        let connection = property_set(Service::ConnectionManager, 42, &[], true);

        assert!(initial.contains("<e:property><SystemUpdateID>42</SystemUpdateID></e:property>"));
        assert!(
            initial.contains("<e:property><ContainerUpdateIDs></ContainerUpdateIDs></e:property>")
        );
        assert!(roxmltree::Document::parse(&initial).is_ok());
        assert!(told.contains("<ContainerUpdateIDs>playlist-3,43,0,43</ContainerUpdateIDs>"));
        assert!(roxmltree::Document::parse(&told).is_ok());
        assert!(unnamed.contains("<SystemUpdateID>44</SystemUpdateID>"));
        assert!(!unnamed.contains("ContainerUpdateIDs"));
        assert!(connection.contains("<CurrentConnectionIDs>0</CurrentConnectionIDs>"));
        assert!(roxmltree::Document::parse(&connection).is_ok());
    }
}
