use std::{env, fs, net::SocketAddr, path::PathBuf, process, sync::Arc, time::Duration};

use base64::{Engine, engine::general_purpose::STANDARD};
use parking_lot::Mutex;
use resonate_server::{Cache, Digest, Gateway, Keeping, Keys, gateway_tls};
use resonate_tidal::{HifiApi, MediaHosts};
use rustls::{ClientConfig, RootCertStore, crypto::ring::default_provider, version::TLS13};
use rustls_pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_rustls::TlsConnector;

const SECRET: &str = "a-secret-handed-to-one-listener";
const SEARCH_ANSWER: &str = r#"{"data":{"items":[{"id":77,"isrc":"GBN9Y1100089"}]}}"#;

struct Gatewayed {
    address: SocketAddr,
    connector: TlsConnector,
    asked: Arc<Mutex<Vec<String>>>,
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = env::temp_dir().join(format!("resonate-hifi-{}-{name}", process::id()));
        let _ = fs::remove_dir_all(&path);
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Reply {
    status: u16,
    head: String,
    body: String,
}

async fn upstream(
    answer: &'static str,
    status: &'static str,
) -> (SocketAddr, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
    let address = listener.local_addr().expect("an address");
    let asked = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&asked);
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let kept = Arc::clone(&kept);
            tokio::spawn(async move {
                let mut request = vec![0; 4096];
                let read = stream.read(&mut request).await.unwrap_or(0);
                kept.lock()
                    .push(String::from_utf8_lossy(&request[..read]).into_owned());
                let reply = format!(
                    "HTTP/1.0 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{answer}",
                    answer.len()
                );
                stream.write_all(reply.as_bytes()).await.ok();
            });
        }
    });
    (address, asked)
}

async fn gatewayed(upstream: SocketAddr, asked: Arc<Mutex<Vec<String>>>) -> Gatewayed {
    gatewayed_through(upstream, asked, None).await
}

async fn gatewayed_through(
    upstream: SocketAddr,
    asked: Arc<Mutex<Vec<String>>>,
    hifi: Option<HifiApi>,
) -> Gatewayed {
    kept_through(upstream, asked, hifi, Keeping::default()).await
}

async fn kept_through(
    upstream: SocketAddr,
    asked: Arc<Mutex<Vec<String>>>,
    hifi: Option<HifiApi>,
    keeping: Keeping,
) -> Gatewayed {
    let signing = rcgen::KeyPair::generate().expect("a key");
    let certificate = rcgen::CertificateParams::new(vec!["localhost".to_owned()])
        .and_then(|params| params.self_signed(&signing))
        .expect("a certificate");
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing.serialize_der()));
    let acceptor = gateway_tls(vec![certificate.der().clone()], key).expect("an acceptor");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
    let address = listener.local_addr().expect("an address");
    let keys = Keys::new(vec![Digest::of(SECRET.as_bytes())]);
    let gateway = match hifi {
        Some(hifi) => Gateway::through_keeping(keys, upstream, hifi, keeping),
        None => Gateway::keeping(keys, upstream, keeping),
    };
    tokio::spawn(gateway.serve(listener, acceptor));

    let mut roots = RootCertStore::empty();
    roots.add(certificate.der().clone()).expect("a root");
    let mut client = ClientConfig::builder_with_provider(Arc::new(default_provider()))
        .with_protocol_versions(&[&TLS13])
        .expect("versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    client.alpn_protocols = vec![b"http/1.1".to_vec()];
    Gatewayed {
        address,
        connector: TlsConnector::from(Arc::new(client)),
        asked,
    }
}

impl Gatewayed {
    async fn send(&self, method: &str, target: &str, authorization: Option<&str>) -> Reply {
        self.sent(method, target, authorization, None).await
    }

    async fn sent(
        &self,
        method: &str,
        target: &str,
        authorization: Option<&str>,
        range: Option<&str>,
    ) -> Reply {
        let tcp = TcpStream::connect(self.address).await.expect("connected");
        let mut tls = self
            .connector
            .connect(ServerName::try_from("localhost").expect("a name"), tcp)
            .await
            .expect("a handshake");
        let authorization = authorization
            .map(|value| format!("Authorization: {value}\r\n"))
            .unwrap_or_default();
        let range = range
            .map(|value| format!("Range: {value}\r\n"))
            .unwrap_or_default();
        let request =
            format!("{method} {target} HTTP/1.1\r\nHost: localhost\r\n{authorization}{range}\r\n");
        tls.write_all(request.as_bytes()).await.expect("written");
        let mut read = Vec::new();
        tls.read_to_end(&mut read).await.ok();
        let text = String::from_utf8_lossy(&read).into_owned();
        let (head, body) = text.split_once("\r\n\r\n").expect("a whole answer");
        let status = head
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .expect("a status");
        Reply {
            status,
            head: head.to_owned(),
            body: body.to_owned(),
        }
    }

    async fn ranging(&self, target: &str, range: &str) -> Reply {
        self.sent(
            "GET",
            target,
            Some(&format!("Bearer {SECRET}")),
            Some(range),
        )
        .await
    }

    async fn bearing(&self, method: &str, target: &str) -> Reply {
        self.send(method, target, Some(&format!("Bearer {SECRET}")))
            .await
    }
}

#[tokio::test]
async fn a_request_with_the_listed_secret_is_asked_of_the_hifi_api_and_answered_without_the_secret()
{
    let (upstream, asked) = upstream(SEARCH_ANSWER, "200 OK").await;
    let gateway = gatewayed(upstream, Arc::clone(&asked)).await;

    let reply = gateway
        .bearing("GET", "/search/?i=GBN9Y1100089&limit=25")
        .await;

    assert_eq!(reply.status, 200);
    assert_eq!(reply.body, SEARCH_ANSWER);
    assert!(
        reply
            .head
            .to_ascii_lowercase()
            .contains("content-type: application/json")
    );
    let asked = gateway.asked.lock();
    assert_eq!(asked.len(), 1);
    assert!(asked[0].starts_with("GET /search/?i=GBN9Y1100089&limit=25 HTTP/1.0\r\n"));
    assert!(!asked[0].to_ascii_lowercase().contains("authorization"));
    assert!(!asked[0].contains(SECRET));
}

#[tokio::test]
async fn a_request_without_a_secret_or_with_one_not_listed_is_turned_away_and_never_forwarded() {
    let (upstream, asked) = upstream(SEARCH_ANSWER, "200 OK").await;
    let gateway = gatewayed(upstream, asked).await;

    let none = gateway.send("GET", "/search/?i=X", None).await;
    let wrong = gateway
        .send("GET", "/search/?i=X", Some("Bearer another-secret"))
        .await;
    let basic = gateway
        .send("GET", "/search/?i=X", Some(&format!("Basic {SECRET}")))
        .await;
    let unknown_route = gateway
        .send("GET", "/admin", Some("Bearer another-secret"))
        .await;

    for reply in [&none, &wrong, &basic, &unknown_route] {
        assert_eq!(reply.status, 401);
        assert!(reply.head.contains("WWW-Authenticate: Bearer"));
        assert!(reply.body.is_empty());
    }
    assert!(gateway.asked.lock().is_empty());
}

#[tokio::test]
async fn a_listed_secret_reaches_only_the_hifi_routes_and_methods() {
    let (upstream, asked) = upstream(SEARCH_ANSWER, "200 OK").await;
    let gateway = gatewayed(upstream, asked).await;

    let elsewhere = gateway.bearing("GET", "/docs").await;
    let manifest = gateway
        .bearing("GET", "/track/?id=77&quality=HI_RES_LOSSLESS")
        .await;
    let traversal = gateway
        .bearing("GET", "/playback/requests/../../admin")
        .await;
    let posted = gateway.bearing("POST", "/search/?i=X").await;
    let deleted_search = gateway.bearing("DELETE", "/search/?i=X").await;

    assert_eq!(elsewhere.status, 404);
    assert_eq!(manifest.status, 404);
    assert_eq!(traversal.status, 404);
    assert_eq!(posted.status, 405);
    assert_eq!(deleted_search.status, 404);
    assert!(gateway.asked.lock().is_empty());
}

#[tokio::test]
async fn a_queued_request_is_looked_at_and_withdrawn_through_the_gateway() {
    let (upstream, asked) = upstream(r#"{"requestId":"f1b5-c0de"}"#, "202 Accepted").await;
    let gateway = gatewayed(upstream, asked).await;

    let looked = gateway.bearing("GET", "/playback/requests/f1b5-c0de").await;
    let withdrawn = gateway
        .bearing("DELETE", "/playback/requests/f1b5-c0de")
        .await;

    assert_eq!(looked.status, 202);
    assert_eq!(withdrawn.status, 202);
    let asked = gateway.asked.lock();
    assert!(asked[0].starts_with("GET /playback/requests/f1b5-c0de HTTP/1.0"));
    assert!(asked[1].starts_with("DELETE /playback/requests/f1b5-c0de HTTP/1.0"));
}

#[tokio::test]
async fn a_hifi_api_that_is_not_listening_is_a_bad_gateway() {
    let closed = {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
        listener.local_addr().expect("an address")
    };
    let gateway = gatewayed(closed, Arc::default()).await;

    let reply = gateway.bearing("GET", "/search/?i=GBN9Y1100089").await;

    assert_eq!(reply.status, 502);
    assert!(reply.body.is_empty());
}

const TRACK: u64 = 77;
const AUDIO: &[u8] = b"fLaC-and-a-great-many-bytes-of-a-song";

async fn hifi_with_a_track() -> (Gatewayed, Arc<Mutex<Vec<String>>>) {
    hifi_with_a_track_kept(Keeping::default()).await
}

async fn hifi_with_a_track_kept(keeping: Keeping) -> (Gatewayed, Arc<Mutex<Vec<String>>>) {
    hifi_serving(AUDIO, keeping).await
}

async fn hifi_serving(
    audio: &'static [u8],
    keeping: Keeping,
) -> (Gatewayed, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
    let address = listener.local_addr().expect("an address");
    let asked = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&asked);
    let manifest = STANDARD.encode(format!(
        r#"{{"mimeType":"audio/flac","codecs":"flac","encryptionType":"NONE","urls":["http://{address}/media/{TRACK}.flac"]}}"#
    ));
    let playback = format!(
        r#"{{"data":{{"trackId":{TRACK},"assetPresentation":"FULL","manifestMimeType":"application/vnd.tidal.bts","manifest":"{manifest}"}}}}"#
    );
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let kept = Arc::clone(&kept);
            let playback = playback.clone();
            tokio::spawn(async move {
                let mut request = vec![0; 4096];
                let read = stream.read(&mut request).await.unwrap_or(0);
                let text = String::from_utf8_lossy(&request[..read]).into_owned();
                kept.lock().push(text.clone());
                let target = text.split_whitespace().nth(1).unwrap_or_default();
                let (status, body): (&str, Vec<u8>) =
                    if target.starts_with(&format!("/track/?id={TRACK}")) {
                        ("200 OK", playback.into_bytes())
                    } else if target == format!("/media/{TRACK}.flac") {
                        ("200 OK", audio.to_vec())
                    } else {
                        ("404 Not Found", b"{}".to_vec())
                    };
                let head = format!(
                    "HTTP/1.0 {status}\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                );
                stream.write_all(head.as_bytes()).await.ok();
                stream.write_all(&body).await.ok();
            });
        }
    });
    let hifi = HifiApi::fetching_from(
        &format!("http://{address}"),
        MediaHosts {
            scheme: "http".to_owned(),
            domain: "127.0.0.1".to_owned(),
        },
    );
    (
        kept_through(address, Arc::clone(&asked), Some(hifi), keeping).await,
        asked,
    )
}

#[tokio::test]
async fn a_track_is_downloaded_by_the_server_through_the_hifi_api_and_sent_to_a_listed_key() {
    let (gateway, asked) = hifi_with_a_track().await;

    let reply = gateway.bearing("GET", &format!("/file/?id={TRACK}")).await;

    assert_eq!(reply.status, 200);
    assert!(
        reply
            .head
            .to_ascii_lowercase()
            .contains("content-type: audio/flac")
    );
    assert_eq!(reply.body.as_bytes(), AUDIO);
    let asked = asked.lock();
    assert!(asked[0].starts_with(&format!("GET /track/?id={TRACK}&quality=HI_RES_LOSSLESS")));
    assert!(asked.iter().all(|request| !request.contains(SECRET)));
}

#[tokio::test]
async fn a_track_is_never_downloaded_for_a_request_without_a_listed_key() {
    let (gateway, asked) = hifi_with_a_track().await;

    let none = gateway
        .send("GET", &format!("/file/?id={TRACK}"), None)
        .await;
    let wrong = gateway
        .send(
            "GET",
            &format!("/file/?id={TRACK}"),
            Some("Bearer another-secret"),
        )
        .await;

    assert_eq!(none.status, 401);
    assert_eq!(wrong.status, 401);
    assert!(none.body.is_empty() && wrong.body.is_empty());
    assert!(asked.lock().is_empty());
}

#[tokio::test]
async fn a_track_the_hifi_api_lacks_is_not_found_and_an_id_that_is_not_a_number_is_no_route() {
    let (gateway, asked) = hifi_with_a_track().await;

    let lacking = gateway.bearing("GET", "/file/?id=999").await;
    let spelt = gateway.bearing("GET", "/file/?id=77/../x").await;
    let bare = gateway.bearing("GET", "/file/").await;

    assert_eq!(lacking.status, 404);
    assert_eq!(spelt.status, 404);
    assert_eq!(bare.status, 404);
    assert_eq!(asked.lock().len(), 1);
}

fn downloads_asked(asked: &Arc<Mutex<Vec<String>>>) -> usize {
    asked
        .lock()
        .iter()
        .filter(|request| request.starts_with(&format!("GET /media/{TRACK}.flac")))
        .count()
}

fn cached(scratch: &Scratch) -> Keeping {
    Keeping {
        tracks: Some(Cache::open(&scratch.0, 1024).expect("a cache")),
        searches_for: Duration::ZERO,
    }
}

fn header_of(reply: &Reply, name: &str) -> Option<String> {
    reply.head.lines().find_map(|line| {
        let (found, value) = line.split_once(':')?;
        found
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_owned())
    })
}

#[tokio::test]
async fn a_track_asked_for_twice_is_downloaded_once_and_the_second_answer_comes_from_the_cache() {
    let scratch = Scratch::new("twice");
    let (gateway, asked) = hifi_with_a_track_kept(cached(&scratch)).await;

    let first = gateway.bearing("GET", &format!("/file/?id={TRACK}")).await;
    let second = gateway.bearing("GET", &format!("/file/?id={TRACK}")).await;

    assert_eq!(first.body.as_bytes(), AUDIO);
    assert_eq!(second.body.as_bytes(), AUDIO);
    assert_eq!(second.status, 200);
    assert_eq!(downloads_asked(&asked), 1);
    assert_eq!(
        fs::read(scratch.0.join(format!("{TRACK}.flac"))).expect("a cached file"),
        AUDIO
    );
}

#[tokio::test]
async fn a_track_is_downloaded_each_time_where_no_cache_is_kept() {
    let (gateway, asked) = hifi_with_a_track().await;

    gateway.bearing("GET", &format!("/file/?id={TRACK}")).await;
    gateway.bearing("GET", &format!("/file/?id={TRACK}")).await;

    assert_eq!(downloads_asked(&asked), 2);
}

#[tokio::test]
async fn a_range_is_answered_partially_whether_the_track_is_downloaded_or_cached() {
    let scratch = Scratch::new("ranged");
    let (gateway, asked) = hifi_with_a_track_kept(cached(&scratch)).await;
    let target = format!("/file/?id={TRACK}");
    let total = AUDIO.len();

    let downloaded = gateway.ranging(&target, "bytes=5-").await;
    let cached_tail = gateway.ranging(&target, "bytes=5-").await;
    let middle = gateway.ranging(&target, "bytes=2-6").await;
    let last = gateway.ranging(&target, "bytes=-4").await;

    for reply in [&downloaded, &cached_tail] {
        assert_eq!(reply.status, 206);
        assert_eq!(reply.body.as_bytes(), &AUDIO[5..]);
        assert_eq!(
            header_of(reply, "content-range"),
            Some(format!("bytes 5-{}/{total}", total - 1))
        );
        assert_eq!(header_of(reply, "accept-ranges").as_deref(), Some("bytes"));
    }
    assert_eq!(middle.status, 206);
    assert_eq!(middle.body.as_bytes(), &AUDIO[2..=6]);
    assert_eq!(
        header_of(&middle, "content-range"),
        Some(format!("bytes 2-6/{total}"))
    );
    assert_eq!(last.body.as_bytes(), &AUDIO[total - 4..]);
    assert_eq!(downloads_asked(&asked), 1);
}

#[tokio::test]
async fn a_range_asked_of_a_track_not_yet_cached_still_fills_the_cache_whole() {
    let scratch = Scratch::new("filled-by-range");
    let (gateway, asked) = hifi_with_a_track_kept(cached(&scratch)).await;
    let target = format!("/file/?id={TRACK}");

    let tail = gateway.ranging(&target, "bytes=10-").await;
    let whole = gateway.bearing("GET", &target).await;

    assert_eq!(tail.body.as_bytes(), &AUDIO[10..]);
    assert_eq!(whole.status, 200);
    assert_eq!(whole.body.as_bytes(), AUDIO);
    assert_eq!(downloads_asked(&asked), 1);
}

#[tokio::test]
async fn a_range_past_the_end_is_unsatisfiable_and_one_not_understood_is_the_whole_track() {
    let scratch = Scratch::new("unsatisfiable");
    let (gateway, _) = hifi_with_a_track_kept(cached(&scratch)).await;
    let target = format!("/file/?id={TRACK}");
    gateway.bearing("GET", &target).await;

    let past = gateway
        .ranging(&target, &format!("bytes={}-", AUDIO.len()))
        .await;
    let several = gateway.ranging(&target, "bytes=0-1,4-5").await;
    let other = gateway.ranging(&target, "items=0-1").await;

    assert_eq!(past.status, 416);
    assert_eq!(
        header_of(&past, "content-range"),
        Some(format!("bytes */{}", AUDIO.len()))
    );
    assert!(past.body.is_empty());
    for reply in [&several, &other] {
        assert_eq!(reply.status, 200);
        assert_eq!(reply.body.as_bytes(), AUDIO);
    }
}

#[tokio::test]
async fn a_track_over_the_limit_is_sent_and_never_kept() {
    let scratch = Scratch::new("outgrown");
    let keeping = Keeping {
        tracks: Some(Cache::open(&scratch.0, 20).expect("a cache")),
        searches_for: Duration::ZERO,
    };
    let (gateway, asked) = hifi_with_a_track_kept(keeping).await;

    let first = gateway.bearing("GET", &format!("/file/?id={TRACK}")).await;
    let second = gateway.bearing("GET", &format!("/file/?id={TRACK}")).await;

    assert_eq!(first.body.as_bytes(), AUDIO);
    assert_eq!(second.body.as_bytes(), AUDIO);
    assert_eq!(downloads_asked(&asked), 2);
    assert_eq!(fs::read_dir(&scratch.0).expect("a listing").count(), 0);
}

#[tokio::test]
async fn an_empty_answer_for_a_track_is_never_kept() {
    let scratch = Scratch::new("empty");
    let (gateway, asked) = hifi_serving(b"", cached(&scratch)).await;

    let first = gateway.bearing("GET", &format!("/file/?id={TRACK}")).await;
    let second = gateway.bearing("GET", &format!("/file/?id={TRACK}")).await;

    assert_eq!(first.status, 200);
    assert_eq!(second.status, 200);
    assert_eq!(downloads_asked(&asked), 2);
    assert_eq!(fs::read_dir(&scratch.0).expect("a listing").count(), 0);
}

fn searched(minutes: u64) -> Keeping {
    Keeping {
        tracks: None,
        searches_for: Duration::from_secs(minutes * 60),
    }
}

#[tokio::test]
async fn a_search_asked_twice_is_asked_of_the_hifi_api_once() {
    let (upstream, asked) = upstream(SEARCH_ANSWER, "200 OK").await;
    let gateway = kept_through(upstream, Arc::clone(&asked), None, searched(10)).await;

    let first = gateway.bearing("GET", "/search/?i=GBN9Y1100089").await;
    let second = gateway.bearing("GET", "/search/?i=GBN9Y1100089").await;
    let another = gateway.bearing("GET", "/search/?i=GBN9Y1100090").await;

    assert_eq!(first.body, SEARCH_ANSWER);
    assert_eq!(second.body, SEARCH_ANSWER);
    assert_eq!(second.status, 200);
    assert!(
        second
            .head
            .to_ascii_lowercase()
            .contains("content-type: application/json")
    );
    assert_eq!(another.status, 200);
    assert_eq!(asked.lock().len(), 2);
}

#[tokio::test]
async fn a_search_is_asked_every_time_where_none_is_remembered() {
    let (upstream, asked) = upstream(SEARCH_ANSWER, "200 OK").await;
    let gateway = kept_through(upstream, Arc::clone(&asked), None, searched(0)).await;

    gateway.bearing("GET", "/search/?i=GBN9Y1100089").await;
    gateway.bearing("GET", "/search/?i=GBN9Y1100089").await;

    assert_eq!(asked.lock().len(), 2);
}

#[tokio::test]
async fn a_search_the_hifi_api_failed_is_not_remembered_and_a_request_in_the_queue_never_is() {
    let (upstream, asked) = upstream("{}", "404 Not Found").await;
    let gateway = kept_through(upstream, Arc::clone(&asked), None, searched(10)).await;

    gateway.bearing("GET", "/search/?i=GBN9Y1100089").await;
    gateway.bearing("GET", "/search/?i=GBN9Y1100089").await;
    gateway.bearing("GET", "/playback/requests/f1b5-c0de").await;
    gateway.bearing("GET", "/playback/requests/f1b5-c0de").await;

    assert_eq!(asked.lock().len(), 4);
}
