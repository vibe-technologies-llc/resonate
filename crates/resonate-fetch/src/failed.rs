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
    use ureq::Timeout;

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

    fn untrusted() -> ureq::Error {
        ureq::Error::Rustls(rustls::Error::InvalidCertificate(
            rustls::CertificateError::UnknownIssuer,
        ))
    }

    fn io_kind_of(error: Error) -> ErrorKind {
        let Error::Io { source, .. } = error else {
            panic!("an I/O failure of the provider");
        };

        source.kind()
    }

    #[test]
    fn a_request_that_ran_out_of_time_in_any_phase_is_the_provider_away_timed_out() {
        for phase in [
            Timeout::Global,
            Timeout::PerCall,
            Timeout::Resolve,
            Timeout::Connect,
            Timeout::SendRequest,
            Timeout::PerRead,
            Timeout::PerWrite,
            Timeout::SendBody,
            Timeout::RecvResponse,
            Timeout::RecvBody,
        ] {
            let timed_out = unreached(provider(), ProviderOp::Search, ureq::Error::Timeout(phase));

            assert!(timed_out.is_the_provider_away(), "{phase:?}");
            assert_eq!(io_kind_of(timed_out), ErrorKind::TimedOut, "{phase:?}");
            assert_eq!(
                as_io(ureq::Error::Timeout(phase)).kind(),
                ErrorKind::TimedOut,
                "{phase:?}"
            );
        }
    }

    #[test]
    fn a_body_that_stalled_is_the_provider_away_timed_out() {
        let stalled = unreached(provider(), ProviderOp::Search, ureq::Error::BodyStalled);

        assert!(stalled.is_the_provider_away());
        assert_eq!(io_kind_of(stalled), ErrorKind::TimedOut);
        assert_eq!(as_io(ureq::Error::BodyStalled).kind(), ErrorKind::TimedOut);
    }

    #[test]
    fn a_certificate_the_client_refuses_is_permission_denied_to_a_reader() {
        let in_the_handshake = ureq::Error::Io(io::Error::new(
            ErrorKind::InvalidData,
            rustls::Error::InvalidCertificate(rustls::CertificateError::Expired),
        ));

        assert_eq!(as_io(untrusted()).kind(), ErrorKind::PermissionDenied);
        assert_eq!(as_io(in_the_handshake).kind(), ErrorKind::PermissionDenied);
    }

    #[test]
    fn a_tls_failure_that_is_no_refused_certificate_is_a_refused_connection_to_a_reader() {
        let handshake = ureq::Error::Rustls(rustls::Error::HandshakeNotComplete);

        assert_eq!(as_io(handshake).kind(), ErrorKind::ConnectionRefused);
        assert_eq!(
            as_io(ureq::Error::Tls("no protocol in common")).kind(),
            ErrorKind::ConnectionRefused
        );
    }

    #[test]
    fn a_failure_to_connect_is_a_refused_connection_to_a_reader() {
        for error in [
            ureq::Error::ConnectionFailed,
            ureq::Error::InvalidProxyUrl,
            ureq::Error::TlsRequired,
            ureq::Error::RequireHttpsOnly("http://music.local/rest".to_owned()),
        ] {
            let described = error.to_string();

            assert_eq!(
                as_io(error).kind(),
                ErrorKind::ConnectionRefused,
                "{described}"
            );
        }
    }

    #[test]
    fn a_host_that_is_not_found_is_not_found_to_a_reader() {
        assert_eq!(as_io(ureq::Error::HostNotFound).kind(), ErrorKind::NotFound);
    }

    #[test]
    fn an_address_that_does_not_read_is_invalid_input_to_a_reader() {
        let kind = as_io(ureq::Error::BadUri("music.local/rest".to_owned())).kind();

        assert_eq!(kind, ErrorKind::InvalidInput);
    }

    #[test]
    fn a_status_the_client_was_told_to_treat_as_an_error_is_refused_and_aborts_a_reader() {
        let refused = unreached(provider(), ProviderOp::Search, ureq::Error::StatusCode(503));

        assert!(matches!(refused, Error::Refused { status: 503, .. }));
        assert_eq!(
            as_io(ureq::Error::StatusCode(503)).kind(),
            ErrorKind::ConnectionAborted
        );
    }

    #[test]
    fn a_transport_failure_already_an_io_error_keeps_its_own_kind_for_a_reader() {
        for kind in [
            ErrorKind::ConnectionReset,
            ErrorKind::UnexpectedEof,
            ErrorKind::BrokenPipe,
        ] {
            let failed = ureq::Error::Io(io::Error::from(kind));

            assert_eq!(as_io(failed).kind(), kind);
        }
    }

    #[test]
    fn a_response_that_cannot_be_read_keeps_a_class_apart_from_every_transport_failure() {
        for error in [
            ureq::Error::BodyExceedsLimit(4),
            ureq::Error::TooManyRedirects,
            ureq::Error::RedirectFailed,
            ureq::Error::LargeResponseHeader(2, 1),
        ] {
            let described = error.to_string();
            let unreadable = unreached(provider(), ProviderOp::Search, error);

            assert!(
                matches!(unreadable, Error::Unreadable { .. }),
                "{described}"
            );
            assert!(!unreadable.is_the_provider_away(), "{described}");
        }

        assert_eq!(
            as_io(ureq::Error::LargeResponseHeader(2, 1)).kind(),
            ErrorKind::InvalidData
        );
        assert_eq!(
            as_io(ureq::Error::TooManyRedirects).kind(),
            ErrorKind::InvalidData
        );
    }
}
