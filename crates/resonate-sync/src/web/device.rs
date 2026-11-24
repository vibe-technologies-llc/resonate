use std::{net::SocketAddr, sync::Arc};

use rustls::{ClientConfig, RootCertStore, crypto::ring::default_provider, version::TLS13};
use rustls_pki_types::ServerName;
use tokio::net::{TcpStream, lookup_host};
use tokio_rustls::TlsConnector;
use tokio_tungstenite::{client_async_with_config, tungstenite::http::Uri};

use super::{Budget, Incoming, Outgoing, Tunnel, halves, limits};
use crate::{
    Error, Result, StreamOp, TlsOp,
    routes::{each_once, first_answering},
};

pub const WEB_SERVER_PORT: u16 = 443;
pub(super) const WEB_ALPN: &[u8] = b"http/1.1";

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

pub(super) async fn on_this_machine(address: &WebAddress) -> Result<Vec<SocketAddr>> {
    let resolved = resolved(address).await?;
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

pub async fn connect(
    address: &WebAddress,
    tls: &Arc<ClientConfig>,
) -> Result<(Outgoing, Incoming)> {
    let routes = if address.secure {
        resolved(address).await?
    } else {
        on_this_machine(address).await?
    };
    connect_through(address, &routes, tls).await
}

async fn resolved(address: &WebAddress) -> Result<Vec<SocketAddr>> {
    let resolved = lookup_host((address.host.as_str(), address.port))
        .await
        .map_err(|source| Error::Resolve {
            server: address.host.clone().into(),
            source,
        })?;
    Ok(each_once(resolved))
}

pub(super) async fn connect_through(
    address: &WebAddress,
    routes: &[SocketAddr],
    tls: &Arc<ClientConfig>,
) -> Result<(Outgoing, Incoming)> {
    let (halves, _) =
        first_answering(&address.host, routes, |route| opened(address, route, tls)).await?;
    Ok(halves)
}

async fn opened(
    address: &WebAddress,
    route: SocketAddr,
    tls: &Arc<ClientConfig>,
) -> Result<(Outgoing, Incoming)> {
    let tcp = TcpStream::connect(route)
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
