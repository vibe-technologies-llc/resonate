use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::Path,
    sync::{Arc, atomic::Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use parking_lot::Mutex;
use quinn::{Endpoint, Incoming, ServerConfig};
use resonate_sync::{
    DeviceId, Duplex, Incoming as Reader, Outgoing as Writer, SpaceId, accepted, transport,
    wire::{FromServer, LARGEST_RECORD, PROTOCOL_VERSION, Record, Refusal, SERVER_ALPN, ToServer},
};
use rustls::{crypto::ring::default_provider, version::TLS13};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use tokio::{
    net::TcpListener,
    sync::{
        Semaphore,
        mpsc::{UnboundedSender, unbounded_channel},
    },
    time::{Instant, interval, timeout},
};

use crate::{
    Error, PemOp, Result,
    guard::{Admitted, Allowance, Guard},
    hub::{Hub, Joined, Notice},
    relay::Bucket,
    store::{Appended, CaughtUp, Claim, Compacted, Store},
};

const HELLO_WITHIN: Duration = Duration::from_secs(10);
const KEEP_EVERY: Duration = Duration::from_secs(30);
const SILENT_FOR: Duration = Duration::from_secs(90);
const RECORDS_PER_FRAME: usize = 256;
const RECORD_BYTES_PER_FRAME: usize = 4 * 1024 * 1024;
const RATE_WINDOW: Duration = Duration::from_secs(60);
const LONGEST_SWEEP_WAIT: Duration = Duration::from_secs(60 * 60);
const UNUSED_SPACE_KEPT_FOR: i64 = 24 * 60 * 60;
const WEB_HANDSHAKES_AT_ONCE: usize = 256;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub record_bytes: usize,
    pub space_bytes: u64,
    pub devices: usize,
    pub frames_per_minute: u32,
    pub relay_bytes_per_second: u64,
    pub idle_for: Duration,
    pub allowance: Allowance,
}

struct Seat {
    space: SpaceId,
    device: DeviceId,
    after: u64,
    seen: SocketAddr,
}

pub struct Server {
    store: Arc<Mutex<Store>>,
    hub: Hub,
    limits: Limits,
    guard: Arc<Guard>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            record_bytes: LARGEST_RECORD,
            space_bytes: 64 * 1024 * 1024,
            devices: 16,
            frames_per_minute: 240,
            relay_bytes_per_second: 2 * 1024 * 1024,
            idle_for: Duration::from_secs(365 * 24 * 60 * 60),
            allowance: Allowance::default(),
        }
    }
}

pub fn tls(certificate: &Path, key: &Path) -> Result<ServerConfig> {
    let (chain, key) = pem(certificate, key)?;
    serving(chain, key)
}

pub fn pem(
    certificate: &Path,
    key: &Path,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let chain = CertificateDer::pem_file_iter(certificate)
        .and_then(Iterator::collect::<std::result::Result<Vec<_>, _>>)
        .map_err(|source| Error::Pem {
            op: PemOp::Certificate,
            path: certificate.to_path_buf(),
            source,
        })?;
    if chain.is_empty() {
        return Err(Error::NoCertificate {
            path: certificate.to_path_buf(),
        });
    }
    let key = PrivateKeyDer::from_pem_file(key).map_err(|source| Error::Pem {
        op: PemOp::Key,
        path: key.to_path_buf(),
        source,
    })?;
    Ok((chain, key))
}

pub fn serving(
    chain: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) -> Result<ServerConfig> {
    let mut tls = rustls::ServerConfig::builder_with_provider(Arc::new(default_provider()))
        .with_protocol_versions(&[&TLS13])
        .map_err(|source| Error::Tls(Box::new(source)))?
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .map_err(|source| Error::Tls(Box::new(source)))?;
    tls.alpn_protocols = vec![SERVER_ALPN.to_vec()];
    let crypto =
        quinn::crypto::rustls::QuicServerConfig::try_from(tls).map_err(|_| Error::QuicCrypto)?;
    let mut config = ServerConfig::with_crypto(Arc::new(crypto));
    config.transport_config(Arc::new(transport(KEEP_EVERY, SILENT_FOR)));
    Ok(config)
}

pub fn listening(config: ServerConfig, address: SocketAddr) -> Result<Endpoint> {
    Endpoint::server(config, address).map_err(Error::Listen)
}

impl Server {
    pub fn new(store: Store, limits: Limits) -> Arc<Self> {
        Arc::new(Self {
            store: Arc::new(Mutex::new(store)),
            hub: Hub::default(),
            guard: Guard::new(limits.allowance),
            limits,
        })
    }

    pub async fn serve(self: Arc<Self>, endpoint: Endpoint) {
        let sweeping = tokio::spawn(Arc::clone(&self).sweep());
        while let Some(incoming) = endpoint.accept().await {
            if !incoming.remote_address_validated() && incoming.may_retry() {
                let _ = incoming.retry();
                continue;
            }
            let Some(admitted) = self.guard.admit(incoming.remote_address().ip()) else {
                incoming.refuse();
                continue;
            };
            let server = Arc::clone(&self);
            tokio::spawn(async move {
                if let Err(error) = server.welcome(incoming, admitted).await {
                    tracing::debug!(%error, "a device's session ended");
                }
            });
        }
        sweeping.abort();
    }

    pub async fn serve_web(self: Arc<Self>, listener: TcpListener, forwarded_by: Option<String>) {
        let handshaking = Arc::new(Semaphore::new(WEB_HANDSHAKES_AT_ONCE));
        let forwarded_by: Option<Arc<str>> = forwarded_by.map(Into::into);
        loop {
            let Ok((stream, peer)) = listener.accept().await else {
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            };
            let admitted = match forwarded_by {
                Some(_) => None,
                None => match self.guard.admit(peer.ip()) {
                    Some(admitted) => Some(admitted),
                    None => continue,
                },
            };
            let Ok(permit) = Arc::clone(&handshaking).try_acquire_owned() else {
                continue;
            };
            let server = Arc::clone(&self);
            let forwarded_by = forwarded_by.clone();
            tokio::spawn(async move {
                let served = server
                    .welcome_web(stream, forwarded_by, admitted, permit)
                    .await;
                if let Err(error) = served {
                    tracing::debug!(%error, "a device's web session ended");
                }
            });
        }
    }

    async fn sweep(self: Arc<Self>) {
        let wait = (self.limits.idle_for / 30).min(LONGEST_SWEEP_WAIT);
        loop {
            tokio::time::sleep(wait).await;
            let idle = i64::try_from(self.limits.idle_for.as_secs()).unwrap_or(i64::MAX);
            let store = Arc::clone(&self.store);
            let unused = Arc::clone(&self.store);
            let _ = in_the_store(move || {
                unused
                    .lock()
                    .sweep_unused(now().saturating_sub(UNUSED_SPACE_KEPT_FOR))
            })
            .await;
            self.guard.forget_quiet_addresses();
            let swept = in_the_store(move || store.lock().sweep(now().saturating_sub(idle))).await;
            match swept {
                Ok(0) => {}
                Ok(swept) => tracing::info!(swept, "spaces left untouched were swept away"),
                Err(error) => tracing::warn!(%error, "the idle spaces could not be swept"),
            }
        }
    }

    async fn welcome(self: Arc<Self>, incoming: Incoming, admitted: Arc<Admitted>) -> Result<()> {
        let connection = incoming
            .await
            .map_err(|source| resonate_sync::Error::Connection(Box::new(source)))?;
        let address = connection.remote_address();
        let streams = timeout(HELLO_WITHIN, connection.accept_bi()).await;
        let Ok(Ok((send, recv))) = streams else {
            return Ok(());
        };
        self.hello(Writer::Quic(send), Reader::Quic(recv), address, admitted)
            .await
    }

    async fn welcome_web(
        self: Arc<Self>,
        stream: impl Duplex + 'static,
        forwarded_by: Option<Arc<str>>,
        admitted: Option<Arc<Admitted>>,
        handshake: tokio::sync::OwnedSemaphorePermit,
    ) -> Result<()> {
        let upgraded = timeout(HELLO_WITHIN, accepted(stream, forwarded_by.as_deref())).await;
        let Ok(upgraded) = upgraded else {
            return Ok(());
        };
        let upgraded = upgraded?;
        drop(handshake);
        let admitted = match (admitted, upgraded.forwarded_for) {
            (Some(admitted), _) => admitted,
            (None, Some(client)) => match self.guard.admit(client) {
                Some(admitted) => admitted,
                None => return Ok(()),
            },
            (None, None) => return Ok(()),
        };
        let unobserved = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);
        self.hello(upgraded.outgoing, upgraded.incoming, unobserved, admitted)
            .await
    }

    async fn hello(
        &self,
        mut send: Writer,
        mut recv: Reader,
        address: SocketAddr,
        admitted: Arc<Admitted>,
    ) -> Result<()> {
        let hello = timeout(HELLO_WITHIN, recv.read()).await;
        let Ok(Ok(Some(hello))) = hello else {
            return Ok(());
        };
        let Ok(ToServer::Hello {
            version,
            space,
            token,
            device,
            after,
        }) = ToServer::decoded(&hello)
        else {
            return Ok(());
        };
        if version != PROTOCOL_VERSION {
            return refuse(&mut send, Refusal::Version).await;
        }
        let store = Arc::clone(&self.store);
        let digest = token.digest();
        let making = Arc::clone(&admitted);
        let claim = in_the_store(move || {
            store
                .lock()
                .claim(space, &digest, now(), || making.may_make_a_space())
        })
        .await?;
        match claim {
            Claim::Refused => return refuse(&mut send, Refusal::Token).await,
            Claim::Throttled => return refuse(&mut send, Refusal::TooFast).await,
            Claim::Full => return refuse(&mut send, Refusal::Busy).await,
            Claim::Made => tracing::info!("a space was made"),
            Claim::Held => {}
        }
        let Some(joined) = self.hub.join(space, device, address, self.limits.devices) else {
            return refuse(&mut send, Refusal::Busy).await;
        };
        let session = joined.session;
        let seat = Seat {
            space,
            device,
            after,
            seen: address,
        };
        let served = self.session(send, recv, seat, joined).await;
        self.hub.leave(space, device, session);
        served
    }

    async fn session(
        &self,
        mut send: Writer,
        recv: Reader,
        seat: Seat,
        joined: Joined,
    ) -> Result<()> {
        let Seat {
            space,
            device,
            after,
            seen,
        } = seat;
        let Joined {
            mut notices,
            others,
            session,
            backlog,
        } = joined;
        let store = Arc::clone(&self.store);
        let (head, _) = in_the_store(move || store.lock().head(space)).await?;
        let welcome = FromServer::Welcome {
            online: others,
            seen_at: seen,
            head,
        };
        send.send(&welcome.encoded()).await?;

        let mut sent_through = after;
        self.send_records(&mut send, space, device, &mut sent_through)
            .await?;

        let (frames_in, mut frames) = unbounded_channel();
        let reading = tokio::spawn(read_frames(recv, frames_in));
        let mut window = Instant::now();
        let mut counted = 0_u32;
        let mut allowance = Bucket::new(self.limits.relay_bytes_per_second);
        let mut keeping = interval(KEEP_EVERY);
        let ended = loop {
            tokio::select! {
                frame = frames.recv() => {
                    let Some(frame) = frame else {
                        break Ok(());
                    };
                    let Ok(frame) = frame else {
                        break Ok(());
                    };
                    let frame = match frame {
                        ToServer::Relaying => {
                            if self.limits.relay_bytes_per_second > 0 {
                                self.hub.relaying(space, device, session);
                            }
                            continue;
                        }
                        ToServer::Relay { to, datagram } => {
                            if allowance.spend(datagram.len()) {
                                self.hub.relay(space, device, to, datagram);
                            }
                            continue;
                        }
                        frame => frame,
                    };
                    if window.elapsed() >= RATE_WINDOW {
                        window = Instant::now();
                        counted = 0;
                    }
                    counted += 1;
                    if counted > self.limits.frames_per_minute {
                        break refuse(&mut send, Refusal::TooFast).await;
                    }
                    match frame {
                        ToServer::Bye => break Ok(()),
                        ToServer::Hello { .. } | ToServer::Relaying | ToServer::Relay { .. } => {}
                        ToServer::Append { blobs } => self.append(&mut send, space, device, blobs).await?,
                        ToServer::Compact { through, parts } => self.compact(&mut send, space, through, parts).await?,
                        ToServer::Punch { to, sealed } => {
                            if !self.hub.punch(space, device, to, sealed) {
                                tracing::debug!("a punch named a device that is not here");
                            }
                        }
                    }
                }
                _ = keeping.tick(), if send.is_web() => send.keep_alive().await?,
                notice = notices.recv() => match notice {
                    None | Some(Notice::Replaced) => break Ok(()),
                    Some(Notice::Appended) => self.send_records(&mut send, space, device, &mut sent_through).await?,
                    Some(Notice::Arrived(other)) => send.send(&FromServer::Arrived { device: other }.encoded()).await?,
                    Some(Notice::Left(other)) => send.send(&FromServer::Left { device: other }.encoded()).await?,
                    Some(Notice::Relayed { from, datagram }) => {
                        backlog.fetch_sub(datagram.len(), Ordering::Relaxed);
                        send.send(&FromServer::Relayed { from, datagram }.encoded()).await?;
                    }
                    Some(Notice::Punched { device: from, address, sealed }) => {
                        let frame = FromServer::Punched { device: from, address, sealed };
                        send.send(&frame.encoded()).await?;
                    }
                    Some(Notice::Compacted(through)) => {
                        self.send_records(&mut send, space, device, &mut sent_through).await?;
                        send.send(&FromServer::Compacted { through }.encoded()).await?;
                    }
                },
            }
        };
        reading.abort();
        send.finish().await;
        ended
    }

    async fn append(
        &self,
        send: &mut Writer,
        space: SpaceId,
        device: DeviceId,
        blobs: Vec<Vec<u8>>,
    ) -> Result<()> {
        if blobs
            .iter()
            .any(|blob| blob.len() > self.limits.record_bytes)
        {
            return refuse(send, Refusal::TooLarge).await;
        }
        let count = u32::try_from(blobs.len()).unwrap_or(u32::MAX);
        let store = Arc::clone(&self.store);
        let space_bytes = self.limits.space_bytes;
        let appended =
            in_the_store(move || store.lock().append(space, device, &blobs, space_bytes)).await?;
        match appended {
            Appended::Full => refuse(send, Refusal::SpaceFull).await,
            Appended::Through(_) => {
                send.send(&FromServer::Appended { count }.encoded()).await?;
                self.hub.tell_all(space, &Notice::Appended);
                Ok(())
            }
        }
    }

    async fn compact(
        &self,
        send: &mut Writer,
        space: SpaceId,
        through: u64,
        parts: Vec<Vec<u8>>,
    ) -> Result<()> {
        let store = Arc::clone(&self.store);
        let space_bytes = self.limits.space_bytes;
        let compacted =
            in_the_store(move || store.lock().compact(space, through, &parts, space_bytes)).await?;
        match compacted {
            Compacted::Through(through) => {
                self.hub.tell_all(space, &Notice::Compacted(through));
                Ok(())
            }
            Compacted::Stale => refuse(send, Refusal::Stale).await,
            Compacted::Full => refuse(send, Refusal::SpaceFull).await,
        }
    }

    async fn send_records(
        &self,
        send: &mut Writer,
        space: SpaceId,
        device: DeviceId,
        sent_through: &mut u64,
    ) -> Result<()> {
        loop {
            let store = Arc::clone(&self.store);
            let after = *sent_through;
            let caught = in_the_store(move || {
                store
                    .lock()
                    .catch_up(space, after, RECORDS_PER_FRAME, RECORD_BYTES_PER_FRAME)
            })
            .await?;
            match caught {
                CaughtUp::Snapshot(snapshot) => {
                    let through = snapshot.through;
                    let frame = FromServer::Snapshot {
                        through,
                        parts: snapshot.parts,
                    };
                    send.send(&frame.encoded()).await?;
                    *sent_through = through;
                }
                CaughtUp::Records(stored) => {
                    let Some(last) = stored.last().map(|record| record.seq) else {
                        return Ok(());
                    };
                    let entries = stored
                        .into_iter()
                        .map(|record| Record {
                            seq: record.seq,
                            blob: (record.device != device).then_some(record.blob),
                        })
                        .collect();
                    send.send(&FromServer::Records { entries }.encoded())
                        .await?;
                    *sent_through = last;
                }
            }
        }
    }
}

async fn refuse(send: &mut Writer, refusal: Refusal) -> Result<()> {
    send.send(&FromServer::Refused(refusal).encoded()).await?;
    Ok(())
}

async fn read_frames(mut recv: Reader, frames: UnboundedSender<Result<ToServer>>) {
    loop {
        let frame = match recv.read().await {
            Ok(Some(bytes)) => ToServer::decoded(&bytes).map_err(Error::from),
            Ok(None) => return,
            Err(error) => Err(Error::from(error)),
        };
        let failed = frame.is_err();
        if frames.send(frame).is_err() || failed {
            return;
        }
    }
}

async fn in_the_store<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| Error::Worker)?
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| i64::try_from(since.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}
