use std::{path::Path, sync::Arc, time::Duration};

use quinn::{
    ClientConfig, IdleTimeout, RecvStream, SendStream, ServerConfig, TransportConfig,
    crypto::rustls::{QuicClientConfig, QuicServerConfig},
};
use ring::signature::{ED25519, UnparsedPublicKey};
use rustls::{
    CertificateError, DigitallySignedStruct, DistinguishedName, PeerMisbehaved, RootCertStore,
    SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{CryptoProvider, ring::default_provider},
    server::danger::{ClientCertVerified, ClientCertVerifier},
    version::TLS13,
};
use rustls_pki_types::{
    CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime, pem::PemObject,
};

use crate::{
    Derived, Error, Result, StreamOp, TlsOp,
    key::PEER_PUBLIC_KEY_BYTES,
    wire::{LARGEST_FRAME, PEER_ALPN, SERVER_ALPN},
};

pub const PEER_NAME: &str = "resonate-peer";
pub const KEEP_THE_SERVER_EVERY: Duration = Duration::from_secs(25);
pub const SERVER_SILENT_FOR: Duration = Duration::from_secs(90);
pub const KEEP_A_PEER_EVERY: Duration = Duration::from_secs(10);
pub const PEER_SILENT_FOR: Duration = Duration::from_secs(60);
const FRAME_LENGTH_BYTES: usize = 4;

pub struct PeerIdentity {
    certificate: CertificateDer<'static>,
    key: PrivatePkcs8KeyDer<'static>,
    public: [u8; PEER_PUBLIC_KEY_BYTES],
}

#[derive(Debug)]
struct SpaceVerifier {
    public: [u8; PEER_PUBLIC_KEY_BYTES],
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(default_provider())
}

pub fn trusted_roots(extra: Option<&Path>) -> Result<RootCertStore> {
    let mut roots = RootCertStore::empty();
    roots.add_parsable_certificates(webpki_root_certs::TLS_SERVER_ROOT_CERTS.iter().cloned());
    let system = rustls_native_certs::load_native_certs();
    for error in &system.errors {
        tracing::debug!(%error, "a certificate in the system's store could not be read");
    }
    roots.add_parsable_certificates(system.certs);
    if let Some(path) = extra {
        let certificates = CertificateDer::pem_file_iter(path)
            .and_then(Iterator::collect::<std::result::Result<Vec<_>, _>>)
            .map_err(|source| Error::TrustedCertificate {
                path: path.to_path_buf(),
                source,
            })?;
        for certificate in certificates {
            roots
                .add(certificate)
                .map_err(|source| Error::tls(TlsOp::Roots, source))?;
        }
    }
    Ok(roots)
}

pub fn to_the_server(roots: RootCertStore) -> Result<ClientConfig> {
    let mut tls = rustls::ClientConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&TLS13])
        .map_err(|source| Error::tls(TlsOp::Client, source))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    tls.alpn_protocols = vec![SERVER_ALPN.to_vec()];
    let crypto = QuicClientConfig::try_from(tls).map_err(|_| Error::QuicCrypto)?;
    let mut config = ClientConfig::new(Arc::new(crypto));
    config.transport_config(Arc::new(transport(
        KEEP_THE_SERVER_EVERY,
        SERVER_SILENT_FOR,
    )));
    Ok(config)
}

pub fn transport(keep_alive: Duration, silent_for: Duration) -> TransportConfig {
    let mut transport = TransportConfig::default();
    transport.keep_alive_interval(Some(keep_alive));
    transport.max_idle_timeout(IdleTimeout::try_from(silent_for).ok());
    transport
}

impl PeerIdentity {
    pub fn of(derived: &Derived) -> Result<Self> {
        let key = PrivatePkcs8KeyDer::from(derived.peer_pkcs8());
        let signing = rcgen::KeyPair::from_pkcs8_der_and_sign_algo(&key, &rcgen::PKCS_ED25519)
            .map_err(|source| Error::Certificate(Box::new(source)))?;
        let certificate = rcgen::CertificateParams::new(vec![PEER_NAME.to_owned()])
            .and_then(|params| params.self_signed(&signing))
            .map_err(|source| Error::Certificate(Box::new(source)))?;
        Ok(Self {
            certificate: certificate.der().clone(),
            key,
            public: derived.peer_public,
        })
    }

    pub fn serving(&self) -> Result<ServerConfig> {
        let verifier = Arc::new(SpaceVerifier {
            public: self.public,
        });
        let mut tls = rustls::ServerConfig::builder_with_provider(provider())
            .with_protocol_versions(&[&TLS13])
            .map_err(|source| Error::tls(TlsOp::Server, source))?
            .with_client_cert_verifier(verifier)
            .with_single_cert(
                vec![self.certificate.clone()],
                PrivateKeyDer::Pkcs8(self.key.clone_key()),
            )
            .map_err(|source| Error::tls(TlsOp::Server, source))?;
        tls.alpn_protocols = vec![PEER_ALPN.to_vec()];
        let crypto = QuicServerConfig::try_from(tls).map_err(|_| Error::QuicCrypto)?;
        let mut config = ServerConfig::with_crypto(Arc::new(crypto));
        config.transport_config(Arc::new(transport(KEEP_A_PEER_EVERY, PEER_SILENT_FOR)));
        Ok(config)
    }

    pub fn dialling(&self) -> Result<ClientConfig> {
        let verifier = Arc::new(SpaceVerifier {
            public: self.public,
        });
        let mut tls = rustls::ClientConfig::builder_with_provider(provider())
            .with_protocol_versions(&[&TLS13])
            .map_err(|source| Error::tls(TlsOp::Client, source))?
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_client_auth_cert(
                vec![self.certificate.clone()],
                PrivateKeyDer::Pkcs8(self.key.clone_key()),
            )
            .map_err(|source| Error::tls(TlsOp::Client, source))?;
        tls.alpn_protocols = vec![PEER_ALPN.to_vec()];
        let crypto = QuicClientConfig::try_from(tls).map_err(|_| Error::QuicCrypto)?;
        let mut config = ClientConfig::new(Arc::new(crypto));
        config.transport_config(Arc::new(transport(KEEP_A_PEER_EVERY, PEER_SILENT_FOR)));
        Ok(config)
    }
}

impl SpaceVerifier {
    fn signed_by_the_space(
        &self,
        message: &[u8],
        signed: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        if signed.scheme != SignatureScheme::ED25519 {
            return Err(rustls::Error::PeerMisbehaved(
                PeerMisbehaved::SignedHandshakeWithUnadvertisedSigScheme,
            ));
        }
        UnparsedPublicKey::new(&ED25519, &self.public)
            .verify(message, signed.signature())
            .map(|()| HandshakeSignatureValid::assertion())
            .map_err(|_| rustls::Error::InvalidCertificate(CertificateError::BadSignature))
    }
}

impl ServerCertVerifier for SpaceVerifier {
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
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _signed: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::PeerMisbehaved(
            PeerMisbehaved::SignedHandshakeWithUnadvertisedSigScheme,
        ))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        _cert: &CertificateDer<'_>,
        signed: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.signed_by_the_space(message, signed)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ED25519]
    }
}

impl ClientCertVerifier for SpaceVerifier {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> std::result::Result<ClientCertVerified, rustls::Error> {
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _signed: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::PeerMisbehaved(
            PeerMisbehaved::SignedHandshakeWithUnadvertisedSigScheme,
        ))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        _cert: &CertificateDer<'_>,
        signed: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.signed_by_the_space(message, signed)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ED25519]
    }
}

pub async fn send_frame(stream: &mut SendStream, payload: &[u8]) -> Result<()> {
    let length = u32::try_from(payload.len()).map_err(|_| Error::Oversized {
        length: payload.len(),
    })?;
    if payload.len() > LARGEST_FRAME {
        return Err(Error::Oversized {
            length: payload.len(),
        });
    }
    stream
        .write_all(&length.to_be_bytes())
        .await
        .map_err(|source| Error::stream(StreamOp::Write, source))?;
    stream
        .write_all(payload)
        .await
        .map_err(|source| Error::stream(StreamOp::Write, source))
}

pub async fn read_frame(stream: &mut RecvStream) -> Result<Option<Vec<u8>>> {
    let mut length = [0; FRAME_LENGTH_BYTES];
    match stream.read_exact(&mut length).await {
        Ok(()) => {}
        Err(quinn::ReadExactError::FinishedEarly(0)) => return Ok(None),
        Err(quinn::ReadExactError::FinishedEarly(_)) => {
            return Err(Error::stream(
                StreamOp::Read,
                std::io::Error::from(std::io::ErrorKind::UnexpectedEof),
            ));
        }
        Err(quinn::ReadExactError::ReadError(source)) => {
            return Err(Error::stream(StreamOp::Read, source));
        }
    }
    let length = usize::try_from(u32::from_be_bytes(length)).unwrap_or(usize::MAX);
    if length > LARGEST_FRAME {
        return Err(Error::Oversized { length });
    }
    let mut payload = vec![0; length];
    match stream.read_exact(&mut payload).await {
        Ok(()) => Ok(Some(payload)),
        Err(quinn::ReadExactError::FinishedEarly(_)) => Err(Error::stream(
            StreamOp::Read,
            std::io::Error::from(std::io::ErrorKind::UnexpectedEof),
        )),
        Err(quinn::ReadExactError::ReadError(source)) => Err(Error::stream(StreamOp::Read, source)),
    }
}
