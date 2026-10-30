use std::sync::{Arc, LazyLock};

use rustls::{
    ClientConfig, DigitallySignedStruct, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{WebPkiSupportedAlgorithms, ring},
    pki_types::{CertificateDer, ServerName, UnixTime},
};

use crate::{Error, Result};

static CONFIG: LazyLock<std::result::Result<Arc<ClientConfig>, rustls::Error>> =
    LazyLock::new(|| {
        let provider = Arc::new(ring::default_provider());
        let verifier = Arc::new(TheReceiversOwn(provider.signature_verification_algorithms));
        let config = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()?
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth();
        Ok(Arc::new(config))
    });

pub(crate) fn config() -> Result<Arc<ClientConfig>> {
    CONFIG
        .as_ref()
        .map(Arc::clone)
        .map_err(|source| Error::Tls {
            source: Box::new(source.clone()),
        })
}

#[derive(Debug)]
struct TheReceiversOwn(WebPkiSupportedAlgorithms);

impl ServerCertVerifier for TheReceiversOwn {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signed: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, signed, &self.0)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signed: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, signed, &self.0)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_config_builds_on_ring() {
        assert!(config().is_ok());
    }
}
