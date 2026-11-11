use std::{
    io,
    net::IpAddr,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};

use futures_util::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use quinn::{RecvStream, SendStream};
use rustls::{ClientConfig, RootCertStore, crypto::ring::default_provider, version::TLS13};
use rustls_pki_types::ServerName;
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::TcpStream,
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
    quic::{read_frame_at_most, send_frame},
    wire::LARGEST_FRAME,
};

pub const WEB_SERVER_PORT: u16 = 443;
pub const WEB_SILENT_FOR: Duration = Duration::from_secs(90);
const WEB_ALPN: &[u8] = b"http/1.1";
const WEB_FRAMING_AT_MOST: usize = 64;

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

#[derive(Clone, Default)]
pub struct Budget(Option<Arc<AtomicUsize>>);

impl Budget {
    fn spendable() -> (Self, Arc<AtomicUsize>) {
        let left = Arc::new(AtomicUsize::new(usize::MAX));
        (Self(Some(Arc::clone(&left))), left)
    }

    fn allow(&self, bytes: usize) {
        if let Some(left) = &self.0 {
            left.store(bytes, Ordering::Relaxed);
        }
    }
}

struct Budgeted<S> {
    inner: S,
    left: Arc<AtomicUsize>,
}

impl<S: AsyncRead + Unpin> AsyncRead for Budgeted<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.left.load(Ordering::Relaxed) == 0 {
            return Poll::Ready(Err(io::Error::from(io::ErrorKind::InvalidData)));
        }
        let before = buffer.filled().len();
        let polled = Pin::new(&mut self.inner).poll_read(context, buffer);
        if polled.is_ready() {
            let read = buffer.filled().len() - before;
            let _ = self
                .left
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                    Some(left.saturating_sub(read))
                });
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

fn halves(socket: Socket, budget: Budget) -> (Outgoing, Incoming) {
    let (sink, stream) = socket.split();
    (Outgoing::Web(sink), Incoming::Web(stream, budget))
}

pub async fn connect(
    address: &WebAddress,
    tls: &Arc<ClientConfig>,
) -> Result<(Outgoing, Incoming)> {
    let tcp = TcpStream::connect((address.host.as_str(), address.port))
        .await
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
    let (budget, left) = Budget::spendable();
    let tunnel: Tunnel = Box::new(Budgeted {
        inner: stream,
        left,
    });
    let socket = accept_hdr_async_with_config(tunnel, &mut forwarding, Some(limits()))
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
        match self {
            Self::Quic(stream) => read_frame_at_most(stream, most).await,
            Self::Web(stream, budget) => {
                budget.allow(most.saturating_add(WEB_FRAMING_AT_MOST));
                loop {
                    let heard = timeout(WEB_SILENT_FOR, stream.next())
                        .await
                        .map_err(|_| Error::ServerSilent)?;
                    match heard {
                        None
                        | Some(Ok(Message::Close(_)))
                        | Some(Err(
                            tungstenite::Error::ConnectionClosed
                            | tungstenite::Error::AlreadyClosed,
                        )) => return Ok(None),
                        Some(Ok(Message::Binary(payload))) if payload.len() > most => {
                            return Err(Error::Oversized {
                                length: payload.len(),
                            });
                        }
                        Some(Ok(Message::Binary(payload))) => return Ok(Some(payload.to_vec())),
                        Some(Ok(_)) => {}
                        Some(Err(error)) => return Err(Error::web(error)),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt, duplex},
        net::TcpListener,
    };

    use super::*;

    const READ_AT_ONCE: usize = 16;

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
        let (budget, left) = Budget::spendable();
        let mut budgeted = Budgeted {
            inner: &mut near,
            left,
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
