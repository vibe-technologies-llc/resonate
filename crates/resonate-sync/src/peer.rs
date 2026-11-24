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
    Bound, Catalog, Change, DeviceId, Error, FileOp, Held, Ledger, ReadSeek, Result, Revised,
    Revision, SongTags, StreamOp,
    consulted::{
        CONSULT_ANSWERED_WITHIN, Consultable, Consulting, Dates, ExportFetched, ExportHeld,
        ExportPart, ExportReceiving, KEPT_PAGES_A_MEETING,
    },
    quic::{FrameReader, PEER_NAME, read_frame, send_frame},
    remote::{
        CONTROL_ANSWERED_WITHIN, Controlled, Controlling, MIRROR_GOES_STALE_AFTER,
        MIRROR_LOOKED_AT_EVERY, MIRROR_TOLD_AGAIN_AFTER, Mirror, RemoteCommand, RemoteZone,
        Revisions, Watched, told_alike,
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
const A_LISTING_GOES_QUIET_AFTER: Duration = Duration::from_secs(30);
const MIRROR_POLLED_EVERY: Duration = MIRROR_LOOKED_AT_EVERY.saturating_mul(4);
pub const LARGEST_SONG_FETCHED: u64 = 4 * 1024 * 1024 * 1024;

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
        ToPeer::Control {
            zone,
            command,
            queue_seen,
        } => {
            let answer = controlled_here(&asked.answering, zone, command, queue_seen).await;

            send_frame(&mut send, &FromPeer::Controlled(answer).encoded()).await?;
        }
        ToPeer::Watch { revisions } => send_mirror(&mut send, &asked.answering, revisions).await?,
        ToPeer::Consult {
            asked: question,
            dates,
        } => {
            let answer = consulted_here(&asked.answering, question, dates).await;

            send_frame(&mut send, &answer.encoded()).await?;
        }
        ToPeer::Kept { asker, after, most } => {
            send_kept(&mut send, &asked.answering, asker, after, most).await?
        }
        ToPeer::Export { part, offset } => {
            send_export(&mut send, &asked.answering, part, offset).await?;
        }
        ToPeer::Fetch {
            recording,
            offset,
            bound,
        } => {
            if asked.answering.transfers_allowed.load(Ordering::Relaxed) {
                send_song(&mut send, catalog, recording, offset, bound).await?;
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

async fn consulted_here(answering: &Answering, asked: Vec<u8>, dates: Dates) -> FromPeer {
    let Some(consulted) = answering.consultable() else {
        return FromPeer::Unconsulted;
    };
    match blocking(move || Ok(consulted.consult(&asked))).await {
        Ok(Some(told)) if told.answer.len() <= LARGEST_CONSULTED => FromPeer::Consulted {
            answer: told.answer,
            at: (dates == Dates::Carried).then_some(told.at),
        },
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
        match consulted_as(connection, asked.clone(), Dates::Carried).await {
            Ok(Some(consulting)) => return Ok::<_, Error>(consulting),
            Ok(None) => {}
            Err(error) => {
                tracing::debug!(%error, "a device did not date its answer, so it is asked as before");
            }
        }
        if connection.close_reason().is_some() {
            return Ok(Consulting::Unanswered);
        }
        Ok(consulted_as(connection, asked, Dates::Unsaid)
            .await?
            .unwrap_or(Consulting::Unanswered))
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

async fn consulted_as(
    connection: &Connection,
    asked: Vec<u8>,
    dates: Dates,
) -> Result<Option<Consulting>> {
    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|source| Error::Connection(Box::new(source)))?;

    send_frame(&mut send, &ToPeer::Consult { asked, dates }.encoded()).await?;
    send.finish().map_err(|_| Error::StreamClosed)?;

    let Some(frame) = read_frame(&mut recv).await? else {
        return Ok(None);
    };
    Ok(Some(match FromPeer::decoded(&frame)? {
        FromPeer::Consulted { answer, at } => Consulting::Answered { answer, at },
        FromPeer::Unconsulted => Consulting::Unconsulted,
        _ => Consulting::Unanswered,
    }))
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
    queue_seen: Option<u64>,
) -> Controlling {
    let Some((controlled, trust)) = answering.controlled() else {
        return Controlling::Uncontrollable;
    };

    if !command.trusted_by(trust) {
        return Controlling::Untrusted;
    }

    let applied = blocking(move || {
        Ok(match queue_seen {
            Some(revision) => controlled.control_if_the_queue_is_still(zone, revision, command),
            None => controlled.control(zone, command),
        })
    });
    match applied.await {
        Ok(Ok(())) => Controlling::Done,
        Ok(Err(uncontrolled)) => uncontrolled.into(),
        Err(error) => {
            tracing::debug!(%error, "a command from another device could not be applied");
            Controlling::Failed
        }
    }
}

async fn send_mirror(
    send: &mut SendStream,
    answering: &Answering,
    revisions: Revisions,
) -> Result<()> {
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
                    revisions,
                }
                .encoded(),
            )
            .await?;
            told = Some((zones, now));
        }
        tokio::time::sleep(MIRROR_LOOKED_AT_EVERY).await;
    }
}

pub async fn control(
    connection: &Connection,
    zone: ZoneId,
    command: RemoteCommand,
    queue_seen: Option<u64>,
) -> Controlling {
    let asked = timeout(CONTROL_ANSWERED_WITHIN, async {
        let (mut send, mut recv) = connection
            .open_bi()
            .await
            .map_err(|source| Error::Connection(Box::new(source)))?;
        let asked = ToPeer::Control {
            zone,
            command,
            queue_seen,
        };

        send_frame(&mut send, &asked.encoded()).await?;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Watching {
    Heard,
    NothingHeard,
}

pub async fn watch(
    connection: &Connection,
    device: DeviceId,
    mirrors: &RwLock<BTreeMap<DeviceId, Watched>>,
    still_watched: impl Fn() -> bool,
) -> Result<()> {
    let mut watching = Watching::NothingHeard;

    let revised = watched_as(
        connection,
        device,
        mirrors,
        &still_watched,
        Revisions::Carried,
        &mut watching,
    )
    .await;

    if watching == Watching::Heard || !still_watched() || connection.close_reason().is_some() {
        return revised;
    }
    watched_as(
        connection,
        device,
        mirrors,
        &still_watched,
        Revisions::Unsaid,
        &mut watching,
    )
    .await
}

async fn watched_as(
    connection: &Connection,
    device: DeviceId,
    mirrors: &RwLock<BTreeMap<DeviceId, Watched>>,
    still_watched: &impl Fn() -> bool,
    revisions: Revisions,
    watching: &mut Watching,
) -> Result<()> {
    let (mut send, recv) = connection
        .open_bi()
        .await
        .map_err(|source| Error::Connection(Box::new(source)))?;

    send_frame(&mut send, &ToPeer::Watch { revisions }.encoded()).await?;
    send.finish().map_err(|_| Error::StreamClosed)?;

    let mut frames = FrameReader::over(recv);
    let mut heard_at = Instant::now();

    while still_watched() && connection.close_reason().is_none() {
        let frame = match timeout(MIRROR_POLLED_EVERY, frames.next()).await {
            Err(_) if heard_at.elapsed() < MIRROR_GOES_STALE_AFTER => continue,
            Err(_) => break,
            Ok(frame) => frame?,
        };
        let Some(frame) = frame else {
            break;
        };

        heard_at = Instant::now();
        *watching = Watching::Heard;
        match FromPeer::decoded(&frame)? {
            FromPeer::Zones { zones, .. } => {
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

    frames.stop();
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
            | FromPeer::KeptEnd { .. }
            | FromPeer::Exporting { .. } => {}
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
            | FromPeer::KeptEnd { .. }
            | FromPeer::Exporting { .. } => {}
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
            | FromPeer::KeptEnd { .. }
            | FromPeer::Exporting { .. } => {}
        }
    }
    Ok(landed)
}

async fn send_song(
    send: &mut SendStream,
    catalog: Arc<dyn Catalog>,
    recording: Recording,
    offset: u64,
    bound: Bound,
) -> Result<()> {
    let opened = blocking(move || catalog.open(&recording).map_err(Error::Catalog)).await?;
    let Some(opened) = opened else {
        return send_frame(send, &FromPeer::Missing.encoded()).await;
    };
    let Bound::Carried(held) = bound else {
        let header = FromPeer::Sending {
            size: opened.size,
            extension: opened.extension,
            revised: None,
        };

        send_frame(send, &header.encoded()).await?;
        return streamed_from(send, opened.reader, offset).await;
    };

    let size = opened.size;
    let reader = opened.reader;

    let Some((reader, revision)) = blocking(move || Ok(revised(reader, size))).await? else {
        return send_frame(send, &FromPeer::Missing.encoded()).await;
    };
    let from = if held == Some(revision) && offset <= size {
        offset
    } else {
        0
    };

    let header = FromPeer::Sending {
        size,
        extension: opened.extension,
        revised: Some(Revised { revision, from }),
    };

    send_frame(send, &header.encoded()).await?;
    streamed_from(send, reader, from).await
}

fn revised(mut reader: Box<dyn ReadSeek>, size: u64) -> Option<(Box<dyn ReadSeek>, Revision)> {
    match Revision::of(&mut *reader, size) {
        Ok(revision) => Some((reader, revision)),
        Err(error) => {
            tracing::debug!(%error, "the revision of a song asked for could not be read");
            None
        }
    }
}

async fn send_export(
    send: &mut SendStream,
    answering: &Answering,
    part: ExportPart,
    offset: u64,
) -> Result<()> {
    let offered = match answering.consultable() {
        Some(consulted) => blocking(move || Ok(consulted.export(part)))
            .await
            .unwrap_or_default(),
        None => None,
    };
    let Some(offered) = offered else {
        return send_frame(send, &FromPeer::Missing.encoded()).await;
    };
    let header = FromPeer::Exporting {
        dump: offered.dump,
        sequence: offered.sequence,
        size: offered.size,
    };

    send_frame(send, &header.encoded()).await?;
    if offset >= offered.size {
        return Ok(());
    }
    streamed_from(send, offered.reader, offset).await
}

async fn streamed_from<R: Read + Seek + Send + 'static>(
    send: &mut SendStream,
    mut reader: R,
    offset: u64,
) -> Result<()> {
    let (chunks, mut arriving) = mpsc::channel::<std::io::Result<Vec<u8>>>(CHUNKS_IN_FLIGHT);
    let reading = tokio::task::spawn_blocking(move || {
        if let Err(error) = reader.seek(SeekFrom::Start(offset)) {
            let _ = chunks.blocking_send(Err(error));

            return;
        }
        loop {
            let mut chunk = vec![0; CHUNK_BYTES];
            match reader.read(&mut chunk) {
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
    holdings_within(connection, A_LISTING_GOES_QUIET_AFTER).await
}

async fn holdings_within(connection: &Connection, quiet_after: Duration) -> Result<Vec<Held>> {
    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|source| Error::Connection(Box::new(source)))?;

    send_frame(&mut send, &ToPeer::Holdings.encoded()).await?;
    send.finish().map_err(|_| Error::StreamClosed)?;

    let mut holdings = Vec::new();

    while let Some(frame) = quiet_within(quiet_after, read_frame(&mut recv)).await? {
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
            | FromPeer::KeptEnd { .. }
            | FromPeer::Exporting { .. } => {}
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
        | FromPeer::KeptEnd { .. }
        | FromPeer::Exporting { .. } => Err(Error::Unmet),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Partial {
    offset: u64,
    size: u64,
    revision: Revision,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Asking {
    Revised(Option<Partial>),
    Unrevised,
}

enum Answered {
    Whole(Fetched),
    StartOver,
    Unknown,
}

struct Fetching<'a, F> {
    connection: &'a Connection,
    device: DeviceId,
    recording: &'a Recording,
    staged: &'a Path,
    noted: &'a Path,
    received: F,
}

pub async fn fetch(
    connection: &Connection,
    device: DeviceId,
    recording: &Recording,
    staged: &Path,
    received: impl Fn(u64, u64),
) -> Result<Fetched> {
    let noted = staged.with_extension(NOTED_EXTENSION);
    let fetching = Fetching {
        connection,
        device,
        recording,
        staged,
        noted: &noted,
        received,
    };

    let partial = bound_partial(staged, &noted, device).await;

    let answered = match fetching.answered(Asking::Revised(partial)).await? {
        Answered::StartOver => fetching.answered(Asking::Revised(None)).await?,
        answered => answered,
    };
    let answered = match answered {
        Answered::Unknown => fetching.answered(Asking::Unrevised).await?,
        answered => answered,
    };
    let Answered::Whole(fetched) = answered else {
        return Err(Error::Missing(device));
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

fn song_note(device: DeviceId, size: u64, revision: Revision) -> String {
    format!("{device} {size} {}", revision.hex())
}

fn noted_revision(note: &str, device: DeviceId) -> Option<(u64, Revision)> {
    let mut fields = note.trim().split(' ');

    let (from, size, revision) = (fields.next()?, fields.next()?, fields.next()?);

    if from != device.to_string() || fields.next().is_some() {
        return None;
    }
    Some((size.parse().ok()?, Revision::from_hex(revision)?))
}

async fn quietly_short<T>(waiting: impl Future<Output = Result<T>>) -> Result<T> {
    quiet_within(A_FETCH_GOES_QUIET_AFTER, waiting).await
}

async fn quiet_within<T>(
    quiet_after: Duration,
    waiting: impl Future<Output = Result<T>>,
) -> Result<T> {
    timeout(quiet_after, waiting).await.unwrap_or_else(|_| {
        Err(Error::stream(
            StreamOp::Read,
            std::io::Error::from(std::io::ErrorKind::TimedOut),
        ))
    })
}

async fn bound_partial(staged: &Path, noted: &Path, device: DeviceId) -> Option<Partial> {
    let revised = fs::read_to_string(noted)
        .await
        .ok()
        .and_then(|note| noted_revision(&note, device));
    let held = fs::metadata(staged)
        .await
        .map(|metadata| metadata.len())
        .ok();

    match (revised, held) {
        (Some((size, revision)), Some(offset)) if offset > 0 && offset <= size => Some(Partial {
            offset,
            size,
            revision,
        }),
        _ => {
            started_over(staged, noted).await;
            None
        }
    }
}

async fn started_over(staged: &Path, noted: &Path) {
    let _ = fs::remove_file(staged).await;
    let _ = fs::remove_file(noted).await;
}

impl<F: Fn(u64, u64)> Fetching<'_, F> {
    async fn answered(&self, asked: Asking) -> Result<Answered> {
        let (mut send, mut recv) = quietly_short(async {
            self.connection
                .open_bi()
                .await
                .map_err(|source| Error::Connection(Box::new(source)))
        })
        .await?;
        let request = ToPeer::Fetch {
            recording: self.recording.clone(),
            offset: match asked {
                Asking::Revised(Some(partial)) => partial.offset,
                Asking::Revised(None) | Asking::Unrevised => 0,
            },
            bound: match asked {
                Asking::Revised(partial) => Bound::Carried(partial.map(|partial| partial.revision)),
                Asking::Unrevised => Bound::Unsaid,
            },
        };

        send_frame(&mut send, &request.encoded()).await?;
        send.finish().map_err(|_| Error::StreamClosed)?;

        let Some(header) = quietly_short(read_frame(&mut recv)).await? else {
            return match asked {
                Asking::Revised(_) => Ok(Answered::Unknown),
                Asking::Unrevised => Err(Error::Missing(self.device)),
            };
        };
        let (size, extension, revised) = match FromPeer::decoded(&header)? {
            FromPeer::Sending {
                size,
                extension,
                revised,
            } => (size, extension, revised),
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
            | FromPeer::KeptEnd { .. }
            | FromPeer::Exporting { .. } => {
                return Err(Error::Missing(self.device));
            }
        };

        if size > LARGEST_SONG_FETCHED {
            let _ = recv.stop(0_u32.into());

            return Err(Error::SongTooLarge {
                device: self.device,
                size,
            });
        }

        let Some(from) = resumed_from(asked, size, revised) else {
            let _ = recv.stop(0_u32.into());

            started_over(self.staged, self.noted).await;
            return Ok(Answered::StartOver);
        };

        match revised {
            Some(revised) => fs::write(self.noted, song_note(self.device, size, revised.revision))
                .await
                .map_err(|source| file_failed(FileOp::Stage, self.noted, source))?,
            None => {
                let _ = fs::remove_file(self.noted).await;
            }
        }

        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(from > 0)
            .truncate(from == 0)
            .open(self.staged)
            .await
            .map_err(|source| file_failed(FileOp::Stage, self.staged, source))?;

        let streamed = self.streamed_into(&mut file, &mut recv, from, size).await;

        file.sync_all()
            .await
            .map_err(|source| file_failed(FileOp::Write, self.staged, source))?;

        let have = streamed?;

        let _ = fs::remove_file(self.noted).await;

        if have != size {
            let _ = fs::remove_file(self.staged).await;

            return Err(Error::stream(
                StreamOp::Read,
                std::io::Error::from(std::io::ErrorKind::UnexpectedEof),
            ));
        }
        if let Some(revised) = revised
            && !self.holds_revision(size, revised.revision).await?
        {
            let _ = fs::remove_file(self.staged).await;

            return Err(Error::SongUnlikeItsRevision(self.device));
        }
        Ok(Answered::Whole(Fetched {
            staged: self.staged.to_path_buf(),
            extension,
            tags: SongTags::default(),
        }))
    }

    async fn streamed_into(
        &self,
        file: &mut fs::File,
        recv: &mut RecvStream,
        from: u64,
        size: u64,
    ) -> Result<u64> {
        let mut have = from;
        let mut chunk = vec![0; CHUNK_BYTES];

        loop {
            let read = quietly_short(async {
                recv.read(&mut chunk)
                    .await
                    .map_err(|source| Error::stream(StreamOp::Read, source))
            })
            .await?;
            let Some(read) = read else {
                return Ok(have);
            };

            file.write_all(&chunk[..read])
                .await
                .map_err(|source| file_failed(FileOp::Write, self.staged, source))?;
            have += u64::try_from(read).unwrap_or(u64::MAX);
            (self.received)(have, size);
            if have > size {
                return Ok(have);
            }
        }
    }

    async fn holds_revision(&self, size: u64, revision: Revision) -> Result<bool> {
        let staged = self.staged.to_path_buf();

        blocking(move || {
            let held = std::fs::File::open(&staged)
                .and_then(|mut file| Revision::of(&mut file, size))
                .map_err(|source| file_failed(FileOp::Read, &staged, source))?;

            Ok(held == revision)
        })
        .await
    }
}

fn resumed_from(asked: Asking, size: u64, revised: Option<Revised>) -> Option<u64> {
    match (asked, revised) {
        (Asking::Unrevised, None) | (Asking::Revised(_), Some(Revised { from: 0, .. })) => Some(0),
        (Asking::Revised(Some(partial)), Some(Revised { revision, from }))
            if from == partial.offset && revision == partial.revision && size == partial.size =>
        {
            Some(from)
        }
        (Asking::Unrevised, Some(_)) | (Asking::Revised(_), _) => None,
    }
}

pub struct ExportStaged<'a> {
    pub part: ExportPart,
    pub held: Option<&'a ExportHeld>,
    pub staged: &'a Path,
    pub noted: &'a Path,
}

const EXPORT_TRIES: usize = 2;

pub async fn export_from(
    connection: &Connection,
    device: DeviceId,
    asked: &ExportStaged<'_>,
    receiving: &ExportReceiving,
    allowed: &(impl Fn() -> bool + Sync),
) -> Result<ExportFetched> {
    for _ in 0..EXPORT_TRIES {
        if let Some(fetched) = export_once(connection, device, asked, receiving, allowed).await? {
            return Ok(fetched);
        }
    }
    Ok(ExportFetched::Failed)
}

fn export_note(device: DeviceId, size: u64, offered: &ExportHeld) -> String {
    format!("{device} {size} {} {}", offered.sequence, offered.dump)
}

async fn export_once(
    connection: &Connection,
    device: DeviceId,
    asked: &ExportStaged<'_>,
    receiving: &ExportReceiving,
    allowed: &(impl Fn() -> bool + Sync),
) -> Result<Option<ExportFetched>> {
    let offset = resumable(asked.staged, asked.noted, device).await;
    let (mut send, mut recv) = quietly_short(async {
        connection
            .open_bi()
            .await
            .map_err(|source| Error::Connection(Box::new(source)))
    })
    .await?;
    let request = ToPeer::Export {
        part: asked.part,
        offset,
    };

    send_frame(&mut send, &request.encoded()).await?;
    send.finish().map_err(|_| Error::StreamClosed)?;

    let Some(header) = quietly_short(read_frame(&mut recv)).await? else {
        return Ok(Some(ExportFetched::NotHeld));
    };
    let FromPeer::Exporting {
        dump,
        sequence,
        size,
    } = FromPeer::decoded(&header)?
    else {
        return Ok(Some(ExportFetched::NotHeld));
    };

    let offered = ExportHeld { dump, sequence };

    if asked.held == Some(&offered) {
        let _ = recv.stop(0_u32.into());

        return Ok(Some(ExportFetched::AlreadyHeld));
    }

    let note = export_note(device, size, &offered);

    if offset > 0 && fs::read_to_string(asked.noted).await.ok().as_deref() != Some(note.as_str()) {
        let _ = recv.stop(0_u32.into());
        let _ = fs::remove_file(asked.staged).await;
        let _ = fs::remove_file(asked.noted).await;

        return Ok(None);
    }
    fs::write(asked.noted, &note)
        .await
        .map_err(|source| file_failed(FileOp::Stage, asked.noted, source))?;

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(asked.staged)
        .await
        .map_err(|source| file_failed(FileOp::Stage, asked.staged, source))?;

    let streamed = exported_into(
        &mut file, &mut recv, asked, receiving, allowed, offset, size,
    )
    .await;

    file.sync_all()
        .await
        .map_err(|source| file_failed(FileOp::Write, asked.staged, source))?;

    let Some(have) = streamed? else {
        return Ok(Some(ExportFetched::Failed));
    };

    if have != size {
        let _ = fs::remove_file(asked.staged).await;
        let _ = fs::remove_file(asked.noted).await;

        return Err(Error::stream(
            StreamOp::Read,
            std::io::Error::from(std::io::ErrorKind::UnexpectedEof),
        ));
    }

    let _ = fs::remove_file(asked.noted).await;
    Ok(Some(ExportFetched::Whole(offered)))
}

async fn exported_into(
    file: &mut fs::File,
    recv: &mut RecvStream,
    asked: &ExportStaged<'_>,
    receiving: &ExportReceiving,
    allowed: &(impl Fn() -> bool + Sync),
    offset: u64,
    size: u64,
) -> Result<Option<u64>> {
    let mut have = offset;

    receiving.heard(have, size);

    let mut chunk = vec![0; CHUNK_BYTES];

    while have < size {
        if !allowed() {
            let _ = recv.stop(0_u32.into());

            return Ok(None);
        }

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
            .map_err(|source| file_failed(FileOp::Write, asked.staged, source))?;
        have += u64::try_from(read).unwrap_or(u64::MAX);
        receiving.heard(have, size);
    }
    Ok(Some(have))
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
    use std::{env, io::Cursor, process};

    use super::*;
    use crate::{
        LocalListen, LocalPlaylist, LocalPlaylistId, Opened, Refused, RemoteListen, Resumed,
    };

    const SONG_BYTES: usize = 4 * 1024 * 1024;
    const CUT_OFF_PAST: u64 = 256 * 1024;
    const SENDER: DeviceId = DeviceId([1; 16]);

    struct OneSong {
        bytes: Mutex<Vec<u8>>,
    }

    impl Catalog for OneSong {
        fn playlists(&self) -> std::result::Result<Vec<LocalPlaylist>, Refused> {
            Ok(Vec::new())
        }

        fn write_playlist(
            &self,
            _: Option<LocalPlaylistId>,
            _: &str,
            _: &[Recording],
        ) -> std::result::Result<LocalPlaylist, Refused> {
            Err(Refused::Unwritable)
        }

        fn remove_playlist(&self, _: LocalPlaylistId) -> std::result::Result<(), Refused> {
            Ok(())
        }

        fn favourites(&self) -> std::result::Result<Vec<Recording>, Refused> {
            Ok(Vec::new())
        }

        fn favour(&self, _: &Recording, _: bool) -> std::result::Result<bool, Refused> {
            Ok(false)
        }

        fn listens_after(
            &self,
            _: u64,
            _: usize,
        ) -> std::result::Result<Vec<LocalListen>, Refused> {
            Ok(Vec::new())
        }

        fn credit(&self, _: &RemoteListen) -> std::result::Result<bool, Refused> {
            Ok(false)
        }

        fn resumption(&self) -> std::result::Result<Option<Resumed>, Refused> {
            Ok(None)
        }

        fn holds(&self, _: &Recording) -> std::result::Result<bool, Refused> {
            Ok(true)
        }

        fn holdings(&self) -> std::result::Result<Vec<Held>, Refused> {
            Ok(Vec::new())
        }

        fn open(&self, _: &Recording) -> std::result::Result<Option<Opened>, Refused> {
            let bytes = self.bytes.lock().clone();

            Ok(Some(Opened {
                size: u64::try_from(bytes.len()).expect("a size"),
                reader: Box::new(Cursor::new(bytes)),
                extension: "flac".into(),
            }))
        }

        fn land(
            &self,
            _: &Recording,
            _: &SongTags,
            _: &str,
            _: &Path,
        ) -> std::result::Result<bool, Refused> {
            Ok(false)
        }

        fn revision(&self) -> u64 {
            0
        }
    }

    fn a_song_of(seed: u8) -> Vec<u8> {
        (0..SONG_BYTES).map(|at| (at % 251) as u8 ^ seed).collect()
    }

    fn echoes() -> Recording {
        Recording {
            mbid: None,
            title: "Echoes".into(),
            artist: "Pink Floyd".into(),
            album: "Meddle".into(),
            length: None,
        }
    }

    async fn serving(song: Arc<OneSong>) -> (Endpoint, Endpoint, SocketAddr) {
        let derived = crate::SyncKey::minted()
            .and_then(|key| key.derived())
            .expect("a key");
        let identity = crate::quic::PeerIdentity::of(&derived).expect("an identity");
        let sending = Endpoint::server(
            identity.serving().expect("serving"),
            SocketAddr::from(([127, 0, 0, 1], 0)),
        )
        .expect("a device listening");
        let mut asking = Endpoint::client(SocketAddr::from(([127, 0, 0, 1], 0))).expect("a client");
        let (heard, _) = mpsc::unbounded_channel();
        let answering = Arc::new(Answering {
            catalog: song,
            consulted: RwLock::new(None),
            controlled: RwLock::new(None),
            ledger: Arc::new(Mutex::new(Ledger::in_memory().expect("a ledger"))),
            device: SENDER,
            heard,
            transfers_allowed: Arc::new(AtomicBool::new(true)),
        });

        asking.set_default_client_config(identity.dialling().expect("dialling"));

        let address = sending.local_addr().expect("an address");
        let listening = sending.clone();

        tokio::spawn(async move {
            while let Some(incoming) = listening.accept().await {
                if let Ok(connection) = incoming.await {
                    tokio::spawn(serve(connection, Arc::clone(&answering), None));
                }
            }
        });
        (sending, asking, address)
    }

    async fn dialled_at(asking: &Endpoint, address: SocketAddr) -> Connection {
        asking
            .connect(address, PEER_NAME)
            .expect("dialled")
            .await
            .expect("connected")
    }

    async fn cut_off(connection: &Connection, staged: &Path) -> u64 {
        let cutting = connection.clone();

        let fetched = fetch(connection, SENDER, &echoes(), staged, move |have, _| {
            if have >= CUT_OFF_PAST {
                cutting.close(0_u32.into(), b"cut off");
            }
        })
        .await;

        assert!(fetched.is_err());
        std::fs::metadata(staged).expect("a partial").len()
    }

    async fn resumed(connection: &Connection, staged: &Path) -> (Vec<u8>, Vec<u64>) {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let hearing = Arc::clone(&heard);

        let fetched = fetch(connection, SENDER, &echoes(), staged, move |have, _| {
            hearing.lock().push(have);
        })
        .await
        .expect("the song whole");
        let bytes = std::fs::read(&fetched.staged).expect("the song staged");

        let heard = heard.lock().clone();
        (bytes, heard)
    }

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

    async fn met_answering(
        answer: impl Fn(&[u8]) -> Option<Vec<u8>> + Send + Sync + 'static,
    ) -> (Endpoint, Endpoint, Connection) {
        let answer = Arc::new(answer);

        met_streams(move |mut send, mut recv| {
            let answer = Arc::clone(&answer);

            async move {
                let Ok(Some(asked)) = read_frame(&mut recv).await else {
                    return;
                };
                let Some(told) = answer(&asked) else {
                    drop(send);
                    return;
                };
                let _ = send.write_all(&told).await;
                let _ = send.finish();
            }
        })
        .await
    }

    async fn met_streams<Handled>(
        handle: impl Fn(SendStream, RecvStream) -> Handled + Send + 'static,
    ) -> (Endpoint, Endpoint, Connection)
    where
        Handled: Future<Output = ()> + Send + 'static,
    {
        let derived = crate::SyncKey::minted()
            .and_then(|key| key.derived())
            .expect("a key");
        let identity = crate::quic::PeerIdentity::of(&derived).expect("an identity");
        let answering = Endpoint::server(
            identity.serving().expect("serving"),
            SocketAddr::from(([127, 0, 0, 1], 0)),
        )
        .expect("a device listening");
        let mut asking = Endpoint::client(SocketAddr::from(([127, 0, 0, 1], 0))).expect("a client");

        asking.set_default_client_config(identity.dialling().expect("dialling"));

        let address = answering.local_addr().expect("an address");

        let listening = answering.clone();

        tokio::spawn(async move {
            let Some(incoming) = listening.accept().await else {
                return;
            };
            let Ok(connection) = incoming.await else {
                return;
            };
            while let Ok((send, recv)) = connection.accept_bi().await {
                tokio::spawn(handle(send, recv));
            }
        });

        let connection = asking
            .connect(address, PEER_NAME)
            .expect("dialled")
            .await
            .expect("connected");

        (answering, asking, connection)
    }

    async fn met_by_an_older_build(mirrored: RemoteZone) -> (Endpoint, Endpoint, Connection) {
        let a_plain_watch = ToPeer::Watch {
            revisions: Revisions::Unsaid,
        }
        .encoded();
        met_answering(move |asked| {
            (asked == a_plain_watch).then(|| {
                let told = FromPeer::Zones {
                    zones: vec![mirrored.clone()],
                    revisions: Revisions::Unsaid,
                };
                framed(&told.encoded())
            })
        })
        .await
    }

    fn framed(payload: &[u8]) -> Vec<u8> {
        let mut framed = u32::try_from(payload.len())
            .expect("a frame")
            .to_be_bytes()
            .to_vec();

        framed.extend_from_slice(payload);
        framed
    }

    fn a_mirrored_zone(name: &str) -> RemoteZone {
        RemoteZone {
            zone: ZoneId::FIRST,
            name: name.into(),
            playback: crate::RemotePlayback::Paused,
            playing: None,
            position: Duration::ZERO,
            length: None,
            volume: crate::Permille::WHOLE,
            queue_length: 0,
            first_row: 0,
            rows: Vec::new(),
            queue_revision: None,
        }
    }

    fn mirrored_names(
        mirrors: &RwLock<BTreeMap<DeviceId, Watched>>,
        device: DeviceId,
    ) -> Vec<String> {
        mirrors
            .read()
            .get(&device)
            .and_then(Watched::mirror)
            .map(|mirror| mirror.zones.iter().map(|zone| zone.name.clone()).collect())
            .unwrap_or_default()
    }

    async fn mirrored_as(
        mirrors: &RwLock<BTreeMap<DeviceId, Watched>>,
        device: DeviceId,
        names: &[&str],
    ) {
        timeout(Duration::from_secs(10), async {
            while mirrored_names(mirrors, device) != names {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("the mirror reads as told");
    }

    #[tokio::test]
    async fn a_mirror_frame_split_across_the_polling_deadline_reaches_the_mirror_intact() {
        let first = framed(
            &FromPeer::Zones {
                zones: vec![a_mirrored_zone("Desk")],
                revisions: Revisions::Carried,
            }
            .encoded(),
        );
        let second = framed(
            &FromPeer::Zones {
                zones: vec![a_mirrored_zone("Kitchen")],
                revisions: Revisions::Carried,
            }
            .encoded(),
        );
        let split_at = first.len() / 2;
        let (_answering, _asking, connection) = met_streams(move |mut send, mut recv| {
            let first = first.clone();
            let second = second.clone();

            async move {
                let Ok(Some(_)) = read_frame(&mut recv).await else {
                    return;
                };
                let _ = send.write_all(&first[..split_at]).await;

                tokio::time::sleep(MIRROR_POLLED_EVERY + Duration::from_millis(400)).await;

                let _ = send.write_all(&first[split_at..]).await;

                tokio::time::sleep(Duration::from_millis(300)).await;

                let _ = send.write_all(&second).await;
                let _ = send.finish();
            }
        })
        .await;
        let mirrors = Arc::new(RwLock::new(BTreeMap::new()));
        let device = DeviceId([3; 16]);

        let watching = tokio::spawn({
            let mirrors = Arc::clone(&mirrors);

            async move { watch(&connection, device, &mirrors, || true).await }
        });

        mirrored_as(&mirrors, device, &["Desk"]).await;
        mirrored_as(&mirrors, device, &["Kitchen"]).await;

        let ended = timeout(Duration::from_secs(10), watching)
            .await
            .expect("the watch ended with the stream")
            .expect("the watch task");

        assert!(ended.is_ok());
    }

    #[tokio::test]
    async fn a_device_that_stops_listing_its_songs_is_given_up_on_and_asked_again_on_the_same_connection()
     {
        let quiet_after = Duration::from_millis(400);
        let held = Held {
            recording: echoes(),
            size: 1,
            extension: "flac".into(),
        };
        let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let listing = held.clone();
        let (_answering, _asking, connection) = met_streams(move |mut send, mut recv| {
            let listing = listing.clone();
            let times = asked.fetch_add(1, Ordering::SeqCst);

            async move {
                let Ok(Some(_)) = read_frame(&mut recv).await else {
                    return;
                };
                let items = FromPeer::Held {
                    items: vec![listing],
                };
                let _ = send_frame(&mut send, &items.encoded()).await;

                if times == 0 {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    return;
                }

                let _ = send_frame(&mut send, &FromPeer::HeldEnd.encoded()).await;
                let _ = send.finish();
            }
        })
        .await;

        let stalled = timeout(
            Duration::from_secs(10),
            holdings_within(&connection, quiet_after),
        )
        .await
        .expect("the listing was given up on");

        assert!(matches!(
            stalled,
            Err(Error::Stream {
                op: StreamOp::Read,
                ..
            })
        ));

        let again = timeout(
            Duration::from_secs(10),
            holdings_within(&connection, quiet_after),
        )
        .await
        .expect("the second listing ended")
        .expect("listed");

        assert_eq!(again, vec![held]);
    }

    #[tokio::test]
    async fn a_song_announced_past_the_largest_one_fetched_is_refused_before_anything_is_staged() {
        let announced = LARGEST_SONG_FETCHED + 1;
        let (_answering, _asking, connection) = met_answering(move |_| {
            let mut told = framed(
                &FromPeer::Sending {
                    size: announced,
                    extension: "flac".into(),
                    revised: None,
                }
                .encoded(),
            );

            told.extend_from_slice(&[0; 4096]);
            Some(told)
        })
        .await;
        let staged = staged_in("oversized");
        let noted = staged.with_extension(NOTED_EXTENSION);
        let device = DeviceId([5; 16]);
        let recording = Recording {
            mbid: None,
            title: "Echoes".into(),
            artist: "Pink Floyd".into(),
            album: "Meddle".into(),
            length: None,
        };

        let fetched = fetch(&connection, device, &recording, &staged, |_, _| {}).await;

        assert!(matches!(
            fetched,
            Err(Error::SongTooLarge { size, .. }) if size == announced
        ));
        assert!(!staged.exists());
        assert!(!noted.exists());
    }

    #[tokio::test]
    async fn a_device_too_old_to_carry_queue_revisions_is_watched_as_it_always_was() {
        let mirrored = RemoteZone {
            zone: ZoneId::FIRST,
            name: "Desk".into(),
            playback: crate::RemotePlayback::Paused,
            playing: None,
            position: Duration::ZERO,
            length: None,
            volume: crate::Permille::WHOLE,
            queue_length: 0,
            first_row: 0,
            rows: Vec::new(),
            queue_revision: None,
        };
        let (_older, _newer, connection) = met_by_an_older_build(mirrored.clone()).await;
        let mirrors = RwLock::new(BTreeMap::new());
        let device = DeviceId([3; 16]);

        timeout(
            Duration::from_secs(10),
            watch(&connection, device, &mirrors, || true),
        )
        .await
        .expect("the watch ended with the older device's stream")
        .expect("watched");

        let held = mirrors.read().get(&device).cloned();

        assert_eq!(
            held.as_ref()
                .and_then(Watched::mirror)
                .map(|mirror| mirror.zones.clone()),
            Some(vec![mirrored])
        );
    }

    #[tokio::test]
    async fn a_device_too_old_to_date_its_answers_is_consulted_as_it_always_was() {
        let a_plain_consult = ToPeer::Consult {
            asked: b"who sang creep".to_vec(),
            dates: Dates::Unsaid,
        }
        .encoded();
        let (_older, _newer, connection) = met_answering(move |asked| {
            (asked == a_plain_consult).then(|| {
                framed(
                    &FromPeer::Consulted {
                        answer: b"radiohead".to_vec(),
                        at: None,
                    }
                    .encoded(),
                )
            })
        })
        .await;

        let consulted = consult(&connection, b"who sang creep".to_vec()).await;

        assert_eq!(
            consulted,
            Consulting::Answered {
                answer: b"radiohead".to_vec(),
                at: None,
            }
        );
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

    #[tokio::test]
    async fn an_unchanged_song_cut_off_midway_is_resumed_to_its_original_bytes() {
        let song = a_song_of(0);
        let held = Arc::new(OneSong {
            bytes: Mutex::new(song.clone()),
        });
        let (_sending, asking, address) = serving(held).await;
        let staged = staged_in("unchanged");
        let noted = staged.with_extension(NOTED_EXTENSION);

        let first = dialled_at(&asking, address).await;
        let partial = cut_off(&first, &staged).await;

        assert!(partial >= CUT_OFF_PAST);
        assert!(partial < u64::try_from(SONG_BYTES).expect("a size"));
        assert!(noted.exists());

        let second = dialled_at(&asking, address).await;
        let (bytes, heard) = resumed(&second, &staged).await;

        assert!(bytes == song, "the resumed song differs from the one sent");
        assert!(heard.iter().all(|have| *have > partial), "{heard:?}");
        assert!(!noted.exists());
    }

    #[tokio::test]
    async fn a_song_replaced_at_the_same_size_while_cut_off_is_fetched_again_from_zero() {
        let song = a_song_of(0);
        let replaced = a_song_of(0x5a);
        let held = Arc::new(OneSong {
            bytes: Mutex::new(song),
        });
        let (_sending, asking, address) = serving(Arc::clone(&held)).await;
        let staged = staged_in("replaced");

        let first = dialled_at(&asking, address).await;
        let partial = cut_off(&first, &staged).await;

        *held.bytes.lock() = replaced.clone();

        let second = dialled_at(&asking, address).await;
        let (bytes, heard) = resumed(&second, &staged).await;

        assert!(partial >= CUT_OFF_PAST);
        assert!(bytes == replaced, "the old song's start was kept");
        assert!(
            heard.first().is_some_and(|have| *have < partial),
            "{heard:?}"
        );
    }

    #[tokio::test]
    async fn a_device_too_old_to_name_a_revision_sends_the_song_from_zero_whatever_is_staged() {
        let song = a_song_of(0);
        let size = u64::try_from(song.len()).expect("a size");
        let unbound_from_zero = ToPeer::Fetch {
            recording: echoes(),
            offset: 0,
            bound: Bound::Unsaid,
        }
        .encoded();
        let sent = song.clone();

        let (_older, _newer, connection) = met_answering(move |asked| {
            (asked == unbound_from_zero).then(|| {
                let mut told = framed(
                    &FromPeer::Sending {
                        size,
                        extension: "flac".into(),
                        revised: None,
                    }
                    .encoded(),
                );

                told.extend_from_slice(&sent);
                told
            })
        })
        .await;
        let staged = staged_in("older");
        let noted = staged.with_extension(NOTED_EXTENSION);
        let staged_by_a_newer_build = a_song_of(0x5a);
        let revision =
            Revision::of(&mut Cursor::new(&staged_by_a_newer_build), size).expect("a revision");

        std::fs::write(&staged, &staged_by_a_newer_build[..4096]).expect("a partial");
        std::fs::write(&noted, song_note(SENDER, size, revision)).expect("a note");

        let fetched = fetch(&connection, SENDER, &echoes(), &staged, |_, _| {})
            .await
            .expect("the song whole");
        let bytes = std::fs::read(&fetched.staged).expect("the song staged");

        assert!(
            bytes == song,
            "a partial was kept for a device naming no revision"
        );
        assert!(!noted.exists());
    }

    #[tokio::test]
    async fn a_song_unlike_the_revision_its_sender_announced_is_refused_and_nothing_stays_staged() {
        let announced = a_song_of(0);
        let sent = a_song_of(0x5a);
        let size = u64::try_from(sent.len()).expect("a size");

        let revision = Revision::of(&mut Cursor::new(&announced), size).expect("a revision");
        let (_sending, _asking, connection) = met_answering(move |_| {
            let mut told = framed(
                &FromPeer::Sending {
                    size,
                    extension: "flac".into(),
                    revised: Some(Revised { revision, from: 0 }),
                }
                .encoded(),
            );

            told.extend_from_slice(&sent);
            Some(told)
        })
        .await;
        let staged = staged_in("unlike");
        let noted = staged.with_extension(NOTED_EXTENSION);

        let fetched = fetch(&connection, SENDER, &echoes(), &staged, |_, _| {}).await;

        assert!(matches!(
            fetched,
            Err(Error::SongUnlikeItsRevision(device)) if device == SENDER
        ));
        assert!(!staged.exists());
        assert!(!noted.exists());
    }

    #[tokio::test]
    async fn a_partial_noted_by_an_earlier_build_naming_no_revision_is_started_over() {
        let staged = staged_in("unrevised");
        let noted = staged.with_extension(NOTED_EXTENSION);
        let revision = Revision([3; crate::revision::REVISION_BYTES]);

        std::fs::write(&staged, [0_u8; 100]).expect("a partial");
        std::fs::write(&noted, format!("{SENDER} 4000")).expect("a note an earlier build wrote");

        let earlier = bound_partial(&staged, &noted, SENDER).await;

        std::fs::write(&staged, [0_u8; 100]).expect("a partial");
        std::fs::write(&noted, song_note(SENDER, 4000, revision)).expect("a note");

        let bound = bound_partial(&staged, &noted, SENDER).await;
        let of_another = bound_partial(&staged, &noted, DeviceId([2; 16])).await;

        assert_eq!(earlier, None);
        assert_eq!(
            bound,
            Some(Partial {
                offset: 100,
                size: 4000,
                revision,
            })
        );
        assert_eq!(of_another, None);
        assert!(!staged.exists());
        assert!(!noted.exists());
    }
}
