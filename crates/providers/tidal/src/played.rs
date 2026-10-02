use std::io::Read;

use resonate_core::{Isrc, Service, SourceId};
use resonate_providers::{Delivery, Error, Extension, Identity, Obtained, ProviderOp, Result};
use serde::Deserialize;
use ureq::Agent;

use crate::{
    MediaHosts,
    fetched::{Fetched, Unfetched},
    manifest::{self, Container, Manifest, Media},
    remux::{Remuxed, Unremuxable},
};

const FULL: &str = "FULL";
const DELIVERED_AS: &str = "flac";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TrackId(pub(crate) u64);

impl TrackId {
    pub(crate) fn linked(url: &str) -> Option<Self> {
        let path = url.split(['?', '#']).next()?;
        let mut segments = path.split('/');
        segments.by_ref().find(|segment| *segment == "track")?;
        segments.next()?.parse().ok().map(Self)
    }
}

pub(crate) fn named_by(isrc: &Isrc, held: Option<&str>) -> bool {
    held.is_some_and(|held| Isrc::new(held.trim()).is_ok_and(|held| held == *isrc))
}

pub(crate) trait Finds {
    fn tracks_named_by(&self, isrc: &Isrc) -> Result<Vec<TrackId>>;

    fn delivered(&self, track: TrackId) -> Result<Option<Delivery>>;
}

pub(crate) fn obtained(finds: &impl Finds, identity: &Identity) -> Result<Obtained> {
    let linked = identity.track_on(Service::Tidal).and_then(TrackId::linked);
    if linked.is_none() && identity.isrc.is_none() {
        return Ok(Obtained::Nothing);
    }
    if let Some(track) = linked
        && let Some(delivery) = finds.delivered(track)?
    {
        return Ok(Obtained::Found(delivery));
    }
    let Some(isrc) = &identity.isrc else {
        return Ok(Obtained::Nothing);
    };
    for track in finds
        .tracks_named_by(isrc)?
        .into_iter()
        .filter(|track| Some(*track) != linked)
    {
        if let Some(delivery) = finds.delivered(track)? {
            return Ok(Obtained::Found(delivery));
        }
    }
    Ok(Obtained::Nothing)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Playback {
    asset_presentation: String,
    manifest_mime_type: String,
    manifest: String,
}

pub(crate) struct Player<'a> {
    pub(crate) source: &'a SourceId,
    pub(crate) hosts: &'a MediaHosts,
    pub(crate) media: &'a Agent,
}

impl Player<'_> {
    fn unreadable(&self, op: ProviderOp) -> Error {
        Error::Unreadable {
            provider: self.source.clone(),
            op,
        }
    }

    fn media_in(&self, track: TrackId, playback: &Playback) -> Result<Option<Media>> {
        let op = ProviderOp::Playback;
        if !playback.asset_presentation.eq_ignore_ascii_case(FULL) {
            tracing::debug!(track = track.0, presentation = %playback.asset_presentation, provider = %self.source, "offered less than the whole track");
            return Ok(None);
        }
        let media = match manifest::read(&playback.manifest_mime_type, &playback.manifest) {
            Ok(Manifest::Media(media)) => media,
            Ok(Manifest::Withheld(withheld)) => {
                tracing::debug!(track = track.0, ?withheld, provider = %self.source, "a stream this provider does not take");
                return Ok(None);
            }
            Err(unread) => {
                tracing::debug!(track = track.0, ?unread, mime = %playback.manifest_mime_type, provider = %self.source, "a manifest could not be read");
                return Err(self.unreadable(op));
            }
        };
        if let Some(elsewhere) = media.urls.iter().find(|url| !self.hosts.holds(url)) {
            tracing::warn!(track = track.0, host = %elsewhere.split('/').nth(2).unwrap_or_default(), provider = %self.source, "a manifest named media off TIDAL's audio hosts");
            return Err(Error::OffItsHosts {
                provider: self.source.clone(),
                op,
            });
        }
        Ok(Some(media))
    }

    fn downloaded(&self, media: Media) -> Result<Box<dyn Read + Send>> {
        let op = ProviderOp::Download;
        let fetched =
            Fetched::opened(self.media.clone(), media.urls).map_err(
                |unfetched| match unfetched {
                    Unfetched::Io(source) => Error::Io {
                        provider: self.source.clone(),
                        op,
                        source,
                    },
                    Unfetched::Refused(status) => Error::Refused {
                        provider: self.source.clone(),
                        op,
                        status,
                    },
                },
            )?;
        match media.container {
            Container::Flac => Ok(Box::new(fetched)),
            Container::Mp4 => match Remuxed::opened(fetched, media.timeline) {
                Ok(remuxed) => Ok(Box::new(remuxed)),
                Err(Unremuxable::Io(source)) => Err(Error::Io {
                    provider: self.source.clone(),
                    op,
                    source,
                }),
                Err(unremuxable) => {
                    tracing::debug!(?unremuxable, provider = %self.source, "a stream held no FLAC track to take");
                    Err(self.unreadable(op))
                }
            },
        }
    }

    pub(crate) fn delivered(
        &self,
        track: TrackId,
        playback: &Playback,
    ) -> Result<Option<Delivery>> {
        let Some(media) = self.media_in(track, playback)? else {
            return Ok(None);
        };
        Ok(Some(Delivery::Stream {
            key: format!("track/{}", track.0).into_boxed_str(),
            extension: Extension::new(DELIVERED_AS)?,
            reader: self.downloaded(media)?,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tidal_link_names_its_track() {
        for (link, track) in [
            ("https://tidal.com/track/55391743", 55_391_743),
            ("https://tidal.com/browse/track/55391743?u", 55_391_743),
            ("https://listen.tidal.com/track/12/", 12),
        ] {
            assert_eq!(TrackId::linked(link), Some(TrackId(track)), "{link}");
        }
        assert_eq!(TrackId::linked("https://tidal.com/album/55391740"), None);
        assert_eq!(TrackId::linked("https://tidal.com/track/echoes"), None);
    }
}
