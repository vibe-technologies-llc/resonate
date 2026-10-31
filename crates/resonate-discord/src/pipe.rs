use std::{
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::Duration,
};

use crossbeam_channel::{Receiver, RecvTimeoutError, TryRecvError, unbounded};
use interprocess::{
    ConnectWaitMode,
    os::windows::named_pipe::{DuplexPipeStream, pipe_mode},
};
use resonate_core::AppId;

use crate::{
    Error, IpcOp, Result,
    session::{self, ANSWERS_WITHIN, Wire},
};

const PIPE_FOLDER: &str = r"\\.\pipe";
const PIPE_STEM: &str = "discord-ipc-";
const PIPES_IN_ALL: u8 = 10;
const READ_AT_ONCE: usize = 4096;
const READER_THREAD: &str = "resonate-discord-pipe";

type Stream = DuplexPipeStream<pipe_mode::Bytes>;

pub(crate) type Session = session::Session<Pipe>;

pub(crate) fn candidates() -> Vec<PathBuf> {
    (0..PIPES_IN_ALL)
        .map(|number| Path::new(PIPE_FOLDER).join(format!("{PIPE_STEM}{number}")))
        .collect()
}

pub(crate) struct Pipe {
    stream: Arc<Stream>,
    chunks: Receiver<io::Result<Vec<u8>>>,
}

impl Pipe {
    fn connect(path: &Path) -> Result<Self> {
        let stream = Stream::connect_by_path_with_wait_mode(
            path.as_os_str(),
            ConnectWaitMode::Timeout(ANSWERS_WITHIN),
        )
        .map_err(|source| Error::Socket {
            op: IpcOp::Connect,
            source,
        })?;

        let stream = Arc::new(stream);
        let (sender, chunks) = unbounded();
        let reading = Arc::clone(&stream);
        thread::Builder::new()
            .name(READER_THREAD.into())
            .spawn(move || {
                let mut chunk = [0; READ_AT_ONCE];
                loop {
                    let read = (&*reading).read(&mut chunk);
                    let ended = !matches!(read, Ok(1..));
                    let sent = match read {
                        Ok(0) => sender.send(Err(io::ErrorKind::UnexpectedEof.into())),
                        Ok(length) => sender.send(Ok(chunk[..length].to_vec())),
                        Err(error) => sender.send(Err(error)),
                    };
                    if ended || sent.is_err() {
                        break;
                    }
                }
            })
            .map_err(|source| Error::Socket {
                op: IpcOp::Configure,
                source,
            })?;

        Ok(Self { stream, chunks })
    }

    fn taken(arrived: io::Result<Vec<u8>>, into: &mut Vec<u8>) -> Result<()> {
        match arrived {
            Ok(chunk) => {
                into.extend_from_slice(&chunk);
                Ok(())
            }
            Err(source) => Err(Error::Socket {
                op: IpcOp::Read,
                source,
            }),
        }
    }
}

impl Write for Pipe {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        (&*self.stream).write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        (*self.stream).flush()
    }
}

impl Wire for Pipe {
    fn receive(&mut self, within: Duration, into: &mut Vec<u8>) -> Result<()> {
        match self.chunks.recv_timeout(within) {
            Ok(arrived) => Self::taken(arrived, into),
            Err(RecvTimeoutError::Timeout) => Ok(()),
            Err(RecvTimeoutError::Disconnected) => Err(Error::Socket {
                op: IpcOp::Read,
                source: io::ErrorKind::UnexpectedEof.into(),
            }),
        }
    }

    fn poll(&mut self, into: &mut Vec<u8>) -> Result<()> {
        loop {
            match self.chunks.try_recv() {
                Ok(arrived) => Self::taken(arrived, into)?,
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) => {
                    return Err(Error::Socket {
                        op: IpcOp::Read,
                        source: io::ErrorKind::UnexpectedEof.into(),
                    });
                }
            }
        }
    }
}

impl Session {
    pub(crate) fn find(app: AppId, answered_last: Option<&Path>) -> Result<(Self, PathBuf)> {
        let first = answered_last.map(Path::to_path_buf);
        let rest = candidates()
            .into_iter()
            .filter(|path| Some(path.as_path()) != answered_last);
        Self::find_among(app, first.into_iter().chain(rest))
    }

    fn find_among(app: AppId, paths: impl IntoIterator<Item = PathBuf>) -> Result<(Self, PathBuf)> {
        let mut answered = None;
        for path in paths {
            match Self::open(&path, app) {
                Ok(session) => {
                    tracing::debug!(path = %path.display(), "reached Discord");
                    return Ok((session, path));
                }
                Err(error) if error.names_no_application() => return Err(error),
                Err(Error::Socket { .. }) => {}
                Err(error) => {
                    tracing::debug!(%error, path = %path.display(), "a Discord pipe would not take this client; trying the next");
                    answered.get_or_insert(error);
                }
            }
        }
        Err(answered.unwrap_or(Error::NoDiscord))
    }

    pub(crate) fn open(path: &Path, app: AppId) -> Result<Self> {
        Self::begin(Pipe::connect(path)?, app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_pipes_are_tried_by_the_name_discord_gives_them() {
        let names: Vec<String> = candidates()
            .iter()
            .map(|path| path.display().to_string())
            .collect();

        assert_eq!(names.len(), 10);
        assert_eq!(names[0], r"\\.\pipe\discord-ipc-0");
        assert_eq!(names[9], r"\\.\pipe\discord-ipc-9");
    }
}
