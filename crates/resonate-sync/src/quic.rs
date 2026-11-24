#[cfg(feature = "device")]
mod device;

use std::time::Duration;

use quinn::{IdleTimeout, RecvStream, SendStream, TransportConfig, VarInt};

#[cfg(feature = "device")]
pub use self::device::{
    FrameReader, KEEP_THE_SERVER_EVERY, PEER_NAME, PeerIdentity, check_trusted_certificate,
    to_the_server, trusted_roots,
};
use crate::{Error, Result, StreamOp, wire::LARGEST_FRAME};

const FRAME_LENGTH_BYTES: usize = 4;
const A_SERVER_CONNECTION_BUFFERS_AT_MOST: u32 = 1_250_000;
const STREAMS_A_SERVER_SESSION_TAKES: u32 = 1;

fn transport(keep_alive: Duration, silent_for: Duration) -> TransportConfig {
    let mut transport = TransportConfig::default();

    transport.keep_alive_interval(Some(keep_alive));
    transport.max_idle_timeout(IdleTimeout::try_from(silent_for).ok());
    transport
}

pub fn server_transport(keep_alive: Duration, silent_for: Duration) -> TransportConfig {
    let window = VarInt::from_u32(A_SERVER_CONNECTION_BUFFERS_AT_MOST);
    let mut transport = transport(keep_alive, silent_for);

    transport.max_concurrent_bidi_streams(VarInt::from_u32(STREAMS_A_SERVER_SESSION_TAKES));
    transport.max_concurrent_uni_streams(VarInt::from_u32(0));
    transport.stream_receive_window(window);
    transport.receive_window(window);
    transport.datagram_receive_buffer_size(None);
    transport
}

pub async fn send_frame(stream: &mut SendStream, payload: &[u8]) -> Result<()> {
    let length = u32::try_from(payload.len()).map_err(|_| Error::Oversized {
        length: payload.len(),
    })?;

    if payload.len() > LARGEST_FRAME {
        return Err(Error::Oversized {
            length: payload.len(),
        });
    }
    stream
        .write_all(&length.to_be_bytes())
        .await
        .map_err(|source| Error::stream(StreamOp::Write, source))?;
    stream
        .write_all(payload)
        .await
        .map_err(|source| Error::stream(StreamOp::Write, source))
}

pub async fn read_frame(stream: &mut RecvStream) -> Result<Option<Vec<u8>>> {
    read_frame_at_most(stream, LARGEST_FRAME).await
}

pub async fn read_frame_at_most(stream: &mut RecvStream, most: usize) -> Result<Option<Vec<u8>>> {
    let read = read_frame_held(stream, most, |_| Some(())).await?;
    Ok(read.map(|(payload, ())| payload))
}

pub async fn read_frame_held<H>(
    stream: &mut RecvStream,
    most: usize,
    hold: impl FnOnce(usize) -> Option<H>,
) -> Result<Option<(Vec<u8>, H)>> {
    let mut length = [0; FRAME_LENGTH_BYTES];

    match stream.read_exact(&mut length).await {
        Ok(()) => {}
        Err(quinn::ReadExactError::FinishedEarly(0)) => return Ok(None),
        Err(quinn::ReadExactError::FinishedEarly(_)) => {
            return Err(Error::stream(
                StreamOp::Read,
                std::io::Error::from(std::io::ErrorKind::UnexpectedEof),
            ));
        }
        Err(quinn::ReadExactError::ReadError(source)) => {
            return Err(Error::stream(StreamOp::Read, source));
        }
    }

    let length = usize::try_from(u32::from_be_bytes(length)).unwrap_or(usize::MAX);

    if length > most.min(LARGEST_FRAME) {
        return Err(Error::Oversized { length });
    }

    let held = hold(length).ok_or(Error::NoRoom { length })?;
    let mut payload = vec![0; length];
    match stream.read_exact(&mut payload).await {
        Ok(()) => Ok(Some((payload, held))),
        Err(quinn::ReadExactError::FinishedEarly(_)) => Err(Error::stream(
            StreamOp::Read,
            std::io::Error::from(std::io::ErrorKind::UnexpectedEof),
        )),
        Err(quinn::ReadExactError::ReadError(source)) => Err(Error::stream(StreamOp::Read, source)),
    }
}
