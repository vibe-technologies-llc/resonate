use std::{io::Cursor, sync::Arc};

use resonate_codec::{Media, MediaProvider, Reading, Result, Sources};
use resonate_core::{MediaLocation, SourceId};

pub struct Held {
    source: SourceId,
    bytes: Vec<u8>,
}

impl MediaProvider for Held {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn open(&self, _location: &MediaLocation) -> Result<Media> {
        Ok(Media {
            stream: Box::new(Reading::new(Cursor::new(self.bytes.clone()))),
            hint: None,
        })
    }
}

pub fn held(bytes: &[u8]) -> (Sources, MediaLocation) {
    let source = SourceId::new("fuzzed").expect("a lowercase name");
    let sources = Sources::local().and(Arc::new(Held {
        source: source.clone(),
        bytes: bytes.to_vec(),
    }));

    (sources, MediaLocation::new(source, "held"))
}
