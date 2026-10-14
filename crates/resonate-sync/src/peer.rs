use std::{
    io::{Read, Seek, SeekFrom},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use quinn::{ClientConfig, Connection, Endpoint, RecvStream, SendStream};
use resonate_core::Recording;
use tokio::{fs, io::AsyncWriteExt, sync::mpsc, task::JoinSet, time::timeout};

use crate::{
    Catalog, DeviceId, Error, FileOp, Held, Result, StreamOp,
    quic::{PEER_NAME, read_frame, send_frame},
    wire::{FromPeer, ToPeer},
};

pub const DIAL_WITHIN: Duration = Duration::from_secs(8);
const CHUNK_BYTES: usize = 64 * 1024;
const HELD_PER_FRAME: usize = 512;
const CHUNKS_IN_FLIGHT: usize = 8;

pub async fn dialled(
    endpoint: &Endpoint,
    config: &ClientConfig,
    candidates: &[SocketAddr],
    device: DeviceId,
) -> Result<Connection> {
    let mut attempts = JoinSet::new();
    for candidate in candidates {
        let connecting = endpoint
            .connect_with(config.clone(), *candidate, PEER_NAME)
            .map_err(|source| Error::Connect(Box::new(source)));
        match connecting {
            Ok(connecting) => {
                attempts.spawn(async move { timeout(DIAL_WITHIN, connecting).await });
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

pub async fn serve(connection: Connection, catalog: Arc<dyn Catalog>) {
    loop {
        let (send, recv) = match connection.accept_bi().await {
            Ok(streams) => streams,
            Err(error) => {
                tracing::debug!(%error, "a peer went away");
                return;
            }
        };
        let catalog = Arc::clone(&catalog);
        tokio::spawn(async move {
            if let Err(error) = answer(send, recv, catalog).await {
                tracing::debug!(%error, "a peer's request could not be answered");
            }
        });
    }
}

async fn answer(
    mut send: SendStream,
    mut recv: RecvStream,
    catalog: Arc<dyn Catalog>,
) -> Result<()> {
    let Some(request) = read_frame(&mut recv).await? else {
        return Ok(());
    };
    match ToPeer::decoded(&request)? {
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
            send_song(&mut send, catalog, recording, offset).await?;
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
            FromPeer::Sending { .. } | FromPeer::Missing => {}
        }
    }
    Ok(holdings)
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
    let offset = fs::metadata(staged)
        .await
        .map(|metadata| metadata.len())
        .unwrap_or_default();
    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|source| Error::Connection(Box::new(source)))?;
    let request = ToPeer::Fetch {
        recording: recording.clone(),
        offset,
    };
    send_frame(&mut send, &request.encoded()).await?;
    send.finish().map_err(|_| Error::StreamClosed)?;

    let Some(header) = read_frame(&mut recv).await? else {
        return Err(Error::Missing(device));
    };
    let (size, extension) = match FromPeer::decoded(&header)? {
        FromPeer::Sending { size, extension } => (size, extension),
        FromPeer::Missing | FromPeer::Held { .. } | FromPeer::HeldEnd => {
            return Err(Error::Missing(device));
        }
    };

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(staged)
        .await
        .map_err(|source| file_failed(FileOp::Stage, staged, source))?;
    let mut have = offset;
    let mut chunk = vec![0; CHUNK_BYTES];
    loop {
        let read = recv
            .read(&mut chunk)
            .await
            .map_err(|source| Error::stream(StreamOp::Read, source))?;
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
    if have != size {
        let _ = fs::remove_file(staged).await;
        return Err(Error::stream(
            StreamOp::Read,
            std::io::Error::from(std::io::ErrorKind::UnexpectedEof),
        ));
    }
    Ok(Fetched {
        staged: staged.to_path_buf(),
        extension,
    })
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
