use std::{fs::File, io, os::fd::FromRawFd, sync::Arc};

use resonate_codec::{FormatHint, Media, MediaProvider, Reading};
use resonate_core::{AUDIO_EXTENSIONS, Locator, MediaLocation, SourceId};

use crate::{Error, Result};

#[uniffi::export(callback_interface)]
pub trait Documents: Send + Sync {
    fn open_document(&self, uri: String) -> i32;
}

pub(crate) const DOCUMENT_REFUSED: i32 = -1;
pub(crate) const DOCUMENT_NOT_FOUND: i32 = -2;

pub(crate) const GRANTED_SCHEMES: [&str; 2] = ["content", "resonate-folder"];

pub(crate) struct GrantedFiles {
    pub(crate) documents: Arc<dyn Documents>,
    source: SourceId,
}

impl GrantedFiles {
    pub(crate) fn new(documents: Arc<dyn Documents>, scheme: &str) -> Result<Self> {
        Ok(Self {
            documents,
            source: SourceId::new(scheme)?,
        })
    }
}

impl MediaProvider for GrantedFiles {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn open(&self, location: &MediaLocation) -> resonate_codec::Result<Media> {
        let Locator::Key(key) = location.locator() else {
            return Err(resonate_codec::Error::LocatorNotUsable {
                location: location.clone(),
            });
        };
        let descriptor = self
            .documents
            .open_document(format!("{}:{key}", self.source.as_str()));
        if descriptor < 0 {
            let kind = match descriptor {
                DOCUMENT_NOT_FOUND => io::ErrorKind::NotFound,
                _ => io::ErrorKind::PermissionDenied,
            };
            return Err(resonate_codec::Error::Io {
                location: location.clone(),
                source: kind.into(),
            });
        }
        Ok(Media {
            stream: Box::new(Reading::new(owned(descriptor))),
            hint: location
                .extension()
                .map(|extension| FormatHint::Extension(extension.into())),
        })
    }
}

pub(crate) fn owned(descriptor: i32) -> File {
    unsafe { File::from_raw_fd(descriptor) }
}

pub(crate) fn location(uri: &str) -> Result<MediaLocation> {
    let (scheme, key) = GRANTED_SCHEMES
        .iter()
        .find_map(|scheme| {
            let key = uri.strip_prefix(scheme)?.strip_prefix(':')?;
            key.starts_with("//").then_some((scheme, key))
        })
        .ok_or(Error::InvalidUri)?;
    Ok(MediaLocation::new(SourceId::new(scheme)?, key))
}

pub(crate) fn document_uri(location: &MediaLocation) -> Result<String> {
    let source = location.source().as_str();
    match location.locator() {
        Locator::Key(key) if GRANTED_SCHEMES.contains(&source) => Ok(format!("{source}:{key}")),
        _ => Err(Error::InvalidUri),
    }
}

#[uniffi::export]
pub fn document_refused() -> i32 {
    DOCUMENT_REFUSED
}

#[uniffi::export]
pub fn document_not_found() -> i32 {
    DOCUMENT_NOT_FOUND
}

#[uniffi::export]
pub fn audio_extensions() -> Vec<String> {
    AUDIO_EXTENSIONS
        .iter()
        .map(|extension| (*extension).to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use parking_lot::Mutex;

    use super::*;

    struct Refused(Mutex<Vec<String>>);

    impl Documents for Refused {
        fn open_document(&self, uri: String) -> i32 {
            self.0.lock().push(uri);
            DOCUMENT_REFUSED
        }
    }

    struct Answering(i32);

    impl Documents for Answering {
        fn open_document(&self, _: String) -> i32 {
            self.0
        }
    }

    #[test]
    fn only_a_document_that_is_not_found_reads_as_not_found() {
        let at = location("content://documents/tree/a/document/a%2Fb.flac").expect("a location");

        for (answer, kind) in [
            (DOCUMENT_NOT_FOUND, io::ErrorKind::NotFound),
            (DOCUMENT_REFUSED, io::ErrorKind::PermissionDenied),
        ] {
            let provider =
                GrantedFiles::new(Arc::new(Answering(answer)), "content").expect("a provider");

            assert!(matches!(
                provider.open(&at),
                Err(resonate_codec::Error::Io { source, .. }) if source.kind() == kind
            ));
        }
    }

    #[test]
    fn a_grant_keeps_its_escaping_through_the_media_registry_and_saved_queue() {
        for (scheme, uri) in [
            (
                "content",
                "content://documents/tree/Music%3A%2FAlbum/document/Music%3A%2FAlbum%2F100%25.flac",
            ),
            (
                "resonate-folder",
                "resonate-folder://3F2A9C/Album%20One/100%25%3F.flac",
            ),
        ] {
            let original = location(uri).expect("a granted URI");
            let restored = MediaLocation::from_uri(&original.to_uri()).expect("the saved location");

            assert_eq!(original, restored);
            assert_eq!(restored.source().as_str(), scheme);
            assert_eq!(
                document_uri(&restored).expect("the original document key"),
                uri
            );

            let callback = Arc::new(Refused(Mutex::new(Vec::new())));
            let provider = GrantedFiles::new(callback.clone(), scheme).expect("a granted provider");

            assert!(provider.open(&restored).is_err());
            assert_eq!(callback.0.lock().as_slice(), [uri]);
        }
    }

    #[test]
    fn the_ios_document_types_admit_what_a_scan_admits() {
        let plist = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ios/Resonate/Info.plist"),
        )
        .expect("the iOS Info.plist");
        let (_, declared) = plist
            .split_once("<key>public.filename-extension</key>")
            .expect("the declared extensions");
        let (listed, _) = declared.split_once("</array>").expect("the extension list");
        let mut extensions = listed
            .split("<string>")
            .skip(1)
            .filter_map(|entry| entry.split_once("</string>"))
            .map(|(extension, _)| extension.to_owned())
            .collect::<Vec<_>>();
        extensions.sort();

        let mut admitted = audio_extensions();
        admitted.sort();

        assert_eq!(extensions, admitted);
    }

    #[test]
    fn a_filesystem_path_cannot_bypass_the_document_grant() {
        assert!(location("/sdcard/Music/song.flac").is_err());
        assert!(location("file:///sdcard/Music/song.flac").is_err());
        assert!(location("content:song.flac").is_err());
        assert!(location("resonate-folders://root/song.flac").is_err());
    }
}
