use std::{io, result};

use resonate_core::SourceId;
use resonate_eq::EqOp;
use resonate_library::LookupOp;
use resonate_lyrics::LyricOp;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Host {
    MusicBrainz,
    CoverArtArchive,
    Commons,
    Wikidata,
    Lrclib,
    AutoEq,
    AcoustId,
    Shazam,
    AppleArtwork,
    AppleMusic,
    Audd,
    Deezer,
    DeezerPictures,
}

impl Host {
    pub fn base(self) -> &'static str {
        match self {
            Self::MusicBrainz => "https://musicbrainz.org/ws/2",
            Self::CoverArtArchive => "https://coverartarchive.org",
            Self::Commons => "https://commons.wikimedia.org",
            Self::Wikidata => "https://www.wikidata.org",
            Self::Lrclib => "https://lrclib.net/api",
            Self::AutoEq => "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master",
            Self::AcoustId => "https://api.acoustid.org/v2",
            Self::Shazam => "https://amp.shazam.com",
            Self::AppleArtwork => "https://is1-ssl.mzstatic.com",
            Self::AppleMusic => "https://music.apple.com",
            Self::Audd => "https://api.audd.io",
            Self::Deezer => "https://api.deezer.com",
            Self::DeezerPictures => "https://cdn-images.dzcdn.net",
        }
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("{host:?} could not be reached for {op:?}")]
    Unreachable {
        host: Host,
        op: LookupOp,
        #[source]
        source: io::Error,
    },

    #[error("{host:?} refused {op:?} with status {status}")]
    Refused {
        host: Host,
        op: LookupOp,
        status: u16,
    },

    #[error("{host:?} answered {op:?} with something this build cannot read")]
    Unreadable { host: Host, op: LookupOp },

    #[error("{host:?} answered {op:?} with more than the {limit} bytes this build will read")]
    TooLarge {
        host: Host,
        op: LookupOp,
        limit: usize,
    },
}

impl Error {
    pub fn from_ureq(host: Host, op: LookupOp, error: ureq::Error) -> Self {
        tracing::debug!(%error, ?host, ?op, "a request failed");

        let unreachable = |kind: io::ErrorKind| Self::Unreachable {
            host,
            op,
            source: io::Error::from(kind),
        };

        match error {
            ureq::Error::StatusCode(status) => Self::Refused { host, op, status },
            ureq::Error::Io(source) => Self::Unreachable { host, op, source },
            ureq::Error::Timeout(_) => unreachable(io::ErrorKind::TimedOut),
            ureq::Error::HostNotFound => unreachable(io::ErrorKind::NotFound),
            ureq::Error::ConnectionFailed
            | ureq::Error::ConnectProxyFailed(_)
            | ureq::Error::InvalidProxyUrl
            | ureq::Error::Tls(_)
            | ureq::Error::Pem(_)
            | ureq::Error::Rustls(_)
            | ureq::Error::TlsRequired
            | ureq::Error::RequireHttpsOnly(_) => unreachable(io::ErrorKind::ConnectionRefused),
            ureq::Error::BodyExceedsLimit(limit) => Self::TooLarge {
                host,
                op,
                limit: usize::try_from(limit).unwrap_or(usize::MAX),
            },
            _ => Self::Unreadable { host, op },
        }
    }

    pub fn into_eq_error(self, provider: SourceId, op: EqOp) -> resonate_eq::Error {
        match self {
            Self::Unreachable { source, .. } => resonate_eq::Error::Unreachable {
                provider,
                cause: source,
            },
            Self::Refused { .. } | Self::Unreadable { .. } | Self::TooLarge { .. } => {
                resonate_eq::Error::Unreadable { provider, op }
            }
        }
    }

    pub fn into_lyric_error(self, provider: SourceId, op: LyricOp) -> resonate_lyrics::Error {
        match self {
            Self::Unreachable { source, .. } => resonate_lyrics::Error::Unreachable {
                provider,
                cause: source,
            },
            Self::Refused { .. } | Self::Unreadable { .. } | Self::TooLarge { .. } => {
                resonate_lyrics::Error::Unreadable { provider, op }
            }
        }
    }
}

impl Error {
    pub fn into_listen_error(self, service: SourceId) -> resonate_listen::Error {
        match self {
            Self::Unreachable { .. } => resonate_listen::Error::Unreachable { service },
            Self::Refused { status, .. } => resonate_listen::Error::Refused { service, status },
            Self::Unreadable { .. } => resonate_listen::Error::Unreadable { service },
            Self::TooLarge { .. } => resonate_listen::Error::TooLarge { service },
        }
    }
}

impl From<Error> for resonate_library::Error {
    fn from(error: Error) -> Self {
        match error {
            Error::Unreachable { op, source, .. } => Self::Unreachable { op, source },
            Error::Refused { op, status, .. } => Self::Refused { op, status },
            Error::Unreadable { op, .. } | Error::TooLarge { op, .. } => Self::Unreadable { op },
        }
    }
}

pub type Result<T> = result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_stays_small_enough_for_result_large_err() {
        assert!(size_of::<Error>() <= 128, "{}", size_of::<Error>());
    }

    #[test]
    fn a_ureq_failure_is_classified_once_into_the_four_shapes() {
        let classified = |error| Error::from_ureq(Host::MusicBrainz, LookupOp::Release, error);

        assert!(matches!(
            classified(ureq::Error::StatusCode(503)),
            Error::Refused { status: 503, .. }
        ));
        assert!(matches!(
            classified(ureq::Error::Io(io::Error::from(io::ErrorKind::BrokenPipe))),
            Error::Unreachable { source, .. } if source.kind() == io::ErrorKind::BrokenPipe
        ));
        assert!(matches!(
            classified(ureq::Error::Timeout(ureq::Timeout::Connect)),
            Error::Unreachable { source, .. } if source.kind() == io::ErrorKind::TimedOut
        ));
        assert!(matches!(
            classified(ureq::Error::HostNotFound),
            Error::Unreachable { source, .. } if source.kind() == io::ErrorKind::NotFound
        ));
        assert!(matches!(
            classified(ureq::Error::ConnectionFailed),
            Error::Unreachable { source, .. } if source.kind() == io::ErrorKind::ConnectionRefused
        ));
        assert!(matches!(
            classified(ureq::Error::BodyExceedsLimit(4096)),
            Error::TooLarge { limit: 4096, .. }
        ));
        assert!(matches!(
            classified(ureq::Error::TooManyRedirects),
            Error::Unreadable { .. }
        ));
    }

    #[test]
    fn the_library_and_the_lyric_crate_each_read_the_error_in_their_own_vocabulary() {
        let provider = SourceId::new("lrclib").expect("a lowercase name");

        let refused = Error::Refused {
            host: Host::Lrclib,
            op: LookupOp::Lyrics,
            status: 429,
        };
        assert!(matches!(
            resonate_library::Error::from(refused),
            resonate_library::Error::Refused {
                op: LookupOp::Lyrics,
                status: 429
            }
        ));

        let unreachable = Error::Unreachable {
            host: Host::Lrclib,
            op: LookupOp::Lyrics,
            source: io::Error::from(io::ErrorKind::TimedOut),
        };
        assert!(matches!(
            unreachable.into_lyric_error(provider.clone(), LyricOp::Fetch),
            resonate_lyrics::Error::Unreachable { cause, .. } if cause.kind() == io::ErrorKind::TimedOut
        ));

        let too_large = Error::TooLarge {
            host: Host::Lrclib,
            op: LookupOp::Lyrics,
            limit: 1,
        };
        assert!(matches!(
            too_large.into_lyric_error(provider, LyricOp::Search),
            resonate_lyrics::Error::Unreadable {
                op: LyricOp::Search,
                ..
            }
        ));
    }
}
