use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use resonate_core::AppId;
use resonate_win32::{ClientPipe, Received};

use crate::{
    Error, IpcOp, Result,
    session::{self, ANSWERS_WITHIN, Wire},
};

const PIPE_FOLDER: &str = r"\\.\pipe";
const PIPE_STEM: &str = "discord-ipc-";
const PIPES_IN_ALL: u8 = 10;
const READ_AT_ONCE: usize = 4096;

pub(crate) type Session = session::Session<Pipe>;

pub(crate) fn candidates() -> Vec<PathBuf> {
    (0..PIPES_IN_ALL)
        .map(|number| Path::new(PIPE_FOLDER).join(format!("{PIPE_STEM}{number}")))
        .collect()
}

pub(crate) struct Pipe(ClientPipe);

impl Pipe {
    fn connect(path: &Path) -> Result<Self> {
        ClientPipe::open(path, ANSWERS_WITHIN)
            .map(Self)
            .map_err(|source| Error::Socket {
                op: IpcOp::Connect,
                source,
            })
    }

    fn read_within(&self, within: Duration, into: &mut Vec<u8>) -> Result<()> {
        let mut chunk = [0; READ_AT_ONCE];

        match self.0.read_within(&mut chunk, within) {
            Ok(Received::Bytes(read)) => {
                into.extend_from_slice(&chunk[..read]);
                Ok(())
            }
            Ok(Received::Nothing) => Ok(()),
            Ok(Received::Ended) => Err(Error::Socket {
                op: IpcOp::Read,
                source: io::ErrorKind::UnexpectedEof.into(),
            }),
            Err(source) => Err(Error::Socket {
                op: IpcOp::Read,
                source,
            }),
        }
    }
}

impl Write for Pipe {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.0.write_within(buffer, ANSWERS_WITHIN)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Wire for Pipe {
    fn receive(&mut self, within: Duration, into: &mut Vec<u8>) -> Result<()> {
        self.read_within(within, into)
    }

    fn poll(&mut self, into: &mut Vec<u8>) -> Result<()> {
        self.read_within(Duration::ZERO, into)
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
    use std::{
        io::Read as _,
        process,
        sync::mpsc,
        thread::{self, JoinHandle},
        time::Instant,
    };

    use interprocess::os::windows::named_pipe::{
        DuplexPipeStream, PipeListenerOptions, PipeMode, pipe_mode,
    };
    use serde_json::json;

    use super::*;
    use crate::frame::{Opcode, write_frame};

    const APP: &str = "1234567890123456789";
    const SMALLEST_BUFFER: u32 = 1;
    const SENT_AT_MOST: usize = 100_000;
    const LEEWAY: Duration = Duration::from_secs(1);

    type Served = DuplexPipeStream<pipe_mode::Bytes>;

    fn app() -> AppId {
        AppId::parse(APP).expect("a snowflake")
    }

    fn pipe_named(what: &str) -> PathBuf {
        Path::new(PIPE_FOLDER).join(format!("resonate-discord-{what}-{}", process::id()))
    }

    fn discord_at(path: &Path, then: impl FnOnce(Served) + Send + 'static) -> JoinHandle<()> {
        let listener = PipeListenerOptions::new()
            .path(path)
            .mode(PipeMode::Bytes)
            .input_buffer_size_hint(SMALLEST_BUFFER)
            .create_duplex::<pipe_mode::Bytes>()
            .expect("a pipe served");
        let ready =
            serde_json::to_vec(&json!({ "cmd": "DISPATCH", "evt": "READY", "data": { "v": 1 } }))
                .expect("json");

        thread::spawn(move || {
            let mut served = listener.accept().expect("a client");

            write_frame(&mut served, Opcode::Frame, &ready).expect("READY sent");
            then(served);
        })
    }

    #[test]
    fn a_discord_that_never_reads_ends_the_session_within_the_answer_time() {
        let path = pipe_named("unread");
        let (finished, until_finished) = mpsc::channel::<()>();

        let discord = discord_at(&path, move |_held| {
            let _ = until_finished.recv();
        });

        let mut session = Session::open(&path, app()).expect("READY");
        let mut last = Duration::ZERO;
        let mut ended = None;

        for _ in 0..SENT_AT_MOST {
            let began = Instant::now();
            let sent = session.set(None);

            last = began.elapsed();
            if let Err(error) = sent {
                ended = Some(error);
                break;
            }
        }
        drop(session);
        finished.send(()).expect("the fake Discord waits");
        discord.join().expect("the fake Discord ends");

        assert!(
            matches!(
                ended,
                Some(Error::Socket { op: IpcOp::Write, ref source })
                    if source.kind() == io::ErrorKind::TimedOut
            ),
            "{ended:?}"
        );
        assert!(last < ANSWERS_WITHIN + LEEWAY, "{last:?}");
    }

    #[test]
    fn a_session_dropped_lets_go_of_its_pipe_at_once() {
        let path = pipe_named("dropped");
        let (heard, hearing) = mpsc::channel();

        let discord = discord_at(&path, move |mut held| {
            let mut everything = Vec::new();

            let _ = held.read_to_end(&mut everything);
            let _ = heard.send(());
        });

        let session = Session::open(&path, app()).expect("READY");

        drop(session);

        let let_go = hearing.recv_timeout(ANSWERS_WITHIN);

        assert!(let_go.is_ok(), "the pipe is still held");

        discord.join().expect("the fake Discord ends");
    }

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
