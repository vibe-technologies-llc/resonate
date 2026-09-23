use std::time::Duration;

use resonate_core::{Isrc, Link, Mbid, Service};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Identity {
    pub recording: Option<Mbid>,
    pub track: Option<Mbid>,
    pub release: Option<Mbid>,
    pub isrc: Option<Isrc>,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub length: Option<Duration>,
    pub disc: Option<u32>,
    pub position: Option<u32>,
    pub links: Vec<Link>,
    pub release_links: Vec<Link>,
}

impl Identity {
    pub fn named(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            ..Self::default()
        }
    }

    pub fn track_on(&self, service: Service) -> Option<&str> {
        on(&self.links, service)
    }

    pub fn release_on(&self, service: Service) -> Option<&str> {
        on(&self.release_links, service)
    }
}

fn on(links: &[Link], service: Service) -> Option<&str> {
    links
        .iter()
        .find(|link| link.service == service)
        .map(|link| link.url.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_service_is_read_off_the_track_and_the_release_apart() {
        let identity = Identity {
            links: vec![Link::new(
                "streaming",
                "https://tidal.com/track/55391743".to_owned(),
            )],
            release_links: vec![
                Link::new("streaming", "https://tidal.com/album/55391740".to_owned()),
                Link::new(
                    "purchase for download",
                    "https://pinkfloyd.bandcamp.com/album/meddle".to_owned(),
                ),
            ],
            ..Identity::named("Echoes")
        };

        assert_eq!(
            identity.track_on(Service::Tidal),
            Some("https://tidal.com/track/55391743")
        );
        assert_eq!(
            identity.release_on(Service::Tidal),
            Some("https://tidal.com/album/55391740")
        );
        assert_eq!(identity.track_on(Service::Bandcamp), None);
        assert_eq!(
            identity.release_on(Service::Bandcamp),
            Some("https://pinkfloyd.bandcamp.com/album/meddle")
        );
    }
}
