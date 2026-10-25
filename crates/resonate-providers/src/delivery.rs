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

pub enum Opened {
    Reading {
        reader: Box<dyn Read + Send>,
        length: Option<u64>,
    },
    Gone,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Validator(Box<str>);

impl Validator {
    pub fn new(text: &str) -> Option<Self> {
        let text = text.trim();
        (!text.is_empty() && !text.contains(['\r', '\n'])).then(|| Self(text.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_weak(&self) -> bool {
        self.0.starts_with("W/")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    pub bytes: u64,
    pub total: Option<u64>,
    pub validator: Option<Validator>,
}

impl Held {
    pub fn can_be_checked(&self) -> bool {
        self.bytes > 0
            && (self.total.is_some() || self.validator.as_ref().is_some_and(|v| !v.is_weak()))
    }
}

pub struct Resumed {
    pub reader: Box<dyn Read + Send>,
    pub length: Option<u64>,
    pub from: u64,
    pub validator: Option<Validator>,
}

pub enum Reopened {
    Reading(Resumed),
    Gone,
}

type Whole = Box<dyn FnOnce() -> Result<Opened> + Send>;
type Resuming = Box<dyn FnOnce(Option<Held>) -> Result<Reopened> + Send>;

enum Open {
    Whole(Whole),
    Resumable(Resuming),
}

pub struct Opening(Open);

impl Opening {
    pub fn new(open: impl FnOnce() -> Result<Opened> + Send + 'static) -> Self {
        Self(Open::Whole(Box::new(open)))
    }

    pub fn resumable(open: impl FnOnce(Option<Held>) -> Result<Reopened> + Send + 'static) -> Self {
        Self(Open::Resumable(Box::new(open)))
    }

    pub fn ready(reader: impl Read + Send + 'static) -> Self {
        Self::new(move || {
            Ok(Opened::Reading {
                reader: Box::new(reader),
                length: None,
            })
        })
    }

    pub fn is_resumable(&self) -> bool {
        matches!(self.0, Open::Resumable(_))
    }

    pub fn open(self) -> Result<Opened> {
        match self.0 {
            Open::Whole(open) => open(),
            Open::Resumable(open) => Ok(match open(None)? {
                Reopened::Reading(resumed) => Opened::Reading {
                    reader: resumed.reader,
                    length: resumed.length,
                },
                Reopened::Gone => Opened::Gone,
            }),
        }
    }

    pub fn open_resuming(self, held: Option<Held>) -> Result<Reopened> {
        match self.0 {
            Open::Resumable(open) => open(held),
            Open::Whole(open) => Ok(match open()? {
                Opened::Reading { reader, length } => Reopened::Reading(Resumed {
                    reader,
                    length,
                    from: 0,
                    validator: None,
                }),
                Opened::Gone => Reopened::Gone,
            }),
        }
    }
}

pub enum Delivery {
    File(PathBuf),
    Stream {
        key: Box<str>,
        extension: Extension,
        opening: Opening,
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
                opening: Opening::ready(std::io::empty()),
            },
        };
        assert_eq!(delivered.taken_from().to_uri(), "shop:track/42");
    }

    #[test]
    fn a_whole_opening_is_opened_from_the_start_whatever_is_held() {
        let opening = Opening::ready(std::io::Cursor::new(b"abc".to_vec()));
        let held = Held {
            bytes: 2,
            total: Some(3),
            validator: None,
        };

        assert!(!opening.is_resumable());
        let Ok(Reopened::Reading(resumed)) = opening.open_resuming(Some(held)) else {
            panic!("an opening reads");
        };
        assert_eq!(resumed.from, 0);
        assert_eq!(resumed.validator, None);
    }

    #[test]
    fn a_resumable_opening_is_asked_what_is_held_and_opens_whole_without_it() {
        let asked = |held: Option<Held>| {
            Opening::resumable(move |held| {
                Ok(Reopened::Reading(Resumed {
                    reader: Box::new(std::io::empty()),
                    length: held.as_ref().and_then(|held| held.total),
                    from: held.as_ref().map_or(0, |held| held.bytes),
                    validator: None,
                }))
            })
            .open_resuming(held)
        };
        let held = Held {
            bytes: 5,
            total: Some(9),
            validator: None,
        };

        let Ok(Reopened::Reading(resumed)) = asked(Some(held)) else {
            panic!("an opening reads");
        };
        let Ok(Opened::Reading { length, .. }) = Opening::resumable(|_| {
            Ok(Reopened::Reading(Resumed {
                reader: Box::new(std::io::empty()),
                length: Some(4),
                from: 0,
                validator: None,
            }))
        })
        .open() else {
            panic!("an opening reads");
        };

        assert_eq!((resumed.from, resumed.length), (5, Some(9)));
        assert_eq!(length, Some(4));
    }

    #[test]
    fn a_partial_can_be_checked_only_by_a_length_or_a_strong_validator() {
        let held = |total, validator: Option<&str>| Held {
            bytes: 10,
            total,
            validator: validator.and_then(Validator::new),
        };

        assert!(held(Some(100), None).can_be_checked());
        assert!(held(None, Some("\"abc\"")).can_be_checked());
        assert!(!held(None, Some("W/\"abc\"")).can_be_checked());
        assert!(!held(None, None).can_be_checked());
        assert!(
            !Held {
                bytes: 0,
                ..held(Some(100), None)
            }
            .can_be_checked()
        );
    }
}
