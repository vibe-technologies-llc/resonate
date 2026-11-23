use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use quinn::{
    ClientConfig, Connection, Endpoint, RecvStream, SendStream, crypto::rustls::QuicClientConfig,
};
use resonate_server::{Limits, Server, Store, listening, serving};
use resonate_sync::{
    DeviceId, SpaceId, Token, read_frame, send_frame,
    wire::{FIRST_FRAME_AT_MOST, FromServer, PROTOCOL_VERSION, Refusal, SERVER_ALPN, ToServer},
};
use rustls::{RootCertStore, crypto::ring::default_provider, version::TLS13};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::time::timeout;

const SERVER_ENDS_THE_CONNECTION_WITHIN: Duration = Duration::from_secs(3);
const MEBIBYTE: u32 = 1024 * 1024;
const A_STREAM_IS_GRANTED_WITHIN: Duration = Duration::from_millis(300);

pub struct Serving {
    address: SocketAddr,
    certificate: CertificateDer<'static>,
}

pub struct Session {
    _client: Endpoint,
    pub connection: Connection,
    pub send: SendStream,
    pub recv: RecvStream,
}

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

pub fn serving_with(limits: Limits) -> Serving {
    serving_over(Store::in_memory().expect("a store"), limits)
}

pub fn serving_over(store: Store, limits: Limits) -> Serving {
    let signing = rcgen::KeyPair::generate().expect("a key");
    let certificate = rcgen::CertificateParams::new(vec!["localhost".to_owned()])
        .and_then(|params| params.self_signed(&signing))
        .expect("a certificate");
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing.serialize_der()));

    let config = serving(vec![certificate.der().clone()], key).expect("a configuration");
    let endpoint =
        listening(config, SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).expect("listening");
    let address = endpoint.local_addr().expect("an address");
    let server = Server::new(store, limits);

    tokio::spawn(server.serve(endpoint));

    Serving {
        address,
        certificate: certificate.der().clone(),
    }
}

async fn connected(serving: &Serving) -> (Endpoint, Connection) {
    let mut client = Endpoint::client(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).expect("client");

    client.set_default_client_config(client_trusting(serving.certificate.clone()));

    let connection = client
        .connect(serving.address, "localhost")
        .expect("connecting")
        .await
        .expect("connected");

    (client, connection)
}

pub async fn welcomed(serving: &Serving, device: u8) -> Session {
    let (client, connection) = connected(serving).await;
    let (mut send, mut recv) = connection.open_bi().await.expect("a stream");
    let hello = ToServer::Hello {
        version: PROTOCOL_VERSION,
        space: SpaceId([7; 16]),
        token: Token([9; 32]),
        device: DeviceId([device; 16]),
        after: 0,
    };

    send_frame(&mut send, &hello.encoded()).await.expect("said");

    let welcome = read_frame(&mut recv).await.expect("read").expect("a frame");

    assert!(matches!(
        FromServer::decoded(&welcome),
        Ok(FromServer::Welcome { .. })
    ));
    Session {
        _client: client,
        connection,
        send,
        recv,
    }
}

pub async fn announce(send: &mut SendStream, length: u32) {
    send.write_all(&length.to_be_bytes())
        .await
        .expect("announced");
    send.write_all(&[0; 64]).await.expect("began");
}

pub async fn refused_with(recv: &mut RecvStream) -> Option<Refusal> {
    while let Ok(Some(frame)) = read_frame(recv).await {
        if let Ok(FromServer::Refused(refusal)) = FromServer::decoded(&frame) {
            return Some(refusal);
        }
    }
    None
}

async fn whichever_is_refused(first: &mut RecvStream, second: &mut RecvStream) -> Option<Refusal> {
    tokio::select! {
        refused = refused_with(first) => refused,
        refused = refused_with(second) => refused,
    }
}

#[tokio::test]
async fn frames_announced_across_sessions_past_what_the_server_holds_are_refused_as_busy() {
    let serving = serving_with(Limits {
        frame_bytes: usize::try_from(MEBIBYTE).expect("a size"),
        ..Limits::default()
    });
    let mut first = welcomed(&serving, 1).await;
    let mut second = welcomed(&serving, 2).await;
    let announced = MEBIBYTE * 3 / 5;

    announce(&mut first.send, announced).await;
    announce(&mut second.send, announced).await;

    let refused = timeout(
        SERVER_ENDS_THE_CONNECTION_WITHIN,
        whichever_is_refused(&mut first.recv, &mut second.recv),
    )
    .await;

    assert_eq!(refused.ok().flatten(), Some(Refusal::Busy));
}

#[tokio::test]
async fn first_frames_still_arriving_hold_their_bytes_from_every_other_session() {
    let first_frame = u32::try_from(FIRST_FRAME_AT_MOST).expect("a size");
    let serving = serving_with(Limits {
        frame_bytes: FIRST_FRAME_AT_MOST * 3 / 2,
        ..Limits::default()
    });
    let (_first_client, first) = connected(&serving).await;
    let (_second_client, second) = connected(&serving).await;
    let (mut first_greeting, mut first_answers) = first.open_bi().await.expect("a stream");
    let (mut second_greeting, mut second_answers) = second.open_bi().await.expect("a stream");

    announce(&mut first_greeting, first_frame).await;
    announce(&mut second_greeting, first_frame).await;

    let refused = timeout(
        SERVER_ENDS_THE_CONNECTION_WITHIN,
        whichever_is_refused(&mut first_answers, &mut second_answers),
    )
    .await;

    assert_eq!(
        refused.ok().flatten(),
        Some(Refusal::Busy),
        "a first frame still arriving held nothing from the budget every session shares"
    );
}

#[tokio::test]
async fn a_first_frame_announced_past_the_ceiling_ends_the_connection_without_waiting_for_it() {
    let serving = serving_with(Limits::default());
    let (_client, connection) = connected(&serving).await;
    let (mut send, _recv) = connection.open_bi().await.expect("a stream");

    let announced = 20 * MEBIBYTE;

    assert!(usize::try_from(announced).expect("a size") > FIRST_FRAME_AT_MOST);

    announce(&mut send, announced).await;

    let ended = timeout(SERVER_ENDS_THE_CONNECTION_WITHIN, connection.closed()).await;

    assert!(
        ended.is_ok(),
        "the server kept waiting for a first frame it should never read"
    );
}

#[tokio::test]
async fn a_client_may_open_no_stream_and_send_no_datagram_beside_the_one_its_session_reads() {
    let serving = serving_with(Limits::default());
    let session = welcomed(&serving, 1).await;

    let second = timeout(A_STREAM_IS_GRANTED_WITHIN, session.connection.open_bi()).await;
    let unidirectional = timeout(A_STREAM_IS_GRANTED_WITHIN, session.connection.open_uni()).await;
    let datagrams = session.connection.max_datagram_size();

    assert!(second.is_err(), "a second stream was granted");
    assert!(
        unidirectional.is_err(),
        "a unidirectional stream was granted"
    );
    assert_eq!(datagrams, None);
}
