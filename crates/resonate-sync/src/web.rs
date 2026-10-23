use std::{net::IpAddr, sync::Arc, time::Duration};

use futures_util::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use quinn::{RecvStream, SendStream};
use rustls::{ClientConfig, RootCertStore, crypto::ring::default_provider, version::TLS13};
use rustls_pki_types::ServerName;
use tokio::{
    io::{AsyncRead, AsyncWrite},
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
    quic::{read_frame, send_frame},
    wire::LARGEST_FRAME,
};

pub const WEB_SERVER_PORT: u16 = 443;
pub const WEB_SILENT_FOR: Duration = Duration::from_secs(90);
const WEB_ALPN: &[u8] = b"http/1.1";

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
    Web(SplitStream<Socket>),
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

fn halves(socket: Socket) -> (Outgoing, Incoming) {
    let (sink, stream) = socket.split();
    (Outgoing::Web(sink), Incoming::Web(stream))
}

pub async fn connect(
    address: &WebAddress,
    tls: &Arc<ClientConfig>,
) -> Result<(Outgoing, Incoming)> {
    let tcp = TcpStream::connect((address.host.as_str(), address.port))
        .await
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
    Ok(halves(socket))
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
    let tunnel: Tunnel = Box::new(stream);
    let socket = accept_hdr_async_with_config(tunnel, &mut forwarding, Some(limits()))
        .await
        .map_err(Error::web)?;
    let (outgoing, incoming) = halves(socket);
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
        match self {
            Self::Quic(stream) => read_frame(stream).await,
            Self::Web(stream) => loop {
                let heard = timeout(WEB_SILENT_FOR, stream.next())
                    .await
                    .map_err(|_| Error::ServerSilent)?;
                match heard {
                    None
                    | Some(Ok(Message::Close(_)))
                    | Some(Err(
                        tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed,
                    )) => return Ok(None),
                    Some(Ok(Message::Binary(payload))) => return Ok(Some(payload.to_vec())),
                    Some(Ok(_)) => {}
                    Some(Err(error)) => return Err(Error::web(error)),
                }
            },
        }
    }
}
