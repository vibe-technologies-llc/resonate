use std::sync::{Arc, LazyLock};

use ureq::tls::{Certificate, RootCerts, TlsConfig};

static SYSTEM_AND_BUILT_IN: LazyLock<RootCerts> = LazyLock::new(|| {
    let system = rustls_native_certs::load_native_certs();
    for error in &system.errors {
        tracing::debug!(%error, "a certificate in the system's store could not be read");
    }
    let built_in = webpki_root_certs::TLS_SERVER_ROOT_CERTS
        .iter()
        .map(|der| Certificate::from_der(der.as_ref()));
    let held = system
        .certs
        .iter()
        .map(|der| Certificate::from_der(der.as_ref()).to_owned());
    RootCerts::Specific(Arc::new(built_in.chain(held).collect()))
});

pub fn system_and_built_in() -> TlsConfig {
    TlsConfig::builder()
        .root_certs(SYSTEM_AND_BUILT_IN.clone())
        .build()
}

pub fn certificate_refused(error: &ureq::Error) -> bool {
    let refused = match error {
        ureq::Error::Rustls(error) => Some(error),
        ureq::Error::Io(error) => error
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<rustls::Error>()),
        _ => None,
    };
    matches!(refused, Some(rustls::Error::InvalidCertificate(_)))
}

#[cfg(test)]
mod tests {
    use std::io;

    use rustls::CertificateError;

    use super::*;

    #[test]
    fn the_built_in_roots_are_trusted_beside_the_systems() {
        let RootCerts::Specific(trusted) = &*SYSTEM_AND_BUILT_IN else {
            panic!("the roots are named one by one");
        };

        assert!(trusted.len() >= webpki_root_certs::TLS_SERVER_ROOT_CERTS.len());
    }

    #[test]
    fn a_certificate_the_handshake_refused_is_told_from_any_other_failure() {
        let unknown_issuer = rustls::Error::InvalidCertificate(CertificateError::UnknownIssuer);
        let in_the_handshake = ureq::Error::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            unknown_issuer.clone(),
        ));

        assert!(certificate_refused(&in_the_handshake));
        assert!(certificate_refused(&ureq::Error::Rustls(unknown_issuer)));
        assert!(!certificate_refused(&ureq::Error::Io(io::Error::from(
            io::ErrorKind::ConnectionRefused
        ))));
        assert!(!certificate_refused(&ureq::Error::Rustls(
            rustls::Error::HandshakeNotComplete
        )));
    }
}
