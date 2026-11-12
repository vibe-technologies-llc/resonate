use std::{
    io::{self, Read, Seek, SeekFrom},
    sync::Arc,
};

use resonate_codec::{FormatHint, Media, MediaProvider, MediaStream};
use resonate_core::{MediaLocation, SourceId};
use resonate_providers::{Named, PlaysInPlace, Streamed};

pub struct PlayedInPlace<F> {
    scheme: SourceId,
    serving: F,
}

impl<F> PlayedInPlace<F>
where
    F: Fn(&SourceId) -> Option<Arc<dyn PlaysInPlace>> + Send + Sync,
{
    pub const fn new(scheme: SourceId, serving: F) -> Self {
        Self { scheme, serving }
    }
}

impl<F> MediaProvider for PlayedInPlace<F>
where
    F: Fn(&SourceId) -> Option<Arc<dyn PlaysInPlace>> + Send + Sync,
{
    fn source(&self) -> &SourceId {
        &self.scheme
    }

    fn open(&self, location: &MediaLocation) -> resonate_codec::Result<Media> {
        let service = (self.serving)(location.source());
        let named = service
            .as_ref()
            .and_then(|service| service.named_by(location));
        let (Some(service), Some(Named::Song(id))) = (service, named) else {
            return Err(resonate_codec::Error::LocatorNotUsable {
                location: location.clone(),
            });
        };

        let streamed = service
            .streamed(&id)
            .map_err(|error| resonate_codec::Error::Io {
                location: location.clone(),
                source: io::Error::other(error),
            })?;
        let hint = streamed
            .media_type
            .as_deref()
            .map(|media_type| FormatHint::MediaType(Box::from(media_type)));

        Ok(Media {
            stream: Box::new(Streaming(streamed)),
            hint,
        })
    }
}

struct Streaming(Streamed);

impl Read for Streaming {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Seek for Streaming {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        self.0.seek(to)
    }
}

impl MediaStream for Streaming {
    fn is_seekable(&self) -> bool {
        self.0.is_seekable()
    }

    fn byte_len(&self) -> Option<u64> {
        self.0.length()
    }
}
