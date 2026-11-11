use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use quinn::{ClientConfig, Endpoint, crypto::rustls::QuicClientConfig};
use resonate_server::{Limits, Server, Store, listening, serving};
use resonate_sync::wire::{FIRST_FRAME_AT_MOST, SERVER_ALPN};
use rustls::{RootCertStore, crypto::ring::default_provider, version::TLS13};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::time::timeout;

const SERVER_ENDS_THE_CONNECTION_WITHIN: Duration = Duration::from_secs(3);
const MEBIBYTE: u32 = 1024 * 1024;

fn client_trusting(certificate: CertificateDer<'static>) -> ClientConfig {
    let mut roots = RootCertStore::empty();
    roots.add(certificate).expect("a root");
    let mut tls = rustls::ClientConfig::builder_with_provider(Arc::new(default_provider()))
        .with_protocol_versions(&[&TLS13])
        .expect("a version")
        .with_root_certificates(roots)
        .with_no_client_auth();
    tls.alpn_protocols = vec![SERVER_ALPN.to_vec()];
    let crypto = QuicClientConfig::try_from(tls).expect("a configuration");

    ClientConfig::new(Arc::new(crypto))
}

#[tokio::test]
async fn a_first_frame_announced_past_the_ceiling_ends_the_connection_without_waiting_for_it() {
    let signing = rcgen::KeyPair::generate().expect("a key");
    let certificate = rcgen::CertificateParams::new(vec!["localhost".to_owned()])
        .and_then(|params| params.self_signed(&signing))
        .expect("a certificate");
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing.serialize_der()));
    let config = serving(vec![certificate.der().clone()], key).expect("a configuration");
    let endpoint =
        listening(config, SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).expect("listening");
    let address = endpoint.local_addr().expect("an address");
    let server = Server::new(Store::in_memory().expect("a store"), Limits::default());
    tokio::spawn(server.serve(endpoint));

    let mut client = Endpoint::client(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).expect("client");
    client.set_default_client_config(client_trusting(certificate.der().clone()));
    let connection = client
        .connect(address, "localhost")
        .expect("connecting")
        .await
        .expect("connected");
    let (mut send, _recv) = connection.open_bi().await.expect("a stream");

    let announced = 20 * MEBIBYTE;
    assert!(usize::try_from(announced).expect("a size") > FIRST_FRAME_AT_MOST);
    send.write_all(&announced.to_be_bytes())
        .await
        .expect("announced");
    send.write_all(&[0; 64]).await.expect("began");

    let ended = timeout(SERVER_ENDS_THE_CONNECTION_WITHIN, connection.closed()).await;

    assert!(
        ended.is_ok(),
        "the server kept waiting for a first frame it should never read"
    );
}
