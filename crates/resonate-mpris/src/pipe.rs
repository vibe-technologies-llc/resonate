use std::{
    io::{self, Read, Write},
    os::windows::io::AsHandle as _,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use interprocess::{
    ConnectWaitMode,
    os::windows::{
        ToWtf16,
        named_pipe::{DuplexPipeStream, PipeListener, PipeListenerOptions, PipeMode, pipe_mode},
        security_descriptor::SecurityDescriptor,
    },
};
use resonate_core::ZoneId;
use zbus::names::WellKnownName;

use crate::{
    Error, PipeOp, Result,
    answering::Answering,
    listening::{Serves, Sever, accepting},
    peer::{Peer, PipePeer, Severable},
    registry::{self, Claim},
    remote::ANSWERS_WITHIN,
    service::{INSTANCES, bus_name_of, instance_name},
};

const PIPE_FOLDER: &str = r"\\.\pipe\";
const SYSTEM_AND_OWNER_ONLY: &str = "D:P(A;;GA;;;SY)(A;;GA;;;OW)";
const WAKE_WITHIN: Duration = Duration::from_millis(500);
const ACCEPT_THREAD: &str = "resonate-mpris-pipe";

type Stream = DuplexPipeStream<pipe_mode::Bytes>;
type Listener = PipeListener<pipe_mode::Bytes, pipe_mode::Bytes>;

struct Shared(Arc<Stream>);

impl Read for Shared {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        (&*self.0).read(buffer)
    }
}

impl Write for Shared {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        (&*self.0).write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

#[derive(Default)]
struct Traffic {
    severed: AtomicBool,
    moving: AtomicBool,
}

struct Client {
    stream: Arc<Stream>,
    traffic: Arc<Traffic>,
}

impl Client {
    fn over(stream: Stream) -> Self {
        Self {
            stream: Arc::new(stream),
            traffic: Arc::default(),
        }
    }

    fn carried<T>(&self, io: impl FnOnce(&mut &Stream) -> io::Result<T>) -> io::Result<T> {
        self.traffic.moving.store(true, Ordering::SeqCst);

        let carried = if self.traffic.severed.load(Ordering::SeqCst) {
            Err(io::ErrorKind::BrokenPipe.into())
        } else {
            io(&mut &*self.stream)
        };

        self.traffic.moving.store(false, Ordering::SeqCst);

        carried
    }
}

impl Read for Client {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.carried(|stream| stream.read(buffer))
    }
}

impl Write for Client {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.carried(|stream| stream.write(buffer))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.carried(|stream| stream.flush())
    }
}

impl Severable for Client {
    fn severer(&self) -> io::Result<Sever> {
        let stream = Arc::clone(&self.stream);
        let traffic = Arc::clone(&self.traffic);

        Ok(Box::new(move || {
            traffic.severed.store(true, Ordering::SeqCst);
            while traffic.moving.load(Ordering::SeqCst) {
                if let Err(error) = resonate_win32::cancel_pipe_io(stream.as_handle()) {
                    tracing::debug!(%error, "a pipe's waiting read could not be cancelled");
                    break;
                }
                thread::yield_now();
            }
        }))
    }
}

fn accepted(listener: &Listener) -> io::Result<Shared> {
    listener.accept().map(|stream| Shared(Arc::new(stream)))
}

fn severed(stream: &Shared) -> Option<Sever> {
    let held = Arc::clone(&stream.0);
    Some(Box::new(move || {
        if let Err(error) = resonate_win32::disconnect_pipe(held.as_handle()) {
            tracing::debug!(%error, "a client of the pipe could not be severed");
        }
    }))
}

fn pipe_path(player: &str) -> String {
    format!("{PIPE_FOLDER}{}", registry::pipe_name_of(player))
}

fn listen(player: &str) -> io::Result<Listener> {
    let descriptor = SYSTEM_AND_OWNER_ONLY
        .to_wtf_16()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
        .and_then(|text| SecurityDescriptor::deserialize(&text))?;

    PipeListenerOptions::new()
        .path(pipe_path(player))
        .mode(PipeMode::Bytes)
        .security_descriptor(Some(descriptor))
        .create_duplex::<pipe_mode::Bytes>()
}

fn connect(player: &str) -> io::Result<Client> {
    Stream::connect_by_path_with_wait_mode(
        pipe_path(player),
        ConnectWaitMode::Timeout(ANSWERS_WITHIN),
    )
    .map(Client::over)
}

pub(crate) fn serving() -> Result<Vec<(WellKnownName<'static>, Arc<dyn Peer>)>> {
    let names = registry::listed(&registry::players_folder())
        .map_err(|source| Error::pipe(PipeOp::List, source))?;

    let mut found = Vec::new();

    for player in names {
        let Ok(name) = WellKnownName::try_from(player.clone()) else {
            continue;
        };
        let reached = connect(&player)
            .map_err(|source| Error::pipe(PipeOp::Connect, source))
            .and_then(PipePeer::over);
        match reached {
            Ok(peer) => {
                let peer: Arc<dyn Peer> = Arc::new(peer);

                found.push((name, peer));
            }
            Err(error) => {
                tracing::debug!(%error, %player, "a player named in the registry did not answer on its pipe");
            }
        }
    }
    Ok(found)
}

pub(crate) struct Serving {
    name: WellKnownName<'static>,
    player: String,
    stop: Arc<AtomicBool>,
    answering: Arc<dyn Serves>,
    thread: Option<JoinHandle<()>>,
    claim: Option<Claim>,
}

impl Serving {
    pub(crate) fn start(zone: ZoneId, answering: Arc<Answering>) -> Result<Self> {
        let answering: Arc<dyn Serves> = answering;
        let folder = registry::players_folder();
        let mut last = None;

        let names = std::iter::once(bus_name_of(zone))
            .chain((1..=INSTANCES).map(|attempt| instance_name(zone, attempt)));

        for player in names {
            let name = WellKnownName::try_from(player.clone())
                .map_err(|source| Error::bus(crate::BusOp::ClaimName, source))?;
            match Self::taking(&folder, &player)? {
                Some((claim, listener)) => {
                    return Self::begun(name, player, claim, listener, answering);
                }
                None => last = Some(name),
            }
        }

        match last {
            Some(name) => Err(Error::NameTaken { name: name.into() }),
            None => Err(Error::ThreadStopped),
        }
    }

    fn taking(folder: &Path, player: &str) -> Result<Option<(Claim, Listener)>> {
        let listener = match listen(player) {
            Ok(listener) => listener,
            Err(error) => {
                tracing::warn!(%error, %player, "the pipe is already taken; trying the next name");
                return Ok(None);
            }
        };

        match Claim::make(folder, player) {
            Ok(claim) => Ok(Some((claim, listener))),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                tracing::debug!(%player, "another player already holds this name");
                Ok(None)
            }
            Err(source) => Err(Error::pipe(PipeOp::Claim, source)),
        }
    }

    fn begun(
        name: WellKnownName<'static>,
        player: String,
        claim: Claim,
        listener: Listener,
        answering: Arc<dyn Serves>,
    ) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = Arc::clone(&stop);
            let answering = Arc::clone(&answering);
            thread::Builder::new()
                .name(ACCEPT_THREAD.to_owned())
                .spawn(move || accepting(|| accepted(&listener), severed, &answering, &stop))
                .map_err(|source| Error::pipe(PipeOp::Spawn, source))?
        };

        tracing::info!(%name, "the player is served on a pipe");
        Ok(Self {
            name,
            player,
            stop,
            answering,
            thread: Some(thread),
            claim: Some(claim),
        })
    }

    pub(crate) fn name(&self) -> WellKnownName<'static> {
        self.name.clone()
    }

    pub(crate) fn rest(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.answering.close();
        if let Some(thread) = self.thread.take() {
            let _ = Stream::connect_by_path_with_wait_mode(
                pipe_path(&self.player),
                ConnectWaitMode::Timeout(WAKE_WITHIN),
            );
            let _ = thread.join();
        }
        drop(self.claim.take());
    }
}

impl Drop for Serving {
    fn drop(&mut self) {
        self.rest();
    }
}

#[cfg(test)]
mod tests {
    use std::{process, sync::mpsc};

    use super::*;
    use crate::wire::{Call, read_frame, write_frame};

    struct Echo;

    impl Serves for Echo {
        fn reply_to(&self, frame: &[u8]) -> Option<Vec<u8>> {
            Some(frame.to_vec())
        }

        fn close(&self) {}
    }

    #[test]
    fn a_client_still_connected_when_the_pipe_stops_serving_is_severed() {
        let player = format!("org.mpris.MediaPlayer2.resonate.severed{}", process::id());
        let listener = listen(&player).expect("a pipe");
        let stop = Arc::new(AtomicBool::new(false));
        let serving: Arc<dyn Serves> = Arc::new(Echo);

        let accepting_thread = {
            let stop = Arc::clone(&stop);
            thread::spawn(move || accepting(|| accepted(&listener), severed, &serving, &stop))
        };
        let mut client = connect(&player).expect("connected");

        write_frame(&mut client, &[1]).expect("a frame sent");

        let echoed = read_frame(&mut client).expect("a reply");

        assert_eq!(echoed, Some(vec![1]));

        stop.store(true, Ordering::Release);

        let _ = Stream::connect_by_path_with_wait_mode(
            pipe_path(&player),
            ConnectWaitMode::Timeout(WAKE_WITHIN),
        );

        accepting_thread.join().expect("the accept loop ends");

        let (heard, ending) = mpsc::channel();

        thread::spawn(move || {
            let _ = heard.send(read_frame(&mut client).map(|frame| frame.is_none()));
        });

        let ended = ending.recv_timeout(Duration::from_secs(5));

        assert!(
            matches!(ended, Ok(Ok(true) | Err(_))),
            "the client's connection was left open"
        );
    }

    #[test]
    fn a_call_the_player_reads_but_never_answers_is_unconfirmed_and_lets_the_pipe_go() {
        let player = format!("org.mpris.MediaPlayer2.resonate.silent{}", process::id());
        let listener = listen(&player).expect("a pipe");
        let (heard, gone) = mpsc::channel();

        thread::spawn(move || {
            let mut held = accepted(&listener).expect("a client");

            let asked = read_frame(&mut held).is_ok_and(|frame| frame.is_some());
            let after = read_frame(&mut held);

            let _ = heard.send((asked, !matches!(after, Ok(Some(_)))));
        });

        let peer = PipePeer::over(connect(&player).expect("connected")).expect("a peer");

        let asked = peer.zone();
        let ended = gone.recv_timeout(ANSWERS_WITHIN * 2);

        assert!(
            matches!(asked, Err(Error::Unconfirmed { call: Call::Zone })),
            "{asked:?}"
        );
        assert_eq!(
            ended.ok(),
            Some((true, true)),
            "the link's thread still holds the pipe"
        );
    }

    #[test]
    fn a_pipe_lies_in_the_pipe_folder_under_the_users_name() {
        assert!(pipe_path("org.mpris.MediaPlayer2.resonate").starts_with(r"\\.\pipe\resonate-"));
    }
}
