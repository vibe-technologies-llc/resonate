use std::{fs::File, io, os::fd::FromRawFd, sync::Arc};

use resonate_codec::{FormatHint, Media, MediaProvider, Reading};
use resonate_core::{Locator, MediaLocation, SourceId};

use crate::{Error, Result};

#[uniffi::export(callback_interface)]
pub trait Documents: Send + Sync {
    fn open(&self, uri: String) -> i32;
}

pub(crate) struct GrantedFiles {
    pub(crate) documents: Arc<dyn Documents>,
    source: SourceId,
}

impl GrantedFiles {
    pub(crate) fn new(documents: Arc<dyn Documents>) -> Result<Self> {
        Ok(Self {
            documents,
            source: SourceId::new("content")?,
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
        let descriptor = self.documents.open(format!("content:{key}"));
        if descriptor < 0 {
            return Err(resonate_codec::Error::Io {
                location: location.clone(),
                source: io::ErrorKind::PermissionDenied.into(),
            });
        }
        let file = unsafe { File::from_raw_fd(descriptor) };
        Ok(Media {
            stream: Box::new(Reading::new(file)),
            hint: location
                .extension()
                .map(|extension| FormatHint::Extension(extension.into())),
        })
    }
}

pub(crate) fn location(uri: &str) -> Result<MediaLocation> {
    if !uri.starts_with("content://") {
        return Err(Error::InvalidUri);
    }
    Ok(MediaLocation::new(
        SourceId::new("content")?,
        &uri["content:".len()..],
    ))
}

pub(crate) fn document_uri(location: &MediaLocation) -> Result<String> {
    match location.locator() {
        Locator::Key(key) if location.source().as_str() == "content" => {
            Ok(format!("content:{key}"))
        }
        _ => Err(Error::InvalidUri),
    }
}

#[cfg(test)]
mod tests {
    use parking_lot::Mutex;

    use super::*;

    struct Refused(Mutex<Vec<String>>);

    impl Documents for Refused {
        fn open(&self, uri: String) -> i32 {
            self.0.lock().push(uri);
            -1
        }
    }

    #[test]
    fn a_grant_keeps_its_escaping_through_the_media_registry_and_saved_queue() {
        let uri =
            "content://documents/tree/Music%3A%2FAlbum/document/Music%3A%2FAlbum%2F100%25.flac";
        let original = location(uri).expect("a content URI");
        let restored = MediaLocation::from_uri(&original.to_uri()).expect("the saved location");
        assert_eq!(original, restored);
        assert_eq!(
            document_uri(&restored).expect("the original document key"),
            uri
        );
        let callback = Arc::new(Refused(Mutex::new(Vec::new())));
        let provider = GrantedFiles::new(callback.clone()).expect("a content provider");
        assert!(provider.open(&restored).is_err());
        assert_eq!(callback.0.lock().as_slice(), [uri]);
    }

    #[test]
    fn a_filesystem_path_cannot_bypass_the_document_grant() {
        assert!(location("/sdcard/Music/song.flac").is_err());
        assert!(location("file:///sdcard/Music/song.flac").is_err());
    }
}
