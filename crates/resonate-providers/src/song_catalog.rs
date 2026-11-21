use std::{
    io::{self, Read},
    sync::Arc,
};

use resonate_core::SourceId;

use crate::{
    Delivery, Error, Extension, Id, Identity, Named, Obtained, Opened, Opening, PlaysInPlace,
    Provider, Providers, Result, Streamed,
};

#[derive(Clone)]
pub struct SongCatalog {
    namespace: String,
    service: Arc<dyn PlaysInPlace>,
}

impl SongCatalog {
    pub fn new(namespace: String, service: Arc<dyn PlaysInPlace>) -> Self {
        Self { namespace, service }
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn service(&self) -> &dyn PlaysInPlace {
        self.service.as_ref()
    }

    pub fn same_service(&self, other: &Self) -> bool {
        self.namespace == other.namespace && Arc::ptr_eq(&self.service, &other.service)
    }

    pub fn delivery(&self, id: Id, suffix: Option<String>) -> Providers {
        Providers::none().and(Arc::new(ExactSong {
            service: Arc::clone(&self.service),
            id,
            suffix,
        }))
    }
}

struct ExactSong {
    service: Arc<dyn PlaysInPlace>,
    id: Id,
    suffix: Option<String>,
}

impl Provider for ExactSong {
    fn source(&self) -> &SourceId {
        self.service.scheme()
    }

    fn find(&self, _identity: &Identity) -> Result<Obtained> {
        let streamed = match self.service.streamed(&self.id) {
            Ok(streamed) => streamed,
            Err(Error::Refused {
                status: 404 | 410, ..
            }) => return Ok(Obtained::Nothing),
            Err(error) => return Err(error),
        };
        let extension = stream_extension(self.suffix.as_deref(), streamed.media_type.as_deref());
        let key = Named::Song(self.id.clone()).location(self.source());
        let length = streamed.length();
        Ok(Obtained::Found(Delivery::Stream {
            key: key.locator().as_key().unwrap_or_default().into(),
            extension,
            opening: Opening::new(move || {
                Ok(Opened::Reading {
                    reader: Box::new(ExactBytes {
                        streamed,
                        remaining: length,
                    }),
                    length,
                })
            }),
        }))
    }

    fn picture(&self, address: &str) -> Result<Vec<u8>> {
        let id = Id::new(address)
            .ok_or_else(|| crate::not_offered(self.source(), crate::ProviderOp::Cover))?;
        self.service.cover(&id, 512)
    }
}

struct ExactBytes {
    streamed: Streamed,
    remaining: Option<u64>,
}

impl Read for ExactBytes {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let Some(remaining) = &mut self.remaining else {
            return self.streamed.read(buffer);
        };

        if *remaining == 0 || buffer.is_empty() {
            return Ok(0);
        }

        let limit = usize::try_from(*remaining)
            .unwrap_or(usize::MAX)
            .min(buffer.len());
        let read = self.streamed.read(&mut buffer[..limit])?;

        if read == 0 {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
        }
        *remaining -= read as u64;
        Ok(read)
    }
}

pub fn stream_extension(known: Option<&str>, media_type: Option<&str>) -> Extension {
    let from_type = || {
        let media_type = media_type?.split(';').next()?.trim().to_ascii_lowercase();
        Some(match media_type.as_str() {
            "audio/aac" => "aac",
            "audio/aiff" | "audio/x-aiff" => "aiff",
            "audio/alac" | "audio/mp4" | "audio/x-m4a" => "m4a",
            "audio/flac" | "audio/x-flac" => "flac",
            "audio/mpeg" | "audio/mp3" | "audio/x-mp3" => "mp3",
            "audio/ogg" | "application/ogg" => "ogg",
            "audio/opus" => "opus",
            "audio/wav" | "audio/wave" | "audio/x-wav" => "wav",
            "audio/webm" => "webm",
            _ => return None,
        })
    };
    known
        .and_then(|suffix| Extension::new(suffix).ok())
        .or_else(|| from_type().and_then(|suffix| Extension::new(suffix).ok()))
        .unwrap_or_else(Extension::flac)
}
