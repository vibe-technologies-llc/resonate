use std::{
    io::{self, Write},
    process,
    time::{Duration, Instant},
};

use resonate_core::AppId;
use serde::{Deserialize, Serialize};

use crate::{
    Error, IpcOp, JsonOp, Result,
    activity::Activity,
    frame::{Frame, Opcode, split_frame, write_frame},
};

const PROTOCOL: u8 = 1;
pub(crate) const ANSWERS_WITHIN: Duration = Duration::from_secs(2);
const READY_WITHIN: Duration = Duration::from_secs(5);
const SET_ACTIVITY: &str = "SET_ACTIVITY";
const DISPATCH: &str = "DISPATCH";
const READY: &str = "READY";
const ERROR: &str = "ERROR";

pub(crate) trait Wire: Write {
    fn receive(&mut self, within: Duration, into: &mut Vec<u8>) -> Result<()>;

    fn poll(&mut self, into: &mut Vec<u8>) -> Result<()>;
}

#[derive(Serialize)]
struct Handshake {
    v: u8,
    client_id: String,
}

#[derive(Serialize)]
struct Command<'a> {
    cmd: &'static str,
    args: Arguments<'a>,
    nonce: String,
}

#[derive(Serialize)]
struct Arguments<'a> {
    pid: u32,
    activity: Option<&'a Activity>,
}

#[derive(Deserialize)]
struct ReplyDoc {
    #[serde(default)]
    cmd: Option<String>,
    #[serde(default)]
    evt: Option<String>,
    #[serde(default)]
    data: Option<FaultDoc>,
}

#[derive(Deserialize)]
struct FaultDoc {
    #[serde(default)]
    code: Option<i64>,
}

#[derive(Deserialize)]
struct CloseDoc {
    #[serde(default)]
    code: i64,
}

pub(crate) struct Session<W: Wire> {
    wire: W,
    nonce: u64,
    received: Vec<u8>,
}

impl<W: Wire> Session<W> {
    pub(crate) fn begin(wire: W, app: AppId) -> Result<Self> {
        let mut session = Self {
            wire,
            nonce: 0,
            received: Vec::new(),
        };
        let handshake = Handshake {
            v: PROTOCOL,
            client_id: app.to_string(),
        };
        session.send(Opcode::Handshake, &encoded(&handshake)?)?;
        session.await_ready()?;
        Ok(session)
    }

    pub(crate) fn set(&mut self, activity: Option<&Activity>) -> Result<()> {
        self.nonce = self.nonce.wrapping_add(1);
        let command = Command {
            cmd: SET_ACTIVITY,
            args: Arguments {
                pid: process::id(),
                activity,
            },
            nonce: self.nonce.to_string(),
        };
        self.send(Opcode::Frame, &encoded(&command)?)
    }

    pub(crate) fn drain(&mut self) -> Result<()> {
        while let Some(frame) = self.waiting()? {
            self.answer(frame)?;
        }
        Ok(())
    }

    fn await_ready(&mut self) -> Result<()> {
        let deadline = Instant::now() + READY_WITHIN;
        while let Some(frame) = self.next_frame_by(deadline)? {
            if frame.opcode == Opcode::Frame {
                let reply: ReplyDoc = decoded(&frame.body)?;
                match (reply.cmd.as_deref(), reply.evt.as_deref()) {
                    (Some(DISPATCH), Some(READY)) => return Ok(()),
                    (_, Some(ERROR)) => {
                        return Err(Error::Refused {
                            code: reply.data.and_then(|fault| fault.code).unwrap_or_default(),
                        });
                    }
                    _ => {}
                }
            } else {
                self.answer(frame)?;
            }
        }
        Err(Error::Socket {
            op: IpcOp::Read,
            source: io::ErrorKind::TimedOut.into(),
        })
    }

    fn next_frame_by(&mut self, deadline: Instant) -> Result<Option<Frame>> {
        loop {
            if let Some(frame) = split_frame(&mut self.received)? {
                return Ok(Some(frame));
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            self.wire
                .receive(left.min(ANSWERS_WITHIN), &mut self.received)?;
        }
    }

    fn answer(&mut self, frame: Frame) -> Result<()> {
        match frame.opcode {
            Opcode::Ping => self.send(Opcode::Pong, &frame.body),
            Opcode::Close => {
                let close: CloseDoc = decoded(&frame.body)?;
                Err(Error::Closed { code: close.code })
            }
            Opcode::Frame => {
                let reply: ReplyDoc = decoded(&frame.body)?;
                match (reply.evt.as_deref(), reply.data) {
                    (Some(ERROR), Some(FaultDoc { code: Some(code) })) => {
                        Err(Error::Refused { code })
                    }
                    _ => Ok(()),
                }
            }
            Opcode::Handshake | Opcode::Pong => Ok(()),
        }
    }

    fn waiting(&mut self) -> Result<Option<Frame>> {
        if let Some(frame) = split_frame(&mut self.received)? {
            return Ok(Some(frame));
        }

        self.wire.poll(&mut self.received)?;

        split_frame(&mut self.received)
    }

    fn send(&mut self, opcode: Opcode, body: &[u8]) -> Result<()> {
        write_frame(&mut self.wire, opcode, body)
    }
}

fn encoded(value: &impl Serialize) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|source| Error::Json {
        op: JsonOp::Encode,
        source,
    })
}

fn decoded<'a, T: Deserialize<'a>>(body: &'a [u8]) -> Result<T> {
    serde_json::from_slice(body).map_err(|source| Error::Json {
        op: JsonOp::Decode,
        source,
    })
}
