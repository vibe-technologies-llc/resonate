#![allow(dead_code)]

use std::{
    io::{self, Cursor, Read, Seek, SeekFrom},
    sync::Arc,
};

use resonate_codec::{FormatHint, Media, MediaProvider, MediaStream, Reading, Result, Sources};
use resonate_core::{MediaLocation, SourceId};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Arrival {
    Seekable,
    Piped,
}

#[derive(Clone)]
pub struct Shape {
    pub arrival: Arrival,
    pub hint: Option<FormatHint>,
}

impl Shape {
    pub const FILE: Self = Self {
        arrival: Arrival::Seekable,
        hint: None,
    };
}

struct Piped(Cursor<Vec<u8>>);

impl Read for Piped {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Seek for Piped {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}

impl MediaStream for Piped {
    fn is_seekable(&self) -> bool {
        false
    }

    fn byte_len(&self) -> Option<u64> {
        None
    }
}

pub struct Held {
    source: SourceId,
    bytes: Vec<u8>,
    shape: Shape,
}

impl MediaProvider for Held {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn open(&self, _location: &MediaLocation) -> Result<Media> {
        let bytes = Cursor::new(self.bytes.clone());
        let stream: Box<dyn MediaStream> = match self.shape.arrival {
            Arrival::Seekable => Box::new(Reading::new(bytes)),
            Arrival::Piped => Box::new(Piped(bytes)),
        };

        Ok(Media {
            stream,
            hint: self.shape.hint.clone(),
        })
    }
}

pub fn held(bytes: &[u8]) -> (Sources, MediaLocation) {
    held_as(bytes, Shape::FILE)
}

pub fn held_as(bytes: &[u8], shape: Shape) -> (Sources, MediaLocation) {
    let source = SourceId::new("fuzzed").expect("a lowercase name");
    let sources = Sources::local().and(Arc::new(Held {
        source: source.clone(),
        bytes: bytes.to_vec(),
        shape,
    }));

    (sources, MediaLocation::new(source, "held"))
}
