use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::Path,
    sync::{Arc, atomic::Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use parking_lot::Mutex;
use quinn::{Endpoint, Incoming, ServerConfig};
use resonate_sync::{
    DeviceId, Duplex, Incoming as Reader, Outgoing as Writer, SpaceId, Token, accepted, transport,
    wire::{
        FromServer, LARGEST_RECORD, PROTOCOL_VERSION, Record, Refusal, SERVER_ALPN, SpaceAsked,
        ToServer,
    },
};
use rustls::{crypto::ring::default_provider, version::TLS13};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    sync::{
        Semaphore,
        mpsc::{Sender, channel},
    },
    time::{Instant, interval, timeout},
};

use crate::{
    Error, PemOp, Result,
    guard::{Admitted, Allowance, Guard},
    hub::{Hub, Joined, Notice, relayed_weight},
    pages,
    relay::Bucket,
    rooms::{self, JoinedRoom, Leaving, RoomLimits, RoomNotice, Rooms},
    store::{Appended, CaughtUp, Claim, Compacted, Store},
};

const HELLO_WITHIN: Duration = Duration::from_secs(10);
const KEEP_EVERY: Duration = Duration::from_secs(30);
const SILENT_FOR: Duration = Duration::from_secs(90);
const RECORDS_PER_FRAME: usize = 256;
const RECORD_BYTES_PER_FRAME: usize = 4 * 1024 * 1024;
const FRAMES_READ_AHEAD: usize = 4;
const RATE_WINDOW: Duration = Duration::from_secs(60);
const A_PAGE_ASKED_AGAIN_AFTER: Duration = Duration::from_millis(5);
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
    pub rooms: usize,
    pub room_members: usize,
    pub host_back_within: Duration,
    pub room_bytes_per_second: u64,
}

struct Knock {
    room: SpaceId,
    token: Token,
    host: Option<Token>,
    member: DeviceId,
}

struct Door {
    version: u16,
    space: SpaceId,
    token: Token,
    viewer: Option<[u8; 32]>,
    device: DeviceId,
    after: u64,
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
    rooms: Arc<Rooms>,
    began: Instant,
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
            rooms: 1024,
            room_members: 32,
            host_back_within: Duration::from_secs(30),
            room_bytes_per_second: 512 * 1024,
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
            rooms: Arc::new(Rooms::default()),
            began: Instant::now(),
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
        self.serve_web_claiming(listener, forwarded_by, Vec::new())
            .await;
    }

    pub async fn serve_web_claiming(
        self: Arc<Self>,
        listener: TcpListener,
        forwarded_by: Option<String>,
        android_certificates: Vec<String>,
    ) {
        let handshaking = Arc::new(Semaphore::new(WEB_HANDSHAKES_AT_ONCE));
        let forwarded_by: Option<Arc<str>> = forwarded_by.map(Into::into);
        let android_certificates: Arc<[String]> = android_certificates.into();
        loop {
            let Ok((stream, peer)) = listener.accept().await else {
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            };
            if let Err(error) = stream.set_nodelay(true) {
                tracing::debug!(%error, "a connection could not leave Nagle's algorithm");
            }
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
            let certificates = Arc::clone(&android_certificates);
            tokio::spawn(async move {
                let Some(stream) = answered_as_a_page(stream, &certificates).await else {
                    return;
                };
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
        let Ok(first) = ToServer::decoded(&hello) else {
            return Ok(());
        };

        match first {
            ToServer::Hello {
                version,
                space,
                token,
                device,
                after,
            } => {
                let door = Door {
                    version,
                    space,
                    token,
                    viewer: None,
                    device,
                    after,
                };
                self.enter(send, recv, door, address, admitted).await
            }
            ToServer::Share {
                version,
                space,
                token,
                viewer,
                device,
                after,
            } => {
                let door = Door {
                    version,
                    space,
                    token,
                    viewer: Some(viewer),
                    device,
                    after,
                };
                self.enter(send, recv, door, address, admitted).await
            }
            ToServer::View {
                version,
                space,
                token,
                device,
                after,
            } => {
                let door = Door {
                    version,
                    space,
                    token,
                    viewer: None,
                    device,
                    after,
                };
                self.view(send, door, address).await
            }
            ToServer::Heads { version, asked } => self.heads(send, version, asked).await,
            ToServer::Room {
                version,
                room,
                token,
                host,
                member,
            } => {
                if version != PROTOCOL_VERSION {
                    return refuse(&mut send, Refusal::Version).await;
                }
                let knock = Knock {
                    room,
                    token,
                    host,
                    member,
                };
                self.room(send, recv, knock, &admitted).await
            }
            ToServer::Append { .. }
            | ToServer::Compact { .. }
            | ToServer::Punch { .. }
            | ToServer::Bye
            | ToServer::Relaying
            | ToServer::Relay { .. }
            | ToServer::Close { .. }
            | ToServer::RoomSay { .. }
            | ToServer::RoomPing { .. }
            | ToServer::RoomKick { .. } => Ok(()),
        }
    }

    async fn enter(
        &self,
        mut send: Writer,
        recv: Reader,
        door: Door,
        address: SocketAddr,
        admitted: Arc<Admitted>,
    ) -> Result<()> {
        let Door {
            version,
            space,
            token,
            viewer,
            device,
            after,
        } = door;
        if version != PROTOCOL_VERSION {
            return refuse(&mut send, Refusal::Version).await;
        }
        let store = Arc::clone(&self.store);
        let digest = token.digest();
        let making = Arc::clone(&admitted);
        let claim = in_the_store(move || {
            let mut store = store.lock();
            let claim = store.claim(space, &digest, now(), || match viewer {
                Some(_) => making.may_make_a_shared_space(),
                None => making.may_make_a_space(),
            })?;
            if let (Some(viewer), Claim::Made | Claim::Held) = (viewer.as_ref(), claim) {
                store.allow_viewers(space, viewer)?;
            }
            Ok(claim)
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

    async fn view(&self, mut send: Writer, door: Door, address: SocketAddr) -> Result<()> {
        let Door {
            version,
            space,
            token,
            device,
            after,
            ..
        } = door;
        if version != PROTOCOL_VERSION {
            return refuse(&mut send, Refusal::Version).await;
        }
        let store = Arc::clone(&self.store);
        let digest = token.digest();
        let (may, head, closed) = in_the_store(move || {
            let mut store = store.lock();
            if !store.may_view(space, &digest, now())? {
                return Ok((false, 0, None));
            }
            let (head, _) = store.head(space)?;

            Ok((true, head, store.closed(space)?))
        })
        .await?;
        if !may {
            return refuse(&mut send, Refusal::Token).await;
        }

        let welcome = FromServer::Welcome {
            online: Vec::new(),
            seen_at: address,
            head,
        };
        send.send(&welcome.encoded()).await?;
        let mut sent_through = after;
        self.send_records(&mut send, space, device, &mut sent_through)
            .await?;
        if let Some(sealed) = closed {
            send.send(&FromServer::Closed { sealed }.encoded()).await?;
        }
        send.finish().await;

        Ok(())
    }

    async fn heads(&self, mut send: Writer, version: u16, asked: Vec<SpaceAsked>) -> Result<()> {
        if version != PROTOCOL_VERSION {
            return refuse(&mut send, Refusal::Version).await;
        }
        let store = Arc::clone(&self.store);
        let heads = in_the_store(move || store.lock().heads(&asked, now())).await?;
        send.send(&FromServer::Heads { heads }.encoded()).await?;
        send.finish().await;

        Ok(())
    }

    async fn room(
        &self,
        mut send: Writer,
        recv: Reader,
        knock: Knock,
        admitted: &Arc<Admitted>,
    ) -> Result<()> {
        let Knock {
            room,
            token,
            host,
            member,
        } = knock;
        let digest = token.digest();
        let host_digest = host.as_ref().map(Token::digest);
        let asked = rooms::Asked {
            room,
            token: &digest,
            host_token: host_digest.as_ref(),
            member,
        };
        let limits = RoomLimits {
            rooms: self.limits.rooms,
            members: self.limits.room_members,
        };
        let joined = match self
            .rooms
            .join(&asked, &limits, || admitted.may_host_a_room())
        {
            Ok(joined) => joined,
            Err(refusal) => return refuse(&mut send, refusal).await,
        };
        let session = joined.session;

        let served = self.in_the_room(send, recv, room, member, joined).await;

        if self.rooms.leave(room, member, session) == Leaving::TheHostLeft {
            let server_rooms = Arc::clone(&self.rooms);
            let grace = self.limits.host_back_within;
            tokio::spawn(async move {
                tokio::time::sleep(grace).await;
                server_rooms.end_unless_the_host_came_back(room);
            });
        }

        served
    }

    async fn in_the_room(
        &self,
        mut send: Writer,
        recv: Reader,
        room: SpaceId,
        member: DeviceId,
        joined: JoinedRoom,
    ) -> Result<()> {
        let JoinedRoom {
            mut notices,
            session,
            members,
            host,
            backlog,
            ..
        } = joined;
        send.send(&FromServer::RoomWelcome { members, host }.encoded())
            .await?;

        let (frames_in, mut frames) = channel(FRAMES_READ_AHEAD);
        let reading = tokio::spawn(read_frames(recv, frames_in));
        let mut window = Instant::now();
        let mut counted = 0_u32;
        let mut allowance = Bucket::new(self.limits.room_bytes_per_second);
        let mut keeping = interval(KEEP_EVERY);
        let ended = loop {
            tokio::select! {
                frame = frames.recv() => {
                    let Some(Ok(frame)) = frame else {
                        break Ok(());
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
                        ToServer::RoomSay { to, sealed } => {
                            if allowance.spend(sealed.len()) {
                                self.rooms.say(room, member, session, to, &sealed);
                            }
                        }
                        ToServer::RoomPing { nonce } => {
                            let at_nanos = u64::try_from(self.began.elapsed().as_nanos())
                                .unwrap_or(u64::MAX);
                            send.send(&FromServer::RoomPong { nonce, at_nanos }.encoded()).await?;
                        }
                        ToServer::RoomKick { member: kicked } => {
                            self.rooms.kick(room, session, kicked);
                        }
                        ToServer::Close { .. } => self.rooms.end(room, session),
                        ToServer::Hello { .. }
                        | ToServer::Share { .. }
                        | ToServer::View { .. }
                        | ToServer::Heads { .. }
                        | ToServer::Room { .. }
                        | ToServer::Append { .. }
                        | ToServer::Compact { .. }
                        | ToServer::Punch { .. }
                        | ToServer::Relaying
                        | ToServer::Relay { .. } => {}
                    }
                }
                _ = keeping.tick(), if send.is_web() => send.keep_alive().await?,
                notice = notices.recv() => match notice {
                    None | Some(RoomNotice::Replaced) => break Ok(()),
                    Some(RoomNotice::Ended) => {
                        send.send(&FromServer::RoomEnded.encoded()).await?;
                        break Ok(());
                    }
                    Some(RoomNotice::Arrived(arrived)) => {
                        send.send(&FromServer::RoomArrived { member: arrived }.encoded()).await?;
                    }
                    Some(RoomNotice::Left(left)) => {
                        send.send(&FromServer::RoomLeft { member: left }.encoded()).await?;
                    }
                    Some(RoomNotice::Heard { from, sealed }) => {
                        backlog.fetch_sub(sealed.len(), Ordering::Relaxed);
                        send.send(&FromServer::RoomHeard { from, sealed }.encoded()).await?;
                    }
                },
            }
        };
        reading.abort();
        send.finish().await;

        ended
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
        let store = Arc::clone(&self.store);
        if let Some(sealed) = in_the_store(move || store.lock().closed(space)).await? {
            send.send(&FromServer::Closed { sealed }.encoded()).await?;
        }

        let (frames_in, mut frames) = channel(FRAMES_READ_AHEAD);
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
                        ToServer::Relay { to, datagram } => {
                            if allowance.spend(relayed_weight(datagram.len())) {
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
                        ToServer::Hello { .. }
                        | ToServer::Share { .. }
                        | ToServer::View { .. }
                        | ToServer::Heads { .. }
                        | ToServer::Room { .. }
                        | ToServer::RoomSay { .. }
                        | ToServer::RoomPing { .. }
                        | ToServer::RoomKick { .. }
                        | ToServer::Relay { .. } => {}
                        ToServer::Relaying => {
                            if self.limits.relay_bytes_per_second > 0 {
                                self.hub.relaying(space, device, session);
                            }
                        }
                        ToServer::Close { sealed } => self.close(&mut send, space, sealed).await?,
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
                        backlog.fetch_sub(relayed_weight(datagram.len()), Ordering::Relaxed);
                        send.send(&FromServer::Relayed { from, datagram }.encoded()).await?;
                    }
                    Some(Notice::Punched { device: from, address, sealed }) => {
                        let frame = FromServer::Punched { device: from, address, sealed };
                        send.send(&frame.encoded()).await?;
                    }
                    Some(Notice::Closed(sealed)) => {
                        send.send(&FromServer::Closed { sealed }.encoded()).await?;
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
            Appended::Closed => self.told_it_closed(send, space).await,
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
            Compacted::Closed => self.told_it_closed(send, space).await,
        }
    }

    async fn close(&self, send: &mut Writer, space: SpaceId, sealed: Vec<u8>) -> Result<()> {
        if sealed.len() > self.limits.record_bytes {
            return refuse(send, Refusal::TooLarge).await;
        }
        let store = Arc::clone(&self.store);
        let kept = in_the_store(move || store.lock().close(space, &sealed)).await?;
        self.hub.tell_all(space, &Notice::Closed(kept));

        Ok(())
    }

    async fn told_it_closed(&self, send: &mut Writer, space: SpaceId) -> Result<()> {
        let store = Arc::clone(&self.store);
        if let Some(sealed) = in_the_store(move || store.lock().closed(space)).await? {
            send.send(&FromServer::Closed { sealed }.encoded()).await?;
        }

        Ok(())
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

async fn answered_as_a_page(mut stream: TcpStream, certificates: &[String]) -> Option<TcpStream> {
    let asked = timeout(HELLO_WITHIN, page_asked_of(&stream))
        .await
        .ok()
        .flatten();
    let Some(asked) = asked else {
        return Some(stream);
    };

    let answer = pages::answer(asked, certificates);
    if let Ok(Ok(())) = timeout(HELLO_WITHIN, stream.write_all(&answer)).await {
        let _ = stream.shutdown().await;
    }

    None
}

async fn page_asked_of(stream: &TcpStream) -> Option<pages::Asked> {
    let mut seen = vec![0; pages::reads_at_most()];
    loop {
        let read = stream.peek(&mut seen).await.ok()?;
        if read == 0 {
            return None;
        }
        if seen[..read].contains(&b'\n') || read == seen.len() {
            return pages::asked_of(&seen[..read]);
        }
        tokio::time::sleep(A_PAGE_ASKED_AGAIN_AFTER).await;
    }
}

async fn refuse(send: &mut Writer, refusal: Refusal) -> Result<()> {
    send.send(&FromServer::Refused(refusal).encoded()).await?;
    Ok(())
}

async fn read_frames(mut recv: Reader, frames: Sender<Result<ToServer>>) {
    loop {
        let frame = match recv.read().await {
            Ok(Some(bytes)) => ToServer::decoded(&bytes).map_err(Error::from),
            Ok(None) => return,
            Err(error) => Err(Error::from(error)),
        };
        let failed = frame.is_err();
        if frames.send(frame).await.is_err() || failed {
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
