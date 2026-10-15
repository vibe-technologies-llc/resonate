use std::{
    net::SocketAddr,
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use parking_lot::Mutex;
use quinn::{Connection, Endpoint, Incoming, RecvStream, SendStream, ServerConfig};
use resonate_sync::{
    DeviceId, SpaceId, read_frame, send_frame, transport,
    wire::{FromServer, LARGEST_RECORD, PROTOCOL_VERSION, Record, Refusal, SERVER_ALPN, ToServer},
};
use rustls::{crypto::ring::default_provider, version::TLS13};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use tokio::{
    sync::mpsc::{UnboundedSender, unbounded_channel},
    time::{Instant, timeout},
};

use crate::{
    Error, PemOp, Result,
    hub::{Hub, Joined, Notice},
    store::{Appended, CaughtUp, Claim, Compacted, Store},
};

const HELLO_WITHIN: Duration = Duration::from_secs(10);
const KEEP_EVERY: Duration = Duration::from_secs(30);
const SILENT_FOR: Duration = Duration::from_secs(90);
const RECORDS_PER_FRAME: usize = 256;
const RECORD_BYTES_PER_FRAME: usize = 4 * 1024 * 1024;
const RATE_WINDOW: Duration = Duration::from_secs(60);
const LONGEST_SWEEP_WAIT: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub record_bytes: usize,
    pub space_bytes: u64,
    pub devices: usize,
    pub frames_per_minute: u32,
    pub idle_for: Duration,
}

struct Seat {
    space: SpaceId,
    device: DeviceId,
    after: u64,
}

pub struct Server {
    store: Arc<Mutex<Store>>,
    hub: Hub,
    limits: Limits,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            record_bytes: LARGEST_RECORD,
            space_bytes: 64 * 1024 * 1024,
            devices: 16,
            frames_per_minute: 240,
            idle_for: Duration::from_secs(365 * 24 * 60 * 60),
        }
    }
}

pub fn tls(certificate: &Path, key: &Path) -> Result<ServerConfig> {
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
    serving(chain, key)
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
            limits,
        })
    }

    pub async fn serve(self: Arc<Self>, endpoint: Endpoint) {
        let sweeping = tokio::spawn(Arc::clone(&self).sweep());
        while let Some(incoming) = endpoint.accept().await {
            let server = Arc::clone(&self);
            tokio::spawn(async move {
                if let Err(error) = server.welcome(incoming).await {
                    tracing::debug!(%error, "a device's session ended");
                }
            });
        }
        sweeping.abort();
    }

    async fn sweep(self: Arc<Self>) {
        let wait = (self.limits.idle_for / 30).min(LONGEST_SWEEP_WAIT);
        loop {
            tokio::time::sleep(wait).await;
            let idle = i64::try_from(self.limits.idle_for.as_secs()).unwrap_or(i64::MAX);
            let store = Arc::clone(&self.store);
            let swept = in_the_store(move || store.lock().sweep(now().saturating_sub(idle))).await;
            match swept {
                Ok(0) => {}
                Ok(swept) => tracing::info!(swept, "spaces left untouched were swept away"),
                Err(error) => tracing::warn!(%error, "the idle spaces could not be swept"),
            }
        }
    }

    async fn welcome(self: Arc<Self>, incoming: Incoming) -> Result<()> {
        let connection = incoming
            .await
            .map_err(|source| resonate_sync::Error::Connection(Box::new(source)))?;
        let address = connection.remote_address();
        let streams = timeout(HELLO_WITHIN, connection.accept_bi()).await;
        let Ok(Ok((mut send, mut recv))) = streams else {
            return Ok(());
        };
        let hello = timeout(HELLO_WITHIN, read_frame(&mut recv)).await;
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
        let claim = in_the_store(move || store.lock().claim(space, &digest, now())).await?;
        if claim == Claim::Refused {
            return refuse(&mut send, Refusal::Token).await;
        }
        if claim == Claim::Made {
            tracing::info!("a space was made");
        }
        let Some(joined) = self.hub.join(space, device, address, self.limits.devices) else {
            return refuse(&mut send, Refusal::Busy).await;
        };
        let session = joined.session;
        let seat = Seat {
            space,
            device,
            after,
        };
        let served = self.session(&connection, send, recv, seat, joined).await;
        self.hub.leave(space, device, session);
        served
    }

    async fn session(
        &self,
        connection: &Connection,
        mut send: SendStream,
        recv: RecvStream,
        seat: Seat,
        joined: Joined,
    ) -> Result<()> {
        let Seat {
            space,
            device,
            after,
        } = seat;
        let Joined {
            mut notices,
            others,
            ..
        } = joined;
        let store = Arc::clone(&self.store);
        let (head, _) = in_the_store(move || store.lock().head(space)).await?;
        let welcome = FromServer::Welcome {
            online: others,
            seen_at: connection.remote_address(),
            head,
        };
        send_frame(&mut send, &welcome.encoded()).await?;

        let mut sent_through = after;
        self.send_records(&mut send, space, device, &mut sent_through)
            .await?;

        let (frames_in, mut frames) = unbounded_channel();
        let reading = tokio::spawn(read_frames(recv, frames_in));
        let mut window = Instant::now();
        let mut counted = 0_u32;
        let ended = loop {
            tokio::select! {
                frame = frames.recv() => {
                    let Some(frame) = frame else {
                        break Ok(());
                    };
                    let Ok(frame) = frame else {
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
                        ToServer::Hello { .. } => {}
                        ToServer::Append { blobs } => self.append(&mut send, space, device, blobs).await?,
                        ToServer::Compact { through, parts } => self.compact(&mut send, space, through, parts).await?,
                        ToServer::Punch { to, sealed } => {
                            if !self.hub.punch(space, device, to, sealed) {
                                tracing::debug!("a punch named a device that is not here");
                            }
                        }
                    }
                }
                notice = notices.recv() => match notice {
                    None | Some(Notice::Replaced) => break Ok(()),
                    Some(Notice::Appended) => self.send_records(&mut send, space, device, &mut sent_through).await?,
                    Some(Notice::Arrived(other)) => send_frame(&mut send, &FromServer::Arrived { device: other }.encoded()).await?,
                    Some(Notice::Left(other)) => send_frame(&mut send, &FromServer::Left { device: other }.encoded()).await?,
                    Some(Notice::Punched { device: from, address, sealed }) => {
                        let frame = FromServer::Punched { device: from, address, sealed };
                        send_frame(&mut send, &frame.encoded()).await?;
                    }
                    Some(Notice::Compacted(through)) => {
                        self.send_records(&mut send, space, device, &mut sent_through).await?;
                        send_frame(&mut send, &FromServer::Compacted { through }.encoded()).await?;
                    }
                },
            }
        };
        reading.abort();
        let _ = send.finish();
        ended
    }

    async fn append(
        &self,
        send: &mut SendStream,
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
                send_frame(send, &FromServer::Appended { count }.encoded()).await?;
                self.hub.tell_all(space, &Notice::Appended);
                Ok(())
            }
        }
    }

    async fn compact(
        &self,
        send: &mut SendStream,
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
        send: &mut SendStream,
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
                    send_frame(send, &frame.encoded()).await?;
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
                    send_frame(send, &FromServer::Records { entries }.encoded()).await?;
                    *sent_through = last;
                }
            }
        }
    }
}

async fn refuse(send: &mut SendStream, refusal: Refusal) -> Result<()> {
    send_frame(send, &FromServer::Refused(refusal).encoded()).await?;
    Ok(())
}

async fn read_frames(mut recv: RecvStream, frames: UnboundedSender<Result<ToServer>>) {
    loop {
        let frame = match read_frame(&mut recv).await {
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
