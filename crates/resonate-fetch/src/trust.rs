use std::sync::{Arc, LazyLock};

use ureq::tls::{Certificate, RootCerts, TlsConfig};

static SYSTEM_STORE: LazyLock<Vec<Certificate<'static>>> = LazyLock::new(|| {
    let system = rustls_native_certs::load_native_certs();

    for error in &system.errors {
        tracing::debug!(%error, "a certificate in the system's store could not be read");
    }

    system
        .certs
        .iter()
        .map(|der| Certificate::from_der(der.as_ref()).to_owned())
        .collect()
});

fn built_in_beside(held: &[Certificate<'static>]) -> TlsConfig {
    let built_in = webpki_root_certs::TLS_SERVER_ROOT_CERTS
        .iter()
        .map(|der| Certificate::from_der(der.as_ref()));
    let roots = built_in.chain(held.iter().cloned()).collect();

    TlsConfig::builder()
        .root_certs(RootCerts::Specific(Arc::new(roots)))
        .build()
}

pub fn system_and_built_in() -> TlsConfig {
    built_in_beside(&SYSTEM_STORE)
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
    use std::{
        io::{self, Read, Write},
        net::{Ipv4Addr, TcpListener, TcpStream},
        thread,
        time::Duration,
    };

    use rcgen::{
        BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair, KeyUsagePurpose,
    };
    use rustls::{
        CertificateError, ServerConfig, ServerConnection, StreamOwned,
        pki_types::PrivatePkcs8KeyDer,
    };
    use ureq::Agent;

    use super::*;
    use crate::testing::answering;

    const PRIVATE_BODY: &[u8] = b"reached over a private root";

    struct Fixture {
        url: String,
        root: Certificate<'static>,
    }

    fn fixture() -> Fixture {
        let root_key = KeyPair::generate().expect("a key");
        let mut root_params = CertificateParams::new(Vec::new()).expect("parameters");

        root_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        root_params.key_usages = vec![KeyUsagePurpose::KeyCertSign];
        root_params
            .distinguished_name
            .push(DnType::CommonName, "a private root");

        let root = root_params.self_signed(&root_key).expect("a root");
        let issuer = Issuer::new(root_params, root_key);

        let leaf_key = KeyPair::generate().expect("a key");
        let leaf = CertificateParams::new(vec![Ipv4Addr::LOCALHOST.to_string()])
            .and_then(|params| params.signed_by(&leaf_key, &issuer))
            .expect("a leaf");

        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let server = Arc::new(
            ServerConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .expect("protocol versions")
                .with_no_client_auth()
                .with_single_cert(
                    vec![leaf.der().clone()],
                    PrivatePkcs8KeyDer::from(leaf_key.serialize_der()).into(),
                )
                .expect("a serving certificate"),
        );

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("binds");
        let url = format!("https://{}/", listener.local_addr().expect("an address"));

        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let server = Arc::clone(&server);

                thread::spawn(move || answer(stream, server));
            }
        });

        Fixture {
            url,
            root: Certificate::from_der(root.der().as_ref()).to_owned(),
        }
    }

    fn answer(stream: TcpStream, server: Arc<ServerConfig>) {
        let Ok(connection) = ServerConnection::new(server) else {
            return;
        };
        let mut tls = StreamOwned::new(connection, stream);
        let mut head = Vec::new();
        let mut buffer = [0; 1024];

        while !head.ends_with(b"\r\n\r\n") {
            match tls.read(&mut buffer) {
                Ok(0) | Err(_) => return,
                Ok(read) => head.extend_from_slice(&buffer[..read]),
            }
        }

        let _ = tls.write_all(&answering("200 OK", &[], PRIVATE_BODY, PRIVATE_BODY.len()));
        let _ = tls.flush();

        tls.conn.send_close_notify();

        let _ = tls.flush();
    }

    fn reaching(policy: TlsConfig, url: &str) -> Result<Vec<u8>, ureq::Error> {
        let agent = Agent::config_builder()
            .tls_config(policy)
            .timeout_global(Some(Duration::from_secs(5)))
            .build()
            .new_agent();

        agent.get(url).call()?.body_mut().read_to_vec()
    }

    #[test]
    fn a_server_signed_by_a_private_root_is_reached_once_the_system_holds_that_root() {
        let Fixture { url, root } = fixture();

        let body = reaching(built_in_beside(&[root]), &url).expect("the private root is trusted");

        assert_eq!(body, PRIVATE_BODY);
    }

    #[test]
    fn a_server_signed_by_a_private_root_is_refused_by_the_built_in_roots_alone() {
        let Fixture { url, .. } = fixture();

        let built_in_only = reaching(built_in_beside(&[]), &url).expect_err("an unknown issuer");

        assert!(certificate_refused(&built_in_only), "{built_in_only}");
        assert_eq!(
            crate::failed::as_io(built_in_only).kind(),
            io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn a_server_signed_by_a_private_root_is_refused_by_an_unrelated_system_root() {
        let Fixture { url, .. } = fixture();
        let Fixture {
            root: unrelated, ..
        } = fixture();

        let refused = reaching(built_in_beside(&[unrelated]), &url).expect_err("another root");

        assert!(certificate_refused(&refused), "{refused}");
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
