use std::{
    io,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use futures_util::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use parking_lot::Mutex;
use quinn::{RecvStream, SendStream};
use rustls::{ClientConfig, RootCertStore, crypto::ring::default_provider, version::TLS13};
use rustls_pki_types::ServerName;
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::{TcpStream, lookup_host},
    sync::OwnedSemaphorePermit,
    time::timeout,
};
use tokio_rustls::TlsConnector;
use tokio_tungstenite::{
    WebSocketStream, accept_hdr_async_with_config, client_async_with_config,
    tungstenite::{
        self, Message,
        handshake::server::{Callback, ErrorResponse, Request, Response},
        http::Uri,
        protocol::WebSocketConfig,
    },
};

use crate::{
    Error, Result, StreamOp, TlsOp,
    quic::{read_frame_held, send_frame},
    wire::LARGEST_FRAME,
};

pub const WEB_SERVER_PORT: u16 = 443;
pub const WEB_SILENT_FOR: Duration = Duration::from_secs(90);
const WEB_ALPN: &[u8] = b"http/1.1";
const WEB_FRAMING_AT_MOST: usize = 64;
const WEB_READ_AT_ONCE: usize = 16 * 1024;
const A_FRAME_HEADER_AT_LEAST: usize = 2;
const A_CLIENTS_MASK: usize = 4;
const SHORT_LENGTH_AT_MOST: usize = 125;
const SHORT_PAST: usize = SHORT_LENGTH_AT_MOST + 1;
const MEDIUM_LENGTH_AT_MOST: usize = u16::MAX as usize;
const MEDIUM_LENGTH_BYTES: usize = 2;
const LONG_LENGTH_BYTES: usize = 8;

pub trait Duplex: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> Duplex for T {}

type Tunnel = Box<dyn Duplex>;
type Socket = WebSocketStream<Tunnel>;

pub enum Outgoing {
    Quic(SendStream),
    Web(SplitSink<Socket, Message>),
}

pub enum Incoming {
    Quic(RecvStream),
    Web(SplitStream<Socket>, Budget),
}

pub trait Holds: Send + Sync {
    fn hold(&self, length: usize) -> Option<OwnedSemaphorePermit>;
    fn draw(&self, bytes: usize) -> Option<OwnedSemaphorePermit>;
}

pub enum Heard {
    Frame {
        payload: Vec<u8>,
        held: Option<OwnedSemaphorePermit>,
    },
    PassedOver,
}

#[derive(Clone, Default)]
pub struct Budget(Option<Arc<Mutex<Spending>>>);

struct Spending {
    left: usize,
    drawing: Option<Arc<dyn Holds>>,
    drawn: Option<OwnedSemaphorePermit>,
    refused_at: Option<usize>,
}

impl Spending {
    fn spend(&mut self, read: usize) -> io::Result<()> {
        self.left = self.left.saturating_sub(read);

        let Some(drawing) = self.drawing.as_ref().filter(|_| read > 0) else {
            return Ok(());
        };
        let Some(more) = drawing.draw(read) else {
            let drawn = self
                .drawn
                .as_ref()
                .map_or(0, OwnedSemaphorePermit::num_permits);

            self.refused_at = Some(drawn.saturating_add(read));
            return Err(io::Error::from(io::ErrorKind::OutOfMemory));
        };

        match &mut self.drawn {
            Some(drawn) => drawn.merge(more),
            None => self.drawn = Some(more),
        }
        Ok(())
    }

    fn taken_by(&mut self, weight: usize) -> Option<OwnedSemaphorePermit> {
        let mut taken = self.drawn.take()?;

        let past = taken.num_permits().saturating_sub(weight);

        if let Some(mut carried) = taken.split(past).filter(|_| past > 0) {
            let unread = carried.num_permits().saturating_sub(WEB_READ_AT_ONCE);

            drop(carried.split(unread));
            self.drawn = Some(carried);
        }
        Some(taken)
    }
}

fn as_framed_by_a_client(payload: usize) -> usize {
    let length_bytes = match payload {
        0..=SHORT_LENGTH_AT_MOST => 0,
        SHORT_PAST..=MEDIUM_LENGTH_AT_MOST => MEDIUM_LENGTH_BYTES,
        _ => LONG_LENGTH_BYTES,
    };
    A_FRAME_HEADER_AT_LEAST + length_bytes + A_CLIENTS_MASK + payload
}

impl Budget {
    fn spendable() -> (Self, Arc<Mutex<Spending>>) {
        let spending = Arc::new(Mutex::new(Spending {
            left: usize::MAX,
            drawing: None,
            drawn: None,
            refused_at: None,
        }));
        (Self(Some(Arc::clone(&spending))), spending)
    }

    fn allow(&self, bytes: usize) {
        if let Some(spending) = &self.0 {
            spending.lock().left = bytes;
        }
    }

    fn draw_from(&self, holds: &Arc<dyn Holds>) {
        if let Some(spending) = &self.0 {
            spending
                .lock()
                .drawing
                .get_or_insert_with(|| Arc::clone(holds));
        }
    }

    fn taken_by(&self, message: &Message) -> Option<OwnedSemaphorePermit> {
        let weight = as_framed_by_a_client(message.len());
        self.0
            .as_ref()
            .and_then(|spending| spending.lock().taken_by(weight))
    }

    fn refused_at(&self) -> Option<usize> {
        self.0
            .as_ref()
            .and_then(|spending| spending.lock().refused_at)
    }
}

struct Budgeted<S> {
    inner: S,
    spending: Arc<Mutex<Spending>>,
}

impl<S: AsyncRead + Unpin> AsyncRead for Budgeted<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.spending.lock().left == 0 {
            return Poll::Ready(Err(io::Error::from(io::ErrorKind::InvalidData)));
        }

        let before = buffer.filled().len();
        let polled = Pin::new(&mut self.inner).poll_read(context, buffer);

        if let Poll::Ready(Ok(())) = polled {
            let read = buffer.filled().len() - before;
            if let Err(refused) = self.spending.lock().spend(read) {
                return Poll::Ready(Err(refused));
            }
        }
        polled
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Budgeted<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(context, buffer)
    }

    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(context)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(context)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebAddress {
    pub secure: bool,
    pub host: String,
    pub port: u16,
    pub target: String,
}

pub fn web_address(server: &str) -> Option<WebAddress> {
    let target = server.trim();
    let uri = target.parse::<Uri>().ok()?;
    let secure = match uri.scheme_str()? {
        "wss" => true,
        "ws" => false,
        _ => return None,
    };
    let host = uri.host()?.trim_matches(['[', ']']).to_owned();

    let port = uri
        .port_u16()
        .unwrap_or(if secure { WEB_SERVER_PORT } else { 80 });
    Some(WebAddress {
        secure,
        host,
        port,
        target: target.to_owned(),
    })
}

async fn on_this_machine(address: &WebAddress) -> Result<Vec<SocketAddr>> {
    let resolved = lookup_host((address.host.as_str(), address.port))
        .await
        .map_err(|source| Error::Resolve {
            server: address.host.clone().into(),
            source,
        })?
        .collect::<Vec<_>>();
    let only_here = !resolved.is_empty()
        && resolved
            .iter()
            .all(|resolved| resolved.ip().to_canonical().is_loopback());

    if !only_here {
        return Err(Error::PlainOffThisMachine {
            server: address.host.clone().into(),
        });
    }
    Ok(resolved)
}

pub fn to_the_web(roots: RootCertStore) -> Result<Arc<ClientConfig>> {
    let mut tls = ClientConfig::builder_with_provider(Arc::new(default_provider()))
        .with_protocol_versions(&[&TLS13])
        .map_err(|source| Error::tls(TlsOp::Client, source))?
        .with_root_certificates(roots)
        .with_no_client_auth();

    tls.alpn_protocols = vec![WEB_ALPN.to_vec()];
    Ok(Arc::new(tls))
}

fn limits() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(LARGEST_FRAME))
        .max_frame_size(Some(LARGEST_FRAME))
}

fn accepting() -> WebSocketConfig {
    limits().read_buffer_size(WEB_READ_AT_ONCE)
}

fn halves(socket: Socket, budget: Budget) -> (Outgoing, Incoming) {
    let (sink, stream) = socket.split();
    (Outgoing::Web(sink), Incoming::Web(stream, budget))
}

pub async fn connect(
    address: &WebAddress,
    tls: &Arc<ClientConfig>,
) -> Result<(Outgoing, Incoming)> {
    let tcp = if address.secure {
        TcpStream::connect((address.host.as_str(), address.port)).await
    } else {
        TcpStream::connect(on_this_machine(address).await?.as_slice()).await
    }
    .map_err(|source| Error::stream(StreamOp::Open, source))?;

    tcp.set_nodelay(true)
        .map_err(|source| Error::stream(StreamOp::Open, source))?;

    let tunnel: Tunnel = if address.secure {
        let name = ServerName::try_from(address.host.clone()).map_err(|_| Error::ServerName {
            server: address.host.clone().into(),
        })?;
        let secured = TlsConnector::from(Arc::clone(tls))
            .connect(name, tcp)
            .await
            .map_err(|source| Error::stream(StreamOp::Open, source))?;
        Box::new(secured)
    } else {
        Box::new(tcp)
    };
    let (socket, _) = client_async_with_config(address.target.as_str(), tunnel, Some(limits()))
        .await
        .map_err(Error::web)?;
    Ok(halves(socket, Budget::default()))
}

pub struct Accepted {
    pub outgoing: Outgoing,
    pub incoming: Incoming,
    pub forwarded_for: Option<IpAddr>,
}

struct Forwarding<'a> {
    header: Option<&'a str>,
    client: Option<IpAddr>,
}

impl Callback for &mut Forwarding<'_> {
    fn on_request(
        self,
        request: &Request,
        response: Response,
    ) -> std::result::Result<Response, ErrorResponse> {
        self.client = self
            .header
            .and_then(|name| request.headers().get(name))
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .and_then(|value| value.trim().parse().ok());
        Ok(response)
    }
}

pub async fn accepted(
    stream: impl Duplex + 'static,
    forwarded_by: Option<&str>,
) -> Result<Accepted> {
    let mut forwarding = Forwarding {
        header: forwarded_by,
        client: None,
    };
    let (budget, spending) = Budget::spendable();
    let tunnel: Tunnel = Box::new(Budgeted {
        inner: stream,
        spending,
    });

    let socket = accept_hdr_async_with_config(tunnel, &mut forwarding, Some(accepting()))
        .await
        .map_err(Error::web)?;
    let (outgoing, incoming) = halves(socket, budget);
    Ok(Accepted {
        outgoing,
        incoming,
        forwarded_for: forwarding.client,
    })
}

impl Outgoing {
    pub async fn send(&mut self, payload: &[u8]) -> Result<()> {
        match self {
            Self::Quic(stream) => send_frame(stream, payload).await,
            Self::Web(sink) => {
                if payload.len() > LARGEST_FRAME {
                    return Err(Error::Oversized {
                        length: payload.len(),
                    });
                }
                sink.send(Message::Binary(payload.to_vec().into()))
                    .await
                    .map_err(Error::web)
            }
        }
    }

    pub async fn keep_alive(&mut self) -> Result<()> {
        match self {
            Self::Quic(_) => Ok(()),
            Self::Web(sink) => sink
                .send(Message::Ping(Vec::new().into()))
                .await
                .map_err(Error::web),
        }
    }

    pub async fn finish(&mut self) {
        match self {
            Self::Quic(stream) => {
                let _ = stream.finish();
            }
            Self::Web(sink) => {
                let _ = sink.close().await;
            }
        }
    }

    pub const fn is_web(&self) -> bool {
        matches!(self, Self::Web(_))
    }
}

impl Incoming {
    pub async fn read(&mut self) -> Result<Option<Vec<u8>>> {
        self.read_at_most(LARGEST_FRAME).await
    }

    pub async fn read_at_most(&mut self, most: usize) -> Result<Option<Vec<u8>>> {
        let read = self.read_held(most, |_| Some(())).await?;
        Ok(read.map(|(payload, ())| payload))
    }

    pub async fn read_held<H>(
        &mut self,
        most: usize,
        hold: impl FnOnce(usize) -> Option<H>,
    ) -> Result<Option<(Vec<u8>, H)>> {
        let (stream, budget) = match self {
            Self::Quic(stream) => return read_frame_held(stream, most, hold).await,
            Self::Web(stream, budget) => (stream, budget),
        };

        budget.allow(most.saturating_add(WEB_FRAMING_AT_MOST));
        loop {
            match next_message(stream, budget).await? {
                None => return Ok(None),
                Some((Message::Binary(payload), _)) if payload.len() > most => {
                    return Err(Error::Oversized {
                        length: payload.len(),
                    });
                }
                Some((Message::Binary(payload), _)) => {
                    let length = payload.len();
                    let held = hold(length).ok_or(Error::NoRoom { length })?;

                    return Ok(Some((payload.to_vec(), held)));
                }
                Some(_) => {}
            }
        }
    }

    pub async fn read_heard(
        &mut self,
        most: usize,
        holds: &Arc<dyn Holds>,
    ) -> Result<Option<Heard>> {
        let (stream, budget) = match self {
            Self::Quic(stream) => {
                let read = read_frame_held(stream, most, |length| holds.hold(length)).await?;

                return Ok(read.map(|(payload, held)| Heard::Frame {
                    payload,
                    held: Some(held),
                }));
            }
            Self::Web(stream, budget) => (stream, budget),
        };

        budget.draw_from(holds);
        budget.allow(most.saturating_add(WEB_FRAMING_AT_MOST));
        match next_message(stream, budget).await? {
            None => Ok(None),
            Some((Message::Binary(payload), _)) if payload.len() > most => Err(Error::Oversized {
                length: payload.len(),
            }),
            Some((Message::Binary(payload), held)) => Ok(Some(Heard::Frame {
                payload: payload.to_vec(),
                held,
            })),
            Some(_) => Ok(Some(Heard::PassedOver)),
        }
    }
}

async fn next_message(
    stream: &mut SplitStream<Socket>,
    budget: &Budget,
) -> Result<Option<(Message, Option<OwnedSemaphorePermit>)>> {
    let heard = timeout(WEB_SILENT_FOR, stream.next())
        .await
        .map_err(|_| Error::ServerSilent)?;
    match heard {
        None
        | Some(Ok(Message::Close(_)))
        | Some(Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed)) => {
            Ok(None)
        }
        Some(Ok(message)) => {
            let held = budget.taken_by(&message);
            Ok(Some((message, held)))
        }
        Some(Err(error)) => Err(budget
            .refused_at()
            .map_or_else(|| Error::web(error), |length| Error::NoRoom { length })),
    }
}

#[cfg(test)]
mod tests {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt, duplex},
        net::TcpListener,
        sync::Semaphore,
        task::JoinHandle,
        time::{Instant, sleep},
    };

    use super::*;

    const READ_AT_ONCE: usize = 16;
    const SETTLES_WITHIN: Duration = Duration::from_secs(5);
    const BINARY_AND_FINAL: u8 = 0x82;
    const PING_AND_FINAL: u8 = 0x89;
    const MASKED_WITH_A_LENGTH_OF_EIGHT_BYTES: u8 = 0x80 | 127;
    const MASKED_WITH_A_SHORT_LENGTH: u8 = 0x80;
    const UNMASKING_KEY: [u8; 4] = [0; 4];

    struct Pool(Arc<Semaphore>);

    impl Holds for Pool {
        fn hold(&self, length: usize) -> Option<OwnedSemaphorePermit> {
            self.draw(length)
        }

        fn draw(&self, bytes: usize) -> Option<OwnedSemaphorePermit> {
            let weight = u32::try_from(bytes).ok()?;
            Arc::clone(&self.0).try_acquire_many_owned(weight).ok()
        }
    }

    fn pool_of(bytes: usize) -> (Arc<Semaphore>, Arc<dyn Holds>) {
        let semaphore = Arc::new(Semaphore::new(bytes));
        let holds: Arc<dyn Holds> = Arc::new(Pool(Arc::clone(&semaphore)));
        (semaphore, holds)
    }

    fn binary_header(length: usize) -> Vec<u8> {
        let mut header = vec![BINARY_AND_FINAL, MASKED_WITH_A_LENGTH_OF_EIGHT_BYTES];

        header.extend_from_slice(&u64::try_from(length).expect("a length").to_be_bytes());
        header.extend_from_slice(&UNMASKING_KEY);
        header
    }

    fn ping() -> Vec<u8> {
        let mut ping = vec![PING_AND_FINAL, MASKED_WITH_A_SHORT_LENGTH];

        ping.extend_from_slice(&UNMASKING_KEY);
        ping
    }

    async fn upgraded_by_hand(port: u16) -> TcpStream {
        let mut stream = TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("connected");

        stream
            .write_all(
                format!(
                    "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\n\
                     Connection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
                     Sec-WebSocket-Version: 13\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .expect("asked to upgrade");

        let mut answer = Vec::new();
        let mut byte = [0; 1];

        while !answer.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).await.expect("an answer");
            answer.push(byte[0]);
        }
        stream
    }

    async fn reading_with(
        holds: Arc<dyn Holds>,
        most: usize,
    ) -> (u16, JoinHandle<(Vec<Result<Option<Heard>>>, Outgoing)>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bound");
        let port = listener.local_addr().expect("an address").port();
        let reading = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accepted");
            let mut accepted = accepted(stream, None).await.expect("upgraded");
            let mut heard = Vec::new();
            loop {
                let read = accepted.incoming.read_heard(most, &holds).await;
                let ended = !matches!(read, Ok(Some(_)));

                heard.push(read);
                if ended {
                    return (heard, accepted.outgoing);
                }
            }
        });
        (port, reading)
    }

    async fn settled(what: &str, done: impl Fn() -> bool) {
        let deadline = Instant::now() + SETTLES_WITHIN;
        while !done() {
            assert!(Instant::now() < deadline, "{what} never happened");

            sleep(Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn the_bytes_of_a_message_not_yet_whole_are_held_from_the_budget_as_they_arrive() {
        let total = 1024 * 1024;
        let arrived = 100 * 1024;
        let (semaphore, holds) = pool_of(total);

        let (port, reading) = reading_with(holds, total).await;
        let mut stream = upgraded_by_hand(port).await;

        stream
            .write_all(&binary_header(512 * 1024))
            .await
            .expect("announced");
        stream.write_all(&vec![7; arrived]).await.expect("sent");

        settled("the arrived bytes being held", || {
            semaphore.available_permits() <= total - arrived
        })
        .await;
        drop(stream);

        let (heard, _) = reading.await.expect("the reading task");

        assert!(matches!(heard.as_slice(), [Err(_)]));
        assert_eq!(semaphore.available_permits(), total);
    }

    #[tokio::test]
    async fn a_message_past_what_the_budget_has_left_is_no_room_before_it_is_whole() {
        let (_, holds) = pool_of(64 * 1024);
        let (port, reading) = reading_with(holds, 1024 * 1024).await;
        let mut stream = upgraded_by_hand(port).await;

        stream
            .write_all(&binary_header(512 * 1024))
            .await
            .expect("announced");

        let _ = stream.write_all(&vec![7; 128 * 1024]).await;

        let (heard, _) = reading.await.expect("the reading task");

        assert!(
            matches!(heard.as_slice(), [Err(Error::NoRoom { .. })]),
            "{:?}",
            heard.iter().map(Result::is_ok).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn a_ping_is_heard_as_passed_over_and_gives_back_what_it_drew() {
        let total = 64 * 1024;
        let (semaphore, holds) = pool_of(total);

        let (port, reading) = reading_with(holds, total).await;
        let mut stream = upgraded_by_hand(port).await;

        stream.write_all(&ping()).await.expect("pinged");
        stream
            .write_all(&binary_header(3))
            .await
            .expect("announced");
        stream.write_all(&[1, 2, 3]).await.expect("sent");
        stream.shutdown().await.expect("closed");

        let (heard, _) = reading.await.expect("the reading task");
        let [
            Ok(Some(Heard::PassedOver)),
            Ok(Some(Heard::Frame { payload, held })),
            ..,
        ] = heard.as_slice()
        else {
            panic!("a ping then a frame were not heard");
        };

        assert_eq!(payload, &[1, 2, 3]);
        assert_eq!(
            held.as_ref().map(OwnedSemaphorePermit::num_permits),
            Some(as_framed_by_a_client(3))
        );

        drop(heard);

        assert_eq!(semaphore.available_permits(), total);
    }

    #[tokio::test]
    async fn a_plain_web_socket_is_refused_to_any_host_but_this_machine() {
        let tls = to_the_web(RootCertStore::empty()).expect("a configuration");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bound");
        let port = listener.local_addr().expect("an address").port();
        let elsewhere = web_address("ws://192.0.2.7:9/").expect("an address");
        let mapped = web_address("ws://[::ffff:192.0.2.7]:9/").expect("an address");
        let here = web_address(&format!("ws://127.0.0.1:{port}/")).expect("an address");

        let refused = connect(&elsewhere, &tls).await;
        let refused_mapped = connect(&mapped, &tls).await;
        let reached = on_this_machine(&here).await;

        assert!(matches!(refused, Err(Error::PlainOffThisMachine { .. })));
        assert!(matches!(
            refused_mapped,
            Err(Error::PlainOffThisMachine { .. })
        ));
        assert_eq!(
            reached.expect("resolved"),
            vec![SocketAddr::from(([127, 0, 0, 1], port))]
        );
    }

    async fn sent_to_a_server_reading_at_most(
        most: usize,
        sent: usize,
    ) -> (Result<Option<Vec<u8>>>, Outgoing) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bound");
        let port = listener.local_addr().expect("an address").port();
        let serving = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accepted");
            let mut accepted = accepted(stream, None).await.expect("upgraded");

            let heard = accepted.incoming.read_at_most(most).await;
            (heard, accepted.outgoing)
        });

        let address = web_address(&format!("ws://127.0.0.1:{port}/")).expect("an address");
        let tls = to_the_web(RootCertStore::empty()).expect("a configuration");

        let (mut outgoing, _incoming) = connect(&address, &tls).await.expect("connected");
        let _ = outgoing.send(&vec![7; sent]).await;

        let (heard, server_side) = serving.await.expect("the server task");
        (heard, server_side)
    }

    #[tokio::test]
    async fn a_budgeted_stream_stops_reading_once_its_budget_is_spent() {
        let (mut near, mut far) = duplex(4096);
        let (budget, spending) = Budget::spendable();

        let mut budgeted = Budgeted {
            inner: &mut near,
            spending,
        };

        budget.allow(40);
        far.write_all(&[1; 1000]).await.expect("written");

        let mut taken = 0;
        let mut buffer = [0; READ_AT_ONCE];

        let refused = loop {
            match budgeted.read(&mut buffer).await {
                Ok(read) => taken += read,
                Err(error) => break error,
            }
        };

        assert_eq!(refused.kind(), io::ErrorKind::InvalidData);
        assert!((40..40 + READ_AT_ONCE).contains(&taken), "{taken}");
    }

    #[tokio::test]
    async fn a_web_frame_past_what_the_server_reads_is_refused_and_one_within_it_is_heard() {
        let (past, _) = sent_to_a_server_reading_at_most(1024, 512 * 1024).await;
        let (within, _) = sent_to_a_server_reading_at_most(1024, 1024).await;

        assert!(past.is_err());
        assert_eq!(within.expect("heard"), Some(vec![7; 1024]));
    }
}
