use std::{fmt, io, result};

use lofty::error::{FileEncodingError, FileParseError};
use resonate_core::{ChannelCount, Frames, MediaLocation};
use symphonia::core::{
    audio::sample::SampleFormat as SymphoniaSampleFormat,
    codecs::audio::AudioCodecId,
    errors::{self, SeekErrorKind},
};
use thiserror::Error;

use crate::dsd::{DsdChunk, DsdField};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StreamTrackId(pub u32);

impl fmt::Display for StreamTrackId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TrackProperty {
    SampleRate,
    ChannelLayout,
}

impl fmt::Display for TrackProperty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::SampleRate => "sample rate",
            Self::ChannelLayout => "channel layout",
        };
        f.write_str(name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CodecOp {
    Probe,
    ReadPacket,
    Decode,
    Seek,
    Reset,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("cannot read {location}")]
    Io {
        location: MediaLocation,
        #[source]
        source: io::Error,
    },

    #[error("{location} is not a recognised container")]
    UnrecognisedContainer { location: MediaLocation },

    #[error("{location} contains no audio track")]
    NoAudioTrack { location: MediaLocation },

    #[error("{location} track {track} uses codec {codec}, for which no decoder is registered")]
    NoDecoder {
        location: MediaLocation,
        track: StreamTrackId,
        codec: AudioCodecId,
    },

    #[error("{location} track {track} declares no {property}")]
    TrackPropertyMissing {
        location: MediaLocation,
        track: StreamTrackId,
        property: TrackProperty,
    },

    #[error("{location} track {track} declares {rate} Hz")]
    RateNotRepresentable {
        location: MediaLocation,
        track: StreamTrackId,
        rate: u32,
    },

    #[error(
        "{location} track {track} declares {channels} channels in a layout resonate cannot map"
    )]
    LayoutNotRepresentable {
        location: MediaLocation,
        track: StreamTrackId,
        channels: ChannelCount,
    },

    #[error("{location} track {track} decodes to {actual:?}, which resonate cannot represent")]
    SampleFormatNotRepresentable {
        location: MediaLocation,
        track: StreamTrackId,
        actual: SymphoniaSampleFormat,
    },

    #[error("{location} track {track} declares no duration and cannot be seeked")]
    UnknownDuration {
        location: MediaLocation,
        track: StreamTrackId,
    },

    #[error("seek to {requested} is past the track duration of {duration}")]
    SeekOutOfRange { requested: Frames, duration: Frames },

    #[error("{location} is not seekable")]
    NotSeekable { location: MediaLocation },

    #[error("{location} can only be seeked forward")]
    SeekBackwardUnsupported { location: MediaLocation },

    #[error("{location} names an invalid track for seeking")]
    SeekInvalidTrack { location: MediaLocation },

    #[error("the decoder for {location} must be reset before decoding continues")]
    ResetRequired { location: MediaLocation },

    #[error("{op:?} failed for {location}")]
    Symphonia {
        op: CodecOp,
        location: MediaLocation,
        #[source]
        source: Box<errors::Error>,
    },

    #[error("{location} declares no {chunk} chunk")]
    DsdChunkMissing {
        location: MediaLocation,
        chunk: DsdChunk,
    },

    #[error("{location} declares a {field} of {value}, which resonate cannot read")]
    DsdFieldNotUsable {
        location: MediaLocation,
        field: DsdField,
        value: u64,
    },

    #[error(
        "{location} is compressed with {}, for which there is no decoder",
        String::from_utf8_lossy(compression)
    )]
    DsdCompressed {
        location: MediaLocation,
        compression: [u8; 4],
    },

    #[error("{location} declares more than {limit} bytes of cue sheet")]
    SheetTooLarge { location: MediaLocation, limit: u64 },

    #[error("nothing in this build answers for the source that names {location}")]
    NoSuchSource { location: MediaLocation },

    #[error("{location} is addressed in a way its source cannot open")]
    LocatorNotUsable { location: MediaLocation },

    #[error("{location} is in a format this build does not write tags to")]
    Unwritable { location: MediaLocation },

    #[error("the tags in {location} could not be read to be written")]
    TagsUnread {
        location: MediaLocation,
        #[source]
        source: FileParseError,
    },

    #[error("the tags in {location} could not be written")]
    TagsUnwritten {
        location: MediaLocation,
        #[source]
        source: FileEncodingError,
    },

    #[error(transparent)]
    Domain(#[from] resonate_core::Error),
}

impl Error {
    pub(crate) fn from_symphonia(
        source: errors::Error,
        op: CodecOp,
        location: &MediaLocation,
    ) -> Self {
        let location = location.clone();
        match source {
            errors::Error::IoError(source) => Self::Io { location, source },
            errors::Error::SeekError(SeekErrorKind::Unseekable) => Self::NotSeekable { location },
            errors::Error::SeekError(SeekErrorKind::ForwardOnly) => {
                Self::SeekBackwardUnsupported { location }
            }
            errors::Error::SeekError(SeekErrorKind::InvalidTrack) => {
                Self::SeekInvalidTrack { location }
            }
            errors::Error::ResetRequired => Self::ResetRequired { location },
            source => Self::Symphonia {
                op,
                location,
                source: Box::new(source),
            },
        }
    }
}

pub type Result<T> = result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    fn located() -> MediaLocation {
        MediaLocation::local("/music/a.flac")
    }

    #[test]
    fn error_stays_small_enough_for_result_large_err() {
        assert!(size_of::<Error>() <= 128, "{}", size_of::<Error>());
    }

    #[test]
    fn seek_failures_are_lifted_out_of_the_catch_all() {
        let lifted = Error::from_symphonia(
            errors::Error::SeekError(SeekErrorKind::Unseekable),
            CodecOp::Seek,
            &located(),
        );
        assert!(matches!(lifted, Error::NotSeekable { .. }));

        let lifted =
            Error::from_symphonia(errors::Error::ResetRequired, CodecOp::Decode, &located());
        assert!(matches!(lifted, Error::ResetRequired { .. }));
    }

    #[test]
    fn io_failures_carry_the_location_and_the_kind() {
        let location = MediaLocation::local("/music/missing.flac");
        let lifted = Error::from_symphonia(
            errors::Error::IoError(io::Error::from(io::ErrorKind::NotFound)),
            CodecOp::Probe,
            &location,
        );
        match lifted {
            Error::Io {
                location: named,
                source,
            } => {
                assert_eq!(named, location);
                assert_eq!(source.kind(), io::ErrorKind::NotFound);
            }
            other => panic!("expected Io, got {other:?}"),
        }
    }

    #[test]
    fn only_unclassifiable_failures_reach_the_residual() {
        let lifted = Error::from_symphonia(
            errors::Error::DecodeError("bad frame header"),
            CodecOp::Decode,
            &located(),
        );
        assert!(matches!(
            lifted,
            Error::Symphonia {
                op: CodecOp::Decode,
                ..
            }
        ));
    }

    #[test]
    fn a_source_nothing_answers_for_names_the_media_it_refused() {
        let location = MediaLocation::new(
            resonate_core::SourceId::new("subsonic").expect("a lowercase name"),
            "track/1",
        );
        let refused = Error::NoSuchSource {
            location: location.clone(),
        };

        assert!(refused.to_string().contains(&location.to_string()));
    }
}
