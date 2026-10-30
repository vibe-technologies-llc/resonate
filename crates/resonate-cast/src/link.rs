use std::{
    collections::BTreeMap,
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{
    Receiver, RecvTimeoutError, Sender, TryRecvError, TrySendError, bounded, unbounded,
};
use parking_lot::Mutex;
use rustls::{ClientConnection, StreamOwned, pki_types::ServerName};

use crate::{
    Asked, Error, LinkOp, Refusal, Result,
    frame::{self, LENGTH_BYTES, Message, Payload},
    json::Json,
    tls,
};

pub(crate) const RECEIVER: &str = "receiver-0";

const SENDER: &str = "sender-0";

pub(crate) const USER_AGENT: &str = concat!("resonate/", env!("CARGO_PKG_VERSION"));

const CONNECTED_WITHIN: Duration = Duration::from_secs(3);

pub(crate) const ANSWERED_WITHIN: Duration = Duration::from_secs(5);

const READS_WAKE_EVERY: Duration = Duration::from_millis(20);

const PINGED_EVERY: Duration = Duration::from_secs(5);

const SILENT_AT_MOST: Duration = Duration::from_secs(16);

const TOLD_AT_MOST: usize = 64;

const READ_AT_ONCE: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Namespace {
    Connection,
    Heartbeat,
    Receiver,
    Media,
}

impl Namespace {
    const ALL: [Self; 4] = [
        Self::Connection,
        Self::Heartbeat,
        Self::Receiver,
        Self::Media,
    ];

    pub(crate) const fn urn(self) -> &'static str {
        match self {
            Self::Connection => "urn:x-cast:com.google.cast.tp.connection",
            Self::Heartbeat => "urn:x-cast:com.google.cast.tp.heartbeat",
            Self::Receiver => "urn:x-cast:com.google.cast.receiver",
            Self::Media => "urn:x-cast:com.google.cast.media",
        }
    }

    fn named(urn: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|namespace| namespace.urn() == urn)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Told {
    pub(crate) namespace: Namespace,
    pub(crate) from: String,
    pub(crate) said: Json,
}

type Waiting = Arc<Mutex<BTreeMap<u32, Sender<Json>>>>;

pub(crate) struct Link {
    at: SocketAddr,
    outgoing: Sender<Message>,
    waiting: Waiting,
    minted: AtomicU32,
    told: Receiver<Told>,
    alive: Arc<AtomicBool>,
}

type Tls = StreamOwned<ClientConnection, TcpStream>;

impl Link {
    pub(crate) fn connect(at: SocketAddr) -> Result<Self> {
        let failed = |op| move |source| Error::Link { at, op, source };

        let socket =
            TcpStream::connect_timeout(&at, CONNECTED_WITHIN).map_err(failed(LinkOp::Connect))?;
        socket.set_nodelay(true).map_err(failed(LinkOp::Connect))?;
        socket
            .set_read_timeout(Some(ANSWERED_WITHIN))
            .map_err(failed(LinkOp::Connect))?;
        socket
            .set_write_timeout(Some(ANSWERED_WITHIN))
            .map_err(failed(LinkOp::Connect))?;
        let connection =
            ClientConnection::new(tls::config()?, ServerName::IpAddress(at.ip().into())).map_err(
                |source| Error::Tls {
                    source: Box::new(source),
                },
            )?;
        let mut tls = StreamOwned::new(connection, socket);
        while tls.conn.is_handshaking() {
            tls.conn
                .complete_io(&mut tls.sock)
                .map_err(failed(LinkOp::Handshake))?;
        }
        tls.sock
            .set_read_timeout(Some(READS_WAKE_EVERY))
            .map_err(failed(LinkOp::Connect))?;

        let (outgoing, sent) = unbounded();
        let (tell, told) = bounded(TOLD_AT_MOST);
        let waiting = Waiting::default();
        let alive = Arc::new(AtomicBool::new(true));
        let link = Self {
            at,
            outgoing,
            waiting: Arc::clone(&waiting),
            minted: AtomicU32::new(1),
            told,
            alive: Arc::clone(&alive),
        };

        let carried = Carried {
            tls,
            sent,
            waiting,
            tell,
            alive,
        };
        thread::Builder::new()
            .name("resonate-cast-link".to_owned())
            .spawn(move || carried.run())
            .map_err(failed(LinkOp::Spawn))?;

        link.tell(
            Namespace::Connection,
            RECEIVER,
            Json::object([
                ("type", Json::text("CONNECT")),
                ("origin", Json::Object(Vec::new())),
                ("userAgent", Json::text(USER_AGENT)),
            ]),
        );
        Ok(link)
    }

    pub(crate) const fn at(&self) -> SocketAddr {
        self.at
    }

    pub(crate) fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    pub(crate) const fn told(&self) -> &Receiver<Told> {
        &self.told
    }

    pub(crate) fn tell(&self, namespace: Namespace, destination: &str, said: Json) {
        let _ = self.outgoing.send(Message {
            source: SENDER.to_owned(),
            destination: destination.to_owned(),
            namespace: namespace.urn().to_owned(),
            payload: Payload::Text(said.written()),
        });
    }

    pub(crate) fn ask(
        &self,
        namespace: Namespace,
        destination: &str,
        asked: Asked,
        said: Json,
        within: Duration,
    ) -> Result<Json> {
        let id = self.minted.fetch_add(1, Ordering::Relaxed);
        let (answer, answered) = bounded(1);
        self.waiting.lock().insert(id, answer);
        if !self.is_alive() {
            self.waiting.lock().remove(&id);
            return Err(Error::Closed { asked });
        }

        self.tell(
            namespace,
            destination,
            said.with("type", Json::text(asked.name()))
                .with("requestId", id.into()),
        );
        let reply = match answered.recv_timeout(within) {
            Ok(reply) => reply,
            Err(RecvTimeoutError::Timeout) => {
                self.waiting.lock().remove(&id);
                return Err(Error::Unanswered { asked });
            }
            Err(RecvTimeoutError::Disconnected) => return Err(Error::Closed { asked }),
        };
        match reply.kind().and_then(Refusal::named) {
            Some(refusal) => Err(Error::Refused { asked, refusal }),
            None => Ok(reply),
        }
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.tell(
            Namespace::Connection,
            RECEIVER,
            Json::object([("type", Json::text("CLOSE"))]),
        );
    }
}

struct Carried {
    tls: Tls,
    sent: Receiver<Message>,
    waiting: Waiting,
    tell: Sender<Told>,
    alive: Arc<AtomicBool>,
}

enum Carrying {
    On,
    Ended,
}

impl Carried {
    fn run(mut self) {
        let mut held = Vec::new();
        let mut buffer = vec![0; READ_AT_ONCE];
        let mut pinged_at = Instant::now();
        let mut heard_at = Instant::now();

        loop {
            match self.send_what_is_waiting() {
                Ok(Carrying::On) => {}
                Ok(Carrying::Ended) => {
                    self.tls.conn.send_close_notify();
                    let _ = self.tls.flush();
                    break;
                }
                Err(error) => {
                    tracing::debug!(%error, "a Cast receiver could not be written to");
                    break;
                }
            }
            if pinged_at.elapsed() >= PINGED_EVERY {
                pinged_at = Instant::now();
                if let Err(error) = self.write(&heartbeat("PING")) {
                    tracing::debug!(%error, "a Cast receiver could not be pinged");
                    break;
                }
            }
            if heard_at.elapsed() > SILENT_AT_MOST {
                tracing::debug!("a Cast receiver fell silent");
                break;
            }

            match self.tls.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    heard_at = Instant::now();
                    held.extend_from_slice(buffer.get(..read).unwrap_or_default());
                    if !self.take_whole(&mut held) {
                        break;
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                            | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => {
                    tracing::debug!(%error, "a Cast receiver could not be read");
                    break;
                }
            }
        }

        self.alive.store(false, Ordering::Release);
        self.waiting.lock().clear();
    }

    fn send_what_is_waiting(&mut self) -> io::Result<Carrying> {
        loop {
            match self.sent.try_recv() {
                Ok(message) => self.write(&message)?,
                Err(TryRecvError::Empty) => return Ok(Carrying::On),
                Err(TryRecvError::Disconnected) => return Ok(Carrying::Ended),
            }
        }
    }

    fn write(&mut self, message: &Message) -> io::Result<()> {
        self.tls.write_all(&message.framed())?;
        self.tls.flush()
    }

    fn take_whole(&mut self, held: &mut Vec<u8>) -> bool {
        loop {
            let length = match frame::whole(held) {
                Ok(Some(length)) => length,
                Ok(None) => return true,
                Err(error) => {
                    tracing::debug!(%error, "a Cast receiver's message could not be framed");
                    return false;
                }
            };
            let end = LENGTH_BYTES + length;
            let read = Message::read(held.get(LENGTH_BYTES..end).unwrap_or_default());
            held.drain(..end);
            match read {
                Ok(message) => {
                    if !self.take(message) {
                        return false;
                    }
                }
                Err(error) => tracing::debug!(%error, "a Cast receiver's message was skipped"),
            }
        }
    }

    fn take(&mut self, message: Message) -> bool {
        let Some(namespace) = Namespace::named(&message.namespace) else {
            return true;
        };
        let Payload::Text(text) = message.payload else {
            return true;
        };
        let Some(said) = Json::read(&text) else {
            tracing::debug!(namespace = %message.namespace, "a Cast receiver said something that is not JSON");
            return true;
        };

        match (namespace, said.kind()) {
            (Namespace::Heartbeat, Some("PING")) => {
                return match self.write(&heartbeat("PONG")) {
                    Ok(()) => true,
                    Err(error) => {
                        tracing::debug!(%error, "a Cast receiver's ping could not be answered");
                        false
                    }
                };
            }
            (Namespace::Heartbeat, _) => return true,
            (Namespace::Connection, Some("CLOSE")) if message.source == RECEIVER => return false,
            _ => {}
        }

        let request = said
            .get("requestId")
            .and_then(Json::as_number)
            .filter(|id| *id >= 1.0 && *id <= f64::from(u32::MAX))
            .map(|id| id as u32);
        if let Some(waiter) = request.and_then(|id| self.waiting.lock().remove(&id)) {
            let _ = waiter.send(said);
            return true;
        }
        match self.tell.try_send(Told {
            namespace,
            from: message.source,
            said,
        }) {
            Ok(()) | Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => true,
        }
    }
}

fn heartbeat(kind: &str) -> Message {
    Message {
        source: SENDER.to_owned(),
        destination: RECEIVER.to_owned(),
        namespace: Namespace::Heartbeat.urn().to_owned(),
        payload: Payload::Text(Json::object([("type", Json::text(kind))]).written()),
    }
}
