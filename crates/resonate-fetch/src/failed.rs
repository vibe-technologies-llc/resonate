use std::io::{self, ErrorKind};

use resonate_core::SourceId;
use resonate_providers::{Error, ProviderOp};

use crate::trust;

enum Failure {
    Unreached(io::Error),
    Untrusted,
    Unaddressable,
    Refused(u16),
    Unreadable,
}

fn classed(error: ureq::Error) -> Failure {
    if trust::certificate_refused(&error) {
        return Failure::Untrusted;
    }
    let unreached = |kind: ErrorKind| Failure::Unreached(io::Error::from(kind));
    match error {
        ureq::Error::StatusCode(status) => Failure::Refused(status),
        ureq::Error::Io(source) => Failure::Unreached(source),
        ureq::Error::Timeout(_) | ureq::Error::BodyStalled => unreached(ErrorKind::TimedOut),
        ureq::Error::HostNotFound => unreached(ErrorKind::NotFound),
        ureq::Error::ConnectionFailed
        | ureq::Error::ConnectProxyFailed(_)
        | ureq::Error::InvalidProxyUrl
        | ureq::Error::Tls(_)
        | ureq::Error::Pem(_)
        | ureq::Error::Rustls(_)
        | ureq::Error::TlsRequired
        | ureq::Error::RequireHttpsOnly(_) => unreached(ErrorKind::ConnectionRefused),
        ureq::Error::BadUri(_) | ureq::Error::Http(_) => Failure::Unaddressable,
        _ => Failure::Unreadable,
    }
}

pub fn unreached(provider: SourceId, op: ProviderOp, error: ureq::Error) -> Error {
    match classed(error) {
        Failure::Unreached(source) => Error::Io {
            provider,
            op,
            source,
        },
        Failure::Untrusted => Error::Untrusted { provider, op },
        Failure::Unaddressable => Error::Unaddressable { provider, op },
        Failure::Refused(status) => Error::Refused {
            provider,
            op,
            status,
        },
        Failure::Unreadable => Error::Unreadable { provider, op },
    }
}

pub fn as_io(error: ureq::Error) -> io::Error {
    match classed(error) {
        Failure::Unreached(source) => source,
        Failure::Untrusted => io::Error::from(ErrorKind::PermissionDenied),
        Failure::Unaddressable => io::Error::from(ErrorKind::InvalidInput),
        Failure::Refused(_) => io::Error::from(ErrorKind::ConnectionAborted),
        Failure::Unreadable => io::Error::from(ErrorKind::InvalidData),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> SourceId {
        SourceId::new("shop").expect("a nameable source")
    }

    #[test]
    fn a_server_that_cannot_be_reached_is_the_provider_away() {
        let refused = unreached(
            provider(),
            ProviderOp::Search,
            ureq::Error::Io(io::Error::from(ErrorKind::ConnectionRefused)),
        );
        let unknown = unreached(provider(), ProviderOp::Search, ureq::Error::HostNotFound);

        assert!(matches!(refused, Error::Io { .. }));
        assert!(refused.is_the_provider_away());
        assert!(unknown.is_the_provider_away());
    }

    #[test]
    fn a_certificate_the_client_does_not_trust_is_told_apart_from_a_refused_connection() {
        let refused_certificate = ureq::Error::Rustls(rustls::Error::InvalidCertificate(
            rustls::CertificateError::UnknownIssuer,
        ));

        let untrusted = unreached(provider(), ProviderOp::Search, refused_certificate);

        assert!(matches!(
            untrusted,
            Error::Untrusted {
                op: ProviderOp::Search,
                ..
            }
        ));
        assert!(untrusted.is_the_provider_away());
    }

    #[test]
    fn an_answer_too_large_or_malformed_is_unreadable_and_no_refused_connection() {
        let too_large = unreached(
            provider(),
            ProviderOp::Search,
            ureq::Error::BodyExceedsLimit(4),
        );
        let redirected = unreached(
            provider(),
            ProviderOp::Search,
            ureq::Error::TooManyRedirects,
        );

        assert!(matches!(too_large, Error::Unreadable { .. }));
        assert!(!too_large.is_the_provider_away());
        assert!(matches!(redirected, Error::Unreadable { .. }));
        assert_eq!(
            as_io(ureq::Error::BodyExceedsLimit(4)).kind(),
            ErrorKind::InvalidData
        );
    }

    #[test]
    fn an_address_that_does_not_read_is_the_provider_away_and_says_so() {
        let unaddressable = unreached(
            provider(),
            ProviderOp::Search,
            ureq::Error::BadUri("music.local/rest".to_owned()),
        );

        assert!(matches!(unaddressable, Error::Unaddressable { .. }));
        assert!(unaddressable.is_the_provider_away());
    }
}
