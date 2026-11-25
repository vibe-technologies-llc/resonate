use std::{io, result};

use resonate_core::SourceId;
use resonate_eq::EqOp;
use resonate_library::LookupOp;
use resonate_lyrics::LyricOp;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum Host {
    MusicBrainz,
    CoverArtArchive,
    Commons,
    Wikidata,
    Lrclib,
    BetterLyrics,
    AutoEq,
    AcoustId,
    Shazam,
    AppleArtwork,
    AppleMusic,
    Audd,
    Deezer,
    DeezerPictures,
    Spotify,
    SpotifyPictures,
    SpotifyAccounts,
    SpotifyApi,
    SoundCloud,
    SoundCloudPictures,
    SongLink,
    NullPointer,
    ListenBrainz,
    LastFm,
    Youtube,
    TidalEmbed,
    Tidal,
    GitHub,
}

impl Host {
    pub const fn is_an_enrichment(self) -> bool {
        !matches!(
            self,
            Self::ListenBrainz | Self::LastFm | Self::Shazam | Self::Audd | Self::SpotifyAccounts
        )
    }

    pub fn base(self) -> &'static str {
        match self {
            Self::MusicBrainz => "https://musicbrainz.org/ws/2",
            Self::CoverArtArchive => "https://coverartarchive.org",
            Self::Commons => "https://commons.wikimedia.org",
            Self::Wikidata => "https://www.wikidata.org",
            Self::Lrclib => "https://lrclib.net/api",
            Self::BetterLyrics => "https://api.betterlyrics.org",
            Self::AutoEq => "https://raw.githubusercontent.com/jaakkopasanen/AutoEq/master",
            Self::AcoustId => "https://api.acoustid.org/v2",
            Self::Shazam => "https://amp.shazam.com",
            Self::AppleArtwork => "https://is1-ssl.mzstatic.com",
            Self::AppleMusic => "https://music.apple.com",
            Self::Audd => "https://api.audd.io",
            Self::Deezer => "https://api.deezer.com",
            Self::DeezerPictures => "https://cdn-images.dzcdn.net",
            Self::Spotify => "https://open.spotify.com",
            Self::SpotifyPictures => "https://i.scdn.co",
            Self::SpotifyAccounts => "https://accounts.spotify.com",
            Self::SpotifyApi => "https://api.spotify.com/v1",
            Self::SoundCloud => "https://soundcloud.com",
            Self::SoundCloudPictures => "https://i1.sndcdn.com",
            Self::NullPointer => "https://0x0.st",
            Self::SongLink => "https://song.link",
            Self::ListenBrainz => "https://api.listenbrainz.org",
            Self::LastFm => "https://ws.audioscrobbler.com/2.0/",
            Self::Youtube => "https://www.youtube.com",
            Self::TidalEmbed => "https://embed.tidal.com",
            Self::Tidal => "https://tidal.com",
            Self::GitHub => "https://api.github.com",
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

    #[error("{host:?} was not asked for {op:?}, the network being switched off")]
    Offline { host: Host, op: LookupOp },

    #[error("{host:?} was not asked for {op:?}, the connection being metered")]
    Metered { host: Host, op: LookupOp },

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

    #[error("a clip could not be signed for {host:?}")]
    Unsigned {
        host: Host,
        #[source]
        source: Box<resonate_analysis::Error>,
    },
}

impl Error {
    pub fn from_ureq(host: Host, op: LookupOp, error: ureq::Error) -> Self {
        tracing::debug!(error = %without_queries(&error.to_string()), ?host, ?op, "a request failed");

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
            Self::Offline { .. } | Self::Metered { .. } => resonate_eq::Error::Unreachable {
                provider,
                cause: switched_off(),
            },
            Self::Refused { status, .. } => resonate_eq::Error::Refused {
                provider,
                op,
                status,
            },
            Self::Unreadable { .. } | Self::TooLarge { .. } | Self::Unsigned { .. } => {
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
            Self::Offline { .. } | Self::Metered { .. } => resonate_lyrics::Error::Unreachable {
                provider,
                cause: switched_off(),
            },
            Self::Refused { status, .. } => resonate_lyrics::Error::Refused {
                provider,
                op,
                status,
            },
            Self::Unreadable { .. } | Self::TooLarge { .. } | Self::Unsigned { .. } => {
                resonate_lyrics::Error::Unreadable { provider, op }
            }
        }
    }
}

impl Error {
    pub fn into_listen_error(self, service: SourceId) -> resonate_listen::Error {
        match self {
            Self::Unreachable { .. } | Self::Offline { .. } | Self::Metered { .. } => {
                resonate_listen::Error::Unreachable { service }
            }
            Self::Refused { status, .. } => resonate_listen::Error::Refused { service, status },
            Self::Unreadable { .. } => resonate_listen::Error::Unreadable { service },
            Self::TooLarge { .. } => resonate_listen::Error::TooLarge { service },
            Self::Unsigned { .. } => resonate_listen::Error::Unsigned { service },
        }
    }
}

impl From<Error> for resonate_library::Error {
    fn from(error: Error) -> Self {
        match error {
            Error::Unreachable { op, source, .. } => Self::Unreachable { op, source },
            Error::Offline { op, .. } => Self::Offline { op },
            Error::Metered { op, .. } => Self::Metered { op },
            Error::Refused { op, status, .. } => Self::Refused { op, status },
            Error::Unreadable { op, .. } => Self::Unreadable { op },
            Error::TooLarge { op, limit, .. } => Self::TooLarge { op, limit },
            Error::Unsigned { .. } => Self::Unreadable {
                op: LookupOp::Recognise,
            },
        }
    }
}

fn without_queries(text: &str) -> String {
    const SCHEME_END: &str = "://";

    let mut written = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(at) = rest.find(SCHEME_END) {
        let (head, tail) = rest.split_at(at + SCHEME_END.len());
        let end = tail
            .find(|letter: char| letter.is_whitespace() || "\"'<>)]".contains(letter))
            .unwrap_or(tail.len());

        let (address, after) = tail.split_at(end);

        written.push_str(head);
        written.push_str(address.split(['?', '#']).next().unwrap_or(address));
        rest = after;
    }

    written.push_str(rest);
    written
}

fn switched_off() -> io::Error {
    io::Error::from(io::ErrorKind::NetworkDown)
}

pub type Result<T> = result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_stays_small_enough_for_result_large_err() {
        assert!(size_of::<Error>() <= 128, "{}", size_of::<Error>());
    }

    #[derive(Clone, Default)]
    struct Logged(std::sync::Arc<parking_lot::Mutex<Vec<u8>>>);

    impl io::Write for Logged {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl tracing_subscriber::fmt::MakeWriter<'_> for Logged {
        type Writer = Self;

        fn make_writer(&self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    fn a_failed_request_is_logged_with_no_query_of_the_address() {
        const SECRET: &str = "a-token-the-listener-registered";

        let logged = Logged::default();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_ansi(false)
            .with_writer(logged.clone())
            .finish();

        let classified = tracing::subscriber::with_default(subscriber, || {
            Error::from_ureq(
                Host::MusicBrainz,
                LookupOp::Release,
                ureq::Error::RequireHttpsOnly(format!(
                    "http://media.example/rest/ping?t={SECRET}&s=salt&api_key={SECRET}#frag"
                )),
            )
        });
        let logged = String::from_utf8(logged.0.lock().clone()).expect("logged as text");

        assert!(matches!(classified, Error::Unreachable { .. }));
        assert!(
            logged.contains("http://media.example/rest/ping"),
            "{logged}"
        );
        assert!(!logged.contains(SECRET), "{logged}");
        assert!(!logged.contains("api_key"), "{logged}");
    }

    #[test]
    fn a_query_is_cut_from_each_address_in_a_sentence() {
        assert_eq!(
            without_queries(
                "GET (https://a.example/x?k=1) then \"http://b.example/y?z=2#f\" ended"
            ),
            "GET (https://a.example/x) then \"http://b.example/y\" ended"
        );
        assert_eq!(without_queries("no address here?"), "no address here?");
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

        let refused = Error::Refused {
            host: Host::Lrclib,
            op: LookupOp::Lyrics,
            status: 429,
        };

        assert!(matches!(
            refused.into_lyric_error(provider.clone(), LyricOp::Fetch),
            resonate_lyrics::Error::Refused { status: 429, .. }
        ));

        let refused = Error::Refused {
            host: Host::AutoEq,
            op: LookupOp::Correction,
            status: 429,
        };

        assert!(matches!(
            refused.into_eq_error(provider.clone(), EqOp::Index),
            resonate_eq::Error::Refused {
                op: EqOp::Index,
                status: 429,
                ..
            }
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

    #[test]
    fn a_shut_gate_and_a_switched_off_network_reach_the_library_as_themselves_not_unreached() {
        let metered = Error::Metered {
            host: Host::MusicBrainz,
            op: LookupOp::Release,
        };
        let offline = Error::Offline {
            host: Host::MusicBrainz,
            op: LookupOp::Artist,
        };

        assert!(matches!(
            resonate_library::Error::from(metered),
            resonate_library::Error::Metered {
                op: LookupOp::Release
            }
        ));
        assert!(matches!(
            resonate_library::Error::from(offline),
            resonate_library::Error::Offline {
                op: LookupOp::Artist
            }
        ));
    }
}
