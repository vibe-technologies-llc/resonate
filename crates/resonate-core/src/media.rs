use std::{
    borrow::Cow,
    ffi::OsString,
    fmt,
    os::unix::ffi::{OsStrExt as _, OsStringExt as _},
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use crate::{Error, FrameSpan, Frames, Result};

const LOCAL: &str = "local";
const SOURCE_NAME_LIMIT: usize = 32;
const SOURCE_SEPARATOR: char = ':';
const KEY_SEPARATOR: char = '/';
const EXTENSION_SEPARATOR: char = '.';
const FILE_SCHEME: &str = "file://";
const FILE_LOCALHOST: &str = "localhost";
const PERCENT: u8 = b'%';
const SPAN_FRAGMENT: &str = "#frames=";
const SPAN_TO: char = '-';

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceId(Arc<str>);

static THE_LOCAL_SOURCE: OnceLock<Arc<str>> = OnceLock::new();

impl SourceId {
    pub fn local() -> Self {
        Self(Arc::clone(
            THE_LOCAL_SOURCE.get_or_init(|| Arc::from(LOCAL)),
        ))
    }

    pub fn new(name: &str) -> Result<Self> {
        if !nameable(name) {
            return Err(Error::SourceNameNotUsable);
        }
        Ok(Self(Arc::from(name)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_local(&self) -> bool {
        self.as_str() == LOCAL
    }
}

fn nameable(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= SOURCE_NAME_LIMIT
        && name
            .chars()
            .all(|character| matches!(character, 'a'..='z' | '0'..='9' | '-' | '_'))
}

impl fmt::Display for SourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Locator {
    Path(PathBuf),
    Key(Box<str>),
}

impl Locator {
    pub fn as_path(&self) -> Option<&Path> {
        match self {
            Self::Path(path) => Some(path),
            Self::Key(_) => None,
        }
    }

    pub fn as_key(&self) -> Option<&str> {
        match self {
            Self::Key(key) => Some(key),
            Self::Path(_) => None,
        }
    }
}

impl fmt::Display for Locator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path(path) => write!(f, "{}", path.display()),
            Self::Key(key) => f.write_str(key),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MediaLocation {
    source: SourceId,
    locator: Locator,
}

impl MediaLocation {
    pub fn local(path: impl Into<PathBuf>) -> Self {
        Self {
            source: SourceId::local(),
            locator: Locator::Path(path.into()),
        }
    }

    pub fn new(source: SourceId, key: impl Into<Box<str>>) -> Self {
        Self {
            source,
            locator: Locator::Key(key.into()),
        }
    }

    pub const fn source(&self) -> &SourceId {
        &self.source
    }

    pub const fn locator(&self) -> &Locator {
        &self.locator
    }

    pub fn is_local(&self) -> bool {
        self.source.is_local()
    }

    pub fn as_path(&self) -> Option<&Path> {
        self.locator.as_path()
    }

    pub fn stem(&self) -> Option<Cow<'_, str>> {
        match &self.locator {
            Locator::Path(path) => path.file_stem().map(|stem| stem.to_string_lossy()),
            Locator::Key(key) => {
                let leaf = leaf_of(key);
                let stem = leaf
                    .rsplit_once(EXTENSION_SEPARATOR)
                    .map_or(leaf, |(stem, _)| stem);
                (!stem.is_empty()).then_some(Cow::Borrowed(stem))
            }
        }
    }

    pub fn to_uri(&self) -> String {
        match &self.locator {
            Locator::Path(path) => {
                format!("{FILE_SCHEME}{}", escaped(path.as_os_str().as_bytes()))
            }
            Locator::Key(key) => {
                format!(
                    "{}{SOURCE_SEPARATOR}{}",
                    self.source,
                    escaped(key.as_bytes())
                )
            }
        }
    }

    pub fn to_uri_within(&self, span: Option<FrameSpan>) -> String {
        let uri = self.to_uri();
        let Some(span) = span else {
            return uri;
        };
        let end = span
            .end()
            .map_or_else(String::new, |end| end.get().to_string());
        format!("{uri}{SPAN_FRAGMENT}{}{SPAN_TO}{end}", span.start().get())
    }

    pub fn from_uri_within(uri: &str) -> Option<(Self, Option<FrameSpan>)> {
        let Some((whole, fragment)) = uri.split_once(SPAN_FRAGMENT) else {
            return Some((Self::from_uri(uri)?, None));
        };
        let (start, end) = fragment.split_once(SPAN_TO)?;
        let start = Frames(start.parse().ok()?);
        let span = if end.is_empty() {
            FrameSpan::starting(start)
        } else {
            let end = Frames(end.parse().ok()?);
            if end <= start {
                return None;
            }
            FrameSpan::between(start, end)
        };
        Some((Self::from_uri(whole)?, Some(span)))
    }

    pub fn from_uri(uri: &str) -> Option<Self> {
        if let Some(encoded) = uri.strip_prefix(FILE_SCHEME) {
            let encoded = encoded.strip_prefix(FILE_LOCALHOST).unwrap_or(encoded);
            if !encoded.starts_with(KEY_SEPARATOR) {
                return None;
            }
            return Some(Self::local(OsString::from_vec(unescaped(encoded)?)));
        }

        let (scheme, key) = uri.split_once(SOURCE_SEPARATOR)?;
        let key = String::from_utf8(unescaped(key)?).ok()?;
        Some(Self::new(SourceId::new(scheme).ok()?, key))
    }

    pub fn extension(&self) -> Option<&str> {
        match &self.locator {
            Locator::Path(path) => path.extension().and_then(|extension| extension.to_str()),
            Locator::Key(key) => leaf_of(key)
                .rsplit_once(EXTENSION_SEPARATOR)
                .map(|(_, extension)| extension)
                .filter(|extension| !extension.is_empty()),
        }
    }
}

fn escaped(text: &[u8]) -> String {
    let mut escaped = String::with_capacity(text.len());
    for &byte in text {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                escaped.push(char::from(byte));
            }
            other => {
                escaped.push(char::from(PERCENT));
                escaped.push_str(&format!("{other:02X}"));
            }
        }
    }
    escaped
}

fn unescaped(encoded: &str) -> Option<Vec<u8>> {
    let mut decoded = Vec::with_capacity(encoded.len());
    let mut bytes = encoded.bytes();
    while let Some(byte) = bytes.next() {
        if byte != PERCENT {
            decoded.push(byte);
            continue;
        }
        let high = hex(bytes.next()?)?;
        let low = hex(bytes.next()?)?;
        decoded.push(high << 4 | low);
    }
    Some(decoded)
}

const fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn leaf_of(key: &str) -> &str {
    key.rsplit_once(KEY_SEPARATOR).map_or(key, |(_, leaf)| leaf)
}

impl fmt::Display for MediaLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_local() {
            return write!(f, "{}", self.locator);
        }
        write!(f, "{}{SOURCE_SEPARATOR}{}", self.source, self.locator)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_name_is_an_identifier_rather_than_a_sentence() {
        assert_eq!(
            SourceId::new("subsonic")
                .expect("a lowercase name")
                .as_str(),
            "subsonic"
        );
        assert!(SourceId::new("my_source-2").is_ok());
        assert!(SourceId::new("").is_err());
        assert!(SourceId::new("Subsonic").is_err());
        assert!(SourceId::new("sub sonic").is_err());
        assert!(SourceId::new("sub:sonic").is_err());
        assert!(SourceId::new(&"s".repeat(SOURCE_NAME_LIMIT + 1)).is_err());
    }

    #[test]
    fn a_local_file_keeps_the_path_it_was_given() {
        let location = MediaLocation::local("/music/Pink Floyd/Echoes.flac");

        assert!(location.is_local());
        assert_eq!(
            location.as_path(),
            Some(Path::new("/music/Pink Floyd/Echoes.flac"))
        );
        assert_eq!(location.extension(), Some("flac"));
        assert_eq!(location.stem().as_deref(), Some("Echoes"));
        assert_eq!(location.to_string(), "/music/Pink Floyd/Echoes.flac");
    }

    #[test]
    fn a_source_that_is_not_local_addresses_its_media_by_key() {
        let source = SourceId::new("subsonic").expect("a lowercase name");
        let location = MediaLocation::new(source, "library/42/Echoes.flac");

        assert!(!location.is_local());
        assert_eq!(location.as_path(), None);
        assert_eq!(location.locator().as_key(), Some("library/42/Echoes.flac"));
        assert_eq!(location.extension(), Some("flac"));
        assert_eq!(location.stem().as_deref(), Some("Echoes"));
        assert_eq!(location.to_string(), "subsonic:library/42/Echoes.flac");
    }

    #[test]
    fn a_key_that_names_no_extension_still_has_a_name_of_last_resort() {
        let source = SourceId::new("radio").expect("a lowercase name");
        let location = MediaLocation::new(source, "streams/bbc-6-music");

        assert_eq!(location.extension(), None);
        assert_eq!(location.stem().as_deref(), Some("bbc-6-music"));
    }

    #[test]
    fn a_local_file_round_trips_through_the_uri_a_desktop_hands_over() {
        for path in [
            "/music/Pink Floyd/Echoes.flac",
            "/music/100% Live/a+b.mp3",
            "/music/Sigur Rós/Ágætis byrjun.flac",
        ] {
            let location = MediaLocation::local(path);
            let uri = location.to_uri();

            assert!(uri.starts_with(FILE_SCHEME), "{uri} named no scheme");
            assert!(!uri.contains(' '), "{uri} was left unescaped");
            assert_eq!(MediaLocation::from_uri(&uri), Some(location));
        }
    }

    #[test]
    fn a_source_that_is_not_local_is_named_by_its_scheme_in_a_uri() {
        let location = MediaLocation::new(
            SourceId::new("subsonic").expect("a lowercase name"),
            "track/1 2",
        );
        let uri = location.to_uri();

        assert_eq!(uri, "subsonic:track/1%202");
        assert_eq!(MediaLocation::from_uri(&uri), Some(location));
    }

    #[test]
    fn a_cut_of_a_file_round_trips_through_a_uri_that_names_its_frames() {
        let file = MediaLocation::local("/music/Meddle #1.flac");
        let closed = FrameSpan::between(Frames(588), Frames(44_100));
        let open = FrameSpan::starting(Frames(44_100));

        for span in [None, Some(closed), Some(open)] {
            let uri = file.to_uri_within(span);
            assert_eq!(
                MediaLocation::from_uri_within(&uri),
                Some((file.clone(), span)),
                "{uri}"
            );
        }
        assert_eq!(
            file.to_uri_within(Some(closed)),
            "file:///music/Meddle%20%231.flac#frames=588-44100"
        );
    }

    #[test]
    fn a_cut_that_ends_before_it_starts_or_names_no_number_is_refused() {
        for uri in [
            "file:///music/a.flac#frames=100-50",
            "file:///music/a.flac#frames=100-100",
            "file:///music/a.flac#frames=x-",
            "file:///music/a.flac#frames=100",
        ] {
            assert_eq!(MediaLocation::from_uri_within(uri), None, "{uri}");
        }
    }

    #[test]
    fn a_path_that_is_not_utf8_survives_the_uri_it_is_written_as() {
        use std::os::unix::ffi::OsStrExt as _;

        let named = PathBuf::from(std::ffi::OsStr::from_bytes(b"/music/caf\xe9.flac"));
        let uri = MediaLocation::local(&named).to_uri();

        assert_eq!(uri, "file:///music/caf%E9.flac");
        assert_eq!(
            MediaLocation::from_uri(&uri)
                .as_ref()
                .and_then(MediaLocation::as_path),
            Some(named.as_path())
        );
    }

    #[test]
    fn a_uri_no_source_could_answer_for_is_refused() {
        assert_eq!(MediaLocation::from_uri("file://relative/../a.mp3"), None);
        assert_eq!(MediaLocation::from_uri("file:///music/a%2.mp3"), None);
        assert_eq!(MediaLocation::from_uri("Subsonic:track/1"), None);
        assert_eq!(MediaLocation::from_uri("nothing"), None);
        assert_eq!(MediaLocation::from_uri("/music/Pink Floyd/a:b.flac"), None);
        assert_eq!(
            MediaLocation::from_uri("file://localhost/music/a.mp3"),
            Some(MediaLocation::local("/music/a.mp3"))
        );
    }

    #[test]
    fn two_sources_holding_the_same_key_are_different_media() {
        let left = MediaLocation::new(
            SourceId::new("subsonic").expect("a lowercase name"),
            "track/1",
        );
        let right = MediaLocation::new(
            SourceId::new("jellyfin").expect("a lowercase name"),
            "track/1",
        );

        assert_ne!(left, right);
    }
}
