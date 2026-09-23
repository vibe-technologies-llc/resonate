use std::{fmt, io::Read, path::PathBuf};

use resonate_core::{MediaLocation, SourceId};

use crate::{Error, Result};

const LONGEST_EXTENSION: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Extension(Box<str>);

impl Extension {
    pub fn new(text: &str) -> Result<Self> {
        let bare = text.strip_prefix('.').unwrap_or(text);
        if bare.is_empty()
            || bare.len() > LONGEST_EXTENSION
            || !bare.bytes().all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err(Error::NotAnExtension);
        }
        Ok(Self(bare.to_ascii_lowercase().into_boxed_str()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Extension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub enum Delivery {
    File(PathBuf),
    Stream {
        key: Box<str>,
        extension: Extension,
        reader: Box<dyn Read + Send>,
    },
}

impl fmt::Debug for Delivery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File(path) => f.debug_tuple("File").field(path).finish(),
            Self::Stream { key, extension, .. } => f
                .debug_struct("Stream")
                .field("key", key)
                .field("extension", extension)
                .finish_non_exhaustive(),
        }
    }
}

#[derive(Debug)]
pub enum Obtained {
    Nothing,
    Found(Delivery),
}

#[derive(Debug)]
pub struct Delivered {
    pub provider: SourceId,
    pub delivery: Delivery,
}

impl Delivered {
    pub fn taken_from(&self) -> MediaLocation {
        match &self.delivery {
            Delivery::File(path) => MediaLocation::local(path),
            Delivery::Stream { key, .. } => MediaLocation::new(self.provider.clone(), key.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_extension_is_read_without_its_dot_and_in_lowercase() {
        assert_eq!(
            Extension::new(".FLAC").expect("an extension").as_str(),
            "flac"
        );
        assert_eq!(Extension::new("m4a").expect("an extension").as_str(), "m4a");
    }

    #[test]
    fn an_extension_that_could_name_a_path_is_refused() {
        for refused in ["", ".", "../wav", "fl ac", "a/b", "waytoolong", "wav\0"] {
            assert!(
                matches!(Extension::new(refused), Err(Error::NotAnExtension)),
                "{refused:?} was taken for an extension"
            );
        }
    }

    #[test]
    fn a_stream_is_taken_from_the_provider_under_its_own_key() {
        let provider = SourceId::new("shop").expect("a nameable source");
        let delivered = Delivered {
            provider,
            delivery: Delivery::Stream {
                key: "track/42".into(),
                extension: Extension::new("flac").expect("an extension"),
                reader: Box::new(std::io::empty()),
            },
        };
        assert_eq!(delivered.taken_from().to_uri(), "shop:track/42");
    }
}
