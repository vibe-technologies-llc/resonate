use std::{
    io,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
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
    peer::{Peer, PipePeer},
    registry::{self, Claim},
    remote::ANSWERS_WITHIN,
    service::{INSTANCES, bus_name_of, instance_name},
};

const PIPE_FOLDER: &str = r"\\.\pipe\";
const SYSTEM_AND_OWNER_ONLY: &str = "D:P(A;;GA;;;SY)(A;;GA;;;OW)";
const CONNECTIONS_AT_MOST: usize = 32;
const ACCEPT_FAILURES_AT_MOST: u32 = 5;
const ACCEPT_RETRY_AFTER: Duration = Duration::from_millis(100);
const WAKE_WITHIN: Duration = Duration::from_millis(500);
const ACCEPT_THREAD: &str = "resonate-mpris-pipe";
const CONNECTION_THREAD: &str = "resonate-mpris-pipe-connection";

type Stream = DuplexPipeStream<pipe_mode::Bytes>;
type Listener = PipeListener<pipe_mode::Bytes, pipe_mode::Bytes>;

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

fn connect(player: &str) -> io::Result<Stream> {
    Stream::connect_by_path_with_wait_mode(
        pipe_path(player),
        ConnectWaitMode::Timeout(ANSWERS_WITHIN),
    )
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
    answering: Arc<Answering>,
    thread: Option<JoinHandle<()>>,
    claim: Option<Claim>,
}

impl Serving {
    pub(crate) fn start(zone: ZoneId, answering: Arc<Answering>) -> Result<Self> {
        let folder = registry::players_folder();
        let mut last = None;

        let names = std::iter::once(bus_name_of(zone))
            .chain((1..=INSTANCES).map(|attempt| instance_name(zone, attempt)));
        for player in names {
            let name = WellKnownName::try_from(player.clone())
                .map_err(|source| Error::bus(crate::BusOp::ClaimName, source))?;
            match Self::taking(&folder, &player)? {
                Some((claim, listener)) => {
                    return Self::serving(name, player, claim, listener, answering);
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

    fn serving(
        name: WellKnownName<'static>,
        player: String,
        claim: Claim,
        listener: Listener,
        answering: Arc<Answering>,
    ) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = Arc::clone(&stop);
            let answering = Arc::clone(&answering);
            thread::Builder::new()
                .name(ACCEPT_THREAD.to_owned())
                .spawn(move || accepting(&listener, &answering, &stop))
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

fn accepting(listener: &Listener, answering: &Arc<Answering>, stop: &AtomicBool) {
    let open = Arc::new(AtomicUsize::new(0));
    let mut failures = 0;

    loop {
        let accepted = listener.accept();
        if stop.load(Ordering::Acquire) {
            return;
        }
        let stream = match accepted {
            Ok(stream) => {
                failures = 0;
                stream
            }
            Err(error) => {
                failures += 1;
                tracing::debug!(%error, "a client of the pipe was not accepted");
                if failures >= ACCEPT_FAILURES_AT_MOST {
                    tracing::warn!("the pipe has stopped accepting clients");
                    return;
                }
                thread::sleep(ACCEPT_RETRY_AFTER);
                continue;
            }
        };

        if open.fetch_add(1, Ordering::AcqRel) >= CONNECTIONS_AT_MOST {
            open.fetch_sub(1, Ordering::AcqRel);
            tracing::debug!("a client of the pipe was turned away: too many are connected");
            continue;
        }

        let answering = Arc::clone(answering);
        let counted = Arc::clone(&open);
        let spawned = thread::Builder::new()
            .name(CONNECTION_THREAD.to_owned())
            .spawn(move || {
                if let Err(error) = answering.serve(stream) {
                    tracing::debug!(%error, "a client of the pipe was let go");
                }
                counted.fetch_sub(1, Ordering::AcqRel);
            });
        if spawned.is_err() {
            open.fetch_sub(1, Ordering::AcqRel);
            tracing::warn!("a client of the pipe could not be given a thread");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pipe_lies_in_the_pipe_folder_under_the_users_name() {
        assert!(pipe_path("org.mpris.MediaPlayer2.resonate").starts_with(r"\\.\pipe\resonate-"));
    }
}
