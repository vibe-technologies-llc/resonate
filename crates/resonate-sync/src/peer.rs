use std::{
    io::{Read, Seek, SeekFrom},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use parking_lot::Mutex;
use quinn::{ClientConfig, Connection, Endpoint, RecvStream, SendStream};
use resonate_core::Recording;
use tokio::{
    fs,
    io::AsyncWriteExt,
    sync::mpsc::{self, UnboundedSender},
    task::JoinSet,
    time::timeout,
};

use crate::{
    Catalog, Change, DeviceId, Error, FileOp, Held, Ledger, Result, StreamOp,
    quic::{PEER_NAME, read_frame, send_frame},
    wire::{FromPeer, ToPeer},
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
            | FromPeer::Taken => {}
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
        | FromPeer::Taken => Err(Error::Unmet),
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
    if let Some(fetched) = fetched_from(
        connection, device, recording, staged, &noted, offset, &received,
    )
    .await?
    {
        return Ok(fetched);
    }
    fetched_from(connection, device, recording, staged, &noted, 0, &received)
        .await?
        .ok_or(Error::Missing(device))
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
        | FromPeer::Taken => {
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
