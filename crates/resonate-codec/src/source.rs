use std::{
    cmp::min,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    sync::Arc,
};

use resonate_core::{FrameSpan, MediaLocation, SourceId, TrackHints};

use crate::{Error, Result, TagSet};

pub trait MediaStream: Read + Seek + Send + Sync {
    fn is_seekable(&self) -> bool;

    fn byte_len(&self) -> Option<u64>;
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FormatHint {
    Extension(Box<str>),
    MediaType(Box<str>),
}

impl FormatHint {
    pub(crate) fn of(location: &MediaLocation) -> Option<Self> {
        location
            .extension()
            .map(|extension| Self::Extension(Box::from(extension)))
    }
}

pub struct Media {
    pub stream: Box<dyn MediaStream>,
    pub hint: Option<FormatHint>,
}

pub trait MediaProvider: Send + Sync {
    fn source(&self) -> &SourceId;

    fn open(&self, location: &MediaLocation) -> Result<Media>;
}

pub struct Reading<R> {
    inner: R,
    len: Option<u64>,
}

impl<R: Read + Seek> Reading<R> {
    pub fn new(mut inner: R) -> Self {
        let len = measure(&mut inner);
        Self { inner, len }
    }
}

fn measure<R: Seek>(reader: &mut R) -> Option<u64> {
    let here = reader.stream_position().ok()?;
    let end = reader.seek(SeekFrom::End(0)).ok()?;
    reader.seek(SeekFrom::Start(here)).ok()?;
    Some(end)
}

impl<R: Read> Read for Reading<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl<R: Seek> Seek for Reading<R> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        self.inner.seek(to)
    }
}

impl<R: Read + Seek + Send + Sync> MediaStream for Reading<R> {
    fn is_seekable(&self) -> bool {
        self.len.is_some()
    }

    fn byte_len(&self) -> Option<u64> {
        self.len
    }
}

pub(crate) struct Replaying {
    head: Vec<u8>,
    at: usize,
    inner: Box<dyn MediaStream>,
}

impl Replaying {
    pub(crate) fn over(inner: Box<dyn MediaStream>, head: Vec<u8>) -> Self {
        Self { head, at: 0, inner }
    }
}

impl Read for Replaying {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let Some(replaying) = self.head.get(self.at..) else {
            return self.inner.read(buf);
        };
        if replaying.is_empty() {
            return self.inner.read(buf);
        }

        let taken = min(replaying.len(), buf.len());
        let (Some(from), Some(into)) = (replaying.get(..taken), buf.get_mut(..taken)) else {
            return Ok(0);
        };
        into.copy_from_slice(from);
        self.at = self.at.saturating_add(taken);
        Ok(taken)
    }
}

impl Seek for Replaying {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "this source cannot seek",
        ))
    }
}

impl MediaStream for Replaying {
    fn is_seekable(&self) -> bool {
        false
    }

    fn byte_len(&self) -> Option<u64> {
        None
    }
}

pub struct LocalFiles {
    source: SourceId,
}

impl Default for LocalFiles {
    fn default() -> Self {
        Self {
            source: SourceId::local(),
        }
    }
}

impl MediaProvider for LocalFiles {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn open(&self, location: &MediaLocation) -> Result<Media> {
        let path = location.as_path().ok_or_else(|| Error::LocatorNotUsable {
            location: location.clone(),
        })?;
        let file = File::open(path).map_err(|source| Error::Io {
            location: location.clone(),
            source,
        })?;

        Ok(Media {
            stream: Box::new(Reading::new(file)),
            hint: FormatHint::of(location),
        })
    }
}

pub trait StandIn: Send + Sync {
    fn stands_in(&self, location: &MediaLocation, span: Option<FrameSpan>) -> Option<StoodIn>;
}

pub trait Hinting: Send + Sync {
    fn hints(&self, location: &MediaLocation, span: Option<FrameSpan>) -> TrackHints;
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoodIn {
    pub location: MediaLocation,
    pub tags: TagSet,
}

pub struct Sources {
    providers: Vec<Arc<dyn MediaProvider>>,
    stand_in: Option<Arc<dyn StandIn>>,
    hinting: Option<Arc<dyn Hinting>>,
}

impl Sources {
    pub fn local() -> Self {
        Self {
            providers: vec![Arc::new(LocalFiles::default())],
            stand_in: None,
            hinting: None,
        }
    }

    #[must_use]
    pub fn standing_in(mut self, stand_in: Arc<dyn StandIn>) -> Self {
        self.stand_in = Some(stand_in);
        self
    }

    #[must_use]
    pub fn hinted_by(mut self, hinting: Arc<dyn Hinting>) -> Self {
        self.hinting = Some(hinting);
        self
    }

    pub fn hints(&self, location: &MediaLocation, span: Option<FrameSpan>) -> TrackHints {
        self.hinting
            .as_ref()
            .map_or_else(TrackHints::default, |hinting| hinting.hints(location, span))
    }

    pub(crate) fn stood_in(
        &self,
        location: &MediaLocation,
        span: Option<FrameSpan>,
    ) -> Option<StoodIn> {
        self.stand_in.as_ref()?.stands_in(location, span)
    }

    #[must_use]
    pub fn and(mut self, provider: Arc<dyn MediaProvider>) -> Self {
        self.providers
            .retain(|held| held.source() != provider.source());
        self.providers.push(provider);
        self
    }

    pub fn names(&self) -> Vec<SourceId> {
        self.providers
            .iter()
            .map(|provider| provider.source().clone())
            .collect()
    }

    pub fn provider(&self, source: &SourceId) -> Option<&dyn MediaProvider> {
        self.providers
            .iter()
            .find(|provider| provider.source() == source)
            .map(AsRef::as_ref)
    }

    pub fn open(&self, location: &MediaLocation) -> Result<Media> {
        self.provider(location.source())
            .ok_or_else(|| Error::NoSuchSource {
                location: location.clone(),
            })?
            .open(location)
    }
}

impl Default for Sources {
    fn default() -> Self {
        Self::local()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use resonate_core::Locator;

    use super::*;

    struct Held {
        source: SourceId,
    }

    impl MediaProvider for Held {
        fn source(&self) -> &SourceId {
            &self.source
        }

        fn open(&self, _location: &MediaLocation) -> Result<Media> {
            Ok(Media {
                stream: Box::new(Reading::new(Cursor::new(b"held".to_vec()))),
                hint: None,
            })
        }
    }

    fn named(name: &str) -> SourceId {
        SourceId::new(name).expect("a lowercase name")
    }

    #[test]
    fn a_registry_starts_with_the_local_files_and_nothing_else() {
        let sources = Sources::local();

        assert_eq!(sources.names(), vec![SourceId::local()]);
        assert!(sources.provider(&named("subsonic")).is_none());
    }

    #[test]
    fn a_location_no_provider_answers_for_is_refused_by_name() {
        let location = MediaLocation::new(named("subsonic"), "track/1");

        let opened = Sources::local().open(&location);

        assert!(matches!(opened, Err(Error::NoSuchSource { .. })));
    }

    #[test]
    fn a_registered_provider_is_the_one_its_own_locations_reach() {
        let sources = Sources::local().and(Arc::new(Held {
            source: named("subsonic"),
        }));
        let location = MediaLocation::new(named("subsonic"), "track/1");

        let mut media = sources.open(&location).expect("the provider answers");
        let mut held = String::new();
        media.stream.read_to_string(&mut held).expect("it reads");

        assert_eq!(held, "held");
        assert_eq!(sources.names().len(), 2);
    }

    #[test]
    fn registering_a_source_twice_replaces_it_rather_than_shadowing_it() {
        let sources = Sources::local()
            .and(Arc::new(LocalFiles::default()))
            .and(Arc::new(Held {
                source: named("subsonic"),
            }));

        assert_eq!(sources.names(), vec![SourceId::local(), named("subsonic")]);
    }

    #[test]
    fn the_local_provider_refuses_a_locator_that_is_not_a_path() {
        let location = MediaLocation::new(SourceId::local(), "track/1");

        let opened = LocalFiles::default().open(&location);

        assert!(matches!(opened, Err(Error::LocatorNotUsable { .. })));
        assert_eq!(location.locator().as_key(), Some("track/1"));
        assert!(matches!(location.locator(), Locator::Key(_)));
    }
}
