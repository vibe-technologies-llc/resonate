use std::{
    collections::BTreeMap,
    io::{Read, Seek, SeekFrom},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use parking_lot::{Mutex, RwLock};
use quinn::{ClientConfig, Connection, Endpoint, RecvStream, SendStream};
use resonate_core::{
    Mbid, Recording, RemoteTrust, SharedLyricsfile, SharedPrint, WholeFile, ZoneId,
};
use tokio::{
    fs,
    io::AsyncWriteExt,
    sync::mpsc::{self, UnboundedSender},
    task::JoinSet,
    time::timeout,
};

use crate::{
    Catalog, Change, DeviceId, Error, FileOp, Held, Ledger, Result, SongTags, StreamOp,
    consulted::{CONSULT_ANSWERED_WITHIN, Consultable, Consulting, KEPT_PAGES_A_MEETING},
    quic::{PEER_NAME, read_frame, send_frame},
    remote::{
        CONTROL_ANSWERED_WITHIN, Controlled, Controlling, MIRROR_GOES_STALE_AFTER,
        MIRROR_LOOKED_AT_EVERY, MIRROR_TOLD_AGAIN_AFTER, Mirror, RemoteCommand, RemoteZone,
        Watched, told_alike,
    },
    wire::{
        FromPeer, KEPT_ASKED_AT_MOST, LARGEST_CONSULTED, LARGEST_KEPT_ANSWER, LARGEST_KEPT_KEY,
        LARGEST_SHARED_COVER, ToPeer,
    },
};

pub const DIAL_WITHIN: Duration = Duration::from_secs(8);
const CHUNK_BYTES: usize = 64 * 1024;
const HELD_PER_FRAME: usize = 512;
const CHUNKS_IN_FLIGHT: usize = 8;
const NOTED_EXTENSION: &str = "from";
pub const A_FETCH_GOES_QUIET_AFTER: Duration = Duration::from_secs(30);

pub async fn dialled(
    endpoint: &Endpoint,
    config: &ClientConfig,
    candidates: &[SocketAddr],
    device: DeviceId,
) -> Result<Connection> {
    dialled_within(endpoint, config, candidates, device, DIAL_WITHIN).await
}

pub async fn dialled_within(
    endpoint: &Endpoint,
    config: &ClientConfig,
    candidates: &[SocketAddr],
    device: DeviceId,
    within: Duration,
) -> Result<Connection> {
    let mut attempts = JoinSet::new();
    for candidate in candidates {
        let connecting = endpoint
            .connect_with(config.clone(), *candidate, PEER_NAME)
            .map_err(|source| Error::Connect(Box::new(source)));
        match connecting {
            Ok(connecting) => {
                attempts.spawn(async move { timeout(within, connecting).await });
            }
            Err(error) => {
                tracing::debug!(%error, %candidate, "a peer candidate could not be dialled")
            }
        }
    }
    while let Some(attempt) = attempts.join_next().await {
        if let Ok(Ok(Ok(connection))) = attempt {
            attempts.abort_all();
            return Ok(connection);
        }
    }
    Err(Error::Unreachable(device))
}

pub struct Answering {
    pub catalog: Arc<dyn Catalog>,
    pub consulted: RwLock<Option<Arc<dyn Consultable>>>,
    pub controlled: RwLock<Option<(Arc<dyn Controlled>, RemoteTrust)>>,
    pub ledger: Arc<Mutex<Ledger>>,
    pub device: DeviceId,
    pub heard: UnboundedSender<Heard>,
    pub transfers_allowed: Arc<AtomicBool>,
}

pub enum Heard {
    Met {
        device: DeviceId,
        connection: Connection,
        records: bool,
    },
    Told {
        whole: bool,
    },
}

struct Asked {
    connection: Connection,
    answering: Arc<Answering>,
    met: Arc<Mutex<Option<DeviceId>>>,
}

pub async fn serve(connection: Connection, answering: Arc<Answering>, met: Option<DeviceId>) {
    let met = Arc::new(Mutex::new(met));
    loop {
        let (send, recv) = match connection.accept_bi().await {
            Ok(streams) => streams,
            Err(error) => {
                tracing::debug!(%error, "a peer went away");
                return;
            }
        };
        let asked = Asked {
            connection: connection.clone(),
            answering: Arc::clone(&answering),
            met: Arc::clone(&met),
        };
        tokio::spawn(async move {
            if let Err(error) = answer(send, recv, asked).await {
                tracing::debug!(%error, "a peer's request could not be answered");
            }
        });
    }
}

async fn answer(mut send: SendStream, mut recv: RecvStream, asked: Asked) -> Result<()> {
    let Some(request) = read_frame(&mut recv).await? else {
        return Ok(());
    };
    let catalog = Arc::clone(&asked.answering.catalog);
    match ToPeer::decoded(&request)? {
        ToPeer::Meet { device, records } => {
            *asked.met.lock() = Some(device);
            let met = FromPeer::Met {
                device: asked.answering.device,
            };
            send_frame(&mut send, &met.encoded()).await?;
            let _ = asked.answering.heard.send(Heard::Met {
                device,
                connection: asked.connection,
                records,
            });
        }
        ToPeer::Changes { whole, changes } => {
            if asked.met.lock().is_none() {
                return Err(Error::Unintroduced);
            }
            let ledger = Arc::clone(&asked.answering.ledger);
            blocking(move || {
                ledger
                    .lock()
                    .arrived(catalog.as_ref(), &changes)
                    .map(|_| ())
            })
            .await?;
            send_frame(&mut send, &FromPeer::Taken.encoded()).await?;
            let _ = asked.answering.heard.send(Heard::Told { whole });
        }
        ToPeer::Holdings => {
            let holdings = blocking(move || catalog.holdings().map_err(Error::Catalog)).await?;
            for items in holdings.chunks(HELD_PER_FRAME) {
                let frame = FromPeer::Held {
                    items: items.to_vec(),
                };
                send_frame(&mut send, &frame.encoded()).await?;
            }
            send_frame(&mut send, &FromPeer::HeldEnd.encoded()).await?;
        }
        ToPeer::Tags { recording } => {
            let answer = if asked.answering.transfers_allowed.load(Ordering::Relaxed) {
                match blocking(move || catalog.tags(&recording).map_err(Error::Catalog)).await {
                    Ok(tags) => FromPeer::Tagged { tags },
                    Err(error) => {
                        tracing::debug!(%error, "the tags of a song could not be read to send");
                        FromPeer::Missing
                    }
                }
            } else {
                FromPeer::Missing
            };
            send_frame(&mut send, &answer.encoded()).await?;
        }
        ToPeer::Covers { releases } => {
            if asked.answering.transfers_allowed.load(Ordering::Relaxed) {
                send_covers(&mut send, catalog, releases).await?;
            }
            send_frame(&mut send, &FromPeer::CoversEnd.encoded()).await?;
        }
        ToPeer::Lyricsfiles { recordings } => {
            if asked.answering.transfers_allowed.load(Ordering::Relaxed) {
                send_lyricsfiles(&mut send, catalog, recordings).await?;
            }
            send_frame(&mut send, &FromPeer::LyricsfilesEnd.encoded()).await?;
        }
        ToPeer::Prints { files } => {
            if asked.answering.transfers_allowed.load(Ordering::Relaxed) {
                send_prints(&mut send, catalog, files).await?;
            }
            send_frame(&mut send, &FromPeer::PrintsEnd.encoded()).await?;
        }
        ToPeer::Control { zone, command } => {
            let answer = controlled_here(&asked.answering, zone, command).await;
            send_frame(&mut send, &FromPeer::Controlled(answer).encoded()).await?;
        }
        ToPeer::Watch => send_mirror(&mut send, &asked.answering).await?,
        ToPeer::Consult { asked: question } => {
            let answer = consulted_here(&asked.answering, question).await;
            send_frame(&mut send, &answer.encoded()).await?;
        }
        ToPeer::Kept { asker, after, most } => {
            send_kept(&mut send, &asked.answering, asker, after, most).await?
        }
        ToPeer::Fetch { recording, offset } => {
            if asked.answering.transfers_allowed.load(Ordering::Relaxed) {
                send_song(&mut send, catalog, recording, offset).await?;
            } else {
                send_frame(&mut send, &FromPeer::Missing.encoded()).await?;
            }
        }
    }
    send.finish().map_err(|_| Error::StreamClosed)
}

impl Answering {
    fn controlled(&self) -> Option<(Arc<dyn Controlled>, RemoteTrust)> {
        self.controlled.read().clone()
    }

    fn consultable(&self) -> Option<Arc<dyn Consultable>> {
        if self.transfers_allowed.load(Ordering::Relaxed) {
            self.consulted.read().clone()
        } else {
            None
        }
    }
}

async fn consulted_here(answering: &Answering, asked: Vec<u8>) -> FromPeer {
    let Some(consulted) = answering.consultable() else {
        return FromPeer::Unconsulted;
    };
    match blocking(move || Ok(consulted.consult(&asked))).await {
        Ok(Some(answer)) if answer.len() <= LARGEST_CONSULTED => FromPeer::Consulted { answer },
        Ok(_) => FromPeer::Unconsulted,
        Err(error) => {
            tracing::debug!(%error, "another device's lookup could not be answered");
            FromPeer::Unconsulted
        }
    }
}

async fn send_kept(
    send: &mut SendStream,
    answering: &Answering,
    asker: DeviceId,
    after: u64,
    most: u32,
) -> Result<()> {
    let kept = match answering.consultable() {
        Some(consulted) => {
            let most = (most as usize).min(KEPT_ASKED_AT_MOST);
            blocking(move || Ok(consulted.kept_after(after, most, asker)))
                .await
                .unwrap_or_default()
        }
        None => Vec::new(),
    };
    let mut mark = after;
    for answer in kept {
        mark = mark.max(answer.seq);
        if answer.key.len() <= LARGEST_KEPT_KEY && answer.packed.len() <= LARGEST_KEPT_ANSWER {
            send_frame(send, &FromPeer::KeptAnswer(answer).encoded()).await?;
        }
    }
    send_frame(send, &FromPeer::KeptEnd { mark }.encoded()).await
}

pub async fn consult(connection: &Connection, asked: Vec<u8>) -> Consulting {
    let asking = timeout(CONSULT_ANSWERED_WITHIN, async {
        let (mut send, mut recv) = connection
            .open_bi()
            .await
            .map_err(|source| Error::Connection(Box::new(source)))?;
        send_frame(&mut send, &ToPeer::Consult { asked }.encoded()).await?;
        send.finish().map_err(|_| Error::StreamClosed)?;
        Ok::<_, Error>(match read_frame(&mut recv).await? {
            Some(frame) => match FromPeer::decoded(&frame)? {
                FromPeer::Consulted { answer } => Consulting::Answered(answer),
                FromPeer::Unconsulted => Consulting::Unconsulted,
                _ => Consulting::Unanswered,
            },
            None => Consulting::Unanswered,
        })
    })
    .await;
    match asking {
        Ok(Ok(consulting)) => consulting,
        Ok(Err(error)) => {
            tracing::debug!(%error, "a device was not asked a lookup");
            Consulting::Unanswered
        }
        Err(_) => Consulting::Unanswered,
    }
}

pub async fn kept_from(
    connection: &Connection,
    asker: DeviceId,
    device: DeviceId,
    consulted: Arc<dyn Consultable>,
) -> Result<usize> {
    let mut taken = 0;
    for _ in 0..KEPT_PAGES_A_MEETING {
        let marking = Arc::clone(&consulted);
        let after = blocking(move || Ok(marking.kept_mark(device))).await?;
        let (mut send, mut recv) = quietly_short(async {
            connection
                .open_bi()
                .await
                .map_err(|source| Error::Connection(Box::new(source)))
        })
        .await?;
        let most = u32::try_from(KEPT_ASKED_AT_MOST).unwrap_or(u32::MAX);
        send_frame(&mut send, &ToPeer::Kept { asker, after, most }.encoded()).await?;
        send.finish().map_err(|_| Error::StreamClosed)?;
        let mut answers = Vec::new();
        let mut mark = None;
        while let Some(frame) = quietly_short(read_frame(&mut recv)).await? {
            match FromPeer::decoded(&frame)? {
                FromPeer::KeptAnswer(answer) => answers.push(answer),
                FromPeer::KeptEnd { mark: told } => {
                    mark = Some(told);
                    break;
                }
                _ => {}
            }
        }
        let Some(mark) = mark else {
            break;
        };
        let told = answers.len();
        let taking = Arc::clone(&consulted);
        blocking(move || {
            taking.take_kept(device, answers, mark);
            Ok(())
        })
        .await?;
        taken += told;
        if told < KEPT_ASKED_AT_MOST || mark <= after {
            break;
        }
    }
    Ok(taken)
}

async fn controlled_here(
    answering: &Answering,
    zone: ZoneId,
    command: RemoteCommand,
) -> Controlling {
    let Some((controlled, trust)) = answering.controlled() else {
        return Controlling::Uncontrollable;
    };
    if !command.trusted_by(trust) {
        return Controlling::Untrusted;
    }
    match blocking(move || Ok(controlled.control(zone, command))).await {
        Ok(Ok(())) => Controlling::Done,
        Ok(Err(uncontrolled)) => uncontrolled.into(),
        Err(error) => {
            tracing::debug!(%error, "a command from another device could not be applied");
            Controlling::Failed
        }
    }
}

async fn send_mirror(send: &mut SendStream, answering: &Answering) -> Result<()> {
    let mut told: Option<(Vec<RemoteZone>, Instant)> = None;
    loop {
        let (controlled, trust) = match answering.controlled() {
            Some(held) => held,
            None => {
                let refusal = FromPeer::Controlled(Controlling::Uncontrollable);
                return send_frame(send, &refusal.encoded()).await;
            }
        };
        if !trust.shows() {
            let refusal = FromPeer::Controlled(Controlling::Untrusted);
            return send_frame(send, &refusal.encoded()).await;
        }
        let zones = blocking(move || Ok(controlled.zones())).await?;
        let now = Instant::now();
        let alike = told.as_ref().is_some_and(|(was, at)| {
            now.saturating_duration_since(*at) < MIRROR_TOLD_AGAIN_AFTER
                && told_alike(was, *at, &zones, now)
        });
        if !alike {
            send_frame(
                send,
                &FromPeer::Zones {
                    zones: zones.clone(),
                }
                .encoded(),
            )
            .await?;
            told = Some((zones, now));
        }
        tokio::time::sleep(MIRROR_LOOKED_AT_EVERY).await;
    }
}

pub async fn control(connection: &Connection, zone: ZoneId, command: RemoteCommand) -> Controlling {
    let asked = timeout(CONTROL_ANSWERED_WITHIN, async {
        let (mut send, mut recv) = connection
            .open_bi()
            .await
            .map_err(|source| Error::Connection(Box::new(source)))?;
        send_frame(&mut send, &ToPeer::Control { zone, command }.encoded()).await?;
        send.finish().map_err(|_| Error::StreamClosed)?;
        Ok::<_, Error>(match read_frame(&mut recv).await? {
            Some(answer) => match FromPeer::decoded(&answer)? {
                FromPeer::Controlled(controlling) => controlling,
                _ => Controlling::Unanswered,
            },
            None => Controlling::Unanswered,
        })
    })
    .await;
    match asked {
        Ok(Ok(controlling)) => controlling,
        Ok(Err(error)) => {
            tracing::debug!(%error, "a device was not told the command");
            Controlling::Unanswered
        }
        Err(_) => Controlling::Unanswered,
    }
}

pub async fn watch(
    connection: &Connection,
    device: DeviceId,
    mirrors: &RwLock<BTreeMap<DeviceId, Watched>>,
    still_watched: impl Fn() -> bool,
) -> Result<()> {
    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|source| Error::Connection(Box::new(source)))?;
    send_frame(&mut send, &ToPeer::Watch.encoded()).await?;
    send.finish().map_err(|_| Error::StreamClosed)?;
    let mut heard_at = Instant::now();
    while still_watched() && connection.close_reason().is_none() {
        let frame = match timeout(MIRROR_LOOKED_AT_EVERY * 4, read_frame(&mut recv)).await {
            Err(_) if heard_at.elapsed() < MIRROR_GOES_STALE_AFTER => continue,
            Err(_) => break,
            Ok(frame) => frame?,
        };
        let Some(frame) = frame else {
            break;
        };
        heard_at = Instant::now();
        match FromPeer::decoded(&frame)? {
            FromPeer::Zones { zones } => {
                mirrors
                    .write()
                    .insert(device, Watched::Mirrored(Mirror { zones, heard_at }));
            }
            FromPeer::Controlled(refusal) => {
                mirrors.write().insert(device, Watched::Refused(refusal));
                return Ok(());
            }
            _ => {}
        }
    }
    let _ = recv.stop(0_u32.into());
    Ok(())
}

async fn send_covers(
    send: &mut SendStream,
    catalog: Arc<dyn Catalog>,
    releases: Vec<Mbid>,
) -> Result<()> {
    for release in releases {
        let reading = Arc::clone(&catalog);
        let asked = release.clone();
        let picture = match blocking(move || reading.cover(&asked).map_err(Error::Catalog)).await {
            Ok(picture) => picture,
            Err(error) => {
                tracing::debug!(%error, "a cover could not be read to send");
                None
            }
        };
        let Some(picture) = picture.filter(|picture| sendable_cover(picture)) else {
            continue;
        };
        send_frame(send, &FromPeer::Covered { release, picture }.encoded()).await?;
    }
    Ok(())
}

async fn send_lyricsfiles(
    send: &mut SendStream,
    catalog: Arc<dyn Catalog>,
    recordings: Vec<Recording>,
) -> Result<()> {
    for recording in recordings {
        let reading = Arc::clone(&catalog);
        let asked = recording.clone();
        let kept = match blocking(move || reading.lyricsfile(&asked).map_err(Error::Catalog)).await
        {
            Ok(kept) => kept,
            Err(error) => {
                tracing::debug!(%error, "a lyricsfile could not be read to send");
                None
            }
        };
        let Some(lyricsfile) = kept.filter(SharedLyricsfile::travels) else {
            continue;
        };
        send_frame(
            send,
            &FromPeer::Documented {
                recording,
                lyricsfile,
            }
            .encoded(),
        )
        .await?;
    }
    Ok(())
}

pub async fn lyricsfiles_from(
    connection: &Connection,
    recordings: Vec<Recording>,
    catalog: Arc<dyn Catalog>,
) -> Result<usize> {
    let (mut send, mut recv) = quietly_short(async {
        connection
            .open_bi()
            .await
            .map_err(|source| Error::Connection(Box::new(source)))
    })
    .await?;
    send_frame(&mut send, &ToPeer::Lyricsfiles { recordings }.encoded()).await?;
    send.finish().map_err(|_| Error::StreamClosed)?;

    let mut landed = 0;
    while let Some(frame) = quietly_short(read_frame(&mut recv)).await? {
        match FromPeer::decoded(&frame)? {
            FromPeer::Documented {
                recording,
                lyricsfile,
            } => {
                let taking = Arc::clone(&catalog);
                let taken = blocking(move || {
                    taking
                        .take_lyricsfile(&recording, &lyricsfile)
                        .map_err(Error::Catalog)
                })
                .await;
                match taken {
                    Ok(true) => landed += 1,
                    Ok(false) => {}
                    Err(error) => {
                        tracing::debug!(%error, "a lyricsfile from another device was refused")
                    }
                }
            }
            FromPeer::LyricsfilesEnd => break,
            FromPeer::Held { .. }
            | FromPeer::HeldEnd
            | FromPeer::Sending { .. }
            | FromPeer::Missing
            | FromPeer::Met { .. }
            | FromPeer::Taken
            | FromPeer::Tagged { .. }
            | FromPeer::Covered { .. }
            | FromPeer::CoversEnd
            | FromPeer::Printed { .. }
            | FromPeer::PrintsEnd
            | FromPeer::Controlled(_)
            | FromPeer::Zones { .. }
            | FromPeer::Consulted { .. }
            | FromPeer::Unconsulted
            | FromPeer::KeptAnswer(_)
            | FromPeer::KeptEnd { .. } => {}
        }
    }
    Ok(landed)
}

async fn send_prints(
    send: &mut SendStream,
    catalog: Arc<dyn Catalog>,
    files: Vec<WholeFile>,
) -> Result<()> {
    for file in files {
        let reading = Arc::clone(&catalog);
        let kept = match blocking(move || reading.print(file).map_err(Error::Catalog)).await {
            Ok(kept) => kept,
            Err(error) => {
                tracing::debug!(%error, "a print could not be read to send");
                None
            }
        };
        let Some(print) = kept.filter(SharedPrint::travels) else {
            continue;
        };
        send_frame(send, &FromPeer::Printed { file, print }.encoded()).await?;
    }
    Ok(())
}

pub async fn prints_from(
    connection: &Connection,
    files: Vec<WholeFile>,
    catalog: Arc<dyn Catalog>,
) -> Result<usize> {
    let (mut send, mut recv) = quietly_short(async {
        connection
            .open_bi()
            .await
            .map_err(|source| Error::Connection(Box::new(source)))
    })
    .await?;
    send_frame(&mut send, &ToPeer::Prints { files }.encoded()).await?;
    send.finish().map_err(|_| Error::StreamClosed)?;

    let mut landed = 0;
    while let Some(frame) = quietly_short(read_frame(&mut recv)).await? {
        match FromPeer::decoded(&frame)? {
            FromPeer::Printed { file, print } => {
                let taking = Arc::clone(&catalog);
                let taken =
                    blocking(move || taking.take_print(file, &print).map_err(Error::Catalog)).await;
                match taken {
                    Ok(true) => landed += 1,
                    Ok(false) => {}
                    Err(error) => {
                        tracing::debug!(%error, "a print from another device was refused")
                    }
                }
            }
            FromPeer::PrintsEnd => break,
            FromPeer::Held { .. }
            | FromPeer::HeldEnd
            | FromPeer::Sending { .. }
            | FromPeer::Missing
            | FromPeer::Met { .. }
            | FromPeer::Taken
            | FromPeer::Tagged { .. }
            | FromPeer::Covered { .. }
            | FromPeer::CoversEnd
            | FromPeer::Documented { .. }
            | FromPeer::LyricsfilesEnd
            | FromPeer::Controlled(_)
            | FromPeer::Zones { .. }
            | FromPeer::Consulted { .. }
            | FromPeer::Unconsulted
            | FromPeer::KeptAnswer(_)
            | FromPeer::KeptEnd { .. } => {}
        }
    }
    Ok(landed)
}

fn sendable_cover(picture: &[u8]) -> bool {
    !picture.is_empty() && picture.len() <= LARGEST_SHARED_COVER
}

pub async fn covers_from(
    connection: &Connection,
    releases: Vec<Mbid>,
    catalog: Arc<dyn Catalog>,
) -> Result<usize> {
    let (mut send, mut recv) = quietly_short(async {
        connection
            .open_bi()
            .await
            .map_err(|source| Error::Connection(Box::new(source)))
    })
    .await?;
    send_frame(&mut send, &ToPeer::Covers { releases }.encoded()).await?;
    send.finish().map_err(|_| Error::StreamClosed)?;

    let mut landed = 0;
    while let Some(frame) = quietly_short(read_frame(&mut recv)).await? {
        match FromPeer::decoded(&frame)? {
            FromPeer::Covered { release, picture } => {
                let taking = Arc::clone(&catalog);
                let taken = blocking(move || {
                    taking
                        .take_cover(&release, &picture)
                        .map_err(Error::Catalog)
                })
                .await;
                match taken {
                    Ok(true) => landed += 1,
                    Ok(false) => {}
                    Err(error) => {
                        tracing::debug!(%error, "a cover from another device was refused")
                    }
                }
            }
            FromPeer::CoversEnd => break,
            FromPeer::Held { .. }
            | FromPeer::HeldEnd
            | FromPeer::Sending { .. }
            | FromPeer::Missing
            | FromPeer::Met { .. }
            | FromPeer::Taken
            | FromPeer::Tagged { .. }
            | FromPeer::Documented { .. }
            | FromPeer::LyricsfilesEnd
            | FromPeer::Printed { .. }
            | FromPeer::PrintsEnd
            | FromPeer::Controlled(_)
            | FromPeer::Zones { .. }
            | FromPeer::Consulted { .. }
            | FromPeer::Unconsulted
            | FromPeer::KeptAnswer(_)
            | FromPeer::KeptEnd { .. } => {}
        }
    }
    Ok(landed)
}

async fn send_song(
    send: &mut SendStream,
    catalog: Arc<dyn Catalog>,
    recording: Recording,
    offset: u64,
) -> Result<()> {
    let opened = blocking(move || catalog.open(&recording).map_err(Error::Catalog)).await?;
    let Some(mut opened) = opened else {
        return send_frame(send, &FromPeer::Missing.encoded()).await;
    };
    let header = FromPeer::Sending {
        size: opened.size,
        extension: opened.extension.clone(),
    };
    send_frame(send, &header.encoded()).await?;

    let (chunks, mut arriving) = mpsc::channel::<std::io::Result<Vec<u8>>>(CHUNKS_IN_FLIGHT);
    let reading = tokio::task::spawn_blocking(move || {
        if let Err(error) = opened.reader.seek(SeekFrom::Start(offset)) {
            let _ = chunks.blocking_send(Err(error));
            return;
        }
        loop {
            let mut chunk = vec![0; CHUNK_BYTES];
            match opened.reader.read(&mut chunk) {
                Ok(0) => return,
                Ok(read) => {
                    chunk.truncate(read);
                    if chunks.blocking_send(Ok(chunk)).is_err() {
                        return;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => {
                    let _ = chunks.blocking_send(Err(error));
                    return;
                }
            }
        }
    });
    while let Some(chunk) = arriving.recv().await {
        let chunk = chunk.map_err(|source| Error::stream(StreamOp::Read, source))?;
        send.write_all(&chunk)
            .await
            .map_err(|source| Error::stream(StreamOp::Write, source))?;
    }
    let _ = reading.await;
    Ok(())
}

pub async fn holdings_of(connection: &Connection) -> Result<Vec<Held>> {
    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|source| Error::Connection(Box::new(source)))?;
    send_frame(&mut send, &ToPeer::Holdings.encoded()).await?;
    send.finish().map_err(|_| Error::StreamClosed)?;
    let mut holdings = Vec::new();
    while let Some(frame) = read_frame(&mut recv).await? {
        match FromPeer::decoded(&frame)? {
            FromPeer::Held { items } => holdings.extend(items),
            FromPeer::HeldEnd => break,
            FromPeer::Sending { .. }
            | FromPeer::Missing
            | FromPeer::Met { .. }
            | FromPeer::Taken
            | FromPeer::Tagged { .. }
            | FromPeer::Covered { .. }
            | FromPeer::CoversEnd
            | FromPeer::Documented { .. }
            | FromPeer::LyricsfilesEnd
            | FromPeer::Printed { .. }
            | FromPeer::PrintsEnd
            | FromPeer::Controlled(_)
            | FromPeer::Zones { .. }
            | FromPeer::Consulted { .. }
            | FromPeer::Unconsulted
            | FromPeer::KeptAnswer(_)
            | FromPeer::KeptEnd { .. } => {}
        }
    }
    Ok(holdings)
}

pub async fn meet(connection: &Connection, device: DeviceId, records: bool) -> Result<DeviceId> {
    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|source| Error::Connection(Box::new(source)))?;
    send_frame(&mut send, &ToPeer::Meet { device, records }.encoded()).await?;
    send.finish().map_err(|_| Error::StreamClosed)?;
    let Some(answer) = read_frame(&mut recv).await? else {
        return Err(Error::Unmet);
    };
    match FromPeer::decoded(&answer)? {
        FromPeer::Met { device } => Ok(device),
        FromPeer::Held { .. }
        | FromPeer::HeldEnd
        | FromPeer::Sending { .. }
        | FromPeer::Missing
        | FromPeer::Taken
        | FromPeer::Tagged { .. }
        | FromPeer::Covered { .. }
        | FromPeer::CoversEnd
        | FromPeer::Documented { .. }
        | FromPeer::LyricsfilesEnd
        | FromPeer::Printed { .. }
        | FromPeer::PrintsEnd
        | FromPeer::Controlled(_)
        | FromPeer::Zones { .. }
        | FromPeer::Consulted { .. }
        | FromPeer::Unconsulted
        | FromPeer::KeptAnswer(_)
        | FromPeer::KeptEnd { .. } => Err(Error::Unmet),
    }
}

pub async fn tell(
    connection: &Connection,
    device: DeviceId,
    whole: bool,
    changes: Vec<Change>,
) -> Result<()> {
    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|source| Error::Connection(Box::new(source)))?;
    send_frame(&mut send, &ToPeer::Changes { whole, changes }.encoded()).await?;
    send.finish().map_err(|_| Error::StreamClosed)?;
    match read_frame(&mut recv).await? {
        Some(answer) if FromPeer::decoded(&answer)? == FromPeer::Taken => Ok(()),
        Some(_) | None => Err(Error::Untaken(device)),
    }
}

pub struct Fetched {
    pub staged: PathBuf,
    pub extension: String,
    pub tags: SongTags,
}

pub async fn fetch(
    connection: &Connection,
    device: DeviceId,
    recording: &Recording,
    staged: &Path,
    received: impl Fn(u64, u64),
) -> Result<Fetched> {
    let noted = staged.with_extension(NOTED_EXTENSION);
    let offset = resumable(staged, &noted, device).await;
    let fetched = match fetched_from(
        connection, device, recording, staged, &noted, offset, &received,
    )
    .await?
    {
        Some(fetched) => fetched,
        None => fetched_from(connection, device, recording, staged, &noted, 0, &received)
            .await?
            .ok_or(Error::Missing(device))?,
    };
    Ok(Fetched {
        tags: tags_from(connection, recording).await,
        ..fetched
    })
}

async fn tags_from(connection: &Connection, recording: &Recording) -> SongTags {
    match asked_for_tags(connection, recording).await {
        Ok(tags) => tags,
        Err(error) => {
            tracing::debug!(%error, "a device did not say what tags a song carried");
            SongTags::default()
        }
    }
}

async fn asked_for_tags(connection: &Connection, recording: &Recording) -> Result<SongTags> {
    let (mut send, mut recv) = quietly_short(async {
        connection
            .open_bi()
            .await
            .map_err(|source| Error::Connection(Box::new(source)))
    })
    .await?;
    let request = ToPeer::Tags {
        recording: recording.clone(),
    };
    send_frame(&mut send, &request.encoded()).await?;
    send.finish().map_err(|_| Error::StreamClosed)?;

    let answer = quietly_short(read_frame(&mut recv)).await?;
    match answer.as_deref().map(FromPeer::decoded).transpose()? {
        Some(FromPeer::Tagged { tags }) => Ok(tags),
        Some(_) | None => Ok(SongTags::default()),
    }
}

async fn resumable(staged: &Path, noted: &Path, device: DeviceId) -> u64 {
    let from_here = fs::read_to_string(noted).await.ok().is_some_and(|note| {
        note.split_once(' ')
            .is_some_and(|(from, _)| from == device.to_string())
    });
    if !from_here {
        let _ = fs::remove_file(staged).await;
        let _ = fs::remove_file(noted).await;
        return 0;
    }
    fs::metadata(staged)
        .await
        .map(|metadata| metadata.len())
        .unwrap_or_default()
}

async fn noted_size(noted: &Path) -> Option<u64> {
    let note = fs::read_to_string(noted).await.ok()?;
    note.split_once(' ')?.1.trim().parse().ok()
}

async fn quietly_short<T>(waiting: impl Future<Output = Result<T>>) -> Result<T> {
    timeout(A_FETCH_GOES_QUIET_AFTER, waiting)
        .await
        .unwrap_or_else(|_| {
            Err(Error::stream(
                StreamOp::Read,
                std::io::Error::from(std::io::ErrorKind::TimedOut),
            ))
        })
}

async fn fetched_from(
    connection: &Connection,
    device: DeviceId,
    recording: &Recording,
    staged: &Path,
    noted: &Path,
    offset: u64,
    received: &impl Fn(u64, u64),
) -> Result<Option<Fetched>> {
    let (mut send, mut recv) = quietly_short(async {
        connection
            .open_bi()
            .await
            .map_err(|source| Error::Connection(Box::new(source)))
    })
    .await?;
    let request = ToPeer::Fetch {
        recording: recording.clone(),
        offset,
    };
    send_frame(&mut send, &request.encoded()).await?;
    send.finish().map_err(|_| Error::StreamClosed)?;

    let Some(header) = quietly_short(read_frame(&mut recv)).await? else {
        return Err(Error::Missing(device));
    };
    let (size, extension) = match FromPeer::decoded(&header)? {
        FromPeer::Sending { size, extension } => (size, extension),
        FromPeer::Missing
        | FromPeer::Held { .. }
        | FromPeer::HeldEnd
        | FromPeer::Met { .. }
        | FromPeer::Taken
        | FromPeer::Tagged { .. }
        | FromPeer::Covered { .. }
        | FromPeer::CoversEnd
        | FromPeer::Documented { .. }
        | FromPeer::LyricsfilesEnd
        | FromPeer::Printed { .. }
        | FromPeer::PrintsEnd
        | FromPeer::Controlled(_)
        | FromPeer::Zones { .. }
        | FromPeer::Consulted { .. }
        | FromPeer::Unconsulted
        | FromPeer::KeptAnswer(_)
        | FromPeer::KeptEnd { .. } => {
            return Err(Error::Missing(device));
        }
    };
    if offset > 0 && noted_size(noted).await != Some(size) {
        let _ = recv.stop(0_u32.into());
        let _ = fs::remove_file(staged).await;
        let _ = fs::remove_file(noted).await;
        return Ok(None);
    }
    fs::write(noted, format!("{device} {size}"))
        .await
        .map_err(|source| file_failed(FileOp::Stage, noted, source))?;

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(staged)
        .await
        .map_err(|source| file_failed(FileOp::Stage, staged, source))?;
    let mut have = offset;
    let mut chunk = vec![0; CHUNK_BYTES];
    loop {
        let read = quietly_short(async {
            recv.read(&mut chunk)
                .await
                .map_err(|source| Error::stream(StreamOp::Read, source))
        })
        .await?;
        let Some(read) = read else {
            break;
        };
        file.write_all(&chunk[..read])
            .await
            .map_err(|source| file_failed(FileOp::Write, staged, source))?;
        have += u64::try_from(read).unwrap_or(u64::MAX);
        received(have, size);
        if have > size {
            break;
        }
    }
    file.sync_all()
        .await
        .map_err(|source| file_failed(FileOp::Write, staged, source))?;
    let _ = fs::remove_file(noted).await;
    if have != size {
        let _ = fs::remove_file(staged).await;
        return Err(Error::stream(
            StreamOp::Read,
            std::io::Error::from(std::io::ErrorKind::UnexpectedEof),
        ));
    }
    Ok(Some(Fetched {
        staged: staged.to_path_buf(),
        extension,
        tags: SongTags::default(),
    }))
}

fn file_failed(op: FileOp, path: &Path, source: std::io::Error) -> Error {
    Error::File {
        op,
        path: path.to_path_buf(),
        source,
    }
}

pub async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| Error::Stopped)?
}

#[cfg(test)]
mod tests {
    use std::{env, process};

    use super::*;

    fn staged_in(name: &str) -> PathBuf {
        let folder = env::temp_dir().join(format!("resonate-peer-{name}-{}", process::id()));
        std::fs::create_dir_all(&folder).expect("a folder");
        folder.join("song.part")
    }

    #[tokio::test]
    async fn a_partial_is_resumed_only_from_the_device_it_came_from() {
        let staged = staged_in("resumed");
        let noted = staged.with_extension(NOTED_EXTENSION);
        let sender = DeviceId([1; 16]);
        let other = DeviceId([2; 16]);
        std::fs::write(&staged, [0_u8; 100]).expect("a partial");
        std::fs::write(&noted, format!("{sender} 4000")).expect("a note");

        let from_the_sender = resumable(&staged, &noted, sender).await;
        let from_another = resumable(&staged, &noted, other).await;

        assert_eq!(from_the_sender, 100);
        assert_eq!(from_another, 0);
        assert!(!staged.exists());
        assert!(!noted.exists());
    }

    #[tokio::test]
    async fn a_partial_no_note_names_is_started_over() {
        let staged = staged_in("unnoted");
        let noted = staged.with_extension(NOTED_EXTENSION);
        std::fs::write(&staged, [0_u8; 100]).expect("a partial");

        let offset = resumable(&staged, &noted, DeviceId([1; 16])).await;

        assert_eq!(offset, 0);
        assert!(!staged.exists());
    }
}
