use std::{
    env, fs,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine, engine::general_purpose::STANDARD};
use quinn::{ClientConfig, Endpoint, crypto::rustls::QuicClientConfig};
use resonate_server::{
    Credentials, Error, Gateway, Keys, Limits, PemOp, Server, Store, gateway_tls, listening,
    serving,
};
use resonate_sync::wire::SERVER_ALPN;
use rustls::{RootCertStore, client::Resumption, crypto::ring::default_provider, version::TLS13};
use rustls_pki_types::{CertificateDer, ServerName};
use tokio::{net::TcpStream, time::timeout};
use tokio_rustls::TlsConnector;

const A_HANDSHAKE_ENDS_WITHIN: Duration = Duration::from_secs(2);
const HTTP_ALPN: &[u8] = b"http/1.1";
const NOWHERE_UPSTREAM: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9);

struct Pair {
    certificate: CertificateDer<'static>,
    chain_pem: String,
    key_pem: String,
}

struct Listening {
    quic: SocketAddr,
    tls: SocketAddr,
}

fn scratch(name: &str) -> PathBuf {
    let folder = env::temp_dir().join(format!(
        "resonate-server-renewal-{name}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos())
    ));

    fs::create_dir_all(&folder).expect("a scratch folder");
    folder
}

fn pem(label: &str, der: &[u8]) -> String {
    format!(
        "-----BEGIN {label}-----\n{}\n-----END {label}-----\n",
        STANDARD.encode(der)
    )
}

fn minted() -> Pair {
    let signing = rcgen::KeyPair::generate().expect("a key");
    let certificate = rcgen::CertificateParams::new(vec!["localhost".to_owned()])
        .and_then(|params| params.self_signed(&signing))
        .expect("a certificate");

    Pair {
        certificate: certificate.der().clone(),
        chain_pem: pem("CERTIFICATE", certificate.der()),
        key_pem: pem("PRIVATE KEY", &signing.serialize_der()),
    }
}

fn files_in(folder: &Path) -> (PathBuf, PathBuf) {
    (folder.join("fullchain.pem"), folder.join("privkey.pem"))
}

fn installed(folder: &Path, chain: &str, key: &str) {
    let (chain_path, key_path) = files_in(folder);

    fs::write(chain_path, chain).expect("the chain written");
    fs::write(key_path, key).expect("the key written");
}

fn roots(trusted: &[&Pair]) -> RootCertStore {
    let mut roots = RootCertStore::empty();

    for pair in trusted {
        roots.add(pair.certificate.clone()).expect("a root");
    }
    roots
}

fn client(trusted: &[&Pair], alpn: &[u8]) -> rustls::ClientConfig {
    let mut tls = rustls::ClientConfig::builder_with_provider(Arc::new(default_provider()))
        .with_protocol_versions(&[&TLS13])
        .expect("a version")
        .with_root_certificates(roots(trusted))
        .with_no_client_auth();

    tls.alpn_protocols = vec![alpn.to_vec()];
    tls.resumption = Resumption::disabled();
    tls
}

async fn quic_leaf(address: SocketAddr, trusted: &[&Pair]) -> Option<CertificateDer<'static>> {
    let crypto = QuicClientConfig::try_from(client(trusted, SERVER_ALPN)).expect("a configuration");
    let mut endpoint =
        Endpoint::client(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).expect("a client");

    endpoint.set_default_client_config(ClientConfig::new(Arc::new(crypto)));

    let connecting = endpoint.connect(address, "localhost").expect("connecting");
    let connection = timeout(A_HANDSHAKE_ENDS_WITHIN, connecting)
        .await
        .ok()?
        .ok()?;
    let chain = connection
        .peer_identity()?
        .downcast::<Vec<CertificateDer<'static>>>()
        .ok()?;

    connection.close(0_u32.into(), b"seen");
    endpoint.wait_idle().await;
    chain.first().cloned()
}

async fn tls_leaf(address: SocketAddr, trusted: &[&Pair]) -> CertificateDer<'static> {
    let connector = TlsConnector::from(Arc::new(client(trusted, HTTP_ALPN)));
    let tcp = TcpStream::connect(address).await.expect("connected");

    let stream = timeout(
        A_HANDSHAKE_ENDS_WITHIN,
        connector.connect(ServerName::try_from("localhost").expect("a name"), tcp),
    )
    .await
    .expect("a handshake in time")
    .expect("a handshake");

    stream
        .get_ref()
        .1
        .peer_certificates()
        .and_then(<[_]>::first)
        .cloned()
        .expect("a leaf presented")
}

async fn served(credentials: &Credentials) -> Listening {
    let endpoint = listening(
        serving(credentials.presented()).expect("a configuration"),
        SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
    )
    .expect("listening");
    let quic = endpoint.local_addr().expect("an address");
    let server = Server::new(Store::in_memory().expect("a store"), Limits::default());

    tokio::spawn(server.serve(endpoint));

    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("a port");
    let tls = listener.local_addr().expect("an address");
    let acceptor = gateway_tls(credentials.presented()).expect("an acceptor");

    tokio::spawn(Gateway::new(Keys::new(Vec::new()), NOWHERE_UPSTREAM).serve(listener, acceptor));
    Listening { quic, tls }
}

async fn both_present(listening: &Listening, trusted: &[&Pair], expected: &Pair) {
    let quic = quic_leaf(listening.quic, trusted).await;
    let tls = tls_leaf(listening.tls, trusted).await;

    assert_eq!(quic.as_ref(), Some(&expected.certificate), "QUIC's leaf");
    assert_eq!(tls, expected.certificate, "the gateway's leaf");
}

#[tokio::test]
async fn the_next_handshake_after_a_renewal_presents_the_renewed_chain_on_both_listeners() {
    let folder = scratch("renewed");
    let (chain, key) = files_in(&folder);
    let first = minted();
    let renewed = minted();

    installed(&folder, &first.chain_pem, &first.key_pem);

    let credentials = Credentials::read(&chain, &key).expect("the first pair");
    let listening = served(&credentials).await;

    both_present(&listening, &[&first, &renewed], &first).await;

    installed(&folder, &renewed.chain_pem, &renewed.key_pem);
    credentials.reload().expect("the renewed pair");

    both_present(&listening, &[&first, &renewed], &renewed).await;
}

#[tokio::test]
async fn a_renewal_that_does_not_read_or_pair_keeps_the_working_chain_presented() {
    let folder = scratch("refused");
    let (chain, key) = files_in(&folder);
    let working = minted();
    let renewed = minted();
    let stranger = minted();
    let garbled_leaf = pem("CERTIFICATE", b"not a certificate");

    installed(&folder, &working.chain_pem, &working.key_pem);

    let credentials = Credentials::read(&chain, &key).expect("the working pair");
    let listening = served(&credentials).await;
    let trusted = [&working, &renewed, &stranger];

    installed(&folder, &renewed.chain_pem, &stranger.key_pem);

    assert!(matches!(credentials.reload(), Err(Error::Unpaired { .. })));

    both_present(&listening, &trusted, &working).await;

    installed(&folder, "garbled", &renewed.key_pem);

    assert!(matches!(
        credentials.reload(),
        Err(Error::NoCertificate { .. })
    ));

    both_present(&listening, &trusted, &working).await;

    installed(&folder, &garbled_leaf, &renewed.key_pem);

    assert!(matches!(credentials.reload(), Err(Error::Unpaired { .. })));

    both_present(&listening, &trusted, &working).await;

    installed(&folder, &renewed.chain_pem, "garbled");

    assert!(matches!(
        credentials.reload(),
        Err(Error::Pem { op: PemOp::Key, .. })
    ));

    both_present(&listening, &trusted, &working).await;

    fs::remove_file(&chain).expect("the chain taken away");

    assert!(matches!(
        credentials.reload(),
        Err(Error::Pem {
            op: PemOp::Certificate,
            ..
        })
    ));

    both_present(&listening, &trusted, &working).await;

    installed(&folder, &renewed.chain_pem, &renewed.key_pem);
    credentials.reload().expect("the renewed pair");

    both_present(&listening, &trusted, &renewed).await;
}

#[cfg(unix)]
mod hung_up {
    use std::{
        net::{Ipv4Addr, SocketAddr, UdpSocket},
        path::Path,
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };

    use super::{Pair, files_in, installed, minted, quic_leaf, scratch};

    const THE_BINARY_LISTENS_WITHIN: Duration = Duration::from_secs(20);
    const A_HANGUP_IS_HEEDED_WITHIN: Duration = Duration::from_secs(10);
    const ASKED_AGAIN_AFTER: Duration = Duration::from_millis(50);

    fn a_free_udp_port() -> SocketAddr {
        UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
            .and_then(|socket| socket.local_addr())
            .expect("a port")
    }

    fn the_server(listen: SocketAddr, folder: &Path) -> Child {
        let (chain, key) = files_in(folder);

        Command::new(env!("CARGO_BIN_EXE_resonate-server"))
            .arg("--listen")
            .arg(listen.to_string())
            .arg("--certificate")
            .arg(chain)
            .arg("--key")
            .arg(key)
            .arg("--store")
            .arg(folder.join("records.sqlite"))
            .env("RUST_LOG", "warn")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the server started")
    }

    async fn presented_within(
        address: SocketAddr,
        trusted: &[&Pair],
        expected: &Pair,
        patience: Duration,
    ) -> bool {
        let began = Instant::now();

        while began.elapsed() < patience {
            if quic_leaf(address, trusted).await.as_ref() == Some(&expected.certificate) {
                return true;
            }
            tokio::time::sleep(ASKED_AGAIN_AFTER).await;
        }
        false
    }

    #[tokio::test]
    async fn the_server_presents_a_renewed_chain_once_it_is_hung_up_on() {
        let folder = scratch("hangup");
        let first = minted();
        let renewed = minted();

        let trusted = [&first, &renewed];
        let listen = a_free_udp_port();

        installed(&folder, &first.chain_pem, &first.key_pem);

        let mut server = the_server(listen, &folder);
        let listened = presented_within(listen, &trusted, &first, THE_BINARY_LISTENS_WITHIN).await;

        installed(&folder, &renewed.chain_pem, &renewed.key_pem);

        let hung_up = Command::new("kill")
            .arg("-HUP")
            .arg(server.id().to_string())
            .status()
            .expect("kill ran");
        let renewed_seen =
            presented_within(listen, &trusted, &renewed, A_HANGUP_IS_HEEDED_WITHIN).await;
        let still_running = server.try_wait().expect("asked").is_none();

        let _ = server.kill();
        let _ = server.wait();

        assert!(listened, "the server never presented its first chain");
        assert!(hung_up.success());
        assert!(renewed_seen, "the renewed chain was never presented");
        assert!(still_running, "the hangup ended the server");
    }
}
